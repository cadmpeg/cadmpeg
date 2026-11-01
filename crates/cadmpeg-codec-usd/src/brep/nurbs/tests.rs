// SPDX-License-Identifier: Apache-2.0

use super::{harmonic_arc, Nurbs};

/// Agreement of evaluated points with their closed form.
const POINT_TOLERANCE: f64 = 1e-12;
/// Agreement of a found parameter with the parameter of its point.
const PARAMETER_TOLERANCE: f64 = 1e-6;

fn close<const N: usize>(a: [f64; N], b: [f64; N]) -> bool {
    a.iter()
        .zip(b)
        .all(|(x, y)| (x - y).abs() <= POINT_TOLERANCE)
}

/// A cubic with distinct interior knots and nonuniform weights.
fn cubic() -> Nurbs<2> {
    Nurbs {
        degree: 3,
        knots: vec![0.0, 0.0, 0.0, 0.0, 0.4, 1.0, 1.0, 1.0, 1.0],
        points: vec![[0.0, 0.0], [1.0, 2.0], [2.0, -1.0], [3.0, 1.0], [4.0, 0.5]],
        weights: vec![1.0, 2.0, 0.5, 1.5, 1.0],
    }
}

#[test]
fn a_harmonic_arc_lies_on_its_ellipse() {
    let (center, cosine, sine) = ([1.0, 2.0], [3.0, 0.0], [0.0, 1.5]);
    let arc = harmonic_arc(center, cosine, sine, 0.25, 4.0).expect("arc");
    let [low, high] = arc.domain().expect("domain");
    for step in 0..=40 {
        let t = low + (high - low) * f64::from(step) / 40.0;
        let point = arc.point_at(t).expect("point");
        let across = (point[0] - center[0]) / cosine[0];
        let along = (point[1] - center[1]) / sine[1];
        assert!(
            (across * across + along * along - 1.0).abs() <= POINT_TOLERANCE,
            "t = {t}"
        );
    }
    let start = arc.point_at(low).expect("start");
    let end = arc.point_at(high).expect("end");
    assert!(close(
        start,
        [1.0 + 3.0 * 0.25_f64.cos(), 2.0 + 1.5 * 0.25_f64.sin()]
    ));
    assert!(close(
        end,
        [1.0 + 3.0 * 4.0_f64.cos(), 2.0 + 1.5 * 4.0_f64.sin()]
    ));
}

#[test]
fn a_reversed_curve_traces_the_same_points_backward() {
    let curve = cubic();
    let reversed = curve.reversed().expect("reversed");
    for step in 0..=20 {
        let t = f64::from(step) / 20.0;
        assert!(close(
            curve.point_at(t).expect("forward"),
            reversed.point_at(1.0 - t).expect("backward")
        ));
    }
}

#[test]
fn a_segment_keeps_the_points_of_its_interval() {
    let curve = cubic();
    let segment = curve.segment(0.2, 0.7).expect("segment");
    assert_eq!(segment.domain(), Some([0.2, 0.7]));
    assert_eq!(
        segment.knots.len(),
        segment.points.len() + segment.degree + 1
    );
    for step in 0..=20 {
        let t = 0.2 + 0.5 * f64::from(step) / 20.0;
        assert!(close(
            curve.point_at(t).expect("curve"),
            segment.point_at(t).expect("segment")
        ));
    }
}

#[test]
fn a_segment_at_the_domain_ends_is_the_curve() {
    let curve = cubic();
    let segment = curve.segment(0.0, 1.0).expect("segment");
    for step in 0..=10 {
        let t = f64::from(step) / 10.0;
        assert!(close(
            curve.point_at(t).expect("curve"),
            segment.point_at(t).expect("segment")
        ));
    }
}

#[test]
fn the_nearest_parameter_finds_a_curve_point() {
    let curve = cubic();
    let target = curve.point_at(0.63).expect("point");
    let found = curve.nearest(target).expect("nearest");
    assert!((found - 0.63).abs() < PARAMETER_TOLERANCE);
}
