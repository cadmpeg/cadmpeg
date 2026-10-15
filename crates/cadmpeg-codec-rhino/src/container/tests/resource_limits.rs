// SPDX-License-Identifier: Apache-2.0

use crate::chunks::ArchiveVersion;
use crate::test_support::test_dump::{anonymous_chunk, crc_chunk};

fn assert_checksum_refusal(record: &[u8], typecode: u32, archive: ArchiveVersion, operation: &str) {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(record, &arena, &policy)
        .expect("root bytes admitted");
    let error =
        crate::container::checksum_warning(&ctx, record, typecode, 0, record.len(), archive)
            .expect_err("checksum child exceeds collection limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal) if refusal.operation == operation),
        "expected resource operation {operation}"
    );
}

#[test]
fn mesh_checksum_child_refuses_collection_limit() {
    let archive = ArchiveVersion::V5;
    let mut body = vec![0x1f];
    body.extend([0; 111]);
    body.extend(anonymous_chunk(archive, 3, &[4, 0, 0, 0, 2, 0, 0, 0, 1, 0]));
    let typecode = 0x2000_8032;
    let record = crc_chunk(archive, typecode, &body);
    assert_checksum_refusal(&record, typecode, archive, "Rhino mesh checksum children");
}

#[test]
fn render_settings_checksum_child_refuses_collection_limit() {
    let archive = ArchiveVersion::V6;
    let body = anonymous_chunk(archive, 3, &[0; 16]);
    let typecode = 0x2000_803d;
    let record = crc_chunk(archive, typecode, &body);
    assert_checksum_refusal(
        &record,
        typecode,
        archive,
        "Rhino render settings checksum children",
    );
}

#[test]
fn settings_attributes_checksum_child_refuses_collection_limit() {
    let archive = ArchiveVersion::V5;
    let mut body = vec![0x11];
    body.extend(1.0_f64.to_le_bytes());
    body.extend([1, 2, 3, 4]);
    for _ in 0..3 {
        body.extend(0_i32.to_le_bytes());
    }
    body.extend(anonymous_chunk(archive, 0, &[]));
    let typecode = 0x2000_8134;
    let record = crc_chunk(archive, typecode, &body);
    assert_checksum_refusal(
        &record,
        typecode,
        archive,
        "Rhino settings checksum children",
    );
}

#[test]
fn preview_checksum_child_refuses_collection_limit() {
    let archive = ArchiveVersion::V5;
    let mut body = Vec::new();
    for value in [40_i32, 1, 1] {
        body.extend(value.to_le_bytes());
    }
    body.extend(1_i16.to_le_bytes());
    body.extend(24_i16.to_le_bytes());
    for value in [0_i32, 4, 0, 0, 0, 0] {
        body.extend(value.to_le_bytes());
    }
    body.extend(4_u32.to_le_bytes());
    body.extend(0x1122_3344_u32.to_le_bytes());
    body.push(1);
    body.extend(crc_chunk(archive, 0x4000_8000, &[0xde, 0xad, 0xbe, 0xef]));
    let typecode = 0x2000_8025;
    let record = crc_chunk(archive, typecode, &body);
    assert_checksum_refusal(
        &record,
        typecode,
        archive,
        "Rhino preview checksum children",
    );
}

#[test]
fn user_table_checksum_child_refuses_collection_limit() {
    let archive = ArchiveVersion::V5;
    let mut body = vec![0; 16];
    body.extend(crc_chunk(
        archive,
        0x2000_8082,
        &[1, 5, 0, 0, 0, 202, 47, 31, 120],
    ));
    let typecode = 0x2000_8080;
    let record = crc_chunk(archive, typecode, &body);
    assert_checksum_refusal(
        &record,
        typecode,
        archive,
        "Rhino user table checksum children",
    );
}

fn assert_scan_descriptor_refusal(bytes: &[u8], operation: &str) {
    let mut cap = 0;
    for _ in 0..256 {
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(bytes, &arena, &policy)
            .expect("fixture");
        match crate::container::scan(&ctx, bytes).map(drop) {
            Err(cadmpeg_core::CodecError::ResourceLimit(limit)) => {
                assert_eq!(
                    limit.dimension,
                    cadmpeg_core::decode::ResourceDimension::CollectionItems
                );
                if limit.operation == operation {
                    assert_eq!(
                        ctx.finish_session().unwrap_err().to_string(),
                        cadmpeg_core::CodecError::ResourceLimit(limit).to_string()
                    );
                    return;
                }
                cap = limit
                    .used
                    .checked_add(limit.additional)
                    .expect("small fixture");
            }
            other => panic!("descriptor refusal was not reached: {other:?}"),
        }
    }
    panic!("descriptor refusal was not reached");
}

#[test]
fn scan_object_descriptors_refuse_collection_growth() {
    let bytes = crate::test_support::test_archive::archive(&[
        crate::test_support::test_archive::object_record(
            1,
            crate::test_support::test_dump::POINT_CLASS,
            &crate::test_support::test_dump::point_payload([0.0, 0.0, 0.0]),
        ),
    ]);
    assert_scan_descriptor_refusal(&bytes, "Rhino scanned object descriptors");
}

#[test]
fn scan_table_descriptors_refuse_collection_growth() {
    let bytes = crate::test_support::test_archive::archive(&[]);
    assert_scan_descriptor_refusal(&bytes, "Rhino scanned tables");
}

#[test]
fn scan_retained_records_refuse_collection_growth() {
    use crate::test_support::test_dump::{long_chunk, minimal_document, table};
    let archive = ArchiveVersion::V5;
    let bytes = minimal_document(
        "50",
        &[
            table(
                archive,
                0x1000_0014,
                &[long_chunk(archive, 0x7000_0001, &[])],
            ),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[]),
        ],
    );
    assert_scan_descriptor_refusal(&bytes, "Rhino scanned opaque records");
    assert_scan_descriptor_refusal(&bytes, "Rhino scanned table records");
}

#[test]
fn view_list_refuses_work_before_framing_truncated_children() {
    let bytes = crc_chunk(ArchiveVersion::V5, 0x2000_803b, &1_i32.to_le_bytes());
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let chunk = crate::chunks::chunk_at(&bytes, 0, bytes.len(), ArchiveVersion::V5, false).unwrap();
    let mut storage = ctx.reserve_scoped(0, "fixture ranges").unwrap();
    let error = crate::container::list_checksum_children(
        &ctx,
        &bytes,
        &chunk,
        ArchiveVersion::V5,
        &mut storage,
    )
    .unwrap_err();
    assert!(matches!(error, crate::chunks::FramingError::Resource(limit)
        if limit.operation == "Rhino view checksum child ranges" && limit.used == 0 && limit.additional == 1));
}

#[test]
fn malformed_plugin_child_does_not_admit_unvisited_range_slots() {
    let archive = ArchiveVersion::V5;
    let mut body = vec![0x10];
    body.extend(256_i32.to_le_bytes());
    body.extend(crate::test_support::test_dump::long_chunk(
        archive,
        0x7000_0001,
        &[],
    ));
    for _ in 1..256 {
        body.extend(anonymous_chunk(archive, 0, &[]));
    }
    let bytes = crc_chunk(archive, super::super::TCODE_PLUGIN_LIST, &body);
    let chunk = crate::chunks::chunk_at(&bytes, 0, bytes.len(), archive, false).unwrap();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    policy.limits.max_work_units = 1;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let error = super::super::plugin_list_checksum_children(&ctx, &bytes, &chunk, archive)
        .expect_err("the first child has a non-anonymous typecode");
    assert!(
        matches!(error, crate::chunks::FramingError::Structural { offset, message }
        if offset == chunk.body().start + 1 + std::mem::size_of::<i32>()
            && message == "plugin-list child must be an anonymous long chunk")
    );
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn plugin_checksum_first_range_refuses_one_collection_slot() {
    let archive = ArchiveVersion::V5;
    let mut body = vec![0x10];
    body.extend(256_i32.to_le_bytes());
    for _ in 0..256 {
        body.extend(anonymous_chunk(archive, 0, &[]));
    }
    let bytes = crc_chunk(archive, super::super::TCODE_PLUGIN_LIST, &body);
    let chunk = crate::chunks::chunk_at(&bytes, 0, bytes.len(), archive, false).unwrap();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let error = super::super::plugin_list_checksum_children(&ctx, &bytes, &chunk, archive)
        .expect_err("the first validated child needs one range slot");
    assert!(matches!(error, crate::chunks::FramingError::Resource(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && limit.operation == "Rhino plugin-list child ranges"
            && limit.used == 0 && limit.additional == 1
            && ctx.resource_refusal() == Some(limit)));
}

#[test]
fn plugin_checksum_valid_children_keep_exact_source_ranges() {
    let archive = ArchiveVersion::V5;
    let mut body = vec![0x10];
    body.extend(2_i32.to_le_bytes());
    let first = anonymous_chunk(archive, 0, &[]);
    let second = anonymous_chunk(archive, 1, &[7, 8, 9]);
    body.extend(&first);
    body.extend(&second);
    let bytes = crc_chunk(archive, super::super::TCODE_PLUGIN_LIST, &body);
    let chunk = crate::chunks::chunk_at(&bytes, 0, bytes.len(), archive, false).unwrap();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    policy.limits.max_work_units = 2;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let start = chunk.body().start + 1 + std::mem::size_of::<i32>();
    let first_end = start + first.len();
    let second_end = first_end + second.len();
    let children = super::super::plugin_list_checksum_children(&ctx, &bytes, &chunk, archive)
        .expect("each child admits one visited slot and one framing visit");
    assert_eq!(children, [start..first_end, first_end..second_end]);
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn summary_admits_only_the_first_table_before_refusal() {
    let mut scan = crate::test_support::test_dump::scan_with_objects(&[]);
    let table = scan.tables[0].clone();
    scan.tables = vec![table; 256];
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(scan.data, &arena, &policy).unwrap();
    let error = super::super::summarize(&ctx, &scan).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && limit.operation == "Rhino summarize traversal"
            && limit.used == 0 && limit.additional == 1
            && ctx.resource_refusal() == Some(limit))
    );
}

#[test]
fn summary_admits_only_the_first_object_before_refusal() {
    let record = crate::test_support::test_dump::object_record_with_payload(
        ArchiveVersion::V5,
        1,
        crate::test_support::test_dump::POINT_CLASS,
        &crate::test_support::test_dump::point_payload([0.0, 0.0, 0.0]),
    );
    let mut scan = crate::test_support::test_dump::scan_with_objects(&vec![record; 256]);
    scan.tables.clear();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(scan.data, &arena, &policy).unwrap();
    let error = super::super::summarize(&ctx, &scan).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && limit.operation == "Rhino summarize traversal"
            && limit.used == 0 && limit.additional == 1
            && ctx.resource_refusal() == Some(limit))
    );
}

#[test]
fn scan_descriptor_backings_are_released_with_the_scan() {
    let bytes = crate::test_support::test_archive::archive(&[
        crate::test_support::test_archive::object_record(
            1,
            crate::test_support::test_dump::POINT_CLASS,
            &crate::test_support::test_dump::point_payload([0.0, 0.0, 0.0]),
        ),
    ]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes = 64 * 1024;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let scan = crate::container::scan(&ctx, &bytes).unwrap();
    assert_eq!(scan.objects.len(), 1);
    assert!(scan
        .tables
        .iter()
        .any(|table| !table.object_typecodes.is_empty()));
    drop(scan);
    let reclaimed = ctx
        .reserve_scoped(64 * 1024, "reclaimed scan storage")
        .expect("descriptor and candidate storage is released when the scan drops");
    drop(reclaimed);
    ctx.finish_session().unwrap();
}

#[test]
fn scan_tables_and_typecode_maps_consume_materialized_budget() {
    let bytes = crate::test_support::test_archive::archive(&[
        crate::test_support::test_archive::object_record(
            1,
            crate::test_support::test_dump::POINT_CLASS,
            &crate::test_support::test_dump::point_payload([0.0, 0.0, 0.0]),
        ),
    ]);
    for operation in ["Rhino scanned tables", "Rhino object typecode counts"] {
        cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
            operation,
            |cap| {
                let arena = cadmpeg_core::decode::DecodeArena::new();
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                policy.limits.max_materialized_bytes = cap;
                let (ctx, _) =
                    cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)?;
                crate::container::scan(&ctx, &bytes).map(drop)
            },
        );
    }
}

#[test]
fn checksum_child_error_storage_lives_through_warning_copy() {
    let archive = ArchiveVersion::V5;
    let bytes = crc_chunk(archive, super::super::TCODE_RENDER_USERDATA, &[]);
    let chunk = crate::chunks::chunk_at(&bytes, 0, bytes.len(), archive, false).unwrap();
    let expected = format!(
        "checksum child framing at offset 0 for typecode 0x20008136 could not be verified: framing error at {}: render-settings userdata is missing its class end",
        chunk.body().end,
    );
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes = u64::try_from(expected.len()).unwrap();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let mut output_storage = ctx.reserve_scoped(0, "checksum warning output").unwrap();
    let error = output_storage
        .with_storage(|| {
            super::super::checksum_warning(&ctx, &bytes, chunk.typecode, 0, bytes.len(), archive)
        })
        .expect_err("the live child error and its warning need separate storage");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes = 64 * 1024;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let warning =
        super::super::checksum_warning(&ctx, &bytes, chunk.typecode, 0, bytes.len(), archive)
            .unwrap()
            .unwrap();
    assert_eq!(warning, expected);
    drop(warning);
    drop(
        ctx.reserve_scoped(
            policy.limits.max_materialized_bytes,
            "released checksum child error",
        )
        .expect("child workspace is released after warning creation"),
    );
    ctx.finish_session().unwrap();
}

fn scan_warning_fixture(property: Vec<u8>, object: Option<Vec<u8>>) -> Vec<u8> {
    let archive = ArchiveVersion::V5;
    crate::test_support::test_dump::minimal_document(
        "50",
        &[
            crate::test_support::test_dump::table(archive, 0x1000_0014, &[property]),
            crate::test_support::test_dump::table(archive, 0x1000_0015, &[]),
            crate::test_support::test_dump::table(
                archive,
                0x1000_0013,
                &object.into_iter().collect::<Vec<_>>(),
            ),
        ],
    )
}

fn assert_scan_warning_storage(bytes: &[u8], expected: &str, degraded: bool) {
    const SCRATCH_BYTES: u64 = 64 * 1024;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let scan = super::super::scan(&ctx, bytes)
        .expect("scan diagnostics and descriptors consume scratch, not retained allowance");
    assert!(scan
        .warnings
        .iter()
        .any(|warning| warning.message.starts_with(expected)));
    if degraded {
        assert!(
            matches!(&scan.objects[0], crate::objects::ObjectRecord::Degraded { warning, .. }
            if warning.starts_with("bounded object record at "))
        );
    }
    let summary =
        super::super::summarize(&cadmpeg_test_support::service_decode_context(), &scan).unwrap();
    assert!(summary.notes.iter().any(|note| note.starts_with(expected)));
    drop(scan);
    drop(
        ctx.reserve_scoped(SCRATCH_BYTES, "released scan diagnostic storage")
            .expect("all scan-owned backing and text is released with the scan"),
    );
    ctx.finish_session().unwrap();
}

#[test]
fn unknown_record_scan_diagnostics_are_scoped() {
    let bytes = scan_warning_fixture(
        crate::test_support::test_dump::long_chunk(ArchiveVersion::V5, 0x1234, b"opaque"),
        None,
    );
    assert_scan_warning_storage(&bytes, "unknown bounded record ", false);
}

#[test]
fn checksum_scan_diagnostics_are_scoped() {
    let mut record = crc_chunk(ArchiveVersion::V5, 0x2000_80ff, b"opaque");
    *record.last_mut().unwrap() ^= 1;
    let bytes = scan_warning_fixture(record, None);
    assert_scan_warning_storage(&bytes, "CRC mismatch ", false);
}

#[test]
fn degraded_object_scan_diagnostics_are_scoped() {
    let bytes = scan_warning_fixture(
        crate::test_support::test_dump::long_chunk(ArchiveVersion::V5, 0x1234, b"opaque"),
        Some(crc_chunk(ArchiveVersion::V5, 0x2000_8070, &[])),
    );
    assert_scan_warning_storage(&bytes, "bounded object record at ", true);
}

fn assert_failed_checksum_backing_released(
    bytes: &[u8],
    error: &crate::chunks::FramingError,
    admitted_error_text: usize,
    child_count: usize,
    parse: impl Fn(
        &cadmpeg_core::decode::DecodeContext<'_>,
        &[u8],
        &crate::chunks::Chunk,
        ArchiveVersion,
    ) -> Result<Vec<std::ops::Range<usize>>, crate::chunks::FramingError>,
) {
    let archive = ArchiveVersion::V5;
    let chunk = crate::chunks::chunk_at(bytes, 0, bytes.len(), archive, false).unwrap();
    let expected = format!(
        "checksum child framing at offset 0 for typecode {:#x} could not be verified: {error}",
        chunk.typecode
    );
    // Amortized growth holds the new backing and the previous half-sized
    // backing together. After failure only the escaping error text stays live.
    let backing = child_count.max(4) * std::mem::size_of::<std::ops::Range<usize>>();
    let cap = u64::try_from(
        (backing + backing / 2 + admitted_error_text).max(expected.len() + admitted_error_text),
    )
    .unwrap();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes = cap;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let mut error_storage = ctx.reserve_scoped(0, "checksum helper error").unwrap();
    let actual_error = error_storage
        .with_storage(|| parse(&ctx, bytes, &chunk, archive))
        .unwrap_err();
    assert_eq!(&actual_error, error);
    drop(
        ctx.reserve_scoped(
            cap - u64::try_from(admitted_error_text).unwrap(),
            "released failed checksum children",
        )
        .expect("dead backing is released while the escaping error remains live"),
    );
    let mut copy_storage = ctx.reserve_scoped(0, "checksum helper error copy").unwrap();
    let copy = copy_storage
        .with_storage(|| {
            super::super::checksum_children_warning(&ctx, chunk.typecode, 0, &actual_error)
        })
        .unwrap();
    assert_eq!(copy, expected);
    drop(copy);
    drop(copy_storage);
    drop(actual_error);
    drop(error_storage);
    let mut output_storage = ctx.reserve_scoped(0, "checksum diagnostic output").unwrap();
    let note = output_storage
        .with_storage(|| {
            super::super::checksum_warning(&ctx, bytes, chunk.typecode, 0, bytes.len(), archive)
        })
        .unwrap()
        .unwrap();
    assert_eq!(note, expected);
    drop(note);
    drop(output_storage);
    drop(
        ctx.reserve_scoped(cap, "released checksum backing and diagnostic")
            .unwrap(),
    );
    ctx.finish_session().unwrap();
}

#[test]
fn missing_class_end_releases_checksum_children_before_warning_copy() {
    let archive = ArchiveVersion::V5;
    let mut body = Vec::new();
    for _ in 0..64 {
        body.extend(crate::test_support::test_dump::short_chunk(
            archive,
            0x8000_1234,
            0,
        ));
    }
    let bytes = crc_chunk(archive, 0x2000_8136, &body);
    let chunk = crate::chunks::chunk_at(&bytes, 0, bytes.len(), archive, false).unwrap();
    let message = "render-settings userdata is missing its class end";
    assert_failed_checksum_backing_released(
        &bytes,
        &crate::chunks::FramingError::structural(chunk.body().end, message),
        message.len(),
        64,
        |ctx, data, chunk, archive| {
            crate::chunks::checksum_children_through_class_end(
                ctx,
                data,
                chunk.body(),
                archive,
                "render-settings userdata",
            )
        },
    );
}

#[test]
fn malformed_plugin_releases_checksum_children_before_warning_copy() {
    let archive = ArchiveVersion::V5;
    let mut body = vec![0x10];
    body.extend(65_i32.to_le_bytes());
    for _ in 0..64 {
        body.extend(crc_chunk(archive, 0x4000_8000, &[]));
    }
    let failure = body.len();
    body.extend(crate::test_support::test_dump::short_chunk(
        archive,
        0x8000_1234,
        0,
    ));
    let bytes = crc_chunk(archive, 0x2000_8135, &body);
    let chunk = crate::chunks::chunk_at(&bytes, 0, bytes.len(), archive, false).unwrap();
    assert_failed_checksum_backing_released(
        &bytes,
        &crate::chunks::FramingError::structural(
            chunk.body().start + failure,
            "plugin-list child must be an anonymous long chunk",
        ),
        0,
        64,
        super::super::plugin_list_checksum_children,
    );
}

#[test]
fn truncated_view_list_releases_checksum_children_before_warning_copy() {
    let archive = ArchiveVersion::V5;
    let mut body = 65_i32.to_le_bytes().to_vec();
    for _ in 0..64 {
        body.extend(crate::test_support::test_dump::short_chunk(
            archive,
            0x8000_1234,
            0,
        ));
    }
    let failure = body.len();
    body.push(0);
    let bytes = crc_chunk(archive, 0x2000_8037, &body);
    let chunk = crate::chunks::chunk_at(&bytes, 0, bytes.len(), archive, false).unwrap();
    assert_failed_checksum_backing_released(
        &bytes,
        &crate::chunks::FramingError::OutOfBounds {
            offset: chunk.body().start + failure,
            end: chunk.body().start + failure + 4,
            bound: chunk.body().end,
        },
        0,
        64,
        |ctx, data, chunk, archive| {
            let mut storage = ctx.reserve_scoped(0, "view checksum error")?;
            super::super::list_checksum_children(ctx, data, chunk, archive, &mut storage)
        },
    );
}

#[test]
fn malformed_settings_releases_checksum_children_before_warning_copy() {
    let archive = ArchiveVersion::V5;
    let mut body = vec![0x13];
    body.extend(1.0_f64.to_le_bytes());
    body.extend([0; 4]);
    body.extend([0; 12]);
    body.extend(crc_chunk(archive, 0x4000_8000, &[]));
    body.extend([0; 40]);
    let failure = body.len();
    body.extend(crc_chunk(archive, 0x4000_8001, &[]));
    let bytes = crc_chunk(archive, 0x2000_8134, &body);
    let chunk = crate::chunks::chunk_at(&bytes, 0, bytes.len(), archive, false).unwrap();
    let message = "settings-attributes earth anchor must be an anonymous long chunk";
    assert_failed_checksum_backing_released(
        &bytes,
        &crate::chunks::FramingError::structural(chunk.body().start + failure, message),
        message.len(),
        1,
        super::super::settings_attributes_checksum_children,
    );
}

#[test]
fn malformed_preview_releases_checksum_children_before_warning_copy() {
    let archive = ArchiveVersion::V5;
    let mut body = Vec::new();
    for value in [40_i32, 1, 1] {
        body.extend(value.to_le_bytes());
    }
    body.extend(1_i16.to_le_bytes());
    body.extend(8_u16.to_le_bytes());
    for value in [0_i32, 1, 0, 0, 1, 0] {
        body.extend(value.to_le_bytes());
    }
    body.extend(4_u32.to_le_bytes());
    body.extend(0_u32.to_le_bytes());
    body.push(1);
    body.extend(crc_chunk(archive, 0x4000_8000, &[]));
    body.extend(1_u32.to_le_bytes());
    body.extend(0_u32.to_le_bytes());
    let failure = body.len();
    body.push(99);
    let bytes = crc_chunk(archive, 0x2000_8025, &body);
    let chunk = crate::chunks::chunk_at(&bytes, 0, bytes.len(), archive, false).unwrap();
    let message = "compressed preview image buffer has unsupported compression method 99";
    assert_failed_checksum_backing_released(
        &bytes,
        &crate::chunks::FramingError::structural(chunk.body().start + failure, message),
        message.len(),
        1,
        super::super::compressed_preview_checksum_children,
    );
}
