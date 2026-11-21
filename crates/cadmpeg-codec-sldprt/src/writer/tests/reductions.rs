// SPDX-License-Identifier: Apache-2.0
//! Content from another format that the semantic writer degrades and names
//! instead of refusing the write.
#![allow(clippy::unwrap_used)]

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::Codec;
use cadmpeg_ir::topology::ParameterInterval;

use crate::SldprtCodec;

/// The unit cube as another format states it: explicit edge ranges and face
/// names.
fn foreign_cube() -> cadmpeg_ir::CadIr {
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    ir.geometry.bodies[0].name = None;
    ir
}

fn plan_losses(ir: &cadmpeg_ir::CadIr) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    SldprtCodec
        .plan(EncodeInput::new(ir, None), TargetRequest::Inherit)
        .unwrap()
        .report()
        .losses
        .clone()
}

fn loss<'a>(
    losses: &'a [cadmpeg_ir::report::loss::LossNote],
    code: &str,
) -> Option<&'a cadmpeg_ir::report::loss::LossNote> {
    losses.iter().find(|loss| loss.code.local_code() == code)
}

#[test]
fn explicit_edge_ranges_its_vertices_fix_are_written_without_a_loss() {
    let mut ir = foreign_cube();
    ir.geometry
        .faces
        .iter_mut()
        .for_each(|face| face.name = None);
    assert!(ir
        .geometry
        .edges
        .iter()
        .all(|edge| edge.param_range().is_some()));
    let losses = plan_losses(&ir);
    assert!(
        loss(&losses, "writer.edge-ranges-omitted").is_none(),
        "{losses:?}"
    );
    let decoded = crate::test_support::ir::encode_decode(&ir);
    assert_eq!(decoded.geometry.edges.len(), ir.geometry.edges.len());
    assert_eq!(
        crate::test_support::ir::sorted_point_positions(&decoded),
        crate::test_support::ir::sorted_point_positions(&ir)
    );
}

#[test]
fn an_edge_range_its_vertices_do_not_fix_is_named() {
    let mut ir = foreign_cube();
    ir.geometry
        .faces
        .iter_mut()
        .for_each(|face| face.name = None);
    let edge = &mut ir.geometry.edges[0];
    let [start, end] = edge.param_range().unwrap().get();
    // Past the linear resolution, inside the validator's coincidence allowance.
    edge.set_param_range(Some(ParameterInterval::new([start, end + 1.0e-3]).unwrap()));
    let id = edge.id.as_str().to_owned();
    let losses = plan_losses(&ir);
    let note = loss(&losses, "writer.edge-ranges-omitted").unwrap();
    assert!(note.message.contains(&id), "{}", note.message);
    assert!(note.message.starts_with("1 edge(s)"), "{}", note.message);
}

fn sketch_feature(id: &str, ordinal: u64) -> cadmpeg_ir::features::Feature {
    use cadmpeg_ir::features::{
        DistinctMembers, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation, FeatureId,
        FeatureOperation, SketchFeatureBinding,
    };
    Feature {
        product_definition: None,
        id: FeatureId::mint(id).unwrap(),
        ordinal,
        name: Some(format!("Feature {ordinal}")),
        suppressed: Some(false),
        dependencies: DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Sketch {
                sketch: SketchFeatureBinding::Unresolved,
            },
        )),
        native_ref: None,
    }
}

fn written_cube() -> cadmpeg_ir::CadIr {
    let mut ir = foreign_cube();
    ir.geometry
        .faces
        .iter_mut()
        .for_each(|face| face.name = None);
    ir
}

#[test]
fn a_source_tag_that_is_no_xml_name_does_not_refuse_the_write() {
    let mut ir = written_cube();
    let mut feature = sketch_feature("other:model:feature#sketch", 0);
    feature.source_tag = Some("Sketch Feature".into());
    ir.model.features.push(feature);
    let losses = plan_losses(&ir);
    assert!(
        losses
            .iter()
            .all(|loss| !loss.message.contains("invalid source tag")),
        "{losses:?}"
    );
}

#[test]
fn a_feature_the_history_writer_cannot_write_is_dropped_with_the_features_that_need_it() {
    let mut ir = written_cube();
    let mut unresolved = sketch_feature("other:model:feature#unresolved", 0);
    unresolved.suppressed = None;
    let mut dependent = sketch_feature("other:model:feature#dependent", 1);
    dependent.dependencies.insert(unresolved.id.clone());
    let kept = sketch_feature("other:model:feature#kept", 2);
    ir.model.features.extend([unresolved, dependent, kept]);
    let losses = plan_losses(&ir);
    let note = loss(&losses, "writer.features-omitted").unwrap();
    assert!(
        note.message.contains("other:model:feature#unresolved")
            && note.message.contains("other:model:feature#dependent"),
        "{}",
        note.message
    );
    assert!(
        !note.message.contains("other:model:feature#kept"),
        "{}",
        note.message
    );
}

#[test]
fn face_names_and_a_body_name_without_material_are_written_unnamed_and_named() {
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    ir.geometry.bodies[0].name = Some("Bracket".into());
    ir.geometry.bodies[0].color = None;
    let named_faces = ir
        .geometry
        .faces
        .iter()
        .filter(|face| face.name.is_some())
        .count();
    assert!(named_faces > 0);
    let losses = plan_losses(&ir);
    let faces = loss(&losses, "writer.face-names-omitted").unwrap();
    assert!(
        faces.message.starts_with(&format!("{named_faces} face(s)")),
        "{}",
        faces.message
    );
    assert!(faces.message.contains(ir.geometry.faces[0].id.as_str()));
    let body = loss(&losses, "writer.body-names-omitted").unwrap();
    assert!(body.message.contains(ir.geometry.bodies[0].id.as_str()));
}

#[test]
fn appearance_fields_beside_name_and_colour_are_named() {
    let mut ir = written_cube();
    ir.model
        .appearances
        .push(cadmpeg_ir::appearance::Appearance {
            id: cadmpeg_ir::ids::AppearanceId::mint("other:model:appearance#steel").unwrap(),
            name: Some("Steel".into()),
            asset_guid: Some("asset".into()),
            library_id: None,
            visual_guid: None,
            physical_token: Some("token".into()),
            schema: None,
            category: None,
            base_color: cadmpeg_ir::topology::Color::new(0.5, 0.5, 0.5, 1.0),
            properties: std::collections::BTreeMap::new(),
            textures: Vec::new(),
            native_ref: None,
        });
    let losses = plan_losses(&ir);
    let note = loss(&losses, "writer.appearance-fields-omitted").unwrap();
    assert!(
        note.message
            .contains("other:model:appearance#steel (asset_guid, physical_token)"),
        "{}",
        note.message
    );
}

#[test]
fn semantic_writer_names_an_edge_range_its_vertices_do_not_fix() {
    let decoded = SldprtCodec
        .decode(
            &mut std::io::Cursor::new(crate::test_support::container::sldprt_with_body(
                &crate::test_support::parasolid::triangle_body(),
            )),
            &cadmpeg_ir::codec::DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir = decoded.ir_mut();
        let edge = &mut ir.geometry.edges[0];
        edge.carrier =
            cadmpeg_ir::topology::EdgeCarrier::new(edge.curve().cloned(), Some([0.0, 1.0]))
                .unwrap();
    }
    let edge = decoded.ir().geometry.edges[0].id.as_str().to_owned();
    let report = SldprtCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .unwrap()
        .report()
        .clone();
    assert!(
        report.losses.iter().any(|loss| {
            loss.code.local_code() == "writer.edge-ranges-omitted" && loss.message.contains(&edge)
        }),
        "{:#?}",
        report.losses
    );
}

#[test]
fn a_document_with_no_face_writes_an_empty_partition_and_names_it() {
    let mut ir = written_cube();
    let body = ir.geometry.bodies[0].clone();
    ir.geometry = cadmpeg_ir::document::Topology::default();
    ir.geometry.bodies.push(body);
    let losses = plan_losses(&ir);
    assert!(loss(&losses, "output.empty").is_some(), "{losses:#?}");
    let bodies = loss(&losses, "writer.bodies-omitted").unwrap();
    assert!(bodies.message.contains(ir.geometry.bodies[0].id.as_str()));
}

#[test]
fn a_body_on_an_elliptical_cone_is_not_written_and_is_named() {
    use cadmpeg_ir::geometry::analytic::ConeSurface;
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    let mut ir = written_cube();
    let surface = &mut ir.geometry.surfaces[0];
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) = &surface.geometry else {
        panic!("cube faces are planar");
    };
    surface.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(ConeSurface::new(
        plane.origin(),
        *plane.frame(),
        cadmpeg_ir::scalar::NonNegativeLength::new(1.0).unwrap(),
        cadmpeg_ir::scalar::PositiveReal::new(0.5).unwrap(),
        cadmpeg_ir::scalar::Angle::new(0.3).unwrap(),
    )));
    let losses = plan_losses(&ir);
    let bodies = loss(&losses, "writer.bodies-omitted").unwrap();
    assert!(
        bodies.message.contains(ir.geometry.bodies[0].id.as_str())
            && bodies.message.contains("elliptical cone ratio 0.5"),
        "{}",
        bodies.message
    );
}

#[test]
fn a_validation_error_in_the_written_document_is_record_invalid() {
    let mut ir = written_cube();
    // A vertex 1 mm off both its edges' line ends.
    let point = &mut ir.geometry.points[0];
    let at = point.position().get();
    point.set_position(
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            at.x + 1.0,
            at.y,
            at.z,
        ))
        .unwrap(),
    );
    let losses = plan_losses(&ir);
    assert!(loss(&losses, "record.invalid").is_some(), "{losses:#?}");
}
