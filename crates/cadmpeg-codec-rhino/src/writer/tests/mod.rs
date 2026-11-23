// SPDX-License-Identifier: Apache-2.0
//! Writer unit tests.

use cadmpeg_ir::codec::write::target::TargetRequest;
use std::io::Cursor;

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::math::Point3;

use crate::{RhinoArchiveVersion, RhinoCodec};

mod computed_values;
mod coverage;
mod encoding;
mod free_geometry;
mod model;
mod nurbs;
mod planar;
mod targets;

#[test]
fn frame_admission_refuses_nonfinite_axes() {
    use cadmpeg_ir::units::OrthonormalFrame3;
    let valid = cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0);
    let x = cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0);
    let nan = cadmpeg_ir::math::Vector3::new(f64::NAN, 0.0, 1.0);
    let frame = OrthonormalFrame3::new(valid, x).expect("orthonormal frame");
    assert!(super::check_frame("test", frame, "circle").is_ok());
    assert!(OrthonormalFrame3::new(nan, x).is_none());
    assert!(OrthonormalFrame3::new(valid, nan).is_none());
}

#[test]
fn empty_point_cloud_cannot_emit_infinite_bounds() {
    assert!(super::point_cloud_payload(&[]).is_err());
}

fn assert_planar_sheet_round_trip(ir: &CadIr, loop_count: usize, edge_count: usize) {
    for version in [
        RhinoArchiveVersion::V5,
        RhinoArchiveVersion::V6,
        RhinoArchiveVersion::V7,
        RhinoArchiveVersion::V8,
    ] {
        let mut bytes = Vec::new();
        RhinoCodec
            .plan(
                cadmpeg_ir::codec::write::EncodeInput::new(ir, None),
                TargetRequest::Explicit(version.descriptor().id.as_str()),
            )
            .and_then(|plan| plan.write_to(&mut bytes))
            .expect("required invariant");
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("required invariant");
        assert!(
            decoded
                .report()
                .losses
                .iter()
                .all(|loss| !loss.message.contains("Brep mesh cache degraded")),
            "{version:?}: {:?}",
            decoded.report().losses
        );
        assert_eq!(decoded.ir().geometry.bodies.len(), 1, "{version:?}");
        assert_eq!(
            decoded.ir().geometry.bodies[0].kind,
            cadmpeg_ir::topology::BodyKind::Sheet,
            "{version:?}"
        );
        assert_eq!(decoded.ir().geometry.faces.len(), 1, "{version:?}");
        assert_eq!(decoded.ir().geometry.loops.len(), loop_count, "{version:?}");
        assert_eq!(
            decoded.ir().geometry.coedges.len(),
            edge_count,
            "{version:?}"
        );
        assert_eq!(decoded.ir().geometry.edges.len(), edge_count, "{version:?}");
        assert_eq!(
            decoded.ir().geometry.vertices.len(),
            edge_count,
            "{version:?}"
        );
        for (actual, expected) in decoded.ir().geometry.edges.iter().zip(&ir.geometry.edges) {
            assert_eq!(actual.param_range(), expected.param_range(), "{version:?}");
        }
        cadmpeg_measure::test_support::round_trip::measures_agree("planar sheet", ir, decoded.ir())
            .expect("the round trip keeps its measured geometry");
        assert!(
            cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
                .expect("validate the neutral model")
                .is_ok(),
            "{version:?}"
        );
    }
}

fn polygon_sheet(points: &[Point3]) -> CadIr {
    use cadmpeg_ir::geometry::{
        Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{
        BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, RegionId, ShellId, SurfaceId,
        VertexId,
    };
    use cadmpeg_ir::math::Vector3;
    use cadmpeg_ir::topology::{
        Body, BodyKind, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
    };

    let mut ir = CadIr::empty();
    let body: BodyId = "cadir:model:body#polygon"
        .try_into()
        .expect("valid identity");
    let region: RegionId = "cadir:model:region#polygon"
        .try_into()
        .expect("valid identity");
    let shell: ShellId = "cadir:model:shell#polygon"
        .try_into()
        .expect("valid identity");
    let face: FaceId = "cadir:model:face#polygon"
        .try_into()
        .expect("valid identity");
    let loop_id: LoopId = "cadir:model:loop#polygon"
        .try_into()
        .expect("valid identity");
    let surface: SurfaceId = "cadir:model:surface#polygon"
        .try_into()
        .expect("valid identity");
    let point_ids = (0..points.len())
        .map(|index| {
            PointId::mint(format!("cadir:model:point#polygon.{index}")).expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let vertex_ids = (0..points.len())
        .map(|index| {
            VertexId::mint(format!("cadir:model:vertex#polygon.{index}")).expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let edge_ids = (0..points.len())
        .map(|index| {
            EdgeId::mint(format!("cadir:model:edge#polygon.{index}")).expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let curve_ids = (0..points.len())
        .map(|index| {
            CurveId::mint(format!("cadir:model:curve#polygon.{index}")).expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let coedge_ids = (0..points.len())
        .map(|index| {
            CoedgeId::mint(format!("cadir:model:coedge#polygon.{index}")).expect("identity grammar")
        })
        .collect::<Vec<_>>();
    ir.geometry.bodies.push(Body {
        id: body.clone(),
        kind: BodyKind::Sheet,
        transform: None,
        name: None,
        color: None,
        visible: None,
        native_ref: None,
    });
    ir.geometry.regions.push(Region {
        id: region.clone(),
        body,
        shells: vec![shell.clone()],
        native_ref: None,
    });
    ir.geometry.shells.push(Shell::new(shell.clone(), region));
    ir.geometry.faces.push(Face {
        id: face.clone(),
        shell,
        surface: surface.clone(),
        sense: Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(vec![loop_id.clone()]),
        name: None,
        color: None,
        tolerance: None,
        native_ref: None,
    });
    ir.geometry.loops.push(Loop {
        id: loop_id.clone(),
        face,
        boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
            cadmpeg_ir::topology::LoopRing::new(coedge_ids.clone(), Vec::new())
                .expect("valid loop ring"),
        ),
        native_ref: None,
    });
    ir.geometry.surfaces.push(Surface {
        id: surface,
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                points[0],
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
        native_ref: None,
    });
    for index in 0..points.len() {
        let next = points[(index + 1) % points.len()];
        let delta = Vector3::new(
            next.x - points[index].x,
            next.y - points[index].y,
            next.z - points[index].z,
        );
        let length = delta.norm();
        let direction = Vector3::new(delta.x / length, delta.y / length, delta.z / length);
        ir.geometry.points.push(Point::new(
            point_ids[index].clone(),
            cadmpeg_ir::features::geometry::FinitePoint3::new(points[index])
                .expect("a finite position is a point"),
            None,
        ));
        ir.geometry.vertices.push(Vertex {
            id: vertex_ids[index].clone(),
            point: point_ids[index].clone(),
            tolerance: None,
            native_ref: None,
        });
        ir.geometry.curves.push(Curve {
            parameter_range: None,
            id: curve_ids[index].clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::try_new(points[index], direction)
                    .unwrap(),
            )),
            source_object: None,
            native_ref: None,
        });
        ir.geometry.edges.push(Edge {
            id: edge_ids[index].clone(),
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                Some(curve_ids[index].clone()),
                Some([0.0, length]),
            )
            .unwrap(),
            ends: Some(cadmpeg_ir::topology::EdgeEnds::new(
                vertex_ids[index].clone(),
                vertex_ids[(index + 1) % points.len()].clone(),
            )),
            tolerance: None,
            native_ref: None,
        });
        ir.geometry.coedges.push(Coedge {
            id: coedge_ids[index].clone(),
            owner_loop: loop_id.clone(),
            edge: edge_ids[index].clone(),
            radial_next: coedge_ids[index].clone(),
            sense: Sense::Forward,
            pcurves: Vec::new(),
            use_curve: None,
            native_ref: None,
        });
    }
    ir.finalize();
    ir
}

fn add_polygon_hole(ir: &mut CadIr, points: &[Point3]) {
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::ids::{CoedgeId, CurveId, EdgeId, LoopId, PointId, VertexId};
    use cadmpeg_ir::math::Vector3;
    use cadmpeg_ir::topology::{Coedge, Edge, Loop, Point, Sense, Vertex};

    let base = ir.geometry.edges.len();
    let face = ir.geometry.faces[0].id.clone();
    let loop_id = LoopId::mint(format!(
        "cadir:model:loop#polygon.{}",
        ir.geometry.loops.len()
    ))
    .expect("identity grammar");
    let point_ids = (0..points.len())
        .map(|index| {
            PointId::mint(format!("cadir:model:point#polygon.{}", base + index))
                .expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let vertex_ids = (0..points.len())
        .map(|index| {
            VertexId::mint(format!("cadir:model:vertex#polygon.{}", base + index))
                .expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let edge_ids = (0..points.len())
        .map(|index| {
            EdgeId::mint(format!("cadir:model:edge#polygon.{}", base + index))
                .expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let curve_ids = (0..points.len())
        .map(|index| {
            CurveId::mint(format!("cadir:model:curve#polygon.{}", base + index))
                .expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let coedge_ids = (0..points.len())
        .map(|index| {
            CoedgeId::mint(format!("cadir:model:coedge#polygon.{}", base + index))
                .expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let face_loops = ir.geometry.faces[0]
        .loops
        .iter()
        .cloned()
        .chain(std::iter::once(loop_id.clone()))
        .collect();
    ir.geometry.faces[0].loops = cadmpeg_ir::topology::FaceLoops::unspecified(face_loops);
    ir.geometry.loops.push(Loop {
        id: loop_id.clone(),
        face,
        boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
            cadmpeg_ir::topology::LoopRing::new(coedge_ids.clone(), Vec::new())
                .expect("valid loop ring"),
        ),
        native_ref: None,
    });
    for index in 0..points.len() {
        let next_index = (index + 1) % points.len();
        let next = points[next_index];
        let delta = Vector3::new(
            next.x - points[index].x,
            next.y - points[index].y,
            next.z - points[index].z,
        );
        let length = delta.norm();
        ir.geometry.points.push(Point::new(
            point_ids[index].clone(),
            cadmpeg_ir::features::geometry::FinitePoint3::new(points[index])
                .expect("a finite position is a point"),
            None,
        ));
        ir.geometry.vertices.push(Vertex {
            id: vertex_ids[index].clone(),
            point: point_ids[index].clone(),
            tolerance: None,
            native_ref: None,
        });
        ir.geometry.curves.push(Curve {
            parameter_range: None,
            id: curve_ids[index].clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                    points[index],
                    Vector3::new(delta.x / length, delta.y / length, delta.z / length),
                )
                .unwrap(),
            )),
            source_object: None,
            native_ref: None,
        });
        ir.geometry.edges.push(Edge {
            id: edge_ids[index].clone(),
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                Some(curve_ids[index].clone()),
                Some([0.0, length]),
            )
            .unwrap(),
            ends: Some(cadmpeg_ir::topology::EdgeEnds::new(
                vertex_ids[index].clone(),
                vertex_ids[next_index].clone(),
            )),
            tolerance: None,
            native_ref: None,
        });
        ir.geometry.coedges.push(Coedge {
            id: coedge_ids[index].clone(),
            owner_loop: loop_id.clone(),
            edge: edge_ids[index].clone(),
            radial_next: coedge_ids[index].clone(),
            sense: Sense::Forward,
            pcurves: Vec::new(),
            use_curve: None,
            native_ref: None,
        });
    }
    ir.finalize();
}

fn adjacent_quad_sheet() -> CadIr {
    use cadmpeg_ir::geometry::{
        Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{
        BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, RegionId, ShellId, SurfaceId,
        VertexId,
    };
    use cadmpeg_ir::math::Vector3;
    use cadmpeg_ir::topology::{
        Body, BodyKind, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
    };

    let mut ir = CadIr::empty();
    let body: BodyId = "cadir:model:body#adjacent"
        .try_into()
        .expect("valid identity");
    let region: RegionId = "cadir:model:region#adjacent"
        .try_into()
        .expect("valid identity");
    let shell: ShellId = "cadir:model:shell#adjacent"
        .try_into()
        .expect("valid identity");
    let face_ids = [
        FaceId::mint("cadir:model:face#adjacent.0").expect("identity grammar"),
        FaceId::mint("cadir:model:face#adjacent.1").expect("identity grammar"),
    ];
    let loop_ids = [
        LoopId::mint("cadir:model:loop#adjacent.0").expect("identity grammar"),
        LoopId::mint("cadir:model:loop#adjacent.1").expect("identity grammar"),
    ];
    let surface_ids = [
        SurfaceId::mint("cadir:model:surface#adjacent.0").expect("identity grammar"),
        SurfaceId::mint("cadir:model:surface#adjacent.1").expect("identity grammar"),
    ];
    let positions = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(1.0, 1.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(2.0, 1.0, 0.0),
    ];
    let point_ids = (0..positions.len())
        .map(|index| {
            PointId::mint(format!("cadir:model:point#adjacent.{index}")).expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let vertex_ids = (0..positions.len())
        .map(|index| {
            VertexId::mint(format!("cadir:model:vertex#adjacent.{index}"))
                .expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let edge_ids = (0..7)
        .map(|index| {
            EdgeId::mint(format!("cadir:model:edge#adjacent.{index}")).expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let curve_ids = (0..7)
        .map(|index| {
            CurveId::mint(format!("cadir:model:curve#adjacent.{index}")).expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let coedge_ids = (0..8)
        .map(|index| {
            CoedgeId::mint(format!("cadir:model:coedge#adjacent.{index}"))
                .expect("identity grammar")
        })
        .collect::<Vec<_>>();
    ir.geometry.bodies.push(Body {
        id: body.clone(),
        kind: BodyKind::Sheet,
        transform: None,
        name: None,
        color: None,
        visible: None,
        native_ref: None,
    });
    ir.geometry.regions.push(Region {
        id: region.clone(),
        body,
        shells: vec![shell.clone()],
        native_ref: None,
    });
    ir.geometry.shells.push(Shell::new(shell.clone(), region));
    for index in 0..2 {
        ir.geometry.faces.push(Face {
            id: face_ids[index].clone(),
            shell: shell.clone(),
            surface: surface_ids[index].clone(),
            sense: Sense::Forward,
            loops: cadmpeg_ir::topology::FaceLoops::unspecified(vec![loop_ids[index].clone()]),
            name: None,
            color: None,
            tolerance: None,
            native_ref: None,
        });
        ir.geometry.surfaces.push(Surface {
            id: surface_ids[index].clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    positions[0],
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            )),
            source_object: None,
            native_ref: None,
        });
    }
    ir.geometry.loops.push(Loop {
        id: loop_ids[0].clone(),
        face: face_ids[0].clone(),
        boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
            cadmpeg_ir::topology::LoopRing::new(coedge_ids[0..4].to_vec(), Vec::new())
                .expect("valid loop ring"),
        ),
        native_ref: None,
    });
    ir.geometry.loops.push(Loop {
        id: loop_ids[1].clone(),
        face: face_ids[1].clone(),
        boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
            cadmpeg_ir::topology::LoopRing::new(coedge_ids[4..8].to_vec(), Vec::new())
                .expect("valid loop ring"),
        ),
        native_ref: None,
    });
    for index in 0..positions.len() {
        ir.geometry.points.push(Point::new(
            point_ids[index].clone(),
            cadmpeg_ir::features::geometry::FinitePoint3::new(positions[index])
                .expect("a finite position is a point"),
            None,
        ));
        ir.geometry.vertices.push(Vertex {
            id: vertex_ids[index].clone(),
            point: point_ids[index].clone(),
            tolerance: None,
            native_ref: None,
        });
    }
    let endpoints = [(0, 1), (1, 2), (2, 3), (3, 0), (1, 4), (4, 5), (5, 2)];
    for (index, (start, end)) in endpoints.into_iter().enumerate() {
        let delta = Vector3::new(
            positions[end].x - positions[start].x,
            positions[end].y - positions[start].y,
            positions[end].z - positions[start].z,
        );
        ir.geometry.curves.push(Curve {
            parameter_range: None,
            id: curve_ids[index].clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                    Point3::new(
                        positions[start].x - 2.0 * delta.x,
                        positions[start].y - 2.0 * delta.y,
                        positions[start].z - 2.0 * delta.z,
                    ),
                    delta,
                )
                .unwrap(),
            )),
            source_object: None,
            native_ref: None,
        });
        ir.geometry.edges.push(Edge {
            id: edge_ids[index].clone(),
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                Some(curve_ids[index].clone()),
                Some([2.0, 3.0]),
            )
            .unwrap(),
            ends: Some(cadmpeg_ir::topology::EdgeEnds::new(
                vertex_ids[start].clone(),
                vertex_ids[end].clone(),
            )),
            tolerance: None,
            native_ref: None,
        });
    }
    let uses = [
        (0, Sense::Forward),
        (1, Sense::Forward),
        (2, Sense::Forward),
        (3, Sense::Forward),
        (4, Sense::Forward),
        (5, Sense::Forward),
        (6, Sense::Forward),
        (1, Sense::Reversed),
    ];
    for (index, (edge, sense)) in uses.into_iter().enumerate() {
        let radial_next = if index == 1 {
            7
        } else if index == 7 {
            1
        } else {
            index
        };
        ir.geometry.coedges.push(Coedge {
            id: coedge_ids[index].clone(),
            owner_loop: loop_ids[usize::from(index >= 4)].clone(),
            edge: edge_ids[edge].clone(),
            radial_next: coedge_ids[radial_next].clone(),
            sense,
            pcurves: Vec::new(),
            use_curve: None,
            native_ref: None,
        });
    }
    ir.finalize();
    ir
}

fn planar_tetrahedron() -> CadIr {
    use cadmpeg_ir::geometry::{
        Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{
        BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, RegionId, ShellId, SurfaceId,
        VertexId,
    };
    use cadmpeg_ir::math::Vector3;
    use cadmpeg_ir::topology::{
        Body, BodyKind, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
    };

    let mut ir = CadIr::empty();
    let body: BodyId = "cadir:model:body#tetrahedron"
        .try_into()
        .expect("valid identity");
    let region: RegionId = "cadir:model:region#tetrahedron"
        .try_into()
        .expect("valid identity");
    let shell: ShellId = "cadir:model:shell#tetrahedron"
        .try_into()
        .expect("valid identity");
    let positions = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
        Point3::new(0.0, 0.0, 1.0),
    ];
    let point_ids = (0..4)
        .map(|index| {
            PointId::mint(format!("cadir:model:point#tetrahedron.{index}"))
                .expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let vertex_ids = (0..4)
        .map(|index| {
            VertexId::mint(format!("cadir:model:vertex#tetrahedron.{index}"))
                .expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let edge_ids = (0..6)
        .map(|index| {
            EdgeId::mint(format!("cadir:model:edge#tetrahedron.{index}")).expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let curve_ids = (0..6)
        .map(|index| {
            CurveId::mint(format!("cadir:model:curve#tetrahedron.{index}"))
                .expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let face_ids = (0..4)
        .map(|index| {
            FaceId::mint(format!("cadir:model:face#tetrahedron.{index}")).expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let loop_ids = (0..4)
        .map(|index| {
            LoopId::mint(format!("cadir:model:loop#tetrahedron.{index}")).expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let surface_ids = (0..4)
        .map(|index| {
            SurfaceId::mint(format!("cadir:model:surface#tetrahedron.{index}"))
                .expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let coedge_ids = (0..12)
        .map(|index| {
            CoedgeId::mint(format!("cadir:model:coedge#tetrahedron.{index}"))
                .expect("identity grammar")
        })
        .collect::<Vec<_>>();
    ir.geometry.bodies.push(Body {
        id: body.clone(),
        kind: BodyKind::Solid,
        transform: None,
        name: Some("tetrahedron".into()),
        color: None,
        visible: Some(true),
        native_ref: None,
    });
    ir.geometry.regions.push(Region {
        id: region.clone(),
        body,
        shells: vec![shell.clone()],
        native_ref: None,
    });
    ir.geometry.shells.push(Shell::new(shell.clone(), region));
    for index in 0..4 {
        ir.geometry.points.push(Point::new(
            point_ids[index].clone(),
            cadmpeg_ir::features::geometry::FinitePoint3::new(positions[index])
                .expect("a finite position is a point"),
            None,
        ));
        ir.geometry.vertices.push(Vertex {
            id: vertex_ids[index].clone(),
            point: point_ids[index].clone(),
            tolerance: None,
            native_ref: None,
        });
    }
    let endpoints = [(0, 1), (1, 2), (2, 0), (0, 3), (1, 3), (2, 3)];
    for (index, (start, end)) in endpoints.into_iter().enumerate() {
        let delta = Vector3::new(
            positions[end].x - positions[start].x,
            positions[end].y - positions[start].y,
            positions[end].z - positions[start].z,
        );
        let length = delta.norm();
        let direction = Vector3::new(delta.x / length, delta.y / length, delta.z / length);
        ir.geometry.curves.push(Curve {
            parameter_range: None,
            id: curve_ids[index].clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                    Point3::new(
                        positions[start].x - 2.0 * direction.x,
                        positions[start].y - 2.0 * direction.y,
                        positions[start].z - 2.0 * direction.z,
                    ),
                    direction,
                )
                .unwrap(),
            )),
            source_object: None,
            native_ref: None,
        });
        ir.geometry.edges.push(Edge {
            id: edge_ids[index].clone(),
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                Some(curve_ids[index].clone()),
                Some([2.0, 2.0 + length]),
            )
            .unwrap(),
            ends: Some(cadmpeg_ir::topology::EdgeEnds::new(
                vertex_ids[start].clone(),
                vertex_ids[end].clone(),
            )),
            tolerance: None,
            native_ref: None,
        });
    }
    let inverse_sqrt_2 = 1.0 / 2.0_f64.sqrt();
    let inverse_sqrt_3 = 1.0 / 3.0_f64.sqrt();
    let planes = [
        (Vector3::new(0.0, 0.0, -1.0), Vector3::new(1.0, 0.0, 0.0)),
        (Vector3::new(0.0, -1.0, 0.0), Vector3::new(1.0, 0.0, 0.0)),
        (
            Vector3::new(inverse_sqrt_3, inverse_sqrt_3, inverse_sqrt_3),
            Vector3::new(-inverse_sqrt_2, inverse_sqrt_2, 0.0),
        ),
        (Vector3::new(-1.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)),
    ];
    let face_uses = [
        [
            (2, Sense::Reversed),
            (1, Sense::Reversed),
            (0, Sense::Reversed),
        ],
        [
            (0, Sense::Forward),
            (4, Sense::Forward),
            (3, Sense::Reversed),
        ],
        [
            (1, Sense::Forward),
            (5, Sense::Forward),
            (4, Sense::Reversed),
        ],
        [
            (3, Sense::Forward),
            (5, Sense::Reversed),
            (2, Sense::Forward),
        ],
    ];
    for face in 0..4 {
        let start = face * 3;
        ir.geometry.faces.push(Face {
            id: face_ids[face].clone(),
            shell: shell.clone(),
            surface: surface_ids[face].clone(),
            sense: Sense::Forward,
            loops: cadmpeg_ir::topology::FaceLoops::unspecified(vec![loop_ids[face].clone()]),
            name: None,
            color: None,
            tolerance: None,
            native_ref: None,
        });
        ir.geometry.loops.push(Loop {
            id: loop_ids[face].clone(),
            face: face_ids[face].clone(),
            boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
                cadmpeg_ir::topology::LoopRing::new(
                    coedge_ids[start..start + 3].to_vec(),
                    Vec::new(),
                )
                .expect("valid loop ring"),
            ),
            native_ref: None,
        });
        ir.geometry.surfaces.push(Surface {
            id: surface_ids[face].clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    positions[face_uses[face][0].0],
                    planes[face].0,
                    planes[face].1,
                )
                .unwrap(),
            )),
            source_object: None,
            native_ref: None,
        });
        for offset in 0..3 {
            let index = start + offset;
            ir.geometry.coedges.push(Coedge {
                id: coedge_ids[index].clone(),
                owner_loop: loop_ids[face].clone(),
                edge: edge_ids[face_uses[face][offset].0].clone(),
                radial_next: coedge_ids[index].clone(),
                sense: face_uses[face][offset].1,
                pcurves: Vec::new(),
                use_curve: None,
                native_ref: None,
            });
        }
    }
    for edge_id in &edge_ids {
        let uses = ir
            .geometry
            .coedges
            .iter()
            .enumerate()
            .filter(|(_, coedge)| coedge.edge == *edge_id)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        ir.geometry.coedges[uses[0]].radial_next = coedge_ids[uses[1]].clone();
        ir.geometry.coedges[uses[1]].radial_next = coedge_ids[uses[0]].clone();
    }
    ir.finalize();
    ir
}

fn rectangular_nurbs_patch() -> CadIr {
    use cadmpeg_ir::geometry::{
        nurbs::{NurbsCurve, NurbsSurface},
        pcurve::{Pcurve, PcurveGeometry},
        CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
    };

    let points = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(3.0, 0.0, 0.0),
        Point3::new(3.0, 2.0, 1.0),
        Point3::new(0.0, 2.0, 0.0),
    ];
    let mut ir = polygon_sheet(&points);
    ir.geometry.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
        NurbsSurface::from_lanes(
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(1, vec![2.0, 2.0, 5.0, 5.0], false),
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                1,
                vec![7.0, 7.0, 11.0, 11.0],
                false,
            ),
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(
                vec![vec![points[0], points[3]], vec![points[1], points[2]]],
                Some(vec![1.0, 0.8, 1.2, 1.0])
                    .map(|values| values.chunks(2_usize).map(<[_]>::to_vec).collect()),
            ),
            false,
        )
        .expect("valid patch surface"),
    ));
    let edge_data = [
        (
            [20.0, 23.0],
            vec![points[0], points[1]],
            vec![1.0, 1.2],
            cadmpeg_ir::math::Point2::new(-18.0, 7.0),
            cadmpeg_ir::math::Point2::new(1.0, 0.0),
        ),
        (
            [30.0, 32.0],
            vec![points[1], points[2]],
            vec![1.2, 1.0],
            cadmpeg_ir::math::Point2::new(5.0, -53.0),
            cadmpeg_ir::math::Point2::new(0.0, 2.0),
        ),
        (
            [40.0, 43.0],
            vec![points[2], points[3]],
            vec![1.0, 0.8],
            cadmpeg_ir::math::Point2::new(45.0, 11.0),
            cadmpeg_ir::math::Point2::new(-1.0, 0.0),
        ),
        (
            [50.0, 52.0],
            vec![points[3], points[0]],
            vec![0.8, 1.0],
            cadmpeg_ir::math::Point2::new(2.0, 111.0),
            cadmpeg_ir::math::Point2::new(0.0, -2.0),
        ),
    ];
    for (index, (domain, control_points, weights, origin, direction)) in
        edge_data.into_iter().enumerate()
    {
        ir.geometry.edges[index].carrier = cadmpeg_ir::topology::EdgeCarrier::new(
            ir.geometry.edges[index].curve().cloned(),
            Some(domain),
        )
        .unwrap();
        ir.geometry.curves[index].geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            NurbsCurve::from_lanes(
                1,
                vec![domain[0], domain[0], domain[1], domain[1]],
                control_points,
                Some(weights),
                false,
            )
            .expect("valid patch edge"),
        ));
        let id: cadmpeg_ir::ids::PcurveId = format!("cadir:model:pcurve#patch.{index}")
            .try_into()
            .expect("valid identity");
        ir.geometry.pcurves.push(Pcurve {
            id: id.clone(),
            geometry: PcurveGeometry::Line(
                cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(origin, direction).unwrap(),
            ),
            metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::general(
                None,
                Some(cadmpeg_ir::units::FiniteVector::new(domain).expect("finite fixture range")),
                Some(
                    cadmpeg_ir::geometry::legacy_cache::FitTolerance::try_new(0.001)
                        .expect("finite non-negative fixture tolerance"),
                ),
            ),
            native_ref: None,
        });
        ir.geometry.coedges[index].pcurves = vec![cadmpeg_ir::topology::PcurveUse {
            pcurve: id,
            isoparametric: None,
            parameter_range: None,
        }];
    }
    ir.finalize();
    ir
}

fn mixed_plane_nurbs_sheet() -> CadIr {
    use cadmpeg_ir::geometry::{
        nurbs::{NurbsCurve, NurbsSurface},
        pcurve::{Pcurve, PcurveGeometry},
        CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
    };

    let mut ir = adjacent_quad_sheet();
    let points = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(1.0, 1.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
    ];
    ir.geometry.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
        NurbsSurface::from_lanes(
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(1, vec![2.0, 2.0, 5.0, 5.0], false),
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                1,
                vec![7.0, 7.0, 11.0, 11.0],
                false,
            ),
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(
                vec![vec![points[0], points[3]], vec![points[1], points[2]]],
                Some(vec![1.0, 0.8, 1.2, 1.0])
                    .map(|values| values.chunks(2_usize).map(<[_]>::to_vec).collect()),
            ),
            false,
        )
        .expect("valid mixed surface"),
    ));
    let edge_data = [
        (
            [20.0, 23.0],
            vec![points[0], points[1]],
            vec![1.0, 1.2],
            cadmpeg_ir::math::Point2::new(-18.0, 7.0),
            cadmpeg_ir::math::Point2::new(1.0, 0.0),
        ),
        (
            [30.0, 32.0],
            vec![points[1], points[2]],
            vec![1.2, 1.0],
            cadmpeg_ir::math::Point2::new(5.0, -53.0),
            cadmpeg_ir::math::Point2::new(0.0, 2.0),
        ),
        (
            [40.0, 43.0],
            vec![points[2], points[3]],
            vec![1.0, 0.8],
            cadmpeg_ir::math::Point2::new(45.0, 11.0),
            cadmpeg_ir::math::Point2::new(-1.0, 0.0),
        ),
        (
            [50.0, 52.0],
            vec![points[3], points[0]],
            vec![0.8, 1.0],
            cadmpeg_ir::math::Point2::new(2.0, 111.0),
            cadmpeg_ir::math::Point2::new(0.0, -2.0),
        ),
    ];
    for (index, (domain, control_points, weights, origin, direction)) in
        edge_data.into_iter().enumerate()
    {
        ir.geometry.edges[index].carrier = cadmpeg_ir::topology::EdgeCarrier::new(
            ir.geometry.edges[index].curve().cloned(),
            Some(domain),
        )
        .unwrap();
        ir.geometry.curves[index].geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            NurbsCurve::from_lanes(
                1,
                vec![domain[0], domain[0], domain[1], domain[1]],
                control_points,
                Some(weights),
                false,
            )
            .expect("valid mixed edge"),
        ));
        let id: cadmpeg_ir::ids::PcurveId = format!("cadir:model:pcurve#mixed.{index}")
            .try_into()
            .expect("valid identity");
        ir.geometry.pcurves.push(Pcurve {
            id: id.clone(),
            geometry: PcurveGeometry::Line(
                cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(origin, direction).unwrap(),
            ),
            metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::general(
                None,
                Some(cadmpeg_ir::units::FiniteVector::new(domain).expect("finite fixture range")),
                Some(
                    cadmpeg_ir::geometry::legacy_cache::FitTolerance::try_new(0.001)
                        .expect("finite non-negative fixture tolerance"),
                ),
            ),
            native_ref: None,
        });
        ir.geometry.coedges[index].pcurves = vec![cadmpeg_ir::topology::PcurveUse {
            pcurve: id,
            isoparametric: None,
            parameter_range: None,
        }];
    }
    ir.finalize();
    ir
}

fn make_planar_nurbs_trimmed_face(ir: &mut CadIr) {
    use cadmpeg_ir::geometry::{
        nurbs::NurbsSurface,
        pcurve::{Pcurve, PcurveGeometry},
        SolvedSurfaceGeometry, SurfaceGeometry,
    };

    ir.geometry.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
        NurbsSurface::from_lanes(
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 4.0, 4.0], false),
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 4.0, 4.0], false),
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(
                vec![
                    vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 4.0, 0.0)],
                    vec![Point3::new(4.0, 0.0, 0.0), Point3::new(4.0, 4.0, 0.0)],
                ],
                None,
            ),
            false,
        )
        .expect("valid planar patch"),
    ));
    for index in 0..ir.geometry.coedges.len() {
        let coedge = &ir.geometry.coedges[index];
        let edge = ir
            .geometry
            .edges
            .iter()
            .find(|edge| edge.id == coedge.edge)
            .expect("fixture edge");
        let domain = edge.param_range().expect("fixture edge domain");
        let (start, end) = if coedge.sense == cadmpeg_ir::topology::Sense::Forward {
            (
                &edge.ends.as_ref().unwrap().start,
                &edge.ends.as_ref().unwrap().end,
            )
        } else {
            (
                &edge.ends.as_ref().unwrap().end,
                &edge.ends.as_ref().unwrap().start,
            )
        };
        let vertex_point = |id: &cadmpeg_ir::ids::VertexId| {
            let vertex = ir
                .geometry
                .vertices
                .iter()
                .find(|vertex| vertex.id == *id)?;
            ir.geometry
                .points
                .iter()
                .find(|point| point.id == vertex.point)
                .map(|point| point.position().get())
        };
        let start = vertex_point(start).expect("fixture start");
        let end = vertex_point(end).expect("fixture end");
        let scale = domain[1] - domain[0];
        let direction =
            cadmpeg_ir::math::Point2::new((end.x - start.x) / scale, (end.y - start.y) / scale);
        let origin = cadmpeg_ir::math::Point2::new(
            start.x - direction.u * domain[0],
            start.y - direction.v * domain[0],
        );
        let id: cadmpeg_ir::ids::PcurveId = format!("cadir:model:pcurve#general.{index}")
            .try_into()
            .expect("valid identity");
        ir.geometry.pcurves.push(Pcurve {
            id: id.clone(),
            geometry: PcurveGeometry::Line(
                cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(origin, direction).unwrap(),
            ),
            metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::general(
                None,
                Some(domain),
                Some(
                    cadmpeg_ir::geometry::legacy_cache::FitTolerance::try_new(0.0001)
                        .expect("finite non-negative fixture tolerance"),
                ),
            ),
            native_ref: None,
        });
        ir.geometry.coedges[index].pcurves = vec![cadmpeg_ir::topology::PcurveUse {
            pcurve: id,
            isoparametric: None,
            parameter_range: None,
        }];
    }
    ir.finalize();
}

mod trim_domain;
