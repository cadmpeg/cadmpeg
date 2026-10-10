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
        match crate::container::scan(&ctx, bytes) {
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
    let bytes = minimal_document(
        "50",
        &[
            table(
                archive,
                0x1000_0014,
                &[crate::test_support::test_dump::short_chunk(
                    archive,
                    0xa000_0026,
                    201_000_000,
                )],
            ),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[]),
        ],
    );
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
fn view_list_second_walk_has_separate_work_admission() {
    let mut body = 1_i32.to_le_bytes().to_vec();
    body.extend(crate::test_support::test_dump::short_chunk(
        ArchiveVersion::V5,
        0x8000_0001,
        0,
    ));
    let bytes = crc_chunk(ArchiveVersion::V5, 0x2000_803b, &body);
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 1;
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
        if limit.operation == "Rhino view checksum second walk" && limit.used == 1 && limit.additional == 1));
}
