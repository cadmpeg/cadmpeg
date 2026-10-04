// SPDX-License-Identifier: Apache-2.0
use crate::test_support::test_streams::topology_partition_stream;

#[test]
fn parasolid_cached_records_refuse_collection_at_caller_limit() {
    let bytes = crate::test_support::test_prt::prt_with_partition(
        &crate::test_support::test_streams::blend_surface_topology_partition_stream(),
    );

    crate::test_support::with_decode_context_over(
        &bytes,
        |_| {},
        |ctx| {
            let root = cadmpeg_core::decode::View::over_retained(&bytes);

            let scan = crate::decode::scan(ctx, root).unwrap();
            let parsed = crate::native::substrate::ParsedStreams::parse(ctx, &scan).unwrap();

            crate::test_support::with_decode_context_over(
                &bytes,
                |policy| {
                    policy.limits.max_collection_items = 0;
                },
                |limited_ctx| {
                    let error = crate::native::parasolid::parasolid_blend_surface_records(
                        limited_ctx,
                        &parsed,
                    )
                    .expect_err("cached record refusal");
                    assert!(matches!(
                        error,
                        cadmpeg_core::CodecError::ResourceLimit(limit)
                            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                                && limit.operation == "NX Parasolid cached records"
                    ));
                },
            );
        },
    );
}

struct OneScannedRecord;

impl crate::native::parasolid::ParasolidScanRecords for OneScannedRecord {
    type Row = u32;
    type Record = String;
    const ID_STEM: &'static str = "test-record";

    fn scan(
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _bytes: &[u8],
    ) -> Result<Vec<Self::Row>, cadmpeg_core::CodecError> {
        Ok(vec![7])
    }

    fn xmt(row: &Self::Row) -> u32 {
        *row
    }

    fn record(id: String, _stream_ordinal: u32, _row: Self::Row) -> Self::Record {
        id
    }

    fn id(record: &Self::Record) -> &str {
        record
    }
}

#[test]
fn parasolid_scanned_records_refuse_collection_at_caller_limit() {
    let bytes = crate::test_support::test_prt::prt_with_partition(&topology_partition_stream());

    crate::test_support::with_decode_context_over(
        &bytes,
        |_| {},
        |ctx| {
            let root = cadmpeg_core::decode::View::over_retained(&bytes);

            let scan = crate::decode::scan(ctx, root).unwrap();

            crate::test_support::with_decode_context_over(
                &bytes,
                |policy| {
                    policy.limits.max_collection_items = 0;
                },
                |limited_ctx| {
                    let error = crate::native::parasolid::per_parasolid_scan::<OneScannedRecord>(
                        limited_ctx,
                        &scan.streams,
                    )
                    .expect_err("scanned record refusal");
                    assert!(matches!(
                        error,
                        cadmpeg_core::CodecError::ResourceLimit(limit)
                            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                                && limit.operation == "NX Parasolid scanned records"
                    ));
                },
            );
        },
    );
}

#[test]
fn parasolid_attribute_definitions_refuse_collection_at_caller_limit() {
    let bytes = crate::test_support::test_prt::prt_with_partition(
        &crate::test_support::test_streams::parasolid_entity_records_stream(),
    );

    crate::test_support::with_decode_context_over(
        &bytes,
        |_| {},
        |ctx| {
            let root = cadmpeg_core::decode::View::over_retained(&bytes);

            let scan = crate::decode::scan(ctx, root).unwrap();
            assert!(
                !crate::native::parasolid::parasolid_attribute_definitions(ctx, &scan.streams)
                    .unwrap()
                    .is_empty()
            );

            crate::test_support::with_decode_context_over(
                &bytes,
                |policy| {
                    policy.limits.max_collection_items = 0;
                },
                |limited_ctx| {
                    let error = crate::native::parasolid::parasolid_attribute_definitions(
                        limited_ctx,
                        &scan.streams,
                    )
                    .expect_err("attribute definition collection refusal");
                    assert!(matches!(
                        error,
                        cadmpeg_core::CodecError::ResourceLimit(limit)
                            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                                && limit.operation == "NX attribute identifier index"
                    ));
                },
            );
        },
    );
}

#[test]
fn parasolid_field_names_refuse_collection_at_caller_limit() {
    let mut stream = crate::test_support::test_streams::parasolid_entity_records_stream();
    stream.extend_from_slice(&[
        0x00, 0x63, 0x00, 0x00, 0x00, 0x03, 0x00, 0x19, 0x00, 0x1c, 0x00, 0x1d, 0x00, 0x1e,
    ]);
    let bytes = crate::test_support::test_prt::prt_with_partition(&stream);

    crate::test_support::with_decode_context_over(
        &bytes,
        |_| {},
        |ctx| {
            let root = cadmpeg_core::decode::View::over_retained(&bytes);

            let scan = crate::decode::scan(ctx, root).unwrap();
            assert!(
                !crate::native::parasolid::parasolid_field_names_records(ctx, &scan.streams)
                    .unwrap()
                    .is_empty()
            );

            crate::test_support::with_decode_context_over(
                &bytes,
                |policy| {
                    policy.limits.max_collection_items = 0;
                },
                |limited_ctx| {
                    let error = crate::native::parasolid::parasolid_field_names_records(
                        limited_ctx,
                        &scan.streams,
                    )
                    .expect_err("field names collection refusal");
                    assert!(matches!(
                        error,
                        cadmpeg_core::CodecError::ResourceLimit(limit)
                            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                                && limit.operation == "NX field-name reference lanes"
                    ));
                },
            );
        },
    );
}

#[test]
fn parasolid_entity_51_refuses_collection_at_caller_limit() {
    let bytes = crate::test_support::test_prt::prt_with_partition(
        &crate::test_support::test_streams::parasolid_entity_records_stream(),
    );

    crate::test_support::with_decode_context_over(
        &bytes,
        |_| {},
        |ctx| {
            let root = cadmpeg_core::decode::View::over_retained(&bytes);

            let scan = crate::decode::scan(ctx, root).unwrap();
            assert!(
                !crate::native::parasolid::parasolid_entity_51_records(ctx, &scan.streams)
                    .unwrap()
                    .is_empty()
            );

            crate::test_support::with_decode_context_over(
                &bytes,
                |policy| {
                    policy.limits.max_collection_items = 0;
                },
                |limited_ctx| {
                    let error = crate::native::parasolid::parasolid_entity_51_records(
                        limited_ctx,
                        &scan.streams,
                    )
                    .expect_err("entity 51 collection refusal");
                    assert!(matches!(
                        error,
                        cadmpeg_core::CodecError::ResourceLimit(limit)
                            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                                && limit.operation == "NX entity-51 reference lanes"
                    ));
                },
            );
        },
    );
}
#[test]
fn unicode_record_wire_derives_exact_utf16_and_rejects_disagreement() {
    let wire = r#"{"id":"unicode","stream_ordinal":0,"xmt":2,"code_units":[78,88,55357,56960],"value":"NX🚀","byte_len":8,"inflated_offset":0}"#;
    let record: crate::native::parasolid::ParasolidEntity62UnicodeRecord =
        serde_json::from_str(wire).unwrap();
    assert_eq!(serde_json::to_string(&record).unwrap(), wire);
    assert_eq!(
        serde_json::to_vec(&record).unwrap(),
        serde_json::to_vec(
            &crate::native::parasolid::ParasolidEntity62UnicodeRecordWire::from(record.clone())
        )
        .unwrap()
    );
    let inconsistent = wire.replace("[78,88,55357,56960]", "[78,88,55357]");
    assert!(
        serde_json::from_str::<crate::native::parasolid::ParasolidEntity62UnicodeRecord>(
            &inconsistent
        )
        .is_err()
    );
}

#[test]
fn unicode_record_native_limit_refuses_before_code_unit_copy() {
    let wire = r#"{"id":"nx:parasolid:unicode-record#0","stream_ordinal":0,"xmt":2,"code_units":[78,88,55357,56960],"value":"NX🚀","byte_len":8,"inflated_offset":0}"#;
    let record: crate::native::parasolid::ParasolidEntity62UnicodeRecord =
        serde_json::from_str(wire).unwrap();
    cadmpeg_test_support::native_serialization::assert_native_limit(
        &record,
        serde_json::from_str::<serde_json::Value>(wire).unwrap(),
    );
}
