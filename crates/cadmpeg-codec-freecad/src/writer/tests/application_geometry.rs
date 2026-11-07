// SPDX-License-Identifier: Apache-2.0
//! Application-owned geometry payloads.

use crate::FcstdCodec;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn secondary_empty_shape_survives_source_free_synthesis() {
    let xml = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="App::FeaturePython" name="Owner"/></Objects><ObjectData Count="1"><Object name="Owner"><Properties Count="1"><Property name="BackupShape" type="Part::PropertyPartShape"><Part file="Backup.brp"/></Property></Properties></Object></ObjectData></Document>"#;
    let archive = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", xml),
        ("Backup.brp", b""),
    ]);
    let input = FcstdCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    let plan = FcstdCodec
        .plan(EncodeInput::new(input.ir(), None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let namespace = output.ir().native.namespace("fcstd").unwrap();
    let properties = namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    assert!(properties
        .iter()
        .any(|property| property.name == "BackupShape"));
    let payloads = namespace
        .arena_as::<crate::brep::ShapePayloadRecord>("shape_payloads")
        .unwrap();
    assert!(payloads
        .iter()
        .any(|payload| matches!(payload.payload, crate::brep::ShapePayload::Empty)));
}

#[test]
fn edited_point_cloud_writes_current_transformed_positions() {
    let xml = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Points::Feature" name="Cloud"/></Objects><ObjectData Count="1"><Object name="Cloud"><Properties Count="1"><Property name="Points" type="Points::PropertyPointKernel"><Points file="Cloud.points" mtrx="1 0 0 10 0 1 0 20 0 0 1 30 0 0 0 1"/></Property></Properties></Object></ObjectData></Document>"#;
    let mut payload = 2_u32.to_le_bytes().to_vec();
    for value in [1.0_f32, 2.0, 3.0, -1.0, -2.0, -3.0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    let archive = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", xml),
        ("Cloud.points", &payload),
    ]);
    let input = FcstdCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    assert_eq!(input.ir().model.points.len(), 2);
    let mut ir = input.ir().clone();
    let point = ir
        .model
        .points
        .iter_mut()
        .find(|point| point.position().get().x == 11.0)
        .unwrap();
    point.set_position(
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(12.0, 22.0, 33.0))
            .unwrap(),
    );
    let plan = FcstdCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(output.ir().model.points.len(), 2);
    assert!(output
        .ir()
        .model
        .points
        .iter()
        .any(|point| point.position().get() == cadmpeg_ir::math::Point3::new(12.0, 22.0, 33.0)));
    assert!(output
        .ir()
        .model
        .points
        .iter()
        .any(|point| point.position().get() == cadmpeg_ir::math::Point3::new(9.0, 18.0, 27.0)));
}

#[test]
fn point_cloud_keeps_non_binary32_coordinates_as_exact_geometry() {
    let xml = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Points::Feature" name="Cloud"/></Objects><ObjectData Count="1"><Object name="Cloud"><Properties Count="1"><Property name="Points" type="Points::PropertyPointKernel"><Points file="Cloud.points"/></Property></Properties></Object></ObjectData></Document>"#;
    let mut payload = 2_u32.to_le_bytes().to_vec();
    for value in [1.0_f32, 2.0, 3.0, -1.0, -2.0, -3.0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    let archive = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", xml),
        ("Cloud.points", &payload),
    ]);
    let input = FcstdCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    let mut ir = input.ir().clone();
    let point = ir
        .model
        .points
        .iter_mut()
        .find(|point| point.position().get().x == 1.0)
        .unwrap();
    point.set_position(
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(1.1, 2.0, 3.0))
            .unwrap(),
    );
    let id = point.id.to_string();
    let plan = FcstdCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(&id)));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(output.ir().model.points.len(), 2);
    assert!(output
        .ir()
        .model
        .points
        .iter()
        .any(|point| point.position().get() == cadmpeg_ir::math::Point3::new(1.1, 2.0, 3.0)));
    assert!(output
        .ir()
        .model
        .points
        .iter()
        .any(|point| point.position().get() == cadmpeg_ir::math::Point3::new(-1.0, -2.0, -3.0)));
}

#[test]
fn deleting_cloud_points_does_not_replay_the_old_payload() {
    let xml = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Points::Feature" name="Cloud"/></Objects><ObjectData Count="1"><Object name="Cloud"><Properties Count="1"><Property name="Points" type="Points::PropertyPointKernel"><Points file="Cloud.points"/></Property></Properties></Object></ObjectData></Document>"#;
    let mut payload = 1_u32.to_le_bytes().to_vec();
    for value in [1.0_f32, 2.0, 3.0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    let archive = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", xml),
        ("Cloud.points", &payload),
    ]);
    let input = FcstdCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    assert_eq!(input.ir().model.points.len(), 1);
    let mut ir = input.ir().clone();
    ir.model.points.clear();
    let plan = FcstdCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(output.ir().model.points.is_empty());
}

#[test]
fn point_cloud_keeps_source_order_past_single_digit_indices() {
    let xml = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Points::Feature" name="Cloud"/></Objects><ObjectData Count="1"><Object name="Cloud"><Properties Count="1"><Property name="Points" type="Points::PropertyPointKernel"><Points file="Cloud.points"/></Property></Properties></Object></ObjectData></Document>"#;
    let mut payload = 12_u32.to_le_bytes().to_vec();
    for index in 0_u8..12 {
        for value in [f32::from(index), 0.0, 0.0] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
    }
    let archive = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", xml),
        ("Cloud.points", &payload),
    ]);
    let input = FcstdCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    let mut reordered = input.ir().clone();
    reordered
        .model
        .points
        .sort_by(|left, right| left.id.cmp(&right.id));
    let plan = FcstdCodec
        .plan(EncodeInput::new(&reordered, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    for point in &input.ir().model.points {
        let restored = output
            .ir()
            .model
            .points
            .iter()
            .find(|p| p.id == point.id)
            .unwrap();
        assert_eq!(restored.position(), point.position());
    }
}

#[test]
fn multiple_point_kernels_preserve_each_propertys_points() {
    let xml = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Points::Feature" name="Cloud"/></Objects><ObjectData Count="1"><Object name="Cloud"><Properties Count="2"><Property name="First" type="Points::PropertyPointKernel"><Points file="First.points"/></Property><Property name="Second" type="Points::PropertyPointKernel"><Points file="Second.points"/></Property></Properties></Object></ObjectData></Document>"#;
    let payload = |x: f32| {
        let mut bytes = 1_u32.to_le_bytes().to_vec();
        for value in [x, 0.0, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    };
    let first = payload(1.0);
    let second = payload(2.0);
    let archive = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", xml),
        ("First.points", &first),
        ("Second.points", &second),
    ]);
    let input = FcstdCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    assert_eq!(input.ir().model.points.len(), 2);
    let plan = FcstdCodec
        .plan(EncodeInput::new(input.ir(), None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(output.ir().model.points.len(), 2);
    for point in &input.ir().model.points {
        let restored = output
            .ir()
            .model
            .points
            .iter()
            .find(|p| p.id == point.id)
            .unwrap();
        assert_eq!(restored.position(), point.position());
    }
}

#[test]
fn edited_application_mesh_replaces_the_kernel_payload() {
    let xml = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Mesh::Feature" name="Mesh"/></Objects><ObjectData Count="1"><Object name="Mesh"><Properties Count="2"><Property name="Mesh" type="Mesh::PropertyMeshKernel"><Mesh file="Mesh.bms"/></Property><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="10" Py="20" Pz="30" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object></ObjectData></Document>"#;
    let mut payload = 0xa0b0_c0d0_u32.to_le_bytes().to_vec();
    payload.extend_from_slice(&0x0001_0000_u32.to_le_bytes());
    payload.resize(264, 0);
    payload[8] = 37;
    payload.extend_from_slice(&3_u32.to_le_bytes());
    payload.extend_from_slice(&1_u32.to_le_bytes());
    for value in [0.0_f32, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 3.0, 0.0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    for value in [0_u32, 1, 2, u32::MAX, u32::MAX, u32::MAX] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    for value in [0.0_f32, 2.0, 0.0, 3.0, 0.0, 0.0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    let archive = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", xml),
        ("Mesh.bms", &payload),
    ]);
    let input = FcstdCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    let mut ir = input.ir().clone();
    assert_eq!(ir.model.tessellations.len(), 1);
    ir.model.tessellations[0]
        .edit_vertices(|point| {
            point.x += 2.0;
            Ok(())
        })
        .unwrap();
    let mesh = ir.model.tessellations[0].clone();
    let mut expected = mesh.vertices();
    expected.push(
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(14.0, 23.0, 30.0))
            .unwrap(),
    );
    let topology = cadmpeg_ir::tessellation::TessellationMesh::from_checked_list_lanes(
        expected.clone(),
        vec![[0, 1, 2], [1, 3, 2]],
        None,
    )
    .unwrap();
    ir.model.tessellations[0] = mesh.with_mesh(topology, Vec::new()).unwrap();
    let plan = FcstdCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(output.ir().model.tessellations.len(), 1);
    assert_eq!(output.ir().model.tessellations[0].vertices(), expected);
    assert_eq!(
        output.ir().model.tessellations[0].triangles(),
        [[0, 1, 2], [1, 3, 2]]
    );
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut encoded = Vec::new();
    std::io::Read::read_to_end(&mut archive.by_name("Mesh.bms").unwrap(), &mut encoded).unwrap();
    assert_eq!(encoded[8], 37);
    let word = |offset: usize| u32::from_le_bytes(encoded[offset..offset + 4].try_into().unwrap());
    let facets = 264 + 8 + 4 * 12;
    assert_eq!(
        [word(facets + 12), word(facets + 16), word(facets + 20)],
        [u32::MAX, 1, u32::MAX]
    );
    assert_eq!(
        [word(facets + 36), word(facets + 40), word(facets + 44)],
        [u32::MAX, u32::MAX, 0]
    );
    let bounds = facets + 48;
    let value = |offset: usize| f32::from_bits(word(offset));
    assert_eq!(
        (0..6)
            .map(|index| value(bounds + index * 4))
            .collect::<Vec<_>>(),
        [2.0, 4.0, 0.0, 3.0, 0.0, 0.0]
    );

    let mut precise = ir.clone();
    precise.model.tessellations[0]
        .edit_vertices(|point| {
            if point.x == 12.0 {
                point.x = 12.1;
            }
            Ok(())
        })
        .unwrap();
    let precise_vertices = precise.model.tessellations[0].vertices();
    let plan = FcstdCodec
        .plan(EncodeInput::new(&precise, None), TargetRequest::Inherit)
        .unwrap();
    let mut exact = Vec::new();
    plan.write_to(&mut exact).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(exact), &DecodeOptions::default())
        .unwrap();
    assert_eq!(output.ir().model.tessellations.len(), 1);
    assert_eq!(
        output.ir().model.tessellations[0].vertices(),
        precise_vertices
    );

    ir.model.tessellations.clear();
    let plan = FcstdCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut deleted = Vec::new();
    plan.write_to(&mut deleted).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(deleted), &DecodeOptions::default())
        .unwrap();
    assert!(output.ir().model.tessellations.is_empty());
}
