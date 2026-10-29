// SPDX-License-Identifier: Apache-2.0
//! Genuine header and payload boundary controls.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_test_support::bytes::put_u16;
use cadmpeg_test_support::zip_fixture::{self, RawEntry};

use super::*;

#[test]
fn nonzero_member_frames_split_actual_header_and_payload() {
    let mut entry = RawEntry::stored("é", b"payload");
    entry.extra = vec![0x34, 0x12, 1, 0, b'x'];
    let zip = zip_fixture::archive(&[entry]);
    let mut bytes = b"prefix".to_vec();
    let origin = bytes.len();
    bytes.extend_from_slice(&zip);
    let end = bytes.len();
    bytes.extend_from_slice(b"suffix");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("the fixture fits the service policy");
    let source = root
        .child(origin, end)
        .expect("the archive range lies inside the root");
    let snapshot = ArchiveSnapshot::new(&ctx, source).expect("the embedded archive indexes");
    let frames = snapshot
        .member_frames(&ctx)
        .expect("the member frames locate");
    let frame = frames[0].expect("the stored entry has a frame");
    let header_len = 30 + "é".len() + 5;
    assert_eq!(frame.header().window(), &zip[..header_len]);
    assert_eq!(frame.payload().window(), b"payload");
    assert_eq!(frame.header().start(), origin);
    assert_eq!(frame.header().end(), origin + header_len);
    assert_eq!(frame.payload().start(), origin + header_len);
    assert_eq!(frame.payload().end(), origin + header_len + 7);
    let admitted = snapshot
        .read_member_header(frame)
        .expect("the local header admits the payload");
    assert_eq!(admitted.view().location(), frame.payload().location());
    assert_eq!(
        snapshot
            .open_member(&ctx, &admitted)
            .expect("the stored payload opens")
            .window(),
        b"payload"
    );
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
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("the fixture fits the service policy");
        let snapshot = ArchiveSnapshot::new(&ctx, root).expect("the central directory indexes");
        assert!(snapshot
            .entry("broken")
            .expect("the broken entry is indexed")
            .data_start
            .is_none());
        let frames = snapshot
            .member_frames(&ctx)
            .expect("the member frames locate");
        if bad_length {
            assert!(frames[0].is_none());
        } else {
            let frame = frames[0].expect("a bad signature keeps a bounded frame");
            assert_eq!(frame.header().window(), &bytes[..36]);
            assert_eq!(frame.payload().window(), b"opaque");
            assert!(snapshot.read_member_header(frame).is_err());
        }
        let peer = snapshot
            .read_member_header(frames[1].expect("the peer entry has a frame"))
            .expect("the peer local header admits the payload");
        assert_eq!(
            snapshot
                .open_member(&ctx, &peer)
                .expect("the peer payload opens")
                .window(),
            b"readable"
        );
    }
}

#[test]
fn header_and_payload_proofs_refuse_another_archive() {
    let zip = zip_fixture::archive(&[RawEntry::stored("entry", b"data")]);
    let mut bytes = zip.clone();
    bytes.extend_from_slice(&zip);
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("the doubled fixture fits the service policy");
    let first = ArchiveSnapshot::new(
        &ctx,
        root.child(0, zip.len())
            .expect("the first archive range lies inside the root"),
    )
    .expect("the first archive indexes");
    let second = ArchiveSnapshot::new(
        &ctx,
        root.child(zip.len(), bytes.len())
            .expect("the second archive range lies inside the root"),
    )
    .expect("the second archive indexes");
    let full = ArchiveSnapshot::new(&ctx, root).expect("the full buffer indexes as an archive");
    let frame = first
        .member_frames(&ctx)
        .expect("the first archive frames locate")[0]
        .expect("the first entry has a frame");
    assert!(full.read_member_header(frame).is_err());
    let payload = first
        .read_member_header(frame)
        .expect("the first archive admits its own header");
    assert!(full.open_member(&ctx, &payload).is_err());
    assert!(second.read_member_header(frame).is_err());
    let payload = first
        .read_member_header(frame)
        .expect("the first archive admits its own header");
    assert!(second.open_member(&ctx, &payload).is_err());
}

#[test]
fn member_framing_and_header_checks_retain_no_bytes() {
    let zip = zip_fixture::archive(&[
        RawEntry::stored("first", b"one"),
        RawEntry::stored("second", b"two"),
    ]);
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&zip, &arena, &DecodePolicy::service())
        .expect("the fixture fits the service policy");
    let snapshot = ArchiveSnapshot::new(&ctx, root).expect("the central directory indexes");
    let available = ctx.retained_bytes_available();
    for _ in 0..2 {
        for frame in snapshot
            .member_frames(&ctx)
            .expect("the member frames locate")
        {
            snapshot
                .read_member_header(frame.expect("each stored entry has a frame"))
                .expect("each local header admits its payload");
        }
    }
    assert_eq!(ctx.retained_bytes_available(), available);
}
