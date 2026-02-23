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

/// Validate that every reference in the tree resolves to an existing node.
///
/// Returns an empty vec when all references are valid.
pub fn validate_references(root: &MOTLYNode) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    let mut path: Vec<String> = Vec::new();
    let mut ancestors: Vec<&MOTLYNode> = vec![root];
    walk_refs(root, &mut path, &mut ancestors, root, &mut errors);
    errors
}

/// Recursive walk collecting reference errors.
/// References now live as `MOTLYPropertyValue::Ref` in properties and arrays.
fn walk_refs<'a>(
    node: &'a MOTLYNode,
    path: &mut Vec<String>,
    ancestors: &mut Vec<&'a MOTLYNode>,
    root: &'a MOTLYNode,
    errors: &mut Vec<ValidationError>,
) {
    // Check array elements in eq
    if let Some(EqValue::Array(arr)) = &node.eq {
        walk_array_refs(arr, path, ancestors, node, root, errors);
    }

    // Check properties
    if let Some(props) = &node.properties {
        for (key, child_pv) in props {
            path.push(key.clone());

            match child_pv {
                MOTLYPropertyValue::Ref(ref link_to) => {
                    // This property is a reference — check it
                    if let Some(err_msg) = check_link(link_to, ancestors, root) {
                        errors.push(ValidationError {
                            message: err_msg,
                            path: path.clone(),
                            code: "unresolved-reference",
                        });
                    }
                }
                MOTLYPropertyValue::Node(child) => {
                    // Recurse into child for arrays and sub-properties
                    ancestors.push(node);
                    walk_refs(child, path, ancestors, root, errors);
                    ancestors.pop();
                }
            }

            path.pop();
        }
    }
}

/// Walk array elements looking for references.
fn walk_array_refs<'a>(
    arr: &'a [MOTLYPropertyValue],
    path: &mut Vec<String>,
    ancestors: &mut Vec<&'a MOTLYNode>,
    parent_node: &'a MOTLYNode,
    root: &'a MOTLYNode,
    errors: &mut Vec<ValidationError>,
) {
    for (i, elem_pv) in arr.iter().enumerate() {
        let idx_key = format!("[{}]", i);
        path.push(idx_key);

        match elem_pv {
            MOTLYPropertyValue::Ref(ref link_to) => {
                if let Some(err_msg) = check_link(link_to, ancestors, root) {
                    errors.push(ValidationError {
                        message: err_msg,
                        path: path.clone(),
                        code: "unresolved-reference",
                    });
                }
            }
            MOTLYPropertyValue::Node(elem) => {
                // Recurse into element for its own arrays and properties
                ancestors.push(parent_node);
                walk_refs(elem, path, ancestors, root, errors);
                ancestors.pop();
            }
        }

        path.pop();
    }
}

/// Check whether a link resolves. Returns `Some(error_message)` on failure.
fn check_link(link_to: &str, ancestors: &[&MOTLYNode], root: &MOTLYNode) -> Option<String> {
    let parsed = parse_link_string(link_to);
    let (ups, segments) = match parsed {
        Ok((ups, segs)) => (ups, segs),
        Err(msg) => return Some(msg),
    };

    // Determine the start node for resolution.
    let start = if ups == 0 {
        // Absolute: start from root
        root
    } else {
        // Relative: go up `ups` levels in the ancestor stack.
        let idx = ancestors.len().checked_sub(ups);
        match idx {
            Some(i) if i < ancestors.len() => ancestors[i],
            _ => {
                return Some(format!(
                    "Reference \"{}\" goes {} level(s) up but only {} ancestor(s) available",
                    link_to,
                    ups,
                    ancestors.len()
                ));
            }
        }
    };

    // Follow the path segments from the start node.
    resolve_path(start, &segments, link_to)
}

/// Parsed reference segment for resolution.
enum RefSeg {
    Name(String),
    Index(usize),
}

/// Parse a link_to string like "$^^items[0].name" into (ups, segments).
/// Returns an error for invalid array indices.
fn parse_link_string(s: &str) -> Result<(usize, Vec<RefSeg>), String> {
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
                match idx_buf.parse::<usize>() {
                    Ok(idx) => segments.push(RefSeg::Index(idx)),
                    Err(_) => {
                        return Err(format!(
                            "Reference \"{}\" has invalid array index \"[{}]\"",
                            s, idx_buf
                        ));
                    }
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

    Ok((ups, segments))
}

/// Follow path segments from a start node. Returns Some(error) if unresolved.
fn resolve_path(start: &MOTLYNode, segments: &[RefSeg], link_str: &str) -> Option<String> {
    let mut current: ResolveTarget = ResolveTarget::Node(start);

    for seg in segments {
        match (seg, current) {
            (RefSeg::Name(name), ResolveTarget::Node(node)) => {
                match &node.properties {
                    Some(props) => match props.get(name.as_str()) {
                        Some(MOTLYPropertyValue::Ref(_)) => {
                            // If child is a reference itself, treat as terminal
                            current = ResolveTarget::Terminal;
                        }
                        Some(MOTLYPropertyValue::Node(child)) => {
                            current = ResolveTarget::Node(child);
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
                        MOTLYPropertyValue::Ref(_) => {
                            current = ResolveTarget::Terminal;
                        }
                        MOTLYPropertyValue::Node(elem) => {
                            current = ResolveTarget::Node(elem);
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
    Node(&'a MOTLYNode),
    Terminal,
}

// ── Schema validation ───────────────────────────────────────────────

/// Validate a MOTLY tree against a schema (also a MOTLY tree).
///
/// The schema should have sections like `Required`, `Optional`, `Types`, and `Additional`.
/// Returns an empty vec when the tree conforms to the schema.
pub fn validate_schema(tag: &MOTLYNode, schema: &MOTLYNode) -> Vec<SchemaError> {
    let mut errors = Vec::new();
    let types = extract_section(schema, "Types");
    let path: Vec<String> = Vec::new();
    validate_node_against_schema(tag, schema, &types, &path, &mut errors);
    errors
}

/// Extract a section from a schema node by property name.
fn extract_section<'a>(
    node: &'a MOTLYNode,
    name: &str,
) -> Option<&'a BTreeMap<String, MOTLYPropertyValue>> {
    let pv = node.properties.as_ref()?.get(name)?;
    match pv {
        MOTLYPropertyValue::Node(v) => v.properties.as_ref(),
        MOTLYPropertyValue::Ref(_) => None,
    }
}

/// Get the `eq` string value of a node.
fn get_eq_string(node: &MOTLYNode) -> Option<&str> {
    match &node.eq {
        Some(EqValue::Scalar(Scalar::String(s))) => Some(s.as_str()),
        _ => None,
    }
}

/// Get the `eq` string of a MOTLYPropertyValue if it's a Node with a scalar string eq.
fn pv_eq_string(pv: &MOTLYPropertyValue) -> Option<&str> {
    match pv {
        MOTLYPropertyValue::Node(n) => get_eq_string(n),
        MOTLYPropertyValue::Ref(_) => None,
    }
}

/// Validate a node against a schema node (which has Required/Optional/Additional sections).
fn validate_node_against_schema(
    tag: &MOTLYNode,
    schema: &MOTLYNode,
    types: &Option<&BTreeMap<String, MOTLYPropertyValue>>,
    path: &[String],
    errors: &mut Vec<SchemaError>,
) {
    let required = extract_section(schema, "Required");
    let optional = extract_section(schema, "Optional");
    let additional = get_additional_policy(schema);

    let tag_props = tag.properties.as_ref();

    // Check required properties
    if let Some(req) = required {
        for (key, type_spec_pv) in req {
            let mut prop_path = path.to_vec();
            prop_path.push(key.clone());

            let tag_value_pv = tag_props.and_then(|p| p.get(key.as_str()));
            match tag_value_pv {
                None => {
                    errors.push(SchemaError {
                        message: format!("Missing required property \"{}\"", key),
                        path: prop_path,
                        code: "missing-required",
                    });
                }
                Some(value_pv) => {
                    validate_value_type(value_pv, type_spec_pv, types, &prop_path, errors);
                }
            }
        }
    }

    // Check optional properties that exist
    if let Some(opt) = optional {
        if let Some(tag_p) = tag_props {
            for (key, type_spec_pv) in opt {
                if let Some(value_pv) = tag_p.get(key.as_str()) {
                    let mut prop_path = path.to_vec();
                    prop_path.push(key.clone());
                    validate_value_type(value_pv, type_spec_pv, types, &prop_path, errors);
                }
            }
        }
    }

    // Check for unknown properties
    if let Some(tag_p) = tag_props {
        let known_keys = collect_known_keys(required, optional);
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
                    if let Some(value_pv) = tag_p.get(key.as_str()) {
                        let synthetic =
                            MOTLYPropertyValue::Node(make_type_spec_node(type_name));
                        validate_value_type(value_pv, &synthetic, types, &prop_path, errors);
                    }
                }
            }
        }
    }
}

/// Collect known keys from Required + Optional sections.
fn collect_known_keys<'a>(
    required: Option<&'a BTreeMap<String, MOTLYPropertyValue>>,
    optional: Option<&'a BTreeMap<String, MOTLYPropertyValue>>,
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
fn get_additional_policy(schema: &MOTLYNode) -> AdditionalPolicy {
    let props = match &schema.properties {
        Some(p) => p,
        None => return AdditionalPolicy::Reject,
    };
    let additional_pv = match props.get("Additional") {
        Some(v) => v,
        None => return AdditionalPolicy::Reject,
    };
    let additional = match additional_pv {
        MOTLYPropertyValue::Ref(_) => return AdditionalPolicy::Reject,
        MOTLYPropertyValue::Node(n) => n,
    };
    if let Some(eq_str) = get_eq_string(additional) {
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

/// Create a simple type spec node with eq=type_name.
fn make_type_spec_node(type_name: &str) -> MOTLYNode {
    MOTLYNode::with_eq(EqValue::Scalar(Scalar::String(type_name.to_string())))
}

/// Validate a MOTLYPropertyValue against a type specifier (also a MOTLYPropertyValue).
fn validate_value_type(
    value_pv: &MOTLYPropertyValue,
    type_spec_pv: &MOTLYPropertyValue,
    types: &Option<&BTreeMap<String, MOTLYPropertyValue>>,
    path: &[String],
    errors: &mut Vec<SchemaError>,
) {
    // Skip ref type specs in schema
    let spec_node = match type_spec_pv {
        MOTLYPropertyValue::Ref(_) => return,
        MOTLYPropertyValue::Node(n) => n,
    };

    // If value is a ref, generate appropriate "found a link" error
    if value_pv.is_ref() {
        push_ref_type_error(spec_node, path, errors);
        return;
    }

    // Value is a Node — extract it
    let value = match value_pv {
        MOTLYPropertyValue::Node(n) => n,
        MOTLYPropertyValue::Ref(_) => unreachable!(),
    };

    validate_node_against_type_spec(value, spec_node, types, path, errors);
}

/// Generate the appropriate "found a link" error based on what was expected by the spec.
fn push_ref_type_error(spec_node: &MOTLYNode, path: &[String], errors: &mut Vec<SchemaError>) {
    // Check for enum
    if let Some(spec_props) = &spec_node.properties {
        if let Some(MOTLYPropertyValue::Node(eq_node)) = spec_props.get("eq") {
            if matches!(&eq_node.eq, Some(EqValue::Array(_))) {
                errors.push(SchemaError {
                    message: "Expected an enum value but found a link".to_string(),
                    path: path.to_vec(),
                    code: "wrong-type",
                });
                return;
            }
        }
        if spec_props.contains_key("matches") {
            errors.push(SchemaError {
                message: "Expected a value matching a pattern but found a link".to_string(),
                path: path.to_vec(),
                code: "wrong-type",
            });
            return;
        }
    }

    if let Some(type_name) = get_eq_string(spec_node) {
        errors.push(SchemaError {
            message: format!("Expected type \"{}\" but found a link", type_name),
            path: path.to_vec(),
            code: "wrong-type",
        });
    } else if spec_node.properties.as_ref().map_or(false, |p| {
        p.contains_key("Required")
            || p.contains_key("Optional")
            || p.contains_key("Additional")
    }) {
        errors.push(SchemaError {
            message: "Expected a tag but found a link".to_string(),
            path: path.to_vec(),
            code: "wrong-type",
        });
    }
}

/// Validate a confirmed-node value against a confirmed-node type spec.
fn validate_node_against_type_spec(
    value: &MOTLYNode,
    spec_node: &MOTLYNode,
    types: &Option<&BTreeMap<String, MOTLYPropertyValue>>,
    path: &[String],
    errors: &mut Vec<SchemaError>,
) {
    // Check if this is a union type (oneOf)
    if let Some(one_of_props) = &spec_node.properties {
        if let Some(MOTLYPropertyValue::Node(one_of_node)) = one_of_props.get("oneOf") {
            validate_union(value, one_of_node, types, path, errors);
            return;
        }
    }

    // Check if this is an enum type (eq) or pattern (matches)
    if let Some(spec_props) = &spec_node.properties {
        if let Some(MOTLYPropertyValue::Node(eq_node)) = spec_props.get("eq") {
            if let Some(EqValue::Array(allowed)) = &eq_node.eq {
                validate_enum(value, allowed, path, errors);
                return;
            }
        }
        // Check for pattern matching (matches)
        if let Some(MOTLYPropertyValue::Node(matches_node)) = spec_props.get("matches") {
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
                validate_node_against_schema(value, spec_node, types, path, errors);
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
    types: &Option<&BTreeMap<String, MOTLYPropertyValue>>,
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
        "tag" => {} // tag means the node exists — always valid for a non-ref node
        "flag" => {} // flag means presence-only — always valid for a non-ref node
        "any" => {}  // any — always valid
        custom => {
            // Look up custom type in Types section
            if let Some(types_map) = types {
                if let Some(type_def_pv) = types_map.get(custom) {
                    match type_def_pv {
                        MOTLYPropertyValue::Ref(_) => {
                            // Type definition is a ref — skip
                        }
                        MOTLYPropertyValue::Node(type_def) => {
                            validate_node_against_type_spec(
                                value, type_def, types, path, errors,
                            );
                        }
                    }
                } else {
                    errors.push(SchemaError {
                        message: format!("Unknown type \"{}\" in schema", custom),
                        path: path.to_vec(),
                        code: "invalid-schema",
                    });
                }
            } else {
                errors.push(SchemaError {
                    message: format!(
                        "Unknown type \"{}\" (no Types section in schema)",
                        custom
                    ),
                    path: path.to_vec(),
                    code: "invalid-schema",
                });
            }
        }
    }
}

fn validate_type_string(value: &MOTLYNode, path: &[String], errors: &mut Vec<SchemaError>) {
    match &value.eq {
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

fn validate_type_number(value: &MOTLYNode, path: &[String], errors: &mut Vec<SchemaError>) {
    match &value.eq {
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

fn validate_type_boolean(value: &MOTLYNode, path: &[String], errors: &mut Vec<SchemaError>) {
    match &value.eq {
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

fn validate_type_date(value: &MOTLYNode, path: &[String], errors: &mut Vec<SchemaError>) {
    match &value.eq {
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

/// Validate an array type like string[], number[], tag[], or CustomType[].
fn validate_array_type(
    value: &MOTLYNode,
    inner_type: &str,
    types: &Option<&BTreeMap<String, MOTLYPropertyValue>>,
    path: &[String],
    errors: &mut Vec<SchemaError>,
) {
    let arr = match &value.eq {
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
    for (i, elem_pv) in arr.iter().enumerate() {
        let mut elem_path = path.to_vec();
        elem_path.push(format!("[{}]", i));
        match elem_pv {
            MOTLYPropertyValue::Ref(_) => {
                errors.push(SchemaError {
                    message: format!(
                        "Expected type \"{}\" but found a link",
                        inner_type
                    ),
                    path: elem_path,
                    code: "wrong-type",
                });
            }
            MOTLYPropertyValue::Node(elem) => {
                validate_base_type(elem, inner_type, types, &elem_path, errors);
            }
        }
    }
}

/// Validate a value against an enum (array of allowed values).
fn validate_enum(
    value: &MOTLYNode,
    allowed: &[MOTLYPropertyValue],
    path: &[String],
    errors: &mut Vec<SchemaError>,
) {
    // Check if the node's eq matches any of the allowed values
    let node_eq = match &value.eq {
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
        MOTLYPropertyValue::Node(n) => match &n.eq {
            Some(EqValue::Scalar(s)) => s == node_eq,
            _ => false,
        },
        MOTLYPropertyValue::Ref(_) => false,
    });

    if !matches {
        let allowed_strs: Vec<String> = allowed
            .iter()
            .filter_map(|a| match a {
                MOTLYPropertyValue::Node(n) => match &n.eq {
                    Some(EqValue::Scalar(s)) => Some(format!("{:?}", scalar_display(s))),
                    _ => None,
                },
                MOTLYPropertyValue::Ref(_) => None,
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
    matches_node: &MOTLYNode,
    path: &[String],
    errors: &mut Vec<SchemaError>,
) {
    let pattern = match get_eq_string(matches_node) {
        Some(p) => p,
        None => return,
    };

    let val_str = match &value.eq {
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
    one_of_node: &MOTLYNode,
    types: &Option<&BTreeMap<String, MOTLYPropertyValue>>,
    path: &[String],
    errors: &mut Vec<SchemaError>,
) {
    let type_names = match &one_of_node.eq {
        Some(EqValue::Array(arr)) => arr,
        _ => return,
    };

    // Try each type - if any matches (no errors), it's valid
    let value_pv = MOTLYPropertyValue::Node(value.clone());
    for type_pv in type_names {
        let type_name = match pv_eq_string(type_pv) {
            Some(s) => s,
            None => continue,
        };
        let mut trial_errors = Vec::new();
        let synthetic = MOTLYPropertyValue::Node(make_type_spec_node(type_name));
        validate_value_type(&value_pv, &synthetic, types, path, &mut trial_errors);
        if trial_errors.is_empty() {
            return; // matches one of the types
        }
    }

    // None matched
    let type_strs: Vec<&str> = type_names
        .iter()
        .filter_map(|v| pv_eq_string(v))
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
