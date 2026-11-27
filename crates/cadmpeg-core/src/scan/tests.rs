// SPDX-License-Identifier: Apache-2.0

use super::{DesignScan, FeatureCensus, Readability, StopReason};
use crate::json;

#[test]
fn scan_optional_fields_omit_absence_and_reject_null() {
    let scan = DesignScan::new(None);
    let value = crate::serde_erase::json::to_value(&scan).expect("scan serialization");
    for field in [
        "format",
        "source_version",
        "document_kind",
        "census",
        "stop_reason",
    ] {
        assert!(value.get(field).is_none(), "{field}");
        let mut invalid = value.clone();
        invalid[field] = serde_json::Value::Null;
        let error = crate::serde_erase::json::from_value::<DesignScan>(invalid)
            .expect_err("optional null refusal");
        assert!(error.to_string().contains(field), "{error}");
    }
    assert_eq!(
        crate::serde_erase::json::from_value::<DesignScan>(value).expect("scan round trip"),
        scan
    );
    let census = FeatureCensus::default();
    let mut value = crate::serde_erase::json::to_value(&census).expect("census serialization");
    assert!(value.get("total").is_none());
    assert_eq!(
        crate::serde_erase::json::from_value::<FeatureCensus>(value.clone())
            .expect("census round trip"),
        census
    );
    value["total"] = serde_json::Value::Null;
    assert!(crate::serde_erase::json::from_value::<FeatureCensus>(value)
        .expect_err("total null refusal")
        .to_string()
        .contains("total"));
}

#[test]
fn scan_payload_enums_use_adjacent_wire_forms() {
    for (value, expected) in [
        (Readability::KnownYes, json!({"kind": "known_yes"})),
        (
            Readability::KnownNo("invalid listing".into()),
            json!({"kind": "known_no", "value": "invalid listing"}),
        ),
        (
            Readability::Unknown("pending".into()),
            json!({"kind": "unknown", "value": "pending"}),
        ),
    ] {
        assert_eq!(
            crate::serde_erase::json::to_value(&value).expect("enum serialization"),
            expected
        );
        assert_eq!(
            crate::serde_erase::json::from_value::<Readability>(expected)
                .expect("readability round trip"),
            value
        );
    }
    for (value, expected) in [
        (StopReason::Cancelled, json!({"kind": "cancelled"})),
        (
            StopReason::Truncated("header".into()),
            json!({"kind": "truncated", "value": "header"}),
        ),
        (
            StopReason::Malformed("record".into()),
            json!({"kind": "malformed", "value": "record"}),
        ),
        (
            StopReason::Unsupported("reader".into()),
            json!({"kind": "unsupported", "value": "reader"}),
        ),
    ] {
        assert_eq!(
            crate::serde_erase::json::to_value(&value).expect("enum serialization"),
            expected
        );
        assert_eq!(
            crate::serde_erase::json::from_value::<StopReason>(expected)
                .expect("stop reason round trip"),
            value
        );
    }
}

fn assert_tag_only<T>(kind: &str, expected: &T)
where
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let tag = json!({"kind": kind});
    assert_eq!(
        crate::serde_erase::json::to_value(expected).expect("unit serialization"),
        tag
    );
    assert_eq!(
        &crate::serde_erase::json::from_value::<T>(tag).expect("bare tag reads"),
        expected
    );
    for payload in [serde_json::Value::Null, json!({}), json!([]), json!(true)] {
        for text in [
            format!(r#"{{"kind":"{kind}","value":{payload}}}"#),
            format!(r#"{{"value":{payload},"kind":"{kind}"}}"#),
        ] {
            assert!(serde_json::from_str::<T>(&text).is_err(), "{text}");
        }
    }
}

#[test]
fn scan_tag_only_variants_refuse_stated_payloads() {
    assert_tag_only("known_yes", &Readability::KnownYes);
    for (kind, value) in [
        ("source_bytes", StopReason::SourceBytes),
        ("expanded_bytes", StopReason::ExpandedBytes),
        ("retained_bytes", StopReason::RetainedBytes),
        ("elapsed_time", StopReason::ElapsedTime),
        ("feature_count", StopReason::FeatureCount),
        ("nesting_depth", StopReason::NestingDepth),
        ("cancelled", StopReason::Cancelled),
        ("unrecognized", StopReason::Unrecognized),
    ] {
        assert_tag_only(kind, &value);
    }
}
