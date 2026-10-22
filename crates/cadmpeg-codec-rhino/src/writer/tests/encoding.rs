// SPDX-License-Identifier: Apache-2.0

use crate::writer::{
    archive_body_len, archive_final_size, brep_loop_type, brep_trim_type, native_i32_count, utf16,
    wire_index,
};
use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::EncodeInput;
use std::io::Cursor;

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::PointId;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{Color, Point};

use crate::{RhinoArchiveVersion, RhinoCodec};

#[test]
fn admitted_angular_tolerance_above_pi_is_a_writer_limit() {
    let mut ir = CadIr::empty();
    ir.tolerances = cadmpeg_ir::units::Tolerances::new(1.0e-6, 4.0)
        .expect("positive finite tolerances are admitted");
    let mut output = Vec::new();
    let error = RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut output))
        .expect_err("Rhino cannot state this angular tolerance");
    assert!(matches!(error, cadmpeg_core::CodecError::InvalidInput(_)));
}

#[test]
fn admitted_native_counts_outside_i32_lanes_are_writer_limits() {
    let count = usize::try_from(i32::MAX).expect("i32 fits usize") + 1;
    let count_error = native_i32_count(count, "NURBS count exceeds native lane".into())
        .expect_err("the native lane is signed 32-bit");
    let index_error = wire_index(count).expect_err("the native index lane is signed 32-bit");
    assert!(matches!(
        count_error,
        cadmpeg_core::CodecError::InvalidInput(_)
    ));
    assert!(matches!(
        index_error,
        cadmpeg_core::CodecError::InvalidInput(_)
    ));
}

#[test]
fn admitted_archive_sizes_outside_native_lanes_are_writer_limits() {
    let body_error = archive_body_len(u64::MAX).expect_err("the body lane is signed 64-bit");
    let size_error =
        archive_final_size(u64::MAX).expect_err("the final size must include a footer");
    assert!(matches!(
        body_error,
        cadmpeg_core::CodecError::InvalidInput(_)
    ));
    assert!(matches!(
        size_error,
        cadmpeg_core::CodecError::InvalidInput(_)
    ));
}

#[test]
fn null_containing_name_omits_only_the_name_attribute() {
    let identity = "cadir:model:body#named";
    assert_eq!(
        object_attributes_payload(&CadIr::empty(), identity, Some("left\0right"), None, None)
            .expect("name reduction"),
        object_attributes_payload(&CadIr::empty(), identity, None, None, None)
            .expect("absent name"),
    );
}

#[test]
fn empty_utf16_string_has_zero_count_and_no_terminator() {
    assert_eq!(
        utf16("").expect("empty string count fits"),
        0_u32.to_le_bytes()
    );
    assert_eq!(
        utf16("A").expect("string count fits"),
        [2, 0, 0, 0, b'A', 0, 0, 0]
    );
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
        &CadIr::empty(),
        "body",
        None,
        Some(Color::new(1.0, 0.5, 0.0, 1.0).expect("valid color")),
        Some(false),
    )
    .expect("valid object attributes");
    assert_eq!(&payload[21..], &[6, 255, 128, 0, 0, 11, 0, 13, 1, 0]);
}

#[test]
fn nonempty_user_string_presentation_is_reported_and_geometry_written() {
    let mut source = CadIr::empty();
    source.model.points.push(Point::new(
        PointId::mint("cadir:model:point#user-strings").expect("identity grammar"),
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0))
            .expect("a finite position is a point"),
        None,
    ));
    let mut bytes = Vec::new();
    RhinoCodec
        .plan(
            EncodeInput::new(&source, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("required invariant");
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("required invariant");
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir = decoded.ir_mut();
        let records = ir
            .native
            .namespace_mut("rhino")
            .arenas_mut()
            .get_mut("object_presentation")
            .expect("decoded object presentation");
        let original = records.first().expect("decoded object presentation record");
        let id = original.id().to_string();
        let mut fields = original.fields().unwrap().clone();
        fields.insert(
            "user_strings".into(),
            serde_json::json!([{ "key": "name", "value": "value" }]),
        );
        records[0] = cadmpeg_ir::NativeRecord::new(
            cadmpeg_ir::ids::Identity::new(id).expect("valid identity"),
            fields,
        )
        .expect("valid native identity");
    }

    let plan = RhinoCodec
        .plan(
            EncodeInput::new(decoded.ir(), None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .expect("unsupported native presentation is reported");
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "native.records-omitted"));
    let mut output = Vec::new();
    plan.write_to(&mut output).expect("archive writes");
    let written = RhinoCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("written archive decodes");
    assert_eq!(written.ir().model.points.len(), 1);
}

#[test]
fn nonempty_mesh_modifier_presentation_is_reported_and_geometry_written() {
    let mut source = CadIr::empty();
    source.model.points.push(Point::new(
        PointId::mint("cadir:model:point#mesh-modifiers").expect("identity grammar"),
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0))
            .expect("a finite position is a point"),
        None,
    ));
    let mut bytes = Vec::new();
    RhinoCodec
        .plan(
            EncodeInput::new(&source, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("required invariant");
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("required invariant");
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir = decoded.ir_mut();
        let records = ir
            .native
            .namespace_mut("rhino")
            .arenas_mut()
            .get_mut("object_presentation")
            .expect("decoded object presentation");
        let original = records.first().expect("decoded object presentation record");
        let id = original.id().to_string();
        let mut fields = original.fields().unwrap().clone();
        fields.insert(
            "mesh_modifiers".into(),
            serde_json::json!({ "displacement": { "on": true } }),
        );
        records[0] = cadmpeg_ir::NativeRecord::new(
            cadmpeg_ir::ids::Identity::new(id).expect("valid identity"),
            fields,
        )
        .expect("valid native identity");
    }

    let plan = RhinoCodec
        .plan(
            EncodeInput::new(decoded.ir(), None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .expect("unsupported native presentation is reported");
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "native.records-omitted"));
    let mut output = Vec::new();
    plan.write_to(&mut output).expect("archive writes");
    let written = RhinoCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("written archive decodes");
    assert_eq!(written.ir().model.points.len(), 1);
}

#[test]
fn nonempty_layer_per_viewport_settings_are_reported_and_geometry_written() {
    let mut source = CadIr::empty();
    source.model.points.push(Point::new(
        PointId::mint("cadir:model:point#layer-settings").expect("identity grammar"),
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0))
            .expect("a finite position is a point"),
        None,
    ));
    let mut bytes = Vec::new();
    RhinoCodec
        .plan(
            EncodeInput::new(&source, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("required invariant");
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("required invariant");
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir = decoded.ir_mut();
        let records = ir
            .native
            .namespace_mut("rhino")
            .arenas_mut()
            .get_mut("layers")
            .expect("decoded layer presentation");
        let original = records.first().expect("decoded layer presentation record");
        let id = original.id().to_string();
        let mut fields = original.fields().unwrap().clone();
        fields.insert(
            "per_viewport_settings".into(),
            serde_json::json!([{
                "viewport_uuid": "01020304-0506-0708-090a-0b0c0d0e0f10",
                "settings_mask": 3,
                "color": "0a141e28"
            }]),
        );
        records[0] = cadmpeg_ir::NativeRecord::new(
            cadmpeg_ir::ids::Identity::new(id).expect("valid identity"),
            fields,
        )
        .expect("valid native identity");
    }

    let plan = RhinoCodec
        .plan(
            EncodeInput::new(decoded.ir(), None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .expect("unsupported native presentation is reported");
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "native.records-omitted"));
    let mut output = Vec::new();
    plan.write_to(&mut output).expect("archive writes");
    let written = RhinoCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("written archive decodes");
    assert_eq!(written.ir().model.points.len(), 1);
}

fn object_attributes_payload(
    ir: &CadIr,
    identity: &str,
    name: Option<&str>,
    color: Option<Color>,
    visible: Option<bool>,
) -> Result<Vec<u8>, cadmpeg_core::CodecError> {
    let materials = crate::writer::tables::materials::MaterialPlan::new(ir)?;
    crate::writer::object_attributes_payload(ir, identity, name, color, visible, &materials)
}
