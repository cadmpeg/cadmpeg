// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};

use crate::framing::{FrameParent, PartTable};
use crate::zip_fixture::{archive, crc32, RawEntry};

use super::{frame, rebuild, Edit};

fn source(bytes: &[u8], run: impl FnOnce(&DecodeContext<'_>, View<'_>)) {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())
        .expect("literal ZIP source");
    run(&ctx, root);
}
fn word(bytes: &[u8], at: usize) -> u32 {
    View::u32_le_at(bytes, at).expect("literal ZIP u32")
}
fn put(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn descriptor_archive(signed: bool) -> (Vec<u8>, usize, usize) {
    let mut bytes = archive(&[RawEntry::stored("data", b"ABCDE").with_flags(8)]);
    let payload = 34;
    let descriptor = payload + 5;
    let mut tail = Vec::new();
    if signed {
        tail.extend_from_slice(b"PK\x07\x08");
    }
    tail.extend_from_slice(&crc32(b"ABCDE").to_le_bytes());
    tail.extend_from_slice(&5_u32.to_le_bytes());
    tail.extend_from_slice(&5_u32.to_le_bytes());
    let shift = tail.len();
    bytes.splice(descriptor..descriptor, tail);
    put(&mut bytes, 14, 0);
    put(&mut bytes, 18, 0);
    put(&mut bytes, 22, 0);
    let end = bytes.len() - 22;
    put(
        &mut bytes,
        end + 16,
        u32::try_from(descriptor + shift).expect("fixture directory offset"),
    );
    (bytes, payload, descriptor)
}

#[test]
fn actual_headers_names_extras_prefix_and_comments_keep_exact_source_coordinates() {
    let mut first = RawEntry::stored("one", b"1234");
    first.extra = vec![0x34, 0x12, 2, 0, 0xa5, 0x5a];
    let plain = archive(&[first, RawEntry::stored("two", b"ABCDE")]);
    let mut bytes = b"prefix".to_vec();
    bytes.extend(plain);
    let end = bytes.len() - 22;
    let comment = b"comment PK\x05\x06 incomplete";
    bytes[end + 20..end + 22].copy_from_slice(
        &u16::try_from(comment.len())
            .expect("comment length")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(comment);
    source(&bytes, |ctx, view| {
        let mut frames = Vec::new();
        let entries = frame(ctx, view, &mut frames).expect("shifted source directory");
        PartTable::load()
            .expect("table")
            .validate(ctx, &frames)
            .expect("actual containment");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name().window(), b"one");
        assert_eq!(entries[0].payload().window(), b"1234");
        assert_eq!(entries[0].payload().start(), 45);
        assert_eq!(entries[1].name().window(), b"two");
        assert_eq!(entries[1].payload().window(), b"ABCDE");
        assert_eq!(
            frames[entries[0].frame_index()].parent(),
            FrameParent::Part(0)
        );
        let extra = frames
            .iter()
            .filter(|part| {
                matches!(
                    part.level().as_str(),
                    "zip.central_extra_field" | "zip.local_extra_field"
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(extra.len(), 2);
        for part in extra {
            assert_eq!(part.view().window(), [0x34, 0x12, 2, 0, 0xa5, 0x5a]);
            assert_eq!(
                part.framing(),
                [u64::try_from(part.view().start()).expect("offset")
                    ..u64::try_from(part.view().start() + 4).expect("offset")]
            );
        }
        let ending = frames
            .iter()
            .find(|part| part.level().as_str() == "zip.end_record")
            .expect("EOCD");
        assert_eq!(ending.view().start(), end);
        assert_eq!(
            ending.framing(),
            [u64::try_from(end).expect("offset")..u64::try_from(end + 22).expect("offset")]
        );
        assert_eq!(
            rebuild(ctx, view, &[]).expect("no-op metadata preservation"),
            bytes
        );
        assert!(ctx
            .take_record_captures()
            .expect("no semantic reads")
            .is_empty());
    });
}

#[test]
fn stored_views_host_real_step_and_freecad_member_framers() {
    let step = b"ISO-10303-21;HEADER;ENDSEC;DATA;#1=ITEM(1);ENDSEC;END-ISO-10303-21;";
    let document = b"<Document><Objects><Object name='A'/></Objects></Document>";
    let bytes = archive(&[
        RawEntry::stored("ISO-10303.p21", step),
        RawEntry::stored("Document.xml", document),
    ]);
    source(&bytes, |ctx, view| {
        let mut frames = Vec::new();
        crate::contract::zip::frame_stored_members(ctx, view, &mut frames)
            .expect("real archive parents and embedded source members");
        PartTable::load()
            .expect("table")
            .validate(ctx, &frames)
            .expect("shared roots and member containment");
        assert!(frames.iter().any(
            |part| part.level().as_str() == "step.simple_entity_instance"
                && part.view().window() == b"#1=ITEM(1);"
        ));
        assert!(frames
            .iter()
            .any(|part| part.level().as_str() == "freecad.object_declaration"
                && part.view().window() == b"<Object name='A'/>"));
    });
}

#[test]
fn empty_members_and_empty_directories_keep_their_genuine_zero_byte_extents() {
    for bytes in [archive(&[]), archive(&[RawEntry::stored("empty", b"")])] {
        source(&bytes, |ctx, view| {
            let mut frames = Vec::new();
            let entries = frame(ctx, view, &mut frames).expect("zero bytes are a real extent");
            PartTable::load()
                .expect("table")
                .validate(ctx, &frames)
                .expect("zero-byte sibling boundary");
            if entries.is_empty() {
                let directory = frames
                    .iter()
                    .find(|part| part.level().as_str() == "zip.central_directory")
                    .expect("actual empty directory");
                assert_eq!(directory.view().start(), directory.view().end());
            } else {
                assert_eq!(entries[0].payload().window(), b"");
                let next = frames
                    .iter()
                    .find(|part| part.level().as_str() == "zip.central_directory")
                    .expect("next boundary");
                assert_eq!(entries[0].payload().start(), next.view().start());
            }
            assert_eq!(
                rebuild(ctx, view, &[]).expect("exact empty-source preservation"),
                bytes
            );
        });
    }
}

#[test]
fn signed_and_unsigned_descriptors_are_actual_frames_and_reseal_without_rewriting_metadata() {
    for signed in [false, true] {
        let (bytes, payload, descriptor) = descriptor_archive(signed);
        source(&bytes, |ctx, view| {
            let mut frames = Vec::new();
            let entries = frame(ctx, view, &mut frames).expect("actual ZIP32 descriptor");
            assert_eq!(entries.len(), 1);
            let desc = frames
                .iter()
                .find(|part| part.level().as_str() == "zip.data_descriptor")
                .expect("descriptor frame");
            let prefix = if signed { 4 } else { 0 };
            let end = descriptor + prefix + 12;
            assert_eq!((desc.view().start(), desc.view().end()), (descriptor, end));
            assert!(desc.framing().iter().all(|range| !(range.start
                <= u64::try_from(descriptor + prefix).expect("offset")
                && u64::try_from(descriptor + prefix).expect("offset") < range.end)));
            let output = rebuild(
                ctx,
                view,
                &[Edit {
                    range: u64::try_from(payload + 1).expect("offset")
                        ..u64::try_from(payload + 4).expect("offset"),
                    replacement: b"xyz",
                }],
            )
            .expect("stored edit and descriptor seal");
            let mut expected = bytes.clone();
            expected[payload + 1..payload + 4].copy_from_slice(b"xyz");
            let crc = crc32(b"AxyzE");
            put(&mut expected, end + 16, crc);
            put(&mut expected, descriptor + prefix, crc);
            assert_eq!(output, expected);
            assert_eq!(word(&output, 14), 0);
            assert_eq!(word(&output, end + 16), crc);
        });
    }
}

#[test]
fn payload_damage_reseals_both_headers_and_preserves_every_unedited_source_byte() {
    let bytes = archive(&[
        RawEntry::stored("one", b"1234"),
        RawEntry::stored("two", b"ABCDE"),
    ]);
    source(&bytes, |ctx, view| {
        let mut frames = Vec::new();
        let entries = frame(ctx, view, &mut frames).expect("two members");
        let payload = entries[1].payload().start();
        let central =
            usize::try_from(word(&bytes, bytes.len() - 22 + 16)).expect("directory offset") + 49;
        let local = payload - 33;
        for replacement in [[0_u8; 5], [0xff; 5]] {
            let output = rebuild(
                ctx,
                view,
                &[Edit {
                    range: u64::try_from(payload).expect("offset")
                        ..u64::try_from(payload + 5).expect("offset"),
                    replacement: &replacement,
                }],
            )
            .expect("stored member seal");
            let mut expected = bytes.clone();
            expected[payload..payload + 5].copy_from_slice(&replacement);
            put(&mut expected, local + 14, crc32(&replacement));
            put(&mut expected, central + 16, crc32(&replacement));
            assert_eq!(output, expected);
        }
    });
}

#[test]
fn invalid_extent_extra_descriptor_and_ambiguous_end_candidates_never_create_success() {
    let original = archive(&[RawEntry::stored("data", b"ABCDE")]);
    let mut invalid_offset = original.clone();
    let central = usize::try_from(word(&original, original.len() - 6)).expect("central offset");
    put(&mut invalid_offset, central + 42, 1);
    let mut zip64 = original.clone();
    put(&mut zip64, central + 20, u32::MAX);
    let mut extra_entry = RawEntry::stored("data", b"ABCDE");
    extra_entry.extra = vec![0x34, 0x12, 9, 0, 0];
    let bad_extra = archive(&[extra_entry]);
    let (mut bad_descriptor, _, descriptor) = descriptor_archive(true);
    put(&mut bad_descriptor, descriptor + 8, 999);
    let mut ambiguous = archive(&[]);
    ambiguous[20..22].copy_from_slice(&22_u16.to_le_bytes());
    let mut other = archive(&[]);
    put(&mut other, 16, 22);
    ambiguous.extend(other);
    for bytes in [
        invalid_offset,
        zip64,
        bad_extra,
        bad_descriptor,
        ambiguous,
        original[..original.len() - 1].to_vec(),
    ] {
        source(&bytes, |ctx, view| {
            assert!(frame(ctx, view, &mut Vec::new()).is_err())
        });
    }
}

#[test]
fn edit_boundary_compression_encryption_and_source_provenance_are_enforced() {
    for entry in [
        RawEntry::compressed("data", 8, b"\x03\x00", b""),
        RawEntry::stored("data", b"ABCDE").with_flags(1),
    ] {
        let bytes = archive(&[entry]);
        source(&bytes, |ctx, view| {
            let mut frames = Vec::new();
            let entries =
                frame(ctx, view, &mut frames).expect("physical compressed/encrypted extent");
            let start = u64::try_from(entries[0].payload().start()).expect("offset");
            assert!(rebuild(
                ctx,
                view,
                &[Edit {
                    range: start..start + 1,
                    replacement: b"x"
                }]
            )
            .is_err());
        });
    }
    let bytes = archive(&[RawEntry::stored("data", b"ABCDE")]);
    source(&bytes, |ctx, view| {
        assert!(rebuild(
            ctx,
            view,
            &[Edit {
                range: 0..1,
                replacement: b"x"
            }]
        )
        .is_err());
        assert!(rebuild(
            ctx,
            view,
            &[Edit {
                range: 34..35,
                replacement: b"xx"
            }]
        )
        .is_err());
        assert!(rebuild(
            ctx,
            view,
            &[
                Edit {
                    range: 34..37,
                    replacement: b"xxx"
                },
                Edit {
                    range: 36..38,
                    replacement: b"xx"
                }
            ]
        )
        .is_err());
        assert!(frame(ctx, View::over_retained(&bytes), &mut Vec::new()).is_err());
        source(&bytes, |_, foreign| {
            assert!(frame(ctx, foreign, &mut Vec::new()).is_err())
        });
    });
}

mod zip64;

mod signature;

mod recovery;
