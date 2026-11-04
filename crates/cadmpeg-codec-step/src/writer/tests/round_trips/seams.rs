// SPDX-License-Identifier: Apache-2.0
//! STEP writer seam edge emission: one 3D edge with two pcurves on one
//! surface is a seam, written as SEAM_CURVE with per-coedge SEAM_EDGE uses.

#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::{
    pcurve::PcurveGeometry, Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry,
    Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::CadIr;

use crate::loss::StepLossCode;
use crate::test_support::exchange::export;
use crate::StepCodec;

#[test]
fn seam_edge_writes_seam_curve_and_seam_edges() {
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::geometry::analytic::{CircleCurve, CylinderSurface, LineCurve};
    use cadmpeg_ir::geometry::pcurve::{LinePcurve, Pcurve};
    use cadmpeg_ir::ids::{
        BodyId, CoedgeId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId, VertexId,
    };
    use cadmpeg_ir::topology::{
        Body, BodyKind, Coedge, Edge, Face, Loop, LoopBoundary, LoopRing, PcurveUse, Point, Region,
        Sense, Shell, Vertex,
    };
    use cadmpeg_ir::units::UnitVector3;

    // A closed cylindrical face reuses its seam meridian twice: the two
    // coedges carry different pcurves (u = 0 and u = 2π) on the one
    // surface. A SURFACE_CURVE with both leaves no surface selector, so
    // the writer emits SEAM_CURVE and each seam coedge keeps a SEAM_EDGE
    // with its explicit pcurve reference (#233).
    let mut ir = CadIr::empty();
    let point = |id: &str, position: Point3| {
        Point::new(
            PointId::mint(id).expect("identity grammar"),
            FinitePoint3::new(position).expect("a finite position is a point"),
            None,
        )
    };
    ir.model
        .points
        .push(point("test:model:point#seam0", Point3::new(5.0, 0.0, 0.0)));
    ir.model
        .points
        .push(point("test:model:point#seam1", Point3::new(5.0, 0.0, 10.0)));
    let vertex = |id: &str, point: &str| Vertex {
        native_ref: None,
        id: VertexId::mint(id).expect("identity grammar"),
        point: PointId::mint(point).expect("identity grammar"),
        tolerance: None,
    };
    ir.model
        .vertices
        .push(vertex("test:model:vertex#seam0", "test:model:point#seam0"));
    ir.model
        .vertices
        .push(vertex("test:model:vertex#seam1", "test:model:point#seam1"));
    let curve_id = |id: &str| CurveId::mint(id).expect("identity grammar");
    ir.model.curves.push(Curve {
        native_ref: None,
        parameter_range: None,
        id: curve_id("test:model:curve#seam"),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(LineCurve::new(
            FinitePoint3::new(Point3::new(5.0, 0.0, 0.0)).expect("finite origin"),
            UnitVector3::normalized(Vector3::new(0.0, 0.0, 1.0)).expect("unit direction"),
        ))),
        source_object: None,
    });
    let rim = |id: &str, z: f64| Curve {
        native_ref: None,
        parameter_range: None,
        id: curve_id(id),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            CircleCurve::try_new(
                Point3::new(0.0, 0.0, z),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                5.0,
            )
            .expect("finite circle"),
        )),
        source_object: None,
    };
    ir.model.curves.push(rim("test:model:curve#seam-top", 10.0));
    ir.model
        .curves
        .push(rim("test:model:curve#seam-bottom", 0.0));
    let edge = |id: &str, curve: &str, range: [f64; 2], start: &str, end: &str| Edge {
        native_ref: None,
        id: EdgeId::mint(id).expect("identity grammar"),
        carrier: cadmpeg_ir::topology::EdgeCarrier::new(Some(curve_id(curve)), Some(range))
            .expect("ordered range"),
        start: VertexId::mint(start).expect("identity grammar"),
        end: VertexId::mint(end).expect("identity grammar"),
        tolerance: None,
    };
    ir.model.edges.push(edge(
        "test:model:edge#seam",
        "test:model:curve#seam",
        [0.0, 10.0],
        "test:model:vertex#seam0",
        "test:model:vertex#seam1",
    ));
    ir.model.edges.push(edge(
        "test:model:edge#seam-top",
        "test:model:curve#seam-top",
        [0.0, std::f64::consts::TAU],
        "test:model:vertex#seam1",
        "test:model:vertex#seam1",
    ));
    ir.model.edges.push(edge(
        "test:model:edge#seam-bottom",
        "test:model:curve#seam-bottom",
        [0.0, std::f64::consts::TAU],
        "test:model:vertex#seam0",
        "test:model:vertex#seam0",
    ));
    ir.model.surfaces.push(Surface {
        native_ref: None,
        id: SurfaceId::mint("test:model:surface#seam").expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            CylinderSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                5.0,
            )
            .expect("finite cylinder"),
        )),
        source_object: None,
    });
    let mut pcurve = |id: &str, origin: [f64; 2], direction: [f64; 2]| {
        let pcurve = PcurveId::mint(id).expect("identity grammar");
        ir.model.pcurves.push(Pcurve {
            native_ref: None,
            id: pcurve.clone(),
            geometry: PcurveGeometry::Line(
                LinePcurve::try_new(
                    Point2::new(origin[0], origin[1]),
                    Point2::new(direction[0], direction[1]),
                )
                .expect("nonzero direction"),
            ),
            metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::default(),
        });
        pcurve
    };
    let seam_west = pcurve("test:model:pcurve#seam-west", [0.0, 0.0], [0.0, 1.0]);
    let rim_top = pcurve("test:model:pcurve#seam-top", [0.0, 10.0], [1.0, 0.0]);
    let seam_east = pcurve(
        "test:model:pcurve#seam-east",
        [std::f64::consts::TAU, 0.0],
        [0.0, 1.0],
    );
    let rim_bottom = pcurve("test:model:pcurve#seam-bottom", [0.0, 0.0], [1.0, 0.0]);
    let coedge = |id: &str, edge: &str, sense: Sense, pcurve: PcurveId| Coedge {
        native_ref: None,
        id: CoedgeId::mint(id).expect("identity grammar"),
        owner_loop: LoopId::mint("test:model:loop#seam").expect("identity grammar"),
        edge: EdgeId::mint(edge).expect("identity grammar"),
        radial_next: CoedgeId::mint(id).expect("identity grammar"),
        sense,
        pcurves: vec![PcurveUse {
            pcurve,
            isoparametric: None,
            parameter_range: None,
        }],
        use_curve: None,
    };
    let coedges = [
        coedge(
            "test:model:coedge#seam-west",
            "test:model:edge#seam",
            Sense::Forward,
            seam_west,
        ),
        coedge(
            "test:model:coedge#seam-top",
            "test:model:edge#seam-top",
            Sense::Forward,
            rim_top,
        ),
        coedge(
            "test:model:coedge#seam-east",
            "test:model:edge#seam",
            Sense::Reversed,
            seam_east,
        ),
        coedge(
            "test:model:coedge#seam-bottom",
            "test:model:edge#seam-bottom",
            Sense::Forward,
            rim_bottom,
        ),
    ];
    let coedge_ids: Vec<CoedgeId> = coedges.iter().map(|coedge| coedge.id.clone()).collect();
    ir.model.coedges.extend(coedges);
    ir.model.loops.push(Loop {
        native_ref: None,
        id: LoopId::mint("test:model:loop#seam").expect("identity grammar"),
        face: FaceId::mint("test:model:face#seam").expect("identity grammar"),
        boundary: LoopBoundary::Ring(
            LoopRing::new(coedge_ids, Vec::new()).expect("valid loop ring"),
        ),
    });
    ir.model.faces.push(Face {
        native_ref: None,
        id: FaceId::mint("test:model:face#seam").expect("identity grammar"),
        shell: ShellId::mint("test:model:shell#seam").expect("identity grammar"),
        surface: SurfaceId::mint("test:model:surface#seam").expect("identity grammar"),
        sense: Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(vec![LoopId::mint(
            "test:model:loop#seam",
        )
        .expect("identity grammar")]),
        name: None,
        color: None,
        tolerance: None,
    });
    ir.model.shells.push(Shell::with_face(
        ShellId::mint("test:model:shell#seam").expect("identity grammar"),
        RegionId::mint("test:model:region#seam").expect("identity grammar"),
        FaceId::mint("test:model:face#seam").expect("identity grammar"),
    ));
    ir.model.regions.push(Region {
        native_ref: None,
        id: RegionId::mint("test:model:region#seam").expect("identity grammar"),
        body: BodyId::mint("test:model:body#seam").expect("identity grammar"),
        shells: vec![ShellId::mint("test:model:shell#seam").expect("identity grammar")],
    });
    ir.model.bodies.push(Body {
        native_ref: None,
        id: BodyId::mint("test:model:body#seam").expect("identity grammar"),
        kind: BodyKind::Sheet,
        regions: vec![RegionId::mint("test:model:region#seam").expect("identity grammar")],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });

    let text = export(&ir);
    assert_eq!(text.matches("SEAM_CURVE(").count(), 1);
    assert_eq!(text.matches("SEAM_EDGE(").count(), 2);
    assert_eq!(text.matches("ORIENTED_EDGE(").count(), 2);
    assert_eq!(text.matches("SURFACE_CURVE(").count(), 2);

    let decoded = StepCodec::default()
        .decode(
            &mut Cursor::new(text.into_bytes()),
            &DecodeOptions::default(),
        )
        .expect("decode seam face");
    let pcurves: Vec<_> = decoded
        .ir()
        .model
        .coedges
        .iter()
        .map(|coedge| coedge.pcurves.len())
        .collect();
    assert_eq!(pcurves.len(), 4);
    assert!(
        pcurves.iter().all(|uses| *uses == 1),
        "every coedge keeps its pcurve, {pcurves:?}"
    );
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code == StepLossCode::PcurveAssociationAmbiguous.kind() }));
}
