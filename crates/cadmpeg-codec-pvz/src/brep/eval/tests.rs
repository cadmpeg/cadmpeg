// SPDX-License-Identifier: Apache-2.0
use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2, PI, TAU};

use super::{
    bspline_curve_point, bspline_surface_point, curve_ends, curve_interval, curve_point, distance,
    surface_point,
};
use crate::brep::model::{
    BsplineCurve, BsplineSurface, Conic, CurveGeometry, SurfaceGeometry, Torus,
};

const EPS_POINT: f64 = 1.0e-12;

fn close(a: [f64; 3], b: [f64; 3]) -> bool {
    distance(a, b) <= EPS_POINT
}

#[test]
fn analytic_surfaces_follow_their_section_formulas() {
    let (u, v) = (0.3, 0.7);
    let plane = SurfaceGeometry::Plane {
        origin: [1.0, 0.0, 0.0],
        u_axis: [2.0, 0.0, 0.0],
        v_axis: [0.0, 3.0, 0.0],
        normal: [0.0, 0.0, 1.0],
    };
    assert!(close(
        surface_point(&plane, [u, v]).unwrap(),
        [1.0 + 2.0 * u, 3.0 * v, 0.0]
    ));
    let cylinder = SurfaceGeometry::Cylinder {
        origin: [0.0; 3],
        x: [2.0, 0.0, 0.0],
        y: [0.0, 2.0, 0.0],
        axis: [0.0, 0.0, 5.0],
    };
    assert!(close(
        surface_point(&cylinder, [u, v]).unwrap(),
        [2.0 * u.cos(), 2.0 * u.sin(), 5.0 * v]
    ));
    let cone = SurfaceGeometry::Cone {
        origin: [0.0; 3],
        x: [1.0, 0.0, 0.0],
        y: [0.0, 1.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        a: 2.0,
        b: -1.0,
    };
    let radius = 2.0 - v;
    assert!(close(
        surface_point(&cone, [u, v]).unwrap(),
        [radius * u.cos(), radius * u.sin(), v]
    ));
    let sphere = SurfaceGeometry::Sphere {
        center: [0.0; 3],
        x: [3.0, 0.0, 0.0],
        y: [0.0, 3.0, 0.0],
        z: [0.0, 0.0, 3.0],
    };
    assert!(close(
        surface_point(&sphere, [0.0, FRAC_PI_2]).unwrap(),
        [0.0, 0.0, 3.0]
    ));
    assert!(close(
        surface_point(&sphere, [u, v]).unwrap(),
        [
            3.0 * v.cos() * u.cos(),
            3.0 * v.cos() * u.sin(),
            3.0 * v.sin()
        ]
    ));
}

#[test]
fn torus_minor_circle_turns_with_the_major_angle() {
    let torus = SurfaceGeometry::Torus(Torus {
        origin: [0.0; 3],
        major_cos: [5.0, 0.0, 0.0],
        major_sin: [0.0, 5.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        minor_cos: [1.0, 0.0, 0.0],
        minor_axial: [0.0, 0.0, 1.0],
        minor_sin: [0.0, -1.0, 0.0],
    });
    for (u, v) in [(0.0_f64, 0.0_f64), (0.4, 1.1), (2.5, -0.8)] {
        let ring = 5.0 + v.cos();
        assert!(close(
            surface_point(&torus, [u, v]).unwrap(),
            [ring * u.cos(), ring * u.sin(), v.sin()]
        ));
    }
}

#[test]
fn rational_quadratic_traces_a_quarter_circle() {
    let curve = BsplineCurve {
        rational: true,
        order: 3,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        weighted: vec![
            [1.0, 0.0, 0.0],
            [FRAC_1_SQRT_2, FRAC_1_SQRT_2, 0.0],
            [0.0, 1.0, 0.0],
        ],
        weights: vec![1.0, FRAC_1_SQRT_2, 1.0],
        range: None,
    };
    for step in 0..=8 {
        let t = f64::from(step) / 8.0;
        let point = bspline_curve_point(&curve, t).unwrap();
        assert!((distance(point, [0.0; 3]) - 1.0).abs() <= EPS_POINT);
    }
    assert!(close(
        bspline_curve_point(&curve, 1.0).unwrap(),
        [0.0, 1.0, 0.0]
    ));
}

fn bilinear(periodic: [bool; 2]) -> BsplineSurface {
    BsplineSurface {
        rational: false,
        u_order: 2,
        v_order: 2,
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 2.0, 2.0],
        u_count: 2,
        v_count: 2,
        poles: vec![
            [0.0, 0.0, 0.0, 1.0],
            [0.0, 2.0, 0.0, 1.0],
            [1.0, 0.0, 0.0, 1.0],
            [1.0, 2.0, 0.0, 1.0],
        ],
        periodic,
    }
}

#[test]
fn bspline_surface_poles_are_u_major() {
    let surface = bilinear([false, false]);
    assert!(close(
        bspline_surface_point(&surface, [0.25, 0.5]).unwrap(),
        [0.25, 0.5, 0.0]
    ));
}

#[test]
fn periodic_direction_wraps_by_the_knot_domain() {
    let surface = bilinear([true, false]);
    let inside = bspline_surface_point(&surface, [0.25, 0.5]).unwrap();
    assert!(close(
        bspline_surface_point(&surface, [2.25, 0.5]).unwrap(),
        inside
    ));
    assert!(close(
        bspline_surface_point(&surface, [-0.75, 0.5]).unwrap(),
        inside
    ));
    let clamped = bilinear([false, false]);
    assert!(close(
        bspline_surface_point(&clamped, [2.25, 0.5]).unwrap(),
        [1.0, 0.5, 0.0]
    ));
}

#[test]
fn natural_intervals_follow_the_curve_type() {
    let circle = CurveGeometry::Circle(Conic {
        center: [0.0; 3],
        x: [1.0, 0.0, 0.0],
        y: [0.0, 1.0, 0.0],
        range: None,
    });
    assert_eq!(curve_interval(&circle), Some([0.0, TAU]));
    let arc = CurveGeometry::<3>::Ellipse(Conic {
        center: [0.0; 3],
        x: [2.0, 0.0, 0.0],
        y: [0.0, 1.0, 0.0],
        range: Some([0.0, PI]),
    });
    let [start, end] = curve_ends(&arc).unwrap();
    assert!(close(start, [2.0, 0.0, 0.0]) && close(end, [-2.0, 0.0, 0.0]));
    let line = CurveGeometry::Line {
        origin: [1.0, 1.0, 1.0],
        direction: [1.0, 0.0, 0.0],
    };
    assert_eq!(curve_ends(&line), Some([[1.0, 1.0, 1.0], [2.0, 1.0, 1.0]]));
    let composite = CurveGeometry::<3>::Composite(vec![0]);
    assert_eq!(curve_interval(&composite), None);
}

#[test]
fn polyline_is_parameterised_by_point_index() {
    let polyline = CurveGeometry::Polyline(vec![[0.0, 0.0], [1.0, 0.0], [1.0, 2.0]]);
    assert_eq!(curve_interval(&polyline), Some([0.0, 2.0]));
    assert_eq!(curve_point(&polyline, 1.5), Some([1.0, 1.0]));
    assert_eq!(curve_point(&polyline, 2.0), Some([1.0, 2.0]));
}
