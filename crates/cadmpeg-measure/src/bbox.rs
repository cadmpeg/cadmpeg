// SPDX-License-Identifier: Apache-2.0
//! Axis-aligned boxes of curves and faces.
//!
//! A coordinate restricted to a curve interval attains its extremes at the
//! ends or at roots of its derivative. Lines and conics have closed forms;
//! other curves use the extremum search.

use cadmpeg_ir::math::{Point3, Vector3};

use crate::canonical::curve::{CurveChart, CurveShape};
use crate::context::Eval;
use crate::failure::{Measure, MeasureFailure};
use crate::geom::Aabb;
use crate::quadrature::extrema;

/// The coordinate axes.
fn axes() -> [Vector3; 3] {
    [
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
    ]
}

/// `direction · (point − 0)`.
fn coordinate_along(point: Point3, direction: Vector3) -> f64 {
    point.vector_from(Point3::new(0.0, 0.0, 0.0)).dot(direction)
}

/// The box of a curve interval and the error of its sampled extremes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CurveBox {
    pub(crate) aabb: Aabb,
    pub(crate) numeric_error: f64,
}

/// Whether `angle` lies in `[low, high]` modulo `2π`.
fn angle_within(angle: f64, low: f64, high: f64) -> Option<f64> {
    let tau = std::f64::consts::TAU;
    let shifted = angle + ((low - angle) / tau).ceil() * tau;
    (shifted <= high).then_some(shifted)
}

/// The range of `direction · C(t)` over a curve interval.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CurveRange {
    pub(crate) min: f64,
    pub(crate) max: f64,
    pub(crate) numeric_error: f64,
}

/// The range of the linear coordinate `direction · P` of `chart` between
/// `start` and `end`.
pub(crate) fn curve_range(
    eval: &Eval<'_, '_>,
    chart: &CurveChart<'_>,
    [start, end]: [f64; 2],
    direction: Vector3,
) -> Measure<CurveRange> {
    let (low, high) = (start.min(end), start.max(end));
    let at = |t: f64| -> Measure<f64> { Ok(coordinate_along(chart.point(eval, t)?, direction)) };
    let ends = [at(low)?, at(high)?];
    let mut range = CurveRange {
        min: ends[0].min(ends[1]),
        max: ends[0].max(ends[1]),
        numeric_error: 0.0,
    };
    match &chart.shape {
        CurveShape::Line { .. } | CurveShape::Degenerate { .. } => Ok(range),
        CurveShape::Conic {
            e1,
            e2,
            major,
            minor,
            ..
        } => {
            let critical = (minor * e2.dot(direction)).atan2(major * e1.dot(direction));
            for angle in [critical, critical + std::f64::consts::PI] {
                if let Some(t) = angle_within(angle, low, high) {
                    let value = at(t)?;
                    range.min = range.min.min(value);
                    range.max = range.max.max(value);
                }
            }
            Ok(range)
        }
        CurveShape::Evaluated => {
            let breaks = chart.breaks();
            let mut charge = |units| eval.charge(units);
            let mut f = |t: f64| -> Measure<(f64, f64)> {
                Ok((
                    coordinate_along(chart.point(eval, t)?, direction),
                    chart.tangent(eval, t)?.dot(direction),
                ))
            };
            let found = extrema(&mut charge, &mut f, low, high, &breaks)?;
            range.min = range.min.min(found.min);
            range.max = range.max.max(found.max);
            range.numeric_error = found.error;
            Ok(range)
        }
        CurveShape::Unsupported(reason) => Err(MeasureFailure::unavailable(
            *reason,
            Some(chart.id()),
            "the curve has no parameterization",
        )),
    }
}

/// The box of `chart` between `start` and `end`.
pub(crate) fn curve_box(
    eval: &Eval<'_, '_>,
    chart: &CurveChart<'_>,
    interval: [f64; 2],
) -> Measure<CurveBox> {
    let mut aabb = Aabb::of_point(chart.point(eval, interval[0])?);
    let mut numeric_error: f64 = 0.0;
    for (axis, direction) in axes().into_iter().enumerate() {
        let range = curve_range(eval, chart, interval, direction)?;
        aabb.min[axis] = range.min;
        aabb.max[axis] = range.max;
        numeric_error = numeric_error.max(range.numeric_error);
    }
    Ok(CurveBox {
        aabb,
        numeric_error,
    })
}

/// A box of one point.
pub(crate) fn point_box(point: Point3) -> CurveBox {
    CurveBox {
        aabb: Aabb::of_point(point),
        numeric_error: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::angle_within;

    #[test]
    fn angles_wrap_into_the_interval() {
        let pi = std::f64::consts::PI;
        assert_eq!(angle_within(0.0, -0.5, 0.5), Some(0.0));
        assert!((angle_within(-pi / 2.0, 0.0, 2.0 * pi).unwrap() - 1.5 * pi).abs() < 1.0e-15);
        assert_eq!(angle_within(pi, 0.0, 1.0), None);
        assert!((angle_within(0.0, 1.5 * pi, 2.5 * pi).unwrap() - 2.0 * pi).abs() < 1.0e-15);
    }
}
