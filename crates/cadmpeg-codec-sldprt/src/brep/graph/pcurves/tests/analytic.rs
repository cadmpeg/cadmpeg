// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use crate::brep::graph::pcurves::sphere_latitude;
use crate::brep::graph::test_support::test_nurbs_curve;
const SPHERE_POLE_ROUNDING_OFFSET: f64 = 1.0e-9;

#[test]
fn sphere_latitude_refuses_a_circle_plane_beyond_the_pole() {
    use std::f64::consts::FRAC_PI_2;
    // Planes the sphere carries: the equator and the pole.
    assert_eq!(sphere_latitude(0.0, 2.0), Some(0.0));
    assert_eq!(sphere_latitude(2.0, 2.0), Some(FRAC_PI_2));
    // The rounding the radius match admits reaches the pole and no further.
    assert_eq!(
        sphere_latitude(2.0 + SPHERE_POLE_ROUNDING_OFFSET, 2.0),
        Some(FRAC_PI_2)
    );
    // A plane beyond that band states a circle on another sphere. The
    // radius match alone admits it, because a circle radius near zero
    // matches the saturated `sqrt(radius^2 - height^2)`.
    assert_eq!(sphere_latitude(2.1, 2.0), None);
    // A signed radius keeps the signed ratio.
    assert_eq!(sphere_latitude(2.0, -2.0), Some(-FRAC_PI_2));
}

#[test]
fn homogeneous_quadratic_identity_proves_constant_radius() {
    let radius = 2.0;
    let checked_radius = cadmpeg_ir::scalar::PositiveLength::new(radius).expect("positive radius");
    let controls = [
        cadmpeg_ir::math::Point2::new(radius, 0.0),
        cadmpeg_ir::math::Point2::new(radius, radius),
        cadmpeg_ir::math::Point2::new(0.0, radius),
    ];
    let weights = [1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0];
    let knots = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    assert!(crate::brep::graph::pcurves::quadratic_nurbs_has_constant_radius(
        &cadmpeg_test_support::service_decode_context(),
        &controls,
        Some(&weights),
        &knots,
        checked_radius,
    )
    .unwrap());

    let mut invalid = controls;
    invalid[1].u += 0.01;
    assert!(!crate::brep::graph::pcurves::quadratic_nurbs_has_constant_radius(
        &cadmpeg_test_support::service_decode_context(),
        &invalid,
        Some(&weights),
        &knots,
        checked_radius,
    )
    .unwrap());
}

#[test]
fn ambiguous_cylindrical_endpoint_withholds_the_derived_pcurve() {
    use cadmpeg_ir::annotations::AnnotationBuilder;
    use cadmpeg_ir::geometry::{Curve, Surface};
    use cadmpeg_ir::ids::{CurveId, EdgeId, FaceId, LoopId, PointId, SurfaceId, VertexId};
    use cadmpeg_ir::topology::{Coedge, Edge, Face, Loop, Point, Sense, Vertex};

    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();

    let surface_id = SurfaceId::mint("test:model:entity#surface").expect("identity grammar");
    let curve_id = CurveId::mint("test:model:entity#curve").expect("identity grammar");
    let loop_id = LoopId::mint("test:model:entity#loop").expect("identity grammar");
    let edge_id = EdgeId::mint("test:model:entity#edge").expect("identity grammar");
    let start_vertex = VertexId::mint("test:model:entity#start-vertex").expect("identity grammar");
    let end_vertex = VertexId::mint("test:model:entity#end-vertex").expect("identity grammar");
    let start_point = PointId::mint("test:model:entity#start-point").expect("identity grammar");
    let end_point = PointId::mint("test:model:entity#end-point").expect("identity grammar");
    let coedge_id =
        cadmpeg_ir::ids::CoedgeId::mint("test:model:entity#coedge").expect("identity grammar");
    let mut brep = crate::brep::graph::Brep {
        surfaces: vec![Surface {
            id: surface_id.clone(),
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Solved(
                SolvedSurfaceGeometry::Cylinder(
                    cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                        cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                        cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                        cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                        1000.0,
                    )
                    .unwrap(),
                ),
            ),
            source_object: None,
        }],
        curves: vec![Curve {
            id: curve_id.clone(),
            geometry: cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                test_nurbs_curve(
                    2,
                    vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                    vec![
                        cadmpeg_ir::math::Point3::new(1000.0, 0.0, 0.0),
                        cadmpeg_ir::math::Point3::new(1000.0, 0.0, 1000.0),
                        cadmpeg_ir::math::Point3::new(1000.0, 0.0, 0.0),
                    ],
                    None,
                ),
            )),
            source_object: None,
        }],
        faces: vec![Face {
            id: FaceId::mint("test:model:entity#face").expect("identity grammar"),
            shell: cadmpeg_ir::ids::ShellId::mint("test:model:entity#shell")
                .expect("identity grammar"),
            surface: surface_id,
            sense: Sense::Forward,
            loops: cadmpeg_ir::topology::FaceLoops::unspecified(vec![loop_id.clone()]),
            name: None,
            color: None,
            tolerance: None,
        }],
        loops: vec![Loop {
            id: loop_id.clone(),
            face: FaceId::mint("test:model:entity#face").expect("identity grammar"),
            boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
                cadmpeg_ir::topology::LoopRing::new(
                    &cadmpeg_test_support::service_decode_context(),
                    vec![coedge_id.clone()],
                    Vec::new(),
                )
                .expect("fixture ring admission")
                .expect("valid loop ring"),
            ),
        }],
        coedges: vec![Coedge {
            id: coedge_id,
            owner_loop: loop_id,
            edge: edge_id.clone(),
            radial_next: cadmpeg_ir::ids::CoedgeId::mint("test:model:entity#coedge")
                .expect("identity grammar"),
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
                cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
                    1000.0, 0.0, 0.0,
                ))
                .expect("a finite position is a point"),
                None,
            ),
            Point::new(
                end_point,
                cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
                    1000.0, 0.0, 0.0,
                ))
                .expect("a finite position is a point"),
                None,
            ),
        ],
        ..Default::default()
    };
    let mut annotations = AnnotationBuilder::new();
    let source_stream = cadmpeg_ir::annotations::StreamHandle::new(
        &cadmpeg_test_support::service_decode_context(),
        cadmpeg_ir::stream_name!("test"),
        "fixture stream handle",
    )
    .unwrap();
    crate::brep::graph::pcurves::derive_pcurves(&ctx, &mut brep, &mut annotations, &source_stream)
        .expect("cylindrical pcurve derivation");

    assert!(brep.pcurves.is_empty());
    assert_eq!(brep.stats.ambiguous_pcurve_parameters, 1);
}
