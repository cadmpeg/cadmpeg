// SPDX-License-Identifier: Apache-2.0
//! The boundaries of `NurbsPatch` faces: natural boundaries, trim loops,
//! seams, poles, and their round trip through the writer.

use cadmpeg_ir::codec::DecodeResult;
use cadmpeg_ir::geometry::pcurve::{LinePcurve, PcurveGeometry};
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::topology::{LoopBoundary, Sense};
use cadmpeg_ir::CadIr;

use crate::loss::UsdLossCode;
use crate::test_support::{decode, encode};

/// Agreement of an edge point with its expected position.
const ON_CURVE: f64 = 1e-9;
/// Agreement after a round trip that stores control points as `point3f`.
const ROUNDED: f64 = 1e-5;
/// Fractions of an edge range where points are compared.
const FRACTIONS: [f64; 5] = [0.0, 0.2, 0.5, 0.7, 1.0];

/// A layer in millimetres holding `body` as the prim `P`.
fn layer(body: &str) -> String {
    format!("#usda 1.0\n(\n    metersPerUnit = 0.001\n)\ndef NurbsPatch \"P\"\n{{\n{body}}}\n")
}

/// Homogeneous de Boor evaluation of a NURBS curve at `t`.
fn de_boor(degree: usize, knots: &[f64], points: &[[f64; 4]], t: f64) -> [f64; 3] {
    let last = points.len() - 1;
    let span = if t >= knots[last + 1] {
        last
    } else {
        (degree..=last)
            .rev()
            .find(|index| knots[*index] <= t)
            .unwrap()
    };
    let mut work: Vec<[f64; 4]> = points[span - degree..=span].to_vec();
    for level in 1..=degree {
        for j in (level..=degree).rev() {
            let index = span - degree + j;
            let alpha = (t - knots[index]) / (knots[index + degree + 1 - level] - knots[index]);
            let before = work[j - 1];
            for (value, earlier) in work[j].iter_mut().zip(before) {
                *value = alpha * *value + (1.0 - alpha) * earlier;
            }
        }
    }
    let row = work[degree];
    [row[0] / row[3], row[1] / row[3], row[2] / row[3]]
}

/// The point at `s` of the Bézier curve with control points `points`.
fn bezier(points: &[[f64; 3]], s: f64) -> [f64; 3] {
    let homogeneous: Vec<[f64; 4]> = points.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect();
    let degree = points.len() - 1;
    let mut knots = vec![0.0; degree + 1];
    knots.extend(std::iter::repeat_n(1.0, degree + 1));
    de_boor(degree, &knots, &homogeneous, s)
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3)
        .map(|axis| (a[axis] - b[axis]).powi(2))
        .sum::<f64>()
        .sqrt()
}

/// The points at `FRACTIONS` of the range of each edge.
fn edge_samples(ir: &CadIr) -> Vec<Vec<[f64; 3]>> {
    ir.geometry
        .edges
        .iter()
        .map(|edge| {
            let curve_id = edge.carrier.curve().expect("an edge curve");
            let curve = ir
                .geometry
                .curves
                .iter()
                .find(|curve| curve.id == *curve_id)
                .expect("the edge curve exists");
            let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) = &curve.geometry else {
                panic!("the edge curve is NURBS");
            };
            let weights: Vec<f64> = nurbs.weights().map_or_else(
                || vec![1.0; nurbs.pole_count()],
                |weights| weights.iter().map(|weight| weight.get()).collect(),
            );
            let points: Vec<[f64; 4]> = nurbs
                .control_points()
                .iter()
                .zip(weights)
                .map(|(point, weight)| {
                    let point = point.get();
                    [point.x * weight, point.y * weight, point.z * weight, weight]
                })
                .collect();
            let degree = usize::try_from(nurbs.degree()).unwrap();
            let range = edge.carrier.param_range().expect("an edge range");
            let [low, high] = [range[0], range[1]];
            FRACTIONS
                .iter()
                .map(|fraction| {
                    de_boor(
                        degree,
                        nurbs.knots().as_slice(),
                        &points,
                        low + fraction * (high - low),
                    )
                })
                .collect()
        })
        .collect()
}

/// Whether `samples` lie on `curve` at `FRACTIONS`, in either direction.
fn follows(samples: &[[f64; 3]], curve: impl Fn(f64) -> [f64; 3]) -> bool {
    let along = |forward: bool| {
        FRACTIONS.iter().zip(samples).all(|(fraction, sample)| {
            let s = if forward { *fraction } else { 1.0 - fraction };
            distance(curve(s), *sample) <= ON_CURVE
        })
    };
    along(true) || along(false)
}

fn assert_valid(result: &DecodeResult) {
    let report = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(report.is_ok(), "findings: {:#?}", report.findings);
}

fn has_loss(result: &DecodeResult, code: UsdLossCode) -> bool {
    result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == code.kind())
}

fn rings(ir: &CadIr) -> Vec<(usize, usize)> {
    ir.geometry
        .loops
        .iter()
        .map(|loop_| match &loop_.boundary {
            LoopBoundary::Ring(ring) => (ring.coedges().len(), ring.vertex_uses().len()),
            LoopBoundary::Vertex { .. } => (0, 1),
        })
        .collect()
}

/// `z[i][j]` of the bicubic net, `i` along u and `j` along v.
const Z: [[f64; 4]; 4] = [
    [0.0, 1.0, 0.0, 2.0],
    [1.0, 3.0, 2.0, 0.0],
    [0.0, 2.0, 1.0, 1.0],
    [2.0, 0.0, 1.0, 3.0],
];

/// The control point at U index `i` and V index `j` of the bicubic net.
fn net(i: usize, j: usize) -> [f64; 3] {
    let [x, y] = [i, j].map(|index| f64::from(u8::try_from(index).unwrap()));
    [x, y + 0.25 * x, Z[i][j]]
}

fn bicubic(extra: &str) -> String {
    let points: Vec<String> = (0..4)
        .flat_map(|j| (0..4).map(move |i| net(i, j)))
        .map(|[x, y, z]| format!("({x}, {y}, {z})"))
        .collect();
    format!(
        "    int uVertexCount = 4\n    int vVertexCount = 4\n    int uOrder = 4\n    int vOrder = 4\n    double[] uKnots = [0, 0, 0, 0, 1, 1, 1, 1]\n    double[] vKnots = [0, 0, 0, 0, 1, 1, 1, 1]\n    point3f[] points = [{}]\n{extra}",
        points.join(", ")
    )
}

#[test]
fn an_untrimmed_bicubic_patch_is_bounded_by_its_four_sides() {
    for (orientation, sense) in [
        ("", Sense::Forward),
        (
            "    uniform token orientation = \"leftHanded\"\n",
            Sense::Reversed,
        ),
    ] {
        let result = decode(&layer(&bicubic(orientation)));
        assert_valid(&result);
        let ir = result.ir();
        assert_eq!(ir.geometry.faces.len(), 1);
        assert_eq!(ir.geometry.faces[0].sense, sense);
        assert_eq!(rings(ir), vec![(4, 0)]);
        assert_eq!(ir.geometry.vertices.len(), 4);
        let sides: [Vec<[f64; 3]>; 4] = [
            (0..4).map(|i| net(i, 0)).collect(),
            (0..4).map(|j| net(3, j)).collect(),
            (0..4).map(|i| net(i, 3)).collect(),
            (0..4).map(|j| net(0, j)).collect(),
        ];
        let samples = edge_samples(ir);
        for side in &sides {
            assert_eq!(
                samples
                    .iter()
                    .filter(|edge| follows(edge, |s| bezier(side, s)))
                    .count(),
                1,
                "one edge runs along the side {side:?}"
            );
        }
        assert!(!has_loss(&result, UsdLossCode::NurbsTrimNotTransferred));
        assert!(!has_loss(&result, UsdLossCode::NurbsBoundaryNotTransferred));
    }
}

/// A bilinear plane patch over `[0, 10]²` on the domain `[0, 1]²`.
const PLANE: &str = "    int uVertexCount = 2\n    int vVertexCount = 2\n    int uOrder = 2\n    int vOrder = 2\n    double[] uKnots = [0, 0, 1, 1]\n    double[] vKnots = [0, 0, 1, 1]\n    point3f[] points = [(0, 0, 0), (10, 0, 0), (0, 10, 0), (10, 10, 0)]\n";

/// The trim attributes of the square `[0.1, 0.9]²`, counterclockwise, and
/// of the circle of radius 0.2 about `(0.5, 0.5)`, clockwise when `hole_cw`.
fn square_with_hole(hole_cw: bool) -> String {
    let h = std::f64::consts::FRAC_1_SQRT_2;
    let mut circle = vec![
        (0.7, 0.5, 1.0),
        (0.7 * h, 0.7 * h, h),
        (0.5, 0.7, 1.0),
        (0.3 * h, 0.7 * h, h),
        (0.3, 0.5, 1.0),
        (0.3 * h, 0.3 * h, h),
        (0.5, 0.3, 1.0),
        (0.7 * h, 0.3 * h, h),
        (0.7, 0.5, 1.0),
    ];
    if hole_cw {
        circle.reverse();
    }
    let mut points = vec![
        "(0.1, 0.1, 1)".to_owned(),
        "(0.9, 0.1, 1)".to_owned(),
        "(0.9, 0.1, 1)".to_owned(),
        "(0.9, 0.9, 1)".to_owned(),
        "(0.9, 0.9, 1)".to_owned(),
        "(0.1, 0.9, 1)".to_owned(),
        "(0.1, 0.9, 1)".to_owned(),
        "(0.1, 0.1, 1)".to_owned(),
    ];
    points.extend(circle.iter().map(|(x, y, w)| format!("({x}, {y}, {w})")));
    format!(
        "    int[] trimCurve:counts = [4, 1]\n    int[] trimCurve:orders = [2, 2, 2, 2, 3]\n    int[] trimCurve:vertexCounts = [2, 2, 2, 2, 9]\n    double[] trimCurve:knots = [0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 4]\n    double2[] trimCurve:ranges = [(0, 1), (0, 1), (0, 1), (0, 1), (0, 4)]\n    double3[] trimCurve:points = [{}]\n",
        points.join(", ")
    )
}

/// Checks the face of the plane patch trimmed by `square_with_hole`.
fn assert_square_with_hole(result: &DecodeResult, tolerance: f64) {
    assert_valid(result);
    let ir = result.ir();
    assert_eq!(ir.geometry.faces.len(), 1);
    let face = &ir.geometry.faces[0];
    assert_eq!(face.loops.stated().count(), 2);
    let mut rings = rings(ir);
    rings.sort_unstable();
    assert_eq!(rings, vec![(1, 0), (4, 0)]);
    for samples in edge_samples(ir) {
        let on_hole = samples
            .iter()
            .all(|p| ((p[0] - 5.0).hypot(p[1] - 5.0) - 2.0).abs() <= tolerance);
        let on_square = samples.iter().all(|p| {
            ((p[0] - 5.0).abs() - 4.0).abs() <= tolerance
                || ((p[1] - 5.0).abs() - 4.0).abs() <= tolerance
        });
        assert!(on_hole || on_square, "an edge leaves the trim: {samples:?}");
        assert!(samples.iter().all(|p| p[2].abs() <= tolerance));
    }
}

#[test]
fn a_trimmed_patch_keeps_its_outer_loop_and_hole() {
    let result = decode(&layer(&format!("{PLANE}{}", square_with_hole(true))));
    assert_square_with_hole(&result, ON_CURVE);
    assert!(!has_loss(&result, UsdLossCode::NurbsTrimNotTransferred));
    assert!(!has_loss(&result, UsdLossCode::NurbsTrimRegionAssumed));
}

#[test]
fn a_hole_authored_counterclockwise_is_a_hole_by_its_nesting() {
    let result = decode(&layer(&format!("{PLANE}{}", square_with_hole(false))));
    assert_square_with_hole(&result, ON_CURVE);
    assert!(has_loss(&result, UsdLossCode::NurbsTrimRegionAssumed));
}

/// A rational cylinder of radius 1 and height 2, closed in u.
fn closed_in_u() -> String {
    let h = std::f64::consts::FRAC_1_SQRT_2;
    let circle = [
        (1.0, 0.0, 1.0),
        (h, h, h),
        (0.0, 1.0, 1.0),
        (-h, h, h),
        (-1.0, 0.0, 1.0),
        (-h, -h, h),
        (0.0, -1.0, 1.0),
        (h, -h, h),
        (1.0, 0.0, 1.0),
    ];
    let mut points = Vec::new();
    let mut weights = Vec::new();
    for z in [0.0, 2.0] {
        for (x, y, w) in circle {
            points.push(format!("({}, {}, {z})", x / w, y / w));
            weights.push(format!("{w}"));
        }
    }
    layer(&format!(
        "    int uVertexCount = 9\n    int vVertexCount = 2\n    int uOrder = 3\n    int vOrder = 2\n    double[] uKnots = [0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 4]\n    double[] vKnots = [0, 0, 1, 1]\n    double[] pointWeights = [{}]\n    point3f[] points = [{}]\n",
        weights.join(", "),
        points.join(", ")
    ))
}

#[test]
fn a_patch_closed_in_u_shares_its_seam_edge() {
    let result = decode(&closed_in_u());
    assert_valid(&result);
    let ir = result.ir();
    assert_eq!(rings(ir), vec![(4, 0)]);
    assert_eq!(ir.geometry.edges.len(), 3);
    assert_eq!(ir.geometry.vertices.len(), 2);
    let seam = &ir.geometry.edges.iter().find(|edge| {
        ir.geometry
            .coedges
            .iter()
            .filter(|coedge| coedge.edge == edge.id)
            .count()
            == 2
    });
    assert!(seam.is_some(), "one edge is used twice");
    for samples in edge_samples(ir) {
        let on_circle = samples
            .iter()
            .all(|p| (p[0].hypot(p[1]) - 1.0).abs() <= ON_CURVE);
        let on_seam = samples
            .iter()
            .all(|p| distance([p[0], p[1], 0.0], [1.0, 0.0, 0.0]) <= ON_CURVE);
        assert!(
            on_circle || on_seam,
            "an edge leaves the cylinder: {samples:?}"
        );
    }
}

const APEX: [f64; 3] = [0.0, 0.0, 1.0];
const RIM: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]];

/// A patch whose side `u = 0` meets in `APEX` and whose side `u = 1` is the
/// quadratic `RIM`.
fn pole_patch() -> String {
    let mut points = Vec::new();
    for corner in RIM {
        for point in [APEX, corner] {
            points.push(format!("({}, {}, {})", point[0], point[1], point[2]));
        }
    }
    layer(&format!(
        "    int uVertexCount = 2\n    int vVertexCount = 3\n    int uOrder = 2\n    int vOrder = 3\n    double[] uKnots = [0, 0, 1, 1]\n    double[] vKnots = [0, 0, 0, 1, 1, 1]\n    point3f[] points = [{}]\n",
        points.join(", ")
    ))
}

#[test]
fn trim_sides_along_the_seam_share_one_edge_whatever_their_parameterization() {
    let band = "    int[] trimCurve:counts = [4]\n    int[] trimCurve:orders = [2, 2, 2, 3]\n    int[] trimCurve:vertexCounts = [2, 2, 2, 3]\n    double[] trimCurve:knots = [0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0, 1, 1, 1]\n    double3[] trimCurve:points = [(0, 0.25, 1), (4, 0.25, 1), (4, 0.25, 1), (4, 0.75, 1), (4, 0.75, 1), (0, 0.75, 1), (0, 0.75, 1), (0, 0.7, 1), (0, 0.25, 1)]\n";
    let text = closed_in_u().replace("}\n", &format!("{band}}}\n"));
    let result = decode(&text);
    assert_valid(&result);
    assert!(!has_loss(&result, UsdLossCode::NurbsTrimNotTransferred));
    let ir = result.ir();
    assert_eq!(rings(ir), vec![(4, 0)]);
    assert_eq!(
        ir.geometry.edges.len(),
        3,
        "the two seam sides share one edge"
    );
    for samples in edge_samples(ir) {
        let on_circle = samples.iter().all(|p| {
            (p[0].hypot(p[1]) - 1.0).abs() <= ON_CURVE
                && [0.5, 1.5].iter().any(|z| (p[2] - z).abs() <= ON_CURVE)
        });
        let on_seam = samples.iter().all(|p| {
            distance([p[0], p[1], 0.0], [1.0, 0.0, 0.0]) <= ON_CURVE && (0.5..=1.5).contains(&p[2])
        });
        assert!(on_circle || on_seam, "an edge leaves the band: {samples:?}");
    }
}

#[test]
fn a_side_that_meets_in_one_point_is_a_pole_of_the_loop() {
    let (apex, rim) = (APEX, RIM);
    let result = decode(&pole_patch());
    assert_valid(&result);
    let ir = result.ir();
    assert_eq!(rings(ir), vec![(3, 1)]);
    assert_eq!(ir.geometry.vertices.len(), 3);
    let samples = edge_samples(ir);
    assert!(samples
        .iter()
        .any(|edge| follows(edge, |s| bezier(&rim, s))));
    for end in [rim[0], rim[2]] {
        assert!(samples
            .iter()
            .any(|edge| follows(edge, |s| bezier(&[apex, end], s))));
    }
}

/// The trim attributes of two closed triangles, the second inside the first
/// and clockwise, with the `trimCurve:ranges` line `ranges`.
fn two_triangles(ranges: &str) -> String {
    format!("    int[] trimCurve:counts = [1, 1]\n    int[] trimCurve:orders = [2, 2]\n    int[] trimCurve:vertexCounts = [4, 4]\n    double[] trimCurve:knots = [0, 0, 1, 2, 3, 3, 0, 0, 1, 2, 3, 3]\n{ranges}    double3[] trimCurve:points = [(0.1, 0.1, 1), (0.9, 0.1, 1), (0.9, 0.9, 1), (0.1, 0.1, 1), (0.5, 0.2, 1), (0.8, 0.5, 1), (0.8, 0.2, 1), (0.5, 0.2, 1)]\n")
}

#[test]
fn trim_arrays_with_one_entry_for_each_curve_trim_the_face() {
    for ranges in ["", "    double2[] trimCurve:ranges = [(0, 3), (0, 3)]\n"] {
        let trims = two_triangles(ranges);
        let result = decode(&layer(&format!("{PLANE}{trims}")));
        assert_valid(&result);
        assert_eq!(
            result.ir().geometry.faces[0].loops.stated().count(),
            2,
            "{trims}"
        );
        assert!(
            !has_loss(&result, UsdLossCode::NurbsTrimNotTransferred),
            "{trims}"
        );
    }
}

#[test]
fn trim_counts_that_sum_to_zero_leave_the_natural_boundary_without_a_loss() {
    for counts in ["[]", "[0, 0]"] {
        let result = decode(&layer(&format!(
            "{PLANE}    int[] trimCurve:counts = {counts}\n"
        )));
        assert_valid(&result);
        assert_eq!(rings(result.ir()), vec![(4, 0)], "{counts}");
        assert!(
            !has_loss(&result, UsdLossCode::NurbsTrimNotTransferred),
            "{counts}"
        );
    }
}

#[test]
fn malformed_trims_leave_the_natural_boundary_and_a_loss() {
    let cases = [
        // Knots that decrease.
        "    int[] trimCurve:counts = [1]\n    int[] trimCurve:orders = [2]\n    int[] trimCurve:vertexCounts = [4]\n    double[] trimCurve:knots = [0, 0, 2, 1, 3, 3]\n    double3[] trimCurve:points = [(0.1, 0.1, 1), (0.9, 0.1, 1), (0.9, 0.9, 1), (0.1, 0.1, 1)]\n",
        // A point count that disagrees with the vertex counts.
        "    int[] trimCurve:counts = [1]\n    int[] trimCurve:orders = [2]\n    int[] trimCurve:vertexCounts = [5]\n    double[] trimCurve:knots = [0, 0, 1, 2, 3, 4, 4]\n    double3[] trimCurve:points = [(0.1, 0.1, 1), (0.9, 0.1, 1)]\n",
        // A loop that does not close.
        "    int[] trimCurve:counts = [1]\n    int[] trimCurve:orders = [2]\n    int[] trimCurve:vertexCounts = [3]\n    double[] trimCurve:knots = [0, 0, 1, 2, 2]\n    double3[] trimCurve:points = [(0.1, 0.1, 1), (0.9, 0.1, 1), (0.9, 0.9, 1)]\n",
        // A point that is not a number.
        "    int[] trimCurve:counts = [1]\n    int[] trimCurve:orders = [2]\n    int[] trimCurve:vertexCounts = [4]\n    double[] trimCurve:knots = [0, 0, 1, 2, 3, 3]\n    double3[] trimCurve:points = [(0.1, 0.1, 1), (nan, 0.1, 1), (0.9, 0.9, 1), (0.1, 0.1, 1)]\n",
        // A knot that is not finite.
        "    int[] trimCurve:counts = [1]\n    int[] trimCurve:orders = [2]\n    int[] trimCurve:vertexCounts = [4]\n    double[] trimCurve:knots = [0, 0, 1, 2, inf, inf]\n    double3[] trimCurve:points = [(0.1, 0.1, 1), (0.9, 0.1, 1), (0.9, 0.9, 1), (0.1, 0.1, 1)]\n",
        // A negative weight.
        "    int[] trimCurve:counts = [1]\n    int[] trimCurve:orders = [2]\n    int[] trimCurve:vertexCounts = [4]\n    double[] trimCurve:knots = [0, 0, 1, 2, 3, 3]\n    double3[] trimCurve:points = [(0.1, 0.1, 1), (0.9, 0.1, -1), (0.9, 0.9, 1), (0.1, 0.1, 1)]\n",
        // A curve outside the parameter range.
        "    int[] trimCurve:counts = [1]\n    int[] trimCurve:orders = [2]\n    int[] trimCurve:vertexCounts = [4]\n    double[] trimCurve:knots = [0, 0, 1, 2, 3, 3]\n    double3[] trimCurve:points = [(0.1, 0.1, 1), (1.9, 0.1, 1), (0.9, 0.9, 1), (0.1, 0.1, 1)]\n",
        // A range outside the curve domain.
        "    int[] trimCurve:counts = [1]\n    int[] trimCurve:orders = [2]\n    int[] trimCurve:vertexCounts = [4]\n    double[] trimCurve:knots = [0, 0, 1, 2, 3, 3]\n    double2[] trimCurve:ranges = [(-1, 7)]\n    double3[] trimCurve:points = [(0.1, 0.1, 1), (0.9, 0.1, 1), (0.9, 0.9, 1), (0.1, 0.1, 1)]\n",
    ];
    let mismatched = [
        // Fewer ranges than trim curves.
        two_triangles("    double2[] trimCurve:ranges = [(0, 3)]\n"),
        // More ranges than trim curves.
        two_triangles("    double2[] trimCurve:ranges = [(0, 3), (0, 3), (0, 3)]\n"),
        // Ranges that are not pairs.
        two_triangles("    double[] trimCurve:ranges = [0, 3, 0, 3]\n"),
        // More orders than trim curves.
        two_triangles("").replace("orders = [2, 2]", "orders = [2, 2, 2]"),
        // More vertex counts than trim curves.
        two_triangles("").replace("vertexCounts = [4, 4]", "vertexCounts = [4, 4, 4]"),
        // Knots after the last trim curve.
        two_triangles("").replace("3, 3]\n    double3", "3, 3, 3]\n    double3"),
        // Points after the last trim curve.
        two_triangles("").replace("(0.5, 0.2, 1)]", "(0.5, 0.2, 1), (0.5, 0.2, 1)]"),
    ];
    for trims in cases.into_iter().map(str::to_owned).chain(mismatched) {
        let result = decode(&layer(&format!("{PLANE}{trims}")));
        assert_valid(&result);
        assert_eq!(rings(result.ir()), vec![(4, 0)], "{trims}");
        assert!(
            has_loss(&result, UsdLossCode::NurbsTrimNotTransferred),
            "{trims}"
        );
    }
}

#[test]
fn a_surface_that_is_not_finite_and_ordered_is_rejected() {
    for surface in [
        PLANE.replace("(10, 0, 0)", "(nan, 0, 0)"),
        PLANE.replace("uKnots = [0, 0, 1, 1]", "uKnots = [0, 1, 0, 1]"),
    ] {
        let result = decode(&layer(&surface));
        assert!(result.ir().geometry.faces.is_empty(), "{surface}");
        assert!(
            has_loss(&result, UsdLossCode::NurbsPatchRejected),
            "{surface}"
        );
    }
}

#[test]
fn an_authored_range_bounds_the_face() {
    let result = decode(&layer(&format!(
        "{PLANE}    double2 uRange = (0.25, 0.75)\n    double2 vRange = (0, 0.5)\n"
    )));
    assert_valid(&result);
    let ir = result.ir();
    assert_eq!(rings(ir), vec![(4, 0)]);
    let mut corners: Vec<[f64; 2]> = ir
        .geometry
        .points
        .iter()
        .map(|point| {
            let p = point.position().get();
            [p.x, p.y]
        })
        .collect();
    corners.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    let expected = [[2.5, 0.0], [2.5, 5.0], [7.5, 0.0], [7.5, 5.0]];
    for (corner, expected) in corners.iter().zip(expected) {
        assert!(
            (corner[0] - expected[0]).hypot(corner[1] - expected[1]) <= ON_CURVE,
            "{corners:?}"
        );
    }
    assert_eq!(corners.len(), 4);
    assert!(!has_loss(&result, UsdLossCode::NurbsRangeNotTransferred));
}

/// `text` without its `BrepArray` prim, so the reader reads its patches.
fn without_brep_array(text: &str) -> String {
    let start = text.find("def BrepArray").expect("a BrepArray prim");
    let mut depth = 0_usize;
    let mut end = start;
    for (offset, character) in text[start..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = start + offset + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    format!("{}{}", &text[..start], &text[end..])
}

/// A trimmed face is written as a trimmed patch when its body has no
/// `BrepArray` prim: here an edge curve with no geometry refuses the prim.
#[test]
fn a_trimmed_patch_round_trips_through_its_trim_curves() {
    let first = decode(&layer(&format!("{PLANE}{}", square_with_hole(true))));
    let mut ir = first.ir().clone();
    ir.geometry.curves[0].geometry =
        CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None });
    let (text, _) = encode(&ir);
    assert!(!text.contains("def BrepArray"), "{text}");
    assert!(text.contains("trimCurve:points"), "{text}");
    let second = decode(&text);
    assert_square_with_hole(&second, ROUNDED);
    assert!(!has_loss(&second, UsdLossCode::NurbsTrimNotTransferred));
    assert!(!has_loss(&second, UsdLossCode::NurbsTrimRegionAssumed));
}

#[test]
fn an_untrimmed_patch_round_trips_without_trim_curves() {
    let first = decode(&layer(&bicubic("")));
    let (text, _) = encode(first.ir());
    assert!(!text.contains("trimCurve"), "{text}");
    let second = decode(&without_brep_array(&text));
    assert_valid(&second);
    assert_eq!(rings(second.ir()), vec![(4, 0)]);
}

/// The faces, loop rings, and edges of a decoded model, in a stable order.
fn shape(ir: &CadIr) -> (usize, Vec<(usize, usize)>, usize) {
    let mut rings = rings(ir);
    rings.sort_unstable();
    (ir.geometry.faces.len(), rings, ir.geometry.edges.len())
}

#[test]
fn bounded_patches_round_trip_through_a_brep_array() {
    for text in [
        layer(&bicubic("")),
        layer(&format!("{PLANE}{}", square_with_hole(true))),
        closed_in_u(),
        pole_patch(),
    ] {
        let first = decode(&text);
        let (written, _) = encode(first.ir());
        assert!(written.contains("def BrepArray"), "{written}");
        let second = decode(&written);
        assert_valid(&second);
        assert_eq!(shape(second.ir()), shape(first.ir()), "{written}");
        for loop_ in &second.ir().geometry.loops {
            if let LoopBoundary::Ring(ring) = &loop_.boundary {
                assert!(
                    ring.vertex_uses()
                        .iter()
                        .all(|pole| pole.pcurves.len() == 1),
                    "a pole keeps its trim curve: {written}"
                );
            }
        }
    }
}

#[test]
fn a_pole_with_no_trim_curve_leaves_the_other_trims_of_its_brep_array() {
    // A pole with no pcurve, and a pole whose pcurve is a line with no range,
    // have no trim curve to write.
    for line in [false, true] {
        let (mut ir, _, _) = decode(&pole_patch()).into_parts();
        let mut poles = 0;
        for index in 0..ir.geometry.loops.len() {
            let LoopBoundary::Ring(ring) = &ir.geometry.loops[index].boundary else {
                continue;
            };
            let coedges = ring.coedges().to_vec();
            let mut uses = ring.vertex_uses().to_vec();
            for pole in &mut uses {
                poles += 1;
                if line {
                    let use_ = pole.pcurves.first_mut().expect("the pole has a pcurve");
                    use_.parameter_range = None;
                    let pcurve = ir
                        .geometry
                        .pcurves
                        .iter_mut()
                        .find(|pcurve| pcurve.id == use_.pcurve)
                        .expect("the pole pcurve is in the arena");
                    pcurve.geometry = PcurveGeometry::Line(
                        LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0))
                            .expect("a line"),
                    );
                } else {
                    pole.pcurves.clear();
                }
            }
            ir.geometry.loops[index]
                .replace_ring(coedges, uses)
                .expect("the same ring");
        }
        assert!(poles > 0);
        let (written, report) = encode(&ir);
        assert!(written.contains("def BrepArray"), "{written}");
        assert!(written.contains("brep:curveUv:nurb"), "{written}");
        let reported =
            |code: UsdLossCode| report.losses.iter().any(|loss| loss.code == code.kind());
        assert!(!reported(UsdLossCode::ExportBrepUntrimmed), "{written}");
        assert!(reported(UsdLossCode::ExportBrepPoleNotWritten), "{written}");
        let second = decode(&written);
        assert_valid(&second);
        let (faces, rings, edges) = shape(&ir);
        let unpoled = rings.iter().map(|(coedges, _)| (*coedges, 0)).collect();
        assert_eq!(shape(second.ir()), (faces, unpoled, edges), "{written}");
    }
}

/// Whether the triangle `[a, b, c]` of the XY plane holds `point`.
fn triangle_holds(triangle: [[f64; 2]; 3], point: [f64; 2]) -> bool {
    let side = |a: [f64; 2], b: [f64; 2]| {
        (b[0] - a[0]) * (point[1] - a[1]) - (b[1] - a[1]) * (point[0] - a[0])
    };
    let [a, b, c] = triangle;
    let sides = [side(a, b), side(b, c), side(c, a)];
    sides.iter().all(|value| *value >= 0.0) || sides.iter().all(|value| *value <= 0.0)
}

/// A face that does not classify its loops, listed hole first, writes no
/// trim curves: the writer meshes it, and the hole stays a hole.
#[test]
fn unclassified_loops_listing_the_hole_first_keep_the_hole() {
    let first = decode(&layer(&format!("{PLANE}{}", square_with_hole(true))));
    let mut ir = first.ir().clone();
    let face = &mut ir.geometry.faces[0];
    let mut loops = face.loops.stated().cloned().collect::<Vec<_>>();
    loops.sort_by_key(|id| {
        first
            .ir()
            .geometry
            .loops
            .iter()
            .find(|candidate| &candidate.id == id)
            .map(|candidate| candidate.coedges().len())
    });
    assert_eq!(rings(first.ir()).len(), 2);
    face.loops = cadmpeg_ir::topology::FaceLoops::unspecified(loops);
    let (text, _) = encode(&ir);
    assert!(!text.contains("trimCurve"), "{text}");
    let second = decode(&without_brep_array(&text));
    let [mesh] = second.ir().model.tessellations.as_slice() else {
        panic!("one mesh: {text}");
    };
    let points: Vec<[f64; 2]> = mesh
        .vertices()
        .iter()
        .map(|point| [point.get().x, point.get().y])
        .collect();
    let mut area = 0.0;
    for [a, b, c] in mesh.triangles() {
        let triangle = [a, b, c].map(|index| points[usize::try_from(index).unwrap()]);
        assert!(
            !triangle_holds(triangle, [5.0, 5.0]),
            "a triangle covers the hole: {triangle:?}"
        );
        let [a, b, c] = triangle;
        area += ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() / 2.0;
    }
    let expected = 64.0 - 4.0 * std::f64::consts::PI;
    assert!(
        (area - expected).abs() <= 0.5,
        "area {area}, expected {expected}"
    );
}

/// A body with no `BrepArray` prim writes no trim curves for a face that
/// does not classify its loops: an edge curve with no geometry refuses the
/// prim, where a classified face is written as a trimmed patch.
#[test]
fn unclassified_loops_write_no_trim_curves_without_a_brep_array() {
    let first = decode(&layer(&format!("{PLANE}{}", square_with_hole(true))));
    let mut ir = first.ir().clone();
    let loops = ir.geometry.faces[0]
        .loops
        .stated()
        .cloned()
        .collect::<Vec<_>>();
    ir.geometry.faces[0].loops = cadmpeg_ir::topology::FaceLoops::unspecified(loops);
    ir.geometry.curves[0].geometry =
        CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None });
    let (text, _) = encode(&ir);
    assert!(!text.contains("def BrepArray"), "{text}");
    assert!(!text.contains("trimCurve"), "{text}");
}
