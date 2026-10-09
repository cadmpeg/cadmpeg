// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::eval::decode::Scratch;
use crate::geometry::analytic::{ConeSurface, SphereSurface, TorusSurface};

const EPS_REQUESTED_ANALYTIC: f64 = 1.0e-12;

fn close(actual: Vector3, expected: Vector3) {
    for (actual, expected) in [actual.x, actual.y, actual.z].into_iter().zip([expected.x, expected.y, expected.z]) {
        assert!((actual - expected).abs() <= EPS_REQUESTED_ANALYTIC, "{actual} vs {expected}");
    }
}

#[test]
fn requested_analytic_thirds_follow_cone_ratio_sphere_and_torus_charts() {
    let origin = Point3::new(0.0, 0.0, 0.0);
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let reference = Vector3::new(1.0, 0.0, 0.0);
    let angle = std::f64::consts::FRAC_PI_4;
    let slope = angle.tan();
    let (u, v) = (0.3_f64, 0.4_f64);
    let (su, cu, sv, cv) = (u.sin(), u.cos(), v.sin(), v.cos());
    let zero = Vector3::new(0.0, 0.0, 0.0);
    let cases = [
        (SolvedSurfaceGeometry::Cone(ConeSurface::try_new(origin, axis, reference, 2.0, 2.0, angle).unwrap()), [
            Vector3::new((2.0 + v * slope) * su, -2.0 * (2.0 + v * slope) * cu, 0.0),
            Vector3::new(-slope * cu, -2.0 * slope * su, 0.0), zero, zero,
        ]),
        (SolvedSurfaceGeometry::Sphere(SphereSurface::try_new(origin, axis, reference, 3.0).unwrap()), [
            Vector3::new(3.0 * cv * su, -3.0 * cv * cu, 0.0),
            Vector3::new(3.0 * sv * cu, 3.0 * sv * su, 0.0),
            Vector3::new(3.0 * cv * su, -3.0 * cv * cu, 0.0),
            Vector3::new(3.0 * sv * cu, 3.0 * sv * su, -3.0 * cv),
        ]),
        (SolvedSurfaceGeometry::Torus(TorusSurface::try_new(origin, axis, reference, 5.0, 2.0).unwrap()), [
            Vector3::new((5.0 + 2.0 * cv) * su, -(5.0 + 2.0 * cv) * cu, 0.0),
            Vector3::new(2.0 * sv * cu, 2.0 * sv * su, 0.0),
            Vector3::new(2.0 * cv * su, -2.0 * cv * cu, 0.0),
            Vector3::new(2.0 * sv * cu, 2.0 * sv * su, -2.0 * cv),
        ]),
    ];
    for (geometry, expected) in cases {
        let scratch = Scratch::new(EvaluationAdmission::Standard);
        let requested = crate::eval::surface_requested_jet_solved(&scratch, &geometry, u, v, SurfaceRequest::Third).unwrap();
        for (actual, expected) in requested.higher.third().unwrap().into_iter().zip(expected) {
            close(actual.get(), expected);
        }
    }
}

#[test]
fn requested_spherical_and_toroidal_offsets_keep_true_second_orders() {
    let origin = Point3::new(0.0, 0.0, 0.0);
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let reference = Vector3::new(1.0, 0.0, 0.0);
    let (u, v) = (0.3_f64, 0.4_f64);
    let (su, cu, sv, cv) = (u.sin(), u.cos(), v.sin(), v.cos());
    let cases = [
        (SolvedSurfaceGeometry::Sphere(SphereSurface::try_new(origin, axis, reference, 3.0).unwrap()), [
            Vector3::new(-4.0 * cv * cu, -4.0 * cv * su, 0.0),
            Vector3::new(4.0 * sv * su, -4.0 * sv * cu, 0.0),
            Vector3::new(-4.0 * cv * cu, -4.0 * cv * su, -4.0 * sv),
        ]),
        (SolvedSurfaceGeometry::Torus(TorusSurface::try_new(origin, axis, reference, 5.0, 2.0).unwrap()), [
            Vector3::new(-(5.0 + 3.0 * cv) * cu, -(5.0 + 3.0 * cv) * su, 0.0),
            Vector3::new(3.0 * sv * su, -3.0 * sv * cu, 0.0),
            Vector3::new(-3.0 * cv * cu, -3.0 * cv * su, -3.0 * sv),
        ]),
    ];
    for (geometry, expected) in cases {
        let mut ir = CadIr::empty();
        let base = stored(&mut ir, "analytic", geometry);
        let shifted = offset(&mut ir, "offset", base, 1.0);
        for (actual, expected) in evaluate(&ir, &shifted, u, v, SurfaceRequest::Second).second.unwrap().into_iter().zip(expected) {
            close(actual.get(), expected);
        }
    }
}

#[test]
fn requested_replica_reports_placed_point_overflow_before_singular_orientation() {
    let mut ir = CadIr::empty();
    let base = stored(&mut ir, "plane", SolvedSurfaceGeometry::Plane(PlaneSurface::try_new(
        Point3::new(f64::MAX, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0),
    ).unwrap()));
    let placed = procedural(&mut ir, "singular", ProceduralSurfaceDefinition::Replica {
        source: base, transform: Transform::affine([[2.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]]).unwrap(),
    });
    let index = ModelIndex::build(&ir, StandardIndex);
    for request in [SurfaceRequest::First, SurfaceRequest::Second] {
        match model_jet(EvaluationAdmission::Standard, &index, &placed, 0.0, 0.0, request) {
            Err(EvaluationFailure::NonFinite(reached)) => assert_eq!(reached, Point3::new(f64::INFINITY, 0.0, 0.0)),
            _ => panic!("placement reaches overflow before its orientation has no value"),
        }
    }
}

#[test]
fn requested_offset_keeps_original_degenerate_and_overflowing_normal_errors() {
    use crate::features::{FinitePoint3, FiniteVector3};
    let point = FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap();
    for overflow in [false, true] {
        let extent = if overflow { f64::MAX } else { 1.0 };
        let first = [Vector3::new(extent, 0.0, 0.0), Vector3::new(if overflow { 0.0 } else { 1.0 }, if overflow { extent } else { 0.0 }, 0.0)];
        let source = crate::eval::surface_request::RequestedJet {
            jet: SurfaceJet { point, first: Ok(FiniteVector3::array(first).unwrap()), second: Ok([FiniteVector3::ZERO; 3]) },
            higher: HigherPartials::Third(Ok([FiniteVector3::ZERO; 4])),
        };
        match super::super::offset(source, 1.0, SurfaceRequest::Second) {
            Err(EvaluationFailure::NonFinite(reached)) if overflow => {
                assert!(reached.x.is_nan() && reached.y.is_nan() && reached.z.is_nan());
            }
            Err(EvaluationFailure::NoValue) if !overflow => {}
            _ => panic!("original normal gate must run before higher derivatives"),
        }
    }
}
