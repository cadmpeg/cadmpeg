// SPDX-License-Identifier: Apache-2.0
//! Native conic trim parameters and their STEP spline representation.

use cadmpeg_core::decode::View;
use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveNurbs};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::scalar::PositiveReal;
use cadmpeg_ir::topology::IncreasingParameterInterval;

use crate::families::b2::records::B2Circle;
use crate::wire::bytes::compact_int;

use super::super::rolling_ball_patch::fit_edge_with_error;
use super::near;

const CHART_FIT_ERROR: f64 = 1.0e-7;

fn fit_curve(
    range: [f64; 2],
    evaluate: impl Fn(f64) -> [f64; 2],
) -> Option<(PcurveGeometry, [f64; 2])> {
    let curve = fit_edge_with_error(
        range,
        |t| {
            let [u, v] = evaluate(t);
            (u.is_finite() && v.is_finite()).then_some([u, v, 0.0])
        },
        CHART_FIT_ERROR,
    )?;
    let points = curve
        .control_points()
        .into_iter()
        .map(|point| {
            let p = point.get();
            Point2::new(p.x, p.y)
        })
        .collect();
    let nurbs =
        PcurveNurbs::from_lanes(curve.degree(), curve.knots().clone(), points, None, false).ok()?;
    Some((PcurveGeometry::Nurbs { nurbs }, range))
}

pub(super) fn circle(circle: &B2Circle) -> Option<(PcurveGeometry, [f64; 2])> {
    let [u, v] = circle.center_pair.get();
    fit_circle([u, v], circle.radius.get(), circle.range.endpoints())
}

pub(super) fn owned_circle(bytes: &[u8]) -> Option<(PcurveGeometry, [f64; 2])> {
    let mut at = 0;
    compact_int(bytes, &mut at)?;
    let mut view = View::over_retained(bytes.get(at..)?);
    let center = [view.f64_le()?, view.f64_le()?];
    match view.u8()? {
        0x05 if view.u8()? == 0x05 => (),
        0x0d => (),
        _ => return None,
    }
    let radius = PositiveReal::new(view.f64_le()?)?.get();
    let range = IncreasingParameterInterval::new([view.f64_le()?, view.f64_le()?])?;
    if view.u8()? != 1 || !view.f64_le()?.is_finite() || !view.is_empty() {
        return None;
    }
    fit_circle(center, radius, range.endpoints())
}

fn fit_circle([u, v]: [f64; 2], r: f64, range: [f64; 2]) -> Option<(PcurveGeometry, [f64; 2])> {
    fit_curve(range, |t| {
        let (sin, cos) = (t / r).sin_cos();
        [r.mul_add(cos, u), r.mul_add(sin, v)]
    })
}

#[derive(Clone, Copy)]
pub(super) struct SphereCircle {
    pub(super) range: [f64; 2],
    radius: f64,
    slope: f64,
    phase: f64,
}

impl SphereCircle {
    pub(super) fn fit(self, range: [f64; 2]) -> Option<PcurveGeometry> {
        fit_curve(range, |t| {
            [
                t,
                self.radius * (self.slope * (t / self.radius - self.phase).cos()).atan(),
            ]
        })
        .map(|(curve, _)| curve)
    }
}

pub(super) fn sphere_circle(bytes: &[u8], radius: f64) -> Option<SphereCircle> {
    let mut at = 0;
    compact_int(bytes, &mut at)?;
    let mut view = View::over_retained(bytes.get(at..)?);
    let u = IncreasingParameterInterval::new([view.f64_le()?, view.f64_le()?])?;
    let v = IncreasingParameterInterval::new([view.f64_le()?, view.f64_le()?])?;
    if view.take(3)? != [0x41, 0x05, 0x81] {
        return None;
    }
    let shift = view.f64_le()?;
    let direction = view.f64_le()?;
    if direction.abs() != 1.0 || view.f64_le()? != 0.0 || view.take(2)? != [0x41, 0x1d] {
        return None;
    }
    let scale = PositiveReal::new(view.f64_le()?)?.get();
    let slope = view.f64_le()?;
    let reciprocal = view.f64_le()?;
    let phase = view.f64_le()? + shift / scale;
    if !near(scale, radius)
        || !near(reciprocal, -direction / scale)
        || !near(v.upper() - v.lower(), std::f64::consts::TAU * scale)
        || view.f64_le()? != 0.0
        || view.remaining() != 0
        || !phase.is_finite()
        || !slope.is_finite()
    {
        return None;
    }
    Some(SphereCircle {
        range: u.endpoints(),
        radius: scale,
        slope,
        phase,
    })
}

pub(super) fn spline(bytes: &[u8]) -> Option<(PcurveGeometry, [f64; 2])> {
    let mut at = 0;
    compact_int(bytes, &mut at)?;
    let mut view = View::over_retained(bytes.get(at..)?);
    let inline = |view: &mut View<'_>| {
        let token = view.u8()?;
        (token % 4 == 1).then_some(u32::from(token / 4))
    };
    let kind = view.u8()?;
    let spans = match kind {
        0x1e => 1,
        0x1a => inline(&mut view)?,
        0x16 => {
            let center = view.f64_le()?;
            if !center.is_finite() || center <= 0.0 {
                return None;
            }
            1
        }
        _ => return None,
    };
    let final_span = view.f64_le()?;
    let degree = inline(&mut view)?;
    let count = inline(&mut view)?;
    if !matches!((kind, degree), (0x1e, 1 | 5) | (0x1a, 5) | (0x16, 11))
        || count < 2
        || count - 1 != spans
        || view.u8()? != 0x0c
    {
        return None;
    }
    let distinct = (0..count)
        .map(|_| view.f64_le())
        .collect::<Option<Vec<_>>>()?;
    if !cadmpeg_ir::geometry::nurbs::knots_strictly_increasing(&distinct)
        || !near(*distinct.last()? - distinct[distinct.len() - 2], final_span)
    {
        return None;
    }
    let range = [distinct[0], *distinct.last()?];
    let points = (0..degree + 1 + 3 * (count - 2))
        .map(|_| Some(Point2::new(view.f64_le()?, view.f64_le()?)))
        .collect::<Option<Vec<_>>>()?;
    if !view.is_empty() {
        return None;
    }
    let last = distinct.len() - 1;
    let knots: Vec<_> = distinct
        .into_iter()
        .enumerate()
        .flat_map(|(i, t)| {
            let multiplicity = if i == 0 || i == last { degree + 1 } else { 3 };
            (0..multiplicity).map(move |_| t)
        })
        .collect();
    let nurbs = PcurveNurbs::from_lanes(degree, knots, points, None, false).ok()?;
    Some((PcurveGeometry::Nurbs { nurbs }, range))
}

#[cfg(test)]
mod tests {
    use super::spline;

    #[test]
    fn class21_spline_uses_the_stored_knot_origin() {
        let mut bytes = vec![0x05, 0x1e];
        bytes.extend(3.0_f64.to_le_bytes());
        bytes.extend([0x05, 0x09, 0x0c]);
        for value in [-1.0_f64, 2.0, 0.0, 0.0, 1.0, 1.0] {
            bytes.extend(value.to_le_bytes());
        }
        let (_, range) = spline(&bytes).unwrap();
        assert_eq!(range, [-1.0, 2.0]);
    }
}
