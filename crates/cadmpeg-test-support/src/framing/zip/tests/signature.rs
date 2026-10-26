// SPDX-License-Identifier: Apache-2.0
use super::{archive, frame, put, rebuild, source, word, Edit, RawEntry};
use crate::framing::{FrameParent, FramedPart, PartTable};

struct Signed {
    bytes: Vec<u8>,
    signature: usize,
    next_end: usize,
    central: usize,
}

fn signed(inside: bool, wide: bool) -> Signed {
    let mut bytes = archive(&[RawEntry::stored("data", b"ABCDE")]);
    let old_end = bytes.len() - 22;
    let central = usize::try_from(word(&bytes, old_end + 16)).expect("literal central offset");
    let mut signature = b"PK\x05\x05".to_vec();
    signature.extend_from_slice(&16_u16.to_le_bytes());
    signature.extend_from_slice(b"opaque-signature");
    let signature_len = signature.len();
    bytes.splice(old_end..old_end, signature);
    let mut end = old_end + signature_len;
    if inside {
        let old_size = word(&bytes, end + 12);
        put(
            &mut bytes,
            end + 12,
            old_size + u32::try_from(signature_len).expect("literal signature size"),
        );
    }
    let next_end = end;
    if wide {
        let directory_size = word(&bytes, end + 12);
        let directory_offset = word(&bytes, end + 16);
        let mut records = b"PK\x06\x06".to_vec();
        records.extend_from_slice(&44_u64.to_le_bytes());
        records.extend_from_slice(&45_u16.to_le_bytes());
        records.extend_from_slice(&45_u16.to_le_bytes());
        records.extend_from_slice(&0_u32.to_le_bytes());
        records.extend_from_slice(&0_u32.to_le_bytes());
        records.extend_from_slice(&1_u64.to_le_bytes());
        records.extend_from_slice(&1_u64.to_le_bytes());
        records.extend_from_slice(&u64::from(directory_size).to_le_bytes());
        records.extend_from_slice(&u64::from(directory_offset).to_le_bytes());
        records.extend_from_slice(b"PK\x06\x07");
        records.extend_from_slice(&0_u32.to_le_bytes());
        records.extend_from_slice(&u64::try_from(end).expect("actual ZIP64 end").to_le_bytes());
        records.extend_from_slice(&1_u32.to_le_bytes());
        bytes.splice(end..end, records);
        end += 76;
        bytes[end + 8..end + 12].copy_from_slice(&[0xff; 4]);
        put(&mut bytes, end + 12, u32::MAX);
        put(&mut bytes, end + 16, u32::MAX);
    }
    Signed {
        bytes,
        signature: old_end,
        next_end,
        central,
    }
}

#[test]
fn real_signatures_inside_and_outside_declared_directory_keep_exact_extent_and_owner() {
    for inside in [false, true] {
        for wide in [false, true] {
            let input = signed(inside, wide);
            source(&input.bytes, |ctx, view| {
                let mut parts = Vec::new();
                frame(ctx, view, &mut parts).expect("actual central-header sequence and signature");
                PartTable::load()
                    .expect("table")
                    .validate(ctx, &parts)
                    .expect("actual physical parent and declared archive owner");
                let (directory_index, directory) = parts
                    .iter()
                    .enumerate()
                    .find(|(_, part)| part.level().as_str() == "zip.central_directory")
                    .expect("actual directory");
                let signature = parts
                    .iter()
                    .find(|part| part.level().as_str() == "zip.digital_signature")
                    .expect("actual signature");
                assert_eq!(directory.view().start(), input.central);
                assert_eq!(
                    directory.view().end(),
                    if inside {
                        input.next_end
                    } else {
                        input.signature
                    }
                );
                assert_eq!(
                    (signature.view().start(), signature.view().end()),
                    (input.signature, input.next_end)
                );
                assert_eq!(
                    signature.framing(),
                    [u64::try_from(input.signature).expect("offset")
                        ..u64::try_from(input.signature + 6).expect("offset")]
                );
                assert_eq!(
                    &input.bytes[input.signature + 6..input.next_end],
                    b"opaque-signature"
                );
                assert_eq!(
                    signature.parent(),
                    FrameParent::Part(if inside { directory_index } else { 0 })
                );
                assert_eq!(
                    rebuild(ctx, view, &[]).expect("opaque signature and metadata preserved"),
                    input.bytes
                );
            });
        }
    }
}

#[test]
fn physical_containment_requires_actual_zip_proof_and_does_not_waive_sibling_overlap() {
    let input = signed(true, false);
    source(&input.bytes, |ctx, view| {
        let mut parts = Vec::new();
        frame(ctx, view, &mut parts).expect("actual signed source");
        let index = parts
            .iter()
            .position(|part| part.level().as_str() == "zip.digital_signature")
            .expect("signature index");
        let signature = &parts[index];
        let level = signature.level();
        let original = signature.view();
        let framing = signature.framing().to_vec();
        let physical_parent = signature.parent();
        parts[index] = FramedPart::new(
            ctx,
            level,
            original,
            physical_parent,
            framing.clone(),
            vec![],
        )
        .expect("actual spans without source-parent proof");
        assert!(PartTable::load()
            .expect("table")
            .validate(ctx, &parts)
            .is_err());
        parts[index] = FramedPart::new(ctx, level, original, FrameParent::Part(0), framing, vec![])
            .expect("declared owner but overlapping physical sibling");
        assert!(PartTable::load()
            .expect("table")
            .validate(ctx, &parts)
            .is_err());
    });
}

#[test]
fn a_changed_central_crc_refuses_signature_renewal_and_noop_content_preserves_it() {
    for inside in [false, true] {
        let input = signed(inside, true);
        source(&input.bytes, |ctx, view| {
            let mut parts = Vec::new();
            let entries = frame(ctx, view, &mut parts).expect("actual signed source");
            let start = u64::try_from(entries[0].payload().start()).expect("source offset");
            assert_eq!(
                rebuild(
                    ctx,
                    view,
                    &[Edit {
                        range: start..start + 5,
                        replacement: b"ABCDE"
                    }]
                )
                .expect("identical covered headers"),
                input.bytes
            );
            let error = rebuild(
                ctx,
                view,
                &[Edit {
                    range: start..start + 5,
                    replacement: b"12345",
                }],
            )
            .expect_err("a copied signature cannot validate changed central headers");
            assert!(error.contains("signature renewal is unsupported"));
        });
    }
}

#[test]
fn unrelated_outer_stored_edit_preserves_every_byte_of_a_nested_signed_member() {
    let inner = signed(true, true);
    let outer = archive(&[
        RawEntry::stored("signed.zip", &inner.bytes),
        RawEntry::stored("other", b"ABCDE"),
    ]);
    source(&outer, |ctx, view| {
        let mut parts = Vec::new();
        let entries = frame(ctx, view, &mut parts).expect("actual outer members");
        let original = entries[0].payload();
        let edit = entries[1].payload();
        let output = rebuild(
            ctx,
            view,
            &[Edit {
                range: u64::try_from(edit.start()).expect("offset")
                    ..u64::try_from(edit.end()).expect("offset"),
                replacement: b"12345",
            }],
        )
        .expect("the unrelated archive member is not signed by the inner directory");
        assert_eq!(&output[original.start()..original.end()], inner.bytes);
        assert_eq!(&output[edit.start()..edit.end()], b"12345");
    });
}

#[test]
fn opaque_signature_markers_in_payload_are_not_additional_signature_records() {
    let bytes = archive(&[RawEntry::stored("data", b"PK\x05\x05\0\0 payload")]);
    source(&bytes, |ctx, view| {
        let mut parts = Vec::new();
        frame(ctx, view, &mut parts).expect("payload marker is content");
        assert!(!parts
            .iter()
            .any(|part| part.level().as_str() == "zip.digital_signature"));
    });
    let mut input = signed(true, false);
    input.bytes[input.signature + 4..input.signature + 6].copy_from_slice(&u16::MAX.to_le_bytes());
    source(&input.bytes, |ctx, view| {
        assert!(frame(ctx, view, &mut Vec::new()).is_err())
    });
    let input = signed(false, true);
    let mut cut = input.bytes.clone();
    cut.remove(input.signature + 6);
    source(&cut, |ctx, view| {
        assert!(frame(ctx, view, &mut Vec::new()).is_err())
    });
}

#[test]
fn stored_resizing_refuses_signature_invalidation_and_preserves_unchanged_signed_sources() {
    for inside in [false, true] {
        for wide in [false, true] {
            let input = signed(inside, wide);
            source(&input.bytes, |ctx, view| {
                let error = crate::framing::zip::resize_stored_member(ctx, view, b"data", b"AB")
                    .unwrap_err();
                assert!(error.contains("invalidates the directory signature"));
                assert_eq!(
                    crate::framing::zip::resize_stored_member(ctx, view, b"data", b"ABCDE")
                        .unwrap(),
                    input.bytes
                );
            });
        }
    }
}
