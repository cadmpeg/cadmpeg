// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::codec::EncodeInput;
use cadmpeg_ir::codec::TargetRequest;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, Encoder};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::PointId;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{Color, Point};
use cadmpeg_ir::units::Units;

use super::*;
use crate::{RhinoArchiveVersion, RhinoCodec};

#[test]
fn empty_utf16_string_has_zero_count_and_no_terminator() {
    assert_eq!(utf16(""), 0_u32.to_le_bytes());
    assert_eq!(utf16("A"), [2, 0, 0, 0, b'A', 0, 0, 0]);
}

#[test]
fn brep_trim_type_distinguishes_boundary_mated_and_seam_uses() {
    assert_eq!(brep_trim_type(1, false), 1);
    assert_eq!(brep_trim_type(2, false), 2);
    assert_eq!(brep_trim_type(2, true), 3);
}

#[test]
fn explicit_loop_role_overrides_face_list_order() {
    use cadmpeg_ir::topology::LoopBoundaryRole;

    assert_eq!(brep_loop_type(LoopBoundaryRole::Inner, true), 2);
    assert_eq!(brep_loop_type(LoopBoundaryRole::Outer, false), 1);
    assert_eq!(brep_loop_type(LoopBoundaryRole::Unspecified, true), 1);
}

#[test]
fn object_attribute_items_are_written_in_ascending_order() {
    let payload = object_attributes_payload(
        "body",
        None,
        Some(Color {
            r: 1.0,
            g: 0.5,
            b: 0.0,
            a: 1.0,
        }),
        Some(false),
    );
    assert_eq!(&payload[21..], &[6, 255, 128, 0, 0, 11, 0, 13, 1, 0]);
}

#[test]
fn nonempty_user_string_presentation_is_refused_before_output() {
    let mut source = CadIr::empty(Units::default());
    source.model.points.push(Point {
        id: PointId("cadir:model:point#user-strings".into()),
        position: Point3::new(1.0, 2.0, 3.0),
        source_object: None,
    });
    let mut bytes = Vec::new();
    RhinoCodec
        .plan(
            EncodeInput::new(&source, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("required invariant");
    let mut decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("required invariant");
    {
        let mut ir = decoded.ir_mut();
        let records = ir
            .native
            .namespace_mut("rhino")
            .arenas
            .get_mut("object_presentation")
            .expect("decoded object presentation");
        let original = records.first().expect("decoded object presentation record");
        let id = original.id().to_string();
        let mut fields = original.fields();
        fields.insert(
            "user_strings".into(),
            serde_json::json!([{ "key": "name", "value": "value" }]),
        );
        records[0] = cadmpeg_ir::NativeRecord::new(id, fields);
    }

    let mut output = vec![0xaa];
    let error = RhinoCodec
        .plan(
            EncodeInput::new(decoded.ir(), None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut output))
        .expect_err("user-string metadata must not be discarded");
    assert!(error.to_string().contains("survival handling"));
    assert_eq!(output, [0xaa]);
}

#[test]
fn nonempty_mesh_modifier_presentation_is_refused_before_output() {
    let mut source = CadIr::empty(Units::default());
    source.model.points.push(Point {
        id: PointId("cadir:model:point#mesh-modifiers".into()),
        position: Point3::new(1.0, 2.0, 3.0),
        source_object: None,
    });
    let mut bytes = Vec::new();
    RhinoCodec
        .plan(
            EncodeInput::new(&source, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("required invariant");
    let mut decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("required invariant");
    {
        let mut ir = decoded.ir_mut();
        let records = ir
            .native
            .namespace_mut("rhino")
            .arenas
            .get_mut("object_presentation")
            .expect("decoded object presentation");
        let original = records.first().expect("decoded object presentation record");
        let id = original.id().to_string();
        let mut fields = original.fields();
        fields.insert(
            "mesh_modifiers".into(),
            serde_json::json!({ "displacement": { "on": true } }),
        );
        records[0] = cadmpeg_ir::NativeRecord::new(id, fields);
    }

    let mut output = vec![0xaa];
    let error = RhinoCodec
        .plan(
            EncodeInput::new(decoded.ir(), None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut output))
        .expect_err("mesh modifier metadata must not be discarded");
    assert!(error.to_string().contains("survival handling"));
    assert_eq!(output, [0xaa]);
}

#[test]
fn nonempty_layer_per_viewport_settings_are_refused_before_output() {
    let mut source = CadIr::empty(Units::default());
    source.model.points.push(Point {
        id: PointId("cadir:model:point#layer-settings".into()),
        position: Point3::new(1.0, 2.0, 3.0),
        source_object: None,
    });
    let mut bytes = Vec::new();
    RhinoCodec
        .plan(
            EncodeInput::new(&source, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("required invariant");
    let mut decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("required invariant");
    {
        let mut ir = decoded.ir_mut();
        let records = ir
            .native
            .namespace_mut("rhino")
            .arenas
            .get_mut("layers")
            .expect("decoded layer presentation");
        let original = records.first().expect("decoded layer presentation record");
        let id = original.id().to_string();
        let mut fields = original.fields();
        fields.insert(
            "per_viewport_settings".into(),
            serde_json::json!([{
                "viewport_uuid": "01020304-0506-0708-090a-0b0c0d0e0f10",
                "settings_mask": 3,
                "color": [10, 20, 30, 40]
            }]),
        );
        records[0] = cadmpeg_ir::NativeRecord::new(id, fields);
    }

    let mut output = vec![0xaa];
    let error = RhinoCodec
        .plan(
            EncodeInput::new(decoded.ir(), None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut output))
        .expect_err("layer metadata must not be discarded");
    assert!(error.to_string().contains("survival handling"));
    assert_eq!(output, [0xaa]);
}
