// SPDX-License-Identifier: Apache-2.0
//! Version-prefix ordering and strict key admission.

#![allow(clippy::unwrap_used)]

use super::super::CadIr;

#[test]
fn wire_v6_refusal_names_scope_and_report_changes() {
    let mut value = serde_json::to_value(CadIr::empty()).unwrap();
    value["ir_version"] = serde_json::json!("6");
    let error = CadIr::from_json(&serde_json::to_string(&value).unwrap()).unwrap_err();
    for message in ["expected 8", "decode_depth", "scope", "report"] {
        assert!(error.to_string().contains(message), "{error}");
    }
    // Older writers state the wire version before their depth field.
    let old = r#"{"ir_version":"6","decode_depth":"full"}"#;
    let error = serde_json::from_str::<CadIr>(old).unwrap_err();
    for message in ["expected 8", "decode_depth", "scope", "report"] {
        assert!(error.to_string().contains(message), "{error}");
    }
}

fn rejected_legacy_shape(version: &str, depth: Option<&str>) {
    let mut value = serde_json::to_value(CadIr::empty()).unwrap();
    let object = value.as_object_mut().unwrap();
    object.insert("ir_version".into(), serde_json::json!(version));
    object.remove("scope");
    if let Some(depth) = depth {
        object.insert("decode_depth".into(), serde_json::json!(depth));
    }
    let json = serde_json::to_string(&value).unwrap();
    for error in [
        CadIr::from_json(&json).unwrap_err(),
        serde_json::from_str::<CadIr>(&json).unwrap_err(),
    ] {
        for message in ["expected 8", "decode_depth", "scope", "report"] {
            assert!(error.to_string().contains(message), "{error}");
        }
    }
}

#[test]
fn wire_v061_refusal_names_scope_and_report_changes() {
    rejected_legacy_shape("6", None);
}

#[test]
fn wire_v062_refusal_names_scope_and_report_changes() {
    rejected_legacy_shape("6", Some("full"));
}

#[test]
fn wire_main_v7_refusal_names_scope_and_report_changes() {
    rejected_legacy_shape("7", Some("full"));
}

fn current_with_prefix(field: &str, raw: &str) -> String {
    let mut value = serde_json::to_value(CadIr::empty()).unwrap();
    value.as_object_mut().unwrap().remove(field);
    let remaining = serde_json::to_string(&value).unwrap();
    format!("{{{field:?}:{raw},{}", &remaining[1..])
}

#[test]
fn current_version_after_model_and_native_rejects_nested_duplicate_keys() {
    for (field, raw, key) in [
        (
            "model",
            r#"{"attributes":[],"attributes":[]}"#,
            "attributes",
        ),
        (
            "native",
            r#"{"iges":{"entities":[],"entities":[]}}"#,
            "entities",
        ),
        (
            "model",
            r#"{"attributes":[{"name":"first","name":"second"}]}"#,
            "name",
        ),
    ] {
        let json = current_with_prefix(field, raw);
        assert!(json.find(field).unwrap() < json.find("ir_version").unwrap());
        let error = serde_json::from_str::<CadIr>(&json)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("duplicate") && error.contains(key),
            "{error}"
        );
    }
}

#[test]
fn current_version_after_prefix_still_refuses_unknown_keys() {
    for (field, raw, unknown) in [
        ("unknown_prefix", "7", "unknown_prefix"),
        (
            "model",
            r#"{"unknown_model_prefix":false}"#,
            "unknown_model_prefix",
        ),
    ] {
        let error = serde_json::from_str::<CadIr>(&current_with_prefix(field, raw))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("unknown field") && error.contains(unknown),
            "{error}"
        );
    }
}

#[test]
fn current_version_after_prefix_refuses_repeated_top_level_fields() {
    let scope =
        serde_json::to_string(&serde_json::to_value(CadIr::empty()).unwrap()["scope"]).unwrap();
    let json = current_with_prefix("scope", &scope);
    let repeated = format!("{{\"scope\":{scope},{}", &json[1..]);
    let error = serde_json::from_str::<CadIr>(&repeated)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("duplicate field") && error.contains("scope"),
        "{error}"
    );
}

#[test]
fn unsupported_version_after_changed_model_shape_keeps_version_diagnostic() {
    for version in ["6", "7"] {
        let json = format!(
            r#"{{"model":{{"features":{{"legacy":true}}}},"ir_version":"{version}","decode_depth":"full"}}"#
        );
        let error = serde_json::from_str::<CadIr>(&json)
            .unwrap_err()
            .to_string();
        for expected in [
            "unsupported ir_version",
            "expected 8",
            "decode_depth",
            "scope",
            "report",
        ] {
            assert!(error.contains(expected), "{error}");
        }
    }
}

#[test]
fn current_version_first_and_after_valid_prefix_have_the_same_document() {
    let expected = CadIr::empty();
    let model = serde_json::to_string(&serde_json::to_value(&expected).unwrap()["model"]).unwrap();
    let json = current_with_prefix("model", &model);
    assert_eq!(serde_json::from_str::<CadIr>(&json).unwrap(), expected);
    assert_eq!(
        serde_json::from_str::<CadIr>(&serde_json::to_string(&expected).unwrap()).unwrap(),
        expected
    );
}
