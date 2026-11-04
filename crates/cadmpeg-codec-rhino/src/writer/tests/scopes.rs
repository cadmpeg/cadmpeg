// SPDX-License-Identifier: Apache-2.0
//! Valid mixed-dimensional topology must not be mistaken for malformed input.

use std::io::Cursor;

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
    ir.model.vertices.push(Vertex {
        native_ref: None,
        id: vertex.clone(),
        point,
        tolerance: None,
    });
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
fn dangling_mixed_shell_vertex_is_an_export_omission() {
    let mut ir = sheet();
    add_free_vertex(&mut ir);
    ir.model.vertices.pop();
    let plan = RhinoCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("a dangling vertex reference omits its point");
    let loss = plan
        .report()
        .losses
        .iter()
        .find(|loss| {
            loss.code.namespace() == "rhino" && loss.code.local_code() == "writer.entity-omitted"
        })
        .expect("the omission names its record");
    assert!(
        loss.message.contains("test:model:vertex#mixed-free"),
        "{loss:?}"
    );
}

#[test]
fn inconsistent_wire_endpoint_is_an_export_omission() {
    let mut ir = wire(BodyKind::General);
    ir.model.points[0].set_position(FinitePoint3::new(Point3::new(100.0, 0.0, 0.0)).unwrap());
    let plan = RhinoCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("a disagreeing wire endpoint omits its wire");
    let loss = plan
        .report()
        .losses
        .iter()
        .find(|loss| {
            loss.code.namespace() == "rhino" && loss.code.local_code() == "writer.entity-omitted"
        })
        .expect("the omission names its record");
    assert!(loss.message.contains("carrier disagrees"), "{loss:?}");
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
        Err(crate::writer::WriteError::Omit(reason)) if reason.contains("equal directed uses")));
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
        Err(crate::writer::WriteError::Omit(reason)) if reason.contains("independent pcurve parameter interval")));
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

#[test]
fn transformed_plane_surface_resolves_to_world_plane() {
    use super::super::scopes::resolve_surface;
    use cadmpeg_ir::geometry::{
        analytic::PlaneSurface, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::math::{Point3, Vector3};
    let mut surface = Surface {
        native_ref: None,
        id: cadmpeg_ir::ids::SurfaceId::mint("rhino:test:surface#placed-plane")
            .expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed(
            cadmpeg_ir::geometry::PlacedSurface::try_new(
                Box::new(SolvedSurfaceGeometry::Plane(
                    PlaneSurface::try_new(
                        Point3::new(1.0, 2.0, 3.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .unwrap(),
                )),
                Transform::identity().with_translation(
                    cadmpeg_ir::features::FiniteVector3::new(Vector3::new(10.0, 0.0, 0.0)).unwrap(),
                ),
            )
            .unwrap(),
        )),
        source_object: None,
    };
    let mut losses = Vec::new();
    resolve_surface(&mut surface, &mut losses);
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) = &surface.geometry else {
        panic!("a rigid transformed plane resolves, {surface:?}");
    };
    assert_eq!(plane.origin().get(), Point3::new(11.0, 2.0, 3.0));
    assert!(
        losses
            .iter()
            .any(|loss| loss.message.contains("placed-plane")),
        "{losses:?}"
    );
}

#[test]
fn trimmed_line_pcurve_resolves_to_its_segment() {
    use super::super::scopes::resolve_pcurve;
    use cadmpeg_ir::geometry::pcurve::{
        LinePcurve, Pcurve, PcurveGeometry, PcurveMetadata, TrimmedPcurve,
    };
    use cadmpeg_ir::math::Point2;
    let mut pcurve = Pcurve {
        native_ref: None,
        id: cadmpeg_ir::ids::PcurveId::mint("rhino:test:pcurve#trimmed-line")
            .expect("identity grammar"),
        geometry: PcurveGeometry::Trimmed(
            TrimmedPcurve::try_new(
                [2.0, 5.0],
                true,
                Box::new(PcurveGeometry::Line(
                    LinePcurve::try_new(Point2::new(1.0, 1.0), Point2::new(1.0, 0.0)).unwrap(),
                )),
            )
            .unwrap(),
        ),
        metadata: PcurveMetadata::general(None, None, None),
    };
    let mut losses = Vec::new();
    resolve_pcurve(&mut pcurve, &mut losses);
    let PcurveGeometry::Line(line) = &pcurve.geometry else {
        panic!("a trimmed line resolves, {pcurve:?}");
    };
    assert_eq!(line.origin().get(), Point2::new(1.0, 1.0));
    assert!(
        losses
            .iter()
            .any(|loss| loss.message.contains("trimmed-line")),
        "{losses:?}"
    );
}

#[test]
fn reversed_trimmed_line_pcurve_keeps_its_basis_parametrization() {
    use super::super::scopes::resolve_pcurve;
    use cadmpeg_ir::geometry::pcurve::{
        LinePcurve, Pcurve, PcurveGeometry, PcurveMetadata, TrimmedPcurve,
    };
    use cadmpeg_ir::math::Point2;
    let mut pcurve = Pcurve {
        native_ref: None,
        id: cadmpeg_ir::ids::PcurveId::mint("rhino:test:pcurve#reversed-trimmed-line")
            .expect("identity grammar"),
        geometry: PcurveGeometry::Trimmed(
            TrimmedPcurve::try_new(
                [2.0, 5.0],
                false,
                Box::new(PcurveGeometry::Line(
                    LinePcurve::try_new(Point2::new(1.0, 1.0), Point2::new(1.0, 0.0)).unwrap(),
                )),
            )
            .unwrap(),
        ),
        metadata: PcurveMetadata::general(None, None, None),
    };
    let mut losses = Vec::new();
    resolve_pcurve(&mut pcurve, &mut losses);
    let PcurveGeometry::Line(line) = &pcurve.geometry else {
        panic!("a reversed trimmed line resolves, {pcurve:?}");
    };
    assert_eq!(line.origin().get(), Point2::new(1.0, 1.0));
    assert_eq!(line.direction().as_raw(), &Point2::new(1.0, 0.0));
    assert!(
        losses
            .iter()
            .any(|loss| loss.message.contains("reversed-trimmed-line")),
        "{losses:?}"
    );
}


#[test]
fn traversal_broken_loop_is_pruned_with_a_loss() {
    use super::super::scopes::prune_open_loops;
    use cadmpeg_ir::topology::{Coedge, Edge, EdgeCarrier, Loop, LoopBoundary, LoopRing, Sense};
    use cadmpeg_ir::ids::{CoedgeId, EdgeId, FaceId, LoopId, VertexId};
    use cadmpeg_ir::CadIr;
    let mut ir = CadIr::empty();
    let vertex = |id: &str| -> VertexId {
        id.try_into().expect("identity grammar")
    };
    let edge_id = |id: &str| -> EdgeId {
        id.try_into().expect("identity grammar")
    };
    let coedge_id = |id: &str| -> CoedgeId {
        id.try_into().expect("identity grammar")
    };
    let loop_id = |id: &str| -> LoopId {
        id.try_into().expect("identity grammar")
    };
    let face_id = |id: &str| -> FaceId {
        id.try_into().expect("identity grammar")
    };
    // A closed two-coedge ring stays.
    ir.model.edges.push(Edge {
        native_ref: None,
        id: edge_id("cadir:model:edge#closed"),
        carrier: EdgeCarrier::new(None, None).unwrap(),
        start: vertex("cadir:model:vertex#a"),
        end: vertex("cadir:model:vertex#b"),
        tolerance: None,
    });
    ir.model.edges.push(Edge {
        native_ref: None,
        id: edge_id("cadir:model:edge#closed-back"),
        carrier: EdgeCarrier::new(None, None).unwrap(),
        start: vertex("cadir:model:vertex#b"),
        end: vertex("cadir:model:vertex#a"),
        tolerance: None,
    });
    for (id, edge, sense) in [
        ("cadir:model:coedge#closed.0", "cadir:model:edge#closed", Sense::Forward),
        ("cadir:model:coedge#closed.1", "cadir:model:edge#closed-back", Sense::Forward),
    ] {
        ir.model.coedges.push(Coedge {
            native_ref: None,
            id: coedge_id(id),
            owner_loop: loop_id("cadir:model:loop#closed"),
            edge: edge_id(edge),
            radial_next: coedge_id(id),
            sense,
            pcurves: Vec::new(),
            use_curve: None,
        });
    }
    ir.model.loops.push(Loop {
        native_ref: None,
        id: loop_id("cadir:model:loop#closed"),
        face: face_id("cadir:model:face#closed"),
        boundary: LoopBoundary::Ring(
            LoopRing::new(
                vec![
                    coedge_id("cadir:model:coedge#closed.0"),
                    coedge_id("cadir:model:coedge#closed.1"),
                ],
                Vec::new(),
            )
            .unwrap(),
        ),
    });
    // A ring whose traversal jumps between vertices is pruned.
    ir.model.edges.push(Edge {
        native_ref: None,
        id: edge_id("cadir:model:edge#open"),
        carrier: EdgeCarrier::new(None, None).unwrap(),
        start: vertex("cadir:model:vertex#c"),
        end: vertex("cadir:model:vertex#d"),
        tolerance: None,
    });
    ir.model.coedges.push(Coedge {
        native_ref: None,
        id: coedge_id("cadir:model:coedge#open.0"),
        owner_loop: loop_id("cadir:model:loop#open"),
        edge: edge_id("cadir:model:edge#open"),
        radial_next: coedge_id("cadir:model:coedge#open.0"),
        sense: Sense::Forward,
        pcurves: Vec::new(),
        use_curve: None,
    });
    ir.model.loops.push(Loop {
        native_ref: None,
        id: loop_id("cadir:model:loop#open"),
        face: face_id("cadir:model:face#open"),
        boundary: LoopBoundary::Ring(
            LoopRing::new(vec![coedge_id("cadir:model:coedge#open.0")], Vec::new()).unwrap(),
        ),
    });
    let mut losses = Vec::new();
    prune_open_loops(
        &mut ir,
        &mut |arena: &str, id: &str, reason: String| {
            losses.push(format!("{arena} {id} {reason}"));
        },
    );
    assert!(
        ir.model
            .loops
            .iter()
            .any(|row| row.id.as_str() == "cadir:model:loop#closed"),
        "a closed ring stays"
    );
    assert!(
        ir.model
            .loops
            .iter()
            .all(|row| row.id.as_str() != "cadir:model:loop#open"),
        "an open ring is pruned"
    );
    assert!(
        losses
            .iter()
            .any(|loss| loss.contains("cadir:model:loop#open")),
        "{losses:?}"
    );
}

#[test]
fn orphaned_edge_is_pruned_with_a_loss_while_used_and_wire_edges_stay() {
    use super::super::scopes::prune_orphaned_edges;
    use cadmpeg_ir::ids::{CoedgeId, EdgeId, LoopId, RegionId, ShellId, VertexId};
    use cadmpeg_ir::topology::{Coedge, Edge, EdgeCarrier, Sense, Shell};
    use cadmpeg_ir::CadIr;
    let mut ir = CadIr::empty();
    let edge_id = |id: &str| -> EdgeId { id.try_into().expect("identity grammar") };
    for (id, start, end) in [
        ("cadir:model:edge#used", "cadir:model:vertex#a", "cadir:model:vertex#b"),
        ("cadir:model:edge#orphan", "cadir:model:vertex#c", "cadir:model:vertex#d"),
        ("cadir:model:edge#wire", "cadir:model:vertex#e", "cadir:model:vertex#f"),
    ] {
        ir.model.edges.push(Edge {
            native_ref: None,
            id: edge_id(id),
            carrier: EdgeCarrier::new(None, None).unwrap(),
            start: VertexId::try_from(start).expect("identity grammar"),
            end: VertexId::try_from(end).expect("identity grammar"),
            tolerance: None,
        });
    }
    ir.model.coedges.push(Coedge {
        native_ref: None,
        id: CoedgeId::try_from("cadir:model:coedge#used").expect("identity grammar"),
        owner_loop: LoopId::try_from("cadir:model:loop#used").expect("identity grammar"),
        edge: edge_id("cadir:model:edge#used"),
        radial_next: CoedgeId::try_from("cadir:model:coedge#used").expect("identity grammar"),
        sense: Sense::Forward,
        pcurves: Vec::new(),
        use_curve: None,
    });
    ir.model.shells.push(Shell::with_wire_edge(
        ShellId::try_from("cadir:model:shell#wire").expect("identity grammar"),
        RegionId::try_from("cadir:model:region#wire").expect("identity grammar"),
        edge_id("cadir:model:edge#wire"),
    ));
    let mut losses = Vec::new();
    prune_orphaned_edges(&mut ir, &mut |arena: &str, id: &str, reason: String| {
        losses.push(format!("{arena} {id} {reason}"));
    });
    assert!(
        ir.model.edges.iter().any(|row| row.id.as_str() == "cadir:model:edge#used"),
        "a used edge stays"
    );
    assert!(
        ir.model.edges.iter().any(|row| row.id.as_str() == "cadir:model:edge#wire"),
        "a wire edge stays"
    );
    assert!(
        ir.model.edges.iter().all(|row| row.id.as_str() != "cadir:model:edge#orphan"),
        "an orphaned edge is pruned"
    );
    assert!(
        losses.iter().any(|loss| loss.contains("cadir:model:edge#orphan")),
        "{losses:?}"
    );
}

#[test]
fn admission_passes_a_consistent_document_through_unchanged() {
    let ir = sheet();
    let (admitted, losses) = super::super::scopes::admit(&ir);
    assert!(losses.is_empty(), "{losses:?}");
    assert!(
        std::matches!(admitted, std::borrow::Cow::Borrowed(_)),
        "a consistent document borrows instead of cloning"
    );
}

#[test]
fn admission_drops_duplicate_identities_keeping_the_first() {
    let mut ir = sheet();
    let duplicate = ir.model.coedges[0].clone();
    ir.model.coedges.push(duplicate);
    let curve = ir.model.curves[0].clone();
    ir.model.curves.push(curve);
    let coedge_id = ir.model.coedges[0].id.as_str().to_owned();
    let curve_id = ir.model.curves[0].id.as_str().to_owned();
    let (admitted, losses) = super::super::scopes::admit(&ir);
    let admitted = admitted.into_owned();
    assert_eq!(
        admitted
            .model
            .coedges
            .iter()
            .filter(|row| row.id.as_str() == coedge_id)
            .count(),
        1,
        "one coedge keeps the duplicated identity"
    );
    assert_eq!(
        admitted
            .model
            .curves
            .iter()
            .filter(|row| row.id.as_str() == curve_id)
            .count(),
        1,
        "one curve keeps the duplicated identity"
    );
    assert!(
        losses.iter().any(|loss| loss.message.contains(&coedge_id)),
        "{losses:?}"
    );
    assert!(
        losses.iter().any(|loss| loss.message.contains(&curve_id)),
        "{losses:?}"
    );
}

#[test]
fn admission_drops_records_with_dangling_ownership_back_references() {
    let mut ir = sheet();
    let region_id = ir.model.regions[0].id.as_str().to_owned();
    ir.model.regions[0].body = "test:model:body#missing".try_into().expect("identity");
    let shell_id = ir.model.shells[0].id.as_str().to_owned();
    ir.model.shells[0].region = "test:model:region#missing".try_into().expect("identity");
    let face_id = ir.model.faces[0].id.as_str().to_owned();
    ir.model.faces[0].shell = "test:model:shell#missing".try_into().expect("identity");
    let (admitted, losses) = super::super::scopes::admit(&ir);
    let admitted = admitted.into_owned();
    assert!(
        admitted
            .model
            .regions
            .iter()
            .all(|row| row.id.as_str() != region_id),
        "the region with a dangling body is dropped"
    );
    assert!(
        admitted
            .model
            .shells
            .iter()
            .all(|row| row.id.as_str() != shell_id),
        "the shell with a dangling region is dropped"
    );
    assert!(
        admitted
            .model
            .faces
            .iter()
            .all(|row| row.id.as_str() != face_id),
        "the face with a dangling shell is dropped"
    );
    for id in [&region_id, &shell_id, &face_id] {
        assert!(
            losses.iter().any(|loss| loss.message.contains(id)),
            "{losses:?}"
        );
    }
}

#[test]
fn plan_names_the_invalidated_record_instead_of_refusing() {
    use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
    let mut ir = sheet();
    let region_id = ir.model.regions[0].id.as_str().to_owned();
    ir.model.regions[0].body = "test:model:body#missing".try_into().expect("identity");
    let plan = crate::RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(crate::RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .expect("an invalidated document plans by degrading the invalidated record");
    assert!(
        plan.report()
            .losses
            .iter()
            .any(|loss| loss.message.contains(&region_id)),
        "{:?}",
        plan.report().losses
    );
}

#[test]
fn admission_drops_appearance_bindings_with_dangling_targets() {
    use cadmpeg_ir::appearance::{AppearanceBinding, AppearanceTarget};
    let mut ir = sheet();
    let live: cadmpeg_ir::ids::PointId = ir.model.points[0].id.clone();
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "test:model:binding#live".try_into().expect("identity"),
        native_ref: None,
        target: AppearanceTarget::Point(live),
        appearance: "test:model:appearance#live".try_into().expect("identity"),
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: Default::default(),
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "test:model:binding#dangling".try_into().expect("identity"),
        native_ref: None,
        target: AppearanceTarget::Point("test:model:point#missing".try_into().expect("identity")),
        appearance: "test:model:appearance#live".try_into().expect("identity"),
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: Default::default(),
    });
    let (admitted, losses) = super::super::scopes::admit(&ir);
    let admitted = admitted.into_owned();
    assert!(
        admitted
            .model
            .appearance_bindings
            .iter()
            .any(|row| row.id.as_str() == "test:model:binding#live"),
        "the binding with a held target stays"
    );
    assert!(
        admitted
            .model
            .appearance_bindings
            .iter()
            .all(|row| row.id.as_str() != "test:model:binding#dangling"),
        "the binding with a dangling target is dropped"
    );
    assert!(
        losses
            .iter()
            .any(|loss| loss.message.contains("test:model:binding#dangling")),
        "{losses:?}"
    );
}
