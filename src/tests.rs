use crate::tree::*;
use crate::validate::{validate_references, validate_schema};

/// Strip all location fields from a MOTLYDataNode tree (for fixture comparison).
fn strip_locations(node: &mut MOTLYDataNode) {
    node.location = None;
    if let Some(EqValue::Array(ref mut arr)) = node.eq {
        for pv in arr.iter_mut() {
            if let MOTLYNode::Data(ref mut child) = pv {
                strip_locations(child);
            }
        }
    }
    if let Some(ref mut props) = node.properties {
        for pv in props.values_mut() {
            if let MOTLYNode::Data(ref mut child) = pv {
                strip_locations(child);
            }
        }
    }
}

// ── Shared fixture runners ──────────────────────────────────────────

/// Embed fixture files at compile time.
const PARSE_FIXTURES: &str = include_str!("../test-data/fixtures/parse.json");
const PARSE_ERROR_FIXTURES: &str = include_str!("../test-data/fixtures/parse-errors.json");
const SCHEMA_FIXTURES: &str = include_str!("../test-data/fixtures/schema.json");
const REF_FIXTURES: &str = include_str!("../test-data/fixtures/refs.json");
const SESSION_FIXTURES: &str = include_str!("../test-data/fixtures/session.json");

/// Convert a serde_json::Value (fixture "expected" format) to a MOTLYDataNode.
/// Uses from_wire to handle {"$date": "..."} in expected values.
fn fixture_expected_to_value(expected: &serde_json::Value) -> MOTLYDataNode {
    let json_str = serde_json::to_string(expected).unwrap();
    crate::from_json::from_wire(&json_str).unwrap()
}

#[test]
fn test_fixture_parse() {
    use crate::interpreter::SessionOptions;
    let fixtures: Vec<serde_json::Value> = serde_json::from_str(PARSE_FIXTURES).unwrap();

    for fixture in &fixtures {
        let name = fixture["name"].as_str().unwrap();
        let input = &fixture["input"];
        let expected = fixture.get("expected");
        let expect_errors = fixture
            .get("expectErrors")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let inputs: Vec<String> = if let Some(input_str) = input.as_str() {
            vec![input_str.to_string()]
        } else if let Some(input_arr) = input.as_array() {
            input_arr.iter().map(|v| v.as_str().unwrap().to_string()).collect()
        } else {
            panic!("Fixture '{}': input must be a string or array", name);
        };

        let input_refs: Vec<&str> = inputs.iter().map(|s| s.as_str()).collect();
        // Include reference validation (matches TS MOTLYSession.finish() behavior)
        let (value, errors) = crate::session_finish_ex(&input_refs, SessionOptions::default(), true);

        if expect_errors {
            assert!(
                !errors.is_empty(),
                "Fixture '{}': expected errors but got none",
                name
            );
            if expected.is_none() || expected == Some(&serde_json::Value::Null) {
                continue;
            }
            // expectErrors + expected: errors are non-fatal, check the tree too
        }
        // Note: when expectErrors is false, we do NOT assert errors.is_empty().
        // This matches the TS test runner which only checks the value tree,
        // allowing semantic errors (e.g. unresolved refs in parse-only fixtures).

        if let Some(expected) = expected {
            if !expected.is_null() {
                let expected_value = fixture_expected_to_value(expected);
                let mut stripped = value.clone();
                strip_locations(&mut stripped);
                assert_eq!(
                    stripped, expected_value,
                    "Fixture '{}': value mismatch\n  Got:      {:?}\n  Expected: {:?}",
                    name, stripped, expected_value
                );
            }
        }
    }
}

#[test]
fn test_fixture_parse_errors() {
    let fixtures: Vec<serde_json::Value> = serde_json::from_str(PARSE_ERROR_FIXTURES).unwrap();

    for fixture in &fixtures {
        let name = fixture["name"].as_str().unwrap();
        let input = fixture["input"].as_str().unwrap();

        let result = crate::parse_motly_0(input, MOTLYDataNode::new());
        assert!(
            !result.errors.is_empty(),
            "Fixture '{}': expected parse errors for input '{}'",
            name,
            input
        );
    }
}

#[test]
#[ignore = "schema validation not yet implemented in Rust"]
fn test_fixture_schema() {
    let fixtures: Vec<serde_json::Value> = serde_json::from_str(SCHEMA_FIXTURES).unwrap();

    for fixture in &fixtures {
        let name = fixture["name"].as_str().unwrap();
        let schema_input = fixture["schema"].as_str().unwrap();
        let tag_input = fixture["input"].as_str().unwrap();
        let expected_errors = fixture["expectedErrors"].as_array().unwrap();

        let schema = crate::parse_motly_0(schema_input, MOTLYDataNode::new());
        assert!(
            schema.errors.is_empty(),
            "Fixture '{}': schema parse errors: {:?}",
            name,
            schema.errors
        );

        let tag = crate::parse_motly_0(tag_input, MOTLYDataNode::new());
        assert!(
            tag.errors.is_empty(),
            "Fixture '{}': tag parse errors: {:?}",
            name,
            tag.errors
        );

        let errors = validate_schema(&tag.value, &schema.value);

        assert_eq!(
            errors.len(),
            expected_errors.len(),
            "Fixture '{}': error count mismatch — got {} [{}], expected {}",
            name,
            errors.len(),
            errors
                .iter()
                .map(|e| format!("{} at /{}", e.code, e.path.join("/")))
                .collect::<Vec<_>>()
                .join(", "),
            expected_errors.len()
        );

        // Sort both by (code, path) for stable comparison
        let mut actual: Vec<_> = errors
            .iter()
            .map(|e| (e.code.to_string(), e.path.clone()))
            .collect();
        actual.sort();
        let mut expected: Vec<_> = expected_errors
            .iter()
            .map(|e| {
                let code = e["code"].as_str().unwrap().to_string();
                let path: Vec<String> = e
                    .get("path")
                    .and_then(|p| p.as_array())
                    .map(|arr| {
                        arr.iter()
                            .map(|v| v.as_str().unwrap().to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                (code, path)
            })
            .collect();
        expected.sort();

        for (i, (actual_entry, expected_entry)) in actual.iter().zip(expected.iter()).enumerate() {
            assert_eq!(
                actual_entry.0, expected_entry.0,
                "Fixture '{}': error code mismatch at sorted index {}: got '{}', expected '{}'",
                name, i, actual_entry.0, expected_entry.0
            );
            if !expected_entry.1.is_empty() {
                assert_eq!(
                    actual_entry.1, expected_entry.1,
                    "Fixture '{}': error path mismatch at sorted index {} (code '{}'): got {:?}, expected {:?}",
                    name, i, actual_entry.0, actual_entry.1, expected_entry.1
                );
            }
        }
    }
}

#[test]
fn test_fixture_refs() {
    use crate::validate::validate_references;
    let fixtures: Vec<serde_json::Value> = serde_json::from_str(REF_FIXTURES).unwrap();

    for fixture in &fixtures {
        let name = fixture["name"].as_str().unwrap();
        let input = fixture["input"].as_str().unwrap();
        let expected_errors = fixture["expectedErrors"].as_array().unwrap();

        let result = crate::parse_motly_0(input, MOTLYDataNode::new());
        assert!(
            result.errors.is_empty(),
            "Fixture '{}': parse errors: {:?}",
            name,
            result.errors
        );

        let errors = validate_references(&result.value);

        assert_eq!(
            errors.len(),
            expected_errors.len(),
            "Fixture '{}': error count mismatch — got {}, expected {}",
            name,
            errors.len(),
            expected_errors.len()
        );

        // Sort both by (code, path) for stable comparison
        let mut actual: Vec<_> = errors
            .iter()
            .map(|e| (e.code.to_string(), e.path.clone()))
            .collect();
        actual.sort();
        let mut expected: Vec<_> = expected_errors
            .iter()
            .map(|e| {
                let code = e["code"].as_str().unwrap().to_string();
                let path: Vec<String> = e
                    .get("path")
                    .and_then(|p| p.as_array())
                    .map(|arr| {
                        arr.iter()
                            .map(|v| v.as_str().unwrap().to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                (code, path)
            })
            .collect();
        expected.sort();

        for (i, (actual_entry, expected_entry)) in actual.iter().zip(expected.iter()).enumerate() {
            assert_eq!(
                actual_entry.0, expected_entry.0,
                "Fixture '{}': error code mismatch at sorted index {}",
                name, i
            );
            if !expected_entry.1.is_empty() {
                assert_eq!(
                    actual_entry.1, expected_entry.1,
                    "Fixture '{}': error path mismatch at sorted index {} (code '{}')",
                    name, i, actual_entry.0
                );
            }
        }
    }
}

#[test]
fn test_fixture_session() {
    use crate::interpreter::SessionOptions;
    let fixtures: Vec<serde_json::Value> = serde_json::from_str(SESSION_FIXTURES).unwrap();

    for fixture in &fixtures {
        let name = fixture["name"].as_str().unwrap();
        let steps = fixture["steps"].as_array().unwrap();

        // Accumulate phase: collect parse inputs, track schema, and parse errors
        let mut inputs: Vec<String> = Vec::new();
        let mut _schema: Option<MOTLYDataNode> = None;
        let mut value: Option<MOTLYDataNode> = None;

        for step in steps {
            let action = step["action"].as_str().unwrap();

            match action {
                "parse" => {
                    let input = step["input"].as_str().unwrap();
                    // Check for syntax errors
                    match crate::parser::parse(input) {
                        Ok(_stmts) => {
                            inputs.push(input.to_string());
                        }
                        Err(err) => {
                            if step.get("expectErrors").and_then(|v| v.as_bool()).unwrap_or(false) {
                                // Good — syntax errors expected
                            } else {
                                panic!(
                                    "Fixture '{}': unexpected parse errors for '{}': {:?}",
                                    name, input, err
                                );
                            }
                        }
                    }
                }
                "parseSchema" => {
                    let input = step["input"].as_str().unwrap();
                    let result = crate::parse_motly_0(input, MOTLYDataNode::new());
                    assert!(
                        result.errors.is_empty(),
                        "Fixture '{}': schema parse errors: {:?}",
                        name,
                        result.errors
                    );
                    _schema = Some(result.value);
                }
                "finish" => {
                    let input_refs: Vec<&str> = inputs.iter().map(|s| s.as_str()).collect();
                    let (root, errors) = crate::session_finish(&input_refs, SessionOptions::default());
                    value = Some(root);

                    if let Some(expected_errors) = step.get("expectedErrors").and_then(|v| v.as_array()) {
                        let expected_codes: Vec<&str> = {
                            let mut codes: Vec<&str> = expected_errors.iter()
                                .map(|e| e["code"].as_str().unwrap())
                                .collect();
                            codes.sort();
                            codes
                        };
                        let mut actual_codes: Vec<&str> = errors.iter()
                            .map(|e| e.code.as_str())
                            .collect();
                        actual_codes.sort();
                        assert_eq!(
                            actual_codes, expected_codes,
                            "Fixture '{}' (finish): error codes mismatch — got {:?}, expected {:?}",
                            name, actual_codes, expected_codes
                        );
                    } else {
                        assert!(
                            errors.is_empty(),
                            "Fixture '{}' (finish): unexpected errors: {:?}",
                            name, errors
                        );
                    }
                }
                "getValue" => {
                    assert!(value.is_some(), "Fixture '{}': getValue called before finish", name);
                    if let Some(expected) = step.get("expected") {
                        let expected_value = fixture_expected_to_value(expected);
                        let mut stripped = value.as_ref().unwrap().clone();
                        strip_locations(&mut stripped);
                        assert_eq!(
                            stripped, expected_value,
                            "Fixture '{}' (getValue): value mismatch",
                            name
                        );
                    }
                }
                "validateSchema" => {
                    // Schema validation not yet implemented in Rust — skip these steps
                }
                other => panic!("Fixture '{}': unknown action '{}'", name, other),
            }
        }
    }
}

// ── Error Position/Span Tests (implementation-specific) ─────────────

#[test]
fn test_error_unclosed_bracket() {
    let result = crate::parse_motly_0("a = [", MOTLYDataNode::new());
    assert_eq!(result.errors.len(), 1);
    assert_eq!(result.errors[0].code, "tag-parse-syntax-error");
    assert_eq!(result.errors[0].begin.line, 0);
    assert!(result.errors[0].begin.offset <= result.errors[0].end.offset);
}

#[test]
fn test_error_unclosed_string() {
    let result = crate::parse_motly_0("desc=\"forgot to close\n", MOTLYDataNode::new());
    assert_eq!(result.errors.len(), 1);
    assert_eq!(result.errors[0].begin.line, 0);
    assert!(result.errors[0].begin.offset <= result.errors[0].end.offset);
}

#[test]
fn test_error_on_second_line() {
    let result = crate::parse_motly_0("valid=1\ninvalid=[", MOTLYDataNode::new());
    assert_eq!(result.errors.len(), 1);
    assert_eq!(result.errors[0].begin.line, 1);
}

#[test]
fn test_error_span_covers_region() {
    let result = crate::parse_motly_0("a = [b", MOTLYDataNode::new());
    let err = &result.errors[0];
    assert_eq!(err.begin.line, 0);
    assert_eq!(err.end.line, 0);
    assert!(err.begin.offset < err.end.offset);
}

#[test]
fn test_error_span_unclosed_string() {
    let result = crate::parse_motly_0("x=\"unterminated\n", MOTLYDataNode::new());
    let err = &result.errors[0];
    assert_eq!(err.begin.line, 0);
    assert_eq!(err.end.line, 0);
    assert!(err.begin.offset < err.end.offset);
}

#[test]
fn test_error_has_begin_end_positions() {
    let result = crate::parse_motly_0("a = [", MOTLYDataNode::new());
    let err = &result.errors[0];
    assert_eq!(err.begin.line, 0);
    assert_eq!(err.end.line, 0);
    assert!(err.begin.column > 0);
    assert!(err.end.column > 0);
    assert!(err.begin.offset > 0);
    assert!(err.end.offset > 0);
}

// ── JSON Serialization Tests (implementation-specific) ──────────────

#[test]
fn test_json_simple() {
    let json = crate::parse_motly_0("name=hello", MOTLYDataNode::new())
        .value
        .to_json();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["properties"]["name"]["eq"], "hello");
}

#[test]
fn test_json_link() {
    let json = crate::parse_motly_0("ref=$target", MOTLYDataNode::new())
        .value
        .to_json();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["properties"]["ref"]["linkTo"], serde_json::json!(["target"]));
    assert_eq!(v["properties"]["ref"]["linkUps"], 0);
}

#[test]
fn test_json_deleted() {
    let json = crate::parse_motly_0("-gone", MOTLYDataNode::new())
        .value
        .to_json();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["properties"]["gone"]["deleted"], true);
}

#[test]
fn test_json_array() {
    let json = crate::parse_motly_0("items=[a, b]", MOTLYDataNode::new())
        .value
        .to_json();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let items = &v["properties"]["items"];
    assert!(items["eq"].is_array());
    assert_eq!(items["eq"][0]["eq"], "a");
    assert_eq!(items["eq"][1]["eq"], "b");
}

#[test]
fn test_json_number() {
    let json = crate::parse_motly_0("count=42", MOTLYDataNode::new())
        .value
        .to_json();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["properties"]["count"]["eq"], 42.0);
}

#[test]
fn test_json_boolean() {
    let json = crate::parse_motly_0("active=@true", MOTLYDataNode::new())
        .value
        .to_json();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["properties"]["active"]["eq"], true);
}

#[test]
fn test_json_date() {
    let json = crate::parse_motly_0("created=@2024-01-15", MOTLYDataNode::new())
        .value
        .to_json();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["properties"]["created"]["eq"], "2024-01-15");
}

#[test]
fn test_json_nested() {
    let json = crate::parse_motly_0("a { b { c=1 } }", MOTLYDataNode::new())
        .value
        .to_json();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(
        v["properties"]["a"]["properties"]["b"]["properties"]["c"]["eq"],
        1.0
    );
}

// ── K8s deployment: real-world schema validation ────────────────────

#[test]
fn test_k8s_schema_parses() {
    let schema_src = include_str!("../test-data/k8s-deployment-schema.motly");
    let result = crate::parse_motly_0(schema_src, MOTLYDataNode::new());
    assert!(
        result.errors.is_empty(),
        "K8s schema failed to parse: {:?}",
        result.errors
    );
}

#[test]
fn test_k8s_sample_parses() {
    let sample_src = include_str!("../test-data/k8s-deployment-sample.motly");
    let result = crate::parse_motly_0(sample_src, MOTLYDataNode::new());
    assert!(
        result.errors.is_empty(),
        "K8s sample failed to parse: {:?}",
        result.errors
    );
}

#[test]
#[ignore = "schema validation not yet implemented in Rust"]
fn test_k8s_sample_validates_against_schema() {
    let schema_src = include_str!("../test-data/k8s-deployment-schema.motly");
    let sample_src = include_str!("../test-data/k8s-deployment-sample.motly");
    let schema = crate::parse_motly_0(schema_src, MOTLYDataNode::new());
    let sample = crate::parse_motly_0(sample_src, MOTLYDataNode::new());
    assert!(schema.errors.is_empty());
    assert!(sample.errors.is_empty());
    let errors = validate_schema(&sample.value, &schema.value);
    assert!(
        errors.is_empty(),
        "K8s sample failed to validate against schema ({} errors):\n{}",
        errors.len(),
        errors
            .iter()
            .map(|e| format!("  [{}] {} at /{}", e.code, e.message, e.path.join("/")))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
#[ignore = "schema validation not yet implemented in Rust"]
fn test_k8s_missing_required_fields() {
    let schema_src = include_str!("../test-data/k8s-deployment-schema.motly");
    let schema = crate::parse_motly_0(schema_src, MOTLYDataNode::new());
    assert!(schema.errors.is_empty());
    let tag = crate::parse_motly_0("apiVersion=\"apps/v1\"", MOTLYDataNode::new());
    let errors = validate_schema(&tag.value, &schema.value);
    assert!(errors
        .iter()
        .any(|e| e.code == "missing-required" && e.path == vec!["kind"]));
    assert!(errors
        .iter()
        .any(|e| e.code == "missing-required" && e.path == vec!["metadata"]));
    assert!(errors
        .iter()
        .any(|e| e.code == "missing-required" && e.path == vec!["spec"]));
}

#[test]
#[ignore = "schema validation not yet implemented in Rust"]
fn test_k8s_wrong_kind_enum() {
    let schema_src = include_str!("../test-data/k8s-deployment-schema.motly");
    let schema = crate::parse_motly_0(schema_src, MOTLYDataNode::new());
    assert!(schema.errors.is_empty());
    let tag = crate::parse_motly_0(
        "apiVersion=\"apps/v1\" kind=CronJob metadata { name=test } spec { selector { matchLabels { app=test } } template { metadata { name=test } spec { containers=[{name=x image=\"img:v1\"}] } } }",
        MOTLYDataNode::new(),
    );
    assert!(tag.errors.is_empty());
    let errors = validate_schema(&tag.value, &schema.value);
    assert!(
        errors
            .iter()
            .any(|e| e.code == "invalid-enum-value" && e.path == vec!["kind"]),
        "Expected invalid-enum-value for kind, got: {:?}",
        errors
    );
}

#[test]
#[ignore = "schema validation not yet implemented in Rust"]
fn test_k8s_bad_image_pattern() {
    let schema_src = include_str!("../test-data/k8s-deployment-schema.motly");
    let schema = crate::parse_motly_0(schema_src, MOTLYDataNode::new());
    assert!(schema.errors.is_empty());
    let tag = crate::parse_motly_0(
        "apiVersion=\"apps/v1\" kind=Deployment metadata { name=test } spec { selector { matchLabels { app=test } } template { metadata { name=test } spec { containers=[{name=x image=oopsnotag}] } } }",
        MOTLYDataNode::new(),
    );
    assert!(tag.errors.is_empty());
    let errors = validate_schema(&tag.value, &schema.value);
    assert!(
        errors.iter().any(|e| e.code == "pattern-mismatch"),
        "Expected pattern-mismatch for image, got: {:?}",
        errors
    );
}

#[test]
#[ignore = "schema validation not yet implemented in Rust"]
fn test_k8s_bad_container_port_type() {
    let schema_src = include_str!("../test-data/k8s-deployment-schema.motly");
    let schema = crate::parse_motly_0(schema_src, MOTLYDataNode::new());
    assert!(schema.errors.is_empty());
    let tag = crate::parse_motly_0(
        "apiVersion=\"apps/v1\" kind=Deployment metadata { name=test } spec { selector { matchLabels { app=test } } template { metadata { name=test } spec { containers=[{name=x image=\"img:v1\" ports=[{containerPort=eighty}]}] } } }",
        MOTLYDataNode::new(),
    );
    assert!(tag.errors.is_empty());
    let errors = validate_schema(&tag.value, &schema.value);
    assert!(
        errors.iter().any(|e| e.code == "wrong-type"),
        "Expected wrong-type for containerPort, got: {:?}",
        errors
    );
}

// ── Location tracking tests ─────────────────────────────────────────

/// Helper: get the location of a property node at the given path.
fn prop_loc(node: &MOTLYDataNode, path: &[&str]) -> Option<MOTLYLocation> {
    let mut cur = node;
    for &key in path {
        match cur.properties.as_ref()?.get(key)? {
            MOTLYNode::Data(n) => cur = n,
            MOTLYNode::Ref { .. } => return None,
        }
    }
    cur.location
}

#[test]
fn test_loc_simple_node_gets_location() {
    let result = crate::parse_motly_n("a = 1", MOTLYDataNode::new(), 0);
    assert!(result.errors.is_empty());
    let l = prop_loc(&result.value, &["a"]).expect("a should have location");
    assert_eq!(l.parse_id, 0);
    assert_eq!(l.begin.line, 0);
    assert_eq!(l.begin.column, 0);
}

#[test]
fn test_loc_multiple_nodes() {
    let result = crate::parse_motly_n("a = 1\nb = 2", MOTLYDataNode::new(), 0);
    assert!(result.errors.is_empty());
    let la = prop_loc(&result.value, &["a"]).unwrap();
    let lb = prop_loc(&result.value, &["b"]).unwrap();
    assert_eq!(la.begin.line, 0);
    assert_eq!(la.begin.column, 0);
    assert_eq!(lb.begin.line, 1);
    assert_eq!(lb.begin.column, 0);
}

#[test]
fn test_loc_first_appearance_seteq() {
    let mut node = MOTLYDataNode::new();
    let r1 = crate::parse_motly_n("a = 1", node, 0);
    assert!(r1.errors.is_empty());
    node = r1.value;
    let r2 = crate::parse_motly_n("a = 2", node, 1);
    assert!(r2.errors.is_empty());
    let l = prop_loc(&r2.value, &["a"]).unwrap();
    assert_eq!(l.parse_id, 0, "location should be from first parse");
    // Value should be updated
    match r2.value.properties.as_ref().unwrap().get("a").unwrap() {
        MOTLYNode::Data(n) => {
            assert_eq!(n.eq, Some(EqValue::Scalar(Scalar::Number(2.0))));
        }
        _ => panic!("expected node"),
    }
}

#[test]
fn test_loc_first_appearance_update_properties() {
    let mut node = MOTLYDataNode::new();
    let r1 = crate::parse_motly_n("a { b = 1 }", node, 0);
    assert!(r1.errors.is_empty());
    node = r1.value;
    let r2 = crate::parse_motly_n("a { c = 2 }", node, 1);
    assert!(r2.errors.is_empty());
    let l = prop_loc(&r2.value, &["a"]).unwrap();
    assert_eq!(l.parse_id, 0, "location should be from first parse");
}

#[test]
fn test_loc_first_appearance_replace_properties() {
    let mut node = MOTLYDataNode::new();
    let r1 = crate::parse_motly_n("a = 1", node, 0);
    assert!(r1.errors.is_empty());
    node = r1.value;
    let r2 = crate::parse_motly_n("a: { b = 2 }", node, 1);
    assert!(r2.errors.is_empty());
    let l = prop_loc(&r2.value, &["a"]).unwrap();
    assert_eq!(l.parse_id, 0, "location should be from first parse");
}

#[test]
fn test_loc_assign_both_replaces_location() {
    let mut node = MOTLYDataNode::new();
    let r1 = crate::parse_motly_n("a = 1", node, 0);
    assert!(r1.errors.is_empty());
    node = r1.value;
    let r2 = crate::parse_motly_n("a := 2", node, 1);
    assert!(r2.errors.is_empty());
    let l = prop_loc(&r2.value, &["a"]).unwrap();
    assert_eq!(l.parse_id, 1, ":= should set new location");
}

#[test]
fn test_loc_assign_both_clone_replaces_location() {
    let mut node = MOTLYDataNode::new();
    let r1 = crate::parse_motly_n("a = 1 { x = 10 }", node, 0);
    assert!(r1.errors.is_empty());
    node = r1.value;
    let r2 = crate::parse_motly_n("b := $a", node, 1);
    assert!(r2.errors.is_empty());
    let la = prop_loc(&r2.value, &["a"]).unwrap();
    let lb = prop_loc(&r2.value, &["b"]).unwrap();
    assert_eq!(la.parse_id, 0);
    assert_eq!(lb.parse_id, 1, "cloned node should have new location");
}

#[test]
fn test_loc_nested_properties() {
    let result = crate::parse_motly_n("a { b = 1\n  c = 2 }", MOTLYDataNode::new(), 0);
    assert!(result.errors.is_empty());
    let la = prop_loc(&result.value, &["a"]).unwrap();
    let lb = prop_loc(&result.value, &["a", "b"]).unwrap();
    let lc = prop_loc(&result.value, &["a", "c"]).unwrap();
    assert!(la.begin.line == 0);
    assert!(lb != lc, "b and c should have different locations");
}

#[test]
fn test_loc_intermediate_path_nodes() {
    let result = crate::parse_motly_n("a.b.c = 1", MOTLYDataNode::new(), 0);
    assert!(result.errors.is_empty());
    let la = prop_loc(&result.value, &["a"]).unwrap();
    let lb = prop_loc(&result.value, &["a", "b"]).unwrap();
    let lc = prop_loc(&result.value, &["a", "b", "c"]).unwrap();
    assert_eq!(la.parse_id, 0);
    assert_eq!(lb.parse_id, 0);
    assert_eq!(lc.parse_id, 0);
}

#[test]
fn test_loc_deletion_sets_location() {
    let mut node = MOTLYDataNode::new();
    let r1 = crate::parse_motly_n("a = 1", node, 0);
    assert!(r1.errors.is_empty());
    node = r1.value;
    let r2 = crate::parse_motly_n("-a", node, 1);
    assert!(r2.errors.is_empty());
    let l = prop_loc(&r2.value, &["a"]).unwrap();
    assert_eq!(l.parse_id, 1, "deleted node should have new location");
    match r2.value.properties.as_ref().unwrap().get("a").unwrap() {
        MOTLYNode::Data(n) => assert!(n.deleted),
        _ => panic!("expected node"),
    }
}

#[test]
fn test_loc_span_covers_statement() {
    let result = crate::parse_motly_n("a = 100", MOTLYDataNode::new(), 0);
    assert!(result.errors.is_empty());
    let l = prop_loc(&result.value, &["a"]).unwrap();
    assert_eq!(l.begin.offset, 0);
    assert!(l.end.offset >= 7, "end offset should be >= 7, got {}", l.end.offset);
}

#[test]
fn test_loc_define_bare_mention() {
    let mut node = MOTLYDataNode::new();
    let r1 = crate::parse_motly_n("a", node, 0);
    assert!(r1.errors.is_empty());
    node = r1.value;
    let r2 = crate::parse_motly_n("a = 1", node, 1);
    assert!(r2.errors.is_empty());
    let l = prop_loc(&r2.value, &["a"]).unwrap();
    assert_eq!(l.parse_id, 0, "bare define should set location");
}

#[test]
fn test_loc_session_parse_ids() {
    let mut node = MOTLYDataNode::new();
    let r0 = crate::parse_motly_n("a = 1", node, 0);
    node = r0.value;
    let r1 = crate::parse_motly_n("b = 2", node, 1);
    node = r1.value;
    let r2 = crate::parse_motly_n("c = 3", node, 2);
    node = r2.value;
    assert_eq!(prop_loc(&node, &["a"]).unwrap().parse_id, 0);
    assert_eq!(prop_loc(&node, &["b"]).unwrap().parse_id, 1);
    assert_eq!(prop_loc(&node, &["c"]).unwrap().parse_id, 2);
}

// ── Meta-schema self-validation ─────────────────────────────────────

#[test]
#[ignore = "schema validation not yet implemented in Rust"]
fn test_meta_schema_validates_itself() {
    let schema_src = include_str!("../test-data/motly-schema.motly");
    let schema = crate::parse_motly_0(schema_src, MOTLYDataNode::new());
    assert!(
        schema.errors.is_empty(),
        "Meta-schema failed to parse: {:?}",
        schema.errors
    );
    let errors = validate_schema(&schema.value, &schema.value);
    assert!(
        errors.is_empty(),
        "Meta-schema failed to validate against itself: {:?}",
        errors
    );
}

// ── allow_refs option tests ────────────────────────────────────────

#[test]
fn test_allow_refs_false_rejects_eq_ref() {
    use crate::interpreter::SessionOptions;
    let options = SessionOptions { disable_references: true };
    let (root, errors) = crate::session_finish_ex(&["a = hello\nb = $a"], options, false);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "ref-not-allowed");
    // a should still be set
    assert!(root.properties.as_ref().unwrap().contains_key("a"));
    // b should exist as a ref (disableReferences is diagnostic only)
    assert!(root.properties.as_ref().unwrap().contains_key("b"));
    assert!(matches!(root.properties.as_ref().unwrap().get("b"), Some(MOTLYNode::Ref { .. })));
}

#[test]
fn test_allow_refs_false_rejects_array_ref() {
    use crate::interpreter::SessionOptions;
    let options = SessionOptions { disable_references: true };
    let (root, errors) = crate::session_finish_ex(&["items = [hello, $foo]"], options, false);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "ref-not-allowed");
    // The ref should still be in the tree
    let items_eq = &root.properties.as_ref().unwrap().get("items").unwrap();
    if let MOTLYNode::Data(node) = items_eq {
        if let Some(EqValue::Array(arr)) = &node.eq {
            assert_eq!(arr.len(), 2);
            assert!(matches!(&arr[1], MOTLYNode::Ref { .. }));
        } else {
            panic!("items should have an array eq");
        }
    } else {
        panic!("items should be a data node");
    }
}

#[test]
fn test_allow_refs_false_allows_clone() {
    use crate::interpreter::SessionOptions;
    let options = SessionOptions { disable_references: true };
    let (root, errors) = crate::session_finish_ex(&["a = hello\nb := $a"], options, false);
    assert!(errors.is_empty(), "clone should be allowed: {:?}", errors);
    let b = root.properties.as_ref().unwrap().get("b").unwrap();
    match b {
        MOTLYNode::Data(node) => {
            assert_eq!(node.eq, Some(EqValue::Scalar(Scalar::String("hello".to_string()))));
        }
        MOTLYNode::Ref { .. } => panic!("b should be a data node, not a ref"),
    }
}

#[test]
fn test_allow_refs_default_allows_refs() {
    let result = crate::parse_motly_0("a = hello\nb = $a", MOTLYDataNode::new());
    assert!(result.errors.is_empty());
    let b = result.value.properties.as_ref().unwrap().get("b").unwrap();
    assert!(matches!(b, MOTLYNode::Ref { .. }));
}
