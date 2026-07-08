// SPDX-License-Identifier: Apache-2.0
//! Hand-built IR fixtures.
//!
//! [`unit_cube`] constructs a topologically complete, validation-clean cube: 8
//! vertices, 12 edges, 6 planar faces, 24 coedges (each edge shared by exactly
//! two coedges of opposite sense). It is the worked example serialized in
//! `docs/cad-ir.md` and the anchor for the IR round-trip and validation tests.

use std::collections::HashMap;

use crate::document::CadIr;
use crate::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
use crate::ids::{CoedgeId, CurveId, EdgeId, PointId, SurfaceId, VertexId};
use crate::math::{Point3, Vector3};
use crate::provenance::EntityMeta;
use crate::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, Lump, Point, Sense, Shell, Vertex,
};
use crate::units::Units;

/// One face definition for [`unit_cube`]: name, outward normal, surface origin,
/// and the boundary loop as an ordered ring of `(edge_index, forward)` pairs.
type FaceDef = (
    &'static str,
    (f64, f64, f64),
    (f64, f64, f64),
    [(usize, bool); 4],
);

/// A `10 mm` axis-aligned cube spanning the origin to `(10, 10, 10)`.
pub fn unit_cube() -> CadIr {
    let s = 10.0_f64;
    let meta = EntityMeta::synthetic;

    let corners = [
        (0.0, 0.0, 0.0),
        (s, 0.0, 0.0),
        (s, s, 0.0),
        (0.0, s, 0.0),
        (0.0, 0.0, s),
        (s, 0.0, s),
        (s, s, s),
        (0.0, s, s),
    ];

    // (from_corner, to_corner) for each of the 12 edges.
    let edge_defs = [
        (0, 1),
        (1, 2),
        (2, 3),
        (3, 0),
        (4, 5),
        (5, 6),
        (6, 7),
        (7, 4),
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7),
    ];

    // Each face: name, outward normal, surface origin, and its loop as an
    // ordered ring of (edge_index, forward) pairs. `forward` means the coedge
    // traverses the edge in the edge's own start→end direction. Every edge
    // appears exactly twice across all faces, once in each direction, so the
    // two coedges of an edge always have opposite sense.
    let face_defs: [FaceDef; 6] = [
        (
            "bottom",
            (0.0, 0.0, -1.0),
            (0.0, 0.0, 0.0),
            [(0, true), (1, true), (2, true), (3, true)],
        ),
        (
            "top",
            (0.0, 0.0, 1.0),
            (0.0, 0.0, s),
            [(7, false), (6, false), (5, false), (4, false)],
        ),
        (
            "front",
            (0.0, -1.0, 0.0),
            (0.0, 0.0, 0.0),
            [(0, false), (8, true), (4, true), (9, false)],
        ),
        (
            "right",
            (1.0, 0.0, 0.0),
            (s, 0.0, 0.0),
            [(1, false), (9, true), (5, true), (10, false)],
        ),
        (
            "back",
            (0.0, 1.0, 0.0),
            (0.0, s, 0.0),
            [(2, false), (10, true), (6, true), (11, false)],
        ),
        (
            "left",
            (-1.0, 0.0, 0.0),
            (0.0, 0.0, 0.0),
            [(3, false), (11, true), (7, true), (8, false)],
        ),
    ];

    let mut ir = CadIr::empty(Units::default());

    // Points + vertices.
    for (i, (x, y, z)) in corners.iter().enumerate() {
        ir.points.push(Point {
            id: PointId(format!("p{i}")),
            position: Point3::new(*x, *y, *z),
            meta: meta(),
        });
        ir.vertices.push(Vertex {
            id: VertexId(format!("v{i}")),
            point: PointId(format!("p{i}")),
            meta: meta(),
        });
    }

    // Edges + their line curves.
    for (i, (a, b)) in edge_defs.iter().enumerate() {
        let (ax, ay, az) = corners[*a];
        let (bx, by, bz) = corners[*b];
        let dir = Vector3::new(bx - ax, by - ay, bz - az);
        let len = dir.norm();
        let unit = Vector3::new(dir.x / len, dir.y / len, dir.z / len);
        ir.curves.push(Curve {
            id: CurveId(format!("crv_e{i}")),
            geometry: CurveGeometry::Line {
                origin: Point3::new(ax, ay, az),
                direction: unit,
            },
            meta: meta(),
        });
        ir.edges.push(Edge {
            id: EdgeId(format!("e{i}")),
            curve: Some(CurveId(format!("crv_e{i}"))),
            start: VertexId(format!("v{a}")),
            end: VertexId(format!("v{b}")),
            param_range: Some([0.0, len]),
            meta: meta(),
        });
    }

    // Faces, surfaces, loops, coedges.
    let mut edge_to_coedges: HashMap<usize, Vec<String>> = HashMap::new();
    for (name, normal, origin, ring) in &face_defs {
        let surf_id = format!("srf_{name}");
        ir.surfaces.push(Surface {
            id: SurfaceId(surf_id.clone()),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(origin.0, origin.1, origin.2),
                normal: Vector3::new(normal.0, normal.1, normal.2),
            },
            meta: meta(),
        });

        let loop_id = format!("lp_{name}");
        let coedge_ids: Vec<String> = (0..ring.len()).map(|i| format!("ce_{name}_{i}")).collect();

        for (i, (edge_index, forward)) in ring.iter().enumerate() {
            let next = &coedge_ids[(i + 1) % ring.len()];
            let prev = &coedge_ids[(i + ring.len() - 1) % ring.len()];
            ir.coedges.push(Coedge {
                id: CoedgeId(coedge_ids[i].clone()),
                owner_loop: loop_id.clone().into(),
                edge: EdgeId(format!("e{edge_index}")),
                next: CoedgeId(next.clone()),
                previous: CoedgeId(prev.clone()),
                partner: None, // filled in below
                sense: if *forward {
                    Sense::Forward
                } else {
                    Sense::Reversed
                },
                pcurve: None,
                meta: meta(),
            });
            edge_to_coedges
                .entry(*edge_index)
                .or_default()
                .push(coedge_ids[i].clone());
        }

        ir.loops.push(Loop {
            id: loop_id.clone().into(),
            face: format!("f_{name}").into(),
            coedges: coedge_ids.iter().map(|c| CoedgeId(c.clone())).collect(),
            meta: meta(),
        });
        ir.faces.push(Face {
            id: format!("f_{name}").into(),
            shell: "shell0".into(),
            surface: SurfaceId(surf_id),
            sense: Sense::Forward,
            loops: vec![loop_id.into()],
            name: Some(format!("{name} face")),
            color: None,
            meta: meta(),
        });
    }

    // Pair coedges: each edge has exactly two, which partner each other.
    let partner_of: HashMap<String, String> = edge_to_coedges
        .values()
        .filter(|v| v.len() == 2)
        .flat_map(|v| [(v[0].clone(), v[1].clone()), (v[1].clone(), v[0].clone())])
        .collect();
    for ce in &mut ir.coedges {
        if let Some(p) = partner_of.get(&ce.id.0) {
            ce.partner = Some(CoedgeId(p.clone()));
        }
    }

    // Shell, lump, body.
    ir.shells.push(Shell {
        id: "shell0".into(),
        lump: "lump0".into(),
        faces: face_defs
            .iter()
            .map(|(name, ..)| format!("f_{name}").into())
            .collect(),
        meta: meta(),
    });
    ir.lumps.push(Lump {
        id: "lump0".into(),
        body: "body0".into(),
        shells: vec!["shell0".into()],
        meta: meta(),
    });
    ir.bodies.push(Body {
        id: "body0".into(),
        kind: BodyKind::Solid,
        lumps: vec!["lump0".into()],
        transform: None,
        name: Some("unit cube".into()),
        color: None,
        meta: meta(),
    });

    ir
}
