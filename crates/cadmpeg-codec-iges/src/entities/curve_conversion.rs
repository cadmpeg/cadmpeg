// SPDX-License-Identifier: Apache-2.0
//! Exact conversions from bounded analytic curves to NURBS carriers.

use cadmpeg_ir::geometry::NurbsCurve;
use cadmpeg_ir::math::{Point3, Vector3};

fn add_scaled(center: Point3, x: Vector3, x_scale: f64, y: Vector3, y_scale: f64) -> Point3 {
    Point3::new(
        center.x + x.x * x_scale + y.x * y_scale,
        center.y + x.y * x_scale + y.y * y_scale,
        center.z + x.z * x_scale + y.z * y_scale,
    )
}

fn cross(left: Vector3, right: Vector3) -> Vector3 {
    Vector3::new(
        left.y * right.z - left.z * right.y,
        left.z * right.x - left.x * right.z,
        left.x * right.y - left.y * right.x,
    )
}

/// Angular slack this module allows when bounding or dividing a sweep.
pub(super) const ANGULAR_TOLERANCE: f64 = std::f64::consts::TAU * 1.0e-12;

/// Number of quarter-turn rational spans a positive `sweep` divides into.
///
/// `ceil` is discontinuous exactly at a whole number of quarter turns, which is
/// where CAD sweeps overwhelmingly land: a semicircle divides to exactly `2.0`,
/// and one unit in the last place above that yields three spans and a
/// structurally different curve. A sweep is a difference of decoded angles, so
/// it carries last-place noise and the platform's libm decides which side of the
/// boundary it falls on. Backing the sweep off by [`ANGULAR_TOLERANCE`] first
/// keeps an exact multiple of a quarter turn on the lower side.
pub(super) fn quarter_turn_spans(sweep: f64) -> usize {
    let quarters = (sweep - ANGULAR_TOLERANCE) / std::f64::consts::FRAC_PI_2;
    quarters.ceil().max(1.0) as usize
}

pub(super) fn circular_arc_nurbs(
    center: Point3,
    axis: Vector3,
    reference: Vector3,
    radius: f64,
    interval: [f64; 2],
) -> Option<NurbsCurve> {
    let delta = interval[1] - interval[0];
    if !delta.is_finite() || delta <= 0.0 || delta > std::f64::consts::TAU + ANGULAR_TOLERANCE {
        return None;
    }
    let delta = delta.min(std::f64::consts::TAU);
    let transverse = cross(axis, reference);
    let spans = quarter_turn_spans(delta);
    let step = delta / spans as f64;
    let mut knots = Vec::with_capacity(spans * 2 + 4);
    let mut control_points = Vec::with_capacity(spans * 2 + 1);
    let mut weights = Vec::with_capacity(spans * 2 + 1);
    for span in 0..spans {
        let start = interval[0] + step * span as f64;
        let end = interval[0] + step * (span + 1) as f64;
        let middle = (start + end) * 0.5;
        let middle_weight = ((end - start) * 0.5).cos();
        if !middle_weight.is_finite() || middle_weight <= 0.0 {
            return None;
        }
        if span == 0 {
            control_points.push(add_scaled(
                center,
                reference,
                radius * start.cos(),
                transverse,
                radius * start.sin(),
            ));
            weights.push(1.0);
            knots.extend([start, start, start]);
        } else {
            knots.extend([start, start]);
        }
        control_points.push(add_scaled(
            center,
            reference,
            radius * middle.cos() / middle_weight,
            transverse,
            radius * middle.sin() / middle_weight,
        ));
        weights.push(middle_weight);
        control_points.push(add_scaled(
            center,
            reference,
            radius * end.cos(),
            transverse,
            radius * end.sin(),
        ));
        weights.push(1.0);
        if span + 1 == spans {
            knots.extend([end, end, end]);
        }
    }
    Some(NurbsCurve {
        degree: 2,
        knots,
        control_points,
        weights: Some(weights),
        periodic: false,
    })
}

#[cfg(test)]
mod span_tests {
    use super::quarter_turn_spans;

    /// A sweep that is a whole number of quarter turns must not gain a span from
    /// last-place noise. Expectations come from the geometry: a quarter turn is
    /// one rational span, a semicircle two, a full turn four.
    #[test]
    fn an_exact_quarter_turn_multiple_is_stable_under_last_place_noise() {
        for (quarters, expected) in [(1.0, 1), (2.0, 2), (3.0, 3), (4.0, 4)] {
            let sweep = std::f64::consts::FRAC_PI_2 * quarters;
            for bits in [sweep.to_bits() - 1, sweep.to_bits(), sweep.to_bits() + 1] {
                assert_eq!(
                    quarter_turn_spans(f64::from_bits(bits)),
                    expected,
                    "{quarters} quarter turn(s) at bits {bits:#x}"
                );
            }
        }
    }

    /// A sweep between two boundaries still rounds up, so a span never spans
    /// more than a quarter turn.
    #[test]
    fn a_partial_quarter_turn_rounds_up() {
        for (quarters, expected) in [(0.5, 1), (1.5, 2), (2.5, 3), (3.5, 4)] {
            let sweep = std::f64::consts::FRAC_PI_2 * quarters;
            assert_eq!(
                quarter_turn_spans(sweep),
                expected,
                "{quarters} quarter turns"
            );
        }
    }

    /// A vanishing sweep still yields one span rather than none, which would
    /// produce a curve with no control points.
    #[test]
    fn a_vanishing_sweep_still_yields_one_span() {
        assert_eq!(quarter_turn_spans(f64::MIN_POSITIVE), 1);
    }
}
