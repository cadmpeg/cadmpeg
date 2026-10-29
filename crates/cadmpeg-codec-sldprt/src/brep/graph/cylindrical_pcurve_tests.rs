// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::geometry::analytic::{CylinderSurface, LineCurve};
use cadmpeg_ir::math::Vector3;

fn axial_line_fixture(
    cylinder_origin: Point3,
    radius: f64,
    line_origin: Point3,
    line_direction: Vector3,
    trim_start: Point3,
    trim_end: Point3,
) -> Brep {
    let surface_id = SurfaceId::mint("test:axial:surface#1").expect("identity grammar");
    let curve_id = CurveId::mint("test:axial:curve#1").expect("identity grammar");
    let face_id = FaceId::mint("test:axial:face#1").expect("identity grammar");
    let shell_id = ShellId::mint("test:axial:shell#1").expect("identity grammar");
    let loop_id = LoopId::mint("test:axial:loop#1").expect("identity grammar");
    let coedge_id = CoedgeId::mint("test:axial:coedge#1").expect("identity grammar");
    let edge_id = EdgeId::mint("test:axial:edge#1").expect("identity grammar");
    let start_vertex = VertexId::mint("test:axial:start-vertex#1").expect("identity grammar");
    let end_vertex = VertexId::mint("test:axial:end-vertex#1").expect("identity grammar");
    let start_point = PointId::mint("test:axial:start-point#1").expect("identity grammar");
    let end_point = PointId::mint("test:axial:end-point#1").expect("identity grammar");
    Brep {
        surfaces: vec![Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                CylinderSurface::try_new(
                    cylinder_origin,
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    radius,
                )
                .expect("valid cylinder fixture"),
            )),
            source_object: None,
        }],
        curves: vec![Curve {
            parameter_range: None,
            id: curve_id.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                LineCurve::try_new(line_origin, line_direction).expect("valid line fixture"),
            )),
            source_object: None,
        }],
        faces: vec![Face {
            id: face_id.clone(),
            shell: shell_id,
            surface: surface_id,
            sense: Sense::Forward,
            loops: cadmpeg_ir::topology::FaceLoops::unspecified(vec![loop_id.clone()]),
            name: None,
            color: None,
            tolerance: None,
        }],
        loops: vec![Loop {
            id: loop_id.clone(),
            face: face_id,
            boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
                cadmpeg_ir::topology::LoopRing::new(vec![coedge_id.clone()], Vec::new())
                    .expect("valid loop ring"),
            ),
        }],
        coedges: vec![Coedge {
            id: coedge_id,
            owner_loop: loop_id,
            edge: edge_id.clone(),
            radial_next: CoedgeId::mint("test:axial:coedge#2").expect("identity grammar"),
            sense: Sense::Forward,
            pcurves: Vec::new(),
            use_curve: None,
        }],
        edges: vec![Edge {
            id: edge_id,
            carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(Some(curve_id)),
            start: start_vertex.clone(),
            end: end_vertex.clone(),
            tolerance: None,
        }],
        vertices: vec![
            Vertex {
                id: start_vertex,
                point: start_point.clone(),
                tolerance: None,
            },
            Vertex {
                id: end_vertex,
                point: end_point.clone(),
                tolerance: None,
            },
        ],
        points: vec![
            Point::new(
                start_point,
                cadmpeg_ir::features::FinitePoint3::new(trim_start)
                    .expect("a finite position is a point"),
                None,
            ),
            Point::new(
                end_point,
                cadmpeg_ir::features::FinitePoint3::new(trim_end)
                    .expect("a finite position is a point"),
                None,
            ),
        ],
        ..Default::default()
    }
}

fn derive_axial(brep: &mut Brep) {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let mut annotations = AnnotationBuilder::new();
    let source_stream =
        cadmpeg_ir::annotations::StreamHandle::new(cadmpeg_ir::stream_name!("test"));
    derive_cylindrical_pcurves(&ctx, brep, &mut annotations, &source_stream)
        .expect("cylindrical pcurve derivation");
}

fn line_pcurve(brep: &Brep) -> (f64, f64, f64, f64) {
    assert_eq!(brep.pcurves.len(), 1);
    let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(line) = &brep.pcurves[0].geometry else {
        panic!("expected an affine line pcurve");
    };
    let origin = line.origin().as_raw();
    let direction = line.direction().as_raw();
    (origin.u, origin.v, direction.u, direction.v)
}

#[test]
fn axial_line_pcurve_shares_the_carrier_parameterization() {
    let mut brep = axial_line_fixture(
        Point3::new(0.0, 0.0, 0.0),
        5.0,
        Point3::new(5.0, 0.0, 2.0),
        Vector3::new(0.0, 0.0, 1.0),
        Point3::new(5.0, 0.0, 7.0),
        Point3::new(5.0, 0.0, 9.0),
    );
    derive_axial(&mut brep);
    let (u, v, du, dv) = line_pcurve(&brep);
    assert!((u - 0.0).abs() <= 1.0e-12);
    assert!((v - 2.0).abs() <= 1.0e-12);
    assert!(du.abs() <= 1.0e-12);
    assert!((dv - 1.0).abs() <= 1.0e-12);
    for t in [0.0, 0.5, 3.0] {
        let curve_point = Point3::new(5.0, 0.0, 2.0 + t);
        let observed_u = curve_point.y.atan2(curve_point.x);
        assert!((u + t * du - observed_u).abs() <= 1.0e-12);
        assert!((v + t * dv - curve_point.z).abs() <= 1.0e-12);
    }
}

#[test]
fn axial_line_pcurve_keeps_direction_magnitude_and_sign() {
    let rate = -(1.0 + 5.0e-10);
    let mut brep = axial_line_fixture(
        Point3::new(0.0, 0.0, 0.0),
        5.0,
        Point3::new(0.0, 5.0, 3.0),
        Vector3::new(0.0, 0.0, -(1.0 + 5.0e-10)),
        Point3::new(0.0, 5.0, 8.0),
        Point3::new(0.0, 5.0, 10.0),
    );
    derive_axial(&mut brep);
    let (u, v, du, dv) = line_pcurve(&brep);
    assert!((u - std::f64::consts::FRAC_PI_2).abs() <= 1.0e-12);
    assert!((v - 3.0).abs() <= 1.0e-12);
    assert!(du.abs() <= 1.0e-12);
    assert!((dv - rate).abs() <= 1.0e-15);
    assert!((dv + 1.0).abs() > 1.0e-12);
}

#[test]
fn off_cylinder_line_withholds_the_derived_pcurve() {
    let mut brep = axial_line_fixture(
        Point3::new(0.0, 0.0, 0.0),
        5.0,
        Point3::new(6.0, 0.0, 2.0),
        Vector3::new(0.0, 0.0, 1.0),
        Point3::new(6.0, 0.0, 7.0),
        Point3::new(6.0, 0.0, 9.0),
    );
    derive_axial(&mut brep);
    assert!(brep.pcurves.is_empty());
}

#[test]
fn transverse_line_withholds_the_derived_pcurve() {
    let tilt: f64 = 5.5e-5;
    let axial = (1.0 - tilt * tilt).sqrt();
    let mut brep = axial_line_fixture(
        Point3::new(0.0, 0.0, 0.0),
        5.0,
        Point3::new(5.0, 0.0, 2.0),
        Vector3::new(tilt, 0.0, axial),
        Point3::new(5.0, 0.0, 7.0),
        Point3::new(5.0, 0.0, 9.0),
    );
    derive_axial(&mut brep);
    assert!(brep.pcurves.is_empty());
}
