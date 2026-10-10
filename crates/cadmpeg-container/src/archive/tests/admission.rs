// SPDX-License-Identifier: Apache-2.0
use super::{
    archive_bytes, ArchiveSnapshot, DecodeArena, DecodeContext, DecodePolicy, EntryRecord,
};
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;

#[test]
fn snapshot_metadata_is_scoped_until_snapshot_drop() {
    let bytes = archive_bytes();
    for release in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        // Stay below the session's input-derived materialization allowance.
        policy.limits.max_materialized_bytes = 1_000_000;
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let snapshot = ArchiveSnapshot::new(&ctx, root).expect("metadata is temporary");
        // Three records, their ordinal index, and the three decoded names.
        let metadata_bytes = cadmpeg_core::decode::u64_from_index(
            3 * (std::mem::size_of::<EntryRecord>() + std::mem::size_of::<usize>()) + 30,
        );
        if release {
            drop(snapshot);
        }
        let result = ctx.reserve_scoped(policy.limits.max_materialized_bytes, "snapshot lifetime");
        if release {
            result.expect("snapshot storage is released");
        } else {
            assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::MaterializedBytes
                    && limit.used == metadata_bytes));
        }
    }
}

#[test]
fn data_end_overflow_is_a_fixed_diagnostic() {
    let bytes = archive_bytes();
    let arena = DecodeArena::new();
    let (setup, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
    let snapshot = ArchiveSnapshot::new(&setup, root).expect("snapshot");
    let mut entry = snapshot.entries()[0].clone();
    entry.data_start = u64::MAX;
    entry.compressed_size = 1;
    assert!(
        matches!(entry.data_end(), Err(CodecError::Malformed(message))
        if message == "ZIP data range overflows")
    );
}

#[test]
fn construction_diagnostics_need_no_budget() {
    let mut bytes = archive_bytes();
    let central = bytes
        .windows(4)
        .position(|signature| signature == b"PK\x01\x02")
        .expect("central header");
    // General-purpose flag bit 0 marks the first entry encrypted.
    bytes[central + 8..central + 10].copy_from_slice(&1_u16.to_le_bytes());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
    assert!(matches!(
        ArchiveSnapshot::new(&ctx, root),
        Err(CodecError::Malformed(message)) if message == "encrypted ZIP entry 0"
    ));
    ctx.finish_session()
        .expect("the discarded diagnostic leaves the session unrefused");
}

#[test]
fn snapshot_rejects_distinct_encoded_names_with_same_decoded_name() {
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for name in ["x", "\u{a0}"] {
        writer
            .start_file(
                name,
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
            )
            .expect("entry");
        writer.write_all(b"payload").expect("payload");
    }
    let mut bytes = writer.finish().expect("archive").into_inner();
    let central = bytes
        .windows(4)
        .position(|signature| signature == b"PK\x01\x02")
        .expect("central header");
    // CP437 byte FF and UTF-8 C2 A0 both name U+00A0, with distinct raw keys.
    bytes[30] = 0xff;
    bytes[central + 46] = 0xff;
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
    assert!(
        matches!(ArchiveSnapshot::new(&ctx, root), Err(CodecError::Malformed(message))
        if message == "duplicate ZIP entry name at entry 1")
    );
}

#[test]
fn physical_ledger_diagnostic_is_retained_outside_region_scope() {
    let bytes = archive_bytes();
    let arena = DecodeArena::new();
    let (setup, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
    let mut snapshot = ArchiveSnapshot::new(&setup, root).expect("snapshot");
    snapshot.entries[0].header_start = 1;
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let CodecError::ResourceLimit(first) = snapshot
        .physical_ledger(&ctx)
        .expect_err("escaping diagnostic")
    else {
        panic!("resource refusal");
    };
    assert_eq!(first.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(first.operation, "ZIP structural error");
    let CodecError::ResourceLimit(repeated) =
        ctx.charge_work(1, "later").expect_err("fused refusal")
    else {
        panic!("resource refusal");
    };
    assert_eq!(repeated, first);
}

#[test]
fn stored_entry_integrity_is_checked_before_registering_output() {
    let bytes = archive_bytes();
    let arena = DecodeArena::new();
    let (setup, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
    let mut snapshot = ArchiveSnapshot::new(&setup, root).expect("snapshot");
    let original_crc = snapshot.entries[0].crc32;
    let original_size = snapshot.entries[0].uncompressed_size;
    let mut policy = DecodePolicy::service();
    // The one accepted output owns the session's only borrowed-space slot.
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    snapshot.entries[0].uncompressed_size = original_size + 1;
    assert!(matches!(snapshot.open(&ctx, "stored.bin"),
        Err(CodecError::Malformed(message)) if message == "stored size mismatch for stored.bin"));
    snapshot.entries[0].uncompressed_size = original_size;
    snapshot.entries[0].crc32 = original_crc ^ 1;
    assert!(matches!(snapshot.open(&ctx, "stored.bin"),
        Err(CodecError::Malformed(message)) if message == "CRC mismatch for stored.bin"));
    snapshot.entries[0].crc32 = original_crc;
    assert_eq!(
        snapshot
            .open(&ctx, "stored.bin")
            .expect("only accepted output registers")
            .window(),
        b"stored"
    );
}
