// SPDX-License-Identifier: Apache-2.0
//! Genuine header and payload boundary controls.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_test_support::bytes::put_u16;
use cadmpeg_test_support::zip_fixture::{self, RawEntry};

use super::*;

#[test]
fn nonzero_member_frames_split_actual_header_payload_and_navigation_without_reads() {
    let mut entry = RawEntry::stored("é", b"payload");
    entry.extra = vec![0x34, 0x12, 1, 0, b'x'];
    let zip = zip_fixture::archive(&[entry]);
    let mut bytes = b"prefix".to_vec();
    let origin = bytes.len();
    bytes.extend_from_slice(&zip);
    let end = bytes.len();
    bytes.extend_from_slice(b"suffix");
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let source = root.child(origin, end).unwrap();
    let snapshot = ArchiveSnapshot::new(&ctx, source).unwrap();
    ctx.record("navigation", source, || {
        let frames = snapshot.member_frames(&ctx)?;
        let frame = frames[0].unwrap();
        let header_len = 30 + "é".len() + 5;
        assert_eq!(frame.header().window(), &zip[..header_len]);
        assert_eq!(frame.payload().window(), b"payload");
        assert_eq!(frame.header().start(), origin);
        assert_eq!(frame.header().end(), origin + header_len);
        assert_eq!(frame.payload().start(), origin + header_len);
        assert_eq!(frame.payload().end(), origin + header_len + 7);
        let expected = [(0, 4), (6, 8), (18, 22), (26, 30)];
        for (field, (start, end)) in frame.header_framing()?.into_iter().zip(expected) {
            assert_eq!(
                field.location(),
                root.child(origin + start, origin + end).unwrap().location()
            );
            assert_eq!(field.window(), &zip[start..end]);
        }
        let admitted = snapshot.read_member_header(&ctx, frame)?;
        assert_eq!(admitted.view().location(), frame.payload().location());
        assert_eq!(snapshot.open_member(&ctx, admitted)?.window(), b"payload");
        Ok(())
    })
    .unwrap();
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert!(captures[0].reads().is_empty());
}

#[test]
fn a_bad_signature_is_bounded_but_an_escaping_local_length_is_not() {
    let original = zip_fixture::archive(&[
        RawEntry::stored("broken", b"opaque"),
        RawEntry::stored("peer", b"readable"),
    ]);
    for bad_length in [false, true] {
        let mut bytes = original.clone();
        if bad_length {
            put_u16(&mut bytes, 26, u16::MAX);
        } else {
            bytes[0] ^= 1;
        }
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        let snapshot = ArchiveSnapshot::new(&ctx, root).unwrap();
        assert!(snapshot.entry("broken").unwrap().data_start.is_none());
        let frames = snapshot.member_frames(&ctx).unwrap();
        if bad_length {
            assert!(frames[0].is_none());
        } else {
            let frame = frames[0].unwrap();
            assert_eq!(frame.header().window(), &bytes[..36]);
            assert_eq!(frame.payload().window(), b"opaque");
            assert!(snapshot.read_member_header(&ctx, frame).is_err());
        }
        let peer = snapshot
            .read_member_header(&ctx, frames[1].unwrap())
            .unwrap();
        assert_eq!(
            snapshot.open_member(&ctx, peer).unwrap().window(),
            b"readable"
        );
        assert!(ctx.take_record_captures().unwrap().is_empty());
    }
}

#[test]
fn header_and_payload_proofs_refuse_another_archive_or_session() {
    let zip = zip_fixture::archive(&[RawEntry::stored("entry", b"data")]);
    let mut bytes = zip.clone();
    bytes.extend_from_slice(&zip);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let first = ArchiveSnapshot::new(&ctx, root.child(0, zip.len()).unwrap()).unwrap();
    let second = ArchiveSnapshot::new(&ctx, root.child(zip.len(), bytes.len()).unwrap()).unwrap();
    let full = ArchiveSnapshot::new(&ctx, root).unwrap();
    let frame = first.member_frames(&ctx).unwrap()[0].unwrap();
    assert!(full.read_member_header(&ctx, frame).is_err());
    let payload = first.read_member_header(&ctx, frame).unwrap();
    assert!(full.open_member(&ctx, payload).is_err());
    assert!(second.read_member_header(&ctx, frame).is_err());
    let payload = first.read_member_header(&ctx, frame).unwrap();
    assert!(second.open_member(&ctx, payload).is_err());
    let foreign_arena = DecodeArena::new();
    let (foreign, _) =
        DecodeContext::from_root_bytes(&bytes, &foreign_arena, &DecodePolicy::service()).unwrap();
    assert!(first.member_frames(&foreign).is_err());
    assert!(first.read_member_header(&foreign, frame).is_err());
    let payload = first.read_member_header(&ctx, frame).unwrap();
    assert!(first.open_member(&foreign, payload).is_err());
}

#[test]
fn member_framing_and_header_checks_retain_no_bytes() {
    let zip = zip_fixture::archive(&[
        RawEntry::stored("first", b"one"),
        RawEntry::stored("second", b"two"),
    ]);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&zip, &arena, &DecodePolicy::service()).unwrap();
    let snapshot = ArchiveSnapshot::new(&ctx, root).unwrap();
    let available = ctx.retained_bytes_available();
    for _ in 0..2 {
        for frame in snapshot.member_frames(&ctx).unwrap() {
            snapshot.read_member_header(&ctx, frame.unwrap()).unwrap();
        }
    }
    assert_eq!(ctx.retained_bytes_available(), available);
}
