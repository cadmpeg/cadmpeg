// SPDX-License-Identifier: Apache-2.0
//! One boundary piece: a parameter interval with its image in the carrier's
//! parameter plane and in model space.

use std::rc::Rc;

use cadmpeg_ir::features::FiniteVector3;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::Coedge;

use super::pcurve::Span;
use crate::canonical::curve::CurveChart;
use crate::canonical::surface::Chart;
use crate::context::Eval;
use crate::failure::{Measure, MeasureFailure};
use crate::reason::UnavailableReason;
use crate::report::UvSource;

/// Samples of one piece's unwrap reference or polyline.
const MAX_NESTING_SAMPLES_PER_PIECE: usize = 1 << 10;

/// Golden-section steps of the search for a singular point of the carrier
/// between two samples whose parameter image jumps.
const MAX_SINGULAR_APPROACH_STEPS: usize = 64;

/// Where a piece's points come from.
#[derive(Clone)]
pub(crate) enum Source<'ir> {
    /// A 3D curve interval inverted onto the carrier.
    Curve(Rc<CurveChart<'ir>>),
    /// A pcurve use mapped through the carrier.
    Pcurve {
        span: Span<'ir>,
        chart: Rc<Chart<'ir>>,
    },
    /// A straight parameter segment whose image is one model point.
    Segment {
        from: [f64; 2],
        to: [f64; 2],
        point: Point3,
        /// Whether the segment runs along a line where `u` is not defined.
        singular: bool,
    },
}

/// The coedge a piece belongs to and how it is resolved.
#[derive(Clone, Copy)]
pub(crate) struct CoedgeUse<'ir> {
    pub(crate) coedge: &'ir Coedge,
    pub(crate) uv_source: UvSource,
    pub(crate) alpha: f64,
}

/// A piece's point, tangent and parameter-plane image at one parameter.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Sample {
    pub(crate) uv: [f64; 2],
    pub(crate) duv: [f64; 2],
}

/// One piece of a loop, in traversal order from `t0` to `t1`.
#[derive(Clone)]
pub(crate) struct Piece<'ir> {
    pub(crate) coedge: Option<&'ir Coedge>,
    pub(crate) source: Source<'ir>,
    pub(crate) t0: f64,
    pub(crate) t1: f64,
    /// Whole periods added to the piece's parameter-plane image.
    pub(crate) shift: [f64; 2],
    pub(crate) uv_source: UvSource,
    /// The largest distance between a reference point and its inverted
    /// surface point.
    pub(crate) off_surface: f64,
    /// Unwrapped parameter-plane samples `(t, uv)` from `t0` to `t1`,
    /// without the shift.
    reference: Vec<(f64, [f64; 2])>,
    periods: [Option<f64>; 2],
    /// The distance within which a point is at a singular point of the
    /// chart.
    alpha: f64,
}

impl<'ir> Piece<'ir> {
    /// A synthesized segment from `from` to `to` at one model point.
    pub(crate) fn segment(from: [f64; 2], to: [f64; 2], point: Point3, singular: bool) -> Self {
        Self {
            coedge: None,
            source: Source::Segment {
                from,
                to,
                point,
                singular,
            },
            t0: 0.0,
            t1: 1.0,
            shift: [0.0, 0.0],
            uv_source: UvSource::Synthesized,
            off_surface: 0.0,
            reference: vec![(0.0, from), (1.0, to)],
            periods: [None, None],
            alpha: 0.0,
        }
    }

    /// A pcurve use on `chart`, traversed from `span.t0` to `span.t1`.
    pub(crate) fn pcurve(
        span: Span<'ir>,
        chart: &Chart<'ir>,
        use_: CoedgeUse<'ir>,
        off_surface: f64,
    ) -> Self {
        Self {
            coedge: Some(use_.coedge),
            source: Source::Pcurve {
                span,
                chart: Rc::new(*chart),
            },
            t0: span.t0,
            t1: span.t1,
            shift: [0.0, 0.0],
            uv_source: use_.uv_source,
            off_surface,
            reference: Vec::new(),
            periods: chart.periods(),
            alpha: use_.alpha,
        }
    }

    /// A pole segment of a degenerate coedge from `from` to `to` at `point`.
    pub(crate) fn pole(coedge: &'ir Coedge, from: [f64; 2], to: [f64; 2], point: Point3) -> Self {
        Self {
            coedge: Some(coedge),
            uv_source: UvSource::Pole,
            ..Self::segment(from, to, point, true)
        }
    }

    /// Whether the piece is a synthesized segment along a singular line.
    pub(crate) const fn singular_segment(&self) -> bool {
        matches!(self.source, Source::Segment { singular: true, .. })
    }

    /// Move the end of a synthesized segment by `delta` in `u`.
    pub(crate) fn extend_segment(&mut self, delta: f64) {
        if let Source::Segment { to, .. } = &mut self.source {
            to[0] += delta;
            if let Some((_, end)) = self.reference.last_mut() {
                end[0] += delta;
            }
        }
    }

    /// A curve interval inverted onto `chart`, with an unwrap reference fine
    /// enough that consecutive samples differ by less than a quarter period.
    pub(crate) fn inverted(
        eval: &Eval<'_, '_>,
        chart: &Chart<'_>,
        curve: Rc<CurveChart<'ir>>,
        [t0, t1]: [f64; 2],
        use_: CoedgeUse<'ir>,
    ) -> Measure<Self> {
        let alpha = use_.alpha;
        let periods = chart.periods();
        let mut count = 4_usize;
        let (reference, off_surface) = loop {
            let mut raw_samples: Vec<(f64, [f64; 2], bool)> = Vec::with_capacity(count + 1);
            let mut off_surface: f64 = 0.0;
            let mut seed: Option<[f64; 2]> = None;
            for index in 0..=count {
                let t = interpolate(t0, t1, index, count);
                let point = curve.point(eval, t)?;
                let Some(raw) = chart.invert(eval, point, seed, alpha)? else {
                    return Err(MeasureFailure::unavailable(
                        UnavailableReason::BoundaryInversionFailed,
                        None,
                        "a boundary point does not invert onto the surface",
                    ));
                };
                seed = Some(raw);
                let distance = chart.partials(eval, raw[0], raw[1])?.point.distance(point);
                if distance.is_nan() || distance > alpha {
                    return Err(MeasureFailure::unavailable(
                        UnavailableReason::BoundaryInversionFailed,
                        None,
                        format!("a boundary point lies {distance} mm off the surface"),
                    ));
                }
                off_surface = off_surface.max(distance);
                raw_samples.push((t, raw, chart.singular_at(point, alpha)));
            }
            let (reference, smooth) = unwrap(periods, &raw_samples)?;
            if smooth {
                break (reference, off_surface);
            }
            if count >= MAX_NESTING_SAMPLES_PER_PIECE {
                if passes_singular_point(eval, chart, &curve, &raw_samples, alpha)? {
                    return Err(MeasureFailure::unavailable(
                        UnavailableReason::BoundaryInversionFailed,
                        None,
                        "the curve passes through a singular point of the surface inside its interval",
                    ));
                }
                return Err(MeasureFailure::unavailable(
                    UnavailableReason::EvaluationWorkExhausted,
                    None,
                    "the piece's parameter image could not be unwrapped within the sample cap",
                ));
            }
            count *= 2;
        };
        Ok(Self {
            coedge: Some(use_.coedge),
            source: Source::Curve(curve),
            t0,
            t1,
            shift: [0.0, 0.0],
            uv_source: use_.uv_source,
            off_surface,
            reference,
            periods,
            alpha,
        })
    }

    /// The reference lift at `t`, interpolated between samples.
    fn reference_at(&self, t: f64) -> [f64; 2] {
        let span = self.t1 - self.t0;
        let fraction = if span == 0.0 {
            0.0
        } else {
            ((t - self.t0) / span).clamp(0.0, 1.0)
        };
        let first = self.reference.first().map_or([0.0; 2], |(_, uv)| *uv);
        let Some(last) = self.reference.len().checked_sub(1) else {
            return first;
        };
        let Some(last_f) = cadmpeg_core::convert::f64_from_index(last) else {
            return first;
        };
        let position = fraction * last_f;
        let Some(index) = cadmpeg_core::convert::truncate_f64_to_usize(position.floor()) else {
            return first;
        };
        let index = match last.checked_sub(1) {
            Some(limit) => index.min(limit),
            None => 0,
        };
        let (Some((ta, a)), Some((tb, b))) =
            (self.reference.get(index), self.reference.get(index + 1))
        else {
            return self.reference.first().map_or([0.0; 2], |(_, uv)| *uv);
        };
        let local = if tb == ta {
            0.0
        } else {
            ((t - ta) / (tb - ta)).clamp(0.0, 1.0)
        };
        [a[0] + (b[0] - a[0]) * local, a[1] + (b[1] - a[1]) * local]
    }

    /// The lifted parameters of a curve point at `t`, and whether the point
    /// is singular. At a singular point `u` is the reference lift.
    fn lifted(
        &self,
        eval: &Eval<'_, '_>,
        chart: &Chart<'_>,
        t: f64,
        point: Point3,
    ) -> Measure<([f64; 2], bool)> {
        let reference = self.reference_at(t);
        let seed = match chart.domain() {
            Some(domain) => std::array::from_fn(|axis| match self.periods[axis] {
                Some(period) => {
                    domain[axis][0] + (reference[axis] - domain[axis][0]).rem_euclid(period)
                }
                None => reference[axis],
            }),
            None => reference,
        };
        let Some(raw) = chart.invert(eval, point, Some(seed), self.alpha)? else {
            return Err(MeasureFailure::unavailable(
                UnavailableReason::BoundaryInversionFailed,
                None,
                "a boundary point does not invert onto the surface",
            ));
        };
        Ok(if chart.singular_at(point, self.alpha) {
            ([reference[0], raw[1]], true)
        } else {
            (nearest(self.periods, raw, reference), false)
        })
    }

    /// The piece at parameter `t`.
    pub(crate) fn sample(&self, eval: &Eval<'_, '_>, chart: &Chart<'_>, t: f64) -> Measure<Sample> {
        match &self.source {
            Source::Segment { from, to, .. } => Ok(Sample {
                uv: self.segment_uv(*from, *to, t),
                duv: [to[0] - from[0], to[1] - from[1]],
            }),
            Source::Pcurve { span, .. } => {
                let uv = span.uv(eval, t)?;
                Ok(Sample {
                    uv: [uv[0] + self.shift[0], uv[1] + self.shift[1]],
                    duv: span.duv(eval, t)?,
                })
            }
            Source::Curve(curve) => {
                let point = curve.point(eval, t)?;
                let tangent = curve.tangent(eval, t)?;
                let (lifted, singular) = self.lifted(eval, chart, t, point)?;
                let partials = chart.partials(eval, lifted[0], lifted[1])?;
                let duv = if singular {
                    singular_velocity(partials.dv, tangent)?
                } else {
                    parameter_velocity(partials.du, partials.dv, tangent)?
                };
                Ok(Sample {
                    uv: [lifted[0] + self.shift[0], lifted[1] + self.shift[1]],
                    duv,
                })
            }
        }
    }

    fn segment_uv(&self, from: [f64; 2], to: [f64; 2], t: f64) -> [f64; 2] {
        [
            from[0] + (to[0] - from[0]) * t + self.shift[0],
            from[1] + (to[1] - from[1]) * t + self.shift[1],
        ]
    }

    /// The lifted parameters at `t`, without the parameter velocity.
    pub(crate) fn uv(&self, eval: &Eval<'_, '_>, chart: &Chart<'_>, t: f64) -> Measure<[f64; 2]> {
        match &self.source {
            Source::Segment { from, to, .. } => Ok(self.segment_uv(*from, *to, t)),
            Source::Pcurve { span, .. } => {
                let uv = span.uv(eval, t)?;
                Ok([uv[0] + self.shift[0], uv[1] + self.shift[1]])
            }
            Source::Curve(curve) => {
                let point = curve.point(eval, t)?;
                let (lifted, _) = self.lifted(eval, chart, t, point)?;
                Ok([lifted[0] + self.shift[0], lifted[1] + self.shift[1]])
            }
        }
    }

    pub(crate) fn start_uv(&self, eval: &Eval<'_, '_>, chart: &Chart<'_>) -> Measure<[f64; 2]> {
        self.uv(eval, chart, self.t0)
    }

    pub(crate) fn end_uv(&self, eval: &Eval<'_, '_>, chart: &Chart<'_>) -> Measure<[f64; 2]> {
        self.uv(eval, chart, self.t1)
    }

    pub(crate) fn start_point(&self, eval: &Eval<'_, '_>) -> Measure<Point3> {
        self.point(eval, self.t0)
    }

    pub(crate) fn end_point(&self, eval: &Eval<'_, '_>) -> Measure<Point3> {
        self.point(eval, self.t1)
    }

    /// The model point at `t`.
    pub(crate) fn point(&self, eval: &Eval<'_, '_>, t: f64) -> Measure<Point3> {
        match &self.source {
            Source::Segment { point, .. } => Ok(*point),
            Source::Curve(curve) => curve.point(eval, t),
            Source::Pcurve { span, chart } => {
                let uv = span.uv(eval, t)?;
                Ok(chart.partials(eval, uv[0], uv[1])?.point)
            }
        }
    }

    /// Parameters inside the piece where its curve's smoothness may break.
    pub(crate) fn breaks(&self) -> Vec<f64> {
        match &self.source {
            Source::Curve(curve) => curve.breaks(),
            Source::Pcurve { span, .. } => span.breaks(),
            Source::Segment { .. } => Vec::new(),
        }
    }

    /// Whether the piece is a synthesized segment.
    pub(crate) const fn synthesized(&self) -> bool {
        matches!(self.source, Source::Segment { .. })
    }

    /// The parameter-plane polyline of the piece, refined until each chord's
    /// midpoint deviates from the piece by at most `deviation`.
    pub(crate) fn polyline(
        &self,
        eval: &Eval<'_, '_>,
        chart: &Chart<'_>,
        deviation: f64,
    ) -> Measure<Vec<[f64; 2]>> {
        Ok(self
            .parameter_polyline(eval, chart, deviation)?
            .into_iter()
            .map(|(_, uv)| uv)
            .collect())
    }

    /// The polyline of [`Piece::polyline`] with the piece parameter of each
    /// point.
    pub(crate) fn parameter_polyline(
        &self,
        eval: &Eval<'_, '_>,
        chart: &Chart<'_>,
        deviation: f64,
    ) -> Measure<Vec<(f64, [f64; 2])>> {
        if self.synthesized() {
            let start = self.uv(eval, chart, self.t0)?;
            let end = self.uv(eval, chart, self.t1)?;
            return Ok(vec![(self.t0, start), (self.t1, end)]);
        }
        let mut points: Vec<(f64, [f64; 2])> = match &self.source {
            Source::Pcurve { span, .. } => {
                let mut parameters = vec![self.t0];
                let mut inner = span.breaks();
                inner.sort_by(f64::total_cmp);
                if self.t1 < self.t0 {
                    inner.reverse();
                }
                parameters.extend(inner);
                parameters.push(self.t1);
                let mut points = Vec::with_capacity(parameters.len());
                for t in parameters {
                    points.push((t, self.uv(eval, chart, t)?));
                }
                points
            }
            _ => self
                .reference
                .iter()
                .map(|(t, uv)| (*t, [uv[0] + self.shift[0], uv[1] + self.shift[1]]))
                .collect(),
        };
        let mut index = 0;
        while index + 1 < points.len() {
            let (ta, a) = points[index];
            let (tb, b) = points[index + 1];
            let middle_t = ta.midpoint(tb);
            let middle = self.uv(eval, chart, middle_t)?;
            let chord = [a[0].midpoint(b[0]), a[1].midpoint(b[1])];
            let off = (middle[0] - chord[0]).hypot(middle[1] - chord[1]);
            if off > deviation && points.len() < MAX_NESTING_SAMPLES_PER_PIECE {
                points.insert(index + 1, (middle_t, middle));
            } else {
                index += 1;
            }
        }
        Ok(points)
    }
}

/// `t0 + (t1 − t0)·index/count`, exact at both ends.
fn interpolate(t0: f64, t1: f64, index: usize, count: usize) -> f64 {
    if index == 0 {
        return t0;
    }
    if index == count {
        return t1;
    }
    let (Some(index), Some(count)) = (
        cadmpeg_core::convert::f64_from_index(index),
        cadmpeg_core::convert::f64_from_index(count),
    ) else {
        return t0;
    };
    t0 + (t1 - t0) * (index / count)
}

/// Parameter-plane samples `(t, uv)` in parameter order.
type Reference = Vec<(f64, [f64; 2])>;

/// Unwrap raw samples `(t, uv, singular)` into a continuous lift. A
/// singular sample takes `u` from its nearest regular neighbour, preferring
/// the previous one. The second value is whether consecutive lifted samples
/// differ by less than a quarter period in every periodic direction.
fn unwrap(
    periods: [Option<f64>; 2],
    samples: &[(f64, [f64; 2], bool)],
) -> Measure<(Reference, bool)> {
    let regular: Vec<usize> = samples
        .iter()
        .enumerate()
        .filter(|(_, (_, _, singular))| !singular)
        .map(|(index, _)| index)
        .collect();
    let (Some(&first), Some(&last)) = (regular.first(), regular.last()) else {
        return Err(MeasureFailure::unavailable(
            UnavailableReason::BoundaryInversionFailed,
            None,
            "the piece lies at a singular point of the surface",
        ));
    };
    if regular.windows(2).any(|pair| pair[1] != pair[0] + 1) {
        return Err(MeasureFailure::unavailable(
            UnavailableReason::BoundaryInversionFailed,
            None,
            "the curve passes through a singular point of the surface inside its interval",
        ));
    }
    let mut lifted: Vec<(f64, [f64; 2])> = Vec::with_capacity(samples.len());
    let mut smooth = true;
    for &(t, raw, _) in &samples[first..=last] {
        let next = match lifted.last() {
            Some((_, previous)) => {
                let next = nearest(periods, raw, *previous);
                for direction in 0..2 {
                    if let Some(period) = periods[direction] {
                        smooth &= (next[direction] - previous[direction]).abs() < period / 4.0;
                    }
                }
                next
            }
            None => raw,
        };
        lifted.push((t, next));
    }
    let head = lifted.first().map_or([0.0; 2], |(_, uv)| *uv);
    let tail = lifted.last().map_or([0.0; 2], |(_, uv)| *uv);
    let mut reference: Vec<(f64, [f64; 2])> = samples[..first]
        .iter()
        .map(|&(t, raw, _)| (t, [head[0], raw[1]]))
        .collect();
    reference.extend(lifted);
    reference.extend(
        samples[last + 1..]
            .iter()
            .map(|&(t, raw, _)| (t, [tail[0], raw[1]])),
    );
    Ok((reference, smooth))
}

/// Whether the curve passes within `alpha` of a singular point of `chart`
/// between two consecutive samples `(t, uv, singular)` whose nearest lifts
/// differ by a quarter period or more. Near a singular point the parameter
/// image turns by up to half a period over a vanishing curve length, so
/// such a pair brackets the point; a golden-section search of the distance
/// to the singular points decides.
fn passes_singular_point(
    eval: &Eval<'_, '_>,
    chart: &Chart<'_>,
    curve: &CurveChart<'_>,
    samples: &[(f64, [f64; 2], bool)],
    alpha: f64,
) -> Measure<bool> {
    let singular: Vec<Point3> = chart
        .apex()
        .map(|(_, apex)| apex)
        .into_iter()
        .chain(chart.poles().into_iter().flatten().map(|(_, pole)| pole))
        .collect();
    if singular.is_empty() {
        return Ok(false);
    }
    let periods = chart.periods();
    let distance = |t: f64| -> Measure<f64> {
        let point = curve.point(eval, t)?;
        Ok(singular
            .iter()
            .map(|candidate| candidate.distance(point))
            .fold(f64::INFINITY, f64::min))
    };
    for pair in samples.windows(2) {
        let ((ta, a, _), (tb, b, _)) = (pair[0], pair[1]);
        let lifted = nearest(periods, b, a);
        let jumps = (0..2).any(|axis| {
            periods[axis].is_some_and(|period| (lifted[axis] - a[axis]).abs() >= period / 4.0)
        });
        if jumps && golden_minimum(eval, &distance, [ta, tb])? <= alpha {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The smallest value of `value` that a golden-section search finds on
/// `[low, high]`, including both ends.
fn golden_minimum(
    eval: &Eval<'_, '_>,
    value: &dyn Fn(f64) -> Measure<f64>,
    [low, high]: [f64; 2],
) -> Measure<f64> {
    let ratio = (5.0_f64.sqrt() - 1.0) / 2.0;
    let (mut lower, mut upper) = (low, high);
    let mut best = value(lower)?.min(value(upper)?);
    let mut left = upper - ratio * (upper - lower);
    let mut right = lower + ratio * (upper - lower);
    let (mut at_left, mut at_right) = (value(left)?, value(right)?);
    for _ in 0..MAX_SINGULAR_APPROACH_STEPS {
        eval.charge(1)?;
        best = best.min(at_left).min(at_right);
        if at_left <= at_right {
            upper = right;
            right = left;
            at_right = at_left;
            left = upper - ratio * (upper - lower);
            at_left = value(left)?;
        } else {
            lower = left;
            left = right;
            at_left = at_right;
            right = lower + ratio * (upper - lower);
            at_right = value(right)?;
        }
    }
    Ok(best.min(at_left).min(at_right))
}

/// `raw` lifted by whole periods to the value nearest `reference`.
fn nearest(periods: [Option<f64>; 2], raw: [f64; 2], reference: [f64; 2]) -> [f64; 2] {
    std::array::from_fn(|direction| match periods[direction] {
        Some(period) => {
            raw[direction] + ((reference[direction] - raw[direction]) / period).round() * period
        }
        None => raw[direction],
    })
}

/// The parameter velocity at a singular point, where `S_u` vanishes: the
/// minimum-norm solution `u' = 0`, `v' = C'·S_v / |S_v|²`.
fn singular_velocity(dv: Vector3, tangent: Vector3) -> Measure<[f64; 2]> {
    let norm = dv.dot(dv);
    if norm.is_finite() && norm > 0.0 {
        Ok([0.0, tangent.dot(dv) / norm])
    } else {
        Err(MeasureFailure::unavailable(
            UnavailableReason::EvaluationNoValue,
            None,
            "the surface partials vanish at a boundary point",
        ))
    }
}

/// The parameter velocity whose surface image is the curve tangent: the
/// least-squares solution of `S_u·u' + S_v·v' = C'`.
fn parameter_velocity(du: Vector3, dv: Vector3, tangent: Vector3) -> Measure<[f64; 2]> {
    let no_value = || {
        MeasureFailure::unavailable(
            UnavailableReason::EvaluationNoValue,
            None,
            "the surface partials are singular at a boundary point",
        )
    };
    let (Some(du), Some(dv)) = (FiniteVector3::new(du), FiniteVector3::new(dv)) else {
        return Err(no_value());
    };
    cadmpeg_ir::math::solve::least_squares_step(du, dv, tangent)
        .map(|(u, v)| [u.get(), v.get()])
        .ok_or_else(no_value)
}

#[cfg(test)]
mod tests {
    use super::unwrap;

    const TAU: f64 = std::f64::consts::TAU;
    const PI: f64 = std::f64::consts::PI;

    #[test]
    fn unwrapping_lifts_samples_across_the_period() {
        let samples = [
            (0.0, [PI - 0.2, 0.0], false),
            (0.5, [PI - 0.1, 0.0], false),
            (1.0, [-PI + 0.1, 0.0], false),
        ];
        let (reference, smooth) = unwrap([Some(TAU), None], &samples).unwrap();
        assert!(smooth);
        assert!((reference[2].1[0] - (PI + 0.1)).abs() < 1.0e-15);
    }

    #[test]
    fn singular_end_samples_take_u_from_their_neighbour() {
        let samples = [
            (0.0, [0.0, 7.0], true),
            (0.5, [1.2, 3.0], false),
            (1.0, [1.2, 1.0], false),
        ];
        let (reference, _) = unwrap([Some(TAU), None], &samples).unwrap();
        assert_eq!(reference[0], (0.0, [1.2, 7.0]));
        let samples = [(0.0, [1.2, 1.0], false), (1.0, [0.0, 7.0], true)];
        let (reference, _) = unwrap([Some(TAU), None], &samples).unwrap();
        assert_eq!(reference[1], (1.0, [1.2, 7.0]));
    }

    #[test]
    fn a_singular_sample_inside_the_interval_is_refused() {
        let samples = [
            (0.0, [1.0, 1.0], false),
            (0.5, [0.0, 0.0], true),
            (1.0, [1.0 + PI, 1.0], false),
        ];
        assert!(unwrap([Some(TAU), None], &samples).is_err());
        let all_singular = [(0.0, [0.0, 0.0], true), (1.0, [0.0, 0.0], true)];
        assert!(unwrap([Some(TAU), None], &all_singular).is_err());
    }
}
