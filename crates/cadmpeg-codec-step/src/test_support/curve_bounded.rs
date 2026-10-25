// SPDX-License-Identifier: Apache-2.0
//! A sheet face whose curve-bounded surface leaves its outer boundary
//! implicit and states one square hole.

use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::analytic::{LineCurve, PlaneSurface};
use cadmpeg_ir::geometry::nurbs::{NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis};
use cadmpeg_ir::geometry::pcurve::{LinePcurve, Pcurve, PcurveGeometry, PcurveMetadata};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, DirectedParameterRange, ProceduralSurface, ProceduralSurfaceDefinition,
    SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, ProceduralSurfaceId,
    RegionId, ShellId, SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, EdgeCarrier, Face, FaceLoops, Loop, LoopBoundary, LoopRing,
    PcurveUse, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::CadIr;

/// Side length of the hole square.
pub(crate) const HOLE_SIDE: f64 = 0.5;

/// Corners of the hole, clockwise in `(u, v)` and in model `(x, y)`.
const HOLE: [[f64; 2]; 4] = [[0.25, 0.25], [0.25, 0.75], [0.75, 0.75], [0.75, 0.25]];

fn id<T>(
    mint: fn(String) -> Result<T, cadmpeg_ir::ids::IdentityError>,
    kind: &str,
    key: &str,
) -> T {
    mint(format!("test:model:{kind}#{key}")).expect("identity grammar")
}

/// The plane `z = 0` with `(u, v) = (x, y)`.
pub(crate) fn unbounded_plane() -> SolvedSurfaceGeometry {
    SolvedSurfaceGeometry::Plane(
        PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("plane"),
    )
}

/// The bilinear patch on the unit square of `z = 0` with
/// `(u, v) = (x, y)` on `[0, 1] x [0, 1]`.
pub(crate) fn unit_square_patch() -> SolvedSurfaceGeometry {
    let rows = vec![
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
        vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
    ];
    let axis = || NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false);
    SolvedSurfaceGeometry::Nurbs(
        NurbsSurface::new(
            axis(),
            axis(),
            NurbsPoleGrid::from_lanes(rows, None).expect("pole grid"),
            false,
        )
        .expect("bilinear patch"),
    )
}

/// A sheet with one face of `sense` on a curve-bounded surface over
/// `support` whose outer boundary is implicit. Its one loop is the hole
/// square of [`HOLE_SIDE`], wound so the face lies outside it.
pub(crate) fn implicit_outer_sheet(support: SolvedSurfaceGeometry, sense: Sense) -> CadIr {
    let mut ir = CadIr::empty();
    for (index, [x, y]) in HOLE.into_iter().enumerate() {
        let key = format!("hole-{index}");
        let position = FinitePoint3::new(Point3::new(x, y, 0.0)).expect("finite position");
        ir.model
            .points
            .push(Point::new(id(PointId::mint, "point", &key), position, None));
        ir.model.vertices.push(Vertex {
            id: id(VertexId::mint, "vertex", &key),
            point: id(PointId::mint, "point", &key),
            tolerance: None,
        });
    }
    let mut coedges = Vec::new();
    for (index, [x, y]) in HOLE.into_iter().enumerate() {
        let key = format!("hole-{index}");
        let next = HOLE[(index + 1) % HOLE.len()];
        let direction = [(next[0] - x) / HOLE_SIDE, (next[1] - y) / HOLE_SIDE];
        ir.model.curves.push(Curve {
            parameter_range: None,
            id: id(CurveId::mint, "curve", &key),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                LineCurve::try_new(
                    Point3::new(x, y, 0.0),
                    Vector3::new(direction[0], direction[1], 0.0),
                )
                .expect("line"),
            )),
            source_object: None,
        });
        ir.model.pcurves.push(Pcurve {
            id: id(PcurveId::mint, "pcurve", &key),
            geometry: PcurveGeometry::Line(
                LinePcurve::try_new(Point2::new(x, y), Point2::new(direction[0], direction[1]))
                    .expect("line pcurve"),
            ),
            metadata: PcurveMetadata::default(),
        });
        ir.model.edges.push(Edge {
            id: id(EdgeId::mint, "edge", &key),
            carrier: EdgeCarrier::new(
                Some(id(CurveId::mint, "curve", &key)),
                Some([0.0, HOLE_SIDE]),
            )
            .expect("bounded line"),
            start: id(VertexId::mint, "vertex", &key),
            end: id(
                VertexId::mint,
                "vertex",
                &format!("hole-{}", (index + 1) % HOLE.len()),
            ),
            tolerance: None,
        });
        let (coedge_sense, range) = match sense {
            Sense::Forward => (Sense::Forward, [0.0, HOLE_SIDE]),
            Sense::Reversed => (Sense::Reversed, [HOLE_SIDE, 0.0]),
        };
        coedges.push(Coedge {
            id: id(CoedgeId::mint, "coedge", &key),
            owner_loop: id(LoopId::mint, "loop", "hole"),
            edge: id(EdgeId::mint, "edge", &key),
            radial_next: id(CoedgeId::mint, "coedge", &key),
            sense: coedge_sense,
            pcurves: vec![PcurveUse {
                pcurve: id(PcurveId::mint, "pcurve", &key),
                isoparametric: None,
                parameter_range: Some(DirectedParameterRange::new(range).expect("range")),
            }],
            use_curve: None,
        });
    }
    if sense == Sense::Reversed {
        coedges.reverse();
    }
    let ring = coedges.iter().map(|coedge| coedge.id.clone()).collect();
    ir.model.coedges.extend(coedges);
    ir.model.surfaces.push(Surface {
        id: id(SurfaceId::mint, "surface", "support"),
        geometry: SurfaceGeometry::Solved(support.clone()),
        source_object: None,
    });
    ir.model.surfaces.push(Surface {
        id: id(SurfaceId::mint, "surface", "face"),
        geometry: SurfaceGeometry::Procedural {
            construction: id(ProceduralSurfaceId::mint, "procedural-surface", "face"),
            cache: Some(support),
        },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(ProceduralSurface::new(
        id(ProceduralSurfaceId::mint, "procedural-surface", "face"),
        ProceduralSurfaceDefinition::CurveBounded {
            support: id(SurfaceId::mint, "surface", "support"),
            boundaries: (0..HOLE.len())
                .map(|index| id(CurveId::mint, "curve", &format!("hole-{index}")))
                .collect(),
            boundary_pcurves: (0..HOLE.len())
                .map(|index| id(PcurveId::mint, "pcurve", &format!("hole-{index}")))
                .collect(),
            implicit_outer: true,
        },
        None,
    ));
    ir.model.loops.push(Loop {
        id: id(LoopId::mint, "loop", "hole"),
        face: id(FaceId::mint, "face", "sheet"),
        boundary: LoopBoundary::Ring(LoopRing::new(ring, Vec::new()).expect("non-empty ring")),
    });
    ir.model.faces.push(Face {
        id: id(FaceId::mint, "face", "sheet"),
        shell: id(ShellId::mint, "shell", "sheet"),
        surface: id(SurfaceId::mint, "surface", "face"),
        sense,
        loops: FaceLoops::unspecified(vec![id(LoopId::mint, "loop", "hole")]),
        name: None,
        color: None,
        tolerance: None,
    });
    ir.model.shells.push(
        Shell::new(
            id(ShellId::mint, "shell", "sheet"),
            id(RegionId::mint, "region", "sheet"),
            vec![id(FaceId::mint, "face", "sheet")],
            Vec::new(),
            Vec::new(),
        )
        .expect("shell with a face"),
    );
    ir.model.regions.push(Region {
        id: id(RegionId::mint, "region", "sheet"),
        body: id(BodyId::mint, "body", "sheet"),
        shells: vec![id(ShellId::mint, "shell", "sheet")],
    });
    ir.model.bodies.push(Body {
        id: id(BodyId::mint, "body", "sheet"),
        kind: BodyKind::Sheet,
        regions: vec![id(RegionId::mint, "region", "sheet")],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir.finalize();
    ir
}
