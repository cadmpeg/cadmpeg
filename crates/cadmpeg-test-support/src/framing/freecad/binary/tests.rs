// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::framing::{zip, PartTable};
use crate::zip_fixture::{archive, RawEntry};
use cadmpeg_core::decode::{ByteRange, DecodeArena, DecodePolicy};

fn with_member(
    bytes: &[u8],
    run: impl for<'a> FnOnce(&DecodeContext<'a>, View<'a>, usize, &mut Vec<FramedPart<'a>>),
) {
    let bytes = archive(&[RawEntry::stored("Payload", bytes)]);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let root = ctx.name_stream(root, "file").unwrap();
    let mut frames = Vec::new();
    let entries = zip::frame(&ctx, root, &mut frames).unwrap();
    let payload = entries[0].payload();
    let member = ctx
        .register_named_slice(
            root,
            ByteRange {
                start: u64_from_index(payload.start()),
                end: u64_from_index(payload.end()),
            },
            "Payload",
        )
        .unwrap();
    run(&ctx, member, entries[0].frame_index(), &mut frames);
}

#[test]
fn counted_side_entries_use_exact_appendix_widths_and_header_framing() {
    for (kind, width, level) in [
        (BinaryKind::Color, 4, "freecad.color_list_color"),
        (BinaryKind::Float, 8, "freecad.float_list_value"),
        (BinaryKind::Vector, 24, "freecad.vector_list_vector"),
        (BinaryKind::Vector, 12, "freecad.vector_list_vector"),
        (
            BinaryKind::Placement,
            56,
            "freecad.placement_list_placement",
        ),
        (
            BinaryKind::Placement,
            28,
            "freecad.placement_list_placement",
        ),
        (BinaryKind::Fillet, 20, "freecad.fillet_edge"),
        (BinaryKind::Points, 12, "freecad.point_kernel_point"),
        (BinaryKind::F32 { components: 1 }, 4, "freecad.f32_record"),
        (BinaryKind::F32 { components: 3 }, 12, "freecad.f32_record"),
        (BinaryKind::F32 { components: 8 }, 32, "freecad.f32_record"),
    ] {
        let mut bytes = 2_u32.to_le_bytes().to_vec();
        bytes.resize(4 + width * 2, 0);
        with_member(&bytes, |ctx, source, parent, frames| {
            let before = frames.len();
            ctx.record("test:source:record#framer", source, || {
                frame_binary_member(ctx, source, kind, parent, frames)
                    .map_err(cadmpeg_core::CodecError::Malformed)
            })
            .unwrap();
            PartTable::load().unwrap().validate(ctx, frames).unwrap();
            assert_eq!(frames.len() - before, 3);
            assert_eq!(frames[before].framing(), [0..4]);
            for (index, row) in frames[before + 1..].iter().enumerate() {
                assert_eq!(row.level().as_str(), level);
                assert_eq!(row.capture().offset(), u64_from_index(4 + index * width));
                assert_eq!(row.capture().byte_len(), u64_from_index(width));
                assert!(row.framing().is_empty());
            }
            assert!(ctx.take_record_captures().unwrap()[0].reads().is_empty());
        });
    }
}

#[test]
fn material_v3_strings_and_mesh_arrays_keep_their_count_fields_as_framing() {
    let mut material = 1_u32.to_le_bytes().to_vec();
    material.resize(28, 0);
    for text in [b"a".as_slice(), b"bb".as_slice(), b"ccc".as_slice()] {
        material.extend_from_slice(&u32::try_from(text.len()).unwrap().to_le_bytes());
        material.extend_from_slice(text);
    }
    with_member(&material, |ctx, source, parent, frames| {
        let before = frames.len();
        frame_binary_member(
            ctx,
            source,
            BinaryKind::Material { version: 3 },
            parent,
            frames,
        )
        .unwrap();
        PartTable::load().unwrap().validate(ctx, frames).unwrap();
        assert_eq!(frames.len() - before, 5);
        assert!(frames[before + 1].framing().is_empty());
        assert_eq!(frames[before + 1].capture().offset(), 4);
        assert_eq!(frames[before + 1].capture().byte_len(), 24);
        for (frame, start) in frames[before + 2..].iter().zip([28, 33, 39]) {
            assert_eq!(frame.level().as_str(), "freecad.material_list_string");
            assert_eq!(frame.framing(), [start..start + 4]);
        }
    });
    let mut mesh = 0_u32.to_le_bytes().to_vec();
    for _ in 0..6 {
        mesh.extend_from_slice(&1_u32.to_le_bytes());
        mesh.extend_from_slice(&0_u32.to_le_bytes());
    }
    with_member(&mesh, |ctx, source, parent, frames| {
        let before = frames.len();
        frame_binary_member(ctx, source, BinaryKind::MeshMaterial, parent, frames).unwrap();
        PartTable::load().unwrap().validate(ctx, frames).unwrap();
        assert_eq!(frames.len() - before, 14);
        assert_eq!(
            frames[before + 1].level().as_str(),
            "freecad.mesh_material_binding"
        );
        for pair in frames[before + 2..].chunks_exact(2) {
            assert_eq!(pair[0].level().as_str(), "freecad.mesh_material_array");
            assert_eq!(
                pair[0].framing(),
                [pair[0].capture().offset()..pair[0].capture().offset() + 4]
            );
            assert_eq!(pair[1].level().as_str(), "freecad.mesh_material_value");
        }
    });
}

#[test]
fn cut_short_counts_preserve_only_independently_located_prefixes() {
    for bytes in [
        b"".as_slice(),
        b"\x02\0".as_slice(),
        b"\x02\0\0\0\0\0\0\0".as_slice(),
    ] {
        with_member(bytes, |ctx, source, parent, frames| {
            let before = frames.len();
            assert!(frame_binary_member(ctx, source, BinaryKind::Float, parent, frames).is_err());
            let partial_row = bytes.len() >= 4;
            assert_eq!(frames.len() - before, if partial_row { 2 } else { 1 });
            assert_eq!(
                frames[before].capture().byte_len(),
                u64_from_index(bytes.len())
            );
            if partial_row {
                let row = &frames[before + 1];
                assert_eq!(row.level().as_str(), "freecad.float_list_value");
                assert_eq!(row.capture().offset(), 4);
                assert_eq!(row.capture().byte_len(), 4);
                assert!(row.framing().is_empty());
                assert_eq!(row.parent(), FrameParent::Part(before));
            }
            PartTable::load().unwrap().validate(ctx, frames).unwrap();
        });
    }
}
