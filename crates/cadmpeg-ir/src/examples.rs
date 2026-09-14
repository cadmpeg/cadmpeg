// SPDX-License-Identifier: Apache-2.0
//! Hand-built documents for examples and tests.

use std::collections::HashMap;

use crate::document::CadIr;
use crate::geometry::{
    derive_reference_direction, Curve, CurveGeometry, ProceduralSurface,
    ProceduralSurfaceDefinition, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface,
    SurfaceGeometry,
};
use crate::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, IdentityKey, LoopId, PointId, ProceduralSurfaceId,
    RegionId, ShellId, SubdId, SurfaceId, VertexId,
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

/// Failure while assembling a hand-built example document.
#[derive(Debug, thiserror::Error)]
pub enum ExampleError {
    /// An analytic or topological geometry constructor rejected its payload.
    #[error("example geometry is invalid: {0}")]
    Geometry(&'static str),
    /// A loop ring violated its structural contract.
    #[error(transparent)]
    LoopRing(#[from] crate::topology::LoopRingError),
    /// A shell had no admitted members.
    #[error(transparent)]
    Shell(#[from] crate::features::BodySelectionError),
}

macro_rules! cube_id {
    ($type:ident, $kind:literal, $key:expr) => {
        $type::compose(&crate::identity_namespace!("synthetic", "cube", $kind), $key)
    };
}

macro_rules! v2_id {
    ($type:ident, $kind:literal, $key:expr) => {
        $type::compose(&crate::identity_namespace!("synthetic", "v2", $kind), $key)
    };
}

/// Face input used to construct [`unit_cube`].
type FaceDef = (
    IdentityKey,
    (f64, f64, f64),
    (f64, f64, f64),
    [(usize, bool); 4],
);

/// A `10 mm` axis-aligned cube spanning the origin to `(10, 10, 10)`.
pub fn unit_cube() -> Result<CadIr, ExampleError> {
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
            crate::identity_key!("bottom"),
            (0.0, 0.0, -1.0),
            (0.0, 0.0, 0.0),
            [(0, true), (1, true), (2, true), (3, true)],
        ),
        (
            crate::identity_key!("top"),
            (0.0, 0.0, 1.0),
            (0.0, 0.0, s),
            [(7, false), (6, false), (5, false), (4, false)],
        ),
        (
            crate::identity_key!("front"),
            (0.0, -1.0, 0.0),
            (0.0, 0.0, 0.0),
            [(0, false), (8, true), (4, true), (9, false)],
        ),
        (
            crate::identity_key!("right"),
            (1.0, 0.0, 0.0),
            (s, 0.0, 0.0),
            [(1, false), (9, true), (5, true), (10, false)],
        ),
        (
            crate::identity_key!("back"),
            (0.0, 1.0, 0.0),
            (0.0, s, 0.0),
            [(2, false), (10, true), (6, true), (11, false)],
        ),
        (
            crate::identity_key!("left"),
            (-1.0, 0.0, 0.0),
            (0.0, 0.0, 0.0),
            [(3, false), (11, true), (7, true), (8, false)],
        ),
    ];

    let mut ir = CadIr::empty();

    // Points + vertices.
    for (i, (x, y, z)) in corners.iter().enumerate() {
        ir.model.points.push(Point {
            id: cube_id!(PointId, "point", i),
            position: Point3::new(*x, *y, *z),
            source_object: None,
        });
        ir.model.vertices.push(Vertex {
            id: cube_id!(VertexId, "vertex", i),
            point: cube_id!(PointId, "point", i),
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
            id: cube_id!(CurveId, "curve", i),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                crate::geometry::LineCurve::try_new(Point3::new(ax, ay, az), unit)
                    .map_err(ExampleError::Geometry)?,
            )),
            source_object: None,
        });
        ir.model.edges.push(Edge {
            id: cube_id!(EdgeId, "edge", i),
            carrier: crate::topology::EdgeCarrier::new(
                Some(cube_id!(CurveId, "curve", i)),
                Some([0.0, len]),
            )
            .map_err(ExampleError::Geometry)?,
            start: cube_id!(VertexId, "vertex", *a),
            end: cube_id!(VertexId, "vertex", *b),
            tolerance: None,
        });
    }

    // Faces, surfaces, loops, coedges.
    let mut edge_to_coedges: HashMap<usize, Vec<CoedgeId>> = HashMap::new();
    for (name, normal, origin, ring) in &face_defs {
        let surf_id = cube_id!(SurfaceId, "surface", name.clone());
        ir.model.surfaces.push(Surface {
            id: surf_id.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                crate::geometry::PlaneSurface::try_new(
                    Point3::new(origin.0, origin.1, origin.2),
                    Vector3::new(normal.0, normal.1, normal.2),
                    derive_reference_direction(Vector3::new(normal.0, normal.1, normal.2)),
                )
                .map_err(ExampleError::Geometry)?,
            )),
            source_object: None,
        });

        let loop_id = cube_id!(LoopId, "loop", name.clone());
        let coedge_ids: Vec<CoedgeId> = (0..ring.len())
            .map(|i| cube_id!(CoedgeId, "coedge", name.clone().colon(i)))
            .collect();

        for (i, (edge_index, forward)) in ring.iter().enumerate() {
            ir.model.coedges.push(Coedge {
                id: coedge_ids[i].clone(),
                owner_loop: loop_id.clone(),
                edge: cube_id!(EdgeId, "edge", *edge_index),
                radial_next: coedge_ids[i].clone(),
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
            id: loop_id.clone(),
            face: cube_id!(FaceId, "face", name.clone()),
            boundary: crate::topology::LoopBoundary::Ring(
                crate::topology::LoopRing::new(coedge_ids.clone(), Vec::new())?,
            ),
        });
        ir.model.faces.push(Face {
            id: cube_id!(FaceId, "face", name.clone()),
            shell: cube_id!(ShellId, "shell", 0_usize),
            surface: surf_id,
            sense: Sense::Forward,
            loops: crate::topology::FaceLoops::unspecified(vec![loop_id]),
            name: Some(format!("{name} face")),
            color: None,
            tolerance: None,
        });
    }

    // Pair coedges: each edge has exactly two, which partner each other.
    let partner_of: HashMap<String, CoedgeId> = edge_to_coedges
        .values()
        .filter(|v| v.len() == 2)
        .flat_map(|v| {
            [
                (v[0].as_str().to_owned(), v[1].clone()),
                (v[1].as_str().to_owned(), v[0].clone()),
            ]
        })
        .collect();
    for ce in &mut ir.model.coedges {
        if let Some(p) = partner_of.get(ce.id.as_str()) {
            ce.radial_next = p.clone();
        }
    }

    // Shell, region, body.
    ir.model.shells.push(
        Shell::new(
            cube_id!(ShellId, "shell", 0_usize),
            cube_id!(RegionId, "region", 0_usize),
            face_defs
                .iter()
                .map(|(name, ..)| cube_id!(FaceId, "face", name.clone()))
                .collect(),
            Vec::new(),
            Vec::new(),
        )
        ?,
    );
    ir.model.regions.push(Region {
        id: cube_id!(RegionId, "region", 0_usize),
        body: cube_id!(BodyId, "body", 0_usize),
        shells: vec![cube_id!(ShellId, "shell", 0_usize)],
    });
    ir.model.bodies.push(Body {
        id: cube_id!(BodyId, "body", 0_usize),
        kind: BodyKind::Solid,
        regions: vec![cube_id!(RegionId, "region", 0_usize)],
        transform: None,
        name: Some("unit cube".into()),
        color: None,
        visible: None,
    });

    ir.finalize();

    Ok(ir)
}

/// A canonical fixture covering directed `SubD` and a Sum procedural surface.
pub fn directed_subd_sum() -> Result<CadIr, crate::geometry::ProceduralGeometryError> {
    let mut ir = CadIr::empty();
    ir.model.curves = vec![
        Curve {
            id: v2_id!(CurveId, "curve", crate::identity_key!("u")),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                crate::geometry::LineCurve::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .map_err(|_| {
                    crate::geometry::ProceduralGeometryError::Payload(
                        "invalid directed SubD example curve",
                    )
                })?,
            )),
            source_object: None,
        },
        Curve {
            id: v2_id!(CurveId, "curve", crate::identity_key!("v")),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                crate::geometry::LineCurve::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                )
                .map_err(|_| {
                    crate::geometry::ProceduralGeometryError::Payload(
                        "invalid directed SubD example curve",
                    )
                })?,
            )),
            source_object: None,
        },
    ];
    let construction = v2_id!(
        ProceduralSurfaceId,
        "procedural-surface",
        crate::identity_key!("sum")
    );
    ir.model.surfaces.push(Surface {
        id: v2_id!(SurfaceId, "surface", crate::identity_key!("sum-cache")),
        geometry: SurfaceGeometry::Procedural {
            construction: construction.clone(),
            cache: Some(SolvedSurfaceGeometry::Plane(
                crate::geometry::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .map_err(|_| {
                    crate::geometry::ProceduralGeometryError::Payload(
                        "invalid directed SubD example surface",
                    )
                })?,
            )),
        },
        source_object: None,
    });
    let mut sum_definition = ProceduralSurfaceDefinition::Sum(
        crate::geometry::surface_payloads::SumSurfaceConstruction::try_new(
            v2_id!(CurveId, "curve", crate::identity_key!("u")),
            v2_id!(CurveId, "curve", crate::identity_key!("v")),
            Vector3::new(0.0, 0.0, 0.0),
            crate::geometry::CacheContract::from_form(None),
        )?,
    );
    sum_definition.set_legacy_cache(Some(crate::geometry::LegacyCache::try_new(
        EPS_EXAMPLES_DIRECTED_SUBD_SUM_E9,
    )?))?;
    ir.model
        .procedural_surfaces
        .push(ProceduralSurface::new(construction, sum_definition, None)?);
    ir.model.subds.push(SubdSurface {
        id: v2_id!(SubdId, "subd", crate::identity_key!("directed")),
        scheme: SubdScheme::CatmullClark,
        source_object: None,
        cage: crate::subd::SubdCage::new(
            vec![
                SubdVertex::new(Point3::new(0.0, 0.0, 0.0), SubdVertexTag::Crease, None)
                    .map_err(|_| {
                        crate::geometry::ProceduralGeometryError::Payload(
                            "invalid directed SubD example vertex",
                        )
                    })?,
                SubdVertex::new(Point3::new(1.0, 0.0, 0.0), SubdVertexTag::Smooth, None)
                    .map_err(|_| {
                        crate::geometry::ProceduralGeometryError::Payload(
                            "invalid directed SubD example vertex",
                        )
                    })?,
                SubdVertex::new(Point3::new(0.0, 1.0, 0.0), SubdVertexTag::Corner, None)
                    .map_err(|_| {
                        crate::geometry::ProceduralGeometryError::Payload(
                            "invalid directed SubD example vertex",
                        )
                    })?,
            ],
            vec![
                SubdEdge::new(
                    [0, 1],
                    [0.25, 0.75],
                    SubdEdgeTag::Crease,
                    None,
                    [0.125, 0.875],
                )
                .map_err(|_| {
                    crate::geometry::ProceduralGeometryError::Payload(
                        "invalid directed SubD example edge",
                    )
                })?,
                SubdEdge::new([1, 2], [0.0, 0.5], SubdEdgeTag::SmoothX, None, [0.25, 0.75])
                    .map_err(|_| {
                        crate::geometry::ProceduralGeometryError::Payload(
                            "invalid directed SubD example edge",
                        )
                    })?,
                SubdEdge::new([2, 0], [1.0, 0.0], SubdEdgeTag::Smooth, None, [0.5, 0.5])
                    .map_err(|_| {
                        crate::geometry::ProceduralGeometryError::Payload(
                            "invalid directed SubD example edge",
                        )
                    })?,
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
            .map_err(|_| {
                crate::geometry::ProceduralGeometryError::Payload(
                    "invalid directed SubD example face",
                )
            })?],
            Vec::new(),
        )
        .map_err(|_| {
            crate::geometry::ProceduralGeometryError::Payload(
                "invalid directed SubD example cage",
            )
        })?,
    });
    ir.finalize();
    Ok(ir)
}

#[cfg(test)]
mod tests;
