// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::CodecBackend;
use cadmpeg_ir::native::NativeNamespace;
use serde_json::{json, Value};

use super::{DisplayJtGraph, DisplayJtGraphWire};

fn validate_native(ir: &cadmpeg_ir::CadIr) -> Vec<cadmpeg_ir::report::check::Finding> {
    crate::test_support::with_decode_context(|ctx| {
        crate::NxCodec::validate_native(ctx, ir).expect("validation fits service policy")
    })
}

fn graph_wire() -> Value {
    json!({
        "display_jt_documents": [{
            "id": "nx:display-jt:document#0", "index_row": "nx:display-jt:row#0",
            "version_field": format!("{:<80}", "Version 9.5"),
            "format_major": 9, "format_minor": 5, "byte_order": 0,
            "toc_offset": 105, "lsg_segment_id": vec![0; 16],
            "toc_entries": [
                {"id": "nx:display-jt:toc-entry#0", "ordinal": 0, "segment_id": vec![7; 16],
                 "segment_offset": 200, "segment_byte_len": 64, "attributes": [0, 0, 0, 7], "source_offset": 209},
                {"id": "nx:display-jt:toc-entry#1", "ordinal": 1, "segment_id": vec![31; 16],
                 "segment_offset": 264, "segment_byte_len": 64, "attributes": [0, 0, 0, 31], "source_offset": 237}
            ],
            "physical_byte_len": 328, "source_offset": 100
        }],
        "display_jt_segments": [
            {"id": "nx:display-jt:segment#0", "document": "nx:display-jt:document#0", "toc_entry": "nx:display-jt:toc-entry#0",
             "segment_id": vec![7; 16], "segment_type": 7, "segment_byte_len": 64, "payload_sha256": cadmpeg_ir::hash::sha256_hex(b"payload"), "compression": null, "source_offset": 300},
            {"id": "nx:display-jt:segment#1", "document": "nx:display-jt:document#0", "toc_entry": "nx:display-jt:toc-entry#1",
             "segment_id": vec![31; 16], "segment_type": 31, "segment_byte_len": 64, "payload_sha256": cadmpeg_ir::hash::sha256_hex(b"payload"),
             "compression": {"flag": 2, "compressed_data_byte_len": 32, "algorithm": 2, "compressed_byte_len": 31, "inflated_sha256": cadmpeg_ir::hash::sha256_hex(b"inflated")}, "source_offset": 364}
        ],
        "display_jt_shape_lod_elements": [{
            "id": "nx:display-jt:shape-element#0", "segment": "nx:display-jt:segment#0", "ordinal": 0,
            "object_type_id": vec![0; 16], "object_base_type": 4, "object_id": 1,
            "body_byte_len": 1, "body_sha256": cadmpeg_ir::hash::sha256_hex(b"body"), "source_offset": 324
        }],
        "display_jt_compressed_elements": [{
            "id": "nx:display-jt:compressed-element#0", "segment": "nx:display-jt:segment#1", "segment_type": 31,
            "ordinal": 0, "object_type_id": vec![0; 16], "object_base_type": 1, "object_id": 2,
            "body_byte_len": 1, "body_sha256": cadmpeg_ir::hash::sha256_hex(b"body"), "inflated_offset": 0, "source_offset": 388
        }],
        "display_jt_compressed_element_sequences": [{
            "id": "nx:display-jt:sequence#0", "segment": "nx:display-jt:segment#1", "segment_type": 31,
            "elements": ["nx:display-jt:compressed-element#0"], "framed_byte_len": 46,
            "tail": [], "tail_sha256": cadmpeg_ir::hash::sha256_hex(&[]), "source_offset": 388
        }]
    })
}

#[test]
fn aggregate_admission_preserves_native_json() {
    let wire = graph_wire();
    let graph: DisplayJtGraph = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&graph).unwrap(), wire);
    let namespace: NativeNamespace = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(namespace.admit::<DisplayJtGraph>().unwrap(), graph);
    assert_eq!(serde_json::to_value(&namespace).unwrap(), wire);
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.native.0.insert("nx".into(), namespace);
    assert!(validate_native(&ir).is_empty());
}

#[test]
fn graph_index_refuses_before_btree_allocation() {
    let raw: DisplayJtGraphWire = serde_json::from_value(graph_wire()).unwrap();

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_collection_items = 0;
        },
        |ctx| {
            let error = DisplayJtGraph::from_wire_with_context(ctx, raw).unwrap_err();
            assert!(
                matches!(error, cadmpeg_ir::native::NativeConvertError::Resource(
        CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "index DisplayJT graph records")
            );
            let raw: DisplayJtGraphWire = serde_json::from_value(graph_wire()).unwrap();
            crate::test_support::with_decode_context(|service| {
                assert!(DisplayJtGraph::from_wire_with_context(service, raw).is_ok());
            });
        },
    );
}

#[test]
fn graph_toc_index_refuses_before_btree_allocation() {
    let raw: DisplayJtGraphWire = serde_json::from_value(graph_wire()).unwrap();

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_collection_items = 6;
        },
        |ctx| {
            let error = DisplayJtGraph::from_wire_with_context(ctx, raw).unwrap_err();
            assert!(
                matches!(error, cadmpeg_ir::native::NativeConvertError::Resource(
        CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "index DisplayJT TOC entries")
            );
            let raw: DisplayJtGraphWire = serde_json::from_value(graph_wire()).unwrap();
            crate::test_support::with_decode_context(|service| {
                assert!(DisplayJtGraph::from_wire_with_context(service, raw).is_ok());
            });
        },
    );
}

#[test]
fn graph_index_refuses_scoped_limit() {
    let raw: DisplayJtGraphWire = serde_json::from_value(graph_wire()).unwrap();

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_materialized_bytes = 0;
        },
        |ctx| {
            let error = DisplayJtGraph::from_wire_with_context(ctx, raw).unwrap_err();
            assert!(
                matches!(error, cadmpeg_ir::native::NativeConvertError::Resource(
        CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::MaterializedBytes)
            );
        },
    );
}

#[test]
fn graph_index_refuses_work_limit() {
    let raw: DisplayJtGraphWire = serde_json::from_value(graph_wire()).unwrap();

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_work_units = 0;
        },
        |ctx| {
            let error = DisplayJtGraph::from_wire_with_context(ctx, raw).unwrap_err();
            assert!(
                matches!(error, cadmpeg_ir::native::NativeConvertError::Resource(
        CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::WorkUnits
        && limit.operation == "index DisplayJT graph records")
            );
        },
    );
}

#[test]
fn graph_rejection_identity_refuses_retained_limit() {
    let mut wire = graph_wire();
    wire["display_jt_segments"][0]["document"] = json!("nx:display-jt:document#missing");

    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::RetainedBytes,
        "retain DisplayJT graph rejection",
        |ctx| {
            let raw: DisplayJtGraphWire = serde_json::from_value(wire.clone()).unwrap();
            DisplayJtGraph::from_wire_with_context(ctx, raw).map_err(|error| match error {
                cadmpeg_ir::native::NativeConvertError::Resource(error) => error,
                error => panic!("unexpected graph refusal: {error}"),
            })
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "retain DisplayJT graph rejection")
    );
    let raw: DisplayJtGraphWire = serde_json::from_value(wire).unwrap();
    crate::test_support::with_decode_context(|service| {
        let error = DisplayJtGraph::from_wire_with_context(service, raw).unwrap_err();
        assert!(
            matches!(error, cadmpeg_ir::native::NativeConvertError::InvalidCollection(message)
        if message.contains("nx:display-jt:segment#0: document does not resolve"))
        );
    });
}

#[test]
fn graph_native_reader_refuses_collection_limit() {
    let namespace: NativeNamespace = serde_json::from_value(graph_wire()).unwrap();

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_collection_items = 0;
        },
        |ctx| {
            let error = DisplayJtGraph::from_namespace_with_context(ctx, &namespace).unwrap_err();
            assert!(
                matches!(error, cadmpeg_ir::native::NativeConvertError::Resource(
        CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::CollectionItems)
            );
        },
    );
}

#[test]
fn graph_native_reader_refuses_retained_limit() {
    let namespace: NativeNamespace = serde_json::from_value(graph_wire()).unwrap();

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_retained_bytes = 0;
        },
        |ctx| {
            let error = DisplayJtGraph::from_namespace_with_context(ctx, &namespace).unwrap_err();
            assert!(
                matches!(error, cadmpeg_ir::native::NativeConvertError::Resource(
        CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes)
            );
        },
    );
}

#[test]
fn display_jt_native_validation_propagates_resource_limit() {
    let namespace: NativeNamespace = serde_json::from_value(graph_wire()).unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.native.0.insert("nx".into(), namespace);

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_work_units = 0;
        },
        |ctx| {
            let error = crate::NxCodec::validate_native(ctx, &ir).unwrap_err();
            assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "construct canonical native value"));
            crate::test_support::with_decode_context(|service| {
                assert!(crate::NxCodec::validate_native(service, &ir)
                    .unwrap()
                    .is_empty());
            });
        },
    );
}

#[test]
fn display_jt_native_arena_refuses_before_value_clone() {
    let namespace: NativeNamespace = serde_json::from_value(graph_wire()).unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.native.0.insert("nx".into(), namespace);

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_materialized_bytes = 0;
        },
        |ctx| {
            let error = crate::NxCodec::validate_native(ctx, &ir).unwrap_err();
            assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes
            && limit.operation == "load typed native record"));
            crate::test_support::with_decode_context(|service| {
                assert!(crate::NxCodec::validate_native(service, &ir)
                    .unwrap()
                    .is_empty());
            });
        },
    );
}

#[test]
fn aggregate_admission_rejects_missing_owners_and_repeated_field_disagreement() {
    for (path, value, field) in [
        (
            "/display_jt_segments/0/document",
            json!("nx:display-jt:document#missing"),
            "document",
        ),
        (
            "/display_jt_segments/0/toc_entry",
            json!("nx:display-jt:toc-entry#missing"),
            "toc_entry",
        ),
        (
            "/display_jt_segments/0/segment_id",
            json!(vec![9; 16]),
            "segment_id",
        ),
        (
            "/display_jt_segments/0/segment_type",
            json!(31),
            "segment_type",
        ),
        (
            "/display_jt_segments/0/segment_byte_len",
            json!(65),
            "segment_byte_len",
        ),
        (
            "/display_jt_segments/0/source_offset",
            json!(301),
            "source_offset",
        ),
        (
            "/display_jt_shape_lod_elements/0/segment",
            json!("nx:display-jt:segment#missing"),
            "segment",
        ),
        (
            "/display_jt_shape_lod_elements/0/segment",
            json!("nx:display-jt:segment#1"),
            "type-7",
        ),
        (
            "/display_jt_compressed_elements/0/segment",
            json!("nx:display-jt:segment#missing"),
            "segment",
        ),
        (
            "/display_jt_compressed_elements/0/segment",
            json!("nx:display-jt:segment#0"),
            "compression",
        ),
        (
            "/display_jt_compressed_elements/0/segment_type",
            json!(7),
            "segment_type",
        ),
        (
            "/display_jt_compressed_elements/0/source_offset",
            json!(389),
            "source_offset",
        ),
        (
            "/display_jt_compressed_element_sequences/0/segment",
            json!("nx:display-jt:segment#missing"),
            "segment",
        ),
        (
            "/display_jt_compressed_element_sequences/0/segment_type",
            json!(7),
            "segment_type",
        ),
        (
            "/display_jt_compressed_element_sequences/0/elements/0",
            json!("nx:display-jt:compressed-element#missing"),
            "elements",
        ),
        (
            "/display_jt_compressed_elements/0/ordinal",
            json!(1),
            "ordinal",
        ),
        (
            "/display_jt_compressed_elements/0/body_byte_len",
            json!(2),
            "body_byte_len",
        ),
        (
            "/display_jt_compressed_elements/0/inflated_offset",
            json!(1),
            "inflated_offset",
        ),
        (
            "/display_jt_compressed_element_sequences/0/framed_byte_len",
            json!(47),
            "framed_byte_len",
        ),
    ] {
        let mut wire = graph_wire();
        *wire.pointer_mut(path).unwrap() = value;
        let raw: DisplayJtGraphWire = serde_json::from_value(wire.clone()).unwrap();
        assert!(
            DisplayJtGraph::try_from(raw)
                .unwrap_err()
                .to_string()
                .contains(field),
            "{path}"
        );
        assert!(
            serde_json::from_value::<DisplayJtGraph>(wire.clone())
                .unwrap_err()
                .to_string()
                .contains(field),
            "{path}"
        );
        let namespace: NativeNamespace = serde_json::from_value(wire).unwrap();
        assert!(
            namespace
                .admit::<DisplayJtGraph>()
                .unwrap_err()
                .to_string()
                .contains(field),
            "{path}"
        );
        let mut ir = cadmpeg_ir::CadIr::empty();
        ir.native.0.insert("nx".into(), namespace);
        let findings = validate_native(&ir);
        assert_eq!(findings.len(), 1, "{path}");
        assert!(findings[0].message.contains(field), "{path}");
    }
}

#[test]
fn compressed_element_wire_rejects_invalid_lengths_and_tail_hash() {
    for (path, value, field) in [
        (
            "/display_jt_compressed_elements/0/body_byte_len",
            json!(u32::MAX),
            "body_byte_len",
        ),
        (
            "/display_jt_compressed_element_sequences/0/framed_byte_len",
            json!(19),
            "framed_byte_len",
        ),
        (
            "/display_jt_compressed_element_sequences/0/tail_sha256",
            json!(cadmpeg_ir::hash::sha256_hex(&[1])),
            "tail_sha256",
        ),
    ] {
        let mut wire = graph_wire();
        *wire.pointer_mut(path).unwrap() = value;
        assert!(serde_json::from_value::<DisplayJtGraph>(wire)
            .unwrap_err()
            .to_string()
            .contains(field));
    }
}

fn graph_wire_with_tail(tail_sha256: &str) -> Value {
    let mut wire = graph_wire();
    wire["display_jt_compressed_element_sequences"][0]["tail"] = json!([6, 5]);
    wire["display_jt_compressed_element_sequences"][0]["tail_sha256"] = json!(tail_sha256);
    wire
}

#[test]
fn stored_sequence_tail_digest_is_charged_before_hashing() {
    let namespace: NativeNamespace =
        serde_json::from_value(graph_wire_with_tail(&cadmpeg_ir::hash::sha256_hex(&[6, 5])))
            .unwrap();
    let operation = "check DisplayJT sequence tail digest";
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        operation,
        |ctx| {
            DisplayJtGraph::from_namespace_with_context(ctx, &namespace).map_err(CodecError::from)
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation));
}

#[test]
fn stored_sequence_tail_digest_mismatch_reads_as_the_serialized_rejection() {
    let wire = graph_wire_with_tail(&cadmpeg_ir::hash::sha256_hex(&[]));
    let namespace: NativeNamespace = serde_json::from_value(wire.clone()).unwrap();
    let expected = serde_json::from_value::<DisplayJtGraph>(wire)
        .unwrap_err()
        .to_string();
    assert!(expected.contains("tail_sha256 disagrees with tail"));
    crate::test_support::with_decode_context(|ctx| {
        let error = DisplayJtGraph::from_namespace_with_context(ctx, &namespace).unwrap_err();
        assert_eq!(
            error.to_string(),
            "native arena display_jt_compressed_element_sequences: native record \
             nx:display-jt:sequence#0: DisplayJtCompressedElementSequence.tail_sha256 \
             disagrees with tail"
        );
    });
}

fn native_resource(error: cadmpeg_ir::native::NativeConvertError) -> CodecError {
    match error { cadmpeg_ir::native::NativeConvertError::Resource(error) => error, error => panic!("unexpected refusal: {error}") }
}

#[test]
fn graph_duplicate_identity_does_not_visit_the_remaining_records() {
    let mut records = vec!["duplicate".to_owned(); 2];
    let measure = |records: &[String]| crate::test_support::with_decode_context(|ctx| {
        let mut storage = ctx.reserve_scoped(0, "test graph index").unwrap();
        assert!(matches!(super::by_id(ctx, &mut storage, records, String::as_str, "duplicate identity"), Err(cadmpeg_ir::native::NativeConvertError::InvalidCollection(_))));
        let CodecError::ResourceLimit(limit) = ctx.charge_work(u64::MAX, "measure admission work").unwrap_err() else { panic!("work measurement"); }; limit.used
    });
    let prefix = measure(&records); records.extend(vec!["suffix".to_owned(); 128]);
    assert_eq!(prefix, measure(&records));
}

#[test]
fn graph_duplicate_toc_does_not_visit_the_remaining_entries() {
    let measure = |extra| crate::test_support::with_decode_context(|ctx| {
        let mut raw: DisplayJtGraphWire = serde_json::from_value(graph_wire()).unwrap();
        let first = raw.documents[0].toc_entries[0].clone();
        raw.documents[0].toc_entries = vec![first.clone(), first.clone()];
        raw.documents[0].toc_entries.extend(vec![first; extra]);
        assert!(matches!(DisplayJtGraph::from_wire_with_context(ctx, raw), Err(cadmpeg_ir::native::NativeConvertError::InvalidCollection(_))));
        let CodecError::ResourceLimit(limit) = ctx.charge_work(u64::MAX, "measure admission work").unwrap_err() else { panic!("work measurement"); }; limit.used
    });
    assert_eq!(measure(0), measure(128));
}

#[test]
fn graph_missing_owners_admit_one_record_before_rejection() {
    for (arena, operation) in [
        ("display_jt_segments", "admit DisplayJT segments"),
        ("display_jt_shape_lod_elements", "admit DisplayJT shape elements"),
        ("display_jt_compressed_elements", "admit DisplayJT compressed elements"),
        ("display_jt_compressed_element_sequences", "admit DisplayJT compressed element sequences"),
    ] {
        let mut wire = graph_wire(); let source = wire[arena][0].clone();
        let mut records = Vec::new();
        for ordinal in 0..129 { let mut record = source.clone(); record["id"] = json!(format!("nx:display-jt:record#{ordinal}")); records.push(record); }
        let field = if arena == "display_jt_segments" { "document" } else { "segment" };
        records[0][field] = json!("nx:display-jt:owner#missing"); wire[arena] = json!(records);
        let error = crate::test_support::resource_refusal_at(&[], ResourceDimension::WorkUnits, operation, |ctx| {
            let raw = serde_json::from_value(wire.clone()).unwrap();
            DisplayJtGraph::from_wire_with_context(ctx, raw).map_err(native_resource)
        });
        let CodecError::ResourceLimit(limit) = error else { panic!("owner visit refusal"); };
        assert_eq!(limit.additional, 1, "{arena}");
        crate::test_support::with_decode_context(|ctx| { let raw = serde_json::from_value(wire).unwrap(); assert!(matches!(DisplayJtGraph::from_wire_with_context(ctx, raw), Err(cadmpeg_ir::native::NativeConvertError::InvalidCollection(_)))); });
    }
}

#[test]
fn graph_unresolved_sequence_element_admits_one_identity_before_rejection() {
    let mut wire = graph_wire(); wire["display_jt_compressed_element_sequences"][0]["elements"] = json!(vec!["nx:display-jt:element#missing"; 129]);
    wire["display_jt_compressed_element_sequences"][0]["framed_byte_len"] = json!(129 * 25 + 20);
    let error = crate::test_support::resource_refusal_at(&[], ResourceDimension::WorkUnits, "admit DisplayJT sequence elements", |ctx| {
        let raw = serde_json::from_value(wire.clone()).unwrap();
        DisplayJtGraph::from_wire_with_context(ctx, raw).map_err(native_resource)
    });
    let CodecError::ResourceLimit(limit) = error else { panic!("sequence visit refusal"); };
    assert_eq!(limit.additional, 1);
}

#[test]
fn graph_rejection_charges_the_long_identity_copy() {
    let id = "x".repeat(4096);
    let error = crate::test_support::resource_refusal_at(&[], ResourceDimension::WorkUnits, "retain DisplayJT graph rejection", |ctx| {
        match super::invalid(ctx, &id, "document does not resolve") {
            cadmpeg_ir::native::NativeConvertError::Resource(error) => Err(error),
            cadmpeg_ir::native::NativeConvertError::InvalidCollection(message) => { assert!(message.contains(&id)); Ok(()) },
            error => panic!("unexpected diagnostic: {error}"),
        }
    });
    assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "retain DisplayJT graph rejection" && limit.additional == 4096));
    let measure = |id: &str| crate::test_support::with_decode_context(|ctx| {
        let cadmpeg_ir::native::NativeConvertError::InvalidCollection(message) = super::invalid(ctx, id, "document does not resolve") else { panic!("expected graph diagnostic"); };
        let code = crate::loss::NxLossCode::DisplayJtGraphRejected.code();
        assert_eq!(message, format!("{code}: display_jt {id}: document does not resolve"));
        let CodecError::ResourceLimit(limit) = ctx.charge_work(u64::MAX, "measure diagnostic work").unwrap_err() else { panic!("work measurement"); }; limit.used
    });
    // The diagnostic copies each input identity once through the core append.
    assert_eq!(measure(&id) - measure("x"), 4096 - 1);
}

#[test]
fn graph_invalid_sequence_wire_does_not_visit_the_remaining_records() {
    let measure = |extra| crate::test_support::with_decode_context(|ctx| {
        let mut wire = graph_wire(); wire["display_jt_compressed_element_sequences"][0]["tail_sha256"] = json!(cadmpeg_ir::hash::sha256_hex(b"wrong"));
        let record = wire["display_jt_compressed_element_sequences"][0].clone();
        let mut records = vec![record.clone()]; records.extend(vec![record; extra]);
        wire["display_jt_compressed_element_sequences"] = json!(records);
        let namespace = serde_json::from_value(wire).unwrap();
        assert!(super::sequences_for_decode(ctx, &namespace).is_err());
        let CodecError::ResourceLimit(limit) = ctx.charge_work(u64::MAX, "measure admission work").unwrap_err() else { panic!("work measurement"); }; limit.used
    });
    assert_eq!(measure(0), measure(128));
}

#[test]
fn graph_duplicate_toc_admits_one_document_before_its_entries() {
    let error = crate::test_support::resource_refusal_at(&[], ResourceDimension::WorkUnits, "index DisplayJT TOC entries", |ctx| {
        let mut raw: DisplayJtGraphWire = serde_json::from_value(graph_wire()).unwrap();
        let mut first = raw.documents[0].clone(); first.toc_entries.push(first.toc_entries[0].clone());
        raw.documents = (0..129).map(|ordinal| { let mut document = first.clone(); document.id = format!("nx:display-jt:document#{ordinal}"); document }).collect();
        DisplayJtGraph::from_wire_with_context(ctx, raw).map_err(native_resource)
    });
    let CodecError::ResourceLimit(limit) = error else { panic!("document visit refusal"); };
    assert_eq!(limit.additional, 1);
}
