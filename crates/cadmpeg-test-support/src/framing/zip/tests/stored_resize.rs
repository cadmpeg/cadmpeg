// SPDX-License-Identifier: Apache-2.0
//! Stored resizing preserves authored ZIP metadata and existing edit refusals.

use super::{archive, frame, put, source, RawEntry};
use crate::framing::zip::{rebuild_member, resize_stored_member};
use crate::framing::PartTable;
use crate::zip_fixture::crc32;

fn decorated(payload: &[u8]) -> Vec<u8> {
    let mut target = RawEntry::stored("data", payload).with_flags(0x0800);
    target.extra = vec![0x34, 0x12, 2, 0, 0xa5, 0x5a];
    let mut peer = RawEntry::stored("peer", b"exact source peer");
    peer.extra = vec![0x78, 0x56, 1, 0, 0x69];
    let mut bytes = archive(&[target, peer]);
    let local = bytes
        .windows(4)
        .enumerate()
        .filter_map(|(at, marker)| (marker == b"PK\x03\x04").then_some(at))
        .collect::<Vec<_>>();
    let central = bytes
        .windows(4)
        .enumerate()
        .filter_map(|(at, marker)| (marker == b"PK\x01\x02").then_some(at))
        .collect::<Vec<_>>();
    assert_eq!(local.len(), 2);
    assert_eq!(central.len(), 2);
    for at in local {
        bytes[at + 4..at + 6].copy_from_slice(&20_u16.to_le_bytes());
        bytes[at + 10..at + 14].copy_from_slice(&[0x34, 0x12, 0x78, 0x56]);
    }
    for at in central {
        bytes[at + 4..at + 8].copy_from_slice(&[20, 3, 20, 0]);
        bytes[at + 12..at + 16].copy_from_slice(&[0x34, 0x12, 0x78, 0x56]);
        bytes[at + 36..at + 38].copy_from_slice(&0xa55a_u16.to_le_bytes());
        put(&mut bytes, at + 38, 0x81a4_0000);
    }
    let mut prefixed = b"actual-source-prefix".to_vec();
    prefixed.extend(bytes);
    let end = prefixed.len() - 22;
    let comment = b"actual source archive comment";
    prefixed[end + 20..end + 22]
        .copy_from_slice(&u16::try_from(comment.len()).unwrap().to_le_bytes());
    prefixed.extend_from_slice(comment);
    prefixed
}

fn descriptor(payload: &[u8], signed: bool) -> Vec<u8> {
    let mut bytes = archive(&[RawEntry::stored("data", payload).with_flags(8)]);
    let after_payload = 34 + payload.len();
    let mut words = Vec::new();
    if signed {
        words.extend_from_slice(b"PK\x07\x08");
    }
    words.extend_from_slice(&crc32(payload).to_le_bytes());
    let size = u32::try_from(payload.len()).unwrap();
    words.extend_from_slice(&size.to_le_bytes());
    words.extend_from_slice(&size.to_le_bytes());
    let directory = after_payload + words.len();
    bytes.splice(after_payload..after_payload, words);
    for at in [14, 18, 22] {
        put(&mut bytes, at, 0);
    }
    let end = bytes.len() - 22;
    put(&mut bytes, end + 16, u32::try_from(directory).unwrap());
    bytes
}

#[test]
fn resized_stored_sources_keep_exact_flags_metadata_extras_prefix_comments_and_peer() {
    let original = decorated(b"ABCDE");
    for replacement in [b"".as_slice(), b"AB", b"ABCDEFGHIJK"] {
        source(&original, |ctx, view| {
            assert!(rebuild_member(ctx, view, b"data", replacement).is_err());
            let rebuilt = resize_stored_member(ctx, view, b"data", replacement).unwrap();
            assert_eq!(rebuilt, decorated(replacement));
            source(&rebuilt, |ctx, view| {
                let mut parts = Vec::new();
                let entries = frame(ctx, view, &mut parts).unwrap();
                PartTable::load().unwrap().validate(ctx, &parts).unwrap();
                assert_eq!(entries[0].payload().window(), replacement);
                assert_eq!(entries[0].flags(), 0x0800);
                assert_eq!(entries[1].payload().window(), b"exact source peer");
                assert!(ctx.take_record_captures().unwrap().is_empty());
            });
        });
    }
}

#[test]
fn resized_stored_descriptors_preserve_deferred_local_words_and_both_signature_forms() {
    for signed in [false, true] {
        let original = descriptor(b"ABCDE", signed);
        for replacement in [b"".as_slice(), b"AB", b"ABCDEFGHIJK"] {
            source(&original, |ctx, view| {
                let rebuilt = resize_stored_member(ctx, view, b"data", replacement).unwrap();
                assert_eq!(rebuilt, descriptor(replacement, signed));
                assert_eq!(&rebuilt[14..26], &[0; 12]);
                source(&rebuilt, |ctx, view| {
                    let mut parts = Vec::new();
                    let entries = frame(ctx, view, &mut parts).unwrap();
                    PartTable::load().unwrap().validate(ctx, &parts).unwrap();
                    assert_eq!(entries[0].payload().window(), replacement);
                    assert_eq!(
                        parts
                            .iter()
                            .filter(|part| part.level().as_str() == "zip.data_descriptor")
                            .count(),
                        1
                    );
                });
            });
        }
    }
}

#[test]
fn stored_resizing_refuses_encryption_compression_and_foreign_source() {
    let mut encrypted = RawEntry::stored("data", b"ABCDE");
    encrypted.flags = 1;
    let mut raw_deflate = vec![1, 5, 0, 250, 255];
    raw_deflate.extend_from_slice(b"ABCDE");
    for bytes in [
        archive(&[encrypted]),
        archive(&[RawEntry::compressed("data", 8, &raw_deflate, b"ABCDE")]),
        archive(&[RawEntry::compressed("data", 12, b"ABCDE", b"ABCDE")]),
    ] {
        source(&bytes, |ctx, view| {
            assert!(resize_stored_member(ctx, view, b"data", b"AB").is_err());
        });
    }
}

#[test]
fn stored_resize_uses_the_actual_nonzero_source_window_and_rejects_foreign_authority() {
    let original = decorated(b"ABCDE");
    let mut wrapped = b"outside-prefix".to_vec();
    let start = wrapped.len();
    wrapped.extend_from_slice(&original);
    wrapped.extend_from_slice(b"outside-suffix");
    source(&wrapped, |ctx, root| {
        let view = root.child(start, start + original.len()).unwrap();
        assert_eq!(
            resize_stored_member(ctx, view, b"data", b"AB").unwrap(),
            decorated(b"AB")
        );
        source(&wrapped, |_, root| {
            let foreign = root.child(start, start + original.len()).unwrap();
            assert!(resize_stored_member(ctx, foreign, b"data", b"AB").is_err());
        });
    });
}

#[test]
fn same_length_member_rebuild_preserves_noncanonical_local_expanded_size() {
    let mut original = archive(&[RawEntry::stored("data", b"ABCDE")]);
    put(&mut original, 22, 123);
    source(&original, |ctx, view| {
        let entries = frame(ctx, view, &mut Vec::new()).unwrap();
        let entry = &entries[0];
        assert_eq!(entry.uncompressed, 5);
        assert_eq!(super::word(&original, 22), 123);
        let expected = crate::framing::zip::rebuild(
            ctx,
            view,
            &[crate::framing::zip::Edit {
                range: u64::try_from(entry.payload().start()).unwrap()
                    ..u64::try_from(entry.payload().end()).unwrap(),
                replacement: b"VWXYZ",
            }],
        )
        .unwrap();
        let rebuilt = rebuild_member(ctx, view, b"data", b"VWXYZ").unwrap();
        assert_eq!(rebuilt, expected);
        assert_eq!(super::word(&rebuilt, 22), 123);
    });
}

#[test]
fn member_rebuilders_refuse_absent_and_ambiguous_raw_names() {
    for (bytes, selected) in [
        (
            archive(&[RawEntry::stored("peer", b"ABCDE")]),
            b"missing".as_slice(),
        ),
        (
            archive(&[
                RawEntry::stored("data", b"ABCDE"),
                RawEntry::stored("data", b"FGHIJ"),
            ]),
            b"data".as_slice(),
        ),
    ] {
        source(&bytes, |ctx, view| {
            assert!(rebuild_member(ctx, view, selected, b"VWXYZ").is_err());
            assert!(resize_stored_member(ctx, view, selected, b"AB").is_err());
        });
    }
}
