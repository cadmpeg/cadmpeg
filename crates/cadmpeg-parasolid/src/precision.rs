// SPDX-License-Identifier: Apache-2.0
//! Modeller precision of a Parasolid part.
//!
//! Each BODY stores its linear precision in metres as `res_linear`. Two
//! positions closer than that distance are one position. The modeller's
//! angular precision is fixed and is not stored.

use cadmpeg_ir::scalar::{Magnification, PositiveAngle, PositiveLength};
use cadmpeg_ir::units::Tolerances;

const MM_PER_METRE: f64 = Magnification::MILLIMETERS_PER_METER.get();

/// The modeller's default `res_linear`, in metres.
pub const DEFAULT_LINEAR_RESOLUTION: f64 = 1.0e-8;

/// The modeller's default linear precision in millimetres.
const DEFAULT_LINEAR_PRECISION: PositiveLength =
    match PositiveLength::new(DEFAULT_LINEAR_RESOLUTION * MM_PER_METRE) {
        Some(value) => value,
        None => panic!("the default linear precision must be positive and finite"),
    };

/// The modeller's angular precision in radians.
const ANGULAR_PRECISION: f64 = 1.0e-11;

const ANGULAR_PRECISION_RADIANS: PositiveAngle = match PositiveAngle::new(ANGULAR_PRECISION) {
    Some(value) => value,
    None => panic!("the angular precision must be positive and finite"),
};

/// The linear precision, in millimetres, of a body that stores `res_linear`
/// (metres): `None` where the value in millimetres is not a positive finite
/// length.
#[must_use]
pub fn linear_precision(res_linear: f64) -> Option<PositiveLength> {
    PositiveLength::new(res_linear * MM_PER_METRE)
}

/// The `res_linear` (metres) that states the linear tolerance of a part
/// whose bodies store `stored`: the largest value that states a linear
/// precision, else the modeller default.
#[must_use]
pub fn part_linear_resolution(stored: impl IntoIterator<Item = f64>) -> f64 {
    stored
        .into_iter()
        .filter(|res_linear| linear_precision(*res_linear).is_some())
        .reduce(f64::max)
        .unwrap_or(DEFAULT_LINEAR_RESOLUTION)
}

/// The tolerances of a part whose admitted bodies store `res_linear` values
/// (metres) `stored`.
///
/// The linear tolerance is the precision of [`part_linear_resolution`]. The
/// angular tolerance is the modeller's angular precision.
#[must_use]
pub fn part_tolerances(stored: impl IntoIterator<Item = f64>) -> Tolerances {
    Tolerances {
        linear: linear_precision(part_linear_resolution(stored))
            .unwrap_or(DEFAULT_LINEAR_PRECISION),
        angular: ANGULAR_PRECISION_RADIANS,
    }
}

/// The `res_linear` (metres) a written body stores for a part whose linear
/// tolerance is `linear`, where the bodies of its source store `stored`.
///
/// Where `linear` is the tolerance the source states, the value is the
/// source's [`part_linear_resolution`]. Otherwise it is `linear` in metres.
/// `None` where `linear` in metres states no linear precision.
#[must_use]
pub fn written_linear_resolution(
    linear: PositiveLength,
    stored: impl IntoIterator<Item = f64>,
) -> Option<f64> {
    let source = part_linear_resolution(stored);
    if linear_precision(source) == Some(linear) {
        return Some(source);
    }
    let metres = linear.get() / MM_PER_METRE;
    linear_precision(metres).map(|_| metres)
}

/// The message of the loss a decoder charges for `body`, whose stored
/// `res_linear` states no linear precision.
#[must_use]
pub fn unusable_resolution_message(body: &str, res_linear: Option<f64>) -> String {
    match res_linear {
        Some(value) => format!(
            "body {body} stores res_linear {value:e} m, which states no linear precision; the \
             part's linear tolerance excludes it"
        ),
        None => format!(
            "body {body} stores no res_linear value the decoder reads; the part's linear \
             tolerance excludes it"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        linear_precision, part_linear_resolution, part_tolerances, written_linear_resolution,
        DEFAULT_LINEAR_RESOLUTION,
    };
    use cadmpeg_ir::scalar::PositiveLength;

    #[test]
    fn two_stored_resolutions_state_the_larger_in_millimetres() {
        let tolerances = part_tolerances([1.0e-8, 1.0e-6]);
        assert!((tolerances.linear.get() - 1.0e-3).abs() <= 1.0e-18);
        let reversed = part_tolerances([1.0e-6, 1.0e-8]);
        assert_eq!(reversed, tolerances);
    }

    #[test]
    fn a_part_without_a_stored_resolution_states_the_modeller_default() {
        let tolerances = part_tolerances([]);
        assert!((tolerances.linear.get() - 1.0e-5).abs() <= 1.0e-20);
    }

    #[test]
    fn the_angular_tolerance_is_the_modeller_precision() {
        for stored in [&[][..], &[1.0e-8][..], &[1.0e-6, 1.0e-8][..]] {
            let tolerances = part_tolerances(stored.iter().copied());
            assert!((tolerances.angular.get() - 1.0e-11).abs() <= 1.0e-25);
        }
    }

    #[test]
    fn a_value_that_is_not_a_positive_finite_length_is_not_a_resolution() {
        let tolerances = part_tolerances([-1.0e-3, 0.0, f64::NAN, f64::INFINITY, f64::MAX, 1.0e-7]);
        assert!((tolerances.linear.get() - 1.0e-4).abs() <= 1.0e-19);
        assert_eq!(
            part_tolerances([-1.0e-3, 0.0, f64::NAN]),
            part_tolerances([])
        );
    }

    #[test]
    fn the_default_precision_is_the_default_resolution_in_millimetres() {
        assert_eq!(
            part_tolerances([]).linear,
            linear_precision(DEFAULT_LINEAR_RESOLUTION).unwrap()
        );
        assert_eq!(part_linear_resolution([f64::NAN, -1.0e-6]), 1.0e-8);
    }

    #[test]
    fn a_written_body_stores_the_resolution_the_source_states() {
        let stored = 1.0e-9;
        let linear = part_tolerances([stored]).linear;
        assert_ne!(linear.get() / 1000.0, stored);
        assert_eq!(
            written_linear_resolution(linear, [stored, 1.0e-10]),
            Some(stored)
        );
        let written = written_linear_resolution(linear, []).unwrap();
        assert_eq!(linear_precision(written), Some(linear));
    }

    #[test]
    fn a_written_body_stores_another_tolerance_in_metres() {
        let linear = PositiveLength::new(100.0).unwrap();
        assert_eq!(written_linear_resolution(linear, [1.0e-8]), Some(0.1));
        let tiny = PositiveLength::new(f64::from_bits(1)).unwrap();
        assert_eq!(written_linear_resolution(tiny, []), None);
    }
}
