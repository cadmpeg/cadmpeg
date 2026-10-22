// SPDX-License-Identifier: Apache-2.0
//! One boundary piece: a parameter interval with its image in the carrier's
//! parameter plane and in model space.

use std::rc::Rc;

use cadmpeg_ir::features::FiniteVector3;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::Coedge;

use crate::canonical::curve::CurveChart;
use crate::canonical::surface::Chart;
use crate::context::Eval;
use crate::failure::{Measure, MeasureFailure};
use crate::reason::UnavailableReason;
use crate::report::UvSource;

/// Samples of one piece's unwrap reference or polyline.
const MAX_NESTING_SAMPLES_PER_PIECE: usize = 1 << 10;

/// Where a piece's points come from.
#[derive(Clone)]
pub(crate) enum Source<'ir> {
    /// A 3D curve interval inverted onto the carrier.
    Curve(Rc<CurveChart<'ir>>),
    /// A straight parameter segment whose image is one model point.
    Segment {
        from: [f64; 2],
        to: [f64; 2],
        point: Point3,
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
}

impl<'ir> Piece<'ir> {
    /// A synthesized segment from `from` to `to` at one model point.
    pub(crate) fn segment(from: [f64; 2], to: [f64; 2], point: Point3) -> Self {
        Self {
            coedge: None,
            source: Source::Segment { from, to, point },
            t0: 0.0,
            t1: 1.0,
            shift: [0.0, 0.0],
            uv_source: UvSource::Synthesized,
            off_surface: 0.0,
            reference: vec![(0.0, from), (1.0, to)],
            periods: [None, None],
        }
    }

    /// A curve interval inverted onto `chart`, with an unwrap reference fine
    /// enough that consecutive samples differ by less than a quarter period.
    pub(crate) fn inverted(
        eval: &Eval<'_, '_>,
        chart: &Chart,
        curve: Rc<CurveChart<'ir>>,
        [t0, t1]: [f64; 2],
        use_: CoedgeUse<'ir>,
    ) -> Measure<Self> {
        let alpha = use_.alpha;
        let periods = chart.periods();
        let mut count = 4_usize;
        let (reference, off_surface) = loop {
            let mut reference: Vec<(f64, [f64; 2])> = Vec::with_capacity(count + 1);
            let mut off_surface: f64 = 0.0;
            let mut smooth = true;
            for index in 0..=count {
                let t = interpolate(t0, t1, index, count);
                let point = curve.point(eval, t)?;
                let raw = chart.invert(point);
                let distance = chart.partials(raw[0], raw[1]).point.distance(point);
                if distance.is_nan() || distance > alpha {
                    return Err(MeasureFailure::unavailable(
                        UnavailableReason::BoundaryInversionFailed,
                        None,
                        format!("a boundary point lies {distance} mm off the surface"),
                    ));
                }
                off_surface = off_surface.max(distance);
                let lifted = match reference.last() {
                    Some((_, previous)) => nearest(periods, raw, *previous),
                    None => raw,
                };
                if let Some((_, previous)) = reference.last() {
                    for direction in 0..2 {
                        if let Some(period) = periods[direction] {
                            smooth &=
                                (lifted[direction] - previous[direction]).abs() < period / 4.0;
                        }
                    }
                }
                reference.push((t, lifted));
            }
            if smooth {
                break (reference, off_surface);
            }
            if count >= MAX_NESTING_SAMPLES_PER_PIECE {
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

    /// The piece at parameter `t`.
    pub(crate) fn sample(&self, eval: &Eval<'_, '_>, chart: &Chart, t: f64) -> Measure<Sample> {
        match &self.source {
            Source::Segment { from, to, .. } => {
                let uv = [
                    from[0] + (to[0] - from[0]) * t + self.shift[0],
                    from[1] + (to[1] - from[1]) * t + self.shift[1],
                ];
                Ok(Sample {
                    uv,
                    duv: [to[0] - from[0], to[1] - from[1]],
                })
            }
            Source::Curve(curve) => {
                let point = curve.point(eval, t)?;
                let tangent = curve.tangent(eval, t)?;
                let raw = chart.invert(point);
                let lifted = nearest(self.periods, raw, self.reference_at(t));
                let partials = chart.partials(raw[0], raw[1]);
                let duv = parameter_velocity(partials.du, partials.dv, tangent)?;
                Ok(Sample {
                    uv: [lifted[0] + self.shift[0], lifted[1] + self.shift[1]],
                    duv,
                })
            }
        }
    }

    pub(crate) fn start_uv(&self, eval: &Eval<'_, '_>, chart: &Chart) -> Measure<[f64; 2]> {
        Ok(self.sample(eval, chart, self.t0)?.uv)
    }

    pub(crate) fn end_uv(&self, eval: &Eval<'_, '_>, chart: &Chart) -> Measure<[f64; 2]> {
        Ok(self.sample(eval, chart, self.t1)?.uv)
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
        }
    }

    /// Parameters inside the piece where its curve's smoothness may break.
    pub(crate) fn breaks(&self) -> Vec<f64> {
        match &self.source {
            Source::Curve(curve) => curve.breaks(),
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
        chart: &Chart,
        deviation: f64,
    ) -> Measure<Vec<[f64; 2]>> {
        if self.synthesized() {
            let start = self.sample(eval, chart, self.t0)?.uv;
            let end = self.sample(eval, chart, self.t1)?.uv;
            return Ok(vec![start, end]);
        }
        let mut points: Vec<(f64, [f64; 2])> = self
            .reference
            .iter()
            .map(|(t, uv)| (*t, [uv[0] + self.shift[0], uv[1] + self.shift[1]]))
            .collect();
        let mut index = 0;
        while index + 1 < points.len() {
            let (ta, a) = points[index];
            let (tb, b) = points[index + 1];
            let middle_t = ta.midpoint(tb);
            let middle = self.sample(eval, chart, middle_t)?.uv;
            let chord = [a[0].midpoint(b[0]), a[1].midpoint(b[1])];
            let off = (middle[0] - chord[0]).hypot(middle[1] - chord[1]);
            if off > deviation && points.len() < MAX_NESTING_SAMPLES_PER_PIECE {
                points.insert(index + 1, (middle_t, middle));
            } else {
                index += 1;
            }
        }
        Ok(points.into_iter().map(|(_, uv)| uv).collect())
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

/// `raw` lifted by whole periods to the value nearest `reference`.
fn nearest(periods: [Option<f64>; 2], raw: [f64; 2], reference: [f64; 2]) -> [f64; 2] {
    std::array::from_fn(|direction| match periods[direction] {
        Some(period) => {
            raw[direction] + ((reference[direction] - raw[direction]) / period).round() * period
        }
        None => raw[direction],
    })
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
