// SPDX-License-Identifier: Apache-2.0
//! Physical selection checks for triangles with source endpoint topology.

use cadmpeg_ir::ids::{EdgeId, FaceId, VertexId};
use cadmpeg_ir::topology::{Edge, LoopBoundaryRole, Sense};
use cadmpeg_ir::CadIr;

fn position(ir: &CadIr, id: &VertexId) -> [f64; 3] {
    let vertex = ir
        .model
        .vertices
        .iter()
        .find(|vertex| &vertex.id == id)
        .unwrap();
    let point = ir
        .model
        .points
        .iter()
        .find(|point| point.id == vertex.point)
        .unwrap();
    let point = point.position().get();
    [point.x, point.y, point.z]
}

fn ordered_points(mut points: Vec<[f64; 3]>) -> Vec<[f64; 3]> {
    assert!(points
        .iter()
        .flatten()
        .all(|coordinate| coordinate.is_finite()));
    points.sort_by(|left, right| {
        left[0]
            .total_cmp(&right[0])
            .then(left[1].total_cmp(&right[1]))
            .then(left[2].total_cmp(&right[2]))
    });
    points
}

fn endpoints(ir: &CadIr, edge: &Edge, synthesized: bool) -> Vec<[f64; 3]> {
    if synthesized {
        let curve = ir
            .model
            .curves
            .iter()
            .find(|curve| Some(&curve.id) == edge.curve())
            .expect("synthesis emits a straight carrier");
        assert!(matches!(
            curve.geometry,
            cadmpeg_ir::geometry::CurveGeometry::Solved(
                cadmpeg_ir::geometry::SolvedCurveGeometry::Line(_)
            )
        ));
    } else {
        // triangle_body authors edge_use(_, 0); its source states endpoint
        // topology and does not state a geometric curve carrier.
        assert!(
            edge.curve().is_none(),
            "the unchanged source has no edge carrier"
        );
    }
    ordered_points(vec![position(ir, &edge.start), position(ir, &edge.end)])
}

pub(super) fn topology_edge(
    before: &CadIr,
    after: &CadIr,
    id: &EdgeId,
    synthesized: bool,
) -> EdgeId {
    let original = before
        .model
        .edges
        .iter()
        .find(|edge| &edge.id == id)
        .unwrap();
    let expected = endpoints(before, original, false);
    let candidates = after
        .model
        .edges
        .iter()
        .filter(|edge| endpoints(after, edge, synthesized) == expected)
        .collect::<Vec<_>>();
    assert_eq!(
        candidates.len(),
        1,
        "one edge occupies the selected segment"
    );
    candidates[0].id.clone()
}

fn triangle(
    ir: &CadIr,
    id: &FaceId,
    synthesized: bool,
) -> (Vec<[f64; 3]>, LoopBoundaryRole, [f64; 3]) {
    let face = ir.model.faces.iter().find(|face| &face.id == id).unwrap();
    let loops = face.loops.iter().collect::<Vec<_>>();
    assert_eq!(loops.len(), 1);
    let boundary = ir
        .model
        .loops
        .iter()
        .find(|boundary| &boundary.id == loops[0])
        .unwrap();
    assert_eq!(boundary.coedges().len(), 3);
    let points = boundary
        .coedges()
        .iter()
        .map(|id| {
            let coedge = ir
                .model
                .coedges
                .iter()
                .find(|coedge| &coedge.id == id)
                .unwrap();
            let edge = ir
                .model
                .edges
                .iter()
                .find(|edge| edge.id == coedge.edge)
                .unwrap();
            let _ = endpoints(ir, edge, synthesized);
            position(
                ir,
                match coedge.sense {
                    Sense::Forward => &edge.start,
                    Sense::Reversed => &edge.end,
                },
            )
        })
        .collect();
    let surface = ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == face.surface)
        .unwrap();
    let cadmpeg_ir::geometry::SurfaceGeometry::Solved(
        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Plane(plane),
    ) = &surface.geometry
    else {
        panic!("the fixture has a planar triangle");
    };
    let normal = plane.frame().axis().as_raw();
    let sign = if face.sense == Sense::Forward {
        1.0
    } else {
        -1.0
    };
    (
        ordered_points(points),
        face.loop_role(loops[0]),
        [sign * normal.x, sign * normal.y, sign * normal.z],
    )
}

pub(super) fn triangle_face(
    before: &CadIr,
    after: &CadIr,
    id: &FaceId,
    synthesized: bool,
) -> FaceId {
    let expected = triangle(before, id, false);
    let candidates = after
        .model
        .faces
        .iter()
        .filter(|face| triangle(after, &face.id, synthesized) == expected)
        .collect::<Vec<_>>();
    assert_eq!(
        candidates.len(),
        1,
        "one oriented face occupies the selected triangle"
    );
    candidates[0].id.clone()
}
