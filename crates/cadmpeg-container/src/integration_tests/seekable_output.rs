// SPDX-License-Identifier: Apache-2.0
//! ZIP serialization completes under the actual caller's output account.

use std::io::{Cursor, Write};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::encode::EncodeSession;
use cadmpeg_core::io::WriteSeek;
use zip::result::ZipResult;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipWriter};

use crate::ArchiveSnapshot;

fn members(output: &mut dyn WriteSeek, method: CompressionMethod) -> ZipResult<()> {
    let mut zip = ZipWriter::new(output);
    let options = SimpleFileOptions::default()
        .compression_method(method)
        .last_modified_time(DateTime::from_date_and_time(2024, 1, 2, 3, 4, 6).unwrap());
    for (name, bytes) in [
        ("geometry.bin", b"authored geometry".as_slice()),
        ("metadata.txt", b"authored metadata".as_slice()),
    ] {
        zip.start_file(name, options)?;
        zip.write_all(bytes)?;
    }
    zip.finish()?;
    Ok(())
}

#[test]
fn bounded_stored_and_deflated_zip_bytes_match_the_same_cursor_writer() {
    for method in [CompressionMethod::Stored, CompressionMethod::Deflated] {
        let mut cursor = Cursor::new(Vec::new());
        members(&mut cursor, method).unwrap();
        let arena = DecodeArena::new();
        let ctx = DecodeContext::for_encode(&arena, &DecodePolicy::service());
        let session = EncodeSession::from_context(&ctx).unwrap();
        let (bytes, ()) = session
            .write_seekable("two authored ZIP members", |output| members(output, method))
            .unwrap()
            .unwrap();
        assert_eq!(bytes, cursor.into_inner());
        session.finish().unwrap();
        ctx.finish_session().unwrap();
        let read_arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (read_ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &read_arena, &policy).unwrap();
        let archive = ArchiveSnapshot::new(&read_ctx, root).unwrap();
        assert_eq!(archive.entries().len(), 2);
        assert_eq!(
            archive.open(&read_ctx, "geometry.bin").unwrap().window(),
            b"authored geometry"
        );
        assert_eq!(
            archive.open(&read_ctx, "metadata.txt").unwrap().window(),
            b"authored metadata"
        );
        read_ctx.finish_session().unwrap();
    }
}

#[test]
fn real_zip_resource_refusal_cannot_escape_as_operation_io() {
    for method in [CompressionMethod::Stored, CompressionMethod::Deflated] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let ctx = DecodeContext::for_encode(&arena, &policy);
        let session = EncodeSession::from_context(&ctx).unwrap();
        let refusal = session
            .write_seekable("ZIP with no retained allowance", |output| {
                members(output, method)
            })
            .unwrap_err();
        assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
        assert_eq!((refusal.limit, refusal.used), (0, 0));
        assert!(refusal.additional > 0);
        assert_eq!(ctx.resource_refusal(), Some(refusal));
        assert_eq!(session.finish(), Err(refusal));
    }
}

#[test]
fn live_sibling_output_is_not_reset_by_zip_serialization() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 4;
    let ctx = DecodeContext::for_encode(&arena, &policy);
    let session = EncodeSession::from_context(&ctx).unwrap();
    let mut held = session.buffer();
    held.append(b"held", "live sibling output").unwrap();
    let used = cadmpeg_core::decode::u64_from_index(held.capacity());
    let refusal = session
        .write_seekable("ZIP beside live output", |output| {
            members(output, CompressionMethod::Stored)
        })
        .unwrap_err();
    assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
    assert_eq!((refusal.limit, refusal.used), (4, used));
    assert!(refusal.additional > 0);
    assert_eq!(held.as_bytes(), b"held");
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    drop(held);
    assert_eq!(session.finish(), Err(refusal));
}

#[test]
fn central_directory_and_comment_refusal_is_checked_after_zip_finish() {
    const LIMIT: usize = 4096;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index(LIMIT);
    let ctx = DecodeContext::for_encode(&arena, &policy);
    let session = EncodeSession::from_context(&ctx).unwrap();
    let mut saw_finished_refusal = false;
    let refusal = session
        .write_seekable("ZIP final directory", |output| {
            let mut zip = ZipWriter::new(output);
            let options =
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zip.start_file("payload", options)?;
            zip.write_all(b"payload")?;
            zip.set_comment("x".repeat(LIMIT))?;
            session.finish().unwrap();
            let result = zip.finish();
            assert!(result.is_err());
            assert!(ctx.resource_refusal().is_some());
            saw_finished_refusal = true;
            result.map(|_| ())
        })
        .unwrap_err();
    assert!(saw_finished_refusal);
    assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(refusal.limit, cadmpeg_core::decode::u64_from_index(LIMIT));
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert_eq!(session.finish(), Err(refusal));
}
