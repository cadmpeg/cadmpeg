// SPDX-License-Identifier: Apache-2.0
//! Valid mixed-dimensional topology must not be mistaken for malformed input.

use std::io::Cursor;

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::topology::{BodyKind, Point, Shell, Vertex};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

use crate::{loss::RhinoLossCode, RhinoCodec};

fn sheet() -> CadIr {
    super::polygon_sheet(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(4.0, 0.0, 0.0),
        Point3::new(0.0, 3.0, 0.0),
    ])
}

fn styled(ir: &mut CadIr) {
    let body = &mut ir.model.bodies[0];
    body.name = Some("Authored mixed body".into());
    body.color = Some(cadmpeg_ir::topology::Color::new(1.0, 0.0, 0.0, 1.0).unwrap());
    body.visible = Some(false);
    body.transform = Some(
        Transform::identity()
            .with_translation(FiniteVector3::new(Vector3::new(10.0, 20.0, 30.0)).unwrap()),
    );
}

fn wire(kind: BodyKind) -> CadIr {
    let mut ir = sheet();
    ir.model.bodies[0].kind = kind;
    let edge = ir.model.edges[0].clone();
    let shell = &ir.model.shells[0];
    ir.model.shells[0] = Shell::new(
        shell.id.clone(),
        shell.region.clone(),
        Vec::new(),
        vec![edge.id.clone()],
        Vec::new(),
    )
    .unwrap();
    ir.model.edges = vec![edge.clone()];
    ir.model
        .curves
        .retain(|curve| Some(&curve.id) == edge.curve());
    ir.model
        .vertices
        .retain(|vertex| vertex.id == edge.start || vertex.id == edge.end);
    ir.model.points.retain(|point| {
        ir.model
            .vertices
            .iter()
            .any(|vertex| vertex.point == point.id)
    });
    ir.model.faces.clear();
    ir.model.loops.clear();
    ir.model.coedges.clear();
    ir.model.surfaces.clear();
    ir
}

fn add_free_vertex(ir: &mut CadIr) {
    let point: cadmpeg_ir::ids::PointId = "test:model:point#mixed-free".try_into().unwrap();
    let vertex: cadmpeg_ir::ids::VertexId = "test:model:vertex#mixed-free".try_into().unwrap();
    ir.model.points.push(Point::new(
        point.clone(),
        FinitePoint3::new(Point3::new(2.0, 2.0, 2.0)).unwrap(),
        None,
    ));
    ir.model
        .vertices
        .push(Vertex::new(vertex.clone(), point, None));
    let shell = &ir.model.shells[0];
    ir.model.shells[0] = Shell::new(
        shell.id.clone(),
        shell.region.clone(),
        shell.faces().to_vec(),
        shell.wire_edges().to_vec(),
        vec![vertex],
    )
    .unwrap();
}

fn roundtrip(ir: &CadIr) -> (CadIr, Vec<cadmpeg_ir::report::loss::LossNote>) {
    let plan = RhinoCodec
        .plan(EncodeInput::new(ir, None), TargetRequest::Inherit)
        .expect("valid topology is writable");
    let losses = plan.report().losses.clone();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("encode valid topology");
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode synthesized topology");
    (decoded.into_parts().0, losses)
}

fn assert_style(ir: &CadIr, source: &CadIr) {
    let source = &source.model.bodies[0];
    assert!(!ir.model.bodies.is_empty());
    for body in &ir.model.bodies {
        assert_eq!(body.name, source.name);
        assert_eq!(body.color, source.color);
        assert_eq!(body.visible, source.visible);
    }
}

fn assert_curve_style(
    output: &CadIr,
    curve: &cadmpeg_ir::geometry::Curve,
    source: &CadIr,
    name: Option<&str>,
) {
    let target = cadmpeg_ir::appearance::AppearanceTarget::Curve(curve.id.clone());
    assert_eq!(
        crate::writer::presentation::object_name(output, &target),
        name
    );
    assert_eq!(
        output.effective_display_color(&target),
        source.model.bodies[0].color
    );
    assert_eq!(
        output.effective_visibility(&target),
        source.model.bodies[0].visible
    );
}

fn curve_position(curve: &cadmpeg_ir::geometry::Curve, parameter: f64) -> Point3 {
    cadmpeg_ir::eval::decode::curve_point(
        cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
        &curve.geometry,
        parameter,
    )
    .expect("finite wire carrier evaluation")
    .get()
}

fn assert_grouping_loss(losses: &[cadmpeg_ir::report::loss::LossNote]) {
    assert!(losses.iter().any(|loss| {
        loss.code == RhinoLossCode::WriterEntityOmitted.kind() && loss.severity == Severity::Error
    }));
}

#[test]
fn general_and_wire_bodies_preserve_wire_geometry_style_and_placement() {
    for kind in [BodyKind::General, BodyKind::Wire] {
        let mut ir = wire(kind);
        styled(&mut ir);
        let (decoded, losses) = roundtrip(&ir);
        assert_eq!(decoded.model.curves.len(), 1);
        // Modern ONCurve projection emits a free carrier, not an owning body
        // or edge. That structural reduction must be reported even for one wire.
        assert!(decoded.model.edges.is_empty());
        assert!(decoded.model.bodies.is_empty());
        let curve = &decoded.model.curves[0];
        assert_curve_style(&decoded, curve, &ir, ir.model.bodies[0].name.as_deref());
        assert_eq!(curve_position(curve, 0.0), Point3::new(10.0, 20.0, 30.0));
        assert_eq!(curve_position(curve, 4.0), Point3::new(14.0, 20.0, 30.0));
        assert_grouping_loss(&losses);
    }
}

#[test]
fn general_face_body_retains_faces_and_reports_dimensional_reduction() {
    let mut ir = sheet();
    ir.model.bodies[0].kind = BodyKind::General;
    styled(&mut ir);
    let (decoded, losses) = roundtrip(&ir);
    assert_eq!(decoded.model.faces.len(), 1);
    assert_eq!(decoded.model.edges.len(), 3);
    assert_style(&decoded, &ir);
    assert!(decoded
        .model
        .points
        .iter()
        .all(|point| point.position().get().z == 30.0));
    assert_grouping_loss(&losses);
}

#[test]
fn sheet_with_free_vertex_preserves_faces_and_independent_point() {
    let mut ir = sheet();
    add_free_vertex(&mut ir);
    styled(&mut ir);
    let (decoded, losses) = roundtrip(&ir);
    assert_eq!(decoded.model.faces.len(), 1);
    assert_eq!(decoded.model.edges.len(), 3);
    assert_eq!(decoded.model.bodies.len(), 2);
    assert_eq!(decoded.model.points.len(), 4);
    assert!(decoded
        .model
        .points
        .iter()
        .any(|point| point.position().get() == Point3::new(12.0, 22.0, 32.0)));
    assert_style(&decoded, &ir);
    assert_grouping_loss(&losses);
}

#[test]
fn general_wire_and_free_vertex_preserve_both_dimensions() {
    let mut ir = wire(BodyKind::General);
    add_free_vertex(&mut ir);
    styled(&mut ir);
    let (decoded, losses) = roundtrip(&ir);
    assert_eq!(decoded.model.curves.len(), 1);
    assert_eq!(decoded.model.points.len(), 1);
    assert_eq!(decoded.model.bodies.len(), 1);
    assert!(decoded.model.edges.is_empty());
    assert_eq!(
        decoded.model.points[0].position().get(),
        Point3::new(12.0, 22.0, 32.0)
    );
    assert_style(&decoded, &ir);
    assert_curve_style(
        &decoded,
        &decoded.model.curves[0],
        &ir,
        ir.model.bodies[0].name.as_deref(),
    );
    assert_eq!(
        curve_position(&decoded.model.curves[0], 4.0),
        Point3::new(14.0, 20.0, 30.0)
    );
    assert_grouping_loss(&losses);
}

#[test]
fn dangling_mixed_shell_vertex_is_still_a_malformed_precondition() {
    let mut ir = sheet();
    add_free_vertex(&mut ir);
    ir.model.vertices.pop();
    assert!(matches!(
        RhinoCodec.plan(EncodeInput::new(&ir, None), TargetRequest::Inherit),
        Err(CodecError::Malformed(_)),
    ));
}

#[test]
fn inconsistent_wire_endpoint_is_still_a_malformed_precondition() {
    let mut ir = wire(BodyKind::General);
    ir.model.points[0].set_position(FinitePoint3::new(Point3::new(100.0, 0.0, 0.0)).unwrap());
    assert!(matches!(
        RhinoCodec.plan(EncodeInput::new(&ir, None), TargetRequest::Inherit),
        Err(CodecError::Malformed(_)),
    ));
}

#[test]
fn full_domain_rational_nurbs_wire_preserves_poles_weights_and_placement() {
    use cadmpeg_ir::geometry::{nurbs::NurbsCurve, CurveGeometry, SolvedCurveGeometry};
    let mut ir = wire(BodyKind::General);
    ir.model.curves[0].geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(2.0, 1.0, 0.0),
                Point3::new(4.0, 0.0, 0.0),
            ],
            Some(vec![1.0, 0.5, 1.0]),
            false,
        )
        .expect("rational wire"),
    ));
    ir.model.edges[0].carrier = cadmpeg_ir::topology::EdgeCarrier::new(
        ir.model.edges[0].curve().cloned(),
        Some([0.0, 1.0]),
    )
    .expect("wire domain");
    styled(&mut ir);
    let (decoded, losses) = roundtrip(&ir);
    let Some(SolvedCurveGeometry::Nurbs(nurbs)) = decoded.model.curves[0].geometry.solved() else {
        panic!("rational wire remains a NURBS");
    };
    assert_eq!(
        nurbs
            .weights()
            .expect("rational weights")
            .iter()
            .map(|weight| weight.get())
            .collect::<Vec<_>>(),
        vec![1.0, 0.5, 1.0]
    );
    assert_eq!(
        nurbs
            .control_points()
            .iter()
            .map(|point| point.get())
            .collect::<Vec<_>>(),
        vec![
            Point3::new(10.0, 20.0, 30.0),
            Point3::new(12.0, 21.0, 30.0),
            Point3::new(14.0, 20.0, 30.0),
        ]
    );
    assert!(decoded.model.bodies.is_empty());
    assert!(decoded.model.edges.is_empty());
    assert_curve_style(
        &decoded,
        &decoded.model.curves[0],
        &ir,
        ir.model.bodies[0].name.as_deref(),
    );
    assert_grouping_loss(&losses);
}

#[test]
fn equal_directed_shared_uses_are_warning_valid_and_reduce_one_sheet_face() {
    use cadmpeg_ir::topology::Sense;
    let mut ir = super::adjacent_quad_sheet();
    let mut ring = ir.model.loops[1].coedges().to_vec();
    ring.reverse();
    ir.model.loops[1]
        .replace_ring(ring.clone(), Vec::new())
        .expect("reversed ring");
    for id in &ring {
        let coedge = ir
            .model
            .coedges
            .iter_mut()
            .find(|row| row.id == *id)
            .unwrap();
        coedge.sense = match coedge.sense {
            Sense::Forward => Sense::Reversed,
            Sense::Reversed => Sense::Forward,
        };
    }
    ir.finalize();
    let validation =
        cadmpeg_ir::validate::validate_neutral(&ir, Vec::new()).expect("neutral validation");
    assert_eq!(validation.error_count(), 0, "{:?}", validation.findings);
    assert!(validation
        .findings
        .iter()
        .any(|finding| finding.severity == Severity::Warning
            && finding.message.contains("equal coedge senses")));
    assert!(matches!(super::super::model::WritableModel::try_new(&ir),
        Err(CodecError::UnsupportedContent(reason)) if reason.contains("equal directed uses")));
    let (decoded, losses) = roundtrip(&ir);
    assert_eq!(decoded.model.faces.len(), 1);
    assert_grouping_loss(&losses);
}

#[test]
fn independently_parameterized_pcurve_use_degrades_instead_of_becoming_malformed() {
    use cadmpeg_ir::geometry::{
        pcurve::{LinePcurve, PcurveGeometry, PcurveMetadata},
        DirectedParameterRange,
    };
    use cadmpeg_ir::math::Point2;
    let mut ir = sheet();
    super::make_planar_nurbs_trimmed_face(&mut ir);
    let id = ir.model.coedges[0].pcurves[0].pcurve.clone();
    ir.model.coedges[0].pcurves[0].parameter_range =
        Some(DirectedParameterRange::new([10.0, 20.0]).unwrap());
    let pcurve = ir
        .model
        .pcurves
        .iter_mut()
        .find(|row| row.id == id)
        .unwrap();
    pcurve.geometry = PcurveGeometry::Line(
        LinePcurve::try_new(Point2::new(-4.0, 0.0), Point2::new(0.4, 0.0))
            .expect("same geometric boundary on independent interval"),
    );
    pcurve.metadata = PcurveMetadata::general(
        None,
        Some(cadmpeg_ir::units::FiniteVector::new([10.0, 20.0]).unwrap()),
        None,
    );
    ir.finalize();
    let validation =
        cadmpeg_ir::validate::validate_neutral(&ir, Vec::new()).expect("neutral validation");
    assert_eq!(validation.error_count(), 0, "{:?}", validation.findings);
    assert!(matches!(super::super::model::WritableModel::try_new(&ir),
        Err(CodecError::UnsupportedContent(reason)) if reason.contains("independent pcurve parameter interval")));
    let (decoded, losses) = roundtrip(&ir);
    assert!(decoded.model.faces.is_empty());
    assert_grouping_loss(&losses);
}

#[test]
fn null_containing_body_name_is_omitted_with_warning_and_geometry_is_written() {
    let mut ir = wire(BodyKind::General);
    styled(&mut ir);
    ir.model.bodies[0].name = Some("left\0right".into());
    let (decoded, losses) = roundtrip(&ir);
    assert_eq!(decoded.model.curves.len(), 1);
    assert!(decoded.model.edges.is_empty());
    assert!(decoded.model.bodies.is_empty());
    assert_curve_style(&decoded, &decoded.model.curves[0], &ir, None);
    assert_eq!(
        curve_position(&decoded.model.curves[0], 4.0),
        Point3::new(14.0, 20.0, 30.0)
    );
    assert!(losses.iter().any(
        |loss| loss.code == RhinoLossCode::WriterAttributesOmitted.kind()
            && loss.severity == Severity::Warning
            && loss.message.contains(ir.model.bodies[0].id.as_str())
    ));
    assert!(!losses
        .iter()
        .any(|loss| loss.code == RhinoLossCode::WriterBodyOmitted.kind()));
    assert_grouping_loss(&losses);
}

#[test]
fn sheet_with_wire_edge_keeps_the_face_and_the_independent_curve() {
    let mut ir = sheet();
    let mut wire = ir.model.edges[0].clone();
    wire.id = "test:model:edge#extra-wire".try_into().unwrap();
    ir.model.shells[0].add_wire_edge(wire.id.clone());
    ir.model.edges.push(wire);
    styled(&mut ir);
    let (decoded, losses) = roundtrip(&ir);
    assert_eq!(decoded.model.faces.len(), 1);
    assert_eq!(decoded.model.edges.len(), 3);
    assert_eq!(decoded.model.curves.len(), 4);
    assert_eq!(decoded.model.bodies.len(), 1);
    let free = decoded
        .model
        .curves
        .iter()
        .find(|curve| !crate::writer::is_support(curve.source_object.as_ref()))
        .expect("independent wire curve");
    assert_curve_style(&decoded, free, &ir, ir.model.bodies[0].name.as_deref());
    assert_eq!(curve_position(free, 4.0), Point3::new(14.0, 20.0, 30.0));
    assert_style(&decoded, &ir);
    assert_grouping_loss(&losses);
}
