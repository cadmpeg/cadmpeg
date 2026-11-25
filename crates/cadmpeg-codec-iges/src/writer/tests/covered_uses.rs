// SPDX-License-Identifier: Apache-2.0
//! Edge uses that cover part of their parameter curve, on a trimmed face and
//! on a B-rep face: the writer writes the covered part of each curve.

use std::collections::BTreeSet;
use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::eval::decode::{pcurve_uv, surface_point_solved};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::geometry::pcurve::{
    LinePcurve, Pcurve, PcurveGeometry, PcurveMetadata, PcurveNurbs,
};
use cadmpeg_ir::geometry::procedural_curves::DirectedParameterRange;
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::ids::{PcurveId, VertexId};
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::{BodyKind, PcurveUse, Sense};
use cadmpeg_ir::units::FiniteVector;
use cadmpeg_ir::CadIr;

use crate::{IgesCodec, IgesVersion};

/// Bound on the distance between a read-back point and its expected place.
const ON_EDGE: f64 = 1.0e-9;

/// The parameter of the end of a covered B-spline curve; the use covers
/// `[1, 2]`.
const NURBS_END: f64 = 3.0;

/// The point at `t` on the line from `from` that reaches `to` at `t = 1`.
fn along(from: Point2, to: Point2, t: f64) -> Point2 {
    Point2::new(
        t.mul_add(to.u - from.u, from.u),
        t.mul_add(to.v - from.v, from.v),
    )
}

/// A parameter curve from `start` to `end` with its use, as coedge
/// `position` of [`cube_with_pcurves`] states it.
fn carrier(
    id: PcurveId,
    start: Point2,
    end: Point2,
    position: usize,
    covered: bool,
) -> (Pcurve, PcurveUse) {
    let length = (end.u - start.u).hypot(end.v - start.v);
    let unit = Point2::new((end.u - start.u) / length, (end.v - start.v) / length);
    let line = |origin: Point2, direction: Point2| {
        PcurveGeometry::Line(LinePcurve::try_new(origin, direction).expect("a unit direction"))
    };
    let nurbs = |from: Point2, to: Point2| {
        let points = [0.0, 0.5 * NURBS_END, NURBS_END]
            .map(|t| along(from, to, t - 1.0))
            .to_vec();
        let knots = vec![0.0, 0.0, 0.0, NURBS_END, NURBS_END, NURBS_END];
        PcurveGeometry::Nurbs {
            nurbs: PcurveNurbs::from_lanes(2, knots, points, None, false)
                .expect("a quadratic B-spline"),
        }
    };
    let (geometry, range, used) = match (covered, position % 4) {
        (false, _) => (line(start, unit), length, None),
        (true, 0) => (
            line(along(start, end, -1.0 / length), unit),
            length + 2.0,
            Some([1.0, 1.0 + length]),
        ),
        (true, 1) => (
            line(
                along(end, start, -1.0 / length),
                Point2::new(-unit.u, -unit.v),
            ),
            length + 2.0,
            Some([1.0 + length, 1.0]),
        ),
        (true, 2) => (nurbs(start, end), NURBS_END, Some([1.0, 2.0])),
        (true, _) => (nurbs(end, start), NURBS_END, Some([2.0, 1.0])),
    };
    (
        Pcurve {
            id: id.clone(),
            geometry,
            metadata: PcurveMetadata::general(None, FiniteVector::new([0.0, range]), None),
            native_ref: None,
        },
        PcurveUse {
            pcurve: id,
            isoparametric: None,
            parameter_range: used
                .map(|used| DirectedParameterRange::new(used).expect("a parameter range")),
        },
    )
}

/// The example cube with a parameter curve on each coedge, from the start
/// to the end of the coedge on the plane of its face. Where `covered`, each
/// curve reaches past both ends of its coedge and its use covers the middle;
/// the coedges take in turn a line, a line run backward, a quadratic B-spline
/// and a quadratic B-spline run backward.
pub(super) fn cube_with_pcurves(covered: bool) -> CadIr {
    let mut ir = unit_cube().expect("the example cube builds");
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
        .expect("an evaluation context");
    let geometry = &ir.geometry;
    let position = |vertex: &VertexId| {
        let point = &geometry
            .vertices
            .iter()
            .find(|record| record.id == *vertex)
            .expect("the edge vertex")
            .point;
        geometry
            .points
            .iter()
            .find(|record| record.id == *point)
            .expect("the vertex point")
            .position()
            .get()
    };
    let mut carriers = Vec::new();
    for (index, coedge) in geometry.coedges.iter().enumerate() {
        let face = geometry
            .loops
            .iter()
            .find(|loop_| loop_.id == coedge.owner_loop)
            .and_then(|loop_| geometry.faces.iter().find(|face| face.id == loop_.face))
            .expect("the coedge face");
        let SurfaceGeometry::Solved(plane) = &geometry
            .surfaces
            .iter()
            .find(|surface| surface.id == face.surface)
            .expect("the face support")
            .geometry
        else {
            panic!("the cube supports are solved planes");
        };
        let at = |u: f64, v: f64| {
            surface_point_solved(&ctx, plane, u, v)
                .expect("a point on the plane")
                .get()
        };
        let [origin, u_end, v_end] = [at(0.0, 0.0), at(1.0, 0.0), at(0.0, 1.0)];
        let uv = |point: Point3| {
            let dot = |axis: Point3| {
                (point.x - origin.x) * (axis.x - origin.x)
                    + (point.y - origin.y) * (axis.y - origin.y)
                    + (point.z - origin.z) * (axis.z - origin.z)
            };
            Point2::new(dot(u_end), dot(v_end))
        };
        let ends = geometry
            .edges
            .iter()
            .find(|edge| edge.id == coedge.edge)
            .and_then(|edge| edge.ends.as_ref())
            .expect("the coedge edge has two vertices");
        let [first, last] = [position(&ends.start), position(&ends.end)];
        let [start, end] = if coedge.sense == Sense::Forward {
            [first, last]
        } else {
            [last, first]
        };
        for corner in [start, end] {
            let image = uv(corner);
            let back = at(image.u, image.v);
            assert!(
                near(back, corner),
                "the plane maps {image:?} to {back:?}, off {corner:?}"
            );
        }
        let id =
            PcurveId::mint(format!("test:model:pcurve#cube-{index}")).expect("identity grammar");
        carriers.push(carrier(id, uv(start), uv(end), index, covered));
    }
    for (coedge, (pcurve, pcurve_use)) in ir.geometry.coedges.iter_mut().zip(carriers) {
        coedge.pcurves = vec![pcurve_use];
        ir.geometry.pcurves.push(pcurve);
    }
    ir.finalize();
    ir
}

/// `ir` reduced to a sheet of its first face.
fn first_face_sheet(mut ir: CadIr) -> CadIr {
    let geometry = &mut ir.geometry;
    geometry.bodies[0].kind = BodyKind::Sheet;
    let face = geometry.faces[0].clone();
    geometry.faces.truncate(1);
    let loops = face.loops.stated().cloned().collect::<BTreeSet<_>>();
    geometry.loops.retain(|loop_| loops.contains(&loop_.id));
    geometry
        .coedges
        .retain(|coedge| loops.contains(&coedge.owner_loop));
    for coedge in &mut geometry.coedges {
        coedge.radial_next = coedge.id.clone();
    }
    let edges = geometry
        .coedges
        .iter()
        .map(|coedge| coedge.edge.clone())
        .collect::<BTreeSet<_>>();
    geometry.edges.retain(|edge| edges.contains(&edge.id));
    let vertices = geometry
        .edges
        .iter()
        .flat_map(|edge| edge.vertices().cloned())
        .collect::<BTreeSet<_>>();
    geometry
        .vertices
        .retain(|vertex| vertices.contains(&vertex.id));
    let points = geometry
        .vertices
        .iter()
        .map(|vertex| vertex.point.clone())
        .collect::<BTreeSet<_>>();
    geometry.points.retain(|point| points.contains(&point.id));
    let curves = geometry
        .edges
        .iter()
        .filter_map(|edge| edge.curve().cloned())
        .collect::<BTreeSet<_>>();
    geometry.curves.retain(|curve| curves.contains(&curve.id));
    geometry
        .surfaces
        .retain(|surface| surface.id == face.surface);
    let pcurves = geometry
        .coedges
        .iter()
        .flat_map(|coedge| coedge.pcurves.iter().map(|use_| use_.pcurve.clone()))
        .collect::<BTreeSet<_>>();
    geometry
        .pcurves
        .retain(|pcurve| pcurves.contains(&pcurve.id));
    ir.finalize();
    ir
}

fn written(ir: &CadIr) -> (Vec<LossNote>, CadIr) {
    let plan = IgesCodec
        .plan(
            EncodeInput::new(ir, None).with_target(TargetRequest::Explicit(
                IgesVersion::V5_3.descriptor().id.as_str(),
            )),
        )
        .expect("the document is written");
    let losses = plan.report().losses.clone();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("the file is written");
    let read = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("the written file decodes")
        .ir()
        .clone();
    (losses, read)
}

/// Assert that `ir` is written with every face and parameter curve, and
/// that each read-back parameter curve runs between the vertices of its
/// edge and passes the middle of the edge at the middle of its range. Each
/// read-back quadratic B-spline spans the unit range of its use.
fn assert_covered_parts_written(ir: &CadIr) {
    let (losses, read) = written(ir);
    for code in [
        "iges/writer.body-omitted",
        "iges/writer.face-omitted",
        "iges/writer.pcurve-omitted",
    ] {
        assert!(
            !losses.iter().any(|loss| loss.code.to_string() == code),
            "{losses:#?}"
        );
    }
    let geometry = &read.geometry;
    assert_eq!(geometry.faces.len(), ir.geometry.faces.len());
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
        .expect("an evaluation context");
    let point = |vertex: &VertexId| {
        let point = &geometry
            .vertices
            .iter()
            .find(|record| record.id == *vertex)
            .expect("the edge vertex reads back")
            .point;
        geometry
            .points
            .iter()
            .find(|record| record.id == *point)
            .expect("the vertex point reads back")
            .position()
            .get()
    };
    let mut checked = 0;
    for coedge in &geometry.coedges {
        let face = geometry
            .loops
            .iter()
            .find(|loop_| loop_.id == coedge.owner_loop)
            .and_then(|loop_| geometry.faces.iter().find(|face| face.id == loop_.face))
            .expect("the coedge face reads back");
        let support = match &geometry
            .surfaces
            .iter()
            .find(|surface| surface.id == face.surface)
            .expect("the face support reads back")
            .geometry
        {
            SurfaceGeometry::Solved(solved) => solved,
            SurfaceGeometry::Procedural { cache, .. } => {
                cache.as_ref().expect("the support reads back solved")
            }
        };
        let ends = geometry
            .edges
            .iter()
            .find(|edge| edge.id == coedge.edge)
            .and_then(|edge| edge.ends.as_ref())
            .expect("the edge reads back with two vertices");
        let corners = [point(&ends.start), point(&ends.end)];
        for pcurve_use in &coedge.pcurves {
            let pcurve = geometry
                .pcurves
                .iter()
                .find(|pcurve| pcurve.id == pcurve_use.pcurve)
                .expect("the parameter curve reads back");
            let [start, end] = pcurve
                .parameter_range()
                .expect("a bounded parameter curve")
                .get();
            let [first, middle, last] = [start, 0.5 * (start + end), end].map(|parameter| {
                let uv = pcurve_uv(&ctx, &pcurve.geometry, parameter)
                    .expect("a parameter point")
                    .get();
                surface_point_solved(&ctx, support, uv.u, uv.v)
                    .expect("a point on the support")
                    .get()
            });
            let centre = Point3::new(
                0.5 * (corners[0].x + corners[1].x),
                0.5 * (corners[0].y + corners[1].y),
                0.5 * (corners[0].z + corners[1].z),
            );
            assert!(
                (near(first, corners[0]) && near(last, corners[1]))
                    || (near(first, corners[1]) && near(last, corners[0])),
                "{} runs from {first:?} to {last:?}, off the vertices {corners:?}",
                pcurve.id
            );
            assert!(
                near(middle, centre),
                "{} passes {middle:?} at the middle of its range, off {centre:?}",
                pcurve.id
            );
            if let PcurveGeometry::Nurbs { nurbs } = &pcurve.geometry {
                if nurbs.degree() == 2 {
                    assert!(
                        (end - start - 1.0).abs() <= ON_EDGE,
                        "{} spans [{start}, {end}]",
                        pcurve.id
                    );
                }
            }
            checked += 1;
        }
    }
    assert_eq!(checked, ir.geometry.coedges.len());
}

fn near(first: Point3, second: Point3) -> bool {
    (first.x - second.x).abs() <= ON_EDGE
        && (first.y - second.y).abs() <= ON_EDGE
        && (first.z - second.z).abs() <= ON_EDGE
}

#[test]
fn a_trimmed_face_writes_the_covered_part_of_each_edge_use() {
    assert_covered_parts_written(&first_face_sheet(cube_with_pcurves(true)));
}

#[test]
fn a_brep_face_writes_the_covered_part_of_each_edge_use() {
    assert_covered_parts_written(&cube_with_pcurves(true));
}
