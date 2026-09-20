// SPDX-License-Identifier: Apache-2.0
use cadmpeg_test_support::wire;

use crate::report::decode::{DecodeReport, DecodeTransfer, TransferLedger};
use cadmpeg_core::dialect::DialectLayers;
use std::collections::BTreeMap;

use crate::report::decode::{TransferDisposition, TransferOutcome, TransferRecord};

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
}

#[test]
fn transfer_record_keeps_the_nested_outcome_wire_shape() {
    let record = TransferRecord {
        source: "D1".into(),
        outcome: TransferOutcome::Retained {
            target: "iges:entity:directory#1".into(),
            note: Some("retained".into()),
        },
    };
    let wire = serde_json::to_value(&record).expect("transfer record serializes");
    assert_eq!(
        wire,
        serde_json::json!({
            "source": "D1",
            "outcome": {
                "disposition": "retained",
                "target": "iges:entity:directory#1",
                "note": "retained"
            }
        })
    );
    assert_eq!(
        serde_json::from_value::<TransferRecord>(wire).expect("transfer record deserializes"),
        record
    );
}

#[test]
fn transfer_record_wire_rejects_disposition_target_disagreement() {
    // The disposition is the tag, so a target and a note exist only on the
    // dispositions that carry them: each disagreement is a missing or an
    // unknown field, not a value a reader has to refuse.
    for wire in [
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
    ] {
        assert!(serde_json::from_value::<TransferRecord>(wire).is_err());
    }

    let omitted: TransferRecord = serde_json::from_value(serde_json::json!({
        "source": "D2",
        "outcome": {"disposition": "omitted", "note": "unsupported"}
    }))
    .expect("omitted record has no target");
    assert_eq!(omitted.target(), None);
    assert_eq!(
        wire::field::<crate::report::decode::TransferDisposition>(
            &(omitted.outcome),
            "disposition"
        ),
        TransferDisposition::Omitted
    );
    assert_eq!(
        wire::field_or_default::<Option<String>>(&(omitted.outcome), "note").as_deref(),
        Some("unsupported")
    );
}

#[test]
fn classified_report_wire_requires_its_primary_format() {
    let report = DecodeReport::classified(
        DialectLayers::of(cadmpeg_core::dialect::DialectMatch::admitted(
            cadmpeg_core::dialect_id!("rhino:archive-80"),
        )),
        DecodeTransfer::full(true),
        BTreeMap::new(),
        Vec::new(),
        Vec::new(),
        TransferLedger::default(),
    );
    let golden = serde_json::to_string(&report).unwrap();
    assert_eq!(
        golden,
        r#"{"identity":{"classification":"classified","dialects":{"primary":{"dialect":"rhino:archive-80","admission":"admitted"},"extra":[]}},"transfer":{"transfer":"full","geometry_transferred":true},"losses":[],"notes":[]}"#
    );
    assert_eq!(
        serde_json::from_str::<DecodeReport>(&golden).unwrap(),
        report
    );

    let contradictory = golden.replacen(
        "\"transfer\":\"full\"",
        "\"transfer\":\"container_only\"",
        1,
    );
    let error = serde_json::from_str::<DecodeReport>(&contradictory)
        .expect_err("a container-only report has no geometry outcome to state");
    assert!(
        error.to_string().contains("geometry_transferred"),
        "{error}"
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
        DecodeTransfer::ContainerOnly {},
        BTreeMap::new(),
        Vec::new(),
        Vec::new(),
        TransferLedger::default(),
    );

    let rendered = serde_json::to_string(&report).unwrap();
    assert!(
        rendered.contains("\"transfer\":\"container_only\""),
        "{rendered}"
    );
    assert!(!rendered.contains("geometry_transferred"), "{rendered}");
    assert_eq!(
        serde_json::from_str::<DecodeReport>(&rendered).unwrap(),
        report
    );
}

#[test]
fn a_decode_transfer_states_its_scope_and_carries_only_its_own_keys() {
    let base = serde_json::json!({
        "identity": {"classification": "unclassified", "format": "rhino"},
        "transfer": {"transfer": "full", "geometry_transferred": true},
        "losses": [],
        "notes": [],
    });
    let report = serde_json::from_value::<DecodeReport>(base.clone()).expect("a full decode");
    assert_eq!(report.transfer, DecodeTransfer::full(true));
    assert_eq!(
        serde_json::to_value(&report).unwrap()["transfer"]["transfer"],
        "full"
    );

    let mut container_only = base.clone();
    container_only["transfer"]["transfer"] = serde_json::json!("container_only");
    let error = serde_json::from_value::<DecodeReport>(container_only.clone())
        .expect_err("a container-only report has no geometry outcome");
    assert!(
        error.to_string().contains("geometry_transferred"),
        "{error}"
    );

    container_only["transfer"]
        .as_object_mut()
        .expect("map")
        .remove("geometry_transferred");
    let report = serde_json::from_value::<DecodeReport>(container_only).expect("container only");
    assert_eq!(report.transfer, DecodeTransfer::ContainerOnly {});
    let wire = serde_json::to_value(&report).unwrap();
    assert_eq!(wire["transfer"]["transfer"], "container_only");
    assert!(wire["transfer"].get("geometry_transferred").is_none());

    let mut without_outcome = base;
    without_outcome["transfer"]
        .as_object_mut()
        .expect("map")
        .remove("geometry_transferred");
    let error = serde_json::from_value::<DecodeReport>(without_outcome)
        .expect_err("a full decode states its geometry outcome");
    assert!(
        error.to_string().contains("geometry_transferred"),
        "{error}"
    );
}
