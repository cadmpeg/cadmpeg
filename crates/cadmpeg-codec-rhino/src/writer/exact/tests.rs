// SPDX-License-Identifier: Apache-2.0
//! The rational B-spline forms equal their analytic sources at sample
//! parameters, through the parameter maps the forms state.

use std::f64::consts::{FRAC_PI_2, TAU};

use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::eval::decode::{nurbs_curve_point_at, nurbs_surface_point, surface_point_solved};
use cadmpeg_ir::geometry::analytic::{ConeSurface, CylinderSurface, SphereSurface, TorusSurface};
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::math::{Point3, Vector3};

use super::{
    analytic_patch, clamped_curve, composed_pcurve, pcurve_point, trigonometric_curve, ArcAxis,
    ParameterMap, Spline,
};

/// Relative distance within which an exact form and its source agree at a
/// sample parameter.
const EXACT_FORM_AGREEMENT: f64 = 1e-9;

fn samples(range: [f64; 2]) -> impl Iterator<Item = f64> {
    (0..=8).map(move |step| range[0] + (range[1] - range[0]) * f64::from(step) / 8.0)
}

fn axis() -> Vector3 {
    Vector3::new(0.0, 0.0, 1.0)
}

fn reference() -> Vector3 {
    Vector3::new(1.0, 0.0, 0.0)
}

#[test]
fn arc_axis_parameter_map_inverts_and_reaches_the_conic_point() {
    for (range, hyperbolic) in [
        ([0.3, 5.9], false),
        ([-1.0, 0.5], false),
        ([0.0, TAU], false),
        ([-1.5, 2.0], true),
    ] {
        let axis = ArcAxis::new(range[0], range[1], hyperbolic).expect("the range has arcs");
        let (curve, _) = trigonometric_curve(
            Point3::new(0.0, 0.0, 0.0),
            reference(),
            Vector3::new(0.0, 1.0, 0.0),
            hyperbolic,
            range,
        )
        .expect("the conic has a rational form");
        for angle in samples(range) {
            let parameter = axis.parameter(angle);
            assert!(
                (axis.analytic(parameter) - angle).abs()
                    <= EXACT_FORM_AGREEMENT * (1.0 + angle.abs()),
                "{range:?} {angle}"
            );
            let point = nurbs_curve_point_at(EvaluationAdmission::Standard, &curve, parameter)
                .expect("the arc has a point");
            let expected = if hyperbolic {
                Point3::new(angle.cosh(), angle.sinh(), 0.0)
            } else {
                Point3::new(angle.cos(), angle.sin(), 0.0)
            };
            assert!(
                point.distance(expected) <= EXACT_FORM_AGREEMENT * (1.0 + angle.abs().exp()),
                "{range:?} {angle}"
            );
        }
    }
}

fn assert_patch_equals(surface: &SolvedSurfaceGeometry, u: [f64; 2], v: [f64; 2]) {
    let patch = analytic_patch(surface, u, v).expect("the carrier has a NURBS patch");
    for a in samples(u) {
        for b in samples(v) {
            let expected = surface_point_solved(EvaluationAdmission::Standard, surface, a, b)
                .expect("the carrier has a point")
                .get();
            let actual = nurbs_surface_point(
                EvaluationAdmission::Standard,
                &patch.surface,
                patch.u.apply(a),
                patch.v.apply(b),
            )
            .expect("the patch has a point");
            let scale = 1.0 + expected.x.abs().max(expected.y.abs()).max(expected.z.abs());
            assert!(
                actual.distance(expected) <= EXACT_FORM_AGREEMENT * scale,
                "{surface:?} at ({a}, {b})"
            );
        }
    }
}

#[test]
fn analytic_patches_equal_their_carriers() {
    let origin = Point3::new(1.0, -2.0, 0.5);
    let tilted = Vector3::new(0.0, 0.6, 0.8);
    let cylinder =
        CylinderSurface::try_new(origin, tilted, reference(), 2.0).expect("cylinder fixture");
    assert_patch_equals(
        &SolvedSurfaceGeometry::Cylinder(cylinder),
        [0.3, 5.9],
        [-1.0, 3.0],
    );
    for ratio in [1.0, 0.6] {
        let cone = ConeSurface::try_new(origin, axis(), reference(), 1.5, ratio, 0.3)
            .expect("cone fixture");
        assert_patch_equals(&SolvedSurfaceGeometry::Cone(cone), [0.0, TAU], [-0.5, 2.0]);
    }
    let sphere = SphereSurface::try_new(origin, axis(), reference(), 3.0).expect("sphere fixture");
    assert_patch_equals(
        &SolvedSurfaceGeometry::Sphere(sphere),
        [-1.0, 4.0],
        [-1.2, 1.4],
    );
    let torus =
        TorusSurface::try_new(origin, axis(), reference(), 4.0, 1.0).expect("torus fixture");
    assert_patch_equals(&SolvedSurfaceGeometry::Torus(torus), [0.0, TAU], [0.0, TAU]);
}

#[test]
fn a_periodic_curve_is_clamped_across_its_seam() {
    let points = [
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(0.0, 1.0, 0.5),
        Point3::new(-1.0, 0.0, 0.0),
        Point3::new(0.0, -1.0, -0.5),
    ];
    let poles = points
        .iter()
        .chain(&points[..2])
        .copied()
        .collect::<Vec<_>>();
    let knots = (0..9).map(f64::from).collect::<Vec<_>>();
    let periodic =
        NurbsCurve::from_lanes(2, knots, poles, None, true).expect("periodic curve fixture");
    let range = [5.0, 7.0];
    let clamped = clamped_curve(&periodic, range).expect("the range spans one period");
    assert_eq!(clamped.knots().first(), Some(&range[0]));
    assert_eq!(clamped.knots().last(), Some(&range[1]));
    for parameter in samples(range) {
        let wrapped = if parameter > 6.0 {
            parameter - 4.0
        } else {
            parameter
        };
        let expected = nurbs_curve_point_at(EvaluationAdmission::Standard, &periodic, wrapped)
            .expect("the periodic curve has a point")
            .get();
        let actual = nurbs_curve_point_at(EvaluationAdmission::Standard, &clamped, parameter)
            .expect("the clamped curve has a point");
        assert!(
            actual.distance(expected) <= EXACT_FORM_AGREEMENT,
            "{parameter}"
        );
    }
}

fn uv(u: f64, v: f64) -> Point3 {
    Point3::new(u, v, 0.0)
}

#[test]
fn a_pcurve_along_one_angle_maps_to_the_segment_between_its_mapped_ends() {
    let around = ArcAxis::new(0.0, 3.0, false).expect("angular range");
    let u = ParameterMap::Arc(around.clone());
    let pcurve = composed_pcurve(
        Spline::segment(uv(0.5, 1.25), uv(2.5, 1.25), [0.0, 1.0]),
        &u,
        &ParameterMap::Identity,
    )
    .expect("an iso-line composes");
    for (parameter, angle) in [(0.0, 0.5), (1.0, 2.5)] {
        let point = pcurve_point(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            &pcurve,
            parameter,
        )
        .expect("the pcurve has a point");
        assert!((point.u - around.parameter(angle)).abs() <= EXACT_FORM_AGREEMENT);
        assert!((point.v - 1.25).abs() <= EXACT_FORM_AGREEMENT);
    }
    let middle = pcurve_point(
        &cadmpeg_core::decode::test_support::service_decode_context(),
        &pcurve,
        0.5,
    )
    .expect("the pcurve has a point");
    assert!((middle.v - 1.25).abs() <= EXACT_FORM_AGREEMENT);
}

#[test]
fn a_constant_angle_maps_to_its_nurbs_parameter() {
    let around = ArcAxis::new(0.0, 3.0, false).expect("angular range");
    let pcurve = composed_pcurve(
        Spline::segment(uv(FRAC_PI_2, -1.0), uv(FRAC_PI_2, 2.0), [0.0, 3.0]),
        &ParameterMap::Arc(around.clone()),
        &ParameterMap::Identity,
    )
    .expect("an axial line composes");
    for (parameter, height) in [(0.0, -1.0), (1.5, 0.5), (3.0, 2.0)] {
        let point = pcurve_point(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            &pcurve,
            parameter,
        )
        .expect("the pcurve has a point");
        assert!((point.u - around.parameter(FRAC_PI_2)).abs() <= EXACT_FORM_AGREEMENT);
        assert!((point.v - height).abs() <= EXACT_FORM_AGREEMENT);
    }
}

#[test]
fn compositions_without_an_exact_form_are_refused() {
    let around = ParameterMap::Arc(ArcAxis::new(0.0, 3.0, false).expect("angular range"));
    let diagonal = Spline::segment(uv(0.5, 0.0), uv(2.5, 1.0), [0.0, 1.0]);
    let along_and_up = composed_pcurve(diagonal.clone(), &around, &ParameterMap::Identity)
        .expect_err("a helix trace has no exact composed form");
    assert!(along_and_up.contains("moves along both parameters"));
    let twice = composed_pcurve(diagonal, &around, &around)
        .expect_err("a diagonal on a torus has no exact composed form");
    assert!(twice.contains("two angular parameters"));
}
