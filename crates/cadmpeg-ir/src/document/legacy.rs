// SPDX-License-Identifier: Apache-2.0
//! Refusal diagnostics for unsupported CADIR wires.

use super::IR_VERSION;

pub(super) fn unsupported_version(version: Option<&str>) -> String {
    format!(
        "unsupported ir_version {version:?}; expected {IR_VERSION}; wire v8 replaces decode_depth with scope and changes decode report transfer to delivered scope"
    )
}

#[cfg(test)]
mod tests {
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
        let error = CadIr::from_json(old).unwrap_err();
        for message in ["expected 8", "decode_depth", "scope", "report"] {
            assert!(error.to_string().contains(message), "{error}");
        }
    }

    #[test]
    fn unsupported_version_after_changed_model_shape_keeps_version_diagnostic() {
        for version in ["6", "7"] {
            let json = format!(
                r#"{{"model":{{"features":{{"legacy":true}}}},"ir_version":"{version}","decode_depth":"full"}}"#
            );
            let error = CadIr::from_json(&json).unwrap_err().to_string();
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

    fn rejected_legacy_shape(version: &str, depth: Option<&str>) {
        let mut value = serde_json::to_value(CadIr::empty()).unwrap();
        let object = value.as_object_mut().unwrap();
        object.insert("ir_version".into(), serde_json::json!(version));
        object.remove("scope");
        if let Some(depth) = depth {
            object.insert("decode_depth".into(), serde_json::json!(depth));
        }
        let json = serde_json::to_string(&value).unwrap();
        let error = CadIr::from_json(&json).unwrap_err();
        for message in ["expected 8", "decode_depth", "scope", "report"] {
            assert!(error.to_string().contains(message), "{error}");
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
}
