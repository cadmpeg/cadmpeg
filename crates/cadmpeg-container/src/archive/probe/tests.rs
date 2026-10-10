// SPDX-License-Identifier: Apache-2.0
//! ZIP64 footer boundaries and non-structural signatures in comments.

use super::read;
use crate::{
    layout::{end_record, zip64_end, zip64_locator},
    ArchiveSnapshot,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use std::io::{Cursor, Write};

#[test]
fn a_zip64_record_must_fit_its_fixed_fields_before_the_locator() {
    use cadmpeg_test_support::bytes::{put_u32, put_u64};
    for size in [0, 43, 44] {
        let mut bytes = vec![0; size + 12 + zip64_locator::LEN];
        put_u32(&mut bytes, 0, 0x0606_4b50);
        put_u64(
            &mut bytes,
            zip64_end::RECORD_SIZE,
            cadmpeg_core::decode::u64_from_index(size),
        );
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("context");
        let found = super::find_zip64(&ctx, &mut Cursor::new(bytes), 0, size + 12, 0)
            .expect("boundary probe");
        assert_eq!(found, (size == 44).then_some(0));
        ctx.finish_session().expect("session");
    }
}

#[test]
fn zip64_end_record_with_maximum_comment_keeps_the_directory_namespace() {
    use cadmpeg_test_support::bytes::{put_u16, put_u32, put_u64};
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file("Document.xml", zip::write::SimpleFileOptions::default())
        .expect("member");
    writer.write_all(b"authored document").expect("payload");
    let original = writer.finish().expect("archive").into_inner();
    let end = original.len() - end_record::LEN;
    let directory_size =
        View::u32_le_at(&original, end + end_record::DIRECTORY_SIZE).expect("size");
    let directory_start =
        View::u32_le_at(&original, end + end_record::DIRECTORY_POSITION).expect("offset");
    for (count_sentinel, size_sentinel) in [(false, false), (true, true), (false, true)] {
        let mut bytes = original[..end].to_vec();
        let record = bytes.len();
        bytes.resize(record + zip64_end::LEN + zip64_locator::LEN, 0);
        put_u32(&mut bytes, record, 0x0606_4b50);
        put_u64(&mut bytes, record + zip64_end::RECORD_SIZE, 44);
        put_u16(&mut bytes, record + zip64_end::PRODUCER_VERSION, 45);
        put_u16(&mut bytes, record + zip64_end::EXTRACTION_VERSION, 45);
        put_u64(&mut bytes, record + zip64_end::DISK_ENTRIES, 1);
        put_u64(&mut bytes, record + zip64_end::ENTRIES, 1);
        put_u64(
            &mut bytes,
            record + zip64_end::DIRECTORY_SIZE,
            u64::from(directory_size),
        );
        put_u64(
            &mut bytes,
            record + zip64_end::DIRECTORY_POSITION,
            u64::from(directory_start),
        );
        let locator = record + zip64_end::LEN;
        put_u32(&mut bytes, locator, 0x0706_4b50);
        put_u64(
            &mut bytes,
            locator + zip64_locator::END_RECORD_POSITION,
            cadmpeg_core::decode::u64_from_index(record),
        );
        put_u32(&mut bytes, locator + zip64_locator::DISKS, 1);
        let end = bytes.len();
        bytes.extend_from_slice(&original[original.len() - end_record::LEN..]);
        if count_sentinel {
            put_u16(&mut bytes, end + end_record::DISK_ENTRIES, u16::MAX);
            put_u16(&mut bytes, end + end_record::ENTRIES, u16::MAX);
        }
        if size_sentinel {
            put_u32(&mut bytes, end + end_record::DIRECTORY_SIZE, u32::MAX);
            put_u32(&mut bytes, end + end_record::DIRECTORY_POSITION, u32::MAX);
        }
        put_u16(&mut bytes, end + end_record::COMMENT_LENGTH, u16::MAX);
        bytes.resize(bytes.len() + usize::from(u16::MAX), b'c');
        bytes[0] ^= 1;
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 256 * 1024;
        policy.limits.max_collection_items = 32;
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
        let (image, storage) = read(&ctx, &mut Cursor::new(&bytes))
            .expect("probe")
            .expect("namespace image");
        assert!(ArchiveSnapshot::contains_matching_name(
            &ctx,
            View::over_retained(&image),
            |name| name == "Document.xml"
        )
        .expect("admitted namespace"));
        drop((image, storage));
        let snapshot = ArchiveSnapshot::new(&ctx, root).expect("native ZIP64 namespace");
        assert!(snapshot.entry("Document.xml").is_some());
        ctx.finish_session().expect("session");
    }
}

#[test]
fn an_end_signature_inside_an_opaque_comment_does_not_reserve_payload_workspace() {
    use cadmpeg_test_support::bytes::put_u32;
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file(
            "Document.xml",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )
        .expect("member");
    writer
        .write_all(&vec![b'x'; 2 * 1024 * 1024])
        .expect("payload");
    let mut comment = vec![0; end_record::LEN];
    put_u32(&mut comment, 0, 0x0605_4b50);
    put_u32(&mut comment, end_record::DIRECTORY_SIZE, 1024 * 1024);
    writer
        .set_raw_comment(comment.into_boxed_slice())
        .expect("opaque comment");
    let bytes = writer.finish().expect("archive").into_inner();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 256 * 1024;
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
    let (image, storage) = read(&ctx, &mut Cursor::new(&bytes))
        .expect("probe")
        .expect("namespace image");
    assert!(image.len() < 1024);
    assert!(ArchiveSnapshot::contains_matching_name(
        &ctx,
        View::over_retained(&image),
        |name| name == "Document.xml"
    )
    .expect("namespace"));
    drop((image, storage));
    let snapshot = ArchiveSnapshot::new(&ctx, root)
        .expect("opaque footer signatures do not change native namespace selection");
    assert!(snapshot.entry("Document.xml").is_some());
    assert_eq!(
        snapshot
            .open(&ctx, "Document.xml")
            .expect("member")
            .window()
            .len(),
        2 * 1024 * 1024
    );
    let spans = snapshot
        .physical_ledger(&ctx)
        .expect("original source partition");
    assert_eq!(
        spans.last().expect("end record").end,
        cadmpeg_core::decode::u64_from_index(bytes.len())
    );
    ctx.finish_session().expect("session");
}

#[test]
fn a_false_comment_directory_is_framed_before_its_declared_bytes_are_acquired() {
    use crate::layout::{central_header, local_header};
    use cadmpeg_test_support::bytes::{put_u16, put_u32};
    for count in [0, 1] {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file(
                "Document.xml",
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .expect("member");
        let mut payload = vec![b'x'; 2 * 1024 * 1024];
        payload[..central_header::LEN].fill(0);
        payload[..4].copy_from_slice(b"PK\x01\x02");
        writer.write_all(&payload).expect("payload");
        writer
            .set_raw_comment(vec![0; end_record::LEN].into_boxed_slice())
            .expect("comment");
        let mut bytes = writer.finish().expect("archive").into_inner();
        let fake = bytes.len() - end_record::LEN;
        let payload_start = local_header::LEN + "Document.xml".len();
        put_u32(&mut bytes, fake, 0x0605_4b50);
        put_u16(&mut bytes, fake + end_record::DISK_ENTRIES, count);
        put_u16(&mut bytes, fake + end_record::ENTRIES, count);
        put_u32(
            &mut bytes,
            fake + end_record::DIRECTORY_SIZE,
            u32::try_from(fake - payload_start).expect("size"),
        );
        put_u32(
            &mut bytes,
            fake + end_record::DIRECTORY_POSITION,
            u32::try_from(payload_start).expect("offset"),
        );
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 256 * 1024;
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
        let (image, storage) = read(&ctx, &mut Cursor::new(&bytes))
            .expect("invalid comment declaration is skipped before allocation")
            .expect("directory");
        assert!(image.len() < 1024);
        assert!(ArchiveSnapshot::contains_matching_name(
            &ctx,
            View::over_retained(&image),
            |name| name == "Document.xml"
        )
        .expect("namespace"));
        drop((image, storage));
        assert!(ArchiveSnapshot::new(&ctx, root)
            .expect("original archive")
            .entry("Document.xml")
            .is_some());
        ctx.finish_session().expect("session");
    }
}

#[test]
fn signed_directory_detection_handles_included_and_excluded_signature_sizes() {
    use cadmpeg_test_support::bytes::put_u32;
    for prefix in [0, 123] {
        for included in [false, true] {
            let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
            writer
                .start_file("Document.xml", zip::write::SimpleFileOptions::default())
                .expect("member");
            writer.write_all(b"authored document").expect("payload");
            let mut bytes = writer.finish().expect("archive").into_inner();
            let end = bytes.len() - end_record::LEN;
            bytes.splice(end..end, *b"PK\x05\x05\x03\x00abc");
            if included {
                let size =
                    View::u32_le_at(&bytes, end + 9 + end_record::DIRECTORY_SIZE).expect("size");
                put_u32(&mut bytes, end + 9 + end_record::DIRECTORY_SIZE, size + 9);
            }
            bytes.splice(0..0, vec![b'x'; prefix]);
            let arena = DecodeArena::new();
            let policy = DecodePolicy::service();
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("context");
            let (image, storage) = read(&ctx, &mut Cursor::new(&bytes))
                .expect("probe")
                .expect("signed directory");
            assert!(ArchiveSnapshot::contains_matching_name(
                &ctx,
                View::over_retained(&image),
                |name| name == "Document.xml"
            )
            .expect("namespace"));
            drop((image, storage));
            ctx.finish_session().expect("session");
        }
    }
}

#[test]
fn an_embedded_member_directory_does_not_acquire_the_outer_payload_as_index_metadata() {
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut nested = zip::ZipWriter::new(Cursor::new(Vec::new()));
    nested
        .start_file("notes.txt", options)
        .expect("nested member");
    nested
        .write_all(b"authored nested member")
        .expect("payload");
    let nested = nested.finish().expect("nested archive").into_inner();
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer.start_file("archive.zip", options).expect("member");
    writer.write_all(&nested).expect("nested payload");
    writer.start_file("preview.bin", options).expect("member");
    writer
        .write_all(&vec![b'x'; 2 * 1024 * 1024])
        .expect("large payload");
    writer.start_file("Document.xml", options).expect("member");
    writer
        .write_all(b"authored document")
        .expect("required payload");
    let bytes = writer.finish().expect("archive").into_inner();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 256 * 1024;
    policy.limits.max_collection_items = 32;
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
    let snapshot =
        ArchiveSnapshot::new(&ctx, root).expect("only outer directory metadata is acquired");
    assert_eq!(snapshot.entries().len(), 3);
    assert_eq!(
        snapshot
            .open(&ctx, "Document.xml")
            .expect("required member")
            .window(),
        b"authored document"
    );
    ctx.finish_session().expect("session");
}

#[test]
fn conflicting_terminal_namespaces_are_a_required_declaration_failure() {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file("Document.xml", zip::write::SimpleFileOptions::default())
        .expect("member");
    writer.write_all(b"authored document").expect("payload");
    let mut comment = vec![0; end_record::LEN];
    comment[..4].copy_from_slice(b"PK\x05\x06");
    writer
        .set_raw_comment(comment.into_boxed_slice())
        .expect("comment");
    let bytes = writer.finish().expect("archive").into_inner();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
    assert!(
        matches!(ArchiveSnapshot::new(&ctx, root), Err(cadmpeg_core::CodecError::Malformed(message)) if message.contains("ambiguous terminal"))
    );
    ctx.finish_session().expect("session");
}
