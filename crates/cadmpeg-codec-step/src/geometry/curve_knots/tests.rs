// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::eval::{self, admission::EvaluationAdmission};
use cadmpeg_ir::geometry::{nurbs::NurbsCurve, pcurve::PcurveNurbs};
use cadmpeg_ir::math::{Point2, Point3};

fn quintic_spans() -> (Vec<Point3>, Vec<Ref>, Vec<f64>) {
    let limits = [-2.0_f64, 0.0, 3.0, 7.0];
    let mut points = Vec::new();
    let mut references = Vec::new();
    for pair in limits.windows(2) {
        let a = pair[0];
        let h = pair[1] - a;
        for i in 0..=5 {
            let i = f64::from(i);
            let point = if i == 0.0 || i == 5.0 {
                let t = if i == 0.0 { pair[0] } else { pair[1] };
                Point3::new(t, t * t, 0.0)
            } else {
                Point3::new(
                    a + h * i / 5.0,
                    a * a + 2.0 * a * h * i / 5.0 + h * h * i * (i - 1.0) / 20.0,
                    0.0,
                )
            };
            let index = match points.iter().position(|p| *p == point) {
                Some(index) => index,
                None => {
                    let index = points.len();
                    points.push(point);
                    index
                }
            };
            references.push(Ref(u64::try_from(index).expect("fixture index")));
        }
    }
    let knots = limits.into_iter().flat_map(|t| [t; 6]).collect();
    (points, references, knots)
}

fn poles(points: &[Point3], references: &[Ref]) -> Vec<Point3> {
    references
        .iter()
        .map(|r| points[usize::try_from(r.0).expect("fixture index")])
        .collect()
}

#[test]
fn coincident_quintic_joins_preserve_polynomial_and_rational_parameterization() {
    let (points, references, knots) = quintic_spans();
    for rational in [false, true] {
        let mut refs = references.clone();
        let mut weights = rational.then(|| {
            refs.iter()
                .map(|r| 1.0 + f64::from(u32::try_from(r.0 % 3).expect("small residue")) / 4.0)
                .collect::<Vec<_>>()
        });
        let original = NurbsCurve::from_lanes(
            5,
            knots.clone(),
            poles(&points, &refs),
            weights.clone(),
            false,
        )
        .expect("piecewise quintic source");
        let (values, multiplicities) =
            continuous_curve_knots(&mut refs, &mut weights, &knots, 5, false);
        assert_eq!(values, [-2.0, 0.0, 3.0, 7.0]);
        assert_eq!(multiplicities, [6, 5, 5, 6]);
        assert_eq!(refs.len(), 16);
        let joined_knots: Vec<f64> = values
            .iter()
            .zip(&multiplicities)
            .flat_map(|(&t, &count)| std::iter::repeat_n(t, count))
            .collect();
        let joined = NurbsCurve::from_lanes(5, joined_knots, poles(&points, &refs), weights, false)
            .expect("C0 quintic joins");
        for t in values
            .into_iter()
            .chain((0..=180).map(|i| -2.0 + f64::from(i) / 20.0))
        {
            let before =
                eval::decode::nurbs_curve_point_at(EvaluationAdmission::Standard, &original, t)
                    .expect("source point")
                    .get();
            let after =
                eval::decode::nurbs_curve_point_at(EvaluationAdmission::Standard, &joined, t)
                    .expect("joined point")
                    .get();
            assert!(before.distance(after) < 1.0e-11);
            if !rational {
                assert!(after.distance(Point3::new(t, t * t, 0.0)) < 1.0e-11);
            }
        }
    }
}

#[test]
fn three_dimensional_curves_and_pcurves_emit_shared_join_poles() {
    let (points, references, knots) = quintic_spans();
    let points = poles(&points, &references);
    let curve = NurbsCurve::from_lanes(5, knots.clone(), points.clone(), None, false)
        .expect("source curve");
    let pcurve = PcurveNurbs::from_lanes(
        5,
        knots,
        points.iter().map(|p| Point2::new(p.x, p.y)).collect(),
        None,
        false,
    )
    .expect("source pcurve");
    let mut emitter = crate::writer::Emitter::new();
    super::super::nurbs_curve(&mut emitter, &curve);
    super::super::pcurve(
        &mut emitter,
        &super::super::PcurveGeometry::Nurbs { nurbs: pcurve },
    )
    .expect("pcurve export");
    let lines = emitter.into_lines().expect("finite STEP geometry");
    let curves = lines
        .iter()
        .filter(|line| line.contains("B_SPLINE_CURVE_WITH_KNOTS"))
        .collect::<Vec<_>>();
    assert_eq!(curves.len(), 2);
    for curve in curves {
        assert!(curve.contains("(6,5,5,6),(-2.,0.,3.,7.)"));
    }
}

#[test]
fn distinct_endpoints_and_unequal_homogeneous_weights_retain_the_full_join() {
    for same_point in [false, true] {
        let mut points = vec![Ref(1), Ref(2), Ref(if same_point { 2 } else { 3 }), Ref(4)];
        let mut weights = Some(vec![1.0, 2.0, if same_point { 3.0 } else { 2.0 }, 1.0]);
        let original = points.clone();
        let original_weights = weights.clone();
        let (_, multiplicities) = continuous_curve_knots(
            &mut points,
            &mut weights,
            &[0.0, 0.0, 1.0, 1.0, 2.0, 2.0],
            1,
            false,
        );
        assert_eq!(multiplicities, [2, 2, 2]);
        assert_eq!(points, original);
        assert_eq!(weights, original_weights);
    }
}

#[test]
fn partial_join_normalization_preserves_the_other_full_join() {
    let mut points = vec![Ref(1), Ref(2), Ref(2), Ref(3), Ref(4), Ref(5)];
    let mut weights = None;
    let (_, multiplicities) = continuous_curve_knots(
        &mut points,
        &mut weights,
        &[0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0],
        1,
        false,
    );
    assert_eq!(multiplicities, [2, 1, 2, 2]);
    assert_eq!(points, [Ref(1), Ref(2), Ref(3), Ref(4), Ref(5)]);
}

#[test]
fn periodic_curve_knots_and_poles_remain_in_their_native_convention() {
    let mut points = vec![Ref(1), Ref(2), Ref(2), Ref(3)];
    let original = points.clone();
    let mut weights = None;
    let (_, multiplicities) = continuous_curve_knots(
        &mut points,
        &mut weights,
        &[0.0, 0.0, 1.0, 1.0, 2.0, 2.0],
        1,
        true,
    );
    assert_eq!(multiplicities, [2, 2, 2]);
    assert_eq!(points, original);
}
