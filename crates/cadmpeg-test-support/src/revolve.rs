// SPDX-License-Identifier: Apache-2.0
//! The region a CADIR revolution sweeps, computed from the turn sense that
//! `docs/cad-ir.md` states, so a decoder test can predict where a revolve lands.

use cadmpeg_ir::features::{AngularTermination, RevolutionAxis, RevolveExtent};
use cadmpeg_ir::math::Point3;

/// Half turns from `atan2`'s range to the far end of a travel of at most one
/// turn on either side of zero.
const HALF_TURNS_REACHED: i32 = 3;

/// `point` turned right-handed about `axis` by `radians`.
///
/// # Panics
///
/// Panics when the axis direction has no unit form.
#[must_use]
pub fn turned(point: Point3, axis: &RevolutionAxis, radians: f64) -> Point3 {
    let direction = axis
        .direction
        .get()
        .unit()
        .expect("a nonzero axis direction");
    let origin = axis.origin.get();
    let offset = point.vector_from(origin);
    let along = direction.dot(offset);
    let across = direction.cross(offset);
    let (sine, cosine) = radians.sin_cos();
    Point3::new(
        origin.x + offset.x * cosine + across.x * sine + direction.x * along * (1.0 - cosine),
        origin.y + offset.y * cosine + across.y * sine + direction.y * along * (1.0 - cosine),
        origin.z + offset.z * cosine + across.z * sine + direction.z * along * (1.0 - cosine),
    )
}

/// The lowest and highest coordinates of `points` over the angular travel of
/// `extent`.
///
/// # Panics
///
/// Panics when a termination is not a fixed angle.
#[must_use]
pub fn swept_box(
    points: &[Point3],
    axis: &RevolutionAxis,
    extent: &RevolveExtent,
) -> ([f64; 3], [f64; 3]) {
    let angle = |termination: &AngularTermination| match termination {
        AngularTermination::Angle { angle } => angle
            .literal()
            .map(|angle| angle.get())
            .expect("a swept box needs a fixed angle"),
        other => panic!("a swept box needs a fixed angle, not {other:?}"),
    };
    let (start, end) = match extent {
        RevolveExtent::OneSided { termination } => (0.0, angle(termination)),
        RevolveExtent::TwoSided { first, second } => (-angle(second), angle(first)),
        RevolveExtent::Symmetric { termination } => {
            (-angle(termination) / 2.0, angle(termination) / 2.0)
        }
    };
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for point in points {
        for radians in extreme_angles(*point, axis, start, end) {
            let at = turned(*point, axis, radians);
            for (index, value) in [at.x, at.y, at.z].into_iter().enumerate() {
                low[index] = low[index].min(value);
                high[index] = high[index].max(value);
            }
        }
    }
    (low, high)
}

/// The ends of the travel and every angle inside it where a coordinate of
/// `point` is stationary. Each coordinate of a turned point is
/// `c + a cos θ + b sin θ`, which is stationary at `atan2(b, a)` plus whole half
/// turns.
fn extreme_angles(point: Point3, axis: &RevolutionAxis, start: f64, end: f64) -> Vec<f64> {
    let direction = axis
        .direction
        .get()
        .unit()
        .expect("a nonzero axis direction");
    let offset = point.vector_from(axis.origin.get());
    let along = direction.dot(offset);
    let across = direction.cross(offset);
    let mut angles = vec![start, end];
    for (cosine, sine) in [
        (offset.x - direction.x * along, across.x),
        (offset.y - direction.y * along, across.y),
        (offset.z - direction.z * along, across.z),
    ] {
        let stationary = sine.atan2(cosine);
        for half_turns in -HALF_TURNS_REACHED..=HALF_TURNS_REACHED {
            let angle = stationary + f64::from(half_turns) * std::f64::consts::PI;
            if (start..=end).contains(&angle) {
                angles.push(angle);
            }
        }
    }
    angles
}

#[cfg(test)]
mod tests {
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};

    use cadmpeg_ir::features::{
        AngularTermination, FeatureDirection3, FinitePoint3, RevolutionAxis, RevolveExtent,
    };
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::scalar::PositiveAngle;
    use cadmpeg_ir::units::UnitVector3;

    use super::{swept_box, turned};

    const EPS_SWEEP: f64 = 1.0e-9;

    fn about_z(direction: UnitVector3) -> RevolutionAxis {
        RevolutionAxis {
            origin: FinitePoint3::ZERO,
            direction: FeatureDirection3::from(direction),
            reference: None,
        }
    }

    fn fixed(radians: f64) -> AngularTermination {
        AngularTermination::Angle {
            angle: cadmpeg_ir::Dimension::Literal(
                PositiveAngle::new(radians).expect("a positive angle"),
            ),
        }
    }

    fn assert_box(actual: ([f64; 3], [f64; 3]), low: [f64; 3], high: [f64; 3]) {
        for index in 0..3 {
            assert!(
                (actual.0[index] - low[index]).abs() <= EPS_SWEEP,
                "{actual:?}"
            );
            assert!(
                (actual.1[index] - high[index]).abs() <= EPS_SWEEP,
                "{actual:?}"
            );
        }
    }

    #[test]
    fn a_positive_turn_about_z_carries_x_to_y() {
        let at = turned(
            Point3::new(1.0, 0.0, 2.0),
            &about_z(UnitVector3::Z_AXIS),
            FRAC_PI_2,
        );
        assert!(
            (at.x).abs() <= EPS_SWEEP && (at.y - 1.0).abs() <= EPS_SWEEP,
            "{at:?}"
        );
        assert!((at.z - 2.0).abs() <= EPS_SWEEP, "{at:?}");
        let at = turned(
            Point3::new(1.0, 0.0, 2.0),
            &about_z(UnitVector3::Z_AXIS.reversed()),
            FRAC_PI_2,
        );
        assert!(
            (at.x).abs() <= EPS_SWEEP && (at.y + 1.0).abs() <= EPS_SWEEP,
            "{at:?}"
        );
    }

    #[test]
    fn each_extent_sweeps_the_stated_interval() {
        let axis = about_z(UnitVector3::Z_AXIS);
        let point = [Point3::new(1.0, 0.0, 0.0)];
        assert_box(
            swept_box(
                &point,
                &axis,
                &RevolveExtent::OneSided {
                    termination: fixed(FRAC_PI_2),
                },
            ),
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
        );
        assert_box(
            swept_box(
                &point,
                &axis,
                &RevolveExtent::TwoSided {
                    first: fixed(FRAC_PI_2),
                    second: fixed(FRAC_PI_4),
                },
            ),
            [0.0, -FRAC_PI_4.sin(), 0.0],
            [1.0, 1.0, 0.0],
        );
        assert_box(
            swept_box(
                &point,
                &axis,
                &RevolveExtent::OneSided {
                    termination: fixed(3.0 * FRAC_PI_2),
                },
            ),
            [-1.0, -1.0, 0.0],
            [1.0, 1.0, 0.0],
        );
        assert_box(
            swept_box(
                &point,
                &axis,
                &RevolveExtent::Symmetric {
                    termination: fixed(PI),
                },
            ),
            [0.0, -1.0, 0.0],
            [1.0, 1.0, 0.0],
        );
    }
}
