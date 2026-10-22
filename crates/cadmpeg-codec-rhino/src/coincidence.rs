// SPDX-License-Identifier: Apache-2.0
//! The openNURBS point-coincidence test, `ON_PointsAreCoincident`.

use cadmpeg_ir::math::Point3;

use crate::settings::MillimeterScale;

/// `ON_ZERO_TOLERANCE`: the absolute coordinate difference, in model units,
/// below which openNURBS treats two points as one.
pub(crate) const ON_ZERO_TOLERANCE: f64 = 2.328_306_436_538_696_3e-10;
/// `ON_RELATIVE_TOLERANCE`: the coordinate difference, relative to the
/// coordinate magnitudes, below which openNURBS treats two points as one.
pub(crate) const ON_RELATIVE_TOLERANCE: f64 = 2.273_736_754_432_320_6e-13;

/// `ON_PointsAreCoincident` for two Euclidean points in model units.
pub(crate) fn points_are_coincident(left: Point3, right: Point3) -> bool {
    coincident_with_absolute(left, right, ON_ZERO_TOLERANCE)
}

/// `ON_PointsAreCoincident` evaluated in model units for two points that
/// were scaled by `scale`. The relative test is scale invariant; the absolute
/// tolerance scales with the coordinates.
pub(crate) fn points_are_coincident_at_scale(
    left: Point3,
    right: Point3,
    scale: MillimeterScale,
) -> bool {
    coincident_with_absolute(left, right, ON_ZERO_TOLERANCE * scale.value())
}

fn coincident_with_absolute(left: Point3, right: Point3, absolute: f64) -> bool {
    [(left.x, right.x), (left.y, right.y), (left.z, right.z)]
        .into_iter()
        .all(|(a, b)| {
            let difference = (a - b).abs();
            difference <= absolute || difference <= (a.abs() + b.abs()) * ON_RELATIVE_TOLERANCE
        })
}

#[cfg(test)]
mod tests {
    use super::{points_are_coincident, points_are_coincident_at_scale, ON_ZERO_TOLERANCE};
    use cadmpeg_ir::math::Point3;

    #[test]
    fn absolute_tolerance_is_in_model_units() {
        let origin = Point3::new(0.0, 0.0, 0.0);
        let near = Point3::new(2.0e-10, 0.0, 0.0);
        assert!(points_are_coincident(origin, near));
        let scaled = Point3::new(2.0e-10 * 25.4, 0.0, 0.0);
        assert!(!points_are_coincident(origin, scaled));
        assert!(points_are_coincident_at_scale(
            origin,
            scaled,
            crate::settings::MillimeterScale::from(crate::settings::StandardUnit::Inches)
        ));
        let apart = Point3::new(ON_ZERO_TOLERANCE * 2.0, 0.0, 0.0);
        assert!(!points_are_coincident(origin, apart));
    }

    #[test]
    fn relative_tolerance_scales_with_magnitude() {
        let left = Point3::new(1.0e6, 0.0, 0.0);
        let right = Point3::new(1.0e6 + 1.0e-7, 0.0, 0.0);
        assert!(points_are_coincident(left, right));
    }
}
