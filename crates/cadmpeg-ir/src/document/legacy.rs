// SPDX-License-Identifier: Apache-2.0
//! Refusal diagnostics for unsupported CADIR wires.

use super::IR_VERSION;

pub(super) fn unsupported_version(version: Option<&str>) -> String {
    format!(
        "unsupported ir_version {version:?}; expected {IR_VERSION}; wire v7 replaces decode_depth with scope and changes decode report transfer to delivered scope"
    )
}

#[cfg(test)]
mod tests {
    use super::super::CadIr;

    #[test]
    fn wire_v6_refusal_names_scope_and_report_changes() {
        let mut value = serde_json::to_value(CadIr::empty()).unwrap();
        value["ir_version"] = serde_json::json!("6");
        let error = CadIr::from_json(&serde_json::to_string(&value).unwrap()).unwrap_err();
        for message in ["expected 7", "decode_depth", "scope", "report"] {
            assert!(error.to_string().contains(message), "{error}");
        }
        // Older writers state the wire version before their depth field.
        let old = r#"{"ir_version":"6","decode_depth":"full"}"#;
        let error = serde_json::from_str::<CadIr>(old).unwrap_err();
        for message in ["expected 7", "decode_depth", "scope", "report"] {
            assert!(error.to_string().contains(message), "{error}");
        }
    }
}
