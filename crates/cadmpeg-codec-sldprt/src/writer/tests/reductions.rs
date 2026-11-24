// SPDX-License-Identifier: Apache-2.0
//! Content from another format that the semantic writer degrades and names
//! instead of refusing the write.
#![allow(clippy::unwrap_used)]

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::Codec;

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
fn a_line_edge_range_is_written_bounded_by_its_vertices_and_named() {
    let ir = written_cube();
    assert!(ir
        .geometry
        .edges
        .iter()
        .all(|edge| edge.param_range().is_some()));
    let losses = plan_losses(&ir);
    let note = loss(&losses, "writer.edge-ranges-omitted").unwrap();
    let edges = ir.geometry.edges.len();
    assert!(
        note.message.starts_with(&format!("{edges} edge(s)")),
        "{}",
        note.message
    );
    assert!(ir
        .geometry
        .edges
        .iter()
        .all(|edge| note.message.contains(edge.id.as_str())));
    let decoded = crate::test_support::ir::encode_decode(&ir);
    assert_eq!(decoded.geometry.edges.len(), ir.geometry.edges.len());
    assert!(decoded
        .geometry
        .edges
        .iter()
        .all(|edge| edge.param_range().is_none()));
    assert_eq!(
        crate::test_support::ir::sorted_point_positions(&decoded),
        crate::test_support::ir::sorted_point_positions(&ir)
    );
}

fn sketch_feature(id: &str, ordinal: u64) -> cadmpeg_ir::features::Feature {
    use cadmpeg_ir::features::{
        evaluation::FeatureEvaluation, selections::DistinctMembers, Feature, FeatureContent,
        FeatureDefinition, FeatureId, FeatureOperation, SketchFeatureBinding,
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
    let mut bytes = Vec::new();
    SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap()
        .write_to(&mut bytes)
        .unwrap();
    let decoded = SldprtCodec
        .decode(
            &mut std::io::Cursor::new(bytes),
            &cadmpeg_ir::codec::DecodeOptions::default(),
        )
        .unwrap();
    assert!(decoded.ir().geometry.faces.is_empty());
    let residual = loss(&losses, "record.residual-unavailable").unwrap();
    assert!(
        residual.message.contains("Contents/Config-0-Partition"),
        "{}",
        residual.message
    );
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
        cadmpeg_ir::features::geometry::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            at.x + 1.0,
            at.y,
            at.z,
        ))
        .unwrap(),
    );
    let losses = plan_losses(&ir);
    assert!(loss(&losses, "record.invalid").is_some(), "{losses:#?}");
}

/// An Error of a part 10 rule of the design layer, an `Unset` parameter
/// that states a value, is a `record.invalid` loss, and the part is written.
#[test]
fn an_error_of_a_design_rule_writes_the_part_with_a_loss() {
    use cadmpeg_ir::design::parameter::{DesignParameter, ParameterDefinition, ParameterValue};
    let mut ir = written_cube();
    ir.design.parameters.push(DesignParameter {
        id: cadmpeg_ir::features::ParameterId::mint("other:design:parameter#unset").unwrap(),
        scope: cadmpeg_ir::ids::ProductDefinitionId::mint("other:model:definition#part").unwrap(),
        owner: None,
        name: None,
        kind: cadmpeg_ir::design::units::ValueKind::Boolean,
        unit: None,
        definition: ParameterDefinition::Unset,
        value: Some(ParameterValue::Boolean(true)),
        display: None,
        tolerance: None,
        decimals: None,
        comment: None,
        native_options: std::collections::BTreeMap::default(),
        source_settings: std::collections::BTreeMap::default(),
        native_ref: None,
    });
    let report = cadmpeg_ir::validate_neutral(&ir, Vec::new()).unwrap();
    assert!(
        report.findings.iter().any(|finding| {
            finding.severity >= cadmpeg_ir::report::Severity::Error
                && finding
                    .message
                    .contains("an `Unset` parameter states a `value`")
        }),
        "{:#?}",
        report.findings
    );
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let losses = plan.report().losses.clone();
    let invalid = losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.invalid")
        .map(|loss| loss.message.as_str())
        .collect::<Vec<_>>();
    assert!(
        invalid
            .iter()
            .any(|message| message.contains("an `Unset` parameter states a `value`")),
        "{losses:#?}"
    );
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    assert!(!bytes.is_empty());
}

#[test]
fn a_generated_units_member_is_named() {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
    let mut ir = written_cube();
    ir.model.attributes.push(SourceAttribute {
        id: cadmpeg_ir::ids::AttributeId::mint("sldprt:metadata:source_linear_unit_code#0:10")
            .unwrap(),
        target: AttributeTarget::Document.into(),
        name: "source_linear_unit_code".into(),
        values: vec![Some(AttributeValue::Integer(0))],
        native_ref: None,
    });
    let losses = plan_losses(&ir);
    let note = loss(&losses, "writer.units-member-generated").unwrap();
    assert!(note.message.contains("no SolidWorks file is known to hold"));
}

const AUTHORED_PMI_DIMENSION: &[u8] =
    include_bytes!("../../../tests/golden/fixtures/authored_pmi_dimension.sldprt");

fn decode(bytes: &[u8]) -> cadmpeg_test_support::EditableDecodeResult {
    SldprtCodec
        .decode(
            &mut std::io::Cursor::new(bytes.to_vec()),
            &cadmpeg_ir::codec::DecodeOptions::default(),
        )
        .unwrap()
        .into()
}

#[test]
fn pmi_added_to_a_native_part_is_named_on_the_patched_path() {
    let mut decoded = decode(AUTHORED_PMI_DIMENSION);
    assert!(decoded.ir().model.pmi.is_empty());
    let id = "sldprt:model:pmi#added";
    decoded.ir_mut().model.pmi.push(cadmpeg_ir::PmiAnnotation {
        id: cadmpeg_ir::ids::PmiId::mint(id).unwrap(),
        name: Some("datum A".into()),
        visible: None,
        targets: vec![cadmpeg_ir::PmiTarget::ShapeAspect {
            source_id: cadmpeg_core::text::NonBlankString::new("F1").unwrap(),
        }
        .into()],
        definition: cadmpeg_ir::PmiDefinition::Datum {
            identification: "A".into(),
        },
        native_ref: None,
        native_options: cadmpeg_ir::design::options::NativeOptions::new(),
        source_settings: cadmpeg_ir::design::options::NativeOptions::new(),
    });
    let plan = SldprtCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .unwrap();
    assert!(
        matches!(
            plan.report().write_path(),
            cadmpeg_ir::report::export::WritePath::Patched { .. }
        ),
        "{:?}",
        plan.report().write_path()
    );
    let note = loss(&plan.report().losses, "writer.pmi-omitted").unwrap();
    assert!(note.message.contains(id), "{}", note.message);
}

#[test]
fn a_native_operation_of_another_format_is_dropped_with_its_dependents_and_named() {
    use cadmpeg_ir::features::{
        evaluation::FeatureEvaluation, FeatureDefinition, FeatureOperation,
    };
    let mut ir = written_cube();
    let mut insert = sketch_feature("other:model:feature#insert", 0);
    insert.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::Native {
            kind: "Component Insert".into(),
            parameters: std::collections::BTreeMap::new(),
        },
    ));
    let mut dependent = sketch_feature("other:model:feature#dependent", 1);
    dependent.dependencies.insert(insert.id.clone());
    let kept = sketch_feature("other:model:feature#kept", 2);
    ir.model.features.extend([insert, dependent, kept]);
    let losses = plan_losses(&ir);
    let note = losses
        .iter()
        .find(|loss| {
            loss.code.local_code() == "writer.features-omitted"
                && loss.message.contains("other:model:feature#insert")
        })
        .unwrap_or_else(|| panic!("{losses:#?}"));
    assert!(
        note.message.contains("other:model:feature#dependent")
            && note.message.contains("native operation of another format"),
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
fn validation_errors_outside_the_geometry_and_history_arenas_and_without_a_record_are_charged() {
    use cadmpeg_ir::attributes::{AttributeTarget, SourceAttribute};
    use cadmpeg_ir::report::check::{Check, Finding};
    use cadmpeg_ir::report::Severity;
    let mut ir = written_cube();
    let attribute = "other:model:attribute#a";
    ir.model.attributes.push(SourceAttribute {
        id: cadmpeg_ir::ids::AttributeId::mint(attribute).unwrap(),
        target: AttributeTarget::Document.into(),
        name: "a".into(),
        values: Vec::new(),
        native_ref: None,
    });
    let mut validation = cadmpeg_ir::validate::validate_neutral(&ir, Vec::new()).unwrap();
    validation.findings.clear();
    for (entity, message) in [
        (Some(attribute), "attribute finding"),
        (None, "document finding"),
        (Some("other:model:face#absent"), "dangling finding"),
    ] {
        validation.findings.push(Finding {
            check: Check::ReferentialIntegrity,
            severity: Severity::Error,
            message: message.into(),
            entity: entity.map(str::to_owned),
            related: Vec::new(),
        });
    }
    let losses = crate::writer::reduce::invalid_records(&ir, &validation);
    assert_eq!(losses.len(), 2, "{losses:#?}");
    assert!(losses
        .iter()
        .all(|loss| loss.code.local_code() == "record.invalid"));
    assert!(
        losses[0].message.contains(attribute),
        "{}",
        losses[0].message
    );
    let rules = &losses[1].message;
    assert!(
        rules.starts_with("2 finding(s)")
            && rules.contains("document finding")
            && rules.contains("other:model:face#absent"),
        "{rules}"
    );
}

#[test]
fn a_retained_partition_keeps_a_body_on_a_torus_the_compact_records_cannot_hold() {
    use crate::test_support::parasolid::{plane_carrier, torus_carrier, triangle_body};
    let plane = plane_carrier(100, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let triangle = triangle_body();
    assert!(triangle.starts_with(&plane));
    let mut body = torus_carrier(
        100,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        1.0,
        2.0,
        [1.0, 0.0, 0.0],
    );
    body.extend_from_slice(&triangle[plane.len()..]);
    let decoded = decode(&crate::test_support::container::sldprt_with_body(&body));
    assert_eq!(decoded.ir().geometry.faces.len(), 1);
    assert!(matches!(
        decoded.ir().geometry.surfaces[0].geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Solved(
            cadmpeg_ir::geometry::SolvedSurfaceGeometry::Torus(_)
        )
    ));
    let (ir, _, mut fidelity) = decoded.into_parts();
    fidelity.content_digest = None;
    let plan = SldprtCodec
        .plan(
            EncodeInput::new(&ir, Some(&fidelity)),
            TargetRequest::Inherit,
        )
        .unwrap();
    assert!(
        loss(&plan.report().losses, "writer.bodies-omitted").is_none(),
        "{:#?}",
        plan.report().losses
    );
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let written = decode(&bytes);
    assert_eq!(written.ir().geometry.faces.len(), 1);
}

#[test]
fn a_faceless_part_keeps_its_swift_annotations() {
    let decoded = decode(&crate::test_support::swift::faceless_swift_fixture());
    assert!(decoded.ir().geometry.faces.is_empty());
    let annotations = |ir: &cadmpeg_ir::CadIr| {
        ir.model
            .pmi
            .iter()
            .map(|annotation| {
                (
                    annotation.id.clone(),
                    serde_json::to_value(&annotation.definition).unwrap(),
                    serde_json::to_value(&annotation.targets).unwrap(),
                )
            })
            .collect::<Vec<_>>()
    };
    let expected = annotations(decoded.ir());
    assert_eq!(expected.len(), 1);
    let (ir, _, mut fidelity) = decoded.into_parts();
    fidelity.content_digest = None;
    let plan = SldprtCodec
        .plan(
            EncodeInput::new(&ir, Some(&fidelity)),
            TargetRequest::Inherit,
        )
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    assert_eq!(annotations(decode(&bytes).ir()), expected);
}
