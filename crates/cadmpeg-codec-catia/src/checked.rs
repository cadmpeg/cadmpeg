// SPDX-License-Identifier: Apache-2.0
//! Checked direction and interval owners for native record invariants.

use std::marker::PhantomData;

use serde::{Deserialize, Serialize};

/// A tolerance on a direction's measured length deviating from one. Each
/// implementing type names one tolerance, so a direction admits only within a
/// tolerance it names. A tolerance is a copyable marker, so the direction it
/// selects stays `Copy`. Each constructor states which measurement of the
/// direction the tolerance is applied to.
pub(crate) trait DeviationTolerance: Copy {
    /// The largest admitted deviation of the measured length from one.
    const TOLERANCE: f64;
}

/// Admits a measured length within `1e-9` of one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RelaxedDeviation;

impl DeviationTolerance for RelaxedDeviation {
    const TOLERANCE: f64 = 1.0e-9;
}

/// Admits a measured length within `1e-12` of one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ExactDeviation;

impl DeviationTolerance for ExactDeviation {
    const TOLERANCE: f64 = 1.0e-12;
}

/// A measurement of a direction's length. Each implementing type names one
/// measurement, so a direction admits only by a measurement it names. A
/// measurement is a copyable marker, so the direction it selects stays `Copy`.
pub(crate) trait LengthMeasurement: Copy {
    /// The direction's measured length.
    fn length(value: [f64; 3]) -> f64;
}

/// Measures a direction's length as the sum of its squared components.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SquaredLength;

impl LengthMeasurement for SquaredLength {
    fn length(value: [f64; 3]) -> f64 {
        value[0] * value[0] + value[1] * value[1] + value[2] * value[2]
    }
}

/// Measures a direction's length as the square root of the sum of its squared
/// components, the way `Vector3::norm` does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct NormLength;

impl LengthMeasurement for NormLength {
    fn length(value: [f64; 3]) -> f64 {
        SquaredLength::length(value).sqrt()
    }
}

/// Measures a direction's length as a chain of `hypot` calls, the way the
/// records that store a planar pair do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct HypotLength;

impl LengthMeasurement for HypotLength {
    fn length(value: [f64; 3]) -> f64 {
        value[0].hypot(value[1]).hypot(value[2])
    }
}

/// A finite direction admitted by `Measurement` of its length deviating from
/// one by at most `Tolerance`. Both the measurement and the tolerance are the
/// record grammar's, so every route into the type — literal construction,
/// derivation and serde — admits exactly the same directions.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[f64; 3]", into = "[f64; 3]")]
pub(crate) struct UnitVector3<Tolerance: DeviationTolerance, Measurement: LengthMeasurement>(
    [f64; 3],
    PhantomData<(Tolerance, Measurement)>,
);

/// A direction whose squared length is one to `1e-12`.
pub(crate) type ExactUnitVector3 = UnitVector3<ExactDeviation, SquaredLength>;

/// A direction whose norm is one to `1e-12`.
pub(crate) type ExactNormUnitVector3 = UnitVector3<ExactDeviation, NormLength>;

/// A direction whose `hypot` length is one to `1e-12`.
pub(crate) type ExactHypotUnitVector3 = UnitVector3<ExactDeviation, HypotLength>;

/// A direction whose squared length is one to `1e-9`.
pub(crate) type RelaxedUnitVector3 = UnitVector3<RelaxedDeviation, SquaredLength>;

/// A direction whose `hypot` length is one to `1e-9`.
pub(crate) type RelaxedHypotUnitVector3 = UnitVector3<RelaxedDeviation, HypotLength>;

/// A coordinate plane a planar direction is placed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoordinatePlane {
    /// First component on X, second on Y.
    Xy,
    /// First component on X, second on Z.
    Xz,
}

/// A finite planar direction whose `hypot` length is one within `Tolerance`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[f64; 2]", into = "[f64; 2]")]
pub(crate) struct UnitVector2<Tolerance: DeviationTolerance>([f64; 2], PhantomData<Tolerance>);

/// A planar direction stored loosely, to a deviation tolerance of `1e-9`.
pub(crate) type RelaxedUnitVector2 = UnitVector2<RelaxedDeviation>;

impl<Tolerance: DeviationTolerance> UnitVector2<Tolerance> {
    /// Constructs a planar direction whose `hypot` length is one within the
    /// tolerance.
    pub(crate) fn from_hypot(value: [f64; 2]) -> Option<Self> {
        (value.iter().all(|component| component.is_finite())
            && (value[0].hypot(value[1]) - 1.0).abs() <= Tolerance::TOLERANCE)
            .then_some(Self(value, PhantomData))
    }

    /// Returns the direction components.
    #[cfg(test)]
    pub(crate) fn get(self) -> [f64; 2] {
        self.0
    }

    /// Turns the direction a quarter turn, to `[-second, first]`.
    pub(crate) fn quarter_turn(self) -> Self {
        Self([-self.0[1], self.0[0]], PhantomData)
    }

    /// Turns the direction a quarter turn the other way, to `[second, -first]`.
    pub(crate) fn reverse_quarter_turn(self) -> Self {
        Self([self.0[1], -self.0[0]], PhantomData)
    }

    /// Places the components in a coordinate plane, leaving the third axis
    /// zero. `hypot` of a component with zero is that component's magnitude, so
    /// the spatial direction's `hypot` length is this one's, bit for bit: it
    /// inherits this direction's admission and needs no second test.
    pub(crate) fn in_plane(self, plane: CoordinatePlane) -> UnitVector3<Tolerance, HypotLength> {
        let [first, second] = self.0;
        UnitVector3(
            match plane {
                CoordinatePlane::Xy => [first, second, 0.0],
                CoordinatePlane::Xz => [first, 0.0, second],
            },
            PhantomData,
        )
    }
}

impl<Tolerance: DeviationTolerance> TryFrom<[f64; 2]> for UnitVector2<Tolerance> {
    type Error = String;

    fn try_from(value: [f64; 2]) -> Result<Self, Self::Error> {
        Self::from_hypot(value)
            .ok_or_else(|| "planar direction is not a finite unit vector".to_owned())
    }
}

impl<Tolerance: DeviationTolerance> From<UnitVector2<Tolerance>> for [f64; 2] {
    fn from(value: UnitVector2<Tolerance>) -> Self {
        value.0
    }
}

impl<Tolerance: DeviationTolerance, Measurement: LengthMeasurement>
    UnitVector3<Tolerance, Measurement>
{
    /// The +X direction.
    pub(crate) const X: Self = Self([1.0, 0.0, 0.0], PhantomData);

    /// The +Y direction.
    pub(crate) const Y: Self = Self([0.0, 1.0, 0.0], PhantomData);

    /// Constructs a unit direction whose measured length is one within the
    /// tolerance.
    pub(crate) fn new(value: [f64; 3]) -> Option<Self> {
        (value.iter().all(|component| component.is_finite())
            && (Measurement::length(value) - 1.0).abs() <= Tolerance::TOLERANCE)
            .then_some(Self(value, PhantomData))
    }

    /// Normalizes a finite direction whose norm is above [`f64::EPSILON`].
    /// The result must pass this type's unit-length admission.
    pub(crate) fn normalized(value: [f64; 3]) -> Option<Self> {
        let length = value[0].hypot(value[1]).hypot(value[2]);
        if length <= f64::EPSILON {
            return None;
        }
        Self::new(crate::math::unit_vector(value)?)
    }

    /// Constructs a unit direction from a `scale`-scaled stored vector whose
    /// `hypot` length is `scale` within the tolerance. The divided components
    /// must also pass this type's unit-length admission.
    pub(crate) fn from_scaled(stored: [f64; 3], scale: f64) -> Option<Self> {
        let length = stored[0].hypot(stored[1]).hypot(stored[2]);
        if !(length.is_finite() && ((length / scale) - 1.0).abs() <= Tolerance::TOLERANCE) {
            return None;
        }
        Self::new(stored.map(|component| component / scale))
    }

    /// Returns the direction components.
    pub(crate) fn get(self) -> [f64; 3] {
        self.0
    }
}

impl<Tolerance: DeviationTolerance, Measurement: LengthMeasurement> TryFrom<[f64; 3]>
    for UnitVector3<Tolerance, Measurement>
{
    type Error = String;

    fn try_from(value: [f64; 3]) -> Result<Self, Self::Error> {
        Self::new(value).ok_or_else(|| "direction is not a finite unit vector".to_owned())
    }
}

impl<Tolerance: DeviationTolerance, Measurement: LengthMeasurement>
    From<UnitVector3<Tolerance, Measurement>> for [f64; 3]
{
    fn from(value: UnitVector3<Tolerance, Measurement>) -> Self {
        value.0
    }
}

/// A finite, strictly increasing scalar interval.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "[f64; 2]", into = "[f64; 2]")]
pub(crate) struct OrderedInterval([f64; 2]);

impl OrderedInterval {
    /// Constructs a finite interval whose lower bound is below its upper bound.
    pub(crate) fn new(value: [f64; 2]) -> Option<Self> {
        (value.iter().all(|bound| bound.is_finite()) && value[0] < value[1]).then_some(Self(value))
    }

    /// Returns the interval bounds.
    pub(crate) fn get(self) -> [f64; 2] {
        self.0
    }

    /// Returns the lower bound.
    pub(crate) fn lower(self) -> f64 {
        self.0[0]
    }

    /// Returns the upper bound.
    pub(crate) fn upper(self) -> f64 {
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

/// A byte-extent width that reports its own overflow on addition.
pub(crate) trait ByteExtent: Copy + Ord {
    /// The sum, or `None` when the sum leaves the width.
    fn checked_sum(self, other: Self) -> Option<Self>;
}

impl ByteExtent for u64 {
    fn checked_sum(self, other: Self) -> Option<Self> {
        self.checked_add(other)
    }
}

impl ByteExtent for usize {
    fn checked_sum(self, other: Self) -> Option<Self> {
        self.checked_add(other)
    }
}

/// Whether two byte extents share at least one byte. An extent whose end
/// leaves the width overlaps nothing.
pub(crate) fn extents_overlap<Extent: ByteExtent>(
    first_start: Extent,
    first_len: Extent,
    second_start: Extent,
    second_len: Extent,
) -> bool {
    first_start
        .checked_sum(first_len)
        .zip(second_start.checked_sum(second_len))
        .is_some_and(|(first_end, second_end)| first_start < second_end && second_start < first_end)
}

#[cfg(test)]
mod tests {
    use super::{
        CoordinatePlane, DeviationTolerance, ExactDeviation, ExactHypotUnitVector3,
        ExactNormUnitVector3, ExactUnitVector3, RelaxedHypotUnitVector3, RelaxedUnitVector2,
        RelaxedUnitVector3,
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
            let admitted = (norm - 1.0).abs() <= ExactDeviation::TOLERANCE;
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
    fn normalized_directions_enforce_the_unit_invariant_and_length_gate() {
        for value in [[1e200, 0.0, 0.0], [f64::MAX, f64::MAX, 0.0]] {
            let direction = ExactNormUnitVector3::normalized(value)
                .expect("large finite direction can be normalized");
            assert!(ExactNormUnitVector3::new(direction.get()).is_some());
        }
        assert_eq!(
            ExactNormUnitVector3::normalized([1e200, 0.0, 0.0]).map(ExactNormUnitVector3::get),
            Some([1.0, 0.0, 0.0])
        );
        for magnitude in [f64::from_bits(1), f64::EPSILON] {
            assert!(ExactNormUnitVector3::normalized([magnitude, 0.0, 0.0]).is_none());
        }
        assert_eq!(
            ExactNormUnitVector3::normalized([
                f64::from_bits(f64::EPSILON.to_bits() + 1),
                0.0,
                0.0,
            ])
            .map(ExactNormUnitVector3::get),
            Some([1.0, 0.0, 0.0])
        );
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            for axis in 0..3 {
                let mut value = [1.0; 3];
                value[axis] = invalid;
                assert!(ExactNormUnitVector3::normalized(value).is_none());
            }
        }
    }

    #[test]
    fn scaled_directions_reject_a_rounded_length_that_hides_a_nonunit_result() {
        let smallest = f64::from_bits(1);
        assert!(ExactHypotUnitVector3::from_scaled([smallest, smallest, 0.0], smallest).is_none());
        assert_eq!(
            ExactHypotUnitVector3::from_scaled([smallest, 0.0, 0.0], smallest)
                .map(ExactHypotUnitVector3::get),
            Some([1.0, 0.0, 0.0])
        );
        for scale in [0.0, f64::NAN, f64::INFINITY, -1.0] {
            assert!(ExactHypotUnitVector3::from_scaled([1.0, 0.0, 0.0], scale).is_none());
        }
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
            let admitted = ((length / radius) - 1.0).abs() <= ExactDeviation::TOLERANCE;
            assert_eq!(
                ExactHypotUnitVector3::from_scaled(stored, radius).is_some(),
                admitted
            );
        }
    }
}
