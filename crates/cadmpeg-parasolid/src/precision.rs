// SPDX-License-Identifier: Apache-2.0
//! Modeller precision of a Parasolid part.
//!
//! Each BODY stores its linear precision in metres as `res_linear`. Two
//! positions closer than that distance are one position. The modeller's
//! angular precision is fixed and is not stored.

use cadmpeg_ir::scalar::{Magnification, PositiveAngle, PositiveLength};
use cadmpeg_ir::units::Tolerances;

/// The modeller's default linear precision, `1.0e-8` metres, in millimetres.
const DEFAULT_LINEAR_PRECISION_MM: PositiveLength = match PositiveLength::new(1.0e-5) {
    Some(value) => value,
    None => panic!("the default linear precision must be positive and finite"),
};

/// The modeller's angular precision in radians.
const ANGULAR_PRECISION: f64 = 1.0e-11;

const ANGULAR_PRECISION_RADIANS: PositiveAngle = match PositiveAngle::new(ANGULAR_PRECISION) {
    Some(value) => value,
    None => panic!("the angular precision must be positive and finite"),
};

const MM_PER_METRE: f64 = Magnification::MILLIMETERS_PER_METER.get();

/// The tolerances of a part whose admitted bodies store `res_linear` values
/// (metres) `stored`.
///
/// The linear tolerance is the largest positive finite stored value, in
/// millimetres, else the modeller default. The angular tolerance is the
/// modeller's angular precision.
#[must_use]
pub fn part_tolerances(stored: impl IntoIterator<Item = f64>) -> Tolerances {
    let linear = stored
        .into_iter()
        .filter_map(|metres| PositiveLength::new(metres * MM_PER_METRE))
        .reduce(|largest, value| if largest >= value { largest } else { value })
        .unwrap_or(DEFAULT_LINEAR_PRECISION_MM);
    Tolerances {
        linear,
        angular: ANGULAR_PRECISION_RADIANS,
    }
}

#[cfg(test)]
mod tests {
    use super::part_tolerances;

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
}
