// SPDX-License-Identifier: Apache-2.0

use crate::eval::{
    curve_second_derivative, nurbs_surface_isocurve, nurbs_surface_partials,
    nurbs_surface_second_partials, surface_partials, surface_second_partials,
};
use crate::geometry::nurbs::{
    NurbsCurve, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes, SurfaceParameterAxis,
};
use crate::geometry::sampled::{PolylineCurve, PolylineSamples, PolylineVertex};
use crate::geometry::{CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry};
use crate::math::{Point3, Vector3};
use crate::transform::Transform;
const EPS_DERIVATIVE_COORDINATE: f64 = 1.0e-12;
const EPS_RATIONAL_ARC_IDENTITY: f64 = 1.0e-11;

#[test]
fn bilinear_surface_partials_follow_stored_parameterization() {
    let surface = NurbsSurface::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(
            vec![
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 3.0, 0.0)],
                vec![Point3::new(2.0, 0.0, 0.0), Point3::new(2.0, 3.0, 0.0)],
            ],
            None,
        ),
        false,
    )
    .expect("fixture constructor admission")
    .unwrap();
    let partials = nurbs_surface_partials(
        crate::eval::admission::EvaluationAdmission::Standard,
        &surface,
        0.25,
        0.75,
    )
    .expect("partials");
    assert_eq!(partials.point, Point3::new(0.5, 2.25, 0.0));
    assert_eq!(partials.du, Vector3::new(2.0, 0.0, 0.0));
    assert_eq!(partials.dv, Vector3::new(0.0, 3.0, 0.0));
}

#[test]
fn quadratic_surface_second_partials_follow_stored_parameterization() {
    let surface = NurbsSurface::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        NurbsSurfaceAxis::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(
            (0..3)
                .map(|i| {
                    (0..3)
                        .map(|j| {
                            Point3::new(
                                f64::from(i) / 2.0,
                                f64::from(j) / 2.0,
                                f64::from(u8::from(i == 2)) + f64::from(u8::from(j == 2)),
                            )
                        })
                        .collect()
                })
                .collect(),
            None,
        ),
        false,
    )
    .expect("fixture constructor admission")
    .unwrap();
    let partials = nurbs_surface_second_partials(
        crate::eval::admission::EvaluationAdmission::Standard,
        &surface,
        0.25,
        0.75,
    )
    .expect("second partials");
    assert_eq!(partials.point, Point3::new(0.25, 0.75, 0.625));
    assert_eq!(partials.du, Vector3::new(1.0, 0.0, 0.5));
    assert_eq!(partials.dv, Vector3::new(0.0, 1.0, 1.5));
    assert_eq!(partials.duu, Vector3::new(0.0, 0.0, 2.0));
    assert_eq!(partials.duv, Vector3::new(0.0, 0.0, 0.0));
    assert_eq!(partials.dvv, Vector3::new(0.0, 0.0, 2.0));
}

#[test]
fn analytic_and_transformed_surface_partials_follow_parameterization() {
    let cylinder = SolvedSurfaceGeometry::Cylinder(
        crate::geometry::analytic::CylinderSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
        )
        .unwrap(),
    );
    let cone = SolvedSurfaceGeometry::Cone(
        crate::geometry::analytic::ConeSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
            1.0,
            std::f64::consts::FRAC_PI_4,
        )
        .unwrap(),
    );
    let sphere = SolvedSurfaceGeometry::Sphere(
        crate::geometry::analytic::SphereSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            3.0,
        )
        .unwrap(),
    );
    let torus = SolvedSurfaceGeometry::Torus(
        crate::geometry::analytic::TorusSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            5.0,
            2.0,
        )
        .unwrap(),
    );
    let transformed = SolvedSurfaceGeometry::Transformed(
        crate::geometry::PlacedSurface::try_new(
            Box::new(SolvedSurfaceGeometry::Plane(
                crate::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            )),
            Transform::affine([
                [2.0, 0.0, 0.0, 7.0],
                [0.0, 3.0, 0.0, 11.0],
                [0.0, 0.0, 4.0, 13.0],
            ])
            .expect("affine transform"),
        )
        .expect("placed surface"),
    );

    let cylinder_second = surface_second_partials(
        crate::eval::admission::EvaluationAdmission::Standard,
        &SurfaceGeometry::Solved(cylinder.clone()),
        0.0,
        4.0,
    )
    .expect("cylinder second partials evaluate");
    let cylinder = surface_partials(
        crate::eval::admission::EvaluationAdmission::Standard,
        &SurfaceGeometry::Solved(cylinder.clone()),
        0.0,
        4.0,
    )
    .expect("cylinder partials evaluate");
    assert_eq!(cylinder.point, Point3::new(2.0, 0.0, 4.0));
    assert_eq!(cylinder.du, Vector3::new(0.0, 2.0, 0.0));
    assert_eq!(cylinder.dv, Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(cylinder_second.duu, Vector3::new(-2.0, 0.0, 0.0));
    assert_eq!(cylinder_second.duv, Vector3::new(0.0, 0.0, 0.0));
    assert_eq!(cylinder_second.dvv, Vector3::new(0.0, 0.0, 0.0));
    let cone = surface_partials(
        crate::eval::admission::EvaluationAdmission::Standard,
        &SurfaceGeometry::Solved(cone.clone()),
        0.0,
        3.0,
    )
    .expect("cone partials evaluate");
    assert!((cone.point.x - 5.0).abs() < EPS_DERIVATIVE_COORDINATE);
    assert!((cone.du.y - 5.0).abs() < EPS_DERIVATIVE_COORDINATE);
    assert!((cone.dv.x - 1.0).abs() < EPS_DERIVATIVE_COORDINATE);
    assert_eq!(cone.dv.z, 1.0);
    let sphere = surface_partials(
        crate::eval::admission::EvaluationAdmission::Standard,
        &SurfaceGeometry::Solved(sphere.clone()),
        0.0,
        0.0,
    )
    .expect("sphere partials evaluate");
    assert_eq!(sphere.point, Point3::new(3.0, 0.0, 0.0));
    assert_eq!(sphere.du, Vector3::new(0.0, 3.0, 0.0));
    assert_eq!(sphere.dv, Vector3::new(0.0, 0.0, 3.0));
    let torus = surface_partials(
        crate::eval::admission::EvaluationAdmission::Standard,
        &SurfaceGeometry::Solved(torus.clone()),
        0.0,
        0.0,
    )
    .expect("torus partials evaluate");
    assert_eq!(torus.point, Point3::new(7.0, 0.0, 0.0));
    assert_eq!(torus.du, Vector3::new(0.0, 7.0, 0.0));
    assert_eq!(torus.dv, Vector3::new(0.0, 0.0, 2.0));
    let transformed = surface_partials(
        crate::eval::admission::EvaluationAdmission::Standard,
        &SurfaceGeometry::Solved(transformed.clone()),
        2.0,
        3.0,
    )
    .expect("transformed partials evaluate");
    assert_eq!(transformed.point, Point3::new(11.0, 20.0, 13.0));
    assert_eq!(transformed.du, Vector3::new(2.0, 0.0, 0.0));
    assert_eq!(transformed.dv, Vector3::new(0.0, 3.0, 0.0));
}

#[test]
fn analytic_and_rational_curve_derivatives_are_exact() {
    let parameter = 1.0e16;
    let circle = SolvedCurveGeometry::Circle(
        crate::geometry::analytic::CircleCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            3.0,
        )
        .unwrap(),
    );
    let tangent = crate::eval::decode::curve_tangent(
        crate::eval::admission::EvaluationAdmission::Standard,
        &CurveGeometry::Solved(circle.clone()),
        parameter,
    )
    .expect("analytic tangent");
    assert_eq!(
        tangent,
        Vector3::new(-3.0 * parameter.sin(), 3.0 * parameter.cos(), 0.0)
    );
    assert_eq!(
        curve_second_derivative(
            crate::eval::admission::EvaluationAdmission::Standard,
            &CurveGeometry::Solved(circle.clone()),
            parameter
        )
        .ok()
        .map(crate::features::FiniteVector3::get),
        Some(Vector3::new(
            -3.0 * parameter.cos(),
            -3.0 * parameter.sin(),
            0.0,
        ))
    );
    assert_eq!(
        crate::eval::decode::curve_tangent(
            crate::eval::admission::EvaluationAdmission::Standard,
            &CurveGeometry::Solved(circle.clone()),
            f64::NAN
        )
        .ok(),
        None
    );

    let arc = SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(1.0, 1.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
            ],
            Some(vec![1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0]),
            false,
        )
        .expect("fixture constructor admission")
        .unwrap(),
    );
    for parameter in [0.0, 0.5, 1.0] {
        let point = crate::eval::decode::curve_point(
            crate::eval::admission::EvaluationAdmission::Standard,
            &CurveGeometry::Solved(arc.clone()),
            parameter,
        )
        .expect("rational arc point");
        let tangent = crate::eval::decode::curve_tangent(
            crate::eval::admission::EvaluationAdmission::Standard,
            &CurveGeometry::Solved(arc.clone()),
            parameter,
        )
        .expect("rational arc tangent");
        let second = curve_second_derivative(
            crate::eval::admission::EvaluationAdmission::Standard,
            &CurveGeometry::Solved(arc.clone()),
            parameter,
        )
        .expect("rational arc acceleration");
        let radial_dot = point.x * tangent.x + point.y * tangent.y;
        assert!(radial_dot.abs() < EPS_DERIVATIVE_COORDINATE);
        assert!(
            (point.x * second.x + point.y * second.y + tangent.dot(tangent.get())).abs()
                < EPS_RATIONAL_ARC_IDENTITY
        );
        assert!(tangent.norm() > 0.0);
    }

    let corner = SolvedCurveGeometry::Polyline(
        PolylineCurve::new(
            PolylineSamples::Parameterized {
                vertices: vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(1.0, 0.0, 0.0),
                    Point3::new(1.0, 1.0, 0.0),
                ]
                .into_iter()
                .zip(vec![0.0, 1.0, 2.0])
                .map(|(point, parameter)| PolylineVertex { parameter, point })
                .collect::<Vec<_>>()
                .try_into()
                .expect("nonempty polyline fixture"),
            },
            0.0,
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("polyline construction admission")
        .unwrap(),
    );
    assert_eq!(
        crate::eval::decode::curve_tangent(
            crate::eval::admission::EvaluationAdmission::Standard,
            &CurveGeometry::Solved(corner.clone()),
            0.5
        )
        .ok()
        .map(crate::features::FiniteVector3::get),
        Some(Vector3::new(1.0, 0.0, 0.0))
    );
    assert_eq!(
        crate::eval::decode::curve_tangent(
            crate::eval::admission::EvaluationAdmission::Standard,
            &CurveGeometry::Solved(corner.clone()),
            1.0
        )
        .ok(),
        None
    );
}

#[test]
fn rational_surface_partials_apply_the_weight_quotient_rule() {
    let surface = NurbsSurface::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(
            vec![
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 3.0, 0.0)],
                vec![Point3::new(2.0, 0.0, 0.0), Point3::new(2.0, 3.0, 0.0)],
            ],
            Some(vec![1.0, 1.0, 2.0, 2.0])
                .map(|values| values.chunks(2_usize).map(<[_]>::to_vec).collect()),
        ),
        false,
    )
    .expect("fixture constructor admission")
    .unwrap();
    let partials = nurbs_surface_partials(
        crate::eval::admission::EvaluationAdmission::Standard,
        &surface,
        0.5,
        0.25,
    )
    .expect("partials");
    assert!((partials.point.x - 4.0 / 3.0).abs() < EPS_DERIVATIVE_COORDINATE);
    assert!((partials.point.y - 0.75).abs() < EPS_DERIVATIVE_COORDINATE);
    assert!((partials.du.x - 16.0 / 9.0).abs() < EPS_DERIVATIVE_COORDINATE);
    assert!(partials.du.y.abs() < EPS_DERIVATIVE_COORDINATE);
    assert!((partials.dv.y - 3.0).abs() < EPS_DERIVATIVE_COORDINATE);
    let second = nurbs_surface_second_partials(
        crate::eval::admission::EvaluationAdmission::Standard,
        &surface,
        0.5,
        0.25,
    )
    .expect("second partials");
    assert!((second.duu.x + 64.0 / 27.0).abs() < EPS_DERIVATIVE_COORDINATE);
    assert!(second.duu.y.abs() < EPS_DERIVATIVE_COORDINATE);
    assert_eq!(second.duv, Vector3::new(0.0, 0.0, 0.0));
    assert_eq!(second.dvv, Vector3::new(0.0, 0.0, 0.0));
}

#[test]
fn rational_surface_isocurves_preserve_the_tensor_product_parameterization() {
    let surface = NurbsSurface::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(
            vec![
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 3.0, 0.0)],
                vec![Point3::new(2.0, 0.0, 1.0), Point3::new(2.0, 3.0, 1.0)],
            ],
            Some(vec![1.0, 2.0, 3.0, 4.0])
                .map(|values| values.chunks(2_usize).map(<[_]>::to_vec).collect()),
        ),
        false,
    )
    .expect("fixture constructor admission")
    .unwrap();
    for (axis, fixed) in [
        (SurfaceParameterAxis::U, 0.25),
        (SurfaceParameterAxis::V, 0.75),
    ] {
        let isocurve = nurbs_surface_isocurve(
            crate::eval::admission::EvaluationAdmission::Standard,
            &surface,
            axis,
            fixed,
        )
        .expect("resource allocation did not fail")
        .expect("exact isocurve");
        let geometry = SolvedCurveGeometry::Nurbs(isocurve);
        for varying in [0.0, 0.2, 0.7, 1.0] {
            let expected = match axis {
                SurfaceParameterAxis::U => crate::eval::decode::nurbs_surface_point(
                    crate::eval::admission::EvaluationAdmission::Standard,
                    &surface,
                    fixed,
                    varying,
                )
                .expect("surface point"),
                SurfaceParameterAxis::V => crate::eval::decode::nurbs_surface_point(
                    crate::eval::admission::EvaluationAdmission::Standard,
                    &surface,
                    varying,
                    fixed,
                )
                .expect("surface point"),
            };
            let actual = crate::eval::decode::curve_point(
                crate::eval::admission::EvaluationAdmission::Standard,
                &CurveGeometry::Solved(geometry.clone()),
                varying,
            )
            .expect("isocurve point");
            assert!((actual.x - expected.x).abs() < EPS_DERIVATIVE_COORDINATE);
            assert!((actual.y - expected.y).abs() < EPS_DERIVATIVE_COORDINATE);
            assert!((actual.z - expected.z).abs() < EPS_DERIVATIVE_COORDINATE);
        }
    }
}
