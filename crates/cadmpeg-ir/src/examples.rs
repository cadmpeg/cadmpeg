// SPDX-License-Identifier: Apache-2.0
//! Hand-built documents for examples and tests.

use std::collections::HashMap;

use crate::document::CadIr;
use crate::geometry::{
    derive_reference_direction, Curve, CurveGeometry, ProceduralSurface,
    ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
};
use crate::ids::{
    CoedgeId, CurveId, EdgeId, PointId, ProceduralSurfaceId, SubdId, SurfaceId, VertexId,
};
use crate::math::{Point3, Vector3};
use crate::subd::{
    SubdEdge, SubdEdgeTag, SubdEdgeUse, SubdFace, SubdScheme, SubdSurface, SubdVertex,
    SubdVertexTag,
};
use crate::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
};
use crate::units::Units;

/// Face input used to construct [`unit_cube`].
type FaceDef = (
    &'static str,
    (f64, f64, f64),
    (f64, f64, f64),
    [(usize, bool); 4],
);

/// A `10 mm` axis-aligned cube spanning the origin to `(10, 10, 10)`.
pub fn unit_cube() -> CadIr {
    let s = 10.0_f64;

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
        ir.model.points.push(Point {
            id: PointId(format!("synthetic:cube:point#{i}")),
            position: Point3::new(*x, *y, *z),
            source_object: None,
        });
        ir.model.vertices.push(Vertex {
            id: VertexId(format!("synthetic:cube:vertex#{i}")),
            point: PointId(format!("synthetic:cube:point#{i}")),
            tolerance: None,
        });
    }

    // Edges + their line curves.
    for (i, (a, b)) in edge_defs.iter().enumerate() {
        let (ax, ay, az) = corners[*a];
        let (bx, by, bz) = corners[*b];
        let dir = Vector3::new(bx - ax, by - ay, bz - az);
        let len = dir.norm();
        let unit = Vector3::new(dir.x / len, dir.y / len, dir.z / len);
        ir.model.curves.push(Curve {
            id: CurveId(format!("synthetic:cube:curve#{i}")),
            geometry: CurveGeometry::Line {
                origin: Point3::new(ax, ay, az),
                direction: unit,
            },
            source_object: None,
        });
        ir.model.edges.push(Edge {
            id: EdgeId(format!("synthetic:cube:edge#{i}")),
            curve: Some(CurveId(format!("synthetic:cube:curve#{i}"))),
            start: VertexId(format!("synthetic:cube:vertex#{a}")),
            end: VertexId(format!("synthetic:cube:vertex#{b}")),
            param_range: Some([0.0, len]),
            tolerance: None,
        });
    }

    // Faces, surfaces, loops, coedges.
    let mut edge_to_coedges: HashMap<usize, Vec<String>> = HashMap::new();
    for (name, normal, origin, ring) in &face_defs {
        let surf_id = format!("synthetic:cube:surface#{name}");
        ir.model.surfaces.push(Surface {
            id: SurfaceId(surf_id.clone()),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(origin.0, origin.1, origin.2),
                normal: Vector3::new(normal.0, normal.1, normal.2),
                u_axis: derive_reference_direction(Vector3::new(normal.0, normal.1, normal.2)),
            },
            source_object: None,
        });

        let loop_id = format!("synthetic:cube:loop#{name}");
        let coedge_ids: Vec<String> = (0..ring.len())
            .map(|i| format!("synthetic:cube:coedge#{name}:{i}"))
            .collect();

        for (i, (edge_index, forward)) in ring.iter().enumerate() {
            let next = &coedge_ids[(i + 1) % ring.len()];
            let prev = &coedge_ids[(i + ring.len() - 1) % ring.len()];
            ir.model.coedges.push(Coedge {
                id: CoedgeId(coedge_ids[i].clone()),
                owner_loop: loop_id.clone().into(),
                edge: EdgeId(format!("synthetic:cube:edge#{edge_index}")),
                next: CoedgeId(next.clone()),
                previous: CoedgeId(prev.clone()),
                radial_next: CoedgeId(coedge_ids[i].clone()),
                sense: if *forward {
                    Sense::Forward
                } else {
                    Sense::Reversed
                },
                pcurves: Vec::new(),
                use_curve: None,
                use_curve_parameter_range: None,
            });
            edge_to_coedges
                .entry(*edge_index)
                .or_default()
                .push(coedge_ids[i].clone());
        }

        ir.model.loops.push(Loop {
            id: loop_id.clone().into(),
            face: format!("synthetic:cube:face#{name}").into(),
            boundary_role: crate::topology::LoopBoundaryRole::Outer,
            coedges: coedge_ids.iter().map(|c| CoedgeId(c.clone())).collect(),
            vertex_uses: Vec::new(),
        });
        ir.model.faces.push(Face {
            id: format!("synthetic:cube:face#{name}").into(),
            shell: "synthetic:cube:shell#0".into(),
            surface: SurfaceId(surf_id),
            sense: Sense::Forward,
            loops: vec![loop_id.into()],
            name: Some(format!("{name} face")),
            color: None,
            tolerance: None,
        });
    }

    // Pair coedges: each edge has exactly two, which partner each other.
    let partner_of: HashMap<String, String> = edge_to_coedges
        .values()
        .filter(|v| v.len() == 2)
        .flat_map(|v| [(v[0].clone(), v[1].clone()), (v[1].clone(), v[0].clone())])
        .collect();
    for ce in &mut ir.model.coedges {
        if let Some(p) = partner_of.get(&ce.id.0) {
            ce.radial_next = CoedgeId(p.clone());
        }
    }

    // Shell, region, body.
    ir.model.shells.push(Shell {
        id: "synthetic:cube:shell#0".into(),
        region: "synthetic:cube:region#0".into(),
        faces: face_defs
            .iter()
            .map(|(name, ..)| format!("synthetic:cube:face#{name}").into())
            .collect(),
        wire_edges: Vec::new(),
        free_vertices: Vec::new(),
    });
    ir.model.regions.push(Region {
        id: "synthetic:cube:region#0".into(),
        body: "synthetic:cube:body#0".into(),
        shells: vec!["synthetic:cube:shell#0".into()],
    });
    ir.model.bodies.push(Body {
        id: "synthetic:cube:body#0".into(),
        kind: BodyKind::Solid,
        regions: vec!["synthetic:cube:region#0".into()],
        transform: None,
        name: Some("unit cube".into()),
        color: None,
        visible: None,
    });

    ir.finalize();

    ir
}

/// A canonical fixture covering directed `SubD` and a Sum procedural surface.
pub fn directed_subd_sum() -> CadIr {
    let mut ir = CadIr::empty(Units::default());
    ir.model.curves = vec![
        Curve {
            id: CurveId("synthetic:v2:curve#u".into()),
            geometry: CurveGeometry::Line {
                origin: Point3::new(0.0, 0.0, 0.0),
                direction: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
        Curve {
            id: CurveId("synthetic:v2:curve#v".into()),
            geometry: CurveGeometry::Line {
                origin: Point3::new(0.0, 0.0, 0.0),
                direction: Vector3::new(0.0, 1.0, 0.0),
            },
            source_object: None,
        },
    ];
    ir.model.surfaces.push(Surface {
        id: SurfaceId("synthetic:v2:surface#sum-cache".into()),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(ProceduralSurface {
        id: ProceduralSurfaceId("synthetic:v2:procedural-surface#sum".into()),
        surface: SurfaceId("synthetic:v2:surface#sum-cache".into()),
        definition: ProceduralSurfaceDefinition::Sum {
            first: CurveId("synthetic:v2:curve#u".into()),
            second: CurveId("synthetic:v2:curve#v".into()),
            basepoint: Vector3::new(0.0, 0.0, 0.0),
            revision_form: None,
        },
        cache_fit_tolerance: Some(1.0e-9),
        record_bounds: None,
    });
    ir.model.subds.push(SubdSurface {
        id: SubdId("synthetic:v2:subd#directed".into()),
        scheme: SubdScheme::CatmullClark,
        vertices: vec![
            SubdVertex {
                point: Point3::new(0.0, 0.0, 0.0),
                tag: SubdVertexTag::Crease,
            },
            SubdVertex {
                point: Point3::new(1.0, 0.0, 0.0),
                tag: SubdVertexTag::Smooth,
            },
            SubdVertex {
                point: Point3::new(0.0, 1.0, 0.0),
                tag: SubdVertexTag::Corner,
            },
        ],
        edges: vec![
            SubdEdge {
                vertices: [0, 1],
                sharpness: [0.25, 0.75],
                tag: SubdEdgeTag::Crease,
                sector_coefficients: [0.125, 0.875],
            },
            SubdEdge {
                vertices: [1, 2],
                sharpness: [0.0, 0.5],
                tag: SubdEdgeTag::SmoothX,
                sector_coefficients: [0.25, 0.75],
            },
            SubdEdge {
                vertices: [2, 0],
                sharpness: [1.0, 0.0],
                tag: SubdEdgeTag::Smooth,
                sector_coefficients: [0.5, 0.5],
            },
        ],
        faces: vec![SubdFace {
            edges: vec![
                SubdEdgeUse {
                    edge: 0,
                    reversed: false,
                },
                SubdEdgeUse {
                    edge: 1,
                    reversed: false,
                },
                SubdEdgeUse {
                    edge: 2,
                    reversed: false,
                },
            ],
        }],
        source_object: None,
    });
    ir.finalize();
    ir
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directed_subd_sum_fixture_round_trips_validates_and_matches_schema_shape() {
        let ir = directed_subd_sum();
        let report = crate::validate::validate(&ir, Vec::new());
        assert!(report.is_ok(), "{:?}", report.findings);
        let json = ir.to_canonical_json().expect("serialize fixture");
        assert_eq!(CadIr::from_json(&json).expect("parse fixture"), ir);

        let schema = serde_json::to_value(crate::cadir_json_schema()).expect("serialize schema");
        let schema_text = schema.to_string();
        assert!(schema_text.contains("procedural_surfaces"));
        assert!(schema_text.contains("sharpness"));
        assert!(schema_text.contains("\"sum\""));
    }
}
