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

/// Squared-length tolerance selected by a unit direction's exponent.
const fn squared_tolerance(exponent: u32) -> f64 {
    match exponent {
        9 => 1.0e-9,
        12 => 1.0e-12,
        _ => 0.0,
    }
}

/// A finite direction whose squared length is one within `10^-TOLERANCE_EXPONENT`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[f64; 3]", into = "[f64; 3]")]
pub struct UnitVector3<const TOLERANCE_EXPONENT: u32>([f64; 3]);

/// A unit direction stored exactly, to a squared-length tolerance of `1e-12`.
pub type ExactUnitVector3 = UnitVector3<12>;

/// A unit direction stored loosely, to a squared-length tolerance of `1e-9`.
pub type RelaxedUnitVector3 = UnitVector3<9>;

impl<const TOLERANCE_EXPONENT: u32> UnitVector3<TOLERANCE_EXPONENT> {
    /// Squared-length tolerance this direction is held to.
    pub const TOLERANCE: f64 = squared_tolerance(TOLERANCE_EXPONENT);

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

    /// Constructs a unit direction whose length is one within the tolerance.
    pub fn from_length(value: [f64; 3]) -> Option<Self> {
        let length = value[0].hypot(value[1]).hypot(value[2]);
        (value.iter().all(|component| component.is_finite())
            && (length - 1.0).abs() <= Self::TOLERANCE)
            .then_some(Self(value))
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
