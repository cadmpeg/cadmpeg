// SPDX-License-Identifier: Apache-2.0
//! Current kernel geometry survives source-less synthesis and deletion.

use crate::test_support::test_archive::archive_entries;
use crate::FcstdCodec;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

fn source() -> Vec<u8> {
    source_with_mesh_cut(None)
}

fn source_with_mesh_cut(cut: Option<usize>) -> Vec<u8> {
    source_with_cuts(cut, None)
}

fn source_with_cuts(cut: Option<usize>, cloud_cut: Option<usize>) -> Vec<u8> {
    let document = "<Document SchemaVersion='4' FileVersion='1'><Objects Count='2'><Object type='Mesh::Feature' name='Mesh' id='1'/><Object type='Points::Feature' name='Cloud' id='2'/></Objects><ObjectData Count='2'><Object name='Mesh'><Properties Count='1'><Property name='Mesh' type='Mesh::PropertyMeshKernel'><Mesh file='MeshKernel.bms'/></Property></Properties></Object><Object name='Cloud'><Properties Count='1'><Property name='Points' type='Points::PropertyPointKernel'><Points file='Cloud' mtrx='1 0 0 10 0 1 0 20 0 0 1 30 0 0 0 1'/></Property></Properties></Object></ObjectData></Document>";
    let mut mesh = Vec::new();
    mesh.extend_from_slice(&0xa0b0_c0d0_u32.to_le_bytes());
    mesh.extend_from_slice(&0x0001_0000_u32.to_le_bytes());
    mesh.resize(crate::layout::mesh_kernel_side_entry_header::LEN, 0);
    mesh.extend_from_slice(&3_u32.to_le_bytes());
    mesh.extend_from_slice(&1_u32.to_le_bytes());
    for value in [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        mesh.extend_from_slice(&value.to_le_bytes());
    }
    for value in [0_u32, 1, 2, u32::MAX, u32::MAX, u32::MAX] {
        mesh.extend_from_slice(&value.to_le_bytes());
    }
    for value in [0.0_f32, 1.0, 0.0, 1.0, 0.0, 0.0] {
        mesh.extend_from_slice(&value.to_le_bytes());
    }
    if let Some(cut) = cut {
        mesh.truncate(cut);
    }
    let mut cloud = 2_u32.to_le_bytes().to_vec();
    for value in [1.0_f32, 2.0, 3.0, -1.0, -2.0, -3.0] {
        cloud.extend_from_slice(&value.to_le_bytes());
    }
    if let Some(cut) = cloud_cut {
        cloud.truncate(cut);
    }
    archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("MeshKernel.bms", &mesh),
        ("Cloud", &cloud),
    ])
}

fn synthesize(ir: &cadmpeg_ir::CadIr) -> cadmpeg_ir::codec::DecodeResult {
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(ir, None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    crate::test_support::test_archive::assert_valid_document(result.ir());
    assert!(crate::test_support::validate_native(result.ir()).is_empty());
    result
}

#[test]
fn source_less_kernel_synthesis_uses_current_geometry_and_does_not_apply_point_transform_twice() {
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(source()), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.tessellations.len(), 1);
    assert_eq!(decoded.ir().model.points.len(), 2);
    assert!(decoded.ir().model.tessellations[0].native_ref.is_some());
    assert!(decoded
        .ir()
        .model
        .points
        .iter()
        .all(|point| point.native_ref.is_some()));
    let mut ir = decoded.ir().clone();
    ir.model.points[0].set_position(
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(42.0, 22.0, 33.0))
            .unwrap(),
    );
    ir.model.tessellations[0]
        .edit_vertices(|point| {
            point.x += 5.0;
            Ok(())
        })
        .unwrap();
    let output = synthesize(&ir);
    assert_eq!(output.ir().model.points.len(), 2);
    assert_eq!(
        output.ir().model.points[0].position(),
        ir.model.points[0].position()
    );
    assert_eq!(
        output.ir().model.points[1].position(),
        ir.model.points[1].position()
    );
    assert_eq!(output.ir().model.tessellations.len(), 1);
    assert_eq!(
        output.ir().model.tessellations[0].vertices(),
        ir.model.tessellations[0].vertices()
    );
    assert_eq!(
        output.ir().model.tessellations[0].triangles(),
        ir.model.tessellations[0].triangles()
    );
    ir.model.points.clear();
    ir.model.tessellations.clear();
    let output = synthesize(&ir);
    assert!(output.ir().model.points.is_empty());
    assert!(output.ir().model.tessellations.is_empty());
}

#[test]
fn cut_short_mesh_header_gives_up_its_own_part_and_keeps_the_point_peer() {
    use cadmpeg_ir::source_fidelity::parts::PartState;
    for cut in [0, 4, 16, 263] {
        let decoded = FcstdCodec
            .decode(
                &mut Cursor::new(source_with_mesh_cut(Some(cut))),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert!(decoded.ir().model.tessellations.is_empty());
        assert_eq!(decoded.ir().model.points.len(), 2);
        let failures = decoded
            .source_fidelity()
            .parts()
            .values()
            .filter(|part| part.state() == &PartState::GivenUp {})
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "cut {cut}");
        assert_eq!(failures[0].level().as_str(), "freecad.mesh_kernel_header");
        assert_eq!(failures[0].byte_len(), u64::try_from(cut).unwrap());
        assert!(decoded
            .source_fidelity()
            .parts()
            .values()
            .any(|part| part.level().as_str() == "freecad.mesh_kernel_entry"
                && part.state() == &PartState::Decoded {}));
        crate::fidelity::b1::assert_classified_spans(&decoded);
    }
}

#[test]
fn independent_kernel_framing_matches_every_reached_binary_part() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_test_support::framing::{
        freecad::binary::{frame_binary_member, BinaryKind},
        zip, PartTable,
    };
    let original = source();
    for bytes in crate::test_support::framing::zip_forms(&original) {
        for depth in [
            cadmpeg_core::decode::DecodeDepth::Container,
            cadmpeg_core::decode::DecodeDepth::Geometry,
            cadmpeg_core::decode::DecodeDepth::Full,
        ] {
            let decoded = FcstdCodec
                .decode(
                    &mut Cursor::new(&bytes),
                    &DecodeOptions {
                        depth,
                        ..DecodeOptions::default()
                    },
                )
                .unwrap();
            assert!(
                crate::test_support::validate_native(decoded.ir()).is_empty(),
                "{depth:?}: {:?}",
                crate::test_support::validate_native(decoded.ir())
            );
            let arena = DecodeArena::new();
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
            let mut frames = Vec::new();
            let entries = zip::frame(&ctx, root, &mut frames).unwrap();
            let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
            for (name, kind) in [
                ("MeshKernel.bms", BinaryKind::Mesh),
                ("Cloud", BinaryKind::Points),
            ] {
                let entry = entries
                    .iter()
                    .find(|entry| entry.name().window() == name.as_bytes())
                    .unwrap();
                let member = archive.open(&ctx, name).unwrap();
                let start = frames.len();
                frame_binary_member(&ctx, member, kind, entry.frame_index(), &mut frames).unwrap();
                for frame in &frames[start..] {
                    let part = decoded
                        .source_fidelity()
                        .parts()
                        .values()
                        .find(|part| {
                            part.level() == frame.level()
                                && part.stream() == frame.capture().stream()
                                && part.offset() == frame.capture().offset()
                                && part.byte_len() == frame.capture().byte_len()
                        })
                        .unwrap_or_else(|| {
                            panic!(
                                "decoder omitted independent frame {} {}+{}",
                                frame.level().as_str(),
                                frame.capture().offset(),
                                frame.capture().byte_len()
                            )
                        });
                    assert_eq!(
                        part.framing_bytes(),
                        frame.framing(),
                        "{}",
                        frame.level().as_str()
                    );
                }
                let actual = decoded
                    .source_fidelity()
                    .parts()
                    .values()
                    .filter(|part| {
                        part.stream() == frames[start].capture().stream()
                            && part.level().as_str().starts_with("freecad.")
                    })
                    .count();
                assert_eq!(
                    actual,
                    frames.len() - start,
                    "no decoder-only binary frames for {name}"
                );
            }
            PartTable::load().unwrap().validate(&ctx, &frames).unwrap();
        }
    }
}

mod gui;

mod cuts;

mod reads;
