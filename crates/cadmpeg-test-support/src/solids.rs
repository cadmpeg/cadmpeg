// SPDX-License-Identifier: Apache-2.0
//! A neutral solid cylinder whose two circle edges are closed and store no
//! parameter range.

use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::analytic::{CircleCurve, CylinderSurface, LineCurve, PlaneSurface};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
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
use cadmpeg_ir::{CadIr, SourceObjectAssociation};

/// Radius of [`solid_cylinder`] in millimeters.
pub const CYLINDER_RADIUS: f64 = 10.0;
/// Height of [`solid_cylinder`] in millimeters.
pub const CYLINDER_HEIGHT: f64 = 30.0;

fn id<T>(
    mint: fn(String) -> Result<T, cadmpeg_ir::ids::IdentityError>,
    kind: &str,
    key: &str,
) -> T {
    mint(format!("test:model:{kind}#{key}")).expect("identity grammar")
}

/// A closed edge on `curve` whose one vertex is `vertex`, with no stored
/// parameter range.
#[must_use]
pub fn closed_edge(edge: &str, curve: &str, vertex: &str) -> Edge {
    Edge {
        native_ref: None,
        id: id(EdgeId::mint, "edge", edge),
        carrier: EdgeCarrier::new(Some(id(CurveId::mint, "curve", curve)), None)
            .expect("a curve with no range is a carrier"),
        start: id(VertexId::mint, "vertex", vertex),
        end: id(VertexId::mint, "vertex", vertex),
        tolerance: None,
    }
}

/// A right circular solid cylinder about `+z` from `z = 0` to
/// [`CYLINDER_HEIGHT`] with radius [`CYLINDER_RADIUS`]. Its seam line runs
/// from `(r, 0, 0)` to `(r, 0, h)`. The bottom and top circles are closed
/// edges at those vertices with no stored parameter range. No curve has a
/// source association.
#[must_use]
pub fn solid_cylinder() -> CadIr {
    cylinder_with_curve_sources(|_| None)
}

/// [`solid_cylinder`] whose `bottom`, `top` and `seam` curves take the
/// source association that `curve_source` gives for that key.
#[must_use]
pub fn cylinder_with_curve_sources(
    curve_source: fn(&str) -> Option<SourceObjectAssociation>,
) -> CadIr {
    let (r, h) = (CYLINDER_RADIUS, CYLINDER_HEIGHT);
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let reference = Vector3::new(1.0, 0.0, 0.0);
    let mut ir = CadIr::empty();

    for (key, z) in [("bottom", 0.0), ("top", h)] {
        let position = FinitePoint3::new(Point3::new(r, 0.0, z)).expect("finite position");
        ir.model
            .points
            .push(Point::new(id(PointId::mint, "point", key), position, None));
        ir.model.vertices.push(Vertex {
            native_ref: None,
            id: id(VertexId::mint, "vertex", key),
            point: id(PointId::mint, "point", key),
            tolerance: None,
        });
        ir.model.curves.push(Curve {
            native_ref: None,
            parameter_range: None,
            id: id(CurveId::mint, "curve", key),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                CircleCurve::try_new(Point3::new(0.0, 0.0, z), axis, reference, r).expect("circle"),
            )),
            source_object: curve_source(key),
        });
        ir.model.edges.push(closed_edge(key, key, key));
        ir.model.surfaces.push(Surface {
            native_ref: None,
            id: id(SurfaceId::mint, "surface", key),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                PlaneSurface::try_new(Point3::new(0.0, 0.0, z), axis, reference).expect("plane"),
            )),
            source_object: None,
        });
    }
    ir.model.curves.push(Curve {
        native_ref: None,
        parameter_range: None,
        id: id(CurveId::mint, "curve", "seam"),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            LineCurve::try_new(Point3::new(r, 0.0, 0.0), axis).expect("line"),
        )),
        source_object: curve_source("seam"),
    });
    ir.model.edges.push(Edge {
        native_ref: None,
        id: id(EdgeId::mint, "edge", "seam"),
        carrier: EdgeCarrier::new(Some(id(CurveId::mint, "curve", "seam")), Some([0.0, h]))
            .expect("bounded seam"),
        start: id(VertexId::mint, "vertex", "bottom"),
        end: id(VertexId::mint, "vertex", "top"),
        tolerance: None,
    });
    ir.model.surfaces.push(Surface {
        native_ref: None,
        id: id(SurfaceId::mint, "surface", "side"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            CylinderSurface::try_new(Point3::new(0.0, 0.0, 0.0), axis, reference, r)
                .expect("cylinder"),
        )),
        source_object: None,
    });

    // (coedge, loop, edge, sense, radial partner). The side ring runs
    // counterclockwise about the outward normal; each cap traverses its
    // circle opposite to the side's use of it.
    let coedges = [
        ("side-bottom", "side", "bottom", Sense::Forward, "bottom"),
        (
            "side-seam-up",
            "side",
            "seam",
            Sense::Forward,
            "side-seam-down",
        ),
        ("side-top", "side", "top", Sense::Reversed, "top"),
        (
            "side-seam-down",
            "side",
            "seam",
            Sense::Reversed,
            "side-seam-up",
        ),
        ("bottom", "bottom", "bottom", Sense::Reversed, "side-bottom"),
        ("top", "top", "top", Sense::Forward, "side-top"),
    ];
    for (coedge, owner, edge, sense, partner) in coedges {
        ir.model.coedges.push(Coedge {
            native_ref: None,
            id: id(CoedgeId::mint, "coedge", coedge),
            owner_loop: id(LoopId::mint, "loop", owner),
            edge: id(EdgeId::mint, "edge", edge),
            radial_next: id(CoedgeId::mint, "coedge", partner),
            sense,
            pcurves: Vec::new(),
            use_curve: None,
        });
    }

    let faces = [
        (
            "side",
            Sense::Forward,
            &["side-bottom", "side-seam-up", "side-top", "side-seam-down"][..],
        ),
        ("bottom", Sense::Reversed, &["bottom"][..]),
        ("top", Sense::Forward, &["top"][..]),
    ];
    for (face, sense, ring) in faces {
        ir.model.loops.push(Loop {
            native_ref: None,
            id: id(LoopId::mint, "loop", face),
            face: id(FaceId::mint, "face", face),
            boundary: LoopBoundary::Ring(
                LoopRing::new(
                    ring.iter()
                        .map(|coedge| id(CoedgeId::mint, "coedge", coedge))
                        .collect(),
                    Vec::new(),
                )
                .expect("non-empty ring"),
            ),
        });
        ir.model.faces.push(Face {
            native_ref: None,
            id: id(FaceId::mint, "face", face),
            shell: id(ShellId::mint, "shell", "cylinder"),
            surface: id(SurfaceId::mint, "surface", face),
            sense,
            loops: FaceLoops::classified(id(LoopId::mint, "loop", face), Vec::new()),
            name: Some(face.to_owned()),
            color: None,
            tolerance: None,
        });
    }

    ir.model.shells.push(
        Shell::new(
            id(ShellId::mint, "shell", "cylinder"),
            id(RegionId::mint, "region", "cylinder"),
            faces
                .iter()
                .map(|(face, ..)| id(FaceId::mint, "face", face))
                .collect(),
            Vec::new(),
            Vec::new(),
        )
        .expect("shell with faces"),
    );
    ir.model.regions.push(Region {
        native_ref: None,
        id: id(RegionId::mint, "region", "cylinder"),
        body: id(BodyId::mint, "body", "cylinder"),
        shells: vec![id(ShellId::mint, "shell", "cylinder")],
    });
    ir.model.bodies.push(Body {
        native_ref: None,
        id: id(BodyId::mint, "body", "cylinder"),
        kind: BodyKind::Solid,
        regions: vec![id(RegionId::mint, "region", "cylinder")],
        transform: None,
        name: Some("cylinder".into()),
        color: None,
        visible: None,
    });
    ir.finalize();
    ir
}
