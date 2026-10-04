// SPDX-License-Identifier: Apache-2.0
use cadmpeg_test_support::wire;

use crate::report::decode::{DecodeReport, DecodeTransfer, TransferLedger};
use cadmpeg_core::dialect::DialectLayers;
use std::collections::BTreeMap;

use crate::report::decode::{TransferDisposition, TransferOutcome, TransferRecord};

#[test]
fn admitted_coverage_refuses_new_node_and_static_name_limits() {
    use crate::report::decode::{Coverage, CoverageKey};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let key = CoverageKey::new("one_count");
    for (items, bytes, dimension, operation) in [
        (
            0,
            u64::MAX,
            ResourceDimension::CollectionItems,
            "decode coverage nodes",
        ),
        (
            1,
            0,
            ResourceDimension::RetainedBytes,
            "decode coverage names",
        ),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = items;
        policy.limits.max_retained_bytes = bytes;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let error = Coverage::default()
            .record(&ctx, key, 7)
            .expect_err("below-need coverage cap");
        assert!(matches!(error, CodecError::ResourceLimit(resource)
            if resource.dimension == dimension && resource.operation == operation));
    }
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let mut coverage = Coverage::default();
    coverage.record(&ctx, key, 7).expect("service node");
    coverage.record(&ctx, key, 8).expect("existing node");
    assert_eq!(coverage.get("one_count"), Some(&8));
}

#[test]
fn admitted_indexed_coverage_refuses_temporary_name_and_new_node() {
    use crate::report::decode::{Coverage, IndexedCoverageKey};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let key = IndexedCoverageKey::decimal("type_", "_count");
    for (materialized, items, dimension, operation) in [
        (
            0,
            1,
            ResourceDimension::MaterializedBytes,
            "decode indexed coverage name",
        ),
        (
            u64::MAX,
            0,
            ResourceDimension::CollectionItems,
            "decode coverage nodes",
        ),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = materialized;
        policy.limits.max_collection_items = items;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let error = Coverage::default()
            .record_indexed(&ctx, key, 4, 7)
            .expect_err("below-need coverage cap");
        assert!(matches!(error, CodecError::ResourceLimit(resource)
            if resource.dimension == dimension && resource.operation == operation));
    }
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let mut coverage = Coverage::default();
    coverage
        .record_indexed(&ctx, key, 4, 7)
        .expect("service node");
    assert_eq!(coverage.get("type_4_count"), Some(&7));
}

#[test]
fn admitted_hex_coverage_refuses_retained_name_limit() {
    use crate::report::decode::{Coverage, HexByteCoverageKey};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let key = HexByteCoverageKey::new("type_", "_count");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let error = Coverage::default()
        .record_hex_byte(&ctx, key, 0x0a, 7)
        .expect_err("retained name exceeds cap");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "decode hexadecimal coverage name"));
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let mut coverage = Coverage::default();
    coverage
        .record_hex_byte(&ctx, key, 0x0a, 7)
        .expect("service node");
    assert_eq!(coverage.get("type_0a_count"), Some(&7));
}

#[test]
fn owned_coverage_record_preserves_declared_key() {
    let key = crate::report::decode::CoverageKey::new("decoded_entities");
    let mut coverage = crate::report::decode::Coverage::default();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("service context");
    assert!(coverage
        .record_owned(&ctx, key, "other_entities".to_owned(), 3)
        .is_err());
    assert!(coverage.is_empty());
    coverage
        .record_owned(&ctx, key, "decoded_entities".to_owned(), 3)
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

#[test]
fn coverage_entry_refuses_retained_map_storage() {
    use crate::report::decode::{Coverage, CoverageKey};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let key = CoverageKey::new("one_count");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Node storage follows the key copy and includes all backing lanes.
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "decode coverage nodes",
        |cap| {
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            Coverage::default().record(&ctx, key, 7)
        },
    );
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("coverage storage must refuse");
    };
    policy.limits.max_retained_bytes = limit.used + limit.additional - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let mut coverage = Coverage::default();
    assert!(
        matches!(coverage.record(&ctx, key, 7), Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "decode coverage nodes")
    );
    assert!(coverage.is_empty());
    policy.limits.max_retained_bytes += 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    coverage.record(&ctx, key, 7).expect("one admitted entry");
    coverage
        .record(&ctx, key, 8)
        .expect("replacement makes no allocation");
    assert_eq!(coverage.get(key.as_str()), Some(&8));
}

#[test]
fn coverage_entry_refuses_lookup_work() {
    use crate::report::decode::{Coverage, CoverageKey};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let mut coverage = Coverage::default();
    assert!(
        matches!(coverage.record(&ctx, CoverageKey::new("one_count"), 7), Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "decode coverage lookup")
    );
    assert!(coverage.is_empty());
}
