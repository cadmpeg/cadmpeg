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

/// Measures a direction's length as the sum of its squared components.
pub const MEASURE_SQUARED: u8 = 0;

/// Measures a direction's length as the square root of that sum, the way
/// `Vector3::norm` does.
pub const MEASURE_NORM: u8 = 1;

/// Measures a direction's length as a chain of `hypot` calls, the way the
/// records that store a planar pair do.
pub const MEASURE_HYPOT: u8 = 2;

/// A finite direction admitted by `MEASUREMENT` of its length deviating from
/// one by at most `10^-TOLERANCE_EXPONENT`. Both the measurement and the
/// tolerance are the record grammar's, so every route into the type — literal
/// construction, derivation and serde — admits exactly the same directions.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[f64; 3]", into = "[f64; 3]")]
pub struct UnitVector3<const TOLERANCE_EXPONENT: u32, const MEASUREMENT: u8>([f64; 3]);

/// A direction whose squared length is one to `1e-12`.
pub type ExactUnitVector3 = UnitVector3<12, MEASURE_SQUARED>;

/// A direction whose norm is one to `1e-12`.
pub type ExactNormUnitVector3 = UnitVector3<12, MEASURE_NORM>;

/// A direction whose `hypot` length is one to `1e-12`.
pub type ExactHypotUnitVector3 = UnitVector3<12, MEASURE_HYPOT>;

/// A direction whose squared length is one to `1e-9`.
pub type RelaxedUnitVector3 = UnitVector3<9, MEASURE_SQUARED>;

/// A direction whose `hypot` length is one to `1e-9`.
pub type RelaxedHypotUnitVector3 = UnitVector3<9, MEASURE_HYPOT>;

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
    /// zero. `hypot` of a component with zero is that component's magnitude, so
    /// the spatial direction's `hypot` length is this one's, bit for bit: it
    /// inherits this direction's admission and needs no second test.
    pub fn in_plane(
        self,
        plane: CoordinatePlane,
    ) -> UnitVector3<TOLERANCE_EXPONENT, MEASURE_HYPOT> {
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

impl<const TOLERANCE_EXPONENT: u32, const MEASUREMENT: u8>
    UnitVector3<TOLERANCE_EXPONENT, MEASUREMENT>
{
    /// Tolerance on this direction's measured length deviating from one.
    pub const TOLERANCE: f64 = deviation_tolerance(TOLERANCE_EXPONENT);

    /// The +X direction.
    pub const X: Self = Self([1.0, 0.0, 0.0]);

    /// The +Y direction.
    pub const Y: Self = Self([0.0, 1.0, 0.0]);

    /// This direction's measured length, by the measurement the type names.
    fn measured_length(value: [f64; 3]) -> f64 {
        let squared_length = value[0] * value[0] + value[1] * value[1] + value[2] * value[2];
        match MEASUREMENT {
            MEASURE_NORM => squared_length.sqrt(),
            MEASURE_HYPOT => value[0].hypot(value[1]).hypot(value[2]),
            _ => squared_length,
        }
    }

    /// Constructs a unit direction whose measured length is one within the
    /// tolerance.
    pub fn new(value: [f64; 3]) -> Option<Self> {
        (value.iter().all(|component| component.is_finite())
            && (Self::measured_length(value) - 1.0).abs() <= Self::TOLERANCE)
            .then_some(Self(value))
    }

    /// Normalizes a direction whose norm is above [`f64::EPSILON`], dividing
    /// each component by that norm.
    pub fn normalized(value: [f64; 3]) -> Option<Self> {
        let length = (value[0] * value[0] + value[1] * value[1] + value[2] * value[2]).sqrt();
        (length > f64::EPSILON)
            .then(|| Self([value[0] / length, value[1] / length, value[2] / length]))
    }

    /// Constructs a unit direction from a `scale`-scaled stored vector whose
    /// `hypot` length is `scale` within the tolerance.
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

impl<const TOLERANCE_EXPONENT: u32, const MEASUREMENT: u8> TryFrom<[f64; 3]>
    for UnitVector3<TOLERANCE_EXPONENT, MEASUREMENT>
{
    type Error = String;

    fn try_from(value: [f64; 3]) -> Result<Self, Self::Error> {
        Self::new(value).ok_or_else(|| "direction is not a finite unit vector".to_owned())
    }
}

impl<const TOLERANCE_EXPONENT: u32, const MEASUREMENT: u8>
    From<UnitVector3<TOLERANCE_EXPONENT, MEASUREMENT>> for [f64; 3]
{
    fn from(value: UnitVector3<TOLERANCE_EXPONENT, MEASUREMENT>) -> Self {
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
    use super::{
        CoordinatePlane, ExactHypotUnitVector3, ExactNormUnitVector3, ExactUnitVector3,
        RelaxedHypotUnitVector3, RelaxedUnitVector2, RelaxedUnitVector3,
    };

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
            let admitted = (norm - 1.0).abs() <= ExactNormUnitVector3::TOLERANCE;
            assert_eq!(ExactNormUnitVector3::new(value).is_some(), admitted);
        }
    }

    #[test]
    fn normalized_directions_divide_by_the_norm_and_reject_the_degenerate_one() {
        let value = [0.0, 3.0, 4.0];
        assert_eq!(
            ExactNormUnitVector3::normalized(value).map(ExactNormUnitVector3::get),
            Some([0.0, 3.0 / 5.0, 4.0 / 5.0])
        );
        assert!(ExactNormUnitVector3::normalized([0.0, 0.0, 0.0]).is_none());
    }

    #[test]
    fn planar_placements_deserialize_wherever_the_pair_was_admitted() {
        let component = 1.0 + 6.0e-10;
        let pair = RelaxedUnitVector2::from_hypot([component, 0.0]).expect("pair inside the band");
        for plane in [CoordinatePlane::Xy, CoordinatePlane::Xz] {
            let placed = pair.in_plane(plane);
            let wire = serde_json::to_value(placed).expect("serialize placed direction");
            assert_eq!(
                serde_json::from_value::<RelaxedHypotUnitVector3>(wire)
                    .expect("a placed direction deserializes wherever its pair was admitted"),
                placed
            );
        }
        assert!(RelaxedUnitVector3::new([component, 0.0, 0.0]).is_none());
    }

    #[test]
    fn scaled_directions_accept_exactly_the_stored_length_ratio() {
        let radius = 5.0_f64;
        for factor in [1.0, 1.0 + 9.0e-13, 1.0 - 9.0e-13, 1.0 + 2.0e-12] {
            let stored = [0.0, 0.0, radius * factor];
            let length = stored[0].hypot(stored[1]).hypot(stored[2]);
            let admitted = ((length / radius) - 1.0).abs() <= ExactHypotUnitVector3::TOLERANCE;
            assert_eq!(
                ExactHypotUnitVector3::from_scaled(stored, radius).is_some(),
                admitted
            );
        }
    }
}
