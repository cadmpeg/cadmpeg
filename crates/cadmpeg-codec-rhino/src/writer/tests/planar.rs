// SPDX-License-Identifier: Apache-2.0

use super::add_polygon_hole;
use super::adjacent_quad_sheet;
use super::assert_planar_sheet_round_trip;
use super::planar_tetrahedron;
use super::polygon_sheet;
use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::EncodeInput;
use std::io::Cursor;

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use cadmpeg_ir::ids::PointId;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::Point;

use crate::{RhinoArchiveVersion, RhinoCodec};

#[test]
fn planar_triangle_sheet_round_trips_connected_topology() {
    let ir = polygon_sheet(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 2.0, 0.0),
    ]);
    assert_planar_sheet_round_trip(&ir, 1, 3);
}

#[test]
fn planar_quad_sheet_round_trips_connected_topology() {
    let ir = polygon_sheet(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(3.0, 0.0, 0.0),
        Point3::new(3.0, 2.0, 0.0),
        Point3::new(0.0, 2.0, 0.0),
    ]);
    assert_planar_sheet_round_trip(&ir, 1, 4);
}

#[test]
fn planar_sheet_round_trips_object_attributes() {
    let mut ir = polygon_sheet(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 2.0, 0.0),
    ]);
    ir.model.bodies[0].name = Some("named sheet".into());
    ir.model.bodies[0].color = Some(
        cadmpeg_ir::topology::Color::new(64.0 / 255.0, 128.0 / 255.0, 1.0, 192.0 / 255.0)
            .expect("valid color"),
    );
    ir.model.bodies[0].visible = Some(false);
    for version in [
        RhinoArchiveVersion::V5,
        RhinoArchiveVersion::V6,
        RhinoArchiveVersion::V7,
        RhinoArchiveVersion::V8,
    ] {
        let mut bytes = Vec::new();
        RhinoCodec
            .plan(
                EncodeInput::new(&ir, None),
                TargetRequest::Explicit(version.descriptor().id.as_str()),
            )
            .and_then(|plan| plan.write_to(&mut bytes))
            .expect("required invariant");
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("required invariant");
        let body = &decoded.ir().model.bodies[0];
        assert_eq!(body.name.as_deref(), Some("named sheet"), "{version:?}");
        assert_eq!(body.color, ir.model.bodies[0].color, "{version:?}");
        assert_eq!(body.visible, Some(false), "{version:?}");
    }
}

#[test]
fn planar_sheet_with_hole_round_trips_connected_topology() {
    let mut ir = polygon_sheet(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(4.0, 0.0, 0.0),
        Point3::new(4.0, 3.0, 0.0),
        Point3::new(0.0, 3.0, 0.0),
    ]);
    add_polygon_hole(
        &mut ir,
        &[
            Point3::new(1.0, 1.0, 0.0),
            Point3::new(1.0, 2.0, 0.0),
            Point3::new(3.0, 2.0, 0.0),
            Point3::new(3.0, 1.0, 0.0),
        ],
    );
    assert_planar_sheet_round_trip(&ir, 2, 8);
}

#[test]
fn adjacent_planar_faces_round_trip_shared_edge_and_domains() {
    let ir = adjacent_quad_sheet();
    for version in [
        RhinoArchiveVersion::V5,
        RhinoArchiveVersion::V6,
        RhinoArchiveVersion::V7,
        RhinoArchiveVersion::V8,
    ] {
        let mut bytes = Vec::new();
        RhinoCodec
            .plan(
                EncodeInput::new(&ir, None),
                TargetRequest::Explicit(version.descriptor().id.as_str()),
            )
            .and_then(|plan| plan.write_to(&mut bytes))
            .expect("required invariant");
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("required invariant");
        assert_eq!(decoded.ir().model.bodies.len(), 1, "{version:?}");
        assert_eq!(
            decoded.ir().model.bodies[0].kind,
            cadmpeg_ir::topology::BodyKind::Sheet,
            "{version:?}"
        );
        assert_eq!(decoded.ir().model.shells.len(), 1, "{version:?}");
        assert_eq!(decoded.ir().model.faces.len(), 2, "{version:?}");
        assert_eq!(decoded.ir().model.loops.len(), 2, "{version:?}");
        assert_eq!(decoded.ir().model.coedges.len(), 8, "{version:?}");
        assert_eq!(decoded.ir().model.edges.len(), 7, "{version:?}");
        assert_eq!(decoded.ir().model.vertices.len(), 6, "{version:?}");
        assert!(decoded.ir().model.edges.iter().all(|edge| {
            edge.param_range().map(cadmpeg_ir::units::FiniteVector::get) == Some([2.0, 3.0])
        }));
        let shared = decoded
            .ir()
            .model
            .edges
            .iter()
            .find(|edge| {
                decoded
                    .ir()
                    .model
                    .coedges
                    .iter()
                    .filter(|coedge| coedge.edge == edge.id)
                    .count()
                    == 2
            })
            .expect("one shared edge");
        let uses = decoded
            .ir()
            .model
            .coedges
            .iter()
            .filter(|coedge| coedge.edge == shared.id)
            .collect::<Vec<_>>();
        assert_ne!(uses[0].sense, uses[1].sense);
        assert_eq!(uses[0].radial_next, uses[1].id);
        assert_eq!(uses[1].radial_next, uses[0].id);
        assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
            .expect("validate the neutral model")
            .is_ok());
    }
}

#[test]
fn planar_tetrahedron_round_trips_as_closed_solid() {
    let ir = planar_tetrahedron();
    for version in [
        RhinoArchiveVersion::V5,
        RhinoArchiveVersion::V6,
        RhinoArchiveVersion::V7,
        RhinoArchiveVersion::V8,
    ] {
        let mut bytes = Vec::new();
        RhinoCodec
            .plan(
                EncodeInput::new(&ir, None),
                TargetRequest::Explicit(version.descriptor().id.as_str()),
            )
            .and_then(|plan| plan.write_to(&mut bytes))
            .expect("required invariant");
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("required invariant");
        assert_eq!(decoded.ir().model.bodies.len(), 1, "{version:?}");
        assert_eq!(
            decoded.ir().model.bodies[0].kind,
            cadmpeg_ir::topology::BodyKind::Solid,
            "{version:?}"
        );
        assert_eq!(decoded.ir().model.shells.len(), 1, "{version:?}");
        assert_eq!(decoded.ir().model.faces.len(), 4, "{version:?}");
        assert_eq!(decoded.ir().model.loops.len(), 4, "{version:?}");
        assert_eq!(decoded.ir().model.coedges.len(), 12, "{version:?}");
        assert_eq!(decoded.ir().model.edges.len(), 6, "{version:?}");
        assert_eq!(decoded.ir().model.vertices.len(), 4, "{version:?}");
        for (actual, expected) in decoded.ir().model.edges.iter().zip(&ir.model.edges) {
            assert_eq!(actual.param_range(), expected.param_range(), "{version:?}");
            assert_eq!(
                decoded
                    .ir()
                    .model
                    .coedges
                    .iter()
                    .filter(|coedge| coedge.edge == actual.id)
                    .count(),
                2,
                "{version:?}"
            );
        }
        assert!(decoded
            .ir()
            .model
            .coedges
            .iter()
            .all(|coedge| coedge.radial_next != coedge.id));
        assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
            .expect("validate the neutral model")
            .is_ok());
    }
}

#[test]
fn multiple_brep_objects_round_trip_in_one_archive() {
    let mut ir = polygon_sheet(&[
        Point3::new(-2.0, 0.0, 0.0),
        Point3::new(-1.0, 0.0, 0.0),
        Point3::new(-1.5, 1.0, 0.0),
    ]);
    let mut adjacent = adjacent_quad_sheet();
    ir.model.bodies.append(&mut adjacent.model.bodies);
    ir.model.regions.append(&mut adjacent.model.regions);
    ir.model.shells.append(&mut adjacent.model.shells);
    ir.model.faces.append(&mut adjacent.model.faces);
    ir.model.loops.append(&mut adjacent.model.loops);
    ir.model.coedges.append(&mut adjacent.model.coedges);
    ir.model.edges.append(&mut adjacent.model.edges);
    ir.model.vertices.append(&mut adjacent.model.vertices);
    ir.model.points.append(&mut adjacent.model.points);
    ir.model.surfaces.append(&mut adjacent.model.surfaces);
    ir.model.curves.append(&mut adjacent.model.curves);
    ir.model.pcurves.append(&mut adjacent.model.pcurves);
    ir.finalize();
    for version in [
        RhinoArchiveVersion::V5,
        RhinoArchiveVersion::V6,
        RhinoArchiveVersion::V7,
        RhinoArchiveVersion::V8,
    ] {
        let mut bytes = Vec::new();
        RhinoCodec
            .plan(
                EncodeInput::new(&ir, None),
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
                .all(|loss| !loss.message.contains("CRC mismatch")),
            "{version:?}: {:?}",
            decoded.report().losses
        );
        assert_eq!(decoded.ir().model.bodies.len(), 2, "{version:?}");
        assert_eq!(decoded.ir().model.faces.len(), 3, "{version:?}");
        assert_eq!(decoded.ir().model.edges.len(), 10, "{version:?}");
        assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
            .expect("validate the neutral model")
            .is_ok());
    }
}

#[test]
fn brep_and_free_geometry_round_trip_in_one_archive() {
    use cadmpeg_ir::geometry::{
        Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{CurveId, SurfaceId};
    use cadmpeg_ir::math::Vector3;

    let mut ir = polygon_sheet(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 2.0, 0.0),
    ]);
    ir.model.points.push(Point::new(
        PointId::mint("cadir:model:point#free").expect("identity grammar"),
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(5.0, 6.0, 7.0))
            .expect("a finite position is a point"),
        None,
    ));
    ir.model.curves.push(Curve {
        native_ref: None,
        parameter_range: None,
        id: CurveId::mint("cadir:model:curve#free").expect("identity grammar"),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                Point3::new(5.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                2.0,
            )
            .unwrap(),
        )),
        source_object: None,
    });
    ir.model.surfaces.push(Surface {
        native_ref: None,
        id: SurfaceId::mint("cadir:model:surface#free").expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 3.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    });
    ir.finalize();
    for version in [
        RhinoArchiveVersion::V5,
        RhinoArchiveVersion::V6,
        RhinoArchiveVersion::V7,
        RhinoArchiveVersion::V8,
    ] {
        let mut bytes = Vec::new();
        RhinoCodec
            .plan(
                EncodeInput::new(&ir, None),
                TargetRequest::Explicit(version.descriptor().id.as_str()),
            )
            .and_then(|plan| plan.write_to(&mut bytes))
            .expect("required invariant");
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("required invariant");
        assert_eq!(decoded.ir().model.bodies.len(), 2, "{version:?}");
        assert!(decoded
            .ir()
            .model
            .points
            .iter()
            .any(|point| point.position().get() == Point3::new(5.0, 6.0, 7.0)));
        assert!(decoded
            .ir()
            .model
            .curves
            .iter()
            .any(|curve| matches!(curve.geometry, CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) if { circle_curve.radius().get() == 2.0 })));
        assert!(decoded.ir().model.surfaces.iter().any(
            |surface| matches!(surface.geometry, SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface))
            if {
                let origin = plane_surface.origin();
                origin.z == 3.0
            })
        ));
        let validation = cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
            .expect("validate the neutral model");
        assert!(validation.is_ok(), "{version:?}: {validation:#?}");
    }
}

#[test]
fn open_planar_solid_omits_its_body_with_a_named_loss() {
    let mut ir = adjacent_quad_sheet();
    ir.model.bodies[0].kind = cadmpeg_ir::topology::BodyKind::Solid;
    let mut output = Vec::new();
    let report = RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut output))
        .expect("an open solid omits its body");
    assert!(!output.is_empty(), "the remaining document is written");
    let loss = report
        .losses
        .iter()
        .find(|loss| {
            loss.code.namespace() == "rhino" && loss.code.local_code() == "writer.body-omitted"
        })
        .expect("the omission names its record");
    assert!(loss.message.contains("incidence"), "{loss:?}");
}

/// A stated tolerance finer than the Rhino Brep write route resolves.
const EPS_BELOW_WRITE_BOUND: f64 = 1.0e-12;

/// Decimal rendering of `EPS_BELOW_WRITE_BOUND` in a refusal message.
const EPS_BELOW_WRITE_BOUND_TEXT: &str = "0.000000000001";

#[test]
fn sub_resolution_edge_tolerance_omits_sheet_face_without_widening() {
    let mut ir = polygon_sheet(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 2.0, 0.0),
    ]);
    ir.model.edges[0].tolerance = Some(
        cadmpeg_ir::scalar::PositiveReal::new(EPS_BELOW_WRITE_BOUND)
            .expect("positive finite tolerance"),
    );
    let mut output = Vec::new();
    let report = RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut output))
        .expect("unrepresentable tolerance omits only its sheet face");
    let loss = report
        .losses
        .iter()
        .find(|loss| loss.code == crate::loss::RhinoLossCode::WriterEntityOmitted.kind())
        .expect("sheet-face omission is reported");
    assert_eq!(loss.severity, cadmpeg_ir::report::Severity::Error);
    let message = &loss.message;
    assert!(message.contains(EPS_BELOW_WRITE_BOUND_TEXT), "{message}");
    assert!(message.contains("edge#polygon.0"), "{message}");
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("empty archive is valid");
    assert!(decoded.ir().model.bodies.is_empty());
}

#[test]
fn sub_resolution_face_tolerance_omits_sheet_face_without_widening() {
    let mut ir = polygon_sheet(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 2.0, 0.0),
    ]);
    ir.model.faces[0].tolerance = Some(
        cadmpeg_ir::scalar::PositiveReal::new(EPS_BELOW_WRITE_BOUND)
            .expect("positive finite tolerance"),
    );
    let mut output = Vec::new();
    let report = RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut output))
        .expect("unrepresentable tolerance omits only its sheet face");
    let loss = report
        .losses
        .iter()
        .find(|loss| loss.code == crate::loss::RhinoLossCode::WriterEntityOmitted.kind())
        .expect("sheet-face omission is reported");
    assert_eq!(loss.severity, cadmpeg_ir::report::Severity::Error);
    let message = &loss.message;
    assert!(message.contains(EPS_BELOW_WRITE_BOUND_TEXT), "{message}");
    assert!(message.contains("face#polygon"), "{message}");
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("empty archive is valid");
    assert!(decoded.ir().model.bodies.is_empty());
}

#[test]
fn planar_solid_orientation_is_translation_and_scale_invariant() {
    use crate::writer::{model::WritableModel, planar_solid_orientation};
    let ir = planar_tetrahedron();
    for (scale, translation) in [(1.0, 0.0), (1.0, 1e8), (1e200, 0.0), (1e-200, 0.0)] {
        let mut model = WritableModel::try_new(&ir).unwrap();
        // The sign owner consumes the oriented vertex rings; other writer fields
        // do not enter this calculation.
        for vertex in &mut model.vertices {
            vertex.point = Point3::new(
                2.0 * scale * vertex.point.x + translation,
                scale * vertex.point.y + translation,
                scale * vertex.point.z + translation,
            );
        }
        assert_eq!(planar_solid_orientation(&model), 1);
        for loop_ in &mut model.loops {
            loop_.coedges.reverse();
        }
        assert_eq!(planar_solid_orientation(&model), 2);
    }
}

#[test]
fn reversed_trim_with_edge_convention_pcurve_is_admitted() {
    // From `core_operations.FCStd` ChamferBase body: a reversed coedge whose
    // explicit line pcurve shares the edge parametrization (edge convention)
    // instead of Rhino's traversal convention. The writer keeps the authored
    // carrier, so the agreement admits the trim.
    use super::super::trims::validate_plane_trim;
    use crate::writer::model::{WritableEdge, WritableEdgeCurve, WritablePcurve};
    use cadmpeg_ir::geometry::{
        analytic::LineCurve,
        pcurve::{LinePcurve, Pcurve, PcurveGeometry, PcurveMetadata},
    };
    use cadmpeg_ir::math::{Point2, Vector3};
    use cadmpeg_ir::topology::Sense;
    let line = LineCurve::new(
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
            .expect("finite fixture origin"),
        cadmpeg_ir::units::UnitVector3::new(Vector3::new(0.0, 1.0, 0.0))
            .expect("unit fixture direction"),
    );
    let pcurve = Pcurve {
        native_ref: None,
        id: cadmpeg_ir::ids::PcurveId::mint("rhino:test:pcurve#edge-convention")
            .expect("identity grammar"),
        geometry: PcurveGeometry::Line(
            LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(0.0, -1.0))
                .expect("nonzero fixture direction"),
        ),
        metadata: PcurveMetadata::general(None, None, None),
    };
    let source = cadmpeg_ir::examples::unit_cube().expect("fixture document");
    let edge = WritableEdge {
        source: &source.model.edges[0],
        start: 0,
        end: 1,
        domain: [0.0, 12.0].map(|value| {
            cadmpeg_ir::scalar::FiniteReal::new(value).expect("finite fixture domain")
        }),
        curve_id: "edge-convention",
        curve: WritableEdgeCurve::Line(line),
        uses: vec![],
    };
    let explicit = WritablePcurve {
        source: &pcurve,
        payload: ([0; 16], vec![]),
    };
    validate_plane_trim(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(0.0, -1.0, 0.0),
        1e-6,
        &edge,
        Sense::Reversed,
        &explicit,
    )
    .expect("edge-convention trim on a reversed coedge is admitted");
}

/// A disk sheet bounded by one full-circle edge exercises the arc C3 path,
/// the 2D arc C2 path, and the edge-convention trim on a reversed coedge.
#[test]
fn disk_sheet_with_circle_edge_round_trips() {
    use cadmpeg_ir::geometry::{
        analytic::{CircleCurve, PlaneSurface},
        pcurve::{CirclePcurve, Pcurve, PcurveGeometry, PcurveMetadata},
        Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{
        BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
        SurfaceId, VertexId,
    };
    use cadmpeg_ir::math::{Point2, Vector3};
    use cadmpeg_ir::topology::{
        Body, BodyKind, Coedge, Edge, EdgeCarrier, Face, FaceLoops, Loop, LoopBoundary, LoopRing,
        PcurveUse, Point, Region, Sense, Shell, Vertex,
    };
    use cadmpeg_ir::units::FiniteVector;
    use cadmpeg_ir::CadIr;
    const TAU: f64 = std::f64::consts::TAU;
    let mut ir = CadIr::empty();
    let body: BodyId = "cadir:model:body#disk".try_into().unwrap();
    let region: RegionId = "cadir:model:region#disk".try_into().unwrap();
    let shell: ShellId = "cadir:model:shell#disk".try_into().unwrap();
    let face: FaceId = "cadir:model:face#disk".try_into().unwrap();
    let loop_id: LoopId = "cadir:model:loop#disk".try_into().unwrap();
    let surface: SurfaceId = "cadir:model:surface#disk".try_into().unwrap();
    let point: PointId = "cadir:model:point#disk".try_into().unwrap();
    let vertex: VertexId = "cadir:model:vertex#disk".try_into().unwrap();
    let edge: EdgeId = "cadir:model:edge#disk".try_into().unwrap();
    let curve: CurveId = "cadir:model:curve#disk".try_into().unwrap();
    let coedge: CoedgeId = "cadir:model:coedge#disk".try_into().unwrap();
    let pcurve: PcurveId = "cadir:model:pcurve#disk".try_into().unwrap();
    ir.model.bodies.push(Body {
        native_ref: None,
        id: body.clone(),
        kind: BodyKind::Sheet,
        regions: vec![region.clone()],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir.model.regions.push(Region {
        native_ref: None,
        id: region.clone(),
        body,
        shells: vec![shell.clone()],
    });
    ir.model
        .shells
        .push(Shell::with_face(shell.clone(), region, face.clone()));
    ir.model.faces.push(Face {
        native_ref: None,
        id: face.clone(),
        shell,
        surface: surface.clone(),
        sense: Sense::Forward,
        loops: FaceLoops::unspecified(vec![loop_id.clone()]),
        name: None,
        color: None,
        tolerance: None,
    });
    ir.model.loops.push(Loop {
        native_ref: None,
        id: loop_id.clone(),
        face,
        boundary: LoopBoundary::Ring(LoopRing::new(vec![coedge.clone()], Vec::new()).unwrap()),
    });
    ir.model.surfaces.push(Surface {
        native_ref: None,
        id: surface,
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    });
    ir.model.points.push(Point::new(
        point.clone(),
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(2.0, 0.0, 0.0)).unwrap(),
        None,
    ));
    ir.model.vertices.push(Vertex {
        native_ref: None,
        id: vertex.clone(),
        point,
        tolerance: None,
    });
    ir.model.curves.push(Curve {
        native_ref: None,
        parameter_range: None,
        id: curve.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            CircleCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                2.0,
            )
            .unwrap(),
        )),
        source_object: None,
    });
    ir.model.edges.push(Edge {
        native_ref: None,
        id: edge.clone(),
        carrier: EdgeCarrier::new(Some(curve), Some([0.0, TAU])).unwrap(),
        start: vertex.clone(),
        end: vertex,
        tolerance: None,
    });
    ir.model.pcurves.push(Pcurve {
        native_ref: None,
        id: pcurve.clone(),
        geometry: PcurveGeometry::Circle(
            CirclePcurve::try_new(
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 0.0),
                Point2::new(0.0, 1.0),
                2.0,
            )
            .unwrap(),
        ),
        metadata: PcurveMetadata::general(None, Some(FiniteVector::new([0.0, TAU]).unwrap()), None),
    });
    ir.model.coedges.push(Coedge {
        native_ref: None,
        id: coedge.clone(),
        owner_loop: loop_id,
        edge,
        radial_next: coedge,
        sense: Sense::Reversed,
        pcurves: vec![PcurveUse {
            pcurve,
            isoparametric: None,
            parameter_range: None,
        }],
        use_curve: None,
    });
    ir.finalize();
    for version in [
        RhinoArchiveVersion::V5,
        RhinoArchiveVersion::V6,
        RhinoArchiveVersion::V7,
        RhinoArchiveVersion::V8,
    ] {
        let mut bytes = Vec::new();
        RhinoCodec
            .plan(
                EncodeInput::new(&ir, None),
                TargetRequest::Explicit(version.descriptor().id.as_str()),
            )
            .and_then(|plan| plan.write_to(&mut bytes))
            .expect("disk sheet writes");
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("disk sheet re-decodes");
        assert_eq!(decoded.ir().model.bodies.len(), 1, "{version:?}");
        assert_eq!(decoded.ir().model.coedges.len(), 1, "{version:?}");
        assert_eq!(decoded.ir().model.edges.len(), 1, "{version:?}");
        let CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle)) =
            &decoded.ir().model.curves[0].geometry
        else {
            panic!("{version:?}: circle edge re-decodes as a circle");
        };
        assert_eq!(circle.radius().get(), 2.0, "{version:?}");
        let PcurveGeometry::Nurbs { nurbs: trim } = &decoded.ir().model.pcurves[0].geometry else {
            panic!("{version:?}: circle trim re-decodes as a NURBS curve");
        };
        assert_eq!(trim.degree(), 2, "{version:?}");
        assert_eq!(trim.control_points().len(), 9, "{version:?}");
    }
}

/// A closed cylinder solid with plane caps exercises the revolution-surface
/// path for the wall, the arc C3/C2 paths for its rings, and both trim
/// conventions on its reversed coedges.
#[test]
fn cylinder_solid_with_plane_caps_round_trips() {
    use cadmpeg_ir::geometry::{
        analytic::{CircleCurve, CylinderSurface, LineCurve, PlaneSurface},
        pcurve::{CirclePcurve, LinePcurve, Pcurve, PcurveGeometry, PcurveMetadata},
        Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{
        BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
        SurfaceId, VertexId,
    };
    use cadmpeg_ir::math::{Point2, Vector3};
    use cadmpeg_ir::topology::{
        Body, BodyKind, Coedge, Edge, EdgeCarrier, Face, FaceLoops, Loop, LoopBoundary, LoopRing,
        PcurveUse, Point, Region, Sense, Shell, Vertex,
    };
    use cadmpeg_ir::units::FiniteVector;
    use cadmpeg_ir::CadIr;
    const TAU: f64 = std::f64::consts::TAU;
    let mut ir = CadIr::empty();
    let body: BodyId = "cadir:model:body#cylinder".try_into().unwrap();
    let region: RegionId = "cadir:model:region#cylinder".try_into().unwrap();
    let shell: ShellId = "cadir:model:shell#cylinder".try_into().unwrap();
    let wall: FaceId = "cadir:model:face#cylinder.wall".try_into().unwrap();
    let cap_top: FaceId = "cadir:model:face#cylinder.top".try_into().unwrap();
    let cap_bottom: FaceId = "cadir:model:face#cylinder.bottom".try_into().unwrap();
    let wall_loop: LoopId = "cadir:model:loop#cylinder.wall".try_into().unwrap();
    let top_loop: LoopId = "cadir:model:loop#cylinder.top".try_into().unwrap();
    let bottom_loop: LoopId = "cadir:model:loop#cylinder.bottom".try_into().unwrap();
    let wall_surface: SurfaceId = "cadir:model:surface#cylinder.wall".try_into().unwrap();
    let top_surface: SurfaceId = "cadir:model:surface#cylinder.top".try_into().unwrap();
    let bottom_surface: SurfaceId = "cadir:model:surface#cylinder.bottom".try_into().unwrap();
    let top_vertex: VertexId = "cadir:model:vertex#cylinder.top".try_into().unwrap();
    let bottom_vertex: VertexId = "cadir:model:vertex#cylinder.bottom".try_into().unwrap();
    let top_point: PointId = "cadir:model:point#cylinder.top".try_into().unwrap();
    let bottom_point: PointId = "cadir:model:point#cylinder.bottom".try_into().unwrap();
    let top_edge: EdgeId = "cadir:model:edge#cylinder.top".try_into().unwrap();
    let bottom_edge: EdgeId = "cadir:model:edge#cylinder.bottom".try_into().unwrap();
    let seam_edge: EdgeId = "cadir:model:edge#cylinder.seam".try_into().unwrap();
    let top_curve: CurveId = "cadir:model:curve#cylinder.top".try_into().unwrap();
    let bottom_curve: CurveId = "cadir:model:curve#cylinder.bottom".try_into().unwrap();
    let seam_curve: CurveId = "cadir:model:curve#cylinder.seam".try_into().unwrap();
    let wall_top: CoedgeId = "cadir:model:coedge#cylinder.wall-top".try_into().unwrap();
    let wall_seam_down: CoedgeId = "cadir:model:coedge#cylinder.wall-seam-down"
        .try_into()
        .unwrap();
    let wall_bottom: CoedgeId = "cadir:model:coedge#cylinder.wall-bottom"
        .try_into()
        .unwrap();
    let wall_seam_up: CoedgeId = "cadir:model:coedge#cylinder.wall-seam-up"
        .try_into()
        .unwrap();
    let cap_top_coedge: CoedgeId = "cadir:model:coedge#cylinder.cap-top".try_into().unwrap();
    let cap_bottom_coedge: CoedgeId = "cadir:model:coedge#cylinder.cap-bottom".try_into().unwrap();
    let wall_top_pcurve: PcurveId = "cadir:model:pcurve#cylinder.wall-top".try_into().unwrap();
    let wall_seam_pcurve: PcurveId = "cadir:model:pcurve#cylinder.wall-seam".try_into().unwrap();
    let wall_bottom_pcurve: PcurveId = "cadir:model:pcurve#cylinder.wall-bottom"
        .try_into()
        .unwrap();
    let wall_seam_up_pcurve: PcurveId = "cadir:model:pcurve#cylinder.wall-seam-up"
        .try_into()
        .unwrap();
    let cap_top_pcurve: PcurveId = "cadir:model:pcurve#cylinder.cap-top".try_into().unwrap();
    let cap_bottom_pcurve: PcurveId = "cadir:model:pcurve#cylinder.cap-bottom".try_into().unwrap();
    ir.model.bodies.push(Body {
        native_ref: None,
        id: body.clone(),
        kind: BodyKind::Solid,
        regions: vec![region.clone()],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir.model.regions.push(Region {
        native_ref: None,
        id: region.clone(),
        body,
        shells: vec![shell.clone()],
    });
    ir.model.shells.push(
        Shell::with_faces(
            shell.clone(),
            region,
            vec![wall.clone(), cap_top.clone(), cap_bottom.clone()],
        )
        .unwrap(),
    );
    for (face, loops) in [
        (wall.clone(), vec![wall_loop.clone()]),
        (cap_top.clone(), vec![top_loop.clone()]),
        (cap_bottom.clone(), vec![bottom_loop.clone()]),
    ] {
        ir.model.faces.push(Face {
            native_ref: None,
            id: face,
            shell: shell.clone(),
            surface: if loops[0] == wall_loop {
                wall_surface.clone()
            } else if loops[0] == top_loop {
                top_surface.clone()
            } else {
                bottom_surface.clone()
            },
            sense: Sense::Forward,
            loops: FaceLoops::unspecified(loops),
            name: None,
            color: None,
            tolerance: None,
        });
    }
    for (loop_id, face, coedges) in [
        (
            wall_loop.clone(),
            wall.clone(),
            vec![
                wall_top.clone(),
                wall_seam_down.clone(),
                wall_bottom.clone(),
                wall_seam_up.clone(),
            ],
        ),
        (
            top_loop.clone(),
            cap_top.clone(),
            vec![cap_top_coedge.clone()],
        ),
        (
            bottom_loop.clone(),
            cap_bottom.clone(),
            vec![cap_bottom_coedge.clone()],
        ),
    ] {
        ir.model.loops.push(Loop {
            native_ref: None,
            id: loop_id,
            face,
            boundary: LoopBoundary::Ring(LoopRing::new(coedges, Vec::new()).unwrap()),
        });
    }
    ir.model.surfaces.push(Surface {
        native_ref: None,
        id: wall_surface.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            CylinderSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                2.0,
            )
            .unwrap(),
        )),
        source_object: None,
    });
    ir.model.surfaces.push(Surface {
        native_ref: None,
        id: top_surface,
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 5.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    });
    ir.model.surfaces.push(Surface {
        native_ref: None,
        id: bottom_surface,
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, -1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    });
    ir.model.points.push(Point::new(
        top_point.clone(),
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(2.0, 0.0, 5.0)).unwrap(),
        None,
    ));
    ir.model.points.push(Point::new(
        bottom_point.clone(),
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(2.0, 0.0, 0.0)).unwrap(),
        None,
    ));
    for (vertex, point) in [
        (top_vertex.clone(), top_point),
        (bottom_vertex.clone(), bottom_point),
    ] {
        ir.model.vertices.push(Vertex {
            native_ref: None,
            id: vertex,
            point,
            tolerance: None,
        });
    }
    for (curve, geometry) in [
        (
            top_curve.clone(),
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                CircleCurve::try_new(
                    Point3::new(0.0, 0.0, 5.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    2.0,
                )
                .unwrap(),
            )),
        ),
        (
            bottom_curve.clone(),
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                CircleCurve::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    2.0,
                )
                .unwrap(),
            )),
        ),
        (
            seam_curve.clone(),
            CurveGeometry::Solved(SolvedCurveGeometry::Line(
                LineCurve::try_new(Point3::new(2.0, 0.0, 5.0), Vector3::new(0.0, 0.0, -1.0))
                    .unwrap(),
            )),
        ),
    ] {
        ir.model.curves.push(Curve {
            native_ref: None,
            parameter_range: None,
            id: curve,
            geometry,
            source_object: None,
        });
    }
    for (edge, curve, range, start, end) in [
        (
            top_edge.clone(),
            top_curve.clone(),
            [0.0, TAU],
            top_vertex.clone(),
            top_vertex.clone(),
        ),
        (
            bottom_edge.clone(),
            bottom_curve.clone(),
            [0.0, TAU],
            bottom_vertex.clone(),
            bottom_vertex.clone(),
        ),
        (
            seam_edge.clone(),
            seam_curve.clone(),
            [0.0, 5.0],
            top_vertex.clone(),
            bottom_vertex.clone(),
        ),
    ] {
        ir.model.edges.push(Edge {
            native_ref: None,
            id: edge,
            carrier: EdgeCarrier::new(Some(curve), Some(range)).unwrap(),
            start,
            end,
            tolerance: None,
        });
    }
    for (pcurve, geometry, range) in [
        (
            wall_top_pcurve.clone(),
            PcurveGeometry::Line(
                LinePcurve::try_new(Point2::new(0.0, 5.0), Point2::new(1.0, 0.0)).unwrap(),
            ),
            [0.0, TAU],
        ),
        (
            wall_seam_pcurve.clone(),
            PcurveGeometry::Line(
                LinePcurve::try_new(Point2::new(0.0, 5.0), Point2::new(0.0, -1.0)).unwrap(),
            ),
            [0.0, 5.0],
        ),
        (
            wall_bottom_pcurve.clone(),
            PcurveGeometry::Line(
                LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)).unwrap(),
            ),
            [0.0, TAU],
        ),
        (
            wall_seam_up_pcurve.clone(),
            PcurveGeometry::Line(
                LinePcurve::try_new(Point2::new(0.0, 5.0), Point2::new(0.0, -1.0)).unwrap(),
            ),
            [0.0, 5.0],
        ),
        (
            cap_top_pcurve.clone(),
            PcurveGeometry::Circle(
                CirclePcurve::try_new(
                    Point2::new(0.0, 0.0),
                    Point2::new(1.0, 0.0),
                    Point2::new(0.0, 1.0),
                    2.0,
                )
                .unwrap(),
            ),
            [0.0, TAU],
        ),
        (
            cap_bottom_pcurve.clone(),
            PcurveGeometry::Circle(
                CirclePcurve::try_new(
                    Point2::new(0.0, 0.0),
                    Point2::new(1.0, 0.0),
                    Point2::new(0.0, -1.0),
                    2.0,
                )
                .unwrap(),
            ),
            [0.0, TAU],
        ),
    ] {
        ir.model.pcurves.push(Pcurve {
            native_ref: None,
            id: pcurve,
            geometry,
            metadata: PcurveMetadata::general(None, Some(FiniteVector::new(range).unwrap()), None),
        });
    }
    for (coedge, owner_loop, edge, radial_next, sense, pcurve) in [
        (
            wall_top.clone(),
            wall_loop.clone(),
            top_edge.clone(),
            cap_top_coedge.clone(),
            Sense::Forward,
            wall_top_pcurve,
        ),
        (
            wall_seam_down.clone(),
            wall_loop.clone(),
            seam_edge.clone(),
            wall_seam_up.clone(),
            Sense::Forward,
            wall_seam_pcurve,
        ),
        (
            wall_bottom.clone(),
            wall_loop.clone(),
            bottom_edge.clone(),
            cap_bottom_coedge.clone(),
            Sense::Reversed,
            wall_bottom_pcurve,
        ),
        (
            wall_seam_up.clone(),
            wall_loop.clone(),
            seam_edge.clone(),
            wall_seam_down.clone(),
            Sense::Reversed,
            wall_seam_up_pcurve,
        ),
        (
            cap_top_coedge.clone(),
            top_loop.clone(),
            top_edge.clone(),
            wall_top.clone(),
            Sense::Reversed,
            cap_top_pcurve,
        ),
        (
            cap_bottom_coedge.clone(),
            bottom_loop.clone(),
            bottom_edge.clone(),
            wall_bottom.clone(),
            Sense::Forward,
            cap_bottom_pcurve,
        ),
    ] {
        ir.model.coedges.push(Coedge {
            native_ref: None,
            id: coedge,
            owner_loop,
            edge,
            radial_next,
            sense,
            pcurves: vec![PcurveUse {
                pcurve,
                isoparametric: None,
                parameter_range: None,
            }],
            use_curve: None,
        });
    }
    ir.finalize();
    for version in [
        RhinoArchiveVersion::V5,
        RhinoArchiveVersion::V6,
        RhinoArchiveVersion::V7,
        RhinoArchiveVersion::V8,
    ] {
        let mut bytes = Vec::new();
        let report = RhinoCodec
            .plan(
                EncodeInput::new(&ir, None),
                TargetRequest::Explicit(version.descriptor().id.as_str()),
            )
            .and_then(|plan| plan.write_to(&mut bytes))
            .expect("cylinder solid writes");
        assert!(
            report
                .losses
                .iter()
                .any(|loss| loss.message.contains("cadir:model:surface#cylinder.wall")),
            "{version:?}: the wall conversion names its surface"
        );
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("cylinder solid re-decodes");
        assert_eq!(decoded.ir().model.bodies.len(), 1, "{version:?}");
        assert_eq!(decoded.ir().model.faces.len(), 3, "{version:?}");
        assert_eq!(decoded.ir().model.coedges.len(), 6, "{version:?}");
        assert_eq!(decoded.ir().model.edges.len(), 3, "{version:?}");
        assert!(
            decoded
                .ir()
                .model
                .surfaces
                .iter()
                .filter(|surface| matches!(surface.geometry, SurfaceGeometry::Procedural { .. }))
                .count()
                == 1,
            "{version:?}: the wall re-decodes as one procedural carrier"
        );
    }
}
