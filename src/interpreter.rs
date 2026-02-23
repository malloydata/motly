use crate::ast::*;
use crate::error::{MOTLYError, Position};
use crate::tree::*;
use std::collections::BTreeMap;

/// Execute a list of parsed statements against an existing MOTLYNode,
/// mutating it in place and returning any non-fatal errors.
pub fn execute(statements: &[Statement], root: &mut MOTLYNode) -> Vec<MOTLYError> {
    let mut errors = Vec::new();
    for stmt in statements {
        execute_statement(stmt, root, &mut errors);
    }
    errors
}

fn execute_statement(stmt: &Statement, node: &mut MOTLYNode, errors: &mut Vec<MOTLYError>) {
    match stmt {
        Statement::SetEq {
            path,
            value,
            properties,
        } => execute_set_eq(node, path, value, properties.as_deref(), errors),
        Statement::AssignBoth {
            path,
            value,
            properties,
        } => execute_assign_both(node, path, value, properties.as_deref(), errors),
        Statement::ReplaceProperties { path, properties } => {
            execute_replace_properties(node, path, properties, errors)
        }
        Statement::UpdateProperties { path, properties } => {
            execute_update_properties(node, path, properties, errors)
        }
        Statement::Define { path, deleted } => execute_define(node, path, *deleted),
        Statement::ClearAll => {
            node.eq = None;
            node.properties = Some(BTreeMap::new());
        }
    }
}

/// `name = value` — set eq, preserve existing properties.
/// `name = value { props }` — set eq, then merge properties.
///
/// Special case: `name = $ref` inserts a MOTLYPropertyValue::Ref directly.
/// `name = $ref { props }` produces a non-fatal error (ref created, props ignored).
fn execute_set_eq(
    node: &mut MOTLYNode,
    path: &[String],
    value: &TagValue,
    properties: Option<&[Statement]>,
    errors: &mut Vec<MOTLYError>,
) {
    // Special case: reference value → insert as MOTLYPropertyValue::Ref
    if let TagValue::Scalar(ScalarValue::Reference { ups, path: ref_path }) = value {
        let ref_str = format_ref_string(*ups, ref_path);
        if properties.is_some() {
            let zero = Position {
                line: 0,
                column: 0,
                offset: 0,
            };
            errors.push(MOTLYError {
                code: "ref-with-properties".to_string(),
                message: "Cannot add properties to a reference. Did you mean := (clone)?"
                    .to_string(),
                begin: zero,
                end: zero,
            });
        }
        let (write_key, parent) = build_access_path(node, path);
        parent
            .get_or_create_properties()
            .insert(write_key, MOTLYPropertyValue::Ref(ref_str));
        return;
    }

    let (write_key, parent) = build_access_path(node, path);
    let props = parent.get_or_create_properties();

    // Get or create target (preserves existing node and its properties)
    let target_pv = props
        .entry(write_key)
        .or_insert_with(MOTLYPropertyValue::new_node);
    let target = target_pv.ensure_node();

    // Set the value slot
    set_eq_slot(target, value, errors);

    // If properties block present, MERGE them
    if let Some(prop_stmts) = properties {
        for s in prop_stmts {
            execute_statement(s, target, errors);
        }
    }
}

/// `name := value` — assign value + clear properties.
/// `name := value { props }` — assign value + replace properties.
/// `name := $ref` — clone the referenced subtree.
/// `name := $ref { props }` — clone + replace properties.
fn execute_assign_both(
    node: &mut MOTLYNode,
    path: &[String],
    value: &TagValue,
    properties: Option<&[Statement]>,
    errors: &mut Vec<MOTLYError>,
) {
    if let TagValue::Scalar(ScalarValue::Reference {
        ups,
        path: ref_path,
    }) = value
    {
        // CLONE semantics: resolve + deep copy the target
        let cloned = resolve_and_clone(node, path, *ups, ref_path);
        match cloned {
            Ok(mut cloned) => {
                // Check for relative references that escape the clone boundary
                sanitize_cloned_refs(&mut cloned, 0, errors);
                if let Some(prop_stmts) = properties {
                    cloned.properties = Some(BTreeMap::new());
                    for s in prop_stmts {
                        execute_statement(s, &mut cloned, errors);
                    }
                }
                let (write_key, parent) = build_access_path(node, path);
                parent
                    .get_or_create_properties()
                    .insert(write_key, MOTLYPropertyValue::Node(cloned));
            }
            Err(err) => {
                // Fatal clone error — push it and don't create the node
                errors.push(err);
            }
        }
    } else {
        // Literal value: create fresh node (replaces everything)
        let mut result = MOTLYNode::new();
        set_eq_slot(&mut result, value, errors);
        if let Some(prop_stmts) = properties {
            for s in prop_stmts {
                execute_statement(s, &mut result, errors);
            }
        }
        let (write_key, parent) = build_access_path(node, path);
        parent
            .get_or_create_properties()
            .insert(write_key, MOTLYPropertyValue::Node(result));
    }
}

/// `name: { props }` — preserve existing value, replace properties.
fn execute_replace_properties(
    node: &mut MOTLYNode,
    path: &[String],
    properties: &[Statement],
    errors: &mut Vec<MOTLYError>,
) {
    let (write_key, parent) = build_access_path(node, path);

    let mut result = MOTLYNode::new();

    // Always preserve the existing value (if it's a node)
    let parent_props = parent.get_or_create_properties();
    if let Some(existing_pv) = parent_props.get(&write_key) {
        if let MOTLYPropertyValue::Node(existing) = existing_pv {
            result.eq = existing.eq.clone();
        }
        // If it was a Ref, we're replacing it with a node (no eq to preserve)
    }

    for stmt in properties {
        execute_statement(stmt, &mut result, errors);
    }

    parent_props.insert(write_key, MOTLYPropertyValue::Node(result));
}

fn execute_update_properties(
    node: &mut MOTLYNode,
    path: &[String],
    properties: &[Statement],
    errors: &mut Vec<MOTLYError>,
) {
    let (write_key, parent) = build_access_path(node, path);

    let props = parent.get_or_create_properties();

    // Get or create the target node (merging semantics - preserves existing)
    let target_pv = props
        .entry(write_key)
        .or_insert_with(MOTLYPropertyValue::new_node);
    let target = target_pv.ensure_node();

    for stmt in properties {
        execute_statement(stmt, target, errors);
    }
}

fn execute_define(node: &mut MOTLYNode, path: &[String], deleted: bool) {
    let (write_key, parent) = build_access_path(node, path);

    if deleted {
        parent
            .get_or_create_properties()
            .insert(write_key, MOTLYPropertyValue::Node(MOTLYNode::deleted()));
    } else {
        // Get-or-create: if node already exists, leave it alone
        parent
            .get_or_create_properties()
            .entry(write_key)
            .or_insert_with(MOTLYPropertyValue::new_node);
    }
}

/// Navigate to the parent of the final path segment, creating intermediate
/// nodes as needed. Returns (final_key, parent_node).
fn build_access_path<'a>(
    node: &'a mut MOTLYNode,
    path: &[String],
) -> (String, &'a mut MOTLYNode) {
    assert!(!path.is_empty(), "path must not be empty");

    let mut current = node;

    for segment in &path[..path.len() - 1] {
        let props = current.get_or_create_properties();

        let entry = props
            .entry(segment.clone())
            .or_insert_with(MOTLYPropertyValue::new_node);

        current = entry.ensure_node();
    }

    (path.last().unwrap().clone(), current)
}

/// Set the eq slot on a target node from a TagValue.
/// References are NOT handled here — they become MOTLYPropertyValue::Ref
/// at the caller level.
fn set_eq_slot(target: &mut MOTLYNode, value: &TagValue, errors: &mut Vec<MOTLYError>) {
    match value {
        TagValue::Array(elements) => {
            target.eq = Some(EqValue::Array(resolve_array(elements, errors)));
        }
        TagValue::Scalar(sv) => match sv {
            ScalarValue::String(s) => {
                target.eq = Some(EqValue::Scalar(Scalar::String(s.clone())));
            }
            ScalarValue::Number(n) => {
                target.eq = Some(EqValue::Scalar(Scalar::Number(*n)));
            }
            ScalarValue::Boolean(b) => {
                target.eq = Some(EqValue::Scalar(Scalar::Boolean(*b)));
            }
            ScalarValue::Date(d) => {
                target.eq = Some(EqValue::Scalar(Scalar::Date(d.clone())));
            }
            ScalarValue::Reference { .. } => {
                // References are handled by the caller (execute_set_eq / resolve_array_element).
                // They become MOTLYPropertyValue::Ref, not part of the eq slot.
                unreachable!("References should be handled before calling set_eq_slot");
            }
            ScalarValue::Env { name } => {
                target.eq = Some(EqValue::EnvRef(name.clone()));
            }
            ScalarValue::None => {
                target.eq = None;
            }
        },
    }
}

/// Resolve an array of AST elements to MOTLYPropertyValues.
fn resolve_array(
    elements: &[ArrayElement],
    errors: &mut Vec<MOTLYError>,
) -> Vec<MOTLYPropertyValue> {
    elements
        .iter()
        .map(|el| resolve_array_element(el, errors))
        .collect()
}

fn resolve_array_element(
    el: &ArrayElement,
    errors: &mut Vec<MOTLYError>,
) -> MOTLYPropertyValue {
    // Check if the element value is a reference → becomes MOTLYPropertyValue::Ref
    if let Some(TagValue::Scalar(ScalarValue::Reference { ups, path })) = &el.value {
        let ref_str = format_ref_string(*ups, path);
        if el.properties.is_some() {
            let zero = Position {
                line: 0,
                column: 0,
                offset: 0,
            };
            errors.push(MOTLYError {
                code: "ref-with-properties".to_string(),
                message: "Cannot add properties to a reference. Did you mean := (clone)?"
                    .to_string(),
                begin: zero,
                end: zero,
            });
        }
        return MOTLYPropertyValue::Ref(ref_str);
    }

    let mut node = MOTLYNode::new();

    if let Some(ref value) = el.value {
        set_eq_slot(&mut node, value, errors);
    }

    if let Some(ref prop_stmts) = el.properties {
        for stmt in prop_stmts {
            execute_statement(stmt, &mut node, errors);
        }
    }

    MOTLYPropertyValue::Node(node)
}

/// Format a reference path back to its string form: `$^^name[0].sub`
fn format_ref_string(ups: usize, path: &[RefPathSegment]) -> String {
    let mut s = String::from("$");
    for _ in 0..ups {
        s.push('^');
    }
    let mut first = true;
    for seg in path {
        match seg {
            RefPathSegment::Name(name) => {
                if !first {
                    s.push('.');
                }
                s.push_str(name);
                first = false;
            }
            RefPathSegment::Index(idx) => {
                s.push('[');
                s.push_str(&idx.to_string());
                s.push(']');
            }
        }
    }
    s
}

/// Resolve a reference path in the tree and return a deep clone.
fn resolve_and_clone(
    root: &MOTLYNode,
    stmt_path: &[String],
    ups: usize,
    ref_path: &[RefPathSegment],
) -> Result<MOTLYNode, MOTLYError> {
    let ref_str = format_ref_string(ups, ref_path);

    let start: &MOTLYNode;

    if ups == 0 {
        // Absolute reference: start at root
        start = root;
    } else {
        // Relative reference: go up from the current context.
        // stmtPath is the full write path (including the key being assigned to).
        // Current context = parent of write target = stmtPath[0..len-2].
        // Going up `ups` levels: stmtPath[0..len-2-ups].
        let context_len = stmt_path.len().checked_sub(1 + ups);
        match context_len {
            Some(len) => {
                let mut current = root;
                for i in 0..len {
                    match current
                        .properties
                        .as_ref()
                        .and_then(|p| p.get(&stmt_path[i]))
                    {
                        Some(MOTLYPropertyValue::Node(child)) => current = child,
                        Some(MOTLYPropertyValue::Ref(_)) => {
                            return Err(clone_error(format!(
                                "Clone reference {} could not be resolved: path segment \"{}\" is a link reference",
                                ref_str, stmt_path[i]
                            )));
                        }
                        None => {
                            return Err(clone_error(format!(
                                "Clone reference {} could not be resolved: path segment \"{}\" not found",
                                ref_str, stmt_path[i]
                            )));
                        }
                    }
                }
                start = current;
            }
            None => {
                return Err(clone_error(format!(
                    "Clone reference {} goes {} level(s) up but only {} ancestor(s) available",
                    ref_str,
                    ups,
                    stmt_path.len().saturating_sub(1)
                )));
            }
        }
    }

    // Follow refPath segments
    let mut current = start;
    for seg in ref_path {
        match seg {
            RefPathSegment::Name(name) => {
                match current
                    .properties
                    .as_ref()
                    .and_then(|p| p.get(name.as_str()))
                {
                    Some(MOTLYPropertyValue::Node(child)) => current = child,
                    Some(MOTLYPropertyValue::Ref(_)) => {
                        return Err(clone_error(format!(
                            "Clone reference {} could not be resolved: property \"{}\" is a link reference",
                            ref_str, name
                        )));
                    }
                    None => {
                        return Err(clone_error(format!(
                            "Clone reference {} could not be resolved: property \"{}\" not found",
                            ref_str, name
                        )));
                    }
                }
            }
            RefPathSegment::Index(idx) => {
                match &current.eq {
                    Some(EqValue::Array(arr)) => {
                        if *idx >= arr.len() {
                            return Err(clone_error(format!(
                                "Clone reference {} could not be resolved: index [{}] out of bounds (array length {})",
                                ref_str, idx, arr.len()
                            )));
                        }
                        match &arr[*idx] {
                            MOTLYPropertyValue::Node(child) => current = child,
                            MOTLYPropertyValue::Ref(_) => {
                                return Err(clone_error(format!(
                                    "Clone reference {} could not be resolved: index [{}] is a link reference",
                                    ref_str, idx
                                )));
                            }
                        }
                    }
                    _ => {
                        return Err(clone_error(format!(
                            "Clone reference {} could not be resolved: index [{}] used on non-array",
                            ref_str, idx
                        )));
                    }
                }
            }
        }
    }

    Ok(current.clone())
}

fn clone_error(message: String) -> MOTLYError {
    let zero = Position {
        line: 0,
        column: 0,
        offset: 0,
    };
    MOTLYError {
        code: "unresolved-clone-reference".to_string(),
        message,
        begin: zero,
        end: zero,
    }
}

/// Walk a cloned subtree and null out any relative (^) references that
/// escape the clone boundary. A reference at depth D with N ups escapes
/// if N > D. Absolute references (ups=0) are left alone.
fn sanitize_cloned_refs(node: &mut MOTLYNode, depth: usize, errors: &mut Vec<MOTLYError>) {
    if let Some(EqValue::Array(ref mut arr)) = node.eq {
        for elem in arr.iter_mut() {
            sanitize_cloned_pv(elem, depth + 1, errors);
        }
    }

    if let Some(ref mut props) = node.properties {
        for (_key, child) in props.iter_mut() {
            sanitize_cloned_pv(child, depth + 1, errors);
        }
    }
}

/// Sanitize a single MOTLYPropertyValue within a cloned subtree.
fn sanitize_cloned_pv(
    pv: &mut MOTLYPropertyValue,
    depth: usize,
    errors: &mut Vec<MOTLYError>,
) {
    match pv {
        MOTLYPropertyValue::Ref(ref link_to) => {
            let parsed_ups = parse_ref_ups(link_to);
            if parsed_ups > 0 && parsed_ups > depth {
                let zero = Position {
                    line: 0,
                    column: 0,
                    offset: 0,
                };
                errors.push(MOTLYError {
                    code: "clone-reference-out-of-scope".to_string(),
                    message: format!(
                        "Cloned reference \"{}\" escapes the clone boundary ({} level(s) up from depth {})",
                        link_to, parsed_ups, depth
                    ),
                    begin: zero,
                    end: zero,
                });
                // Convert to empty node (equivalent to old node.eq = None)
                *pv = MOTLYPropertyValue::Node(MOTLYNode::new());
            }
        }
        MOTLYPropertyValue::Node(node) => {
            sanitize_cloned_refs(node, depth, errors);
        }
    }
}

/// Extract the ups count from a linkTo string like "$^^name".
fn parse_ref_ups(link_to: &str) -> usize {
    let mut chars = link_to.chars();
    if chars.next() != Some('$') {
        return 0;
    }
    let mut ups = 0;
    for ch in chars {
        if ch == '^' {
            ups += 1;
        } else {
            break;
        }
    }
    ups
}
