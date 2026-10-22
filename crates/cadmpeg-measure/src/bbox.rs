// SPDX-License-Identifier: Apache-2.0
//! Axis-aligned boxes of curves and faces.
//!
//! A coordinate restricted to a curve interval attains its extremes at the
//! ends or at roots of its derivative. Lines and conics have closed forms;
//! other curves use the extremum search.

use cadmpeg_ir::math::Point3;

use crate::canonical::curve::{CurveChart, CurveShape};
use crate::context::Eval;
use crate::failure::{Measure, MeasureFailure};
use crate::geom::{component, coordinate, Aabb};
use crate::quadrature::extrema;

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

/// The box of `chart` between `start` and `end`.
pub(crate) fn curve_box(
    eval: &Eval<'_, '_>,
    chart: &CurveChart<'_>,
    [start, end]: [f64; 2],
) -> Measure<CurveBox> {
    let (low, high) = (start.min(end), start.max(end));
    match &chart.shape {
        CurveShape::Line { .. } | CurveShape::Degenerate { .. } => {
            let mut aabb = Aabb::of_point(chart.point(eval, low)?);
            aabb.include(chart.point(eval, high)?);
            Ok(CurveBox {
                aabb,
                numeric_error: 0.0,
            })
        }
        CurveShape::Conic {
            e1,
            e2,
            major,
            minor,
            ..
        } => {
            let mut aabb = Aabb::of_point(chart.point(eval, low)?);
            aabb.include(chart.point(eval, high)?);
            for axis in 0..3 {
                let critical = (minor * component(*e2, axis)).atan2(major * component(*e1, axis));
                for angle in [critical, critical + std::f64::consts::PI] {
                    if let Some(t) = angle_within(angle, low, high) {
                        aabb.include(chart.point(eval, t)?);
                    }
                }
            }
            Ok(CurveBox {
                aabb,
                numeric_error: 0.0,
            })
        }
        CurveShape::Evaluated => {
            let mut aabb = Aabb::of_point(chart.point(eval, low)?);
            aabb.include(chart.point(eval, high)?);
            let mut numeric_error: f64 = 0.0;
            let breaks = chart.breaks();
            for axis in 0..3 {
                let mut charge = |units| eval.charge(units);
                let mut f = |t: f64| -> Measure<(f64, f64)> {
                    Ok((
                        coordinate(chart.point(eval, t)?, axis),
                        component(chart.tangent(eval, t)?, axis),
                    ))
                };
                let found = extrema(&mut charge, &mut f, low, high, &breaks)?;
                aabb.min[axis] = aabb.min[axis].min(found.min);
                aabb.max[axis] = aabb.max[axis].max(found.max);
                numeric_error = numeric_error.max(found.error);
            }
            Ok(CurveBox {
                aabb,
                numeric_error,
            })
        }
        CurveShape::Unsupported(reason) => Err(MeasureFailure::unavailable(
            *reason,
            Some(chart.id()),
            "the curve has no parameterization",
        )),
    }
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
