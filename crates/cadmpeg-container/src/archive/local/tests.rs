// SPDX-License-Identifier: Apache-2.0

use std::io::{Cursor, Write as _};

use cadmpeg_core::decode::{
    ByteRange, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
};
use cadmpeg_core::CodecError;
use cadmpeg_test_support::bytes::{put_u16, put_u32};
use zip::write::SimpleFileOptions;
use zip::CompressionMethod;

use super::super::{ArchiveSnapshot, EntryRecord, ZipSpanRole};

fn local(name: &[u8], payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0; 30];
    bytes[..4].copy_from_slice(b"PK\x03\x04");
    put_u16(&mut bytes, 4, 20);
    put_u32(&mut bytes, 14, crc32fast::hash(payload));
    let size = u32::try_from(payload.len()).expect("fixture offset fits 32");
    put_u32(&mut bytes, 18, size);
    put_u32(&mut bytes, 22, size);
    put_u16(
        &mut bytes,
        26,
        u16::try_from(name.len()).expect("fixture offset fits 16"),
    );
    bytes.extend_from_slice(name);
    bytes.extend_from_slice(payload);
    bytes
}

fn with_snapshot(bytes: &[u8], run: impl FnOnce(&DecodeContext<'_>, &ArchiveSnapshot<'_>)) {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &policy).expect("fixture fits root policy");
    let archive = ArchiveSnapshot::new(&ctx, root).expect("synthetic archive is readable");
    run(&ctx, &archive);
}

fn assert_partition(archive: &ArchiveSnapshot<'_>, ctx: &DecodeContext<'_>, len: usize) {
    let ledger = archive
        .physical_ledger(ctx)
        .expect("physical partition is bounded");
    let mut next = 0;
    for span in ledger {
        assert_eq!(span.start, next);
        next = span.end;
        assert!(!matches!(span.role, ZipSpanRole::CentralFields(_)));
    }
    assert_eq!(next, u64::try_from(len).expect("fixture offset fits 64"));
}

#[test]
fn missing_directory_recovers_complete_local_members() {
    let mut bytes = local(b"one", b"first");
    bytes.extend(local(b"two", b"second"));
    with_snapshot(&bytes, |ctx, archive| {
        assert_eq!(archive.entries().len(), 2);
        assert!(archive.entries().iter().all(EntryRecord::is_recovered));
        assert!(archive
            .recovery_reason()
            .expect("archive recovery is reported")
            .contains("ZIP end record is absent"));
        assert_eq!(archive.recovery_tail(), None);
        assert_eq!(
            archive
                .open(ctx, "one")
                .expect("complete member passes integrity checks")
                .window(),
            b"first"
        );
        assert_eq!(
            archive
                .open(ctx, "two")
                .expect("complete member passes integrity checks")
                .window(),
            b"second"
        );
        let summaries = archive
            .container_entries(ctx, |_| cadmpeg_core::container::ContainerRole::Other)
            .expect("container summary is bounded");
        assert!(summaries[0].attributes.contains_key("archive_recovery"));
        assert!(!summaries[0]
            .attributes
            .contains_key("central_header_offset"));
        assert_eq!(
            archive.entries()[0].declaration_range(),
            ByteRange { start: 0, end: 33 }
        );
        assert_partition(archive, ctx, bytes.len());
    });
}

#[test]
fn truncated_final_member_retains_frame_and_keeps_previous_payload() {
    let mut bytes = local(b"one", b"first");
    let final_start = u64::try_from(bytes.len()).expect("fixture offset fits 64");
    bytes.extend(local(b"two", b"second"));
    bytes.truncate(bytes.len() - 3);
    with_snapshot(&bytes, |ctx, archive| {
        assert_eq!(
            archive
                .open(ctx, "one")
                .expect("complete member passes integrity checks")
                .window(),
            b"first"
        );
        assert!(
            matches!(archive.open(ctx, "two"), Err(CodecError::Malformed(message)) if message.contains("truncated ZIP local payload"))
        );
        assert_eq!(
            archive
                .entry("two")
                .expect("local name is retained")
                .stored_range()
                .expect("source span is bounded"),
            ByteRange {
                start: final_start,
                end: u64::try_from(bytes.len()).expect("fixture offset fits 64")
            }
        );
        assert_partition(archive, ctx, bytes.len());
    });
}

#[test]
fn unreadable_header_retains_unframed_tail_without_searching_for_signatures() {
    let first = local(b"one", b"first");
    for tail in [
        b"PK\x03\x04".as_slice(),
        b"garbagePK\x03\x04",
        b"PK\x01\x02",
        b"PK\x05\x06",
    ] {
        let mut bytes = first.clone();
        bytes.extend_from_slice(tail);
        with_snapshot(&bytes, |ctx, archive| {
            assert_eq!(archive.entries().len(), 1);
            assert_eq!(
                archive.recovery_tail(),
                Some(ByteRange {
                    start: u64::try_from(first.len()).expect("fixture offset fits 64"),
                    end: u64::try_from(bytes.len()).expect("fixture offset fits 64")
                })
            );
            assert_partition(archive, ctx, bytes.len());
        });
    }
}

#[test]
fn descriptor_without_sizes_ends_walk_at_unreadable_member() {
    let mut bytes = local(b"one", b"first");
    let start = bytes.len();
    let mut unknown = local(b"two", b"PK\x03\x04payload");
    put_u16(&mut unknown, 6, 8);
    put_u32(&mut unknown, 18, 0);
    put_u32(&mut unknown, 22, 0);
    bytes.extend(unknown);
    bytes.extend(local(b"three", b"hidden"));
    with_snapshot(&bytes, |ctx, archive| {
        assert_eq!(archive.entries().len(), 2);
        assert!(archive.entry("three").is_none());
        assert!(archive.open(ctx, "two").is_err());
        assert_eq!(
            archive
                .entry("two")
                .expect("local name is retained")
                .stored_range()
                .expect("source span is bounded")
                .start,
            u64::try_from(start).expect("fixture offset fits 64")
        );
        assert_partition(archive, ctx, bytes.len());
    });
}

#[test]
fn descriptors_with_declared_sizes_must_match_before_walking_on() {
    for signed in [false, true] {
        for valid in [false, true] {
            let mut bytes = local(b"one", b"first");
            put_u16(&mut bytes, 6, 8);
            if signed {
                bytes.extend_from_slice(b"PK\x07\x08");
            }
            bytes.extend_from_slice(&(crc32fast::hash(b"first") ^ u32::from(!valid)).to_le_bytes());
            bytes.extend_from_slice(&5_u32.to_le_bytes());
            bytes.extend_from_slice(&5_u32.to_le_bytes());
            bytes.extend(local(b"two", b"second"));
            with_snapshot(&bytes, |ctx, archive| {
                assert_eq!(archive.entries().len(), if valid { 2 } else { 1 });
                assert_eq!(archive.open(ctx, "one").is_ok(), valid);
                assert_partition(archive, ctx, bytes.len());
            });
        }
    }
}

#[test]
fn zip64_local_sizes_bound_payload_and_reject_missing_extra() {
    let mut bytes = local(b"one", b"first");
    let payload = bytes.split_off(33);
    put_u32(&mut bytes, 18, u32::MAX);
    put_u32(&mut bytes, 22, u32::MAX);
    let mut extra = vec![1, 0, 16, 0];
    extra.extend_from_slice(&5_u64.to_le_bytes());
    extra.extend_from_slice(&5_u64.to_le_bytes());
    put_u16(
        &mut bytes,
        28,
        u16::try_from(extra.len()).expect("fixture offset fits 16"),
    );
    bytes.extend(extra);
    bytes.extend(payload);
    with_snapshot(&bytes, |ctx, archive| {
        assert_eq!(
            archive
                .open(ctx, "one")
                .expect("complete member passes integrity checks")
                .window(),
            b"first"
        );
        assert_partition(archive, ctx, bytes.len());
    });
    put_u16(&mut bytes, 28, 0);
    with_snapshot(&bytes, |ctx, archive| {
        assert!(archive.open(ctx, "one").is_err());
        assert_partition(archive, ctx, bytes.len());
    });
}

#[test]
fn payload_integrity_and_admission_survive_recovery() {
    for defect in ["crc", "encrypted", "unsupported"] {
        let mut bytes = local(b"one", b"first");
        match defect {
            "crc" => put_u32(&mut bytes, 14, 0),
            "encrypted" => {
                put_u16(&mut bytes, 6, 1);
                put_u32(&mut bytes, 18, 17);
                drop(bytes.splice(33..33, [0; 12]));
            }
            _ => put_u16(&mut bytes, 8, 12),
        }
        bytes.extend(local(b"two", b"second"));
        with_snapshot(&bytes, |ctx, archive| {
            assert!(archive.open(ctx, "one").is_err(), "{defect}");
            assert_eq!(
                archive
                    .open(ctx, "two")
                    .expect("complete member passes integrity checks")
                    .window(),
                b"second"
            );
            assert_partition(archive, ctx, bytes.len());
        });
    }
}

#[test]
fn inconsistent_stored_sizes_stop_the_walk_and_retain_the_remaining_source() {
    for expanded in [4, 6] {
        let mut bytes = local(b"one", b"first");
        put_u32(&mut bytes, 22, expanded);
        bytes.extend(local(b"two", b"second"));
        with_snapshot(&bytes, |ctx, archive| {
            assert_eq!(archive.entries().len(), 1);
            assert!(archive.entry("two").is_none());
            assert!(
                matches!(archive.open(ctx, "one"), Err(CodecError::Malformed(message)) if message.contains("stored ZIP local sizes disagree"))
            );
            assert_eq!(archive.entries()[0].data_start, None);
            assert_eq!(
                archive.entries()[0]
                    .stored_range()
                    .expect("unreadable frame retains the source"),
                ByteRange {
                    start: 0,
                    end: u64::try_from(bytes.len()).expect("fixture offset fits 64"),
                }
            );
            assert_partition(archive, ctx, bytes.len());
        });
    }
}

#[test]
fn zip64_local_fields_include_both_sizes_and_match_ordinary_sizes() {
    let payload = b"geometry";
    let mut encoder =
        flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(payload)
        .expect("encode synthetic payload");
    let compressed = encoder.finish().expect("finish synthetic deflate");
    let stored = u32::try_from(compressed.len()).expect("fixture stored size fits u32");
    let expanded = u32::try_from(payload.len()).expect("fixture expanded size fits u32");
    assert_ne!(
        stored, expanded,
        "fixture distinguishes the two ZIP64 sizes"
    );
    for (stored32, expanded32) in [
        (u32::MAX, u32::MAX),
        (u32::MAX, expanded),
        (stored, u32::MAX),
    ] {
        let mut bytes = local(b"one", payload);
        bytes.truncate(33);
        put_u16(&mut bytes, 8, 8);
        put_u32(&mut bytes, 18, stored32);
        put_u32(&mut bytes, 22, expanded32);
        put_u16(&mut bytes, 28, 20);
        bytes.extend_from_slice(&[1, 0, 16, 0]);
        bytes.extend_from_slice(&u64::from(expanded).to_le_bytes());
        bytes.extend_from_slice(&u64::from(stored).to_le_bytes());
        bytes.extend_from_slice(&compressed);
        with_snapshot(&bytes, |ctx, archive| {
            assert_eq!(
                archive
                    .open(ctx, "one")
                    .expect("complete ZIP64 member")
                    .window(),
                payload
            );
            assert_partition(archive, ctx, bytes.len());
        });
        if stored32 != u32::MAX {
            put_u32(&mut bytes, 18, stored32 - 1);
            with_snapshot(&bytes, |ctx, archive| {
                assert!(
                    matches!(archive.open(ctx, "one"), Err(CodecError::Malformed(message)) if message.contains("ZIP64 local sizes disagree"))
                );
                assert_partition(archive, ctx, bytes.len());
            });
        }
    }
}

fn directory_archive() -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, method) in [
        ("stored", CompressionMethod::Stored),
        ("deflate", CompressionMethod::Deflated),
        ("zstd", CompressionMethod::Zstd),
    ] {
        writer
            .start_file(
                name,
                SimpleFileOptions::default().compression_method(method),
            )
            .expect("start synthetic member");
        writer
            .write_all(b"geometry")
            .expect("write synthetic payload");
    }
    writer
        .finish()
        .expect("finish synthetic directory")
        .into_inner()
}

#[test]
fn readable_directory_remains_authoritative_over_local_declarations() {
    let mut bytes = directory_archive();
    // Same-width local name, method, CRC, flags and sizes all disagree with
    // the directory. The existing directory interpretation remains intact.
    bytes[30..36].copy_from_slice(b"hidden");
    put_u16(&mut bytes, 6, 1);
    put_u16(&mut bytes, 8, 12);
    put_u32(&mut bytes, 14, 0);
    put_u32(&mut bytes, 18, 0);
    put_u32(&mut bytes, 22, 0);
    with_snapshot(&bytes, |ctx, archive| {
        assert!(archive.recovery_reason().is_none());
        assert!(archive.entries().iter().all(|entry| !entry.is_recovered()));
        assert!(archive.entry("hidden").is_none());
        assert_eq!(
            archive
                .open(ctx, "stored")
                .expect("complete member passes integrity checks")
                .window(),
            b"geometry"
        );
    });
}

#[test]
fn damaged_directory_recovers_supported_compression_without_changing_open_checks() {
    let bytes = directory_archive();
    let central = with_directory_start(&bytes);
    for length in [central, central + 4, bytes.len() - 10] {
        let truncated = &bytes[..length];
        with_snapshot(truncated, |ctx, archive| {
            assert!(archive.recovery_reason().is_some());
            for name in ["stored", "deflate", "zstd"] {
                assert_eq!(
                    archive
                        .open(ctx, name)
                        .expect("complete member passes integrity checks")
                        .window(),
                    b"geometry"
                );
            }
            assert_partition(archive, ctx, truncated.len());
        });
    }
    let mut damaged = bytes.clone();
    damaged[central] ^= 1;
    with_snapshot(&damaged, |ctx, archive| {
        assert!(archive.recovery_reason().is_some());
        assert_eq!(
            archive
                .open(ctx, "deflate")
                .expect("complete member passes integrity checks")
                .window(),
            b"geometry"
        );
        assert_partition(archive, ctx, damaged.len());
    });
    let mut damaged_extra = bytes.clone();
    put_u32(&mut damaged_extra, central + 20, u32::MAX);
    with_snapshot(&damaged_extra, |ctx, archive| {
        assert!(archive.recovery_reason().is_some());
        assert_eq!(
            archive
                .open(ctx, "stored")
                .expect("complete member passes integrity checks")
                .window(),
            b"geometry"
        );
        assert_partition(archive, ctx, damaged_extra.len());
    });
}

fn with_directory_start(bytes: &[u8]) -> usize {
    let mut start = 0;
    with_snapshot(bytes, |_, archive| {
        start =
            usize::try_from(archive.entries()[0].central_start).expect("fixture offset fits size");
    });
    start
}

#[test]
fn local_names_keep_cp437_and_unicode_identity_and_reject_duplicates() {
    let mut bytes = local(b"\x82", b"legacy");
    let mut utf8 = local("水".as_bytes(), b"unicode");
    put_u16(&mut utf8, 6, 0x800);
    bytes.extend(utf8);
    with_snapshot(&bytes, |ctx, archive| {
        assert_eq!(
            archive
                .open(ctx, "é")
                .expect("complete member passes integrity checks")
                .window(),
            b"legacy"
        );
        assert_eq!(
            archive
                .open(ctx, "水")
                .expect("complete member passes integrity checks")
                .window(),
            b"unicode"
        );
    });
    bytes.extend(local(b"\x82", b"duplicate"));
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("fixture fits root policy");
    assert!(
        matches!(ArchiveSnapshot::new(&ctx, root), Err(CodecError::Malformed(message)) if message.contains("duplicate recovered"))
    );
}

#[test]
fn recovery_preserves_nested_root_offsets_and_budget_refusals() {
    let local = local(b"one", b"first");
    let mut bytes = b"prefix".to_vec();
    bytes.extend_from_slice(&local);
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("fixture fits root policy");
    let nested = root.child(6, bytes.len()).expect("nested root is bounded");
    let archive = ArchiveSnapshot::new(&ctx, nested).expect("nested archive is readable");
    assert_eq!(
        archive
            .open(&ctx, "one")
            .expect("complete member passes integrity checks")
            .window(),
        b"first"
    );
    assert_partition(&archive, &ctx, local.len());
    for (dimension, operation) in [
        (
            ResourceDimension::CollectionItems,
            "ZIP recovered name index",
        ),
        (ResourceDimension::WorkUnits, "ZIP recovery diagnostic"),
        (ResourceDimension::RetainedBytes, "ZIP recovery diagnostic"),
    ] {
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::WorkUnits => {
                policy.limits.max_work_units =
                    u64::try_from(local.len()).expect("fixture offset fits 64");
            }
            _ => policy.limits.max_retained_bytes = 0,
        }
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&local, &arena, &policy)
            .expect("fixture fits root policy");
        // Formatting the recovery diagnostic charges work before the local
        // walk. The end-record scan exhausts this case's work allowance.
        let result = ArchiveSnapshot::new(&ctx, root);
        assert!(
            matches!(&result, Err(CodecError::ResourceLimit(limit)) if limit.dimension == dimension && limit.operation == operation),
            "{dimension:?} at {operation}: {result:?}"
        );
    }
}
