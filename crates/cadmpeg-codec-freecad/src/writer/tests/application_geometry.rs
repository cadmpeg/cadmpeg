// SPDX-License-Identifier: Apache-2.0
//! Application-owned geometry payloads.

use crate::FcstdCodec;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

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
