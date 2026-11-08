// SPDX-License-Identifier: Apache-2.0
//! Product structure the 3DM write carries as instance definitions.
//!
//! #235 writes neutral products and occurrences: definitions become instance
//! definitions and root occurrences become instance references, so neither
//! arena is an export omission.

use std::io::Cursor;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FiniteVector3;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::products::{
    ExternalDocument, Occurrence, OccurrenceParent, ProductDefinition, ProductDefinitionKind,
    PrototypeReference,
};
use cadmpeg_ir::transform::Transform;

use crate::{RhinoArchiveVersion, RhinoCodec};

const UNREPRESENTABLE_FACE_TOLERANCE: f64 = 1.0e-12;
const SINGULAR_TRIM_TOLERANCE: f64 = 1.0e-12;

#[test]
fn unsupported_appearance_metadata_names_the_appearance() {
    let source = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/fixtures/generated_point.3dm"
    ));
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode point");
    let mut ir = decoded.ir().clone();
    let appearance = &mut ir.model.appearances[0];
    appearance.schema = Some("GenericSchema".to_owned());
    appearance.library_id = Some("library".to_owned());
    appearance.properties.insert(
        cadmpeg_core::text::NonBlankString::new("reflectivity_at_0deg").expect("property"),
        cadmpeg_ir::scalar::FiniteReal::new(0.25).expect("finite value"),
    );
    appearance.properties.insert(
        cadmpeg_core::text::NonBlankString::new("refraction_index").expect("property"),
        cadmpeg_ir::scalar::FiniteReal::new(1.5).expect("finite value"),
    );
    let id = appearance.id.to_string();
    let plan = RhinoCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("plan point");
    assert!(plan.report().losses.iter().any(|loss| {
        loss.code.to_string() == "rhino/writer.appearance-metadata-omitted"
            && loss.message.contains(&id)
            && loss.message.contains("schema")
            && loss.message.contains("library_id")
            && loss.message.contains("reflectivity_at_0deg")
    }));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write point");
    let written = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("read point");
    assert_eq!(written.ir().model.points.len(), 1);
    let materials = &written.ir().native.namespace("rhino").unwrap().arenas()["materials"];
    assert!(materials.iter().any(|material| material
        .field("index_of_refraction")
        .and_then(|value| value.as_f64())
        .is_some_and(|value| (value - 1.5).abs() < 1e-12)));
}

#[test]
fn complete_face_colors_reach_supported_archives() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube");
    for (index, face) in ir.model.faces.iter_mut().enumerate() {
        face.name = None;
        face.color = Some(if index % 2 == 0 {
            cadmpeg_ir::topology::Color::new(1.0, 0.0, 0.0, 1.0).expect("red")
        } else {
            cadmpeg_ir::topology::Color::new(0.0, 1.0, 0.0, 1.0).expect("green")
        });
    }
    for version in [
        RhinoArchiveVersion::V5,
        RhinoArchiveVersion::V6,
        RhinoArchiveVersion::V7,
        RhinoArchiveVersion::V8,
    ] {
        let mut bytes = Vec::new();
        let report = RhinoCodec
            .plan(
                EncodeInput::new(&ir, None),
                TargetRequest::Explicit(version.descriptor().id.as_str()),
            )
            .and_then(|plan| plan.write_to(&mut bytes))
            .expect("write colored faces");
        let written = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("read colored faces");
        for (before, after) in ir.model.faces.iter().zip(&written.ir().model.faces) {
            if matches!(version, RhinoArchiveVersion::V7 | RhinoArchiveVersion::V8) {
                assert_eq!(after.color, before.color);
                assert!(!report
                    .losses
                    .iter()
                    .any(|loss| loss.message.contains("face-level name or color")
                        && loss.message.contains(before.id.as_str())));
            } else {
                assert_eq!(after.color, None);
                assert!(report
                    .losses
                    .iter()
                    .any(|loss| loss.message.contains("face-level name or color")
                        && loss.message.contains(before.id.as_str())));
            }
        }
        assert_eq!(written.ir().model.faces.len(), ir.model.faces.len());
    }
}

#[test]
fn face_appearance_bindings_supply_native_face_colors() {
    let source = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/fixtures/generated_point.3dm"
    ));
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode appearance");
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube");
    let color = cadmpeg_ir::topology::Color::new(1.0, 0.0, 0.0, 1.0).expect("red");
    let mut appearance = decoded.ir().model.appearances[0].clone();
    appearance.id = "test:model:appearance#red"
        .try_into()
        .expect("appearance identity");
    appearance.native_ref = None;
    appearance.base_color = Some(color);
    for (index, face) in ir.model.faces.iter_mut().enumerate() {
        face.color = None;
        let mut binding = decoded.ir().model.appearance_bindings[0].clone();
        binding.id = format!("test:model:appearance-binding#{index}")
            .try_into()
            .expect("binding identity");
        binding.native_ref = None;
        binding.appearance = appearance.id.clone();
        binding.target = cadmpeg_ir::appearance::AppearanceTarget::Face(face.id.clone());
        ir.model.appearance_bindings.push(binding);
    }
    ir.model.appearances.push(appearance);
    let mut bytes = Vec::new();
    let report = RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("write face appearances");
    for binding in &ir.model.appearance_bindings {
        assert!(report
            .losses
            .iter()
            .any(|loss| loss.message.contains(binding.id.as_str())));
    }
    let written = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("read face appearances");
    assert_eq!(written.ir().model.faces.len(), ir.model.faces.len());
    assert!(written
        .ir()
        .model
        .faces
        .iter()
        .all(|face| face.color == Some(color)));

    let blue = cadmpeg_ir::topology::Color::new(0.0, 0.0, 1.0, 1.0).expect("blue");
    ir.model.faces[0].color = Some(blue);
    let mut bytes = Vec::new();
    let report = RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("write face color override");
    assert!(report.losses.iter().any(|loss| loss
        .message
        .contains(ir.model.appearance_bindings[0].id.as_str())
        && loss.message.contains("is omitted")));
    let written = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("read face color override");
    assert_eq!(written.ir().model.faces[0].color, Some(blue));
}

#[test]
fn mixed_face_colors_use_the_native_unset_sentinel() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube");
    for face in &mut ir.model.faces {
        face.name = None;
        face.color = None;
    }
    ir.model.faces[0].color =
        Some(cadmpeg_ir::topology::Color::new(1.0, 0.0, 0.0, 1.0).expect("red"));
    let id = ir.model.faces[0].id.to_string();
    let faces = ir.model.faces.iter().collect::<Vec<_>>();
    let colors = super::super::encoded_face_colors(&faces)
        .expect("encode face colors")
        .expect("face color array");
    assert_eq!(colors[0], [255, 0, 0, 0]);
    assert!(colors[1..].iter().all(|color| *color == [255; 4]));
    let mut bytes = Vec::new();
    let report = RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("write cube");
    assert!(!report.losses.iter().any(
        |loss| loss.message.contains("face-level name or color") && loss.message.contains(&id)
    ));
    let written = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("read cube");
    assert_eq!(written.ir().model.faces[0].color, ir.model.faces[0].color);
}

#[test]
fn face_color_equal_to_unset_is_named_as_omitted() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube");
    for face in &mut ir.model.faces {
        face.name = None;
        face.color = None;
    }
    ir.model.faces[0].color =
        Some(cadmpeg_ir::topology::Color::new(1.0, 1.0, 1.0, 0.0).expect("transparent white"));
    ir.model.faces[1].color =
        Some(cadmpeg_ir::topology::Color::new(1.0, 0.0, 0.0, 1.0).expect("red"));
    let plan = RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .expect("plan face colors");
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("face-level name or color")
            && loss.message.contains(ir.model.faces[0].id.as_str())));
    assert!(!plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("face-level name or color")
            && loss.message.contains(ir.model.faces[1].id.as_str())));
}

#[test]
fn appearance_change_channels_name_the_binding_when_omitted() {
    let source = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/fixtures/generated_point.3dm"
    ));
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode point");
    let mut ir = decoded.ir().clone();
    let binding = &mut ir.model.appearance_bindings[0];
    binding.channels.insert(
        cadmpeg_core::text::NonBlankString::new("Appearance").expect("channel"),
        "channel-version".into(),
    );
    let id = binding.id.to_string();
    let plan = RhinoCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("plan point");
    assert!(plan.report().losses.iter().any(|loss| {
        loss.code.to_string() == "rhino/writer.appearance-channels-omitted"
            && loss.message.contains(&id)
            && loss.message.contains("Appearance")
    }));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write point");
    let written = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("read point");
    assert_eq!(written.ir().model.points.len(), 1);
}

#[test]
fn absent_model_scope_is_reported_when_writing_a_valid_empty_archive() {
    let source = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/fixtures/damaged/reject_v2.3dm"
    ));
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode empty model scope");
    assert!(!decoded.ir().scope.model);
    let mut bytes = Vec::new();
    let plan = RhinoCodec
        .plan(EncodeInput::new(decoded.ir(), None), TargetRequest::Inherit)
        .expect("plan empty model archive");
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::RhinoLossCode::WriterScopeExpanded.kind()));
    plan.write_to(&mut bytes)
        .expect("write empty model archive");
    let written = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("read empty model scope");
    assert!(written.ir().scope.model);
    assert!(!written.ir().scope.design);
    assert_eq!(written.ir().tolerances, decoded.ir().tolerances);

    let mut edited = decoded.ir().clone();
    edited.tolerances = cadmpeg_ir::units::Tolerances::new(0.01, 0.001).expect("tolerances");
    let mut bytes = Vec::new();
    RhinoCodec
        .plan(EncodeInput::new(&edited, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("write edited tolerances");
    let written = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("read edited tolerances");
    assert_eq!(written.ir().tolerances, edited.tolerances);
}

#[test]
fn non_object_layer_members_are_named_in_the_export_loss() {
    use cadmpeg_ir::presentation::{PresentationItem, PresentationLayer};

    let mut ir = super::adjacent_quad_sheet();
    let face = ir.model.faces[0].id.clone();
    let layer = cadmpeg_ir::ids::LayerId::mint("test:model:layer#face").expect("layer identity");
    ir.model.presentation_layers.push(PresentationLayer {
        id: layer.clone(),
        native_ref: None,
        name: "Face".into(),
        parent: None,
        description: None,
        visible: None,
        items: vec![PresentationItem::Face { face: face.clone() }],
    });
    let plan = RhinoCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("plan layer");
    assert!(plan.report().losses.iter().any(|loss| {
        loss.code.to_string() == "rhino/writer.layer-membership-omitted"
            && loss.message.contains(layer.as_str())
            && loss.message.contains(face.as_str())
    }));
    assert!(plan.report().losses.iter().any(|loss| {
        loss.code.to_string() == "rhino/writer.layer-visibility-defaulted"
            && loss.message.contains(layer.as_str())
    }));
    let mut output = Vec::new();
    plan.write_to(&mut output).expect("write layer");
    let written = RhinoCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("read layer");
    assert_eq!(written.ir().model.presentation_layers[0].name, "Face");
    assert_eq!(
        written.ir().model.presentation_layers[0].visible,
        Some(true)
    );
    ir.model.presentation_layers[0].visible = Some(true);
    let explicit = RhinoCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("plan explicit layer visibility");
    assert!(explicit
        .report()
        .losses
        .iter()
        .all(|loss| { loss.code.to_string() != "rhino/writer.layer-visibility-defaulted" }));
}

#[test]
fn unplaced_empty_definition_survives_rewrite() {
    let source = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/fixtures/instance_reference_retained.3dm"
    ));
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode definition");
    assert!(decoded.ir().model.occurrences.is_empty());
    assert_eq!(decoded.ir().model.product_definitions.len(), 1);
    for version in [RhinoArchiveVersion::V5, RhinoArchiveVersion::V8] {
        let mut output = Vec::new();
        RhinoCodec
            .plan(
                EncodeInput::new(decoded.ir(), None),
                TargetRequest::Explicit(version.descriptor().id.as_str()),
            )
            .and_then(|plan| plan.write_to(&mut output))
            .expect("write definition");
        let written = RhinoCodec
            .decode(&mut Cursor::new(output), &DecodeOptions::default())
            .expect("read definition");
        assert_eq!(
            written.ir().model.product_definitions,
            decoded.ir().model.product_definitions
        );
    }
}

#[test]
fn subd_without_a_layer_does_not_create_an_unresolved_layer_reference() {
    let source = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/fixtures/subd_v80.3dm"
    ));
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode SubD");
    for version in [
        RhinoArchiveVersion::V5,
        RhinoArchiveVersion::V6,
        RhinoArchiveVersion::V7,
        RhinoArchiveVersion::V8,
    ] {
        let mut output = Vec::new();
        RhinoCodec
            .plan(
                EncodeInput::new(decoded.ir(), None),
                TargetRequest::Explicit(version.descriptor().id.as_str()),
            )
            .and_then(|plan| plan.write_to(&mut output))
            .expect("write SubD");
        let written = RhinoCodec
            .decode(&mut Cursor::new(output), &DecodeOptions::default())
            .expect("read SubD");
        assert_eq!(
            written.ir().model.subds.len(),
            decoded.ir().model.subds.len()
        );
        assert!(!written
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains("layer index")));
        assert!(!written
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains("material selector -1")));
    }
}

#[test]
fn revolution_surface_keeps_its_procedural_construction() {
    let source = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/fixtures/rev_surface.3dm"
    ));
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode revolution");
    for version in [RhinoArchiveVersion::V5, RhinoArchiveVersion::V8] {
        let mut output = Vec::new();
        let report = RhinoCodec
            .plan(
                EncodeInput::new(decoded.ir(), None),
                TargetRequest::Explicit(version.descriptor().id.as_str()),
            )
            .and_then(|plan| plan.write_to(&mut output))
            .expect("write revolution");
        let written = RhinoCodec
            .decode(&mut Cursor::new(output), &DecodeOptions::default())
            .expect("read revolution");
        assert_eq!(
            written.ir().model.procedural_surfaces,
            decoded.ir().model.procedural_surfaces
        );
        assert!(!report.losses.iter().any(|loss| loss
            .message
            .contains(decoded.ir().model.procedural_surfaces[0].id.as_str())));
    }
}

#[test]
fn singular_trim_and_seam_remain_in_the_written_body() {
    let source = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/fixtures/brep_singular_seam.3dm"
    ));
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode singular seam");
    let mut output = Vec::new();
    RhinoCodec
        .plan(EncodeInput::new(decoded.ir(), None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut output))
        .expect("write singular seam");
    let written = RhinoCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("read singular seam");
    assert_eq!(
        written.ir().model.faces.len(),
        decoded.ir().model.faces.len()
    );
    assert_eq!(
        written.ir().model.coedges.len(),
        decoded.ir().model.coedges.len()
    );
    assert_eq!(
        written.ir().model.edges.len(),
        decoded.ir().model.edges.len()
    );
    assert_eq!(
        written.ir().model.pcurves.len(),
        decoded.ir().model.pcurves.len()
    );
    assert_eq!(written.ir().model.edges, decoded.ir().model.edges);
    assert_eq!(written.ir().model.coedges, decoded.ir().model.coedges);
    for (actual, expected) in written
        .ir()
        .model
        .pcurves
        .iter()
        .zip(&decoded.ir().model.pcurves)
    {
        assert_eq!(actual.id, expected.id);
        assert_eq!(actual.metadata, expected.metadata);
        let (
            cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs: actual },
            cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs: expected },
        ) = (&actual.geometry, &expected.geometry)
        else {
            panic!("trim carriers must remain NURBS")
        };
        assert_eq!(actual.knots(), expected.knots());
        for (actual, expected) in actual
            .control_points()
            .iter()
            .zip(expected.control_points())
        {
            assert!((actual.u - expected.u).abs() < SINGULAR_TRIM_TOLERANCE);
            assert!((actual.v - expected.v).abs() < SINGULAR_TRIM_TOLERANCE);
        }
    }
}

#[test]
fn singular_edge_parameter_range_is_named_when_omitted() {
    let source = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/fixtures/brep_singular_seam.3dm"
    ));
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode singular seam");
    let mut ir = decoded.ir().clone();
    let edge = ir
        .model
        .edges
        .iter_mut()
        .find(|edge| edge.curve().is_none())
        .expect("singular edge");
    edge.carrier =
        cadmpeg_ir::topology::EdgeCarrier::new(None, Some([0.0, 1.0])).expect("edge domain");
    let id = edge.id.to_string();
    let mut output = Vec::new();
    let report = RhinoCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut output))
        .expect("write singular seam");
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.message.contains(&id) && loss.message.contains("parameter range")));
    let written = RhinoCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("read singular seam");
    assert_eq!(written.ir().model.faces.len(), 1);
}

#[test]
fn product_definition_keeps_its_native_uuid() {
    let source = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/fixtures/instance_reference_expanded.3dm"
    ));
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode instance");
    let mut output = Vec::new();
    RhinoCodec
        .plan(EncodeInput::new(decoded.ir(), None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut output))
        .expect("write instance");
    let written = RhinoCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("read instance");
    assert_eq!(
        written.ir().model.product_definitions[0].native_ref,
        decoded.ir().model.product_definitions[0].native_ref
    );
    let definition = |ir: &CadIr| {
        ir.native
            .namespace("rhino")
            .expect("namespace")
            .arenas()
            .get("product_definitions")
            .expect("definitions")[0]
            .fields()
            .expect("fields")
    };
    assert_eq!(definition(written.ir()), definition(decoded.ir()));
    assert_eq!(
        written.ir().model.occurrences[0].prototype,
        decoded.ir().model.occurrences[0].prototype
    );
    assert_eq!(
        written.ir().model.occurrences,
        decoded.ir().model.occurrences
    );
}

#[test]
fn nested_occurrence_uses_its_held_uuid_in_the_definition() {
    let source = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/fixtures/instance_reference_expanded.3dm"
    ));
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode instance");
    let mut ir = decoded.ir().clone();
    let mut child_definition = ir.model.product_definitions[0].clone();
    child_definition.id = "test:model:product#child"
        .try_into()
        .expect("definition id");
    child_definition.native_ref = None;
    ir.model.product_definitions.push(child_definition.clone());
    let mut child = ir.model.occurrences[0].clone();
    child.id = "test:model:occurrence#child"
        .try_into()
        .expect("occurrence id");
    child.prototype = PrototypeReference::Local {
        definition: child_definition.id,
    };
    child.parent = OccurrenceParent::Occurrence {
        occurrence: ir.model.occurrences[0].id.clone(),
    };
    child.name = Some("nested child".into());
    child.visible = Some(true);
    let native_id = "rhino:product:occurrence#71717171-7171-7171-7171-717171717171";
    child.native_ref = Some(native_id.into());
    ir.model.occurrences.push(child);
    let rows = ir
        .native
        .namespace_mut("rhino")
        .arenas_mut()
        .get_mut("product_occurrences")
        .expect("native occurrences");
    let mut fields = rows[0].fields().expect("native fields");
    fields.insert(
        "source_uuid".into(),
        serde_json::json!("71717171-7171-7171-7171-717171717171"),
    );
    rows.push(
        cadmpeg_ir::NativeRecord::new(native_id.try_into().expect("native id"), fields)
            .expect("native record"),
    );
    let mut output = Vec::new();
    RhinoCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut output))
        .expect("write assembly");
    let written = RhinoCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("read assembly");
    let nested = written
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.name.as_deref() == Some("nested child"))
        .expect("nested occurrence");
    assert!(matches!(nested.parent, OccurrenceParent::Occurrence { .. }));
    assert_eq!(nested.native_ref.as_deref(), Some(native_id));
}

#[test]
fn external_occurrence_with_path_and_uuid_round_trips() {
    let mut ir = CadIr::empty();
    let prototype = PrototypeReference::External {
        document: ExternalDocument::path("parts/widget.3dm"),
        object: Some("61616161-6161-6161-6161-616161616161".into()),
    };
    ir.model.occurrences.push(Occurrence {
        id: "test:model:occurrence#external"
            .try_into()
            .expect("identity"),
        prototype: prototype.clone(),
        parent: OccurrenceParent::Root {},
        ordinal: 0,
        transform: Some(Transform::identity()),
        linked_prototype: None,
        name: None,
        visible: None,
        link: None,
        native_ref: None,
    });
    for version in [RhinoArchiveVersion::V5, RhinoArchiveVersion::V8] {
        let mut output = Vec::new();
        let report = RhinoCodec
            .plan(
                EncodeInput::new(&ir, None),
                TargetRequest::Explicit(version.descriptor().id.as_str()),
            )
            .and_then(|plan| plan.write_to(&mut output))
            .expect("write external occurrence");
        assert!(report
            .losses
            .iter()
            .all(|loss| loss.code != crate::loss::RhinoLossCode::ProductOccurrenceDropped.kind()));
        let written = RhinoCodec
            .decode(&mut Cursor::new(output), &DecodeOptions::default())
            .expect("read external occurrence");
        assert!(written
            .ir()
            .model
            .occurrences
            .iter()
            .any(|occurrence| occurrence.prototype == prototype
                && occurrence.transform == Some(Transform::identity())));
    }
}

#[test]
fn external_uuid_conflict_drops_only_the_conflicting_occurrence() {
    let mut ir = CadIr::empty();
    let uuid = "61616161-6161-6161-6161-616161616161";
    for (ordinal, path, transform) in [
        (0, "unplaced.3dm", None),
        (1, "first.3dm", Some(Transform::identity())),
        (2, "conflict.3dm", Some(Transform::identity())),
        (3, "first.3dm", Some(Transform::identity())),
    ] {
        ir.model.occurrences.push(Occurrence {
            id: format!("test:model:occurrence#{ordinal}")
                .try_into()
                .expect("identity"),
            prototype: PrototypeReference::External {
                document: ExternalDocument::path(path),
                object: Some(uuid.into()),
            },
            parent: OccurrenceParent::Root {},
            ordinal,
            transform,
            linked_prototype: None,
            name: None,
            visible: None,
            link: None,
            native_ref: None,
        });
    }
    let mut output = Vec::new();
    let report = RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut output))
        .expect("write the unambiguous occurrence");
    for index in [0, 2] {
        assert!(report.losses.iter().any(|loss| loss
            .message
            .contains(ir.model.occurrences[index].id.as_str())));
    }
    let written = RhinoCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("read archive");
    assert_eq!(written.ir().model.occurrences.len(), 2);
    for occurrence in &written.ir().model.occurrences {
        assert_eq!(occurrence.prototype, ir.model.occurrences[1].prototype);
    }
}

#[test]
fn point_body_keeps_the_layer_assigned_to_its_point() {
    let source = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/fixtures/generated_point.3dm"
    ));
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode point");
    let mut output = Vec::new();
    RhinoCodec
        .plan(
            EncodeInput::new(decoded.ir(), None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut output))
        .expect("write point");
    let written = RhinoCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("read point");
    assert_eq!(
        written.ir().model.presentation_layers[0].items,
        decoded.ir().model.presentation_layers[0].items
    );
    assert_eq!(
        written.ir().model.presentation_layers[0].native_ref,
        decoded.ir().model.presentation_layers[0].native_ref
    );
    let layers = |ir: &CadIr| {
        ir.native
            .namespace("rhino")
            .expect("namespace")
            .arenas()
            .get("layers")
            .expect("layers")[0]
            .fields()
            .expect("fields")
    };
    assert_eq!(layers(written.ir()), layers(decoded.ir()));
    assert_eq!(
        written.ir().model.view_presentations,
        decoded.ir().model.view_presentations
    );
    assert_eq!(
        written.ir().model.bodies[0].color,
        decoded.ir().model.bodies[0].color
    );
}

#[test]
fn point_layer_color_and_object_visibility_are_independent() {
    let source = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/fixtures/generated_point.3dm"
    ));
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode point");
    let mut ir = decoded.ir().clone();
    let color = cadmpeg_ir::topology::Color::new(1.0, 0.0, 0.0, 1.0).expect("color");
    ir.model.appearances[0].base_color = Some(color);
    ir.model.bodies[0].color = Some(color);
    ir.model.presentation_layers[0].visible = Some(false);
    let mut output = Vec::new();
    RhinoCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut output))
        .expect("write edited point");
    let written = RhinoCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("read edited point");
    assert_eq!(written.ir().model.appearances, ir.model.appearances);
    assert_eq!(
        written.ir().model.appearance_bindings,
        ir.model.appearance_bindings
    );
    assert_eq!(
        written.ir().model.view_presentations,
        ir.model.view_presentations
    );
    assert_eq!(
        written.ir().model.presentation_layers[0].visible,
        Some(false)
    );
}

#[test]
fn omitted_sheet_face_keeps_its_independent_points() {
    let mut ir = super::adjacent_quad_sheet();
    ir.model.faces[1].tolerance =
        cadmpeg_ir::scalar::PositiveReal::new(UNREPRESENTABLE_FACE_TOLERANCE);
    let omitted = ir.model.faces[1].id.clone();
    let mut output = Vec::new();
    let report = RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut output))
        .expect("write remaining face and geometry");
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.message.contains(omitted.as_str())));
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("read remaining face and geometry");
    assert_eq!(decoded.ir().model.faces.len(), 1);
    for point in &ir.model.points {
        assert!(decoded
            .ir()
            .model
            .points
            .iter()
            .any(|row| row.position() == point.position()));
    }
}

#[test]
fn solid_with_one_unrepresentable_face_keeps_the_other_face() {
    let mut ir = super::adjacent_quad_sheet();
    ir.model.bodies[0].kind = cadmpeg_ir::topology::BodyKind::Solid;
    ir.model.faces[1].tolerance =
        cadmpeg_ir::scalar::PositiveReal::new(UNREPRESENTABLE_FACE_TOLERANCE);
    let body = ir.model.bodies[0].id.clone();
    let omitted = ir.model.faces[1].id.clone();
    let mut output = Vec::new();
    let report = RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut output))
        .expect("write representable face");
    let written = RhinoCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("read representable face");
    assert_eq!(written.ir().model.faces.len(), 1);
    assert!(written
        .ir()
        .model
        .bodies
        .iter()
        .any(|row| row.kind == cadmpeg_ir::topology::BodyKind::Sheet));
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.message.contains(body.as_str())));
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.message.contains(omitted.as_str())));
}

#[test]
fn sheet_with_missing_edge_carriers_retains_its_points_and_surface() {
    let mut ir = super::polygon_sheet(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 2.0, 0.0),
    ]);
    for edge in &mut ir.model.edges {
        edge.carrier = cadmpeg_ir::topology::EdgeCarrier::new(None, Some([0.0, 1.0]))
            .expect("edge without a carrier");
    }
    ir.model.curves.clear();
    let mut output = Vec::new();
    let report = RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut output))
        .expect("write representable geometry");
    for edge in &ir.model.edges {
        assert!(report
            .losses
            .iter()
            .any(|loss| loss.message.contains(edge.id.as_str())));
    }
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("read representable geometry");
    assert!(decoded
        .ir()
        .model
        .surfaces
        .iter()
        .any(|surface| surface.geometry == ir.model.surfaces[0].geometry));
    for point in &ir.model.points {
        assert!(decoded
            .ir()
            .model
            .points
            .iter()
            .any(|written| written.position() == point.position()));
    }
}

#[test]
fn object_without_a_uuid_keeps_its_identity_on_rewrite() {
    let source = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/fixtures/point.3dm"
    ));
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode point");
    let identity = decoded.ir().model.bodies[0].id.clone();
    let plan = RhinoCodec
        .plan(
            EncodeInput::new(decoded.ir(), None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .expect("plan point");
    assert!(plan.report().losses.iter().all(|loss| {
        loss.code.local_code() != "native.records-omitted"
            || !loss.message.contains("rhino.document_settings")
    }));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write point");
    let written = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode written point");
    let written_body = written
        .ir()
        .model
        .bodies
        .iter()
        .find(|body| body.id == identity)
        .expect("body identity survives");
    assert_eq!(written_body.color, decoded.ir().model.bodies[0].color);
    assert_eq!(written_body.name, decoded.ir().model.bodies[0].name);
    assert_eq!(written_body.visible, decoded.ir().model.bodies[0].visible);
}

#[test]
fn omitted_face_display_overrides_name_each_face() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube");
    for face in &mut ir.model.faces {
        face.name = None;
        face.color = None;
    }
    ir.model.faces[0].name = Some("first".into());
    ir.model.faces[1].name = Some("second".into());
    let plan = RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .expect("write cube");
    let loss = plan
        .report()
        .losses
        .iter()
        .find(|loss| {
            loss.code == crate::loss::RhinoLossCode::WriterAttributesOmitted.kind()
                && loss.message.contains("face-level name or color")
        })
        .expect("face override loss");
    assert!(loss.message.contains(ir.model.faces[0].id.as_str()));
    assert!(loss.message.contains(ir.model.faces[1].id.as_str()));
    assert!(!loss.message.contains(ir.model.faces[2].id.as_str()));
}

/// One point body, one part definition holding it, and two root occurrences
/// placing that part at x=100 and x=200.
fn placed_point_product() -> CadIr {
    let mut ir = CadIr::empty();
    let body: cadmpeg_ir::ids::BodyId = "cadir:model:body#pin".try_into().expect("identity");
    let region: cadmpeg_ir::ids::RegionId = "cadir:model:region#pin".try_into().expect("identity");
    let shell: cadmpeg_ir::ids::ShellId = "cadir:model:shell#pin".try_into().expect("identity");
    let vertex = cadmpeg_ir::ids::VertexId::mint("cadir:model:vertex#pin").expect("identity");
    let point = cadmpeg_ir::ids::PointId::mint("cadir:model:point#pin").expect("identity");
    ir.model.bodies.push(cadmpeg_ir::topology::Body {
        native_ref: None,
        id: body.clone(),
        kind: cadmpeg_ir::topology::BodyKind::General,
        regions: vec![region.clone()],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir.model.regions.push(cadmpeg_ir::topology::Region {
        native_ref: None,
        id: region.clone(),
        body: body.clone(),
        shells: vec![shell.clone()],
    });
    ir.model.shells.push(
        cadmpeg_ir::topology::Shell::new(
            shell,
            region,
            Vec::new(),
            Vec::new(),
            vec![vertex.clone()],
        )
        .expect("free-vertex shell"),
    );
    ir.model.vertices.push(cadmpeg_ir::topology::Vertex {
        native_ref: None,
        id: vertex,
        point: point.clone(),
        tolerance: None,
    });
    ir.model.points.push(cadmpeg_ir::topology::Point::new(
        point,
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
            .expect("finite position"),
        None,
    ));
    let part: cadmpeg_ir::ids::ProductDefinitionId =
        "cadir:model:product#pin".try_into().expect("identity");
    ir.model.product_definitions.push(ProductDefinition {
        sketches: Vec::default(),
        id: part.clone(),
        kind: ProductDefinitionKind::Part,
        source_name: Some("Pin".into()),
        label: None,
        description: None,
        part_number: None,
        bom_properties: std::collections::BTreeMap::new(),
        bodies: vec![body],
        native_ref: None,
    });
    for (ordinal, x) in [(0, 100.0), (1, 200.0)] {
        ir.model.occurrences.push(Occurrence {
            id: cadmpeg_ir::ids::OccurrenceId::mint(format!(
                "cadir:model:occurrence#pin.{ordinal}"
            ))
            .expect("identity"),
            prototype: PrototypeReference::Local {
                definition: part.clone(),
            },
            parent: OccurrenceParent::Root {},
            ordinal,
            transform: Some(Transform::identity().with_translation(
                FiniteVector3::new(Vector3::new(x, 0.0, 0.0)).expect("finite offset"),
            )),
            linked_prototype: None,
            name: None,
            visible: None,
            link: None,
            native_ref: None,
        });
    }
    ir.finalize();
    ir
}

#[test]
fn styled_point_body_retains_product_and_group_membership() {
    let source = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/fixtures/e2_structure.3dm"
    ));
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode synthesized product");
    let plan = RhinoCodec
        .plan(
            EncodeInput::new(decoded.ir(), None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .expect("plan product");
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write product");
    let output = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode written product");
    let ir = output.ir();
    assert_eq!(ir.model.product_definitions[0].bodies.len(), 1);
    for appearance in &decoded.ir().model.appearances {
        assert!(ir.model.appearances.iter().any(|row| row.id == appearance.id), "missing appearance {}", appearance.id);
    }
    let source_annotation =
        &decoded.ir().native.namespace("rhino").unwrap().arenas()["annotations"][0];
    let written_annotation = &ir.native.namespace("rhino").unwrap().arenas()["annotations"][0];
    assert_eq!(written_annotation.id(), source_annotation.id());
    assert_eq!(
        written_annotation.field("source_uuid"),
        source_annotation.field("source_uuid")
    );
    let source_view = &decoded.ir().native.namespace("rhino").unwrap().arenas()["views"][0];
    let written_view = &ir.native.namespace("rhino").unwrap().arenas()["views"][0];
    assert_eq!(
        written_view.field("children"),
        source_view.field("children")
    );
    let group = &ir.native.namespace("rhino").unwrap().arenas()["groups"][0];
    assert_eq!(group.field("links").unwrap().as_array().unwrap().len(), 3);
    let attributes = &ir.native.namespace("rhino").unwrap().arenas()["object_presentation"];
    assert!(attributes.iter().any(|record| {
        record
            .field("name")
            .as_ref()
            .and_then(serde_json::Value::as_str)
            == Some("Member")
            && record
                .field("material_index")
                .as_ref()
                .and_then(serde_json::Value::as_i64)
                == Some(0)
    }));
}

#[test]
fn product_structure_is_written_as_instance_definitions_and_references() {
    let ir = placed_point_product();
    let validation =
        cadmpeg_ir::validate_neutral(&ir, Vec::new()).expect("validate the neutral model");
    assert!(validation.is_ok(), "{:?}", validation.findings);

    let plan = RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .expect("the point body is writable");
    let report = plan.report();
    for (arena, kind) in [
        (
            "product_definitions",
            cadmpeg_ir::schema::EntityKind::ProductDefinition,
        ),
        ("occurrences", cadmpeg_ir::schema::EntityKind::Occurrence),
    ] {
        assert!(
            report.coverage.disposition(kind).carries(),
            "{arena}: coverage must state that the target carries products"
        );
    }
    assert!(
        !report.losses.iter().any(|loss| {
            loss.code.namespace() == "export"
                && loss.code.local_code() == "arena.omitted"
                && loss.message.contains("`product_definitions`")
        }),
        "product definitions are written, not omitted: {:?}",
        report.losses
    );
    assert!(
        !report.losses.iter().any(|loss| {
            loss.code.namespace() == "export"
                && loss.code.local_code() == "arena.omitted"
                && loss.message.contains("`occurrences`")
        }),
        "occurrences are written, not omitted: {:?}",
        report.losses
    );
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes)
        .expect("write the product structure");
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("read the written products");
    assert_eq!(
        decoded.ir().model.product_definitions.len(),
        1,
        "the part definition reappears"
    );
    assert_eq!(
        decoded.ir().model.occurrences.len(),
        2,
        "both placements reappear"
    );
}

#[test]
fn layer_descriptions_are_written_and_native_normalization_is_named() {
    use cadmpeg_ir::presentation::PresentationLayer;
    for version in [RhinoArchiveVersion::V5, RhinoArchiveVersion::V8] {
        for (description, expected) in [
            ("Author's layer", Some("Author's layer")),
            ("  layer  ", Some("layer")),
            (" ", None),
        ] {
            let mut ir = CadIr::empty();
            let id = cadmpeg_ir::ids::LayerId::mint("test:model:layer#description").unwrap();
            ir.model.presentation_layers.push(PresentationLayer {
                id: id.clone(),
                native_ref: None,
                name: "Layer".into(),
                parent: None,
                description: Some(description.into()),
                visible: Some(true),
                items: Vec::new(),
            });
            let plan = RhinoCodec
                .plan(
                    EncodeInput::new(&ir, None),
                    TargetRequest::Explicit(version.descriptor().id.as_str()),
                )
                .unwrap();
            let normalized = plan.report().losses.iter().any(|loss| {
                loss.code.to_string() == "rhino/writer.layer-description-normalized"
                    && loss.message.contains(id.as_str())
            });
            assert_eq!(normalized, expected != Some(description));
            let mut bytes = Vec::new();
            plan.write_to(&mut bytes).unwrap();
            let decoded = RhinoCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap();
            assert!(decoded
                .report()
                .losses
                .iter()
                .all(|loss| { loss.code != crate::loss::RhinoLossCode::IntegrityFailure.kind() }));
            assert_eq!(
                decoded.ir().model.presentation_layers[0]
                    .description
                    .as_deref(),
                expected
            );
        }
    }
}
