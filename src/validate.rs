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
                MOTLYPropertyValue::Ref { ref link_to, link_ups } => {
                    // This property is a reference — check it
                    if let Some(err_msg) = check_link(link_to, *link_ups, ancestors, root) {
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
            MOTLYPropertyValue::Ref { ref link_to, link_ups } => {
                if let Some(err_msg) = check_link(link_to, *link_ups, ancestors, root) {
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
fn check_link(segments: &[RefSegment], ups: usize, ancestors: &[&MOTLYNode], root: &MOTLYNode) -> Option<String> {
    let link_str = format_ref_display(ups, segments);

    // Determine the start node for resolution.
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

/// Follow path segments from a start node. Returns Some(error) if unresolved.
fn resolve_path(start: &MOTLYNode, segments: &[RefSegment], link_str: &str) -> Option<String> {
    let mut current: ResolveTarget = ResolveTarget::Node(start);

    for seg in segments {
        match (seg, current) {
            (RefSegment::Name(name), ResolveTarget::Node(node)) => {
                match &node.properties {
                    Some(props) => match props.get(name.as_str()) {
                        Some(MOTLYPropertyValue::Ref { .. }) => {
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
            (RefSegment::Index(idx), ResolveTarget::Node(node)) => match &node.eq {
                Some(EqValue::Array(arr)) => {
                    if *idx >= arr.len() {
                        return Some(format!(
                                "Reference \"{}\" could not be resolved: index [{}] out of bounds (array length {})",
                                link_str, idx, arr.len()
                            ));
                    }
                    match &arr[*idx] {
                        MOTLYPropertyValue::Ref { .. } => {
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
    Node(&'a MOTLYNode),
    Terminal,
}

// ── Schema validation ───────────────────────────────────────────────

/// Validate a MOTLY tree against a schema (also a MOTLY tree).
///
/// TODO: Re-implement with new ALL-CAPS schema language (see docs/schema_spec.md).
/// Currently a no-op that always returns no errors.
pub fn validate_schema(_tag: &MOTLYNode, _schema: &MOTLYNode) -> Vec<SchemaError> {
    Vec::new()
}
