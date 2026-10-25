// SPDX-License-Identifier: Apache-2.0
use super::{archive, descriptor_archive, frame, source, RawEntry};
use crate::framing::zip::rebuild_member;
use std::io::Read;

pub(super) fn plain(entry: &crate::framing::zip::ZipEntry<'_>) -> Vec<u8> {
    match entry.method() {
        0 => entry.payload().window().to_vec(),
        8 => {
            let mut result = Vec::new();
            flate2::read::DeflateDecoder::new(entry.payload().window())
                .read_to_end(&mut result)
                .unwrap();
            result
        }
        other => panic!("unexpected fixture method {other}"),
    }
}

fn check(bytes: &[u8], replacement: &[u8]) -> Vec<u8> {
    let mut rebuilt = None;
    source(bytes, |ctx, view| {
        rebuilt = Some(rebuild_member(ctx, view, b"data", replacement).unwrap());
    });
    let rebuilt = rebuilt.unwrap();
    source(&rebuilt, |ctx, view| {
        let mut frames = Vec::new();
        let entries = frame(ctx, view, &mut frames).unwrap();
        let entry = entries
            .iter()
            .find(|entry| entry.name().window() == b"data")
            .unwrap();
        assert_eq!(plain(entry), replacement);
        assert!(ctx.take_record_captures().unwrap().is_empty());
    });
    rebuilt
}

#[test]
fn recompression_relocates_later_members_and_preserves_their_exact_bytes_and_metadata() {
    let expanded = b"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    let mut compressed = vec![1, 64, 0, 191, 255];
    compressed.extend_from_slice(expanded);
    let mut target = RawEntry::compressed("data", 8, &compressed, expanded);
    target.extra = vec![0x34, 0x12, 2, 0, 0xa5, 0x5a];
    let bytes = archive(&[target, RawEntry::stored("peer", b"exact untouched peer")]);
    let replacement: Vec<_> = (0_u8..64).collect();
    let rebuilt = check(&bytes, &replacement);
    assert_ne!(rebuilt.len(), bytes.len());
    source(&bytes, |ctx, view| {
        let mut before_frames = Vec::new();
        let before = frame(ctx, view, &mut before_frames).unwrap();
        assert_eq!(plain(&before[0]), expanded);
        source(&rebuilt, |ctx, view| {
            let mut after_frames = Vec::new();
            let after = frame(ctx, view, &mut after_frames).unwrap();
            assert_eq!(after[1].name().window(), b"peer");
            assert_eq!(after[1].payload().window(), before[1].payload().window());
            for level in ["zip.local_extra_field", "zip.central_extra_field"] {
                let old = before_frames
                    .iter()
                    .find(|part| part.level().as_str() == level)
                    .unwrap();
                let new = after_frames
                    .iter()
                    .find(|part| part.level().as_str() == level)
                    .unwrap();
                assert_eq!(new.view().window(), old.view().window());
            }
        });
    });
}

#[test]
fn stored_descriptors_preserve_deferred_local_fields_and_signature_forms() {
    for signed in [false, true] {
        let (bytes, _, _) = descriptor_archive(signed);
        let rebuilt = check(&bytes, b"VWXYZ");
        assert_eq!(&rebuilt[14..26], &bytes[14..26]);
        assert_eq!(rebuilt.len(), bytes.len());
    }
}

#[test]
fn replacement_refuses_changed_expanded_sizes_and_unsupported_methods() {
    for method in [0, 12] {
        let bytes = archive(&[RawEntry::compressed("data", method, b"ABCDE", b"ABCDE")]);
        source(&bytes, |ctx, view| {
            assert!(rebuild_member(ctx, view, b"data", b"shorter").is_err());
            if method == 12 {
                assert!(rebuild_member(ctx, view, b"data", b"VWXYZ").is_err());
            }
        });
    }
}
