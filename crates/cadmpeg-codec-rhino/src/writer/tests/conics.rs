// SPDX-License-Identifier: Apache-2.0
//! Exactness of the conic-to-NURBS and periodic-clamp conversions: every
//! converted carrier is sampled against its source carrier's own
//! parameterization, so a wrong formula fails by orders of magnitude more
//! than the bound below.

use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::geometry::analytic::{EllipseCurve, HyperbolaCurve, ParabolaCurve};
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::FiniteReal;

use super::super::conics::{
    ellipse_over_range, hyperbola_over_range, parabola_over_range, unperiod_curve,
};
use super::super::{check_nurbs_curve, WriteError};

const PROOF_TOLERANCE: f64 = 1.0e-9;

fn range(lo: f64, hi: f64) -> [FiniteReal; 2] {
    [
        FiniteReal::new(lo).expect("finite endpoint"),
        FiniteReal::new(hi).expect("finite endpoint"),
    ]
}

fn evaluate(curve: &NurbsCurve, parameter: f64) -> Point3 {
    cadmpeg_ir::eval::decode::nurbs_curve_point_at(EvaluationAdmission::Standard, curve, parameter)
        .expect("converted parameter is evaluable")
        .get()
}

/// Every sample of the converted carrier satisfies the source conic's
/// implicit equation. The conversion reparameterizes (a rational Bezier
/// parameter is not the conic angle), so parameter-wise comparison cannot
/// prove anything; locus membership is the exactness contract.
fn assert_locus(
    converted: &NurbsCurve,
    contains: impl Fn(Point3) -> f64,
    lo: f64,
    hi: f64,
    steps: usize,
) {
    for step in 0..=steps {
        let parameter = lo + (hi - lo) * step as f64 / steps as f64;
        let residual = contains(evaluate(converted, parameter));
        assert!(
            residual <= PROOF_TOLERANCE,
            "parameter {parameter}: locus residual {residual} exceeds proof bound"
        );
    }
}

/// The clamped span evaluates the periodic carrier's own points at the
/// same parameters. Knot insertion preserves the parameterization, so
/// parameter-wise comparison proves this conversion.
fn assert_same_parameter(
    converted: &NurbsCurve,
    expected: impl Fn(f64) -> Point3,
    lo: f64,
    hi: f64,
    steps: usize,
) {
    let mut scale = 1.0_f64;
    for step in 0..=steps {
        let parameter = lo + (hi - lo) * step as f64 / steps as f64;
        scale = scale.max(expected(parameter).distance(Point3::new(0.0, 0.0, 0.0)));
    }
    for step in 0..=steps {
        let parameter = lo + (hi - lo) * step as f64 / steps as f64;
        let distance = evaluate(converted, parameter).distance(expected(parameter));
        assert!(
            distance <= PROOF_TOLERANCE * scale,
            "parameter {parameter}: deviation {distance} exceeds proof bound"
        );
    }
}
fn assert_canonical(id: &str, converted: &NurbsCurve, lo: f64, hi: f64) {
    let degree = usize::try_from(converted.degree()).expect("degree fits");
    assert_eq!(
        [
            converted.knots()[degree],
            converted.knots()[converted.control_points().len()]
        ],
        [lo, hi],
        "converted knots keep the edge's own range"
    );
    assert!(!converted.periodic(), "converted carrier is nonperiodic");
    check_nurbs_curve(id, converted).expect("converted carrier is native-canonical");
}

#[test]
fn ellipse_full_arc_converts_to_closed_rational_nurbs() {
    let ellipse = EllipseCurve::try_new(
        Point3::new(1.0, -2.0, 3.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        4.0,
        2.0,
    )
    .expect("valid ellipse");
    let converted =
        ellipse_over_range("test", &ellipse, range(0.0, std::f64::consts::TAU)).expect("full arc");
    assert_eq!(converted.control_points().len(), 9);
    let contains = |point: Point3| {
        let (s, r) = ((point.x - 1.0) / 4.0, (point.y + 2.0) / 2.0);
        (s * s + r * r - 1.0).abs().max((point.z - 3.0).abs())
    };
    assert_locus(&converted, contains, 0.0, std::f64::consts::TAU, 64);
    assert_canonical("test", &converted, 0.0, std::f64::consts::TAU);
    let poles = converted.control_points();
    assert!(
        poles
            .first()
            .expect("pole")
            .get()
            .distance(poles.last().expect("pole").get())
            <= PROOF_TOLERANCE,
        "full arc closes"
    );
}

#[test]
fn ellipse_partial_arc_converts_over_its_own_knots() {
    let ellipse = EllipseCurve::try_new(
        Point3::new(0.0, 0.0, 5.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(0.0, 1.0, 0.0),
        3.0,
        1.0,
    )
    .expect("valid ellipse");
    let (lo, hi) = (0.3, 2.1);
    let converted = ellipse_over_range("test", &ellipse, range(lo, hi)).expect("partial arc");
    let contains = |point: Point3| {
        let (s, r) = (point.y / 3.0, -point.x);
        (s * s + r * r - 1.0).abs().max((point.z - 5.0).abs())
    };
    assert_locus(&converted, contains, lo, hi, 32);
    assert_canonical("test", &converted, lo, hi);
}

#[test]
fn parabola_span_matches_its_quadratic() {
    let parabola = ParabolaCurve::try_new(
        Point3::new(1.0, 1.0, 1.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        2.0,
    )
    .expect("valid parabola");
    let (lo, hi) = (-1.5, 2.5);
    let converted = parabola_over_range("test", &parabola, range(lo, hi)).expect("span");
    // The IR parabola reads axial `f * t^2` along the major direction and
    // transverse `2 * f * t` along axis-times-major.
    let contains = |point: Point3| {
        let (s, r) = (point.x - 1.0, point.y - 1.0);
        (s - r * r / 8.0).abs().max((point.z - 1.0).abs())
    };
    assert_locus(&converted, contains, lo, hi, 32);
    assert_canonical("test", &converted, lo, hi);
}

#[test]
fn hyperbola_spans_match_both_branches() {
    for (center, major_direction) in [
        (Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0)),
        (Point3::new(5.0, -5.0, 5.0), Vector3::new(0.0, 1.0, 0.0)),
    ] {
        let axis = Vector3::new(0.0, 0.0, 1.0);
        let hyperbola = HyperbolaCurve::try_new(center, axis, major_direction, 3.0, 1.0)
            .expect("valid hyperbola");
        // The minor direction is axis-times-major, as the decoder reads it.
        let minor = axis.cross(major_direction);
        let (lo, hi) = (-2.0, 3.0);
        let converted =
            hyperbola_over_range("test", &hyperbola, range(lo, hi)).expect("hyperbola span");
        let contains = |point: Point3| {
            let s = (point.x - center.x) * major_direction.x
                + (point.y - center.y) * major_direction.y
                + (point.z - center.z) * major_direction.z;
            let r = (point.x - center.x) * minor.x
                + (point.y - center.y) * minor.y
                + (point.z - center.z) * minor.z;
            let out_of_plane = ((point.x - center.x) * axis.x
                + (point.y - center.y) * axis.y
                + (point.z - center.z) * axis.z)
                .abs();
            ((s / 3.0) * (s / 3.0) - r * r - 1.0)
                .abs()
                .max(out_of_plane)
        };
        assert_locus(&converted, contains, lo, hi, 48);
        assert_canonical("test", &converted, lo, hi);
    }
}

#[test]
fn periodic_cubic_clamps_to_an_exact_span() {
    let poles = [
        [-10.0, -10.0, 0.0],
        [10.0, -10.0, 0.0],
        [10.0, 10.0, 0.0],
        [-10.0, 10.0, 0.0],
        [-10.0, -10.0, 0.0],
        [10.0, -10.0, 0.0],
        [10.0, 10.0, 2.0],
        [-10.0, 10.0, 2.0],
    ]
    .map(|[x, y, z]| Point3::new(x, y, z));
    let periodic = NurbsCurve::from_lanes(
        3,
        vec![
            -3.0, -2.0, -1.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0,
        ],
        poles.to_vec(),
        None,
        true,
    )
    .expect("valid periodic fixture");
    let (lo, hi) = (0.7, 4.2);
    let converted = unperiod_curve("test", &periodic, range(lo, hi)).expect("clamped span");
    let expected = |parameter: f64| {
        cadmpeg_ir::eval::decode::nurbs_curve_point_at(
            EvaluationAdmission::Standard,
            &periodic,
            parameter,
        )
        .expect("periodic parameter is evaluable")
        .get()
    };
    assert_same_parameter(&converted, expected, lo, hi, 24);
    assert_canonical("test", &converted, lo, hi);
}

#[test]
fn periodic_full_domain_clamps_to_a_closed_span() {
    let poles = [
        [-10.0, -10.0, 0.0],
        [10.0, -10.0, 0.0],
        [10.0, 10.0, 0.0],
        [-10.0, 10.0, 0.0],
        [-10.0, -10.0, 0.0],
        [10.0, -10.0, 0.0],
    ]
    .map(|[x, y, z]| Point3::new(x, y, z));
    let periodic = NurbsCurve::from_lanes(
        2,
        vec![-2.0, -1.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
        poles.to_vec(),
        None,
        true,
    )
    .expect("valid periodic fixture");
    let converted = unperiod_curve("test", &periodic, range(0.0, 4.0)).expect("full clamp");
    let expected = |parameter: f64| {
        cadmpeg_ir::eval::decode::nurbs_curve_point_at(
            EvaluationAdmission::Standard,
            &periodic,
            parameter,
        )
        .expect("periodic parameter is evaluable")
        .get()
    };
    assert_same_parameter(&converted, expected, 0.0, 4.0, 24);
    assert_canonical("test", &converted, 0.0, 4.0);
    let poles = converted.control_points();
    assert!(
        poles
            .first()
            .expect("pole")
            .get()
            .distance(poles.last().expect("pole").get())
            <= PROOF_TOLERANCE,
        "full-period clamp closes"
    );
}

#[test]
fn degenerate_ranges_are_omitted_with_named_reasons() {
    let ellipse = EllipseCurve::try_new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        4.0,
        2.0,
    )
    .expect("valid ellipse");
    let reasons = [
        ellipse_over_range("edge#1", &ellipse, range(1.0, 1.0)),
        ellipse_over_range("edge#2", &ellipse, range(0.0, std::f64::consts::TAU + 1.0)),
    ];
    for result in reasons {
        let WriteError::Omit(reason) = result.expect_err("degenerate range omits") else {
            panic!("degenerate range must omit, not fail");
        };
        assert!(reason.contains("edge#"), "reason names the edge: {reason}");
    }
}
