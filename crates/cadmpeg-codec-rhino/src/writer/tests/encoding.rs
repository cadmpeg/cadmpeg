// SPDX-License-Identifier: Apache-2.0

use crate::writer::{
    archive_body_len, archive_final_size, brep_loop_type, brep_trim_type, native_i32_count,
    object_attributes_payload, utf16, wire_index,
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
fn admitted_native_counts_outside_i32_lanes_are_writer_limits() {
    let count = usize::try_from(i32::MAX).expect("i32 fits usize") + 1;
    let count_error = native_i32_count(count, "NURBS count exceeds native lane".into())
        .expect_err("the native lane is signed 32-bit");
    let index_error = wire_index(count).expect_err("the native index lane is signed 32-bit");
    assert!(matches!(
        count_error,
        cadmpeg_core::CodecError::NotImplemented(_)
    ));
    assert!(matches!(
        index_error,
        cadmpeg_core::CodecError::NotImplemented(_)
    ));
}

#[test]
fn admitted_archive_sizes_outside_native_lanes_are_writer_limits() {
    let body_error = archive_body_len(u64::MAX).expect_err("the body lane is signed 64-bit");
    let size_error =
        archive_final_size(u64::MAX).expect_err("the final size must include a footer");
    assert!(matches!(
        body_error,
        cadmpeg_core::CodecError::NotImplemented(_)
    ));
    assert!(matches!(
        size_error,
        cadmpeg_core::CodecError::NotImplemented(_)
    ));
}

#[test]
fn a_name_with_a_null_character_is_omitted_and_the_object_written() {
    let mut ir = CadIr::empty();
    let body_id: cadmpeg_ir::ids::BodyId =
        "cadir:model:body#named".try_into().expect("valid identity");
    let region_id: cadmpeg_ir::ids::RegionId =
        "cadir:model:region#named".try_into().expect("valid identity");
    let shell_id: cadmpeg_ir::ids::ShellId =
        "cadir:model:shell#named".try_into().expect("valid identity");
    let vertex_id =
        cadmpeg_ir::ids::VertexId::mint("cadir:model:vertex#named").expect("identity grammar");
    let point_id = PointId::mint("cadir:model:point#named").expect("identity grammar");
    ir.geometry.bodies.push(cadmpeg_ir::topology::Body {
        id: body_id.clone(),
        kind: cadmpeg_ir::topology::BodyKind::General,
        transform: None,
        name: Some("left\0right".into()),
        color: None,
        visible: None,
        native_ref: None,
    });
    ir.geometry.regions.push(cadmpeg_ir::topology::Region {
        id: region_id.clone(),
        body: body_id,
        shells: vec![shell_id.clone()],
        native_ref: None,
    });
    ir.geometry
        .shells
        .push(cadmpeg_ir::topology::Shell::with_members(
            shell_id,
            region_id,
            Vec::new(),
            vec![vertex_id.clone()],
        ));
    ir.geometry.vertices.push(cadmpeg_ir::topology::Vertex {
        id: vertex_id,
        point: point_id.clone(),
        tolerance: None,
        native_ref: None,
    });
    ir.geometry.points.push(Point::new(
        point_id,
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0))
            .expect("a finite position is a point"),
        None,
    ));
    let (bytes, losses) = super::write_reported(&ir, RhinoArchiveVersion::V8);
    super::loss_naming(
        &losses,
        crate::loss::RhinoLossCode::WriterObjectAttributesReduced,
        "cadir:model:body#named",
    );
    let decoded = super::decoded(bytes);
    assert_eq!(decoded.geometry.bodies.len(), 1);
    assert_eq!(decoded.geometry.bodies[0].name, None);
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
        "body",
        None,
        Some(Color::new(1.0, 0.5, 0.0, 1.0).expect("valid color")),
        Some(false),
    )
    .expect("valid object attributes");
    assert_eq!(&payload[21..], &[6, 255, 128, 0, 0, 11, 0, 13, 1, 0]);
}

#[test]
fn nonempty_user_string_presentation_is_omitted_by_id() {
    let mut source = CadIr::empty();
    source.geometry.points.push(Point::new(
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
    let id;
    {
        let mut ir = decoded.ir_mut();
        let records = ir
            .native
            .namespace_mut("rhino")
            .arenas_mut()
            .get_mut("object_presentation")
            .expect("decoded object presentation");
        let original = records.first().expect("decoded object presentation record");
        id = original.id().to_string();
        let mut fields = original.fields().unwrap().clone();
        fields.insert(
            "user_strings".into(),
            serde_json::json!([{ "key": "name", "value": "value" }]),
        );
        records[0] = cadmpeg_ir::NativeRecord::new(
            cadmpeg_ir::ids::Identity::new(id.clone()).expect("valid identity"),
            fields,
        )
        .expect("valid native identity");
    }

    let (bytes, losses) = super::write_reported(decoded.ir(), RhinoArchiveVersion::V8);
    let message = super::loss_naming(
        &losses,
        crate::loss::RhinoLossCode::NativeRecordsOmitted,
        &id,
    );
    assert!(message.contains("`object_presentation`"), "{message}");
    assert_eq!(super::decoded(bytes).geometry.points.len(), 1);
}

#[test]
fn nonempty_mesh_modifier_presentation_is_omitted_by_id() {
    let mut source = CadIr::empty();
    source.geometry.points.push(Point::new(
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
    let id;
    {
        let mut ir = decoded.ir_mut();
        let records = ir
            .native
            .namespace_mut("rhino")
            .arenas_mut()
            .get_mut("object_presentation")
            .expect("decoded object presentation");
        let original = records.first().expect("decoded object presentation record");
        id = original.id().to_string();
        let mut fields = original.fields().unwrap().clone();
        fields.insert(
            "mesh_modifiers".into(),
            serde_json::json!({ "displacement": { "on": true } }),
        );
        records[0] = cadmpeg_ir::NativeRecord::new(
            cadmpeg_ir::ids::Identity::new(id.clone()).expect("valid identity"),
            fields,
        )
        .expect("valid native identity");
    }

    let (bytes, losses) = super::write_reported(decoded.ir(), RhinoArchiveVersion::V8);
    let message = super::loss_naming(
        &losses,
        crate::loss::RhinoLossCode::NativeRecordsOmitted,
        &id,
    );
    assert!(message.contains("`object_presentation`"), "{message}");
    assert_eq!(super::decoded(bytes).geometry.points.len(), 1);
}

#[test]
fn nonempty_layer_per_viewport_settings_are_omitted_by_id() {
    let mut source = CadIr::empty();
    source.geometry.points.push(Point::new(
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
    let id;
    {
        let mut ir = decoded.ir_mut();
        let records = ir
            .native
            .namespace_mut("rhino")
            .arenas_mut()
            .get_mut("layers")
            .expect("decoded layer presentation");
        let original = records.first().expect("decoded layer presentation record");
        id = original.id().to_string();
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
            cadmpeg_ir::ids::Identity::new(id.clone()).expect("valid identity"),
            fields,
        )
        .expect("valid native identity");
    }

    let (bytes, losses) = super::write_reported(decoded.ir(), RhinoArchiveVersion::V8);
    let message = super::loss_naming(
        &losses,
        crate::loss::RhinoLossCode::NativeRecordsOmitted,
        &id,
    );
    assert!(message.contains("`layers`"), "{message}");
    assert_eq!(super::decoded(bytes).geometry.points.len(), 1);
}
