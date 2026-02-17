use crate::ast::*;
use crate::tree::*;
use std::collections::BTreeMap;

/// Execute a list of parsed statements against an existing MOTLYValue,
/// returning the updated value.
pub fn execute(statements: &[Statement], mut root: MOTLYValue) -> MOTLYValue {
    for stmt in statements {
        execute_statement(stmt, &mut root);
    }
    root
}

fn execute_statement(stmt: &Statement, node: &mut MOTLYValue) {
    match stmt {
        Statement::SetEq {
            path,
            value,
            properties,
            preserve_properties,
        } => execute_set_eq(
            node,
            path,
            value,
            properties.as_deref(),
            *preserve_properties,
        ),
        Statement::ReplaceProperties {
            path,
            properties,
            preserve_value,
        } => execute_replace_properties(node, path, properties, *preserve_value),
        Statement::UpdateProperties { path, properties } => {
            execute_update_properties(node, path, properties)
        }
        Statement::Define { path, deleted } => execute_define(node, path, *deleted),
        Statement::ClearAll => {
            node.properties = Some(BTreeMap::new());
        }
    }
}

fn execute_set_eq(
    node: &mut MOTLYValue,
    path: &[String],
    value: &TagValue,
    properties: Option<&[Statement]>,
    preserve_properties: bool,
) {
    // Check if value is a reference (should produce a Link)
    if let TagValue::Scalar(ScalarValue::Reference {
        ups,
        path: ref_path,
    }) = value
    {
        if properties.is_none() && !preserve_properties {
            // Simple reference assignment -> produce a Link
            let (write_key, parent) = build_access_path(node, path);
            parent.get_or_create_properties().insert(
                write_key.to_string(),
                MOTLYNode::Ref(MOTLYRef {
                    link_to: format_ref_string(*ups, ref_path),
                }),
            );
            return;
        }
    }

    let (write_key, parent) = build_access_path(node, path);

    if let Some(prop_stmts) = properties {
        // name = value { new_properties } - set value and replace properties
        let mut result = create_value_node(value);
        for stmt in prop_stmts {
            execute_statement(stmt, &mut result);
        }
        parent
            .get_or_create_properties()
            .insert(write_key.to_string(), MOTLYNode::Value(result));
    } else if preserve_properties {
        // name = value { ... } - update value, preserve existing properties
        let props = parent.get_or_create_properties();
        let existing = props.get(&write_key);

        if let Some(MOTLYNode::Value(existing_node)) = existing {
            let mut result = create_value_node(value);
            // Preserve existing properties
            if let Some(ref existing_props) = existing_node.properties {
                result.properties = Some(existing_props.clone());
            }
            props.insert(write_key.to_string(), MOTLYNode::Value(result));
        } else {
            // No existing node, just create with value
            let result = create_value_node(value);
            props.insert(write_key.to_string(), MOTLYNode::Value(result));
        }
    } else {
        // name = value - simple assignment (replaces everything)
        let result = create_value_node(value);
        parent
            .get_or_create_properties()
            .insert(write_key.to_string(), MOTLYNode::Value(result));
    }
}

fn execute_replace_properties(
    node: &mut MOTLYValue,
    path: &[String],
    properties: &[Statement],
    preserve_value: bool,
) {
    let (write_key, parent) = build_access_path(node, path);

    let mut result = MOTLYValue::new();

    if preserve_value {
        // name = ... { properties } - preserve value, replace properties
        let props = parent.get_or_create_properties();
        if let Some(MOTLYNode::Value(existing)) = props.get(&write_key) {
            result.eq = existing.eq.clone();
        }
    }

    for stmt in properties {
        execute_statement(stmt, &mut result);
    }

    parent
        .get_or_create_properties()
        .insert(write_key.to_string(), MOTLYNode::Value(result));
}

fn execute_update_properties(node: &mut MOTLYValue, path: &[String], properties: &[Statement]) {
    let (write_key, parent) = build_access_path(node, path);

    let props = parent.get_or_create_properties();

    // Get or create the target node (merging semantics - preserves existing)
    let target = props
        .entry(write_key.to_string())
        .or_insert_with(|| MOTLYNode::Value(MOTLYValue::new()));

    match target {
        MOTLYNode::Value(ref mut target_node) => {
            for stmt in properties {
                execute_statement(stmt, target_node);
            }
        }
        MOTLYNode::Ref(_) => {
            // If it's a link, replace with a new node
            let mut new_node = MOTLYValue::new();
            for stmt in properties {
                execute_statement(stmt, &mut new_node);
            }
            *target = MOTLYNode::Value(new_node);
        }
    }
}

fn execute_define(node: &mut MOTLYValue, path: &[String], deleted: bool) {
    let (write_key, parent) = build_access_path(node, path);

    if deleted {
        parent.get_or_create_properties().insert(
            write_key.to_string(),
            MOTLYNode::Value(MOTLYValue::deleted()),
        );
    } else {
        parent
            .get_or_create_properties()
            .insert(write_key.to_string(), MOTLYNode::Value(MOTLYValue::new()));
    }
}

/// Navigate to the parent of the final path segment, creating intermediate
/// nodes as needed. Returns (final_key, parent_node).
fn build_access_path<'a>(
    node: &'a mut MOTLYValue,
    path: &[String],
) -> (String, &'a mut MOTLYValue) {
    assert!(!path.is_empty(), "path must not be empty");

    let mut current = node;

    for segment in &path[..path.len() - 1] {
        let props = current.get_or_create_properties();

        // Ensure intermediate node exists
        let entry = props
            .entry(segment.clone())
            .or_insert_with(|| MOTLYNode::Value(MOTLYValue::new()));

        current = match entry {
            MOTLYNode::Value(ref mut n) => n,
            MOTLYNode::Ref(_) => {
                // Replace link with a new node for intermediate path
                *entry = MOTLYNode::Value(MOTLYValue::new());
                match entry {
                    MOTLYNode::Value(ref mut n) => n,
                    _ => unreachable!(),
                }
            }
        };
    }

    (path.last().unwrap().clone(), current)
}

/// Convert an AST TagValue to a MOTLYValue.
/// Note: References are handled at a higher level (execute_set_eq).
fn create_value_node(value: &TagValue) -> MOTLYValue {
    match value {
        TagValue::Scalar(scalar) => match scalar {
            ScalarValue::String(s) => {
                MOTLYValue::with_eq(EqValue::Scalar(Scalar::String(s.clone())))
            }
            ScalarValue::Number(n) => MOTLYValue::with_eq(EqValue::Scalar(Scalar::Number(*n))),
            ScalarValue::Boolean(b) => MOTLYValue::with_eq(EqValue::Scalar(Scalar::Boolean(*b))),
            ScalarValue::Date(d) => MOTLYValue::with_eq(EqValue::Scalar(Scalar::Date(d.clone()))),
            ScalarValue::Reference { .. } => {
                // Should not be reached for simple ref assignments (handled above).
                // For ref + properties case, TS ignores the ref value.
                MOTLYValue::new()
            }
        },
        TagValue::Array(elements) => {
            let arr = resolve_array(elements);
            MOTLYValue::with_eq(EqValue::Array(arr))
        }
    }
}

/// Resolve an array of AST elements to MOTLYNodes.
fn resolve_array(elements: &[ArrayElement]) -> Vec<MOTLYNode> {
    elements.iter().map(resolve_array_element).collect()
}

fn resolve_array_element(el: &ArrayElement) -> MOTLYNode {
    // Reference without properties becomes a link
    if let Some(TagValue::Scalar(ScalarValue::Reference { ups, path })) = &el.value {
        if el.properties.is_none() {
            return MOTLYNode::Ref(MOTLYRef {
                link_to: format_ref_string(*ups, path),
            });
        }
    }

    let mut node = MOTLYValue::new();

    if let Some(ref value) = el.value {
        match value {
            TagValue::Array(nested_elements) => {
                node.eq = Some(EqValue::Array(resolve_array(nested_elements)));
            }
            TagValue::Scalar(ScalarValue::Reference { .. }) => {
                // Reference with properties: ignore the reference value,
                // just keep the properties (matching TS behavior)
            }
            TagValue::Scalar(ScalarValue::String(s)) => {
                node.eq = Some(EqValue::Scalar(Scalar::String(s.clone())));
            }
            TagValue::Scalar(ScalarValue::Number(n)) => {
                node.eq = Some(EqValue::Scalar(Scalar::Number(*n)));
            }
            TagValue::Scalar(ScalarValue::Boolean(b)) => {
                node.eq = Some(EqValue::Scalar(Scalar::Boolean(*b)));
            }
            TagValue::Scalar(ScalarValue::Date(d)) => {
                node.eq = Some(EqValue::Scalar(Scalar::Date(d.clone())));
            }
        }
    }

    if let Some(ref prop_stmts) = el.properties {
        for stmt in prop_stmts {
            execute_statement(stmt, &mut node);
        }
    }

    MOTLYNode::Value(node)
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
