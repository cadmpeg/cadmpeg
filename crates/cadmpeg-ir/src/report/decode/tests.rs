// SPDX-License-Identifier: Apache-2.0
use crate::report::decode::{DecodeReport, DecodeTransfer};
use cadmpeg_core::dialect::DialectLayers;
use std::collections::BTreeMap;

fn decode_report_for_wire_tests() -> DecodeReport {
    DecodeReport::unclassified(
        "test",
        DecodeTransfer {
            scope: crate::document::ContentScope::full(),
            geometry_transferred: false,
        },
        BTreeMap::new(),
        Vec::new(),
        Vec::new(),
    )
}

#[test]
fn hex_coverage_records_the_formatted_name() {
    use crate::report::decode::{Coverage, HexByteCoverageKey};
    let key = HexByteCoverageKey::new("type_", "_count");
    let mut coverage = Coverage::default();
    coverage.record_hex_byte(key, 0x0a, 7);
    assert_eq!(coverage.get("type_0a_count"), Some(&7));
}

#[test]
fn indexed_coverage_records_the_decimal_name() {
    use crate::report::decode::{Coverage, IndexedCoverageKey};
    let key = IndexedCoverageKey::decimal("type_", "_count");
    let mut coverage = Coverage::default();
    coverage.record_indexed(key, 4, 7);
    assert_eq!(coverage.get("type_4_count"), Some(&7));
}

#[test]
fn owned_coverage_record_preserves_declared_key() {
    let key = crate::report::decode::CoverageKey::new("decoded_entities");
    let mut coverage = crate::report::decode::Coverage::default();
    assert!(coverage
        .record_owned(key, "other_entities".to_owned(), 3)
        .is_err());
    assert!(coverage.is_empty());
    coverage
        .record_owned(key, "decoded_entities".to_owned(), 3)
        .expect("declared key is admitted");
    assert_eq!(coverage.get("decoded_entities"), Some(&3));
}

#[cfg(feature = "schema")]
#[test]
fn current_decode_report_schema_requires_its_format_identity() {
    let schema = serde_json::to_value(schemars::schema_for!(crate::report::decode::DecodeReport))
        .expect("decode report schema serializes");
    let required = schema["required"]
        .as_array()
        .expect("decode report schema has required fields");
    assert!(
        required.iter().any(|field| field == "identity"),
        "{schema:#}"
    );
    assert!(schema["properties"].get("transfer_ledger").is_none());
    assert_eq!(schema["additionalProperties"], false);
    for name in [
        "TransferDisposition",
        "TransferOutcome",
        "TransferRecord",
        "TransferLedger",
    ] {
        for definitions in ["$defs", "definitions"] {
            assert!(
                schema
                    .get(definitions)
                    .and_then(|types| types.get(name))
                    .is_none(),
                "{name}: {schema:#}"
            );
        }
    }
}

#[test]
fn transfer_record_keeps_the_nested_outcome_wire_shape() {
    let record = serde_json::json!({
        "source": "D1",
        "outcome": {
            "disposition": "retained",
            "target": "iges:entity:directory#1",
            "note": "retained"
        }
    });
    let report = decode_report_for_wire_tests();
    let mut wire = serde_json::to_value(&report).expect("decode report serializes");
    assert!(wire.get("transfer_ledger").is_none());
    assert_eq!(
        serde_json::from_value::<DecodeReport>(wire.clone()).unwrap(),
        report
    );
    wire["transfer_ledger"] = serde_json::json!({"entries": [record]});
    let error = serde_json::from_value::<DecodeReport>(wire)
        .expect_err("the removed transfer ledger is not a decode-report field");
    assert!(error.to_string().contains("transfer_ledger"), "{error}");
}

#[test]
fn transfer_record_wire_rejects_disposition_target_disagreement() {
    for record in [
        serde_json::json!({
            "source": "D1",
            "outcome": {"disposition": "retained"}
        }),
        serde_json::json!({
            "source": "D1",
            "outcome": {"disposition": "omitted", "target": "point:1"}
        }),
        serde_json::json!({
            "source": "D1",
            "outcome": {"disposition": "emitted", "target": "point:1", "note": "n"}
        }),
        serde_json::json!({
            "source": "D1",
            "outcome": {"disposition": "emitted"}
        }),
        serde_json::json!({
            "source": "D2",
            "outcome": {"disposition": "omitted", "note": "unsupported"}
        }),
    ] {
        let mut wire = serde_json::to_value(decode_report_for_wire_tests()).unwrap();
        wire["transfer_ledger"] = serde_json::json!({"entries": [record]});
        let error = serde_json::from_value::<DecodeReport>(wire)
            .expect_err("no former outcome shape restores the removed report field");
        assert!(error.to_string().contains("transfer_ledger"), "{error}");
    }
}

#[test]
fn classified_report_wire_requires_its_primary_format() {
    let report = DecodeReport::classified(
        DialectLayers::of(cadmpeg_core::dialect::DialectMatch::admitted(
            cadmpeg_core::dialect_id!("rhino:archive-80"),
        )),
        DecodeTransfer {
            scope: crate::document::ContentScope::full(),
            geometry_transferred: true,
        },
        BTreeMap::new(),
        Vec::new(),
        Vec::new(),
    );
    let golden = serde_json::to_string(&report).unwrap();
    assert_eq!(
        golden,
        r#"{"identity":{"classification":"classified","dialects":{"primary":{"dialect":"rhino:archive-80","admission":"admitted"},"extra":[]}},"transfer":{"scope":{"model":true,"design":true},"geometry_transferred":true},"losses":[],"notes":[]}"#
    );
    assert_eq!(
        serde_json::from_str::<DecodeReport>(&golden).unwrap(),
        report
    );

    let restated = golden.replacen(
        "\"classification\":\"classified\"",
        "\"classification\":\"classified\",\"format\":\"step\"",
        1,
    );
    let error = serde_json::from_str::<DecodeReport>(&restated)
        .expect_err("a classified identity carries no second format");
    assert!(error.to_string().contains("format"), "{error}");
}

#[test]
fn container_only_report_wire_preserves_the_coherent_transfer_state() {
    let report = DecodeReport::unclassified(
        "test",
        DecodeTransfer {
            scope: crate::document::ContentScope::container(),
            geometry_transferred: false,
        },
        BTreeMap::new(),
        Vec::new(),
        Vec::new(),
    );

    let rendered = serde_json::to_string(&report).unwrap();
    assert!(
        rendered.contains("\"scope\":{\"model\":false,\"design\":false}"),
        "{rendered}"
    );
    assert!(
        rendered.contains("\"geometry_transferred\":false"),
        "{rendered}"
    );
    assert_eq!(
        serde_json::from_str::<DecodeReport>(&rendered).unwrap(),
        report
    );
}

#[test]
fn a_decode_transfer_requires_scope_classes_and_geometry_outcome() {
    let transfer = DecodeTransfer {
        scope: crate::ContentScope {
            model: false,
            design: true,
        },
        geometry_transferred: false,
    };
    let value = serde_json::to_value(transfer).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"scope":{"model":false,"design":true},"geometry_transferred":false})
    );
    assert_eq!(
        serde_json::from_value::<DecodeTransfer>(value.clone()).unwrap(),
        transfer
    );
    for key in ["scope", "geometry_transferred"] {
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove(key);
        assert!(serde_json::from_value::<DecodeTransfer>(missing)
            .unwrap_err()
            .to_string()
            .contains(key));
    }
    let mut unknown = value;
    unknown["scope"]["unknown"] = serde_json::json!(true);
    assert!(serde_json::from_value::<DecodeTransfer>(unknown)
        .unwrap_err()
        .to_string()
        .contains("unknown"));
}

#[test]
fn coverage_entry_replaces_its_count() {
    use crate::report::decode::{Coverage, CoverageKey};
    let key = CoverageKey::new("one_count");
    let mut coverage = Coverage::default();
    coverage.record(key, 7);
    coverage.record(key, 8);
    assert_eq!(coverage.get(key.as_str()), Some(&8));
}
