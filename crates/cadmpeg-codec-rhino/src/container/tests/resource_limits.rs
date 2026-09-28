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
