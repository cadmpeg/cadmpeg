use super::super::{ArchiveSpan, ArchiveSpanRole, ByteSpan};
use serde_json::{json, Value};

const NAMED_ROLES: [&str; 11] = [
    "local-signature", "local-fields", "local-name", "local-extra",
    "compressed-payload", "data-descriptor", "central-signature", "central-fields",
    "central-name", "central-extra", "central-comment",
];
const UNIT_ROLES: [&str; 4] = [
    "zip64-end-record", "zip64-end-locator", "end-record", "digital-signature",
];

fn wire(role: &str, entry: Value) -> Value {
    json!({"id": "span", "start": 145, "end": 154, "role": role, "entry": entry})
}

#[test]
fn digital_signature_span_serializes_exact_ownerless_wire() {
    let span = ArchiveSpan {
        id: "span".into(),
        span: ByteSpan::try_new(145, 154).unwrap(),
        role: ArchiveSpanRole::DigitalSignature,
    };
    assert_eq!(span.role.as_str(), "digital-signature");
    assert_eq!(span.role.entry(), None);
    let expected = wire("digital-signature", Value::Null);
    assert_eq!(serde_json::to_value(&span).unwrap(), expected);
    assert_eq!(serde_json::from_value::<ArchiveSpan>(expected).unwrap(), span);
}

#[test]
fn archive_span_roles_retain_all_named_and_unit_ownership_rules() {
    for role in NAMED_ROLES {
        for owner in ["", "Document.xml"] {
            let expected = wire(role, json!(owner));
            let span = serde_json::from_value::<ArchiveSpan>(expected.clone()).unwrap();
            assert_eq!(span.role.as_str(), role);
            assert_eq!(span.role.entry(), Some(owner));
            assert_eq!(serde_json::to_value(span).unwrap(), expected);
        }
        for missing in [false, true] {
            let mut invalid = wire(role, Value::Null);
            if missing {
                invalid.as_object_mut().unwrap().remove("entry");
            }
            let error = serde_json::from_value::<ArchiveSpan>(invalid).unwrap_err();
            assert_eq!(error.to_string(), format!(
                "archive span role {role} requires an owning entry"
            ));
        }
    }
    for role in UNIT_ROLES {
        for missing in [false, true] {
            let expected = wire(role, Value::Null);
            let mut input = expected.clone();
            if missing {
                input.as_object_mut().unwrap().remove("entry");
            }
            let span = serde_json::from_value::<ArchiveSpan>(input).unwrap();
            assert_eq!(span.role.as_str(), role);
            assert_eq!(span.role.entry(), None);
            assert_eq!(serde_json::to_value(span).unwrap(), expected);
        }
        for owner in ["", "Document.xml"] {
            let error = serde_json::from_value::<ArchiveSpan>(wire(role, json!(owner)))
                .unwrap_err();
            assert_eq!(error.to_string(), format!(
                "archive span role {role} cannot carry an owning entry"
            ));
        }
    }
}

#[test]
fn archive_padding_span_retains_optional_owner_and_old_wire() {
    for entry in [Value::Null, json!(""), json!("Document.xml")] {
        let expected = wire("archive-padding", entry.clone());
        let span = serde_json::from_value::<ArchiveSpan>(expected.clone()).unwrap();
        assert_eq!(span.role.as_str(), "archive-padding");
        assert_eq!(span.role.entry(), entry.as_str());
        assert_eq!(serde_json::to_value(span).unwrap(), expected);
    }
    let mut input = wire("archive-padding", Value::Null);
    input.as_object_mut().unwrap().remove("entry");
    let span = serde_json::from_value::<ArchiveSpan>(input).unwrap();
    assert_eq!(span.role, ArchiveSpanRole::ArchivePadding);
}

#[test]
fn archive_span_role_labels_remain_closed_and_exact() {
    for role in ["unknown", "Digital-Signature", "digital-signature ", " digital-signature"] {
        let error = serde_json::from_value::<ArchiveSpan>(wire(role, Value::Null))
            .unwrap_err();
        assert_eq!(error.to_string(), format!("unknown archive span role {role}"));
    }
    let mut input = wire("digital-signature", Value::Null);
    input["additional"] = json!("ignored as before");
    let span = serde_json::from_value::<ArchiveSpan>(input).unwrap();
    assert_eq!(serde_json::to_value(span).unwrap(), wire("digital-signature", Value::Null));
    for entry in [json!(1), json!([]), json!({})] {
        assert!(serde_json::from_value::<ArchiveSpan>(wire("digital-signature", entry)).is_err());
    }
}

#[test]
fn digital_signature_span_rejects_empty_and_reversed_intervals_before_role() {
    for (start, end) in [(145, 145), (154, 145)] {
        let expected = ByteSpan::try_new(start, end).unwrap_err();
        for role in ["digital-signature", "unknown"] {
            let mut input = wire(role, json!("invalid owner"));
            input["start"] = json!(start);
            input["end"] = json!(end);
            let error = serde_json::from_value::<ArchiveSpan>(input).unwrap_err();
            assert_eq!(error.to_string(), expected);
        }
    }
}
