// SPDX-License-Identifier: Apache-2.0
//! The solved cache of a parametric curve on a support (spec §7.6): a cubic
//! model-space B-curve fitted to the support through the pcurve, in the
//! pcurve's own parameter.

use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::math::Point3;

/// The fit's bound, millimetres (spec §7.6).
pub(super) const FIT_TOLERANCE_MM: f64 = 1.0e-5;

/// Share of [`FIT_TOLERANCE_MM`] a span's sampled departure may reach.
const SAMPLE_MARGIN: f64 = 0.25;

/// Span fractions where the departure is sampled.
const SAMPLES: [f64; 7] = [0.5, 0.25, 0.75, 0.118, 0.382, 0.618, 0.882];

/// The step of the one-sided derivative stencil, as a share of its span.
const STENCIL_STEP: f64 = 1.0e-3;

/// A break is a corner when sharing one derivative across it moves a pole
/// further than this, millimetres.
const SMOOTH_MM: f64 = 1.0e-7;

/// Spans a fitted curve may hold.
const MAX_SPANS: usize = 4096;

/// The narrowest span, and the closest breaks kept, as a share of the range.
const LEAST_SPAN_SHARE: f64 = 1.0e-9;

type V3 = [f64; 3];

/// The weights of the fourth-order one-sided derivative stencil, by the
/// number of steps into the span.
const STENCIL: [(f64, f64); 5] = [
    (0.0, -25.0),
    (1.0, 48.0),
    (2.0, -36.0),
    (3.0, 16.0),
    (4.0, -3.0),
];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn axpy(a: V3, scale: f64, b: V3) -> V3 {
    [
        a[0] + scale * b[0],
        a[1] + scale * b[1],
        a[2] + scale * b[2],
    ]
}

fn norm(a: V3) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

/// One cubic Hermite span over `[low, high]`: its ends and the derivative at
/// each, taken inside the span.
#[derive(Clone, Copy)]
struct Span {
    low: f64,
    high: f64,
    ends: [V3; 2],
    derivatives: [V3; 2],
}

impl Span {
    fn new(
        point: &impl Fn(f64) -> Result<Option<V3>, CodecError>,
        low: f64,
        high: f64,
    ) -> Result<Option<Self>, CodecError> {
        let width = high - low;
        if !(width > 0.0 && width.is_finite()) {
            return Ok(None);
        }
        let derivative = |at: f64, step: f64| -> Result<Option<V3>, CodecError> {
            let mut sum = [0.0; 3];
            for (steps, weight) in STENCIL {
                let Some(sample) = point(at + step * steps)? else {
                    return Ok(None);
                };
                sum = axpy(sum, weight, sample);
            }
            Ok(Some(sum.map(|value| value / (12.0 * step))))
        };
        let step = width * STENCIL_STEP;
        let (Some(start), Some(end)) = (point(low)?, point(high)?) else {
            return Ok(None);
        };
        let (Some(start_derivative), Some(end_derivative)) =
            (derivative(low, step)?, derivative(high, -step)?)
        else {
            return Ok(None);
        };
        Ok(Some(Self {
            low,
            high,
            ends: [start, end],
            derivatives: [start_derivative, end_derivative],
        }))
    }

    /// The span's two inner Bézier poles.
    fn inner_poles(&self) -> [V3; 2] {
        let third = (self.high - self.low) / 3.0;
        [
            axpy(self.ends[0], third, self.derivatives[0]),
            axpy(self.ends[1], -third, self.derivatives[1]),
        ]
    }

    fn at(&self, fraction: f64) -> V3 {
        let [p1, p2] = self.inner_poles();
        let s = fraction;
        let r = 1.0 - s;
        let weights = [r * r * r, 3.0 * r * r * s, 3.0 * r * s * s, s * s * s];
        let mut out = [0.0; 3];
        for (weight, pole) in weights
            .into_iter()
            .zip([self.ends[0], p1, p2, self.ends[1]])
        {
            out = axpy(out, weight, pole);
        }
        out
    }

    /// The greatest sampled departure of the span from `point`.
    fn departure(
        &self,
        point: &impl Fn(f64) -> Result<Option<V3>, CodecError>,
    ) -> Result<Option<f64>, CodecError> {
        let mut worst = 0.0_f64;
        for fraction in SAMPLES {
            let Some(exact) = point(self.low + (self.high - self.low) * fraction)? else {
                return Ok(None);
            };
            let gap = norm(sub(self.at(fraction), exact));
            if !gap.is_finite() {
                return Ok(None);
            }
            worst = worst.max(gap);
        }
        Ok(Some(worst))
    }
}

/// A cubic B-curve within [`FIT_TOLERANCE_MM`] of `point` over `range`, in
/// `point`'s parameter (spec §7.6). `breaks` are the pcurve's knots. A span
/// that `resolved` rejects is halved. `None` when `point` does not evaluate
/// or the bound is not reached.
pub(super) fn fit_curve(
    point: impl Fn(f64) -> Result<Option<Point3>, CodecError>,
    range: [f64; 2],
    breaks: &[f64],
    resolved: impl Fn(f64, f64) -> Result<bool, CodecError>,
) -> Result<Option<NurbsCurve>, CodecError> {
    let [low, high] = range;
    if !(low.is_finite() && high.is_finite() && low < high) {
        return Ok(None);
    }
    let point = |t: f64| -> Result<Option<V3>, CodecError> {
        let Some(p) = point(t.clamp(low, high))? else {
            return Ok(None);
        };
        let p = [p.x, p.y, p.z];
        Ok(p.iter().all(|value| value.is_finite()).then_some(p))
    };
    // Breaks within a billionth of the range of a kept break or an end are one (spec §7.6).
    let least = (high - low) * LEAST_SPAN_SHARE;
    let mut inner = Vec::new();
    for t in breaks.iter().copied() {
        if t.is_finite() && t > low + least && t < high - least {
            inner.push(t);
        }
    }
    inner.sort_by(f64::total_cmp);
    let mut parameters = Vec::new();
    let mut last = low;
    parameters.push(low);
    for t in inner {
        if t - last > least {
            parameters.push(t);
            last = t;
        }
    }
    parameters.push(high);

    let mut pending = Vec::new();
    for pair in parameters.windows(2).rev() {
        let Some(span) = Span::new(&point, pair[0], pair[1])? else {
            return Ok(None);
        };
        pending.push(span);
    }
    let mut spans = Vec::<Span>::new();
    while let Some(span) = pending.pop() {
        let Some(departure) = span.departure(&point)? else {
            return Ok(None);
        };
        let within = departure <= FIT_TOLERANCE_MM * SAMPLE_MARGIN;
        if within && resolved(span.low, span.high)? {
            spans.push(span);
            continue;
        }
        let middle = 0.5 * (span.low + span.high);
        if spans.len() + pending.len() + 2 > MAX_SPANS || middle - span.low <= least {
            return Ok(None);
        }
        let (Some(upper), Some(lower)) = (
            Span::new(&point, middle, span.high)?,
            Span::new(&point, span.low, middle)?,
        ) else {
            return Ok(None);
        };
        pending.push(upper);
        pending.push(lower);
    }
    let Some(leading) = spans.first() else {
        return Ok(None);
    };

    let mut knots = Vec::new();
    knots.extend([low; 4]);
    let mut poles = Vec::new();
    poles.push(leading.ends[0]);
    for (index, span) in spans.iter().enumerate() {
        let [mut first, mut second] = span.inner_poles();
        let corner = |a: &Span, b: &Span| {
            let widest = (a.high - a.low).max(b.high - b.low);
            norm(sub(a.derivatives[1], b.derivatives[0])) * widest / 6.0 > SMOOTH_MM
        };
        // A smooth break shares the mean derivative (spec §7.6).
        if let Some(previous) = index.checked_sub(1).map(|i| &spans[i]) {
            if !corner(previous, span) {
                let mean = axpy(previous.derivatives[1], 1.0, span.derivatives[0]).map(|v| v / 2.0);
                first = axpy(span.ends[0], (span.high - span.low) / 3.0, mean);
            }
        }
        poles.push(first);
        match spans.get(index + 1) {
            Some(next) if !corner(span, next) => {
                let mean = axpy(span.derivatives[1], 1.0, next.derivatives[0]).map(|v| v / 2.0);
                second = axpy(span.ends[1], -(span.high - span.low) / 3.0, mean);
                poles.push(second);
                knots.extend([span.high; 2]);
            }
            Some(_) => {
                poles.extend([second, span.ends[1]]);
                knots.extend([span.high; 3]);
            }
            None => {
                poles.extend([second, span.ends[1]]);
                knots.extend([span.high; 4]);
            }
        }
    }
    let mut points = Vec::new();
    points.extend(poles.into_iter().map(|[x, y, z]| Point3::new(x, y, z)));
    Ok(NurbsCurve::from_lanes(3, knots, points, None, false).ok())
}

#[cfg(test)]
mod tests {
    use super::{fit_curve, FIT_TOLERANCE_MM};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::geometry::nurbs::NurbsCurve;
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::math::Point3;

    type Sample = Result<Option<Point3>, CodecError>;

    fn fit(
        point: impl Fn(f64) -> Option<Point3>,
        range: [f64; 2],
        breaks: &[f64],
        resolved: impl Fn(f64, f64) -> bool,
    ) -> Option<NurbsCurve> {
        fit_curve(
            |t| -> Sample { Ok(point(t)) },
            range,
            breaks,
            |low, high| Ok(resolved(low, high)),
        )
        .expect("the fit fits the service policy")
    }

    fn worst(
        fitted: &CurveGeometry,
        point: impl Fn(f64) -> Option<Point3>,
        range: [f64; 2],
    ) -> f64 {
        let ctx = cadmpeg_test_support::service_decode_context();
        (0..=10_007)
            .map(|i| range[0] + (range[1] - range[0]) * f64::from(i) / 10_007.0)
            .map(|t| {
                crate::brep::evaluation::curve_point(&ctx, fitted, t)
                    .expect("evaluation admission")
                    .expect("a point on the fitted curve")
                    .get()
                    .distance(point(t).unwrap())
            })
            .fold(0.0, f64::max)
    }

    fn solved(nurbs: NurbsCurve) -> CurveGeometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs))
    }

    fn multiplicity(knots: &[f64], at: f64) -> usize {
        knots.iter().filter(|knot| **knot == at).count()
    }

    /// A helix of radius 100 mm over two turns: within the bound at dense
    /// samples, in its own parameter, C1 at every break, through both ends.
    #[test]
    fn a_smooth_curve_is_fitted_within_the_bound_in_its_own_parameter() {
        let helix = |t: f64| Some(Point3::new(100.0 * t.cos(), 100.0 * t.sin(), 7.0 * t));
        let range = [0.25, 0.25 + 4.0 * std::f64::consts::PI];
        let nurbs = fit(helix, range, &[], |_, _| true).unwrap();
        assert_eq!(nurbs.degree(), 3);
        let knots = nurbs.knots().to_vec();
        assert_eq!([knots[0], knots[knots.len() - 1]], range);
        assert!(knots[4..knots.len() - 4]
            .iter()
            .all(|knot| multiplicity(&knots, *knot) == 2));
        let poles = nurbs.control_points();
        assert!(poles[0].get().distance(helix(range[0]).unwrap()) < 1e-12);
        assert!(
            poles[poles.len() - 1]
                .get()
                .distance(helix(range[1]).unwrap())
                < 1e-12
        );
        let fitted = solved(nurbs);
        let departure = worst(&fitted, helix, range);
        assert!(departure <= FIT_TOLERANCE_MM, "{departure}");
        assert!(knots.len() > 40, "{}", knots.len());
    }

    /// A corner at a break is kept as a corner.  One the breaks do not name
    /// is still met within the bound, by spans bisected down onto it.
    #[test]
    fn a_corner_is_kept_at_a_break_and_bounded_off_one() {
        let corner = |t: f64| Some(Point3::new(t, (t - 1.0).abs() * 3.0, t * t));
        let nurbs = fit(corner, [0.0, 2.5], &[1.0], |_, _| true).unwrap();
        assert_eq!(multiplicity(nurbs.knots(), 1.0), 3);
        let fitted = solved(nurbs);
        let departure = worst(&fitted, corner, [0.0, 2.5]);
        assert!(departure <= FIT_TOLERANCE_MM, "{departure}");
        let nurbs = fit(corner, [0.0, 2.5], &[], |_, _| true).unwrap();
        assert_eq!(multiplicity(nurbs.knots(), 1.0), 0);
        let fitted = solved(nurbs);
        let departure = worst(&fitted, corner, [0.0, 2.5]);
        assert!(departure <= FIT_TOLERANCE_MM, "{departure}");
    }

    /// A smooth break is C1: the pole either side and the joint are in line.
    #[test]
    fn a_smooth_break_is_a_double_knot() {
        let cubic = |t: f64| Some(Point3::new(t, t * t, t * t * t));
        let nurbs = fit(cubic, [0.0, 2.0], &[0.5], |_, _| true).unwrap();
        assert_eq!(
            nurbs.knots().as_slice(),
            [0.0, 0.0, 0.0, 0.0, 0.5, 0.5, 2.0, 2.0, 2.0, 2.0]
        );
        let fitted = solved(nurbs);
        assert!(worst(&fitted, cubic, [0.0, 2.0]) < 1e-9);
    }

    /// A span the caller does not call resolved is halved though its samples
    /// are within the margin: a line, which one span fits exactly.
    #[test]
    fn an_unresolved_span_is_halved_whatever_its_samples_read() {
        let line = |t: f64| Some(Point3::new(100.0 * t, 0.0, 0.0));
        let whole = fit(line, [0.0, 1.0], &[], |_, _| true).unwrap();
        assert_eq!(whole.knots().len(), 8);
        let halved = fit(line, [0.0, 1.0], &[], |low, high| high - low < 0.3).unwrap();
        assert_eq!(
            halved.knots().as_slice(),
            [0.0, 0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0, 1.0]
        );
    }

    /// A break given twice, or out of order, is one break: a pcurve's knot
    /// vector repeats its interior knots.
    #[test]
    fn repeated_and_unordered_breaks_are_single_breaks() {
        let cubic = |t: f64| Some(Point3::new(t, t * t, t * t * t));
        let nurbs = fit(
            cubic,
            [0.0, 2.0],
            &[1.5, 0.5, 0.5, 1.5, 0.0, 2.0],
            |_, _| true,
        )
        .unwrap();
        assert_eq!(
            nurbs.knots().as_slice(),
            [0.0, 0.0, 0.0, 0.0, 0.5, 0.5, 1.5, 1.5, 2.0, 2.0, 2.0, 2.0]
        );
        // A double knot is C1: the joint lies between its neighbouring poles
        // in the ratio of the span widths.
        let poles = nurbs.control_points();
        let (before, after, joint) = (poles[2].get(), poles[3].get(), cubic(0.5).unwrap());
        let on_line = Point3::new(
            (2.0 * before.x + after.x) / 3.0,
            (2.0 * before.y + after.y) / 3.0,
            (2.0 * before.z + after.z) / 3.0,
        );
        assert!(on_line.distance(joint) < 1e-9, "{on_line:?} {joint:?}");
    }

    /// A point that does not evaluate, or is not finite, refuses the fit.
    #[test]
    fn an_unevaluable_point_refuses_the_fit() {
        let line = |t: f64| Some(Point3::new(t, 0.0, 0.0));
        assert!(fit(line, [0.0, 1.0], &[], |_, _| true).is_some());
        assert!(fit(
            |t| (t < 0.7).then(|| Point3::new(t, 0.0, 0.0)),
            [0.0, 1.0],
            &[],
            |_, _| true
        )
        .is_none());
        assert!(fit(
            |t| Some(Point3::new(t, f64::NAN, 0.0)),
            [0.0, 1.0],
            &[],
            |_, _| true
        )
        .is_none());
        assert!(fit(line, [1.0, 1.0], &[], |_, _| true).is_none());
        assert!(fit(line, [1.0, 0.0], &[], |_, _| true).is_none());
        // A jump is within the bound of no curve.
        let jump = |t: f64| Some(Point3::new(t, if t < 0.3 { 0.0 } else { 1.0 }, 0.0));
        assert!(fit(jump, [0.0, 1.0], &[], |_, _| true).is_none());
    }

    /// An evaluator refusal is the fit's error.
    #[test]
    fn a_refused_evaluation_refuses_the_fit() {
        let refused = fit_curve(
            |_| -> Sample { Err(CodecError::malformed("refused")) },
            [0.0, 1.0],
            &[],
            |_, _| Ok(true),
        );
        assert!(refused.is_err(), "{refused:?}");
    }
}
