// SPDX-License-Identifier: Apache-2.0
//! Planar polyhedral solid bodies built from rings of corner indices.

use std::collections::BTreeMap;

use cadmpeg_ir::examples::ExampleError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::analytic::{LineCurve, PlaneSurface};
use cadmpeg_ir::geometry::{
    derive_reference_direction, Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry,
    Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, RegionId, ShellId, SurfaceId,
    VertexId,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, EdgeCarrier, Face, FaceLoops, Loop, LoopBoundary, LoopRing,
    Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::CadIr;

macro_rules! polyhedron_id {
    ($type:ident, $kind:literal, $key:expr) => {
        $type::compose(
            &cadmpeg_ir::identity_namespace!("synthetic", "polyhedron", $kind),
            $key,
        )
    };
}

/// One solid body whose planar faces are rings of corner indices, each
/// counterclockwise about the outward normal.
///
/// A corner pair that faces share is one edge running from the lower to the
/// higher corner index. The coedges of an edge form one radial ring in face
/// order, so an edge that four faces use is a non-manifold edge.
pub fn polyhedron(corners: &[[f64; 3]], faces: &[&[usize]]) -> Result<CadIr, ExampleError> {
    let corner = |index: usize| {
        corners
            .get(index)
            .map(|[x, y, z]| Point3::new(*x, *y, *z))
            .ok_or(ExampleError::Geometry("polyhedron corner is out of range"))
    };
    let mut ir = CadIr::empty();
    for index in 0..corners.len() {
        let position = FinitePoint3::new(corner(index)?)
            .ok_or(ExampleError::Geometry(Point::NON_FINITE_POSITION))?;
        ir.model.points.push(Point::new(
            polyhedron_id!(PointId, "point", index),
            position,
            None,
        ));
        ir.model.vertices.push(Vertex {
            id: polyhedron_id!(VertexId, "vertex", index),
            point: polyhedron_id!(PointId, "point", index),
            tolerance: None,
        });
    }

    let shell = polyhedron_id!(ShellId, "shell", 0_usize);
    let mut edges = BTreeMap::<(usize, usize), usize>::new();
    let mut edge_uses = Vec::<Vec<usize>>::new();
    for (face_index, ring) in faces.iter().enumerate() {
        let loop_id = polyhedron_id!(LoopId, "loop", face_index);
        let mut coedges = Vec::with_capacity(ring.len());
        let mut normal = [0.0; 3];
        for (from, to) in ring.iter().zip(ring.iter().cycle().skip(1)) {
            let (start, end) = (corner(*from)?, corner(*to)?);
            normal[0] += (start.y - end.y) * (start.z + end.z);
            normal[1] += (start.z - end.z) * (start.x + end.x);
            normal[2] += (start.x - end.x) * (start.y + end.y);
            let key = (*from.min(to), *from.max(to));
            let edge_index = match edges.get(&key) {
                Some(edge_index) => *edge_index,
                None => {
                    let edge_index = edge_uses.len();
                    push_edge(&mut ir, edge_index, key, corner(key.0)?, corner(key.1)?)?;
                    edges.insert(key, edge_index);
                    edge_uses.push(Vec::new());
                    edge_index
                }
            };
            let coedge_index = ir.model.coedges.len();
            let id = polyhedron_id!(CoedgeId, "coedge", coedge_index);
            ir.model.coedges.push(Coedge {
                id: id.clone(),
                owner_loop: loop_id.clone(),
                edge: polyhedron_id!(EdgeId, "edge", edge_index),
                radial_next: id.clone(),
                sense: if from < to {
                    Sense::Forward
                } else {
                    Sense::Reversed
                },
                pcurves: Vec::new(),
                use_curve: None,
            });
            if let Some(uses) = edge_uses.get_mut(edge_index) {
                uses.push(coedge_index);
            }
            coedges.push(id);
        }
        let length = normal.iter().map(|lane| lane * lane).sum::<f64>().sqrt();
        let normal = Vector3::new(normal[0] / length, normal[1] / length, normal[2] / length);
        let origin = corner(
            *ring
                .first()
                .ok_or(ExampleError::Geometry("polyhedron face has no corners"))?,
        )?;
        let surface = polyhedron_id!(SurfaceId, "surface", face_index);
        ir.model.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                PlaneSurface::try_new(origin, normal, derive_reference_direction(normal))
                    .map_err(ExampleError::Geometry)?,
            )),
            source_object: None,
        });
        ir.model.loops.push(Loop {
            id: loop_id.clone(),
            face: polyhedron_id!(FaceId, "face", face_index),
            boundary: LoopBoundary::Ring(LoopRing::new(coedges, Vec::new())?),
        });
        ir.model.faces.push(Face {
            id: polyhedron_id!(FaceId, "face", face_index),
            shell: shell.clone(),
            surface,
            sense: Sense::Forward,
            loops: FaceLoops::unspecified(vec![loop_id]),
            name: None,
            color: None,
            tolerance: None,
        });
    }
    for uses in &edge_uses {
        for (this, next) in uses.iter().zip(uses.iter().cycle().skip(1)) {
            let next = polyhedron_id!(CoedgeId, "coedge", *next);
            if let Some(coedge) = ir.model.coedges.get_mut(*this) {
                coedge.radial_next = next;
            }
        }
    }

    let region = polyhedron_id!(RegionId, "region", 0_usize);
    let body = polyhedron_id!(BodyId, "body", 0_usize);
    ir.model.shells.push(Shell::new(
        shell,
        region.clone(),
        (0..faces.len())
            .map(|face_index| polyhedron_id!(FaceId, "face", face_index))
            .collect(),
        Vec::new(),
        Vec::new(),
    )?);
    ir.model.regions.push(Region {
        id: region.clone(),
        body: body.clone(),
        shells: vec![polyhedron_id!(ShellId, "shell", 0_usize)],
    });
    ir.model.bodies.push(Body {
        id: body,
        kind: BodyKind::Solid,
        regions: vec![region],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir.finalize();
    Ok(ir)
}

fn push_edge(
    ir: &mut CadIr,
    edge_index: usize,
    (from, to): (usize, usize),
    start: Point3,
    end: Point3,
) -> Result<(), ExampleError> {
    let (dx, dy, dz) = (end.x - start.x, end.y - start.y, end.z - start.z);
    let length = (dx * dx + dy * dy + dz * dz).sqrt();
    let curve = polyhedron_id!(CurveId, "curve", edge_index);
    ir.model.curves.push(Curve {
        parameter_range: None,
        id: curve.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            LineCurve::try_new(start, Vector3::new(dx / length, dy / length, dz / length))
                .map_err(ExampleError::Geometry)?,
        )),
        source_object: None,
    });
    ir.model.edges.push(Edge {
        id: polyhedron_id!(EdgeId, "edge", edge_index),
        carrier: EdgeCarrier::new(Some(curve), Some([0.0, length]))
            .map_err(ExampleError::Geometry)?,
        start: polyhedron_id!(VertexId, "vertex", from),
        end: polyhedron_id!(VertexId, "vertex", to),
        tolerance: None,
    });
    Ok(())
}

/// Corner rings of the bottom, top, front, left, back and right faces of an
/// axis-aligned box. The eight corner indices run counterclockwise about `+z`
/// from the low `x` and `y` corner, first at the low `z`, then at the high `z`.
pub fn box_faces(corners: [usize; 8]) -> [[usize; 4]; 6] {
    let [b0, b1, b2, b3, b4, b5, b6, b7] = corners;
    [
        [b0, b3, b2, b1],
        [b4, b5, b6, b7],
        [b0, b1, b5, b4],
        [b3, b0, b4, b7],
        [b2, b3, b7, b6],
        [b1, b2, b6, b5],
    ]
}

/// One solid body of two `10 mm` cubes that touch along the line
/// `x = y = 10`, `0 <= z <= 10`, which is one edge with four coedges.
///
/// Around that edge the radial ring runs counterclockwise about `+z`: the
/// back and right faces of the cube at the origin, then the front and left
/// faces of the cube at `(10, 10, 0)`. Senses alternate, reversed first.
pub fn edge_touching_cubes() -> Result<CadIr, ExampleError> {
    let mut corners = Vec::new();
    for [x, y] in [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]] {
        corners.push([x, y, 0.0]);
    }
    for [x, y] in [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]] {
        corners.push([x, y, 10.0]);
    }
    for z in [0.0, 10.0] {
        for [x, y] in [[20.0, 10.0], [20.0, 20.0], [10.0, 20.0]] {
            corners.push([x, y, z]);
        }
    }
    let low = box_faces([0, 1, 2, 3, 4, 5, 6, 7]);
    let high = box_faces([2, 8, 9, 10, 6, 11, 12, 13]);
    let faces = low
        .iter()
        .chain(&high)
        .map(<[usize; 4]>::as_slice)
        .collect::<Vec<_>>();
    polyhedron(&corners, &faces)
}
