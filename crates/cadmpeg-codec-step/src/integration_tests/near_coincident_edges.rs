// SPDX-License-Identifier: Apache-2.0
//! Open edge subdivision at the STEP representation uncertainty.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{analytic::CircleCurve, Curve, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::PositiveLength;
use cadmpeg_ir::topology::{BodyKind, EdgeCarrier, Point, Shell};
use cadmpeg_ir::CadIr;

use crate::export::write_step;
use crate::{StepCodec, StepSchema, StepWriteOptions};

const UNCERTAINTY: f64 = 1.0e-5;
const RADIUS: f64 = 2.0;

fn arc_wire(gap: f64, closed: bool) -> CadIr {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube");
    let mut edge = ir.model.edges[0].clone();
    let mut vertices = ir.model.vertices[..2].to_vec();
    let curve_id = edge.curve().expect("curve").clone();
    let last = std::f64::consts::TAU - gap / RADIUS;
    let positions = [
        Point3::new(RADIUS, 0.0, 0.0),
        Point3::new(RADIUS * last.cos(), RADIUS * last.sin(), 0.0),
    ];
    let points = vertices
        .iter()
        .zip(positions)
        .map(|(vertex, position)| {
            Point::new(
                vertex.point.clone(),
                FinitePoint3::new(position).expect("position"),
                None,
            )
        })
        .collect();
    edge.start = vertices[0].id.clone();
    edge.end = vertices[usize::from(!closed)].id.clone();
    edge.carrier = EdgeCarrier::new(Some(curve_id.clone()), Some([0.0, last])).expect("range");
    if closed {
        vertices.truncate(1);
    }
    ir.model.points = points;
    ir.model.vertices = vertices;
    ir.model.curves = vec![Curve {
        id: curve_id,
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            CircleCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                RADIUS,
            )
            .expect("circle"),
        )),
        parameter_range: None,
        source_object: None,
    }];
    ir.model.shells = vec![Shell::with_wire_edge(
        ir.model.shells[0].id.clone(),
        ir.model.regions[0].id.clone(),
        edge.id.clone(),
    )];
    ir.model.edges = vec![edge];
    ir.model.faces.clear();
    ir.model.surfaces.clear();
    ir.model.loops.clear();
    ir.model.coedges.clear();
    ir.model.bodies[0].kind = BodyKind::Wire;
    ir.tolerances.linear = PositiveLength::new(UNCERTAINTY).expect("uncertainty");
    ir
}

fn write(ir: &CadIr) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_step(
        ir,
        &mut bytes,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write");
    bytes
}

fn edge_count(bytes: &[u8]) -> usize {
    std::str::from_utf8(bytes)
        .expect("Part 21")
        .matches(" = EDGE_CURVE(")
        .count()
}

#[test]
fn near_full_arc_below_uncertainty_writes_two_edges_and_reads_back_open() {
    let ir = arc_wire(UNCERTAINTY * 0.5, false);
    let bytes = write(&ir);
    assert_eq!(edge_count(&bytes), 2);
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("read back");
    assert_eq!(decoded.ir().model.edges.len(), 2);
    assert_eq!(decoded.ir().model.vertices.len(), 3);
    assert!(decoded
        .ir()
        .model
        .edges
        .iter()
        .all(|edge| edge.start != edge.end));
    for expected in &ir.model.points {
        assert!(decoded
            .ir()
            .model
            .points
            .iter()
            .any(|point| point.position() == expected.position()));
    }
}

#[test]
fn near_full_arc_above_uncertainty_writes_one_edge() {
    assert_eq!(edge_count(&write(&arc_wire(UNCERTAINTY * 2.0, false))), 1);
}

#[test]
fn closed_ring_keeps_one_edge_and_one_vertex() {
    let bytes = write(&arc_wire(0.0, true));
    assert_eq!(edge_count(&bytes), 1);
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("read back");
    assert_eq!(decoded.ir().model.edges.len(), 1);
    assert_eq!(decoded.ir().model.vertices.len(), 1);
    assert_eq!(
        decoded.ir().model.edges[0].start,
        decoded.ir().model.edges[0].end
    );
}

#[test]
fn short_line_at_uncertainty_is_split_with_and_without_an_edge_range() {
    for bounded in [false, true] {
        let mut ir = arc_wire(UNCERTAINTY * 0.5, false);
        let length = 1.0 / 1024.0;
        ir.tolerances.linear = PositiveLength::new(length).expect("uncertainty");
        for (point, x) in ir.model.points.iter_mut().zip([0.0, length]) {
            *point = Point::new(
                point.id.clone(),
                FinitePoint3::new(Point3::new(x, 0.0, 0.0)).expect("point"),
                None,
            );
        }
        ir.model.curves[0].geometry = CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("line"),
        ));
        ir.model.edges[0].carrier = EdgeCarrier::new(
            ir.model.edges[0].curve().cloned(),
            bounded.then_some([0.0, length]),
        )
        .expect("carrier");
        assert_eq!(edge_count(&write(&ir)), 2);
    }
}

#[test]
fn near_closed_nurbs_uses_the_same_split_rule() {
    for bounded in [false, true] {
        let mut ir = arc_wire(UNCERTAINTY * 0.5, false);
        let endpoints = ir
            .model
            .points
            .iter()
            .map(|point| point.position().get())
            .collect::<Vec<_>>();
        ir.model.curves[0].geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
                2,
                vec![4.0, 4.0, 4.0, 6.0, 6.0, 6.0],
                vec![endpoints[0], Point3::new(-2.0, 3.0, 0.0), endpoints[1]],
                None,
                false,
            )
            .expect("NURBS"),
        ));
        ir.model.edges[0].carrier = EdgeCarrier::new(
            ir.model.edges[0].curve().cloned(),
            bounded.then_some([4.0, 6.0]),
        )
        .expect("carrier");
        let bytes = write(&ir);
        assert_eq!(edge_count(&bytes), 2);
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("read back");
        assert_eq!(decoded.ir().model.edges.len(), 2);
        assert_eq!(decoded.ir().model.vertices.len(), 3);
    }
}

#[test]
fn split_face_edges_keep_shared_vertices_and_reversed_coedge_order() {
    use crate::test_support::revolved_wedge::{revolved_wedge, Piece};
    let ir = revolved_wedge(
        [2.0, 0.0],
        &[
            Piece::Line { to: [3.0, 0.0] },
            Piece::Line { to: [3.0, 1.0] },
            Piece::Line { to: [2.0, 1.0] },
            Piece::Line { to: [2.0, 0.0] },
        ],
        [0.0, std::f64::consts::TAU - 1.0e-9],
    );
    let bytes = write(&ir);
    assert_eq!(edge_count(&bytes), 16);
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("read back");
    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 6);
    assert_eq!(decoded.ir().model.edges.len(), 16);
    assert_eq!(decoded.ir().model.vertices.len(), 12);
    assert_eq!(decoded.ir().model.coedges.len(), 32);
    assert!(decoded
        .ir()
        .model
        .edges
        .iter()
        .all(|edge| edge.start != edge.end));
}

#[test]
fn unbounded_edge_parameter_inference_uses_the_model_tolerance() {
    let mut ir = arc_wire(UNCERTAINTY * 0.5, false);
    for (point, x) in ir.model.points.iter_mut().zip([0.0, UNCERTAINTY * 0.5]) {
        *point = Point::new(
            point.id.clone(),
            FinitePoint3::new(Point3::new(x, UNCERTAINTY * 0.25, 0.0)).expect("point"),
            None,
        );
    }
    ir.model.curves[0].geometry = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::analytic::LineCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("line"),
    ));
    ir.model.edges[0].carrier = EdgeCarrier::unbounded(ir.model.edges[0].curve().cloned());
    assert_eq!(edge_count(&write(&ir)), 2);
}

#[test]
fn a_missing_curve_keeps_its_existing_omission_loss() {
    let mut ir = arc_wire(UNCERTAINTY * 0.5, false);
    ir.model.curves.clear();
    let mut bytes = Vec::new();
    let report = write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write available geometry");
    assert_eq!(edge_count(&bytes), 0);
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::StepLossCode::EdgeCurveNotWritable.kind()));
}

#[test]
fn an_unreferenced_edge_preserves_independent_geometry() {
    let mut ir = arc_wire(0.0, false);
    ir.model.edges[0].carrier = EdgeCarrier::unbounded(ir.model.edges[0].curve().cloned());
    ir.model.bodies.clear();
    ir.model.regions.clear();
    ir.model.shells.clear();
    let mut independent = ir.model.curves[0].clone();
    independent.id =
        cadmpeg_ir::ids::CurveId::mint("test:model:curve#independent").expect("curve id");
    ir.model.curves.push(independent);
    ir.model.curves[0].geometry = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::analytic::LineCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("line"),
    ));
    ir.model.points[1] = Point::new(
        ir.model.points[1].id.clone(),
        ir.model.points[0].position(),
        None,
    );
    let bytes = write(&ir);
    assert_eq!(edge_count(&bytes), 0);
    assert_eq!(
        std::str::from_utf8(&bytes)
            .expect("Part 21")
            .matches(" = CIRCLE(")
            .count(),
        1
    );
}

#[test]
fn an_unbounded_near_full_arc_uses_endpoint_parameters() {
    for gap in [0.0, UNCERTAINTY * 0.5] {
        let mut ir = arc_wire(gap, false);
        ir.model.edges[0].carrier = EdgeCarrier::unbounded(ir.model.edges[0].curve().cloned());
        let bytes = write(&ir);
        assert_eq!(edge_count(&bytes), 2);
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("read back");
        assert_eq!(decoded.ir().model.edges.len(), 2);
        assert_eq!(decoded.ir().model.vertices.len(), 3);
    }
}

#[test]
fn an_unbounded_replica_uses_the_source_period() {
    use cadmpeg_ir::geometry::{ProceduralCurve, ProceduralCurveDefinition};
    use cadmpeg_ir::ids::{CurveId, ProceduralCurveId};
    let mut ir = arc_wire(UNCERTAINTY * 0.5, false);
    let mut source = ir.model.curves[0].clone();
    source.id = CurveId::mint("test:model:curve#replica-source").expect("curve id");
    let construction =
        ProceduralCurveId::mint("test:model:procedural_curve#replica").expect("construction id");
    ir.model.curves[0].geometry = CurveGeometry::Procedural {
        construction: construction.clone(),
        cache: source.geometry.solved().cloned(),
    };
    ir.model.procedural_curves.push(ProceduralCurve::new(
        construction,
        ProceduralCurveDefinition::Replica {
            source: source.id.clone(),
            transform: cadmpeg_ir::transform::Transform::identity(),
        },
    ));
    ir.model.curves.push(source);
    ir.model.edges[0].carrier = EdgeCarrier::unbounded(ir.model.edges[0].curve().cloned());
    let bytes = write(&ir);
    assert_eq!(edge_count(&bytes), 2);
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("read back");
    assert_eq!(decoded.ir().model.edges.len(), 2);
    assert_eq!(decoded.ir().model.vertices.len(), 3);
}

#[test]
fn an_unbounded_polyline_uses_its_sample_parameter_bounds() {
    use cadmpeg_ir::geometry::sampled::{PolylineCurve, PolylineSamples, PolylineVertex};
    for parameterized in [false, true] {
        let mut ir = arc_wire(UNCERTAINTY * 0.5, false);
        let points = vec![
            ir.model.points[0].position().get(),
            Point3::new(-2.0, 3.0, 0.0),
            ir.model.points[1].position().get(),
        ];
        let samples = if parameterized {
            PolylineSamples::Parameterized {
                vertices: points
                    .into_iter()
                    .zip([4.0, 5.0, 6.0])
                    .map(|(point, parameter)| PolylineVertex { point, parameter })
                    .collect::<Vec<_>>()
                    .try_into()
                    .expect("samples"),
            }
        } else {
            PolylineSamples::Unparameterized {
                points: points.try_into().expect("points"),
            }
        };
        ir.model.curves[0].geometry = CurveGeometry::Solved(SolvedCurveGeometry::Polyline(
            PolylineCurve::new(samples, 0.0).expect("polyline"),
        ));
        ir.model.edges[0].carrier = EdgeCarrier::unbounded(ir.model.edges[0].curve().cloned());
        let bytes = write(&ir);
        assert_eq!(edge_count(&bytes), 2);
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("read back");
        assert_eq!(decoded.ir().model.edges.len(), 2);
        assert_eq!(decoded.ir().model.vertices.len(), 3);
    }
}

#[test]
fn a_short_arc_near_the_periodic_seam_keeps_its_parameter_midpoint() {
    let mut ir = arc_wire(UNCERTAINTY * 0.5, false);
    let range = [-3.0, -3.0 + UNCERTAINTY / (2.0 * RADIUS)];
    for (point, angle) in ir.model.points.iter_mut().zip(range) {
        *point = Point::new(
            point.id.clone(),
            FinitePoint3::new(Point3::new(RADIUS * angle.cos(), RADIUS * angle.sin(), 0.0))
                .expect("point"),
            None,
        );
    }
    ir.model.edges[0].carrier = EdgeCarrier::unbounded(ir.model.edges[0].curve().cloned());
    let bytes = write(&ir);
    assert_eq!(edge_count(&bytes), 2);
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("read back");
    let mut uses = std::collections::HashMap::new();
    for edge in &decoded.ir().model.edges {
        for vertex in [&edge.start, &edge.end] {
            *uses.entry(vertex).or_insert(0) += 1;
        }
    }
    let midpoint_id = uses
        .iter()
        .find_map(|(id, count)| (*count == 2).then_some(*id))
        .expect("shared midpoint");
    let vertex = decoded
        .ir()
        .model
        .vertices
        .iter()
        .find(|vertex| vertex.id == *midpoint_id)
        .expect("vertex");
    let point = decoded
        .ir()
        .model
        .points
        .iter()
        .find(|point| point.id == vertex.point)
        .expect("point");
    let angle = range[0] * 0.5 + range[1] * 0.5;
    let expected = Point3::new(RADIUS * angle.cos(), RADIUS * angle.sin(), 0.0);
    assert!(point.position().get().distance(expected) <= UNCERTAINTY);
}
