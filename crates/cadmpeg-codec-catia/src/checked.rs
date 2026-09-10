// SPDX-License-Identifier: Apache-2.0
//! Checked scalar owners for native record invariants.

use serde::{Deserialize, Serialize};

/// A finite, strictly positive scalar.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "f64", into = "f64")]
pub struct PositiveFinite(f64);

impl PositiveFinite {
    /// Constructs a finite, strictly positive scalar.
    pub fn new(value: f64) -> Option<Self> {
        (value.is_finite() && value > 0.0).then_some(Self(value))
    }

    /// Returns the scalar.
    pub fn get(self) -> f64 {
        self.0
    }
}

impl TryFrom<f64> for PositiveFinite {
    type Error = String;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        Self::new(value).ok_or_else(|| format!("{value} is not finite and positive"))
    }
}

impl From<PositiveFinite> for f64 {
    fn from(value: PositiveFinite) -> Self {
        value.0
    }
}

/// Deviation-from-one tolerance selected by a unit direction's exponent. Each
/// constructor states which measurement of the direction it is applied to.
const fn deviation_tolerance(exponent: u32) -> f64 {
    match exponent {
        9 => 1.0e-9,
        12 => 1.0e-12,
        _ => 0.0,
    }
}

/// A finite direction whose length is one within `10^-TOLERANCE_EXPONENT`, by
/// the measurement its constructor names.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[f64; 3]", into = "[f64; 3]")]
pub struct UnitVector3<const TOLERANCE_EXPONENT: u32>([f64; 3]);

/// A unit direction stored exactly, to a deviation tolerance of `1e-12`.
pub type ExactUnitVector3 = UnitVector3<12>;

/// A unit direction stored loosely, to a deviation tolerance of `1e-9`.
pub type RelaxedUnitVector3 = UnitVector3<9>;

/// A coordinate plane a planar direction is placed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinatePlane {
    /// First component on X, second on Y.
    Xy,
    /// First component on X, second on Z.
    Xz,
}

/// A finite planar direction whose `hypot` length is one within
/// `10^-TOLERANCE_EXPONENT`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[f64; 2]", into = "[f64; 2]")]
pub struct UnitVector2<const TOLERANCE_EXPONENT: u32>([f64; 2]);

/// A planar direction stored loosely, to a deviation tolerance of `1e-9`.
pub type RelaxedUnitVector2 = UnitVector2<9>;

impl<const TOLERANCE_EXPONENT: u32> UnitVector2<TOLERANCE_EXPONENT> {
    /// Tolerance on this direction's `hypot` length deviating from one.
    pub const TOLERANCE: f64 = deviation_tolerance(TOLERANCE_EXPONENT);

    /// Constructs a planar direction whose `hypot` length is one within the
    /// tolerance.
    pub fn from_hypot(value: [f64; 2]) -> Option<Self> {
        (value.iter().all(|component| component.is_finite())
            && (value[0].hypot(value[1]) - 1.0).abs() <= Self::TOLERANCE)
            .then_some(Self(value))
    }

    /// Returns the direction components.
    pub fn get(self) -> [f64; 2] {
        self.0
    }

    /// Turns the direction a quarter turn, to `[-second, first]`.
    pub fn quarter_turn(self) -> Self {
        Self([-self.0[1], self.0[0]])
    }

    /// Turns the direction a quarter turn the other way, to `[second, -first]`.
    pub fn reverse_quarter_turn(self) -> Self {
        Self([self.0[1], -self.0[0]])
    }

    /// Places the components in a coordinate plane, leaving the third axis
    /// zero. Reordering and sign changes leave the length untouched, so the
    /// spatial direction inherits this one's admission with no second test.
    pub fn in_plane(self, plane: CoordinatePlane) -> UnitVector3<TOLERANCE_EXPONENT> {
        let [first, second] = self.0;
        UnitVector3(match plane {
            CoordinatePlane::Xy => [first, second, 0.0],
            CoordinatePlane::Xz => [first, 0.0, second],
        })
    }
}

impl<const TOLERANCE_EXPONENT: u32> TryFrom<[f64; 2]> for UnitVector2<TOLERANCE_EXPONENT> {
    type Error = String;

    fn try_from(value: [f64; 2]) -> Result<Self, Self::Error> {
        Self::from_hypot(value)
            .ok_or_else(|| "planar direction is not a finite unit vector".to_owned())
    }
}

impl<const TOLERANCE_EXPONENT: u32> From<UnitVector2<TOLERANCE_EXPONENT>> for [f64; 2] {
    fn from(value: UnitVector2<TOLERANCE_EXPONENT>) -> Self {
        value.0
    }
}

impl<const TOLERANCE_EXPONENT: u32> UnitVector3<TOLERANCE_EXPONENT> {
    /// Tolerance on this direction's deviation from unit length. [`Self::new`]
    /// applies it to the squared length; the length-measured constructors apply
    /// it to the length itself, each mirroring the record grammar it admits.
    pub const TOLERANCE: f64 = deviation_tolerance(TOLERANCE_EXPONENT);

    /// The +X direction.
    pub const X: Self = Self([1.0, 0.0, 0.0]);

    /// The +Y direction.
    pub const Y: Self = Self([0.0, 1.0, 0.0]);

    /// Constructs a unit direction whose squared length is one within the tolerance.
    pub fn new(value: [f64; 3]) -> Option<Self> {
        let squared_length = value
            .iter()
            .map(|component| component * component)
            .sum::<f64>();
        (value.iter().all(|component| component.is_finite())
            && (squared_length - 1.0).abs() <= Self::TOLERANCE)
            .then_some(Self(value))
    }

    /// Constructs a unit direction whose Euclidean norm, taken as the square
    /// root of the sum of the squared components, is one within the tolerance.
    pub fn from_norm(value: [f64; 3]) -> Option<Self> {
        let norm = (value[0] * value[0] + value[1] * value[1] + value[2] * value[2]).sqrt();
        ((norm - 1.0).abs() <= Self::TOLERANCE).then_some(Self(value))
    }

    /// Normalizes a direction whose norm is above [`f64::EPSILON`], dividing
    /// each component by that norm.
    pub fn normalized(value: [f64; 3]) -> Option<Self> {
        let length = (value[0] * value[0] + value[1] * value[1] + value[2] * value[2]).sqrt();
        (length > f64::EPSILON)
            .then(|| Self([value[0] / length, value[1] / length, value[2] / length]))
    }

    /// Constructs a unit direction from a `scale`-scaled stored vector whose
    /// length is `scale` within the tolerance.
    pub fn from_scaled(stored: [f64; 3], scale: f64) -> Option<Self> {
        let length = stored[0].hypot(stored[1]).hypot(stored[2]);
        (length.is_finite() && ((length / scale) - 1.0).abs() <= Self::TOLERANCE)
            .then(|| Self(stored.map(|component| component / scale)))
    }

    /// Returns the direction components.
    pub fn get(self) -> [f64; 3] {
        self.0
    }
}

impl<const TOLERANCE_EXPONENT: u32> TryFrom<[f64; 3]> for UnitVector3<TOLERANCE_EXPONENT> {
    type Error = String;

    fn try_from(value: [f64; 3]) -> Result<Self, Self::Error> {
        Self::new(value).ok_or_else(|| "direction is not a finite unit vector".to_owned())
    }
}

impl<const TOLERANCE_EXPONENT: u32> From<UnitVector3<TOLERANCE_EXPONENT>> for [f64; 3] {
    fn from(value: UnitVector3<TOLERANCE_EXPONENT>) -> Self {
        value.0
    }
}

/// A finite, strictly increasing scalar interval.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "[f64; 2]", into = "[f64; 2]")]
pub struct OrderedInterval([f64; 2]);

impl OrderedInterval {
    /// Constructs a finite interval whose lower bound is below its upper bound.
    pub fn new(value: [f64; 2]) -> Option<Self> {
        (value.iter().all(|bound| bound.is_finite()) && value[0] < value[1]).then_some(Self(value))
    }

    /// Returns the interval bounds.
    pub fn get(self) -> [f64; 2] {
        self.0
    }

    /// Returns the lower bound.
    pub fn lower(self) -> f64 {
        self.0[0]
    }

    /// Returns the upper bound.
    pub fn upper(self) -> f64 {
        self.0[1]
    }
}

impl TryFrom<[f64; 2]> for OrderedInterval {
    type Error = String;

    fn try_from(value: [f64; 2]) -> Result<Self, Self::Error> {
        Self::new(value).ok_or_else(|| "interval is not finite and increasing".to_owned())
    }
}

impl From<OrderedInterval> for [f64; 2] {
    fn from(value: OrderedInterval) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use super::{ExactUnitVector3, RelaxedUnitVector3};

    #[test]
    fn exact_directions_reject_the_relaxed_tolerance_band() {
        let value = [0.0, 0.0, (1.0_f64 + 5.0e-10).sqrt()];
        assert!(RelaxedUnitVector3::new(value).is_some());
        assert!(ExactUnitVector3::new(value).is_none());
        assert!(serde_json::from_value::<ExactUnitVector3>(serde_json::json!(value)).is_err());
        assert!(serde_json::from_value::<RelaxedUnitVector3>(serde_json::json!(value)).is_ok());
    }

    #[test]
    fn norm_measured_directions_match_the_record_norm_predicate() {
        for component in [
            1.0_f64,
            1.0 + 9.0e-13,
            1.0 - 9.0e-13,
            1.0 + 2.0e-12,
            1.0 - 2.0e-12,
        ] {
            let value = [0.0, component, 0.0];
            let norm = (value[0] * value[0] + value[1] * value[1] + value[2] * value[2]).sqrt();
            let admitted = (norm - 1.0).abs() <= ExactUnitVector3::TOLERANCE;
            assert_eq!(ExactUnitVector3::from_norm(value).is_some(), admitted);
        }
    }

    #[test]
    fn normalized_directions_divide_by_the_norm_and_reject_the_degenerate_one() {
        let value = [0.0, 3.0, 4.0];
        assert_eq!(
            ExactUnitVector3::normalized(value).map(ExactUnitVector3::get),
            Some([0.0, 3.0 / 5.0, 4.0 / 5.0])
        );
        assert!(ExactUnitVector3::normalized([0.0, 0.0, 0.0]).is_none());
    }

    #[test]
    fn scaled_directions_accept_exactly_the_stored_length_ratio() {
        let radius = 5.0_f64;
        for factor in [1.0, 1.0 + 9.0e-13, 1.0 - 9.0e-13, 1.0 + 2.0e-12] {
            let stored = [0.0, 0.0, radius * factor];
            let length = stored[0].hypot(stored[1]).hypot(stored[2]);
            let admitted = ((length / radius) - 1.0).abs() <= ExactUnitVector3::TOLERANCE;
            assert_eq!(
                ExactUnitVector3::from_scaled(stored, radius).is_some(),
                admitted
            );
        }
    }
}
