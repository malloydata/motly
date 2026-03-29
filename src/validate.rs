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

// ── Schema validation (stub) ────────────────────────────────────────
//
// Schema validation is not yet implemented in the Rust engine.
// See docs/schema_spec.md for the spec and the TypeScript implementation
// in bindings/typescript/parser/src/validate.ts.

/// Validate a MOTLY tree against a schema (also a MOTLY tree).
/// Currently a no-op stub — returns an empty error list.
pub fn validate_schema(_target: &MOTLYDataNode, _schema: &MOTLYDataNode) -> Vec<SchemaError> {
    Vec::new()
}
