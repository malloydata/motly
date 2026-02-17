use crate::tree::*;
use regex::Regex;
use std::collections::BTreeMap;

// ── Error types ─────────────────────────────────────────────────────

/// An error found during reference validation.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationError {
    pub message: String,
    /// Path in the tree where the error was found (e.g. ["outer", "inner", "ref"]).
    pub path: Vec<String>,
    /// Machine-readable error code.
    pub code: &'static str,
}

/// An error found during schema validation.
#[derive(Debug, Clone, PartialEq)]
pub struct SchemaError {
    pub message: String,
    /// Path in the tree where the error was found.
    pub path: Vec<String>,
    /// Machine-readable error code.
    pub code: &'static str,
}

// ── Reference validation ────────────────────────────────────────────

/// Validate that every `MOTLYRef` in the tree resolves to an existing node.
///
/// Returns an empty vec when all references are valid.
pub fn validate_references(root: &MOTLYValue) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    let mut path: Vec<String> = Vec::new();
    let mut ancestors: Vec<&MOTLYValue> = vec![root];
    walk_refs(root, &mut path, &mut ancestors, root, &mut errors);
    errors
}

/// Recursive walk collecting reference errors.
/// `ancestors` is the stack of nodes from the root down to (but not including) `node`.
/// The last element is always the parent of `node`'s properties.
fn walk_refs<'a>(
    node: &'a MOTLYValue,
    path: &mut Vec<String>,
    ancestors: &mut Vec<&'a MOTLYValue>,
    root: &'a MOTLYValue,
    errors: &mut Vec<ValidationError>,
) {
    // Check array elements in eq
    if let Some(EqValue::Array(arr)) = &node.eq {
        walk_array_refs(arr, path, ancestors, node, root, errors);
    }

    // Check properties
    if let Some(props) = &node.properties {
        for (key, value) in props {
            path.push(key.clone());
            match value {
                MOTLYNode::Ref(link) => {
                    if let Some(err_msg) = check_link(link, ancestors, root) {
                        errors.push(ValidationError {
                            message: err_msg,
                            path: path.clone(),
                            code: "unresolved-reference",
                        });
                    }
                }
                MOTLYNode::Value(child) => {
                    ancestors.push(node);
                    walk_refs(child, path, ancestors, root, errors);
                    ancestors.pop();
                }
            }
            path.pop();
        }
    }
}

/// Walk array elements looking for links.
fn walk_array_refs<'a>(
    arr: &'a [MOTLYNode],
    path: &mut Vec<String>,
    ancestors: &mut Vec<&'a MOTLYValue>,
    parent_node: &'a MOTLYValue,
    root: &'a MOTLYValue,
    errors: &mut Vec<ValidationError>,
) {
    for (i, elem) in arr.iter().enumerate() {
        let idx_key = format!("[{}]", i);
        path.push(idx_key);
        match elem {
            MOTLYNode::Ref(link) => {
                if let Some(err_msg) = check_link(link, ancestors, root) {
                    errors.push(ValidationError {
                        message: err_msg,
                        path: path.clone(),
                        code: "unresolved-reference",
                    });
                }
            }
            MOTLYNode::Value(child) => {
                // Array element nodes can themselves contain refs
                ancestors.push(parent_node);
                walk_refs(child, path, ancestors, root, errors);
                ancestors.pop();
            }
        }
        path.pop();
    }
}

/// Check whether a link resolves. Returns `Some(error_message)` on failure.
fn check_link(link: &MOTLYRef, ancestors: &[&MOTLYValue], root: &MOTLYValue) -> Option<String> {
    let (ups, segments) = parse_link_string(&link.link_to);

    // Determine the start node for resolution.
    let start = if ups == 0 {
        // Absolute: start from root
        root
    } else {
        // Relative: go up `ups` levels in the ancestor stack.
        //
        // The ancestors stack is seeded with [root] and each recursion into
        // a child's properties pushes the current (parent) node. So for a
        // link at outer.inner.ref the stack is [root, root, outer_node]:
        //   - root pushed at init
        //   - root pushed again when entering outer (root's child)
        //   - outer_node pushed when entering inner (outer's child)
        //
        // $^name (ups=1) → ancestors[len-1] = outer_node
        //   → looks in outer's properties (siblings of inner)
        // $^^name (ups=2) → ancestors[len-2] = root
        //   → looks in root's properties (siblings of outer)
        let idx = ancestors.len().checked_sub(ups);
        match idx {
            Some(i) if i < ancestors.len() => ancestors[i],
            _ => {
                return Some(format!(
                    "Reference \"{}\" goes {} level(s) up but only {} ancestor(s) available",
                    link.link_to,
                    ups,
                    ancestors.len()
                ));
            }
        }
    };

    // Follow the path segments from the start node.
    resolve_path(start, &segments, &link.link_to)
}

/// Parsed reference segment for resolution.
enum RefSeg {
    Name(String),
    Index(usize),
}

/// Parse a link_to string like "$^^items[0].name" into (ups, segments).
fn parse_link_string(s: &str) -> (usize, Vec<RefSeg>) {
    let mut chars = s.chars().peekable();

    // Skip leading '$'
    if chars.peek() == Some(&'$') {
        chars.next();
    }

    // Count '^' characters
    let mut ups = 0;
    while chars.peek() == Some(&'^') {
        ups += 1;
        chars.next();
    }

    let mut segments = Vec::new();
    let mut name_buf = String::new();

    while let Some(&ch) = chars.peek() {
        match ch {
            '.' => {
                if !name_buf.is_empty() {
                    segments.push(RefSeg::Name(name_buf.clone()));
                    name_buf.clear();
                }
                chars.next();
            }
            '[' => {
                if !name_buf.is_empty() {
                    segments.push(RefSeg::Name(name_buf.clone()));
                    name_buf.clear();
                }
                chars.next(); // consume '['
                let mut idx_buf = String::new();
                while let Some(&c) = chars.peek() {
                    if c == ']' {
                        chars.next();
                        break;
                    }
                    idx_buf.push(c);
                    chars.next();
                }
                if let Ok(idx) = idx_buf.parse::<usize>() {
                    segments.push(RefSeg::Index(idx));
                }
            }
            _ => {
                name_buf.push(ch);
                chars.next();
            }
        }
    }
    if !name_buf.is_empty() {
        segments.push(RefSeg::Name(name_buf));
    }

    (ups, segments)
}

/// Follow path segments from a start node. Returns Some(error) if unresolved.
fn resolve_path(start: &MOTLYValue, segments: &[RefSeg], link_str: &str) -> Option<String> {
    let mut current: ResolveTarget = ResolveTarget::Node(start);

    for seg in segments {
        match (seg, current) {
            (RefSeg::Name(name), ResolveTarget::Node(node)) => {
                match &node.properties {
                    Some(props) => match props.get(name.as_str()) {
                        Some(MOTLYNode::Value(child)) => {
                            current = ResolveTarget::Node(child);
                        }
                        Some(MOTLYNode::Ref(_)) => {
                            // Resolving through a link - the target exists as a link node.
                            // For validation purposes, the link itself exists, so the
                            // reference up to this point is valid. But we can't follow
                            // through a link to further sub-paths.
                            // Treat link as a terminal value.
                            current = ResolveTarget::Terminal;
                        }
                        None => {
                            return Some(format!(
                                "Reference \"{}\" could not be resolved: property \"{}\" not found",
                                link_str, name
                            ));
                        }
                    },
                    None => {
                        return Some(format!(
                            "Reference \"{}\" could not be resolved: property \"{}\" not found (node has no properties)",
                            link_str, name
                        ));
                    }
                }
            }
            (RefSeg::Index(idx), ResolveTarget::Node(node)) => match &node.eq {
                Some(EqValue::Array(arr)) => {
                    if *idx >= arr.len() {
                        return Some(format!(
                                "Reference \"{}\" could not be resolved: index [{}] out of bounds (array length {})",
                                link_str, idx, arr.len()
                            ));
                    }
                    match &arr[*idx] {
                        MOTLYNode::Value(child) => {
                            current = ResolveTarget::Node(child);
                        }
                        MOTLYNode::Ref(_) => {
                            current = ResolveTarget::Terminal;
                        }
                    }
                }
                _ => {
                    return Some(format!(
                        "Reference \"{}\" could not be resolved: index [{}] used on non-array",
                        link_str, idx
                    ));
                }
            },
            (_, ResolveTarget::Terminal) => {
                // Trying to navigate further through a terminal (link).
                // We can't resolve through links, so just consider the remaining path unresolvable.
                return Some(format!(
                    "Reference \"{}\" could not be resolved: cannot follow path through a link",
                    link_str
                ));
            }
        }
    }

    None // resolved successfully
}

enum ResolveTarget<'a> {
    Node(&'a MOTLYValue),
    Terminal,
}

// ── Schema validation ───────────────────────────────────────────────

/// Validate a MOTLY tree against a schema (also a MOTLY tree).
///
/// The schema should have sections like `Required`, `Optional`, `Types`, and `Additional`.
/// Returns an empty vec when the tree conforms to the schema.
pub fn validate_schema(tag: &MOTLYValue, schema: &MOTLYValue) -> Vec<SchemaError> {
    let mut errors = Vec::new();
    let types = extract_section(schema, "Types");
    let path: Vec<String> = Vec::new();
    validate_node_against_schema(tag, schema, &types, &path, &mut errors);
    errors
}

/// Extract a section from a schema node by property name.
fn extract_section<'a>(
    node: &'a MOTLYValue,
    name: &str,
) -> Option<&'a BTreeMap<String, MOTLYNode>> {
    node.properties.as_ref()?.get(name).and_then(|v| match v {
        MOTLYNode::Value(n) => n.properties.as_ref(),
        _ => None,
    })
}

/// Get the `eq` string value of a node.
fn get_eq_string(node: &MOTLYValue) -> Option<&str> {
    match &node.eq {
        Some(EqValue::Scalar(Scalar::String(s))) => Some(s.as_str()),
        _ => None,
    }
}

/// Get the `eq` value of a MOTLYNode if it's a Node with a scalar string eq.
fn value_eq_string(value: &MOTLYNode) -> Option<&str> {
    match value {
        MOTLYNode::Value(n) => get_eq_string(n),
        _ => None,
    }
}

/// Validate a node against a schema node (which has Required/Optional/Additional sections).
fn validate_node_against_schema(
    tag: &MOTLYValue,
    schema: &MOTLYValue,
    types: &Option<&BTreeMap<String, MOTLYNode>>,
    path: &[String],
    errors: &mut Vec<SchemaError>,
) {
    let required = extract_section(schema, "Required");
    let optional = extract_section(schema, "Optional");
    let additional = get_additional_policy(schema);

    let tag_props = tag.properties.as_ref();

    // Check required properties
    if let Some(req) = required {
        for (key, type_spec) in req {
            let mut prop_path = path.to_vec();
            prop_path.push(key.clone());

            let tag_value = tag_props.and_then(|p| p.get(key.as_str()));
            match tag_value {
                None => {
                    errors.push(SchemaError {
                        message: format!("Missing required property \"{}\"", key),
                        path: prop_path,
                        code: "missing-required",
                    });
                }
                Some(value) => {
                    validate_value_type(value, type_spec, types, &prop_path, errors);
                }
            }
        }
    }

    // Check optional properties that exist
    if let Some(opt) = optional {
        if let Some(tag_p) = tag_props {
            for (key, type_spec) in opt {
                if let Some(value) = tag_p.get(key.as_str()) {
                    let mut prop_path = path.to_vec();
                    prop_path.push(key.clone());
                    validate_value_type(value, type_spec, types, &prop_path, errors);
                }
            }
        }
    }

    // Check for unknown properties
    if let Some(tag_p) = tag_props {
        let known_keys = collect_known_keys(required, optional);
        // Also skip schema-internal keys
        for key in tag_p.keys() {
            if known_keys.contains(&key.as_str()) {
                continue;
            }
            let mut prop_path = path.to_vec();
            prop_path.push(key.clone());
            match additional {
                AdditionalPolicy::Reject => {
                    errors.push(SchemaError {
                        message: format!("Unknown property \"{}\"", key),
                        path: prop_path,
                        code: "unknown-property",
                    });
                }
                AdditionalPolicy::Allow => {}
                AdditionalPolicy::ValidateAs(ref type_name) => {
                    if let Some(value) = tag_p.get(key.as_str()) {
                        let synthetic = make_type_spec_node(type_name);
                        validate_value_type(
                            value,
                            &MOTLYNode::Value(synthetic),
                            types,
                            &prop_path,
                            errors,
                        );
                    }
                }
            }
        }
    }
}

/// Collect known keys from Required + Optional sections.
fn collect_known_keys<'a>(
    required: Option<&'a BTreeMap<String, MOTLYNode>>,
    optional: Option<&'a BTreeMap<String, MOTLYNode>>,
) -> Vec<&'a str> {
    let mut keys = Vec::new();
    if let Some(req) = required {
        for k in req.keys() {
            keys.push(k.as_str());
        }
    }
    if let Some(opt) = optional {
        for k in opt.keys() {
            keys.push(k.as_str());
        }
    }
    keys
}

#[derive(Clone)]
enum AdditionalPolicy {
    Reject,
    Allow,
    ValidateAs(String),
}

/// Read the `Additional` property from a schema node.
fn get_additional_policy(schema: &MOTLYValue) -> AdditionalPolicy {
    let props = match &schema.properties {
        Some(p) => p,
        None => return AdditionalPolicy::Reject,
    };
    let additional = match props.get("Additional") {
        Some(v) => v,
        None => return AdditionalPolicy::Reject,
    };
    match additional {
        MOTLYNode::Value(n) => {
            if let Some(eq_str) = get_eq_string(n) {
                match eq_str {
                    "allow" => AdditionalPolicy::Allow,
                    "reject" => AdditionalPolicy::Reject,
                    other => AdditionalPolicy::ValidateAs(other.to_string()),
                }
            } else {
                // Additional without eq: just having it means allow
                AdditionalPolicy::Allow
            }
        }
        _ => AdditionalPolicy::Reject,
    }
}

/// Create a simple type spec node with eq=type_name.
fn make_type_spec_node(type_name: &str) -> MOTLYValue {
    MOTLYValue::with_eq(EqValue::Scalar(Scalar::String(type_name.to_string())))
}

/// Validate a MOTLYNode against a type specifier from the schema.
fn validate_value_type(
    value: &MOTLYNode,
    type_spec: &MOTLYNode,
    types: &Option<&BTreeMap<String, MOTLYNode>>,
    path: &[String],
    errors: &mut Vec<SchemaError>,
) {
    let spec_node = match type_spec {
        MOTLYNode::Value(n) => n,
        MOTLYNode::Ref(_) => return, // links in schema are not type specs
    };

    // Check if this is a union type (oneOf)
    if let Some(one_of_props) = &spec_node.properties {
        if let Some(MOTLYNode::Value(one_of_node)) = one_of_props.get("oneOf") {
            validate_union(value, one_of_node, types, path, errors);
            return;
        }
    }

    // Check if this is an enum type (eq)
    if let Some(spec_props) = &spec_node.properties {
        if let Some(MOTLYNode::Value(eq_node)) = spec_props.get("eq") {
            if let Some(EqValue::Array(allowed)) = &eq_node.eq {
                validate_enum(value, allowed, path, errors);
                return;
            }
        }
        // Check for pattern matching (matches)
        if let Some(MOTLYNode::Value(matches_node)) = spec_props.get("matches") {
            if let Some(base_type) = get_eq_string(spec_node) {
                validate_base_type(value, base_type, types, path, errors);
            }
            validate_pattern(value, matches_node, path, errors);
            return;
        }
    }

    // Get the type name from the spec's eq value
    let type_name = match get_eq_string(spec_node) {
        Some(t) => t,
        None => {
            // If the spec node has no eq, it might be a nested schema (tag type with
            // Required/Optional/Additional sections).
            if spec_node.properties.as_ref().map_or(false, |p| {
                p.contains_key("Required")
                    || p.contains_key("Optional")
                    || p.contains_key("Additional")
            }) {
                // Nested schema validation
                match value {
                    MOTLYNode::Value(tag_node) => {
                        validate_node_against_schema(tag_node, spec_node, types, path, errors);
                    }
                    MOTLYNode::Ref(_) => {
                        errors.push(SchemaError {
                            message: "Expected a tag but found a link".to_string(),
                            path: path.to_vec(),
                            code: "wrong-type",
                        });
                    }
                }
            }
            return;
        }
    };

    validate_base_type(value, type_name, types, path, errors);
}

/// Validate a value against a base type name (string, number, etc. or custom type, or array type).
fn validate_base_type(
    value: &MOTLYNode,
    type_name: &str,
    types: &Option<&BTreeMap<String, MOTLYNode>>,
    path: &[String],
    errors: &mut Vec<SchemaError>,
) {
    // Check for array types: "string[]", "number[]", "tag[]", "TypeName[]"
    if let Some(inner_type) = type_name.strip_suffix("[]") {
        validate_array_type(value, inner_type, types, path, errors);
        return;
    }

    match type_name {
        "string" => validate_type_string(value, path, errors),
        "number" => validate_type_number(value, path, errors),
        "boolean" => validate_type_boolean(value, path, errors),
        "date" => validate_type_date(value, path, errors),
        "tag" => validate_type_tag(value, path, errors),
        "flag" => validate_type_flag(value, path, errors),
        "any" => validate_type_any(value, path, errors),
        custom => {
            // Look up custom type in Types section
            if let Some(types_map) = types {
                if let Some(type_def) = types_map.get(custom) {
                    // The type definition is itself a type specifier — it may
                    // be a structural schema (Required/Optional), a oneOf union,
                    // an enum, a pattern, etc. Route through validate_value_type
                    // so all those forms are handled.
                    validate_value_type(value, type_def, types, path, errors);
                } else {
                    errors.push(SchemaError {
                        message: format!("Unknown type \"{}\" in schema", custom),
                        path: path.to_vec(),
                        code: "invalid-schema",
                    });
                }
            } else {
                errors.push(SchemaError {
                    message: format!("Unknown type \"{}\" (no Types section in schema)", custom),
                    path: path.to_vec(),
                    code: "invalid-schema",
                });
            }
        }
    }
}

fn validate_type_string(value: &MOTLYNode, path: &[String], errors: &mut Vec<SchemaError>) {
    match value {
        MOTLYNode::Value(n) => {
            match &n.eq {
                Some(EqValue::Scalar(Scalar::String(_))) => {} // ok
                _ => {
                    errors.push(SchemaError {
                        message: "Expected type \"string\"".to_string(),
                        path: path.to_vec(),
                        code: "wrong-type",
                    });
                }
            }
        }
        MOTLYNode::Ref(_) => {
            errors.push(SchemaError {
                message: "Expected type \"string\" but found a link".to_string(),
                path: path.to_vec(),
                code: "wrong-type",
            });
        }
    }
}

fn validate_type_number(value: &MOTLYNode, path: &[String], errors: &mut Vec<SchemaError>) {
    match value {
        MOTLYNode::Value(n) => {
            match &n.eq {
                Some(EqValue::Scalar(Scalar::Number(_))) => {} // ok
                _ => {
                    errors.push(SchemaError {
                        message: "Expected type \"number\"".to_string(),
                        path: path.to_vec(),
                        code: "wrong-type",
                    });
                }
            }
        }
        MOTLYNode::Ref(_) => {
            errors.push(SchemaError {
                message: "Expected type \"number\" but found a link".to_string(),
                path: path.to_vec(),
                code: "wrong-type",
            });
        }
    }
}

fn validate_type_boolean(value: &MOTLYNode, path: &[String], errors: &mut Vec<SchemaError>) {
    match value {
        MOTLYNode::Value(n) => {
            match &n.eq {
                Some(EqValue::Scalar(Scalar::Boolean(_))) => {} // ok
                _ => {
                    errors.push(SchemaError {
                        message: "Expected type \"boolean\"".to_string(),
                        path: path.to_vec(),
                        code: "wrong-type",
                    });
                }
            }
        }
        MOTLYNode::Ref(_) => {
            errors.push(SchemaError {
                message: "Expected type \"boolean\" but found a link".to_string(),
                path: path.to_vec(),
                code: "wrong-type",
            });
        }
    }
}

fn validate_type_date(value: &MOTLYNode, path: &[String], errors: &mut Vec<SchemaError>) {
    match value {
        MOTLYNode::Value(n) => {
            match &n.eq {
                Some(EqValue::Scalar(Scalar::Date(_))) => {} // ok
                _ => {
                    errors.push(SchemaError {
                        message: "Expected type \"date\"".to_string(),
                        path: path.to_vec(),
                        code: "wrong-type",
                    });
                }
            }
        }
        MOTLYNode::Ref(_) => {
            errors.push(SchemaError {
                message: "Expected type \"date\" but found a link".to_string(),
                path: path.to_vec(),
                code: "wrong-type",
            });
        }
    }
}

fn validate_type_tag(value: &MOTLYNode, path: &[String], errors: &mut Vec<SchemaError>) {
    // tag means the node should exist (may have properties, no eq required)
    match value {
        MOTLYNode::Value(_) => {} // ok - node exists
        MOTLYNode::Ref(_) => {
            errors.push(SchemaError {
                message: "Expected type \"tag\" but found a link".to_string(),
                path: path.to_vec(),
                code: "wrong-type",
            });
        }
    }
}

fn validate_type_flag(value: &MOTLYNode, path: &[String], errors: &mut Vec<SchemaError>) {
    // flag means the node exists (presence-only)
    match value {
        MOTLYNode::Value(_) => {} // ok
        MOTLYNode::Ref(_) => {
            errors.push(SchemaError {
                message: "Expected type \"flag\" but found a link".to_string(),
                path: path.to_vec(),
                code: "wrong-type",
            });
        }
    }
}

fn validate_type_any(value: &MOTLYNode, path: &[String], errors: &mut Vec<SchemaError>) {
    // any means the value just needs to exist - always valid
    let _ = (value, path, errors);
}

/// Validate an array type like string[], number[], tag[], or CustomType[].
fn validate_array_type(
    value: &MOTLYNode,
    inner_type: &str,
    types: &Option<&BTreeMap<String, MOTLYNode>>,
    path: &[String],
    errors: &mut Vec<SchemaError>,
) {
    let node = match value {
        MOTLYNode::Value(n) => n,
        MOTLYNode::Ref(_) => {
            errors.push(SchemaError {
                message: format!("Expected type \"{}[]\" but found a link", inner_type),
                path: path.to_vec(),
                code: "wrong-type",
            });
            return;
        }
    };

    let arr = match &node.eq {
        Some(EqValue::Array(arr)) => arr,
        _ => {
            errors.push(SchemaError {
                message: format!(
                    "Expected type \"{}[]\" but value is not an array",
                    inner_type
                ),
                path: path.to_vec(),
                code: "wrong-type",
            });
            return;
        }
    };

    // Validate each element
    for (i, elem) in arr.iter().enumerate() {
        let mut elem_path = path.to_vec();
        elem_path.push(format!("[{}]", i));
        validate_base_type(elem, inner_type, types, &elem_path, errors);
    }
}

/// Validate a value against an enum (array of allowed values).
fn validate_enum(
    value: &MOTLYNode,
    allowed: &[MOTLYNode],
    path: &[String],
    errors: &mut Vec<SchemaError>,
) {
    let node = match value {
        MOTLYNode::Value(n) => n,
        MOTLYNode::Ref(_) => {
            errors.push(SchemaError {
                message: "Expected an enum value but found a link".to_string(),
                path: path.to_vec(),
                code: "wrong-type",
            });
            return;
        }
    };

    // Check if the node's eq matches any of the allowed values
    let node_eq = match &node.eq {
        Some(EqValue::Scalar(s)) => s,
        _ => {
            errors.push(SchemaError {
                message: "Expected an enum value".to_string(),
                path: path.to_vec(),
                code: "invalid-enum-value",
            });
            return;
        }
    };

    let matches = allowed.iter().any(|a| match a {
        MOTLYNode::Value(n) => match &n.eq {
            Some(EqValue::Scalar(s)) => s == node_eq,
            _ => false,
        },
        _ => false,
    });

    if !matches {
        let allowed_strs: Vec<String> = allowed
            .iter()
            .filter_map(|a| match a {
                MOTLYNode::Value(n) => match &n.eq {
                    Some(EqValue::Scalar(s)) => Some(format!("{:?}", scalar_display(s))),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        errors.push(SchemaError {
            message: format!(
                "Value does not match any allowed enum value. Allowed: [{}]",
                allowed_strs.join(", ")
            ),
            path: path.to_vec(),
            code: "invalid-enum-value",
        });
    }
}

fn scalar_display(s: &Scalar) -> String {
    match s {
        Scalar::String(v) => v.clone(),
        Scalar::Number(v) => v.to_string(),
        Scalar::Boolean(v) => v.to_string(),
        Scalar::Date(v) => v.clone(),
    }
}

/// Validate a value against a regex pattern (matches property).
fn validate_pattern(
    value: &MOTLYNode,
    matches_node: &MOTLYValue,
    path: &[String],
    errors: &mut Vec<SchemaError>,
) {
    let pattern = match get_eq_string(matches_node) {
        Some(p) => p,
        None => return,
    };

    let node = match value {
        MOTLYNode::Value(n) => n,
        MOTLYNode::Ref(_) => {
            errors.push(SchemaError {
                message: "Expected a value matching a pattern but found a link".to_string(),
                path: path.to_vec(),
                code: "wrong-type",
            });
            return;
        }
    };

    let val_str = match &node.eq {
        Some(EqValue::Scalar(Scalar::String(s))) => s.as_str(),
        _ => {
            errors.push(SchemaError {
                message: format!("Expected a string matching pattern \"{}\"", pattern),
                path: path.to_vec(),
                code: "pattern-mismatch",
            });
            return;
        }
    };

    match Regex::new(pattern) {
        Ok(re) => {
            if !re.is_match(val_str) {
                errors.push(SchemaError {
                    message: format!(
                        "Value \"{}\" does not match pattern \"{}\"",
                        val_str, pattern
                    ),
                    path: path.to_vec(),
                    code: "pattern-mismatch",
                });
            }
        }
        Err(e) => {
            errors.push(SchemaError {
                message: format!("Invalid regex pattern \"{}\": {}", pattern, e),
                path: path.to_vec(),
                code: "invalid-schema",
            });
        }
    }
}

/// Validate a union type (oneOf).
fn validate_union(
    value: &MOTLYNode,
    one_of_node: &MOTLYValue,
    types: &Option<&BTreeMap<String, MOTLYNode>>,
    path: &[String],
    errors: &mut Vec<SchemaError>,
) {
    let type_names = match &one_of_node.eq {
        Some(EqValue::Array(arr)) => arr,
        _ => return,
    };

    // Try each type - if any matches (no errors), it's valid
    for type_val in type_names {
        let type_name = match value_eq_string(type_val) {
            Some(s) => s,
            None => continue,
        };
        let mut trial_errors = Vec::new();
        let synthetic = make_type_spec_node(type_name);
        validate_value_type(
            value,
            &MOTLYNode::Value(synthetic),
            types,
            path,
            &mut trial_errors,
        );
        if trial_errors.is_empty() {
            return; // matches one of the types
        }
    }

    // None matched
    let type_strs: Vec<&str> = type_names
        .iter()
        .filter_map(|v| value_eq_string(v))
        .collect();
    errors.push(SchemaError {
        message: format!(
            "Value does not match any type in oneOf: [{}]",
            type_strs.join(", ")
        ),
        path: path.to_vec(),
        code: "wrong-type",
    });
}
