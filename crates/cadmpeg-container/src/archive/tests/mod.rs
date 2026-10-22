use std::io::{Cursor, Write as _};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_core::CodecError;
use cadmpeg_test_support::zip_fixture::{self, RawEntry};
use zip::write::SimpleFileOptions;
use zip::CompressionMethod;

use super::{ArchiveSnapshot, EntryRecord, PhysicalSpan, ZipCompression, ZipSpanRole};

#[test]
fn indexed_names_preserve_cp437_and_utf8_identity() {
    for (stored, expected, legacy) in [("x", "é", true), ("水", "水", false)] {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        writer.start_file(stored, options).expect("entry");
        writer.write_all(b"payload").expect("payload");
        let mut bytes = writer.finish().expect("archive").into_inner();
        if legacy {
            let archive = zip::ZipArchive::new(Cursor::new(&bytes)).expect("index");
            let central = usize::try_from(archive.central_directory_start()).expect("offset");
            drop(archive);
            bytes[crate::layout::local_header::LEN] = 0x82;
            bytes[central + crate::layout::central_header::LEN] = 0x82;
        }
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        assert!(
            ArchiveSnapshot::contains_matching_name(&ctx, root, |name| name == expected)
                .expect("name probe")
        );
        let archive = ArchiveSnapshot::new(&ctx, root).expect("snapshot");
        assert_eq!(archive.entries().len(), 1);
        assert_eq!(
            archive.open(&ctx, expected).expect("member").window(),
            b"payload"
        );
    }
}

#[test]
fn payload_admission_is_local_to_opened_members() {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, data) in [
        ("required", b"geometry".as_slice()),
        ("optional", b"preview".as_slice()),
    ] {
        writer.start_file(name, options).expect("member");
        writer.write_all(data).expect("payload");
    }
    let original = writer.finish().expect("archive").into_inner();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, root) = DecodeContext::from_root_bytes(&original, &arena, &policy).expect("root");
    let snapshot = ArchiveSnapshot::new(&ctx, root).expect("index");
    let entry = snapshot.entry("optional").expect("optional");
    let local = usize::try_from(entry.header_start).expect("offset");
    let central = usize::try_from(entry.central_start).expect("offset");
    let payload = usize::try_from(entry.data_start.expect("readable frame")).expect("offset");
    for kind in [
        "encrypted",
        "unsupported",
        "crc",
        "local-signature",
        "local-length",
    ] {
        let mut bytes = original.clone();
        match kind {
            "encrypted" => {
                cadmpeg_test_support::bytes::put_u16(&mut bytes, local + 6, 1);
                cadmpeg_test_support::bytes::put_u16(&mut bytes, central + 8, 1);
            }
            "unsupported" => {
                cadmpeg_test_support::bytes::put_u16(&mut bytes, local + 8, 12);
                cadmpeg_test_support::bytes::put_u16(&mut bytes, central + 10, 12);
            }
            "local-signature" => bytes[local] ^= 1,
            "local-length" => {
                cadmpeg_test_support::bytes::put_u16(&mut bytes, local + 26, u16::MAX);
            }
            _ => bytes[payload] ^= 1,
        }
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let snapshot = ArchiveSnapshot::new(&ctx, root)
            .expect("bounded index survives optional payload defect");
        assert_eq!(
            snapshot
                .open(&ctx, "required")
                .expect("independent member")
                .window(),
            b"geometry"
        );
        assert!(snapshot.open(&ctx, "optional").is_err(), "{kind}");
        assert!(!snapshot
            .physical_ledger()
            .expect("physical framing")
            .is_empty());
    }
}

#[test]
fn zstd_member_with_a_window_past_the_allowance_is_skipped_without_fusing() {
    // A streamed frame as a level-22 encoder writes it: a 128 MiB window
    // and no content size. Laid out by `zip_fixture`: `zip` is built
    // without its `zstd` feature.
    let streamed = zip_fixture::zstd_streamed_frame(27, b"preview");
    let bytes = zip_fixture::archive(&[
        RawEntry::compressed("streamed.bin", zip_fixture::ZSTD, &streamed, b"preview"),
        RawEntry::compressed(
            "geometry.bin",
            zip_fixture::ZSTD,
            &zip_fixture::zstd_stored_frame(b"geometry"),
            b"geometry",
        ),
    ]);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
    let snapshot = ArchiveSnapshot::new(&ctx, root).expect("index");
    let Err(CodecError::NotImplemented(message)) = snapshot.open(&ctx, "streamed.bin") else {
        panic!("a window past the per-expansion allowance is unsupported");
    };
    assert!(
        message.starts_with(&format!("Zstandard window of {} bytes", 1_u64 << 27))
            && message.ends_with("per-expansion allowance"),
        "{message}"
    );
    assert_eq!(ctx.resource_refusal(), None);
    assert_eq!(
        snapshot
            .open(&ctx, "geometry.bin")
            .expect("an independent member opens")
            .window(),
        b"geometry"
    );
    assert!(ctx.finish_session().is_ok());
}

#[test]
fn empty_zip_ledger_covers_its_end_record() {
    let bytes = zip::ZipWriter::new(Cursor::new(Vec::new()))
        .finish()
        .expect("empty ZIP finishes")
        .into_inner();
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
        .expect("empty ZIP fits root policy");
    let snapshot = ArchiveSnapshot::new(&ctx, root).expect("empty ZIP is valid");
    assert!(snapshot.entries().is_empty());
    assert_eq!(
        snapshot
            .physical_ledger()
            .expect("empty ZIP has a complete physical ledger"),
        vec![PhysicalSpan {
            start: 0,
            end: cadmpeg_core::decode::u64_from_index(bytes.len()),
            role: ZipSpanRole::EndRecord,
        }]
    );
}

#[test]
fn descriptor_crc_equal_to_optional_signature_keeps_unsigned_layout() {
    let signature = *b"PK\x07\x08";
    let mut bytes = vec![0_u8; 42];
    bytes[30..34].copy_from_slice(&signature);
    let entry = EntryRecord {
        name: "empty".to_owned(),
        compression: ZipCompression::Stored,
        encrypted: false,
        crc32: u32::from_le_bytes(signature),
        compressed_size: 0,
        uncompressed_size: 0,
        header_start: 0,
        data_start: Some(30),
        central_start: 42,
        declaration: super::Declaration::Central(cadmpeg_core::decode::ByteRange {
            start: 42,
            end: 42,
        }),
        utf8_name: false,
        local_header_error: None,
        unreadable_range: None,
    };
    assert_eq!(
        super::parse_data_descriptor(&bytes, &entry, 42)
            .expect("unsigned descriptor matches its central record"),
        42
    );

    bytes.extend_from_slice(&[0; 4]);
    bytes[34..38].copy_from_slice(&signature);
    assert_eq!(
        super::parse_data_descriptor(&bytes, &entry, 46)
            .expect("signed descriptor matches its central record"),
        46
    );
}

/// DEFLATE `plain` the way a ZIP writer would: raw deflate, no zlib wrapper.
fn deflate(plain: &[u8]) -> Vec<u8> {
    let mut encoder =
        flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(plain).expect("deflate accepts input");
    encoder.finish().expect("deflate finishes")
}

/// The mixed fixture: one stored, one deflated and one Zstandard entry.
///
/// Laid out by [`zip_fixture::archive`] rather than by `zip::ZipWriter`, because `zip` is
/// built without its `zstd` feature and its writer cannot emit method 93.  The layout is
/// still judged by two parsers the builder does not share code with -- `zip::ZipArchive`,
/// which indexes the names, and this crate's own central-directory and local-header
/// readers -- and `the_raw_builder_and_zip_writer_produce_the_same_records` compares the
/// builder against `ZipWriter` on the two methods both can write.
fn archive_bytes() -> Vec<u8> {
    let zstd_plain = zip_fixture::zstd_golden_plaintext();
    zip_fixture::archive(&[
        RawEntry::stored("stored.bin", b"stored"),
        RawEntry::compressed(
            "deflated.bin",
            zip_fixture::DEFLATE,
            &deflate(b"deflated payload"),
            b"deflated payload",
        ),
        RawEntry::compressed(
            "zstd.bin",
            zip_fixture::ZSTD,
            zip_fixture::ZSTD_GOLDEN_FRAME,
            &zstd_plain,
        ),
    ])
}

#[test]
fn partition_sort_preserves_equal_region_order() {
    let regions = [
        PhysicalSpan {
            start: 4,
            end: 8,
            role: ZipSpanRole::EndRecord,
        },
        PhysicalSpan {
            start: 0,
            end: 4,
            role: ZipSpanRole::Zip64EndRecord,
        },
        PhysicalSpan {
            start: 0,
            end: 4,
            role: ZipSpanRole::Zip64EndLocator,
        },
    ];
    assert_eq!(
        super::partition(8, &regions).expect("complete partition"),
        vec![regions[1].clone(), regions[0].clone()]
    );
}

#[test]
fn central_directory_indexes_every_entry() {
    let bytes = archive_bytes();
    let arena = DecodeArena::new();
    let (service_ctx, service_root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("build the decode context");
    assert_eq!(
        ArchiveSnapshot::new(&service_ctx, service_root)
            .expect("construct the archive snapshot")
            .entries()
            .len(),
        3
    );
}

#[test]
fn zip_name_probe_retains_no_bytes() {
    let bytes = archive_bytes();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
    assert!(
        ArchiveSnapshot::contains_matching_name(&ctx, root, |name| name == "stored.bin")
            .expect("names are scoped")
    );
}

#[test]
fn zip_end_signature_inside_comment_does_not_replace_directory_count() {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file("entry", SimpleFileOptions::default())
        .expect("entry starts");
    writer.write_all(b"data").expect("entry writes");
    writer
        .set_raw_comment(b"comment PK\x05\x06 suffix".to_vec().into_boxed_slice())
        .expect("comment is valid");
    let bytes = writer.finish().expect("ZIP finishes").into_inner();
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("build the decode context");
    assert_eq!(
        ArchiveSnapshot::new(&ctx, root)
            .expect("directory remains readable")
            .entries()
            .len(),
        1
    );
}

#[test]
fn malformed_end_record_inside_comment_does_not_mask_the_archive() {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file("entry", SimpleFileOptions::default())
        .expect("entry starts");
    writer.write_all(b"data").expect("entry writes");
    let mut comment = vec![0_u8; 40];
    comment[..4].copy_from_slice(b"PK\x05\x06");
    comment[8..10].copy_from_slice(&999_u16.to_le_bytes());
    comment[10..12].copy_from_slice(&999_u16.to_le_bytes());
    writer
        .set_raw_comment(comment.into_boxed_slice())
        .expect("comment is valid");
    let bytes = writer.finish().expect("ZIP finishes").into_inner();
    assert_eq!(
        zip::ZipArchive::new(Cursor::new(bytes.as_slice()))
            .expect("ZIP library finds the preceding end record")
            .len(),
        1
    );
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("build the decode context");
    assert_eq!(
        ArchiveSnapshot::new(&ctx, root)
            .expect("directory remains readable")
            .entries()
            .len(),
        1
    );
}

#[test]
fn false_end_record_cannot_replace_the_selected_directory() {
    let mut bytes = archive_bytes();
    let end = bytes
        .windows(4)
        .rposition(|window| window == b"PK\x05\x06")
        .expect("ZIP end record exists");
    let directory_size = View::u32_le_at(&bytes, end + 12).expect("directory size exists");
    let directory_offset = View::u32_le_at(&bytes, end + 16).expect("directory offset exists");
    bytes[end + 20..end + 22].copy_from_slice(&40_u16.to_le_bytes());
    bytes.resize(bytes.len() + 40, 0);
    let fake = end + 22;
    bytes[fake..fake + 4].copy_from_slice(b"PK\x05\x06");
    bytes[fake + 4..fake + 6].copy_from_slice(&1_u16.to_le_bytes());
    bytes[fake + 8..fake + 10].copy_from_slice(&1_u16.to_le_bytes());
    bytes[fake + 10..fake + 12].copy_from_slice(&1_u16.to_le_bytes());
    bytes[fake + 12..fake + 16].copy_from_slice(&(directory_size + 22).to_le_bytes());
    bytes[fake + 16..fake + 20].copy_from_slice(&directory_offset.to_le_bytes());
    assert_eq!(
        zip::ZipArchive::new(Cursor::new(bytes.as_slice()))
            .expect("ZIP library selects the valid end record")
            .len(),
        3
    );
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("build the decode context");
    assert_eq!(
        ArchiveSnapshot::new(&ctx, root)
            .expect("valid end record selects the directory")
            .entries()
            .len(),
        3
    );
}

#[test]
fn central_directory_signature_after_entries_keeps_archive_readable() {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file("entry", SimpleFileOptions::default())
        .expect("entry starts");
    writer.write_all(b"data").expect("entry writes");
    let mut bytes = writer.finish().expect("ZIP finishes").into_inner();
    let end = bytes
        .windows(4)
        .rposition(|window| window == b"PK\x05\x06")
        .expect("ZIP end record exists");
    bytes.splice(end..end, *b"PK\x05\x05\x03\x00abc");
    assert_eq!(
        zip::ZipArchive::new(Cursor::new(bytes.as_slice()))
            .expect("ZIP library accepts the signature")
            .len(),
        1
    );
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("build the decode context");
    assert_eq!(
        ArchiveSnapshot::new(&ctx, root)
            .expect("directory remains readable")
            .entries()
            .len(),
        1
    );
    drop(ctx);
    let end = end + 9;
    let size = View::u32_le_at(&bytes, end + 12).expect("directory size");
    cadmpeg_test_support::bytes::put_u32(&mut bytes, end + 12, size + 9);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("build the decode context");
    assert_eq!(
        ArchiveSnapshot::new(&ctx, root)
            .expect("declared directory size includes its digital signature")
            .entries()
            .len(),
        1
    );
}

/// Rewrites the central-directory `uncompressed_size` of `name` to `size`.
fn patch_central_uncompressed_size(bytes: &mut [u8], name: &str, size: u32) {
    let mut at = 0;
    while let Some(found) = bytes[at..]
        .windows(4)
        .position(|window| window == [0x50, 0x4b, 0x01, 0x02])
    {
        let record = at + found;
        let name_len = usize::from(u16::from_le_bytes([bytes[record + 28], bytes[record + 29]]));
        let start = record + 46;
        if &bytes[start..start + name_len] == name.as_bytes() {
            bytes[record + 24..record + 28].copy_from_slice(&size.to_le_bytes());
            return;
        }
        at = record + 4;
    }
    panic!("central directory record not found");
}

#[test]
fn a_stored_member_declaring_a_span_under_its_payload_is_reported() {
    use cadmpeg_core::container::{ContainerRole, VerbatimLabel, VerbatimSize};

    let mut bytes = archive_bytes();
    // "stored" is six bytes; the declaration now claims it expands to nine.
    patch_central_uncompressed_size(&mut bytes, "stored.bin", 9);
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
        .expect("archive fits root policy");
    let snapshot = ArchiveSnapshot::new(&ctx, root).expect("archive snapshots");
    let entries = snapshot.container_entries(|_| ContainerRole::Stream);
    let stored = entries
        .iter()
        .find(|entry| entry.name == "stored.bin")
        .expect("stored entry summarized");

    assert_eq!(
        stored.storage,
        cadmpeg_core::container::EntryStorage::Verbatim {
            label: VerbatimLabel::Stored,
            size: VerbatimSize::PayloadOnly(9),
        }
    );
    assert_eq!(stored.storage.stored_size(), None);
    assert_eq!(stored.storage.expanded_size(), Some(9));
    assert_eq!(
        stored.attributes["storage_declaration"],
        "verbatim container entry stores fewer bytes than it expands to: 6/9"
    );

    let deflated = entries
        .iter()
        .find(|entry| entry.name == "deflated.bin")
        .expect("deflated entry summarized");
    assert!(!deflated.attributes.contains_key("storage_declaration"));
}

#[test]
fn snapshot_opens_supported_entries_after_parser_drop() {
    let bytes = archive_bytes();
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
        .expect("archive fits root policy");
    let snapshot = ArchiveSnapshot::new(&ctx, root).expect("archive snapshots");
    assert_eq!(snapshot.entries().len(), 3);
    let stored = snapshot.entry("stored.bin").expect("stored record");
    let deflated = snapshot.entry("deflated.bin").expect("deflated record");
    let zstd = snapshot.entry("zstd.bin").expect("Zstandard record");
    assert_eq!(
        snapshot
            .open(&ctx, &stored.name)
            .expect("stored opens")
            .window(),
        b"stored"
    );
    assert_eq!(
        snapshot
            .open(&ctx, &deflated.name)
            .expect("deflated opens")
            .window(),
        b"deflated payload"
    );
    assert_eq!(
        snapshot
            .open(&ctx, &zstd.name)
            .expect("Zstandard entry opens")
            .window(),
        zip_fixture::zstd_golden_plaintext()
    );
}

#[test]
fn zip_deflate_open_rejects_trailing_declared_payload_bytes() {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file(
            "deflated.bin",
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )
        .expect("deflate entry starts");
    writer.write_all(b"one member").expect("entry writes");
    let mut bytes = writer.finish().expect("ZIP finishes").into_inner();
    let entry = {
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("build the decode context");
        ArchiveSnapshot::new(&ctx, root)
            .expect("original directory is valid")
            .entry("deflated.bin")
            .expect("deflate entry exists")
            .clone()
    };
    let suffix = b"suffix";
    let payload_end =
        usize::try_from(entry.data_end().expect("payload end")).expect("payload end fits memory");
    bytes.splice(payload_end..payload_end, suffix.iter().copied());
    let header = usize::try_from(entry.header_start).expect("header fits memory");
    let central =
        usize::try_from(entry.central_start).expect("central header fits memory") + suffix.len();
    let compressed_size =
        u32::try_from(entry.compressed_size + cadmpeg_core::decode::u64_from_index(suffix.len()))
            .expect("fixture compressed size fits u32");
    bytes[header + 18..header + 22].copy_from_slice(&compressed_size.to_le_bytes());
    bytes[central + 20..central + 24].copy_from_slice(&compressed_size.to_le_bytes());
    let end = bytes
        .windows(4)
        .rposition(|signature| signature == b"PK\x05\x06")
        .expect("ZIP end record exists");
    let central_start = u32::try_from(central).expect("fixture directory start fits u32");
    bytes[end + 16..end + 20].copy_from_slice(&central_start.to_le_bytes());

    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("build the decode context");
    let archive = ArchiveSnapshot::new(&ctx, root).expect("patched directory is valid");
    assert!(matches!(
        archive.open(&ctx, "deflated.bin"),
        Err(CodecError::Malformed(message))
            if message == "raw-DEFLATE member does not exhaust its declared ZIP payload"
    ));
}

#[test]
fn snapshot_opens_names_using_its_own_metadata() {
    let bytes = archive_bytes();
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
        .expect("archive fits root policy");
    let first = ArchiveSnapshot::new(&ctx, root).expect("first archive snapshot");
    let second = ArchiveSnapshot::new(&ctx, root).expect("second archive snapshot");
    let mut detached = second.entry("stored.bin").expect("entry exists").clone();
    detached.data_start = Some(u64::MAX);
    detached.crc32 = 0;
    assert_eq!(
        first
            .open(&ctx, &detached.name)
            .expect("name opens own metadata")
            .window(),
        b"stored"
    );
    assert!(first.open(&ctx, "missing.bin").is_err());
}

#[test]
fn snapshot_opens_entries_from_a_nonzero_root_view() {
    let archive = archive_bytes();
    let mut bytes = b"prefix".to_vec();
    let start = bytes.len();
    bytes.extend_from_slice(&archive);
    let end = bytes.len();
    bytes.extend_from_slice(b"suffix");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
        .expect("outer bytes fit root policy");
    let nested = root.child(start, end).expect("archive child range");
    let snapshot = ArchiveSnapshot::new(&ctx, nested).expect("nested archive snapshots");

    let zstd_plain = zip_fixture::zstd_golden_plaintext();
    for (name, expected) in [
        ("stored.bin", b"stored".as_slice()),
        ("deflated.bin", b"deflated payload".as_slice()),
        ("zstd.bin", zstd_plain.as_slice()),
    ] {
        let entry = snapshot.entry(name).expect("entry exists");
        assert_eq!(
            snapshot
                .open(&ctx, &entry.name)
                .expect("nested entry opens")
                .window(),
            expected
        );
    }
}

#[test]
fn nested_archive_members_keep_distinct_address_spaces() {
    let mut inner = zip::ZipWriter::new(Cursor::new(Vec::new()));
    inner
        .start_file("Data/payload bytes.bin", SimpleFileOptions::default())
        .expect("nested archive fixture");
    inner
        .write_all(b"payload bytes")
        .expect("nested archive fixture");
    let inner_bytes = inner.finish().expect("nested archive fixture").into_inner();
    let mut outer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    outer
        .start_file("Assets/inner archive.zip", SimpleFileOptions::default())
        .expect("nested archive fixture");
    outer
        .write_all(&inner_bytes)
        .expect("nested archive fixture");
    let bytes = outer.finish().expect("nested archive fixture").into_inner();
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
        .expect("nested archive fixture");
    let archive = ArchiveSnapshot::new(&ctx, root).expect("nested archive fixture");
    let inner_view = archive
        .open(&ctx, "Assets/inner archive.zip")
        .expect("nested archive fixture");
    let nested = ArchiveSnapshot::new(&ctx, inner_view).expect("nested archive fixture");
    let payload = nested
        .open(&ctx, "Data/payload bytes.bin")
        .expect("nested archive fixture");
    assert_eq!(payload.window(), b"payload bytes");
    assert_ne!(root.location().space, inner_view.location().space);
    assert_ne!(inner_view.location().space, payload.location().space);
    assert_eq!(payload.location_at(7).offset, 7);
}

mod compression;
mod provenance;
