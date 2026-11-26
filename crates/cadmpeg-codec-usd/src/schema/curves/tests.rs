// SPDX-License-Identifier: Apache-2.0

use super::{basis_curves, nurbs_curves, CurveData};
use crate::schema::tests::with_layer;

fn curves(prim_type: &str, body: &str) -> super::Curves {
    let text = format!("#usda 1.0\ndef {prim_type} \"C\"\n{{\n{body}\n}}\n");
    with_layer(&text, |stage, issues| {
        let prim = &stage.prims[0];
        if prim_type == "BasisCurves" {
            basis_curves(prim, issues)
        } else {
            nurbs_curves(prim, issues)
        }
        .unwrap()
    })
}

/// The point of a NURBS curve at `parameter`, by de Boor's recursion.
fn evaluate(curve: &CurveData, parameter: f64) -> [f64; 3] {
    let degree = usize::try_from(curve.degree).unwrap();
    let knots = &curve.knots;
    let count = curve.points.len();
    let span = (degree..count)
        .rev()
        .find(|span| knots[*span] <= parameter)
        .unwrap_or(degree);
    let weights = curve.weights.clone().unwrap_or_else(|| vec![1.0; count]);
    let mut homogeneous: Vec<[f64; 4]> = (0..=degree)
        .map(|offset| {
            let index = span - degree + offset;
            let weight = weights[index];
            let point = curve.points[index];
            [
                point[0] * weight,
                point[1] * weight,
                point[2] * weight,
                weight,
            ]
        })
        .collect();
    for level in 1..=degree {
        for offset in (level..=degree).rev() {
            let index = span - degree + offset;
            let alpha =
                (parameter - knots[index]) / (knots[index + degree + 1 - level] - knots[index]);
            let previous = homogeneous[offset - 1];
            for (value, before) in homogeneous[offset].iter_mut().zip(previous) {
                *value = (1.0 - alpha) * before + alpha * *value;
            }
        }
    }
    let point = homogeneous[degree];
    [
        point[0] / point[3],
        point[1] / point[3],
        point[2] / point[3],
    ]
}

fn domain(curve: &CurveData) -> [f64; 2] {
    let degree = usize::try_from(curve.degree).unwrap();
    [curve.knots[degree], curve.knots[curve.points.len()]]
}

const EPS_POINT: f64 = 1e-12;

fn assert_point(actual: [f64; 3], expected: [f64; 3]) {
    for (actual, expected) in actual.iter().zip(expected) {
        assert!(
            (actual - expected).abs() <= EPS_POINT,
            "{actual} != {expected}"
        );
    }
}

#[test]
fn a_linear_curve_is_degree_one_through_its_points() {
    let read = curves(
        "BasisCurves",
        "uniform token type = \"linear\"\nint[] curveVertexCounts = [3]\npoint3f[] points = [(0, 0, 0), (1, 0, 0), (1, 1, 0)]",
    );
    let curve = &read.curves[0];
    assert_eq!(curve.degree, 1);
    assert_eq!(curve.knots, [0.0, 0.0, 1.0, 2.0, 2.0]);
    assert_point(evaluate(curve, 1.5), [1.0, 0.5, 0.0]);
}

#[test]
fn a_periodic_linear_curve_closes() {
    let read = curves(
        "BasisCurves",
        "uniform token type = \"linear\"\nuniform token wrap = \"periodic\"\nint[] curveVertexCounts = [3]\npoint3f[] points = [(0, 0, 0), (1, 0, 0), (1, 1, 0)]",
    );
    let curve = &read.curves[0];
    assert_eq!(curve.points.first(), curve.points.last());
    assert_eq!(curve.points.len(), 4);
}

#[test]
fn a_bezier_curve_passes_through_its_segment_ends() {
    let read = curves(
        "BasisCurves",
        "int[] curveVertexCounts = [7]\npoint3f[] points = [(0, 0, 0), (1, 1, 0), (2, 1, 0), (3, 0, 0), (4, -1, 0), (5, -1, 0), (6, 0, 0)]",
    );
    let curve = &read.curves[0];
    assert_eq!(curve.degree, 3);
    assert_point(evaluate(curve, 1.0), [3.0, 0.0, 0.0]);
    assert_point(evaluate(curve, 2.0 - 1e-15), [6.0, 0.0, 0.0]);
    let bezier_mid = [
        0.125 * 0.0 + 0.375 * 1.0 + 0.375 * 2.0 + 0.125 * 3.0,
        0.125 * 0.0 + 0.375 * 1.0 + 0.375 * 1.0 + 0.125 * 0.0,
        0.0,
    ];
    assert_point(evaluate(curve, 0.5), bezier_mid);
}

#[test]
fn a_bspline_segment_follows_the_bspline_basis() {
    let read = curves(
        "BasisCurves",
        "uniform token basis = \"bspline\"\nint[] curveVertexCounts = [4]\npoint3f[] points = [(0, 0, 0), (1, 2, 0), (3, 2, 0), (4, 0, 0)]",
    );
    let curve = &read.curves[0];
    let [start, _] = domain(curve);
    let expected = [
        (0.0 + 4.0 * 1.0 + 3.0) / 6.0,
        (0.0 + 4.0 * 2.0 + 2.0) / 6.0,
        0.0,
    ];
    assert_point(evaluate(curve, start), expected);
}

#[test]
fn a_pinned_bspline_curve_starts_at_its_first_point() {
    let read = curves(
        "BasisCurves",
        "uniform token basis = \"bspline\"\nuniform token wrap = \"pinned\"\nint[] curveVertexCounts = [2]\npoint3f[] points = [(0, 0, 0), (6, 0, 0)]",
    );
    let curve = &read.curves[0];
    assert_point(evaluate(curve, domain(curve)[0]), [0.0, 0.0, 0.0]);
}

#[test]
fn a_catmull_rom_curve_interpolates_its_inner_points() {
    let read = curves(
        "BasisCurves",
        "uniform token basis = \"catmullRom\"\nint[] curveVertexCounts = [5]\npoint3f[] points = [(0, 0, 0), (1, 1, 0), (2, 0, 0), (3, 1, 0), (4, 0, 0)]",
    );
    let curve = &read.curves[0];
    assert_point(evaluate(curve, 0.0), [1.0, 1.0, 0.0]);
    assert_point(evaluate(curve, 1.0), [2.0, 0.0, 0.0]);
    let catmull_mid = [
        0.5 * (2.0 * 1.0
            + (2.0 - 0.0) * 0.5
            + (0.0 - 5.0 + 8.0 - 3.0) * 0.25
            + (-0.0 + 3.0 - 6.0 + 3.0) * 0.125),
        0.5 * (2.0 * 1.0
            + (0.0 - 0.0) * 0.5
            + (0.0 - 5.0 + 0.0 - 1.0) * 0.25
            + (-0.0 + 3.0 - 0.0 + 1.0) * 0.125),
        0.0,
    ];
    assert_point(evaluate(curve, 0.5), catmull_mid);
}

#[test]
fn invalid_counts_are_rejected_per_curve() {
    let read = curves(
        "BasisCurves",
        "int[] curveVertexCounts = [5, 4]\npoint3f[] points = [(0, 0, 0), (1, 0, 0), (2, 0, 0), (3, 0, 0), (4, 0, 0), (0, 1, 0), (1, 1, 0), (2, 1, 0), (3, 1, 0)]",
    );
    assert_eq!(read.curves.len(), 1);
    assert_eq!(read.rejected, 1);
}

#[test]
fn nurbs_curves_split_their_flat_arrays() {
    let read = curves(
        "NurbsCurves",
        "int[] curveVertexCounts = [3, 2]\nint[] order = [3, 2]\ndouble[] knots = [0, 0, 0, 1, 1, 1, 0, 0, 2, 2]\ndouble2[] ranges = [(0, 1), (0, 1)]\ndouble[] pointWeights = [1, 0.7071067811865476, 1, 1, 1]\npoint3f[] points = [(1, 0, 0), (1, 1, 0), (0, 1, 0), (0, 0, 0), (2, 0, 0)]",
    );
    assert_eq!(read.rejected, 0);
    let arc = &read.curves[0];
    assert_eq!(arc.range, None);
    let half = std::f64::consts::FRAC_1_SQRT_2;
    assert_point(evaluate(arc, 0.5), [half, half, 0.0]);
    let line = &read.curves[1];
    assert_eq!(line.knots, [0.0, 0.0, 2.0, 2.0]);
    assert_eq!(line.range, Some([0.0, 1.0]));
}
