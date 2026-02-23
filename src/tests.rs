use crate::tree::*;
use crate::validate::{validate_references, validate_schema};

// ── Shared fixture runners ──────────────────────────────────────────

/// Embed fixture files at compile time.
const PARSE_FIXTURES: &str = include_str!("../test-data/fixtures/parse.json");
const PARSE_ERROR_FIXTURES: &str = include_str!("../test-data/fixtures/parse-errors.json");
const SCHEMA_FIXTURES: &str = include_str!("../test-data/fixtures/schema.json");
const REF_FIXTURES: &str = include_str!("../test-data/fixtures/refs.json");
const SESSION_FIXTURES: &str = include_str!("../test-data/fixtures/session.json");

/// Convert a serde_json::Value (fixture "expected" format) to a MOTLYNode.
/// Uses from_wire to handle {"$date": "..."} in expected values.
fn fixture_expected_to_value(expected: &serde_json::Value) -> MOTLYNode {
    let json_str = serde_json::to_string(expected).unwrap();
    crate::from_json::from_wire(&json_str).unwrap()
}

#[test]
fn test_fixture_parse() {
    let fixtures: Vec<serde_json::Value> = serde_json::from_str(PARSE_FIXTURES).unwrap();

    for fixture in &fixtures {
        let name = fixture["name"].as_str().unwrap();
        let input = &fixture["input"];
        let expected = fixture.get("expected");
        let expect_errors = fixture
            .get("expectErrors")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let value = if let Some(input_str) = input.as_str() {
            // Single input string
            let result = crate::parse_motly(input_str, MOTLYNode::new());
            if expect_errors {
                assert!(
                    !result.errors.is_empty(),
                    "Fixture '{}': expected parse errors but got none",
                    name
                );
                if expected.is_none() || expected == Some(&serde_json::Value::Null) {
                    continue;
                }
                // expectErrors + expected: errors are non-fatal, check the tree too
            } else {
                assert!(
                    result.errors.is_empty(),
                    "Fixture '{}': unexpected parse errors: {:?}",
                    name,
                    result.errors
                );
            }
            result.value
        } else if let Some(input_arr) = input.as_array() {
            // Array of inputs: accumulate
            let mut value = MOTLYNode::new();
            for chunk in input_arr {
                let chunk_str = chunk.as_str().unwrap();
                let result = crate::parse_motly(chunk_str, value);
                if !expect_errors {
                    assert!(
                        result.errors.is_empty(),
                        "Fixture '{}': unexpected parse errors on chunk '{}': {:?}",
                        name,
                        chunk_str,
                        result.errors
                    );
                }
                value = result.value;
            }
            value
        } else {
            panic!("Fixture '{}': input must be a string or array", name);
        };

        if let Some(expected) = expected {
            if !expected.is_null() {
                let expected_value = fixture_expected_to_value(expected);
                assert_eq!(
                    value, expected_value,
                    "Fixture '{}': value mismatch\n  Got:      {:?}\n  Expected: {:?}",
                    name, value, expected_value
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

        let result = crate::parse_motly(input, MOTLYNode::new());
        assert!(
            !result.errors.is_empty(),
            "Fixture '{}': expected parse errors for input '{}'",
            name,
            input
        );
    }
}

#[test]
fn test_fixture_schema() {
    let fixtures: Vec<serde_json::Value> = serde_json::from_str(SCHEMA_FIXTURES).unwrap();

    for fixture in &fixtures {
        let name = fixture["name"].as_str().unwrap();
        let schema_input = fixture["schema"].as_str().unwrap();
        let tag_input = fixture["input"].as_str().unwrap();
        let expected_errors = fixture["expectedErrors"].as_array().unwrap();

        let schema = crate::parse_motly(schema_input, MOTLYNode::new());
        assert!(
            schema.errors.is_empty(),
            "Fixture '{}': schema parse errors: {:?}",
            name,
            schema.errors
        );

        let tag = crate::parse_motly(tag_input, MOTLYNode::new());
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
    let fixtures: Vec<serde_json::Value> = serde_json::from_str(REF_FIXTURES).unwrap();

    for fixture in &fixtures {
        let name = fixture["name"].as_str().unwrap();
        let input = fixture["input"].as_str().unwrap();
        let expected_errors = fixture["expectedErrors"].as_array().unwrap();

        let result = crate::parse_motly(input, MOTLYNode::new());
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
                "Fixture '{}': error code mismatch at index {}",
                name, i
            );
            if !expected_entry.1.is_empty() {
                assert_eq!(
                    actual_entry.1, expected_entry.1,
                    "Fixture '{}': error path mismatch at index {}",
                    name, i
                );
            }
        }
    }
}

#[test]
fn test_fixture_session() {
    let fixtures: Vec<serde_json::Value> = serde_json::from_str(SESSION_FIXTURES).unwrap();

    for fixture in &fixtures {
        let name = fixture["name"].as_str().unwrap();
        let steps = fixture["steps"].as_array().unwrap();

        let mut value = MOTLYNode::new();
        let mut schema: Option<MOTLYNode> = None;

        for step in steps {
            let action = step["action"].as_str().unwrap();

            match action {
                "parse" => {
                    let input = step["input"].as_str().unwrap();
                    let result = crate::parse_motly(input, value);
                    if step
                        .get("expectErrors")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                    {
                        assert!(
                            !result.errors.is_empty(),
                            "Fixture '{}': expected parse errors for '{}'",
                            name,
                            input
                        );
                    } else {
                        assert!(
                            result.errors.is_empty(),
                            "Fixture '{}': unexpected parse errors for '{}': {:?}",
                            name,
                            input,
                            result.errors
                        );
                    }
                    value = result.value;
                }
                "parseSchema" => {
                    let input = step["input"].as_str().unwrap();
                    let result = crate::parse_motly(input, MOTLYNode::new());
                    assert!(
                        result.errors.is_empty(),
                        "Fixture '{}': schema parse errors: {:?}",
                        name,
                        result.errors
                    );
                    schema = Some(result.value);
                }
                "reset" => {
                    value = MOTLYNode::new();
                }
                "getValue" => {
                    if let Some(expected) = step.get("expected") {
                        let expected_value = fixture_expected_to_value(expected);
                        assert_eq!(
                            value, expected_value,
                            "Fixture '{}' (getValue): value mismatch",
                            name
                        );
                    }
                }
                "validateSchema" => {
                    let errors = match &schema {
                        Some(s) => validate_schema(&value, s),
                        None => Vec::new(),
                    };
                    if let Some(expected_errors) =
                        step.get("expectedErrors").and_then(|v| v.as_array())
                    {
                        assert_eq!(
                            errors.len(),
                            expected_errors.len(),
                            "Fixture '{}' (validateSchema): error count mismatch — got {}, expected {}",
                            name, errors.len(), expected_errors.len()
                        );
                        for (i, ee) in expected_errors.iter().enumerate() {
                            if let Some(code) = ee.get("code").and_then(|v| v.as_str()) {
                                assert_eq!(
                                    errors[i].code, code,
                                    "Fixture '{}' (validateSchema): error code mismatch at index {}",
                                    name, i
                                );
                            }
                        }
                    }
                }
                "validateReferences" => {
                    let errors = validate_references(&value);
                    if let Some(expected_errors) =
                        step.get("expectedErrors").and_then(|v| v.as_array())
                    {
                        assert_eq!(
                            errors.len(),
                            expected_errors.len(),
                            "Fixture '{}' (validateReferences): error count mismatch — got {}, expected {}",
                            name, errors.len(), expected_errors.len()
                        );
                        for (i, ee) in expected_errors.iter().enumerate() {
                            if let Some(code) = ee.get("code").and_then(|v| v.as_str()) {
                                assert_eq!(
                                    errors[i].code, code,
                                    "Fixture '{}' (validateReferences): error code mismatch at index {}",
                                    name, i
                                );
                            }
                        }
                    }
                }
                other => panic!("Fixture '{}': unknown action '{}'", name, other),
            }
        }
    }
}

// ── Error Position/Span Tests (implementation-specific) ─────────────

#[test]
fn test_error_unclosed_bracket() {
    let result = crate::parse_motly("a = [", MOTLYNode::new());
    assert_eq!(result.errors.len(), 1);
    assert_eq!(result.errors[0].code, "tag-parse-syntax-error");
    assert_eq!(result.errors[0].begin.line, 0);
    assert!(result.errors[0].begin.offset <= result.errors[0].end.offset);
}

#[test]
fn test_error_unclosed_string() {
    let result = crate::parse_motly("desc=\"forgot to close\n", MOTLYNode::new());
    assert_eq!(result.errors.len(), 1);
    assert_eq!(result.errors[0].begin.line, 0);
    assert!(result.errors[0].begin.offset <= result.errors[0].end.offset);
}

#[test]
fn test_error_on_second_line() {
    let result = crate::parse_motly("valid=1\ninvalid=[", MOTLYNode::new());
    assert_eq!(result.errors.len(), 1);
    assert_eq!(result.errors[0].begin.line, 1);
}

#[test]
fn test_error_span_covers_region() {
    let result = crate::parse_motly("a = [b", MOTLYNode::new());
    let err = &result.errors[0];
    assert_eq!(err.begin.line, 0);
    assert_eq!(err.end.line, 0);
    assert!(err.begin.offset < err.end.offset);
}

#[test]
fn test_error_span_unclosed_string() {
    let result = crate::parse_motly("x=\"unterminated\n", MOTLYNode::new());
    let err = &result.errors[0];
    assert_eq!(err.begin.line, 0);
    assert_eq!(err.end.line, 0);
    assert!(err.begin.offset < err.end.offset);
}

#[test]
fn test_error_has_begin_end_positions() {
    let result = crate::parse_motly("a = [", MOTLYNode::new());
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
    let json = crate::parse_motly("name=hello", MOTLYNode::new())
        .value
        .to_json();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["properties"]["name"]["eq"], "hello");
}

#[test]
fn test_json_link() {
    let json = crate::parse_motly("ref=$target", MOTLYNode::new())
        .value
        .to_json();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    // References are now top-level {"linkTo": "..."} (not inside eq)
    assert_eq!(v["properties"]["ref"]["linkTo"], "$target");
}

#[test]
fn test_json_deleted() {
    let json = crate::parse_motly("-gone", MOTLYNode::new())
        .value
        .to_json();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["properties"]["gone"]["deleted"], true);
}

#[test]
fn test_json_array() {
    let json = crate::parse_motly("items=[a, b]", MOTLYNode::new())
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
    let json = crate::parse_motly("count=42", MOTLYNode::new())
        .value
        .to_json();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["properties"]["count"]["eq"], 42.0);
}

#[test]
fn test_json_boolean() {
    let json = crate::parse_motly("active=@true", MOTLYNode::new())
        .value
        .to_json();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["properties"]["active"]["eq"], true);
}

#[test]
fn test_json_date() {
    let json = crate::parse_motly("created=@2024-01-15", MOTLYNode::new())
        .value
        .to_json();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["properties"]["created"]["eq"], "2024-01-15");
}

#[test]
fn test_json_nested() {
    let json = crate::parse_motly("a { b { c=1 } }", MOTLYNode::new())
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
    let result = crate::parse_motly(schema_src, MOTLYNode::new());
    assert!(
        result.errors.is_empty(),
        "K8s schema failed to parse: {:?}",
        result.errors
    );
}

#[test]
fn test_k8s_sample_parses() {
    let sample_src = include_str!("../test-data/k8s-deployment-sample.motly");
    let result = crate::parse_motly(sample_src, MOTLYNode::new());
    assert!(
        result.errors.is_empty(),
        "K8s sample failed to parse: {:?}",
        result.errors
    );
}

#[test]
fn test_k8s_sample_validates_against_schema() {
    let schema_src = include_str!("../test-data/k8s-deployment-schema.motly");
    let sample_src = include_str!("../test-data/k8s-deployment-sample.motly");
    let schema = crate::parse_motly(schema_src, MOTLYNode::new());
    let sample = crate::parse_motly(sample_src, MOTLYNode::new());
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
fn test_k8s_missing_required_fields() {
    let schema_src = include_str!("../test-data/k8s-deployment-schema.motly");
    let schema = crate::parse_motly(schema_src, MOTLYNode::new());
    assert!(schema.errors.is_empty());
    let tag = crate::parse_motly("apiVersion=\"apps/v1\"", MOTLYNode::new());
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
fn test_k8s_wrong_kind_enum() {
    let schema_src = include_str!("../test-data/k8s-deployment-schema.motly");
    let schema = crate::parse_motly(schema_src, MOTLYNode::new());
    assert!(schema.errors.is_empty());
    let tag = crate::parse_motly(
        "apiVersion=\"apps/v1\" kind=CronJob metadata { name=test } spec { selector { matchLabels { app=test } } template { metadata { name=test } spec { containers=[{name=x image=\"img:v1\"}] } } }",
        MOTLYNode::new(),
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
fn test_k8s_bad_image_pattern() {
    let schema_src = include_str!("../test-data/k8s-deployment-schema.motly");
    let schema = crate::parse_motly(schema_src, MOTLYNode::new());
    assert!(schema.errors.is_empty());
    let tag = crate::parse_motly(
        "apiVersion=\"apps/v1\" kind=Deployment metadata { name=test } spec { selector { matchLabels { app=test } } template { metadata { name=test } spec { containers=[{name=x image=oopsnotag}] } } }",
        MOTLYNode::new(),
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
fn test_k8s_bad_container_port_type() {
    let schema_src = include_str!("../test-data/k8s-deployment-schema.motly");
    let schema = crate::parse_motly(schema_src, MOTLYNode::new());
    assert!(schema.errors.is_empty());
    let tag = crate::parse_motly(
        "apiVersion=\"apps/v1\" kind=Deployment metadata { name=test } spec { selector { matchLabels { app=test } } template { metadata { name=test } spec { containers=[{name=x image=\"img:v1\" ports=[{containerPort=eighty}]}] } } }",
        MOTLYNode::new(),
    );
    assert!(tag.errors.is_empty());
    let errors = validate_schema(&tag.value, &schema.value);
    assert!(
        errors.iter().any(|e| e.code == "wrong-type"),
        "Expected wrong-type for containerPort, got: {:?}",
        errors
    );
}

// ── Meta-schema self-validation ─────────────────────────────────────

#[test]
fn test_meta_schema_validates_itself() {
    let schema_src = include_str!("../test-data/motly-schema.motly");
    let schema = crate::parse_motly(schema_src, MOTLYNode::new());
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
