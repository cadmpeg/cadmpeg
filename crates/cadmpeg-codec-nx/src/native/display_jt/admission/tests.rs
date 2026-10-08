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

fn graph_index_work(input: &Value) -> u64 {
    [
        "display_jt_documents",
        "display_jt_segments",
        "display_jt_compressed_elements",
        "display_jt_shape_lod_elements",
        "display_jt_compressed_element_sequences",
    ]
    .into_iter()
    .map(|field| {
        let records = input[field].as_array().expect("fixture arena");
        let rows_work = records
            .iter()
            .map(|record| {
                let id_len = u64::try_from(
                    record["id"].as_str().expect("fixture identity").len(),
                )
                .expect("fixture length fits u64");
                1 + 2 * id_len
            })
            .sum::<u64>();
        if records.is_empty() {
            rows_work
        } else {
            rows_work + first_hash_map_growth_work::<&str, &()>()
        }
    })
    .sum()
}

fn first_hash_map_growth_work<K, V>() -> u64 {
    // reserve_map's first entry bounds a three-slot table, which uses four
    // buckets plus alignment, control bytes, and fixed table metadata.
    let alignment = std::mem::align_of::<(K, V)>().max(16);
    let bytes = 4 * std::mem::size_of::<(K, V)>() + alignment - 1 + 4 + 16;
    u64::try_from(bytes).expect("fixture hash-table bound fits u64")
}

fn toc_index_work(input: &Value) -> u64 {
    let documents = input["display_jt_documents"]
        .as_array()
        .expect("fixture document arena");
    let rows_work = documents
        .iter()
        .map(|document| {
            let document_id_len = u64::try_from(
                document["id"].as_str().expect("fixture identity").len(),
            )
            .expect("fixture length fits u64");
            1 + document["toc_entries"]
                .as_array()
                .expect("fixture TOC entries")
                .iter()
                .map(|entry| {
                    let entry_id_len = u64::try_from(
                        entry["id"].as_str().expect("fixture identity").len(),
                    )
                    .expect("fixture length fits u64");
                    1 + 2 * (document_id_len + entry_id_len)
                })
                .sum::<u64>()
        })
        .sum::<u64>();
    let entries = documents
        .iter()
        .map(|document| {
            document["toc_entries"]
                .as_array()
                .expect("fixture TOC entries")
                .len()
        })
        .sum::<usize>();
    if entries == 0 {
        rows_work
    } else {
        rows_work + first_hash_map_growth_work::<(&str, &str), &()>()
    }
}

fn graph_rejection_work(id: &str, field: &str) -> u64 {
    let code = crate::loss::NxLossCode::DisplayJtGraphRejected.code();
    let output_bytes = [code.len(), ": display_jt ".len(), id.len(), ": ".len(), field.len()]
        .into_iter()
        .try_fold(0usize, usize::checked_add)
        .expect("fixture rejection message length fits usize");
    u64::try_from(output_bytes)
        .expect("fixture rejection message length fits u64")
        .checked_mul(2)
        .expect("formatted work fits u64")
}

#[test]
fn graph_id_index_stops_at_first_duplicate_before_suffix_work() {
    #[derive(Debug)]
    struct Record {
        id: &'static str,
    }

    let records = [
        Record { id: "same-id" },
        Record { id: "same-id" },
        Record { id: "suffix" },
    ];
    let id_len = u64::try_from(records[0].id.len()).expect("fixture length fits u64");
    // Two per-record visits and two keyed hashes for each visited identity.
    let work = 2
        + 4 * id_len
        + first_hash_map_growth_work::<&str, &()>()
        + graph_rejection_work("same-id", "duplicate identity in test records");
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_work_units = work,
        |ctx| {
            let mut storage = ctx.reserve_scoped(0, "index DisplayJT graph records").unwrap();
            let error = super::by_id(
                ctx,
                &mut storage,
                &records,
                |record| record.id,
                "duplicate identity in test records",
            )
            .unwrap_err();
            assert!(error.to_string().contains("duplicate identity in test records"));
            assert_eq!(ctx.resource_refusal(), None);
        },
    );
}

#[test]
fn segment_admission_returns_first_missing_document_before_suffix_work() {
    let mut input = graph_wire();
    input["display_jt_shape_lod_elements"] = json!([]);
    input["display_jt_compressed_elements"] = json!([]);
    input["display_jt_compressed_element_sequences"] = json!([]);
    let mut segments = Vec::new();
    for ordinal in 0..3 {
        let mut segment = input["display_jt_segments"][0].clone();
        segment["id"] = json!(format!("nx:display-jt:segment#prefix-{ordinal}"));
        segment["document"] = json!("x");
        segments.push(segment);
    }
    input["display_jt_segments"] = json!(segments);

    let index_work = graph_index_work(&input) + toc_index_work(&input);
    let prefix_work = index_work
        + 1
        + 1
        + graph_rejection_work(
            "nx:display-jt:segment#prefix-0",
            "document does not resolve",
        );

    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_work_units = index_work,
        |ctx| {
            let raw: DisplayJtGraphWire = serde_json::from_value(input.clone()).unwrap();
            let error = DisplayJtGraph::from_wire_with_context(ctx, raw).unwrap_err();
            assert!(matches!(
                error,
                cadmpeg_ir::native::NativeConvertError::Resource(
                CodecError::ResourceLimit(limit)
                ) if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "admit DisplayJT segments"
                    && limit.additional == 1
            ));
            assert!(ctx.resource_refusal().is_some());
        },
    );

    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_work_units = prefix_work,
        |ctx| {
            let raw: DisplayJtGraphWire = serde_json::from_value(input).unwrap();
            let error = DisplayJtGraph::from_wire_with_context(ctx, raw).unwrap_err();
            assert!(error
                .to_string()
                .contains("nx:display-jt:segment#prefix-0: document does not resolve"));
            assert_eq!(ctx.resource_refusal(), None);
        },
    );
}

#[test]
fn sequence_admission_returns_first_missing_segment_before_suffix_work() {
    let mut input = graph_wire();
    // Keep earlier owner checks empty so the witness targets sequence visits.
    input["display_jt_segments"] = json!([]);
    input["display_jt_shape_lod_elements"] = json!([]);
    input["display_jt_compressed_elements"] = json!([]);
    let mut sequences = Vec::new();
    for ordinal in 0..3 {
        let mut sequence = input["display_jt_compressed_element_sequences"][0].clone();
        sequence["id"] = json!(format!("nx:display-jt:sequence#prefix-{ordinal}"));
        sequence["segment"] = json!("x");
        sequence["elements"] = json!([]);
        sequence["framed_byte_len"] = json!(20);
        sequences.push(sequence);
    }
    input["display_jt_compressed_element_sequences"] = json!(sequences);

    let index_work = graph_index_work(&input) + toc_index_work(&input);
    let prefix_work = index_work
        + 1
        + 1
        + graph_rejection_work(
            "nx:display-jt:sequence#prefix-0",
            "segment does not resolve",
        );

    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_work_units = index_work,
        |ctx| {
            let raw: DisplayJtGraphWire = serde_json::from_value(input.clone()).unwrap();
            let error = DisplayJtGraph::from_wire_with_context(ctx, raw).unwrap_err();
            assert!(matches!(
                error,
                cadmpeg_ir::native::NativeConvertError::Resource(
                CodecError::ResourceLimit(limit)
                ) if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "admit DisplayJT compressed element sequences"
                    && limit.additional == 1
            ));
            assert!(ctx.resource_refusal().is_some());
        },
    );

    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_work_units = prefix_work,
        |ctx| {
            let raw: DisplayJtGraphWire = serde_json::from_value(input).unwrap();
            let error = DisplayJtGraph::from_wire_with_context(ctx, raw).unwrap_err();
            assert!(error
                .to_string()
                .contains("nx:display-jt:sequence#prefix-0: segment does not resolve"));
            assert_eq!(ctx.resource_refusal(), None);
        },
    );
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
fn stored_sequence_loading_stops_at_first_bad_tail_before_suffix_work() {
    const SUFFIX_COUNT: usize = 4_096;
    let bad_wire = graph_wire_with_tail(&cadmpeg_ir::hash::sha256_hex(&[]));
    let one: NativeNamespace = serde_json::from_value(bad_wire.clone()).unwrap();

    let mut long_wire = graph_wire();
    let template = long_wire["display_jt_compressed_element_sequences"][0].clone();
    let mut sequences = vec![bad_wire["display_jt_compressed_element_sequences"][0].clone()];
    for ordinal in 0..SUFFIX_COUNT {
        let mut sequence = template.clone();
        sequence["id"] = json!(format!("nx:display-jt:sequence#suffix-{ordinal}"));
        sequences.push(sequence);
    }
    long_wire["display_jt_compressed_element_sequences"] = json!(sequences);
    let long: NativeNamespace = serde_json::from_value(long_wire).unwrap();

    let reaches_bad_tail = |namespace: &NativeNamespace, work_limit: u64| {
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_work_units = work_limit,
            |ctx| {
                let error = DisplayJtGraph::from_namespace_with_context(ctx, namespace)
                    .expect_err("the first tail digest is invalid");
                if let Some(refusal) = ctx.resource_refusal() {
                    assert_eq!(
                        refusal.dimension,
                        ResourceDimension::WorkUnits,
                        "work limit {work_limit}: {error:?}"
                    );
                    assert!(matches!(&error,
                        cadmpeg_ir::native::NativeConvertError::Resource(
                            CodecError::ResourceLimit(limit)
                        ) if limit == &refusal),
                        "work limit {work_limit}: returned {error:?}, sticky refusal {refusal:?}"
                    );
                    false
                } else {
                    let detail = error.to_string();
                    assert_eq!(
                        detail,
                        "native arena display_jt_compressed_element_sequences: native record \
                         nx:display-jt:sequence#0: DisplayJtCompressedElementSequence.tail_sha256 \
                         disagrees with tail",
                        "work limit {work_limit}: expected the first tail rejection, got {detail}"
                    );
                    true
                }
            },
        )
    };

    // The namespace path admits/indexes records before it checks the tail.
    // Find the one-record semantic threshold from those real core charges,
    // rather than guessing a total or treating the suffix count as a bound.
    let mut upper = 1_u64;
    while !reaches_bad_tail(&one, upper) {
        upper = upper
            .checked_mul(2)
            .expect("one-record tail rejection fits the work-unit counter");
    }
    let mut lower = 0_u64;
    while lower < upper {
        let middle = lower + (upper - lower) / 2;
        if reaches_bad_tail(&one, middle) {
            upper = middle;
        } else {
            lower = middle + 1;
        }
    }
    assert!(lower > 0);
    assert!(reaches_bad_tail(&one, lower));
    assert!(!reaches_bad_tail(&one, lower - 1));
    assert!(reaches_bad_tail(&long, lower));
    assert!(!reaches_bad_tail(&long, lower - 1));
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
