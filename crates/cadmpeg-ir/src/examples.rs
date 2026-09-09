// SPDX-License-Identifier: Apache-2.0
//! Hand-built documents for examples and tests.

use std::collections::HashMap;

use crate::document::CadIr;
use crate::geometry::{
    derive_reference_direction, Curve, CurveGeometry, ProceduralSurface,
    ProceduralSurfaceDefinition, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use crate::ids::{
    CoedgeId, CurveId, EdgeId, FaceId, PointId, ProceduralSurfaceId, SubdId, SurfaceId, VertexId,
};
use crate::math::{Point3, Vector3};
use crate::subd::{
    SubdEdge, SubdEdgeTag, SubdEdgeUse, SubdFace, SubdScheme, SubdSurface, SubdVertex,
    SubdVertexTag,
};
use crate::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
};

const EPS_EXAMPLES_DIRECTED_SUBD_SUM_E9: f64 = 1.0e-9;

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

    let mut ir = CadIr::empty();

    // Points + vertices.
    for (i, (x, y, z)) in corners.iter().enumerate() {
        ir.model.points.push(Point {
            id: PointId::mint(format!("synthetic:cube:point#{i}")).expect("valid identity"),
            position: Point3::new(*x, *y, *z),
            source_object: None,
        });
        ir.model.vertices.push(Vertex {
            id: VertexId::mint(format!("synthetic:cube:vertex#{i}")).expect("valid identity"),
            point: PointId::mint(format!("synthetic:cube:point#{i}")).expect("valid identity"),
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
            id: CurveId::mint(format!("synthetic:cube:curve#{i}")).expect("valid identity"),
            geometry: CurveGeometry::Line {
                origin: Point3::new(ax, ay, az),
                direction: unit,
            },
            source_object: None,
        });
        ir.model.edges.push(Edge {
            id: EdgeId::mint(format!("synthetic:cube:edge#{i}")).expect("valid identity"),
            curve: Some(
                CurveId::mint(format!("synthetic:cube:curve#{i}")).expect("valid identity"),
            ),
            start: VertexId::mint(format!("synthetic:cube:vertex#{a}")).expect("valid identity"),
            end: VertexId::mint(format!("synthetic:cube:vertex#{b}")).expect("valid identity"),
            param_range: Some([0.0, len]),
            tolerance: None,
        });
    }

    // Faces, surfaces, loops, coedges.
    let mut edge_to_coedges: HashMap<usize, Vec<String>> = HashMap::new();
    for (name, normal, origin, ring) in &face_defs {
        let surf_id = format!("synthetic:cube:surface#{name}");
        ir.model.surfaces.push(Surface {
            id: SurfaceId::mint(surf_id.clone()).expect("valid identity"),
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
            ir.model.coedges.push(Coedge {
                id: CoedgeId::mint(coedge_ids[i].clone()).expect("valid identity"),
                owner_loop: loop_id.clone().try_into().expect("valid identity"),
                edge: EdgeId::mint(format!("synthetic:cube:edge#{edge_index}"))
                    .expect("valid identity"),
                radial_next: CoedgeId::mint(coedge_ids[i].clone()).expect("valid identity"),
                sense: if *forward {
                    Sense::Forward
                } else {
                    Sense::Reversed
                },
                pcurves: Vec::new(),
                use_curve: None,
            });
            edge_to_coedges
                .entry(*edge_index)
                .or_default()
                .push(coedge_ids[i].clone());
        }

        ir.model.loops.push(Loop {
            id: loop_id.clone().try_into().expect("valid identity"),
            face: FaceId::mint(format!("synthetic:cube:face#{name}"))
                .expect("fixed namespace and face name"),
            boundary: crate::topology::LoopBoundary::Ring(
                crate::topology::LoopRing::new(
                    coedge_ids
                        .iter()
                        .map(|c| CoedgeId::mint(c.clone()).expect("valid identity"))
                        .collect(),
                    Vec::new(),
                )
                .expect("valid loop ring"),
            ),
        });
        ir.model.faces.push(Face {
            id: FaceId::mint(format!("synthetic:cube:face#{name}"))
                .expect("fixed namespace and face name"),
            shell: "synthetic:cube:shell#0".try_into().expect("valid identity"),
            surface: SurfaceId::mint(surf_id).expect("valid identity"),
            sense: Sense::Forward,
            loops: vec![loop_id.try_into().expect("valid identity")].into(),
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
        if let Some(p) = partner_of.get(ce.id.as_str()) {
            ce.radial_next = CoedgeId::mint(p.clone()).expect("valid identity");
        }
    }

    // Shell, region, body.
    ir.model.shells.push(
        Shell::new(
            "synthetic:cube:shell#0".try_into().expect("valid identity"),
            "synthetic:cube:region#0"
                .try_into()
                .expect("valid identity"),
            face_defs
                .iter()
                .map(|(name, ..)| {
                    FaceId::mint(format!("synthetic:cube:face#{name}"))
                        .expect("fixed namespace and face name")
                })
                .collect(),
            Vec::new(),
            Vec::new(),
        )
        .expect("unit cube shell owns six faces"),
    );
    ir.model.regions.push(Region {
        id: "synthetic:cube:region#0"
            .try_into()
            .expect("valid identity"),
        body: "synthetic:cube:body#0".try_into().expect("valid identity"),
        shells: vec!["synthetic:cube:shell#0".try_into().expect("valid identity")],
    });
    ir.model.bodies.push(Body {
        id: "synthetic:cube:body#0".try_into().expect("valid identity"),
        kind: BodyKind::Solid,
        regions: vec!["synthetic:cube:region#0"
            .try_into()
            .expect("valid identity")],
        transform: None,
        name: Some("unit cube".into()),
        color: None,
        visible: None,
    });

    ir.finalize();

    ir
}

/// A canonical fixture covering directed `SubD` and a Sum procedural surface.
pub fn directed_subd_sum() -> Result<CadIr, crate::geometry::CacheFitToleranceError> {
    let mut ir = CadIr::empty();
    ir.model.curves = vec![
        Curve {
            id: CurveId::mint("synthetic:v2:curve#u").expect("valid identity"),
            geometry: CurveGeometry::Line {
                origin: Point3::new(0.0, 0.0, 0.0),
                direction: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
        Curve {
            id: CurveId::mint("synthetic:v2:curve#v").expect("valid identity"),
            geometry: CurveGeometry::Line {
                origin: Point3::new(0.0, 0.0, 0.0),
                direction: Vector3::new(0.0, 1.0, 0.0),
            },
            source_object: None,
        },
    ];
    let construction =
        ProceduralSurfaceId::mint("synthetic:v2:procedural-surface#sum").expect("valid identity");
    ir.model.surfaces.push(Surface {
        id: SurfaceId::mint("synthetic:v2:surface#sum-cache").expect("valid identity"),
        geometry: SurfaceGeometry::Procedural {
            construction: construction.clone(),
            cache: Some(
                SolvedSurfaceGeometry::new(SurfaceGeometry::Plane {
                    origin: Point3::new(0.0, 0.0, 0.0),
                    normal: Vector3::new(0.0, 0.0, 1.0),
                    u_axis: Vector3::new(1.0, 0.0, 0.0),
                })
                .expect("solved example surface"),
            ),
        },
        source_object: None,
    });
    ir.model
        .procedural_surfaces
        .push(ProceduralSurface::try_new(
            construction,
            ProceduralSurfaceDefinition::Sum {
                first: CurveId::mint("synthetic:v2:curve#u").expect("valid identity"),
                second: CurveId::mint("synthetic:v2:curve#v").expect("valid identity"),
                basepoint: Vector3::new(0.0, 0.0, 0.0),
                revision_form: None,
            },
            Some(EPS_EXAMPLES_DIRECTED_SUBD_SUM_E9),
            None,
        )?);
    ir.model.subds.push(SubdSurface {
        id: SubdId::mint("synthetic:v2:subd#directed").expect("valid identity"),
        scheme: SubdScheme::CatmullClark,
        source_object: None,
        cage: crate::subd::SubdCage::new(
            vec![
                SubdVertex::new(Point3::new(0.0, 0.0, 0.0), SubdVertexTag::Crease, None)
                    .expect("valid example vertex"),
                SubdVertex::new(Point3::new(1.0, 0.0, 0.0), SubdVertexTag::Smooth, None)
                    .expect("valid example vertex"),
                SubdVertex::new(Point3::new(0.0, 1.0, 0.0), SubdVertexTag::Corner, None)
                    .expect("valid example vertex"),
            ],
            vec![
                SubdEdge::new(
                    [0, 1],
                    [0.25, 0.75],
                    SubdEdgeTag::Crease,
                    None,
                    [0.125, 0.875],
                )
                .expect("valid example edge"),
                SubdEdge::new([1, 2], [0.0, 0.5], SubdEdgeTag::SmoothX, None, [0.25, 0.75])
                    .expect("valid example edge"),
                SubdEdge::new([2, 0], [1.0, 0.0], SubdEdgeTag::Smooth, None, [0.5, 0.5])
                    .expect("valid example edge"),
            ],
            vec![SubdFace::new(vec![
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
            ])
            .expect("valid example cage")],
            Vec::new(),
        )
        .expect("valid example cage"),
    });
    ir.finalize();
    Ok(ir)
}

#[cfg(test)]
mod tests;
