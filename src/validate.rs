use crate::tree::*;

// ── Error types ─────────────────────────────────────────────────────

/// An error found during reference validation.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationError {
    pub message: String,
    /// Path in the tree where the error was found (e.g. ["outer", "inner", "ref"]).
    pub path: Vec<String>,
    /// Machine-readable error code.
    pub code: &'static str,
    /// Source location of the offending node (if available).
    pub location: Option<MOTLYLocation>,
}

/// An error found during schema validation.
#[derive(Debug, Clone, PartialEq)]
pub struct SchemaError {
    pub message: String,
    /// Path in the tree where the error was found.
    pub path: Vec<String>,
    /// Machine-readable error code.
    pub code: &'static str,
    /// Source location of the offending node (if available).
    pub location: Option<MOTLYLocation>,
}

// ── Reference validation ────────────────────────────────────────────

/// Validate that every reference in the tree resolves to an existing node.
pub fn validate_references(root: &MOTLYDataNode) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    let mut path: Vec<String> = Vec::new();
    let mut ancestors: Vec<&MOTLYDataNode> = vec![root];
    walk_refs(root, &mut path, &mut ancestors, root, &mut errors);
    errors
}

fn walk_refs<'a>(
    node: &'a MOTLYDataNode,
    path: &mut Vec<String>,
    ancestors: &mut Vec<&'a MOTLYDataNode>,
    root: &'a MOTLYDataNode,
    errors: &mut Vec<ValidationError>,
) {
    if let Some(EqValue::Array(arr)) = &node.eq {
        walk_array_refs(arr, path, ancestors, node, root, errors);
    }

    if let Some(props) = &node.properties {
        for (key, child_pv) in props {
            path.push(key.clone());

            match child_pv {
                MOTLYNode::Ref { ref link_to, link_ups } => {
                    if let Some(err_msg) = check_link(link_to, *link_ups, ancestors, root) {
                        let mut err = ValidationError {
                            message: err_msg,
                            path: path.clone(),
                            code: "unresolved-reference",
                            location: None,
                        };
                        if let Some(loc) = node.location {
                            err.location = Some(loc);
                        }
                        errors.push(err);
                    }
                }
                MOTLYNode::Data(child) => {
                    ancestors.push(node);
                    walk_refs(child, path, ancestors, root, errors);
                    ancestors.pop();
                }
            }

            path.pop();
        }
    }
}

fn walk_array_refs<'a>(
    arr: &'a [MOTLYNode],
    path: &mut Vec<String>,
    ancestors: &mut Vec<&'a MOTLYDataNode>,
    parent_node: &'a MOTLYDataNode,
    root: &'a MOTLYDataNode,
    errors: &mut Vec<ValidationError>,
) {
    for (i, elem_pv) in arr.iter().enumerate() {
        let idx_key = format!("[{}]", i);
        path.push(idx_key);

        match elem_pv {
            MOTLYNode::Ref { ref link_to, link_ups } => {
                if let Some(err_msg) = check_link(link_to, *link_ups, ancestors, root) {
                    errors.push(ValidationError {
                        message: err_msg,
                        path: path.clone(),
                        code: "unresolved-reference",
                        location: None,
                    });
                }
            }
            MOTLYNode::Data(elem) => {
                ancestors.push(parent_node);
                walk_refs(elem, path, ancestors, root, errors);
                ancestors.pop();
            }
        }

        path.pop();
    }
}

fn check_link(segments: &[RefSegment], ups: usize, ancestors: &[&MOTLYDataNode], root: &MOTLYDataNode) -> Option<String> {
    let link_str = format_ref_display(ups, segments);

    let start = if ups == 0 {
        root
    } else {
        let idx = ancestors.len().checked_sub(ups);
        match idx {
            Some(i) if i < ancestors.len() => ancestors[i],
            _ => {
                return Some(format!(
                    "Reference \"{}\" goes {} level(s) up but only {} ancestor(s) available",
                    link_str, ups, ancestors.len()
                ));
            }
        }
    };

    resolve_path(start, segments, &link_str)
}

fn resolve_path(start: &MOTLYDataNode, segments: &[RefSegment], link_str: &str) -> Option<String> {
    let mut current: ResolveTarget = ResolveTarget::Node(start);

    for seg in segments {
        match (seg, current) {
            (RefSegment::Name(name), ResolveTarget::Node(node)) => {
                match &node.properties {
                    Some(props) => match props.get(name.as_str()) {
                        Some(MOTLYNode::Ref { .. }) => {
                            current = ResolveTarget::Terminal;
                        }
                        Some(MOTLYNode::Data(child)) => {
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
            (RefSegment::Index(idx), ResolveTarget::Node(node)) => match &node.eq {
                Some(EqValue::Array(arr)) => {
                    if *idx >= arr.len() {
                        return Some(format!(
                            "Reference \"{}\" could not be resolved: index [{}] out of bounds (array length {})",
                            link_str, idx, arr.len()
                        ));
                    }
                    match &arr[*idx] {
                        MOTLYNode::Ref { .. } => {
                            current = ResolveTarget::Terminal;
                        }
                        MOTLYNode::Data(elem) => {
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
                return Some(format!(
                    "Reference \"{}\" could not be resolved: cannot follow path through a link",
                    link_str
                ));
            }
        }
    }

    None
}

enum ResolveTarget<'a> {
    Node(&'a MOTLYDataNode),
    Terminal,
}

// ── Schema validation ───────────────────────────────────────────────

type TypesMap = std::collections::BTreeMap<String, MOTLYDataNode>;

fn preloaded_types() -> TypesMap {
    let mut map = std::collections::BTreeMap::new();

    let make_val = |val: &str| {
        let mut props = std::collections::BTreeMap::new();
        props.insert(
            "VALUE".to_string(),
            MOTLYNode::Data(MOTLYDataNode::with_eq(EqValue::Scalar(Scalar::String(val.to_string())))),
        );
        let mut node = MOTLYDataNode::new();
        node.properties = Some(props);
        node
    };

    let make_additional = |val: &str| {
        let mut props = std::collections::BTreeMap::new();
        props.insert(
            "ADDITIONAL".to_string(),
            MOTLYNode::Data(MOTLYDataNode::with_eq(EqValue::Scalar(Scalar::String(val.to_string())))),
        );
        let mut node = MOTLYDataNode::new();
        node.properties = Some(props);
        node
    };

    map.insert("string".to_string(), make_val("string"));
    map.insert("number".to_string(), make_val("number"));
    map.insert("integer".to_string(), make_val("integer"));
    map.insert("boolean".to_string(), make_val("boolean"));
    map.insert("date".to_string(), make_val("date"));

    map.insert("flag".to_string(), make_additional("reject"));
    map.insert("tag".to_string(), make_additional("accept"));
    map.insert("any".to_string(), make_additional("accept"));

    map
}

fn build_types_map(schema: &MOTLYDataNode, errors: &mut Vec<SchemaError>) -> TypesMap {
    let mut types = preloaded_types();
    if let Some(types_node) = get_directive(schema, "TYPES") {
        if let Some(properties) = &types_node.properties {
            let preloaded = ["string", "number", "integer", "boolean", "date", "flag", "tag", "any"];
            for (name, pv) in properties {
                if let MOTLYNode::Data(child_node) = pv {
                    if preloaded.contains(&name.as_str()) {
                        errors.push(SchemaError {
                            message: format!("Type \"{}\" cannot shadow pre-loaded type", name),
                            path: vec!["TYPES".to_string(), name.clone()],
                            code: "invalid-schema",
                            location: child_node.location,
                        });
                        continue;
                    }
                    types.insert(name.clone(), child_node.clone());
                }
            }
        }
    }
    types
}

fn get_directive<'a>(node: &'a MOTLYDataNode, name: &str) -> Option<&'a MOTLYDataNode> {
    if let Some(props) = &node.properties {
        if let Some(MOTLYNode::Data(child)) = props.get(name) {
            return Some(child);
        }
    }
    None
}

fn get_eq_string(node: &MOTLYDataNode) -> Option<&str> {
    if let Some(EqValue::Scalar(Scalar::String(s))) = &node.eq {
        Some(s.as_str())
    } else {
        None
    }
}

/// Validate a MOTLY tree against a schema (also a MOTLY tree).
pub fn validate_schema(target: &MOTLYDataNode, schema: &MOTLYDataNode) -> Vec<SchemaError> {
    let mut errors = Vec::new();
    let types = build_types_map(schema, &mut errors);
    let mut path = Vec::new();
    validate_constraint(target, schema, &types, &mut path, &mut errors, 0);
    errors
}

const MAX_VALIDATION_DEPTH: usize = 64;

fn validate_constraint(
    target: &MOTLYDataNode,
    constraint: &MOTLYDataNode,
    types: &TypesMap,
    path: &mut Vec<String>,
    errors: &mut Vec<SchemaError>,
    depth: usize,
) {
    if depth > MAX_VALIDATION_DEPTH {
        errors.push(SchemaError {
            message: "Maximum validation depth exceeded (possible recursive type cycle)".to_string(),
            path: path.clone(),
            code: "invalid-schema",
            location: target.location,
        });
        return;
    }

    // ONEOF — union dispatch
    if let Some(one_of_node) = get_directive(constraint, "ONEOF") {
        if let Some(EqValue::Array(arr)) = &one_of_node.eq {
            validate_one_of_array(target, arr, types, path, errors, depth);
            return;
        }
    }

    // VALUE — value slot constraint
    if let Some(value_node) = get_directive(constraint, "VALUE") {
        validate_value(target, value_node, types, path, errors, depth);
    }

    // Property structure (REQUIRED, OPTIONAL, ADDITIONAL, EXCLUSIVE, REQUIRES)
    validate_properties(target, constraint, types, path, errors, depth);
}

fn validate_value(
    target: &MOTLYDataNode,
    value_node: &MOTLYDataNode,
    types: &TypesMap,
    path: &mut Vec<String>,
    errors: &mut Vec<SchemaError>,
    depth: usize,
) {
    let value_type = match get_eq_string(value_node) {
        Some(s) => s,
        None => return,
    };

    let eq = &target.eq;
    match value_type {
        "string" => {
            if let Some(EqValue::Scalar(Scalar::String(s))) = eq {
                validate_string_refinements(s, value_node, path, errors, target);
            } else {
                errors.push(SchemaError {
                    message: format!("Expected string, got {}", describe_value(eq)),
                    path: path.clone(),
                    code: "wrong-type",
                    location: target.location,
                });
            }
        }
        "number" => {
            if let Some(EqValue::Scalar(Scalar::Number(n))) = eq {
                validate_number_refinements(*n, value_node, path, errors, target);
            } else {
                errors.push(SchemaError {
                    message: format!("Expected number, got {}", describe_value(eq)),
                    path: path.clone(),
                    code: "wrong-type",
                    location: target.location,
                });
            }
        }
        "integer" => {
            if let Some(EqValue::Scalar(Scalar::Number(n))) = eq {
                if n.fract() == 0.0 {
                    validate_number_refinements(*n, value_node, path, errors, target);
                } else {
                    errors.push(SchemaError {
                        message: format!("Expected integer, got {}", describe_value(eq)),
                        path: path.clone(),
                        code: "wrong-type",
                        location: target.location,
                    });
                }
            } else {
                errors.push(SchemaError {
                    message: format!("Expected integer, got {}", describe_value(eq)),
                    path: path.clone(),
                    code: "wrong-type",
                    location: target.location,
                });
            }
        }
        "boolean" => {
            if let Some(EqValue::Scalar(Scalar::Boolean(b))) = eq {
                validate_enum_refinement(&Scalar::Boolean(*b), value_node, path, errors, target);
            } else {
                errors.push(SchemaError {
                    message: format!("Expected boolean, got {}", describe_value(eq)),
                    path: path.clone(),
                    code: "wrong-type",
                    location: target.location,
                });
            }
        }
        "date" => {
            if let Some(EqValue::Scalar(Scalar::Date(d))) = eq {
                validate_enum_refinement(&Scalar::Date(d.clone()), value_node, path, errors, target);
            } else {
                errors.push(SchemaError {
                    message: format!("Expected date, got {}", describe_value(eq)),
                    path: path.clone(),
                    code: "wrong-type",
                    location: target.location,
                });
            }
        }
        _ => {
            if let Some(type_def) = types.get(value_type) {
                if let Some(inner_value) = get_directive(type_def, "VALUE") {
                    validate_value(target, inner_value, types, path, errors, depth + 1);
                } else {
                    errors.push(SchemaError {
                        message: format!(
                            "Type \"{}\" cannot be used as a VALUE type (no VALUE constraint)",
                            value_type
                        ),
                        path: path.clone(),
                        code: "invalid-schema",
                        location: target.location,
                    });
                }
            } else {
                errors.push(SchemaError {
                    message: format!("Unknown VALUE type \"{}\"", value_type),
                    path: path.clone(),
                    code: "invalid-schema",
                    location: target.location,
                });
            }
        }
    }
}

fn describe_value(eq: &Option<EqValue>) -> &'static str {
    match eq {
        None => "no value",
        Some(EqValue::Scalar(Scalar::String(_))) => "string",
        Some(EqValue::Scalar(Scalar::Number(_))) => "number",
        Some(EqValue::Scalar(Scalar::Boolean(_))) => "boolean",
        Some(EqValue::Scalar(Scalar::Date(_))) => "date",
        Some(EqValue::Array(_)) => "array",
        Some(EqValue::EnvRef(_)) => "env-ref",
    }
}

fn validate_string_refinements(
    value: &str,
    value_node: &MOTLYDataNode,
    path: &mut Vec<String>,
    errors: &mut Vec<SchemaError>,
    target: &MOTLYDataNode,
) {
    validate_enum_refinement(&Scalar::String(value.to_string()), value_node, path, errors, target);

    if let Some(matches_node) = get_directive(value_node, "MATCHES") {
        if let Some(matches_pattern) = get_eq_string(matches_node) {
            match regex::Regex::new(matches_pattern) {
                Ok(re) => {
                    if !re.is_match(value) {
                        errors.push(SchemaError {
                            message: format!("Value \"{}\" does not match pattern \"{}\"", value, matches_pattern),
                            path: path.clone(),
                            code: "pattern-mismatch",
                            location: target.location,
                        });
                    }
                }
                Err(e) => {
                    errors.push(SchemaError {
                        message: format!("Invalid regex pattern \"{}\": {}", matches_pattern, e),
                        path: path.clone(),
                        code: "invalid-schema",
                        location: target.location,
                    });
                }
            }
        }
    }

    if let Some(min_len_node) = get_directive(value_node, "MIN_LENGTH") {
        if let Some(EqValue::Scalar(Scalar::Number(n))) = &min_len_node.eq {
            let min_len = *n as usize;
            if value.len() < min_len {
                errors.push(SchemaError {
                    message: format!("String length {} is less than minimum {}", value.len(), min_len),
                    path: path.clone(),
                    code: "length-violation",
                    location: target.location,
                });
            }
        }
    }

    if let Some(max_len_node) = get_directive(value_node, "MAX_LENGTH") {
        if let Some(EqValue::Scalar(Scalar::Number(n))) = &max_len_node.eq {
            let max_len = *n as usize;
            if value.len() > max_len {
                errors.push(SchemaError {
                    message: format!("String length {} exceeds maximum {}", value.len(), max_len),
                    path: path.clone(),
                    code: "length-violation",
                    location: target.location,
                });
            }
        }
    }
}

fn validate_number_refinements(
    value: f64,
    value_node: &MOTLYDataNode,
    path: &mut Vec<String>,
    errors: &mut Vec<SchemaError>,
    target: &MOTLYDataNode,
) {
    validate_enum_refinement(&Scalar::Number(value), value_node, path, errors, target);

    if let Some(min_node) = get_directive(value_node, "MIN") {
        if let Some(EqValue::Scalar(Scalar::Number(n))) = &min_node.eq {
            if value < *n {
                errors.push(SchemaError {
                    message: format!("Value {} is less than minimum {}", value, n),
                    path: path.clone(),
                    code: "out-of-range",
                    location: target.location,
                });
            }
        }
    }

    if let Some(max_node) = get_directive(value_node, "MAX") {
        if let Some(EqValue::Scalar(Scalar::Number(n))) = &max_node.eq {
            if value > *n {
                errors.push(SchemaError {
                    message: format!("Value {} exceeds maximum {}", value, n),
                    path: path.clone(),
                    code: "out-of-range",
                    location: target.location,
                });
            }
        }
    }
}

fn validate_enum_refinement(
    value: &Scalar,
    value_node: &MOTLYDataNode,
    path: &mut Vec<String>,
    errors: &mut Vec<SchemaError>,
    target: &MOTLYDataNode,
) {
    let enum_node = match get_directive(value_node, "ENUM") {
        Some(n) => n,
        None => return,
    };
    let arr = match &enum_node.eq {
        Some(EqValue::Array(a)) => a,
        _ => return,
    };

    let mut matches = false;
    let mut allowed = Vec::new();

    for a in arr {
        if let MOTLYNode::Data(child) = a {
            if let Some(EqValue::Scalar(s)) = &child.eq {
                allowed.push(scalar_to_string(s));
                if s == value {
                    matches = true;
                }
            }
        }
    }

    if !matches {
        errors.push(SchemaError {
            message: format!(
                "Value does not match any allowed enum value. Allowed: [{}]",
                allowed.join(", ")
            ),
            path: path.clone(),
            code: "invalid-enum-value",
            location: target.location,
        });
    }
}

fn scalar_to_string(s: &Scalar) -> String {
    match s {
        Scalar::String(val) => val.clone(),
        Scalar::Number(val) => val.to_string(),
        Scalar::Boolean(val) => {
            if *val {
                "@true".to_string()
            } else {
                "@false".to_string()
            }
        }
        Scalar::Date(val) => format!("@{}", val),
    }
}

enum AdditionalPolicy<'a> {
    Reject,
    Accept,
    Type(String),
    Inline(&'a MOTLYDataNode),
}

fn get_additional_policy<'a>(constraint: &'a MOTLYDataNode) -> AdditionalPolicy<'a> {
    let props = match &constraint.properties {
        Some(p) => p,
        None => return AdditionalPolicy::Reject,
    };
    let pv = match props.get("ADDITIONAL") {
        Some(v) => v,
        None => return AdditionalPolicy::Reject,
    };
    let additional_node = match pv {
        MOTLYNode::Data(n) => n,
        MOTLYNode::Ref { .. } => return AdditionalPolicy::Reject,
    };

    if let Some(EqValue::Scalar(Scalar::String(val_str))) = &additional_node.eq {
        if val_str == "reject" {
            return AdditionalPolicy::Reject;
        }
        if val_str == "accept" {
            return AdditionalPolicy::Accept;
        }
        return AdditionalPolicy::Type(val_str.clone());
    }

    if let Some(props) = &additional_node.properties {
        let keys = props.keys();
        if keys
            .clone()
            .any(|k| k == "VALUE" || k == "REQUIRED" || k == "OPTIONAL" || k == "ADDITIONAL" || k == "ONEOF")
        {
            return AdditionalPolicy::Inline(additional_node);
        }
    }

    AdditionalPolicy::Accept
}

fn validate_properties(
    target: &MOTLYDataNode,
    constraint: &MOTLYDataNode,
    types: &TypesMap,
    path: &mut Vec<String>,
    errors: &mut Vec<SchemaError>,
    depth: usize,
) {
    let required = get_directive(constraint, "REQUIRED").and_then(|n| n.properties.as_ref());
    let optional = get_directive(constraint, "OPTIONAL").and_then(|n| n.properties.as_ref());
    let additional = get_additional_policy(constraint);
    let target_props = target.properties.as_ref();

    if let Some(req_map) = required {
        for (key, prop_def_pv) in req_map {
            if prop_def_pv.is_ref() {
                continue;
            }
            path.push(key.clone());
            if let Some(target_val) = target_props.and_then(|p| p.get(key)) {
                if let MOTLYNode::Data(prop_def_node) = prop_def_pv {
                    validate_property_value(target_val, prop_def_node, types, path, errors, depth);
                }
            } else {
                errors.push(SchemaError {
                    message: format!("Missing required property \"{}\"", key),
                    path: path.clone(),
                    code: "missing-required",
                    location: target.location,
                });
            }
            path.pop();
        }
    }

    if let Some(opt_map) = optional {
        if let Some(t_props) = target_props {
            for (key, prop_def_pv) in opt_map {
                if prop_def_pv.is_ref() {
                    continue;
                }
                if let Some(target_val) = t_props.get(key) {
                    path.push(key.clone());
                    if let MOTLYNode::Data(prop_def_node) = prop_def_pv {
                        validate_property_value(target_val, prop_def_node, types, path, errors, depth);
                    }
                    path.pop();
                }
            }
        }
    }

    if let Some(t_props) = target_props {
        let mut known_keys = std::collections::HashSet::new();
        if let Some(req_map) = required {
            for k in req_map.keys() {
                known_keys.insert(k.as_str());
            }
        }
        if let Some(opt_map) = optional {
            for k in opt_map.keys() {
                known_keys.insert(k.as_str());
            }
        }

        for (key, pv) in t_props {
            if known_keys.contains(key.as_str()) {
                continue;
            }
            path.push(key.clone());
            match &additional {
                AdditionalPolicy::Reject => {
                    let loc = match pv {
                        MOTLYNode::Data(node) => node.location,
                        MOTLYNode::Ref { .. } => None,
                    };
                    errors.push(SchemaError {
                        message: format!("Unknown property \"{}\"", key),
                        path: path.clone(),
                        code: "unknown-property",
                        location: loc,
                    });
                }
                AdditionalPolicy::Accept => {}
                AdditionalPolicy::Type(type_name) => {
                    let fake_prop_def =
                        MOTLYDataNode::with_eq(EqValue::Scalar(Scalar::String(type_name.clone())));
                    validate_property_value(pv, &fake_prop_def, types, path, errors, depth);
                }
                AdditionalPolicy::Inline(inline_constraint) => match pv {
                    MOTLYNode::Ref { .. } => {
                        errors.push(SchemaError {
                            message: "Expected a value but found a link".to_string(),
                            path: path.clone(),
                            code: "wrong-type",
                            location: None,
                        });
                    }
                    MOTLYNode::Data(node) => {
                        validate_constraint(node, inline_constraint, types, path, errors, depth + 1);
                    }
                },
            }
            path.pop();
        }
    }

    validate_exclusive_groups(required, optional, target_props, path, errors);
    validate_requires_deps(required, optional, target_props, path, errors);
}

fn validate_property_value(
    target_pv: &MOTLYNode,
    prop_def: &MOTLYDataNode,
    types: &TypesMap,
    path: &mut Vec<String>,
    errors: &mut Vec<SchemaError>,
    depth: usize,
) {
    let target_node = match target_pv {
        MOTLYNode::Ref { .. } => {
            errors.push(SchemaError {
                message: "Expected a value but found a link".to_string(),
                path: path.clone(),
                code: "wrong-type",
                location: None,
            });
            return;
        }
        MOTLYNode::Data(n) => n,
    };

    if let Some(type_name) = get_eq_string(prop_def) {
        validate_against_type_name(target_node, type_name, types, path, errors, depth);
        return;
    }

    validate_constraint(target_node, prop_def, types, path, errors, depth + 1);
}

fn validate_against_type_name(
    target: &MOTLYDataNode,
    type_name: &str,
    types: &TypesMap,
    path: &mut Vec<String>,
    errors: &mut Vec<SchemaError>,
    depth: usize,
) {
    if type_name.ends_with("[]") {
        let inner_type = &type_name[..type_name.len() - 2];
        validate_array_type(target, inner_type, types, path, errors, depth);
        return;
    }

    let type_def = match types.get(type_name) {
        Some(t) => t,
        None => {
            errors.push(SchemaError {
                message: format!("Unknown type \"{}\" in schema", type_name),
                path: path.clone(),
                code: "invalid-schema",
                location: target.location,
            });
            return;
        }
    };

    if let Some(EqValue::Array(arr)) = &type_def.eq {
        validate_one_of_array(target, arr, types, path, errors, depth);
        return;
    }

    validate_constraint(target, type_def, types, path, errors, depth + 1);
}

fn validate_array_type(
    target: &MOTLYDataNode,
    inner_type: &str,
    types: &TypesMap,
    path: &mut Vec<String>,
    errors: &mut Vec<SchemaError>,
    depth: usize,
) {
    let arr = match &target.eq {
        Some(EqValue::Array(a)) => a,
        _ => {
            errors.push(SchemaError {
                message: format!("Expected {}[], got {}", inner_type, describe_value(&target.eq)),
                path: path.clone(),
                code: "wrong-type",
                location: target.location,
            });
            return;
        }
    };

    for (i, elem_pv) in arr.iter().enumerate() {
        let elem_key = format!("[{}]", i);
        path.push(elem_key);
        match elem_pv {
            MOTLYNode::Ref { .. } => {
                errors.push(SchemaError {
                    message: format!("Expected {}, got reference", inner_type),
                    path: path.clone(),
                    code: "wrong-type",
                    location: None,
                });
            }
            MOTLYNode::Data(elem) => {
                validate_against_type_name(elem, inner_type, types, path, errors, depth);
            }
        }
        path.pop();
    }
}

fn validate_one_of_array(
    target: &MOTLYDataNode,
    type_refs: &[MOTLYNode],
    types: &TypesMap,
    path: &mut Vec<String>,
    errors: &mut Vec<SchemaError>,
    depth: usize,
) {
    let mut type_names = Vec::new();
    let mut best_errors = None;
    let mut best_branch = None;

    for ref_node in type_refs {
        let child_node = match ref_node {
            MOTLYNode::Data(n) => n,
            MOTLYNode::Ref { .. } => continue,
        };
        let name = match get_eq_string(child_node) {
            Some(s) => s,
            None => continue,
        };
        type_names.push(name);

        let mut trial_errors = Vec::new();
        validate_against_type_name(target, name, types, path, &mut trial_errors, depth);
        if trial_errors.is_empty() {
            return;
        }

        match &best_errors {
            None => {
                best_errors = Some(trial_errors);
                best_branch = Some(name);
            }
            Some(curr_best) => {
                if trial_errors.len() < curr_best.len() {
                    best_errors = Some(trial_errors);
                    best_branch = Some(name);
                }
            }
        }
    }

    let mut msg = format!("Value does not match any type in oneOf: [{}]", type_names.join(", "));
    if let Some(curr_best) = best_errors {
        if !curr_best.is_empty() && type_names.len() > 1 {
            if let Some(branch) = best_branch {
                let details = curr_best
                    .iter()
                    .map(|e| e.message.as_str())
                    .collect::<Vec<_>>()
                    .join("; ");
                msg.push_str(&format!(". Closest match \"{}\": {}", branch, details));
            }
        }
    }

    errors.push(SchemaError {
        message: msg,
        path: path.clone(),
        code: "wrong-type",
        location: target.location,
    });
}

fn validate_exclusive_groups(
    required: Option<&std::collections::BTreeMap<String, MOTLYNode>>,
    optional: Option<&std::collections::BTreeMap<String, MOTLYNode>>,
    target_props: Option<&std::collections::BTreeMap<String, MOTLYNode>>,
    path: &mut Vec<String>,
    errors: &mut Vec<SchemaError>,
) {
    let target_props = match target_props {
        Some(tp) => tp,
        None => return,
    };

    let mut groups: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();

    let mut collect = |prop_defs: Option<&std::collections::BTreeMap<String, MOTLYNode>>| {
        if let Some(defs) = prop_defs {
            for (key, pv) in defs {
                let node = match pv {
                    MOTLYNode::Data(n) => n,
                    MOTLYNode::Ref { .. } => continue,
                };
                if let Some(exclusive_node) = get_directive(node, "EXCLUSIVE") {
                    let mut group_names = Vec::new();
                    if let Some(val_str) = get_eq_string(exclusive_node) {
                        group_names.push(val_str.to_string());
                    } else if let Some(EqValue::Array(arr)) = &exclusive_node.eq {
                        for a in arr {
                            if let MOTLYNode::Data(child) = a {
                                if let Some(s) = get_eq_string(child) {
                                    group_names.push(s.to_string());
                                }
                            }
                        }
                    }
                    for g in group_names {
                        groups.entry(g).or_default().push(key.clone());
                    }
                }
            }
        }
    };

    collect(required);
    collect(optional);

    for (group, members) in groups {
        let present: Vec<String> = members
            .into_iter()
            .filter(|m| target_props.contains_key(m))
            .collect();
        if present.len() > 1 {
            errors.push(SchemaError {
                message: format!(
                    "Properties [{}] are mutually exclusive (group \"{}\")",
                    present.join(", "),
                    group
                ),
                path: path.clone(),
                code: "exclusive-violation",
                location: None,
            });
        }
    }
}

fn validate_requires_deps(
    required: Option<&std::collections::BTreeMap<String, MOTLYNode>>,
    optional: Option<&std::collections::BTreeMap<String, MOTLYNode>>,
    target_props: Option<&std::collections::BTreeMap<String, MOTLYNode>>,
    path: &mut Vec<String>,
    errors: &mut Vec<SchemaError>,
) {
    let target_props = match target_props {
        Some(tp) => tp,
        None => return,
    };

    let mut check = |prop_defs: Option<&std::collections::BTreeMap<String, MOTLYNode>>| {
        if let Some(defs) = prop_defs {
            for (key, pv) in defs {
                let node = match pv {
                    MOTLYNode::Data(n) => n,
                    MOTLYNode::Ref { .. } => continue,
                };
                if target_props.contains_key(key) {
                    if let Some(requires_node) = get_directive(node, "REQUIRES") {
                        if let Some(EqValue::Array(arr)) = &requires_node.eq {
                            for req in arr {
                                if let MOTLYNode::Data(child) = req {
                                    if let Some(req_name) = get_eq_string(child) {
                                        if !target_props.contains_key(req_name) {
                                            let mut err_path = path.clone();
                                            err_path.push(key.clone());
                                            errors.push(SchemaError {
                                                message: format!(
                                                    "Property \"{}\" requires \"{}\" to be present",
                                                    key, req_name
                                                ),
                                                path: err_path,
                                                code: "requires-violation",
                                                location: child.location,
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    };

    check(required);
    check(optional);
}
