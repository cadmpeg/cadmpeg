use crate::test_support::test_prt::assembly_with_external_paths;
use crate::test_support::test_prt::prt_with_named_payloads;
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn external_reference_extraction_refuses_record_collection_limit() {
    use cadmpeg_core::decode::ResourceDimension;

    let file = assembly_with_external_paths();
    let container =
        crate::test_support::with_decode_context(|ctx| crate::container::scan_bytes(ctx, file))
            .expect("external reference container");

    // Two strings enter both the parsed table and the container result before
    // extraction admits the two native records.

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_collection_items = 5;
        },
        |ctx| {
            let error = super::super::external_references(ctx, &container)
                .expect_err("two native records exceed the remaining collection item");
            assert!(matches!(
                error,
                cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.dimension == ResourceDimension::CollectionItems
                        && limit.operation == "nx external references"
            ));
        },
    );
}

fn native_external_record_result(
    configure: impl FnOnce(&mut DecodePolicy),
) -> Result<Vec<super::super::ExternalReferenceRecord>, CodecError> {
    let file = prt_with_named_payloads(&[(
        "/Root/ExternalReferences",
        crate::test_support::test_streams::external_reference_stream(),
    )]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("indexed external-reference container");

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| super::super::external_reference_records(ctx, &container),
    )
}

#[test]
fn native_external_record_route_preserves_indexed_record() {
    let records = native_external_record_result(|_| {}).expect("native indexed record");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].record_id, 6);
}

#[test]
fn native_external_record_route_refuses_collection_limit() {
    let error = native_external_record_result(|policy| policy.limits.max_collection_items = 10)
        .expect_err("native record exceeds the parsed collection budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "nx native external reference records"),
        "{error:?}"
    );
}

#[test]
fn native_external_record_route_refuses_retained_limit() {
    let error = native_external_record_result(|policy| policy.limits.max_retained_bytes = 0)
        .expect_err("indexed record exceeds the retained budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes),
        "{error:?}"
    );
}

#[test]
fn native_external_record_route_charges_each_parser_tuple_visit() {
    let payload = crate::test_support::test_streams::external_reference_handle_sets(9);
    let file = prt_with_named_payloads(&[("/Root/ExternalReferences", payload)]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("multi-record external-reference container");
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "nx native external reference records",
        |ctx| {
            super::super::external_reference_records(ctx, &container).map(|_| ())
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("native record projection must refuse at a tuple visit");
    };
    assert_eq!(limit.operation, "nx native external reference records");
    assert_eq!(limit.additional, 1);
}

fn native_external_indexed_result(
    configure: impl FnOnce(&mut DecodePolicy),
) -> Result<Vec<super::super::ExternalReferenceIndexedRecord>, CodecError> {
    let file = prt_with_named_payloads(&[(
        "/Root/ExternalReferences",
        crate::test_support::test_streams::external_reference_stream(),
    )]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("indexed external-reference container");
    let decoded = crate::test_support::with_decode_context(|ctx| {
        super::super::external_reference_records(ctx, &container)
    })
    .expect("handle-set record");

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| super::super::external_reference_indexed_records(ctx, &container, &decoded),
    )
}

#[test]
fn native_external_indexed_route_preserves_record_links() {
    let records = native_external_indexed_result(|_| {}).expect("native indexed records");
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].record_id, 7);
    assert_eq!(records[1].record_id, 6);
    assert!(records[0].handle_set_record.is_none());
    assert_eq!(
        records[1].handle_set_record.as_deref(),
        Some("nx:external-reference-record:/Root/ExternalReferences#6")
    );
}

#[test]
fn native_external_indexed_route_refuses_collection_limit() {
    let error = native_external_indexed_result(|policy| policy.limits.max_collection_items = 9)
        .expect_err("native indexed record exceeds collection budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "nx native external reference indexed records"),
        "{error:?}"
    );
}

#[test]
fn native_external_indexed_route_refuses_retained_limit() {
    let error = native_external_indexed_result(|policy| policy.limits.max_retained_bytes = 0)
        .expect_err("indexed record exceeds retained budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes),
        "{error:?}"
    );
}

#[test]
fn native_external_indexed_route_refuses_scoped_limit() {
    let error = native_external_indexed_result(|policy| policy.limits.max_materialized_bytes = 0)
        .expect_err("decoded index exceeds scoped budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes
            && limit.operation == "nx external reference decoded index"),
        "{error:?}"
    );
}

#[test]
fn native_external_indexed_route_refuses_work_limit() {
    let error = native_external_indexed_result(|policy| policy.limits.max_work_units = 0)
        .expect_err("indexed scan exceeds work budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits),
        "{error:?}"
    );
}

#[test]
fn native_external_indexed_route_charges_each_parser_tuple_visit() {
    let payload = crate::test_support::test_streams::external_reference_handle_sets(9);
    let file = prt_with_named_payloads(&[("/Root/ExternalReferences", payload)]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("multi-record external-reference container");
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "NX OM parsed visits",
        |ctx| {
            super::super::external_reference_indexed_records(ctx, &container, &[]).map(|_| ())
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("native indexed projection must refuse at a tuple visit");
    };
    assert_eq!(limit.operation, "NX OM parsed visits");
    assert_eq!(limit.additional, 1);
}

fn native_external_empty_result(
    configure: impl FnOnce(&mut DecodePolicy),
) -> Result<Vec<super::super::ExternalReferenceEmptyRecord>, CodecError> {
    let file = prt_with_named_payloads(&[(
        "/Root/ExternalReferences",
        crate::test_support::test_streams::external_reference_stream(),
    )]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("indexed external-reference container");
    let indexed = crate::test_support::with_decode_context(|ctx| {
        let records = super::super::external_reference_records(ctx, &container)?;
        super::super::external_reference_indexed_records(ctx, &container, &records)
    })
    .expect("native indexed records");

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| super::super::external_reference_empty_records(ctx, &container, &indexed),
    )
}

#[test]
fn native_external_empty_route_preserves_record() {
    let records = native_external_empty_result(|_| {}).expect("native empty record");
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].id,
        "nx:external-reference-empty-record:/Root/ExternalReferences#7"
    );
    assert!(!records[0].closing_marker);
}

#[test]
fn native_external_empty_route_charges_each_indexed_visit() {
    let file = prt_with_named_payloads(&[(
        "/Root/ExternalReferences",
        crate::test_support::test_streams::external_reference_stream(),
    )]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("indexed external-reference container");
    let indexed = crate::test_support::with_decode_context(|ctx| -> Result<_, CodecError> {
        let records = super::super::external_reference_records(ctx, &container)?;
        super::super::external_reference_indexed_records(ctx, &container, &records)
    })
    .expect("native indexed records");
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "NX external reference indexed records",
        |ctx| {
            super::super::external_reference_empty_records(ctx, &container, &indexed).map(|_| ())
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("native empty projection must refuse at an indexed visit");
    };
    assert_eq!(limit.operation, "NX external reference indexed records");
    assert_eq!(limit.additional, 1);
}

#[test]
fn native_external_empty_route_refuses_collection_limit() {
    let error = native_external_empty_result(|policy| policy.limits.max_collection_items = 0)
        .expect_err("native empty record exceeds collection budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "nx native external reference empty records"),
        "{error:?}"
    );
}

#[test]
fn native_external_empty_route_refuses_retained_limit() {
    let error = native_external_empty_result(|policy| policy.limits.max_retained_bytes = 0)
        .expect_err("native empty record exceeds retained budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "nx native external reference empty records"),
        "{error:?}"
    );
}

fn native_external_tail_result(
    configure: impl FnOnce(&mut DecodePolicy),
) -> Result<Vec<super::super::ExternalReferenceTailReferencePair>, CodecError> {
    let file = prt_with_named_payloads(&[(
        "/Root/ExternalReferences",
        crate::test_support::test_streams::external_reference_stream(),
    )]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("indexed external-reference container");
    let records = crate::test_support::with_decode_context(|ctx| {
        super::super::external_reference_records(ctx, &container)
    })
    .expect("handle-set record");

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| super::super::external_reference_tail_reference_pairs(ctx, &container, &records),
    )
}

#[test]
fn native_external_tail_route_preserves_pair() {
    let pairs = native_external_tail_result(|_| {}).expect("external tail pair");
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].ordinal, 0);
    assert_eq!(pairs[0].persistent_handle, 5);
}

#[test]
fn native_external_tail_route_charges_each_record_visit() {
    let payload = crate::test_support::test_streams::external_reference_handle_sets(9);
    let file = prt_with_named_payloads(&[("/Root/ExternalReferences", payload)]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("multi-record external-reference container");
    let records = crate::test_support::with_decode_context(|ctx| {
        super::super::external_reference_records(ctx, &container)
    })
    .expect("multi-record handle-set records");
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "NX external reference records",
        |ctx| {
            super::super::external_reference_tail_reference_pairs(ctx, &container, &records)
                .map(|_| ())
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("native tail-pair projection must refuse at a record visit");
    };
    assert_eq!(limit.operation, "NX external reference records");
    assert_eq!(limit.additional, 1);
}

#[test]
fn native_external_tail_route_charges_each_pair_visit() {
    let mut stream = crate::test_support::test_streams::external_reference_stream();
    // Insert another valid pair before the end-anchored string table.
    stream.splice(96..96, [0xe0, 0, 0, 0, 6, 0xc0, 0, 0, 2]);
    let file = prt_with_named_payloads(&[(
        "/Root/ExternalReferences",
        stream,
    )]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("indexed external-reference container");
    let records = crate::test_support::with_decode_context(|ctx| {
        super::super::external_reference_records(ctx, &container)
    })
    .expect("handle-set record");
    let pair_count = crate::test_support::with_decode_context(|ctx| {
        super::super::external_reference_tail_reference_pairs(ctx, &container, &records)
            .map(|pairs| pairs.len())
    })
    .expect("two external-reference tail pairs");
    assert_eq!(pair_count, 2);
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "nx native external reference tail pairs",
        |ctx| {
            super::super::external_reference_tail_reference_pairs(ctx, &container, &records)
                .map(|_| ())
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("native tail-pair projection must refuse at a pair visit");
    };
    assert_eq!(limit.operation, "nx native external reference tail pairs");
    assert_eq!(limit.additional, 1);
}

#[test]
fn native_external_tail_route_refuses_collection_limit() {
    let error = native_external_tail_result(|policy| policy.limits.max_collection_items = 1)
        .expect_err("native pair exceeds parsed collection budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "nx native external reference tail pairs"),
        "{error:?}"
    );
}

#[test]
fn native_external_tail_route_refuses_retained_limit() {
    let error = native_external_tail_result(|policy| policy.limits.max_retained_bytes = 0)
        .expect_err("external pair exceeds retained budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "nx native external reference tail pairs"),
        "{error:?}"
    );
}

#[test]
fn native_external_tail_route_refuses_materialized_scratch_limit() {
    let error = native_external_tail_result(|policy| policy.limits.max_materialized_bytes = 0)
        .expect_err("parsed tail pairs exceed the scratch budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes
            && limit.operation == "nx external reference tail pairs"),
        "{error:?}"
    );
}

#[test]
fn native_external_tail_route_refuses_work_limit() {
    let error = native_external_tail_result(|policy| policy.limits.max_work_units = 0)
        .expect_err("external pair scan exceeds work budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits),
        "{error:?}"
    );
}

fn native_external_string_uses_result(
    configure: impl FnOnce(&mut DecodePolicy),
) -> Result<Vec<super::super::ExternalReferenceRecordStringUse>, CodecError> {
    let file = prt_with_named_payloads(&[(
        "/Root/ExternalReferences",
        crate::test_support::test_streams::external_reference_stream(),
    )]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("indexed external-reference container");
    let (records, references) =
        crate::test_support::with_decode_context(|ctx| -> Result<_, CodecError> {
            Ok((
                super::super::external_reference_records(ctx, &container)?,
                super::super::external_references(ctx, &container)?,
            ))
        })
        .expect("native external-reference inputs");

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| super::super::external_reference_record_string_uses(ctx, &records, &references),
    )
}

#[test]
fn native_external_string_uses_route_preserves_slots() {
    let uses = native_external_string_uses_result(|_| {}).expect("external string uses");
    assert_eq!(uses.len(), 4);
    assert_eq!(
        uses.iter()
            .map(|use_| use_.string_index)
            .collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );
}

#[test]
fn native_external_string_uses_route_refuses_collection_limit() {
    let error = native_external_string_uses_result(|policy| policy.limits.max_collection_items = 0)
        .expect_err("slot index exceeds collection budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "nx external reference slot index"),
        "{error:?}"
    );
}

#[test]
fn native_external_string_uses_route_refuses_retained_limit() {
    let error = native_external_string_uses_result(|policy| policy.limits.max_retained_bytes = 0)
        .expect_err("slot use exceeds retained budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "nx native external reference string uses"),
        "{error:?}"
    );
}

#[test]
fn native_external_string_uses_route_refuses_scoped_limit() {
    let error =
        native_external_string_uses_result(|policy| policy.limits.max_materialized_bytes = 0)
            .expect_err("slot index exceeds scoped budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes
            && limit.operation == "nx external reference slot index"),
        "{error:?}"
    );
}

#[test]
fn native_external_string_uses_route_refuses_work_limit() {
    let error = native_external_string_uses_result(|policy| policy.limits.max_work_units = 0)
        .expect_err("slot index exceeds work budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "nx external reference slot index"),
        "{error:?}"
    );
}

fn native_external_children_result(
    configure: impl FnOnce(&mut DecodePolicy),
) -> Result<Vec<super::super::ExternalReferenceRecordChild>, CodecError> {
    let file = prt_with_named_payloads(&[(
        "/Root/ExternalReferences",
        crate::test_support::test_streams::external_reference_stream(),
    )]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("indexed external-reference container");
    let (records, references, uses) =
        crate::test_support::with_decode_context(|ctx| -> Result<_, CodecError> {
            let records = super::super::external_reference_records(ctx, &container)?;
            let references = super::super::external_references(ctx, &container)?;
            let uses =
                super::super::external_reference_record_string_uses(ctx, &records, &references)?;
            Ok((records, references, uses))
        })
        .expect("complete external-reference child inputs");

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| super::super::external_reference_record_children(ctx, &records, &references, &uses),
    )
}

#[test]
fn native_external_children_route_preserves_child() {
    let children = native_external_children_result(|_| {}).expect("external child");
    assert_eq!(children.len(), 1);
    assert_eq!(
        children[0].id,
        "nx:external-reference-record:/Root/ExternalReferences#6:child"
    );
}

#[test]
fn native_external_children_route_refuses_collection_limit() {
    let error = native_external_children_result(|policy| policy.limits.max_collection_items = 0)
        .expect_err("child index exceeds collection budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "nx external reference child index"),
        "{error:?}"
    );
}

#[test]
fn native_external_children_route_refuses_retained_limit() {
    let error = native_external_children_result(|policy| policy.limits.max_retained_bytes = 0)
        .expect_err("child record exceeds retained budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "nx native external reference children"),
        "{error:?}"
    );
}

#[test]
fn native_external_children_route_refuses_scoped_limit() {
    let error = native_external_children_result(|policy| policy.limits.max_materialized_bytes = 0)
        .expect_err("child index exceeds scoped budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes
            && limit.operation == "nx external reference child index"),
        "{error:?}"
    );
}

#[test]
fn native_external_children_route_refuses_work_limit() {
    let error = native_external_children_result(|policy| policy.limits.max_work_units = 0)
        .expect_err("child index exceeds work budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "nx external reference child index"),
        "{error:?}"
    );
}

#[test]
fn external_reference_record_slots_resolve_atomically_in_the_same_stream() {
    use super::super::{
        external_reference_record_children, external_reference_record_string_uses,
        ExternalReference, ExternalReferenceRecord,
    };

    crate::test_support::with_decode_context(|ctx| {
        let references = (0..4)
            .map(|ordinal| ExternalReference {
                id: format!("reference#{ordinal}"),
                ordinal,
                path: format!("value-{ordinal}"),
                source_entry: "stream".into(),
                source_offset: 100 + u64::from(ordinal),
            })
            .collect::<Vec<_>>();
        let record = ExternalReferenceRecord {
            id: "record#7".into(),
            record_id: 7,
            declared_count: 2,
            id_slots: [0, 3, 1, 2],
            handles: crate::container::extref_handles::ExtrefHandles::new(vec![10, 20, 20])
                .unwrap(),
            tail_byte_len: 5,
            source_entry: "stream".into(),
            source_offset: 20,
        };
        let uses =
            external_reference_record_string_uses(ctx, std::slice::from_ref(&record), &references)
                .expect("complete string use lane");
        assert_eq!(uses.len(), 4);
        assert_eq!(uses[0].id, "nx:external-reference:record-string-use#7-0");
        assert_eq!(
            uses.iter()
                .map(|use_| u8::from(use_.slot))
                .collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
        assert_eq!(
            uses.iter()
                .map(|use_| use_.string_index)
                .collect::<Vec<_>>(),
            [0, 3, 1, 2]
        );
        assert_eq!(uses[1].external_reference, "reference#3");
        assert_eq!(uses[1].source_offset, 31);
        let mut child_references = references.clone();
        child_references[0].path = "child.prt".into();
        let child_uses = external_reference_record_string_uses(
            ctx,
            std::slice::from_ref(&record),
            &child_references,
        )
        .expect("complete child string use lane");
        let children = external_reference_record_children(
            ctx,
            std::slice::from_ref(&record),
            &child_references,
            &child_uses,
        )
        .expect("complete child record");
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].external_record, record.id);
        assert_eq!(children[0].name_reference, "reference#0");
        assert_eq!(children[0].directory_reference, "reference#1");
        assert!(external_reference_record_children(
            ctx,
            std::slice::from_ref(&record),
            &references,
            &uses
        )
        .expect("non-child record")
        .is_empty());

        let mut out_of_range = record.clone();
        out_of_range.id_slots[2] = 4;
        assert!(
            external_reference_record_string_uses(ctx, &[out_of_range], &references)
                .expect("unresolved slot")
                .is_empty()
        );
        let mut duplicate = references.clone();
        duplicate.push(references[0].clone());
        assert!(
            external_reference_record_string_uses(ctx, &[record], &duplicate)
                .expect("duplicate slot")
                .is_empty()
        );
    });
}
