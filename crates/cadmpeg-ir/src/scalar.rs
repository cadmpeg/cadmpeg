// SPDX-License-Identifier: Apache-2.0
//! Checked finite scalar domains.
//!
//! Subset conversions preserve values. Restricting a domain requires `TryFrom`:
//!
//! ```
//! use cadmpeg_ir::scalar::{Length, NonNegativeLength, NonZeroLength, PositiveLength};
//! let positive = PositiveLength::new(2.0).unwrap();
//! let nonzero = NonZeroLength::from(positive);
//! let length = Length::from(nonzero);
//! assert_eq!(PositiveLength::try_from(length).unwrap(), positive);
//! assert_eq!(Length::from(length), length);
//! let identity: Result<Length, std::convert::Infallible> = Length::try_from(length);
//! let widening: Result<Length, std::convert::Infallible> = Length::try_from(positive);
//! assert_eq!(identity.unwrap(), widening.unwrap());
//! assert_eq!(NonNegativeLength::from(positive).get(), 2.0);
//! ```
//!
//! A signed nonzero length cannot become positive without a check:
//!
//! ```compile_fail
//! use cadmpeg_ir::scalar::{NonZeroLength, PositiveLength};
//! let _: PositiveLength = NonZeroLength::new(-2.0).unwrap().into();
//! ```
//!
//! A finite angle or real cannot become positive without a check either:
//!
//! ```compile_fail
//! use cadmpeg_ir::scalar::{Angle, PositiveAngle};
//! let _: PositiveAngle = Angle::ZERO.into();
//! ```
//!
//! ```compile_fail
//! use cadmpeg_ir::scalar::{FiniteReal, PositiveReal};
//! let _: PositiveReal = FiniteReal::new(-1.0).unwrap().into();
//! ```
//!
//! Overlapping domains have no infallible conversion in either direction:
//!
//! ```compile_fail
//! use cadmpeg_ir::scalar::{NonNegativeLength, NonZeroLength};
//! let _: NonNegativeLength = NonZeroLength::new(-2.0).unwrap().into();
//! ```
//!
//! ```compile_fail
//! use cadmpeg_ir::scalar::{NonNegativeLength, NonZeroLength};
//! let _: NonZeroLength = NonNegativeLength::new(0.0).unwrap().into();
//! ```
//!
//! Equal numerical restrictions do not permit conversions between quantity families:
//!
//! ```compile_fail
//! use cadmpeg_ir::scalar::{PositiveAngle, PositiveLength};
//! let _: PositiveAngle = PositiveLength::new(2.0).unwrap().into();
//! ```
//!
//! ```compile_fail
//! use cadmpeg_ir::scalar::{PositiveLength, PositiveReal};
//! let _: PositiveReal = PositiveLength::new(2.0).unwrap().into();
//! ```
//!
//! A dimensionless value takes a quantity family only through a named assignment
//! that its caller's context justifies:
//!
//! ```compile_fail
//! use cadmpeg_ir::scalar::{FiniteReal, Length};
//! let _: Length = FiniteReal::ZERO.into();
//! ```
//!
//! ```
//! use cadmpeg_ir::scalar::{Angle, FiniteReal, Length};
//! let value = FiniteReal::new(-2.5).unwrap();
//! assert_eq!(Length::from_assigned_real(value).get(), -2.5);
//! assert_eq!(Angle::from_assigned_real(value).get(), -2.5);
//! ```

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A positive value in a native signed 64-bit integer lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "i64", into = "i64")]
pub struct PositiveI64(i64);

impl PositiveI64 {
    /// Admit a positive signed 64-bit integer.
    pub const fn new(value: i64) -> Option<Self> {
        if value > 0 {
            Some(Self(value))
        } else {
            None
        }
    }

    /// Return the signed 64-bit integer.
    pub const fn get(&self) -> i64 {
        self.0
    }
}

impl TryFrom<i64> for PositiveI64 {
    type Error = String;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        Self::new(value).ok_or_else(|| format!("PositiveI64 must be positive, got {value}"))
    }
}

impl From<PositiveI64> for i64 {
    fn from(value: PositiveI64) -> Self {
        value.get()
    }
}

macro_rules! checked_scalar {
    ($(#[$attribute:meta])* $name:ident, $value:ident, $condition:expr, $error:literal) => {
        $(#[$attribute])*
        #[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize)]
        #[cfg_attr(feature = "schema", derive(JsonSchema))]
        #[serde(transparent)]
        pub struct $name(f64);

        impl $name {
            /// Admits a value within the scalar's domain.
            pub const fn new($value: f64) -> Option<Self> {
                if $value.is_finite() && $condition { Some(Self($value)) } else { None }
            }

            /// Returns the scalar value.
            pub const fn get(self) -> f64 {
                self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where D: serde::Deserializer<'de>,
            {
                Self::new(f64::deserialize(deserializer)?)
                    .ok_or_else(|| serde::de::Error::custom($error))
            }
        }

        impl TryFrom<f64> for $name {
            type Error = &'static str;

            fn try_from(value: f64) -> Result<Self, Self::Error> {
                Self::new(value).ok_or($error)
            }
        }

        impl From<$name> for f64 {
            fn from(value: $name) -> Self { value.get() }
        }
    };
}

checked_scalar!(
    /// A finite length in canonical millimeters.
    Length, value, true, "Length must be finite"
);
checked_scalar!(
    /// A finite signed angle in canonical radians.
    #[derive(Default)]
    Angle, value, true, "Angle must be finite"
);
checked_scalar!(
    /// A positive finite length in canonical millimeters.
    PositiveLength, value, value > 0.0, "PositiveLength must be positive and finite"
);
checked_scalar!(
    /// A finite nonzero signed length in canonical millimeters.
    NonZeroLength, value, value != 0.0, "NonZeroLength must be finite and nonzero"
);
checked_scalar!(
    /// A nonnegative finite length in canonical millimeters.
    NonNegativeLength, value, value >= 0.0, "NonNegativeLength must be nonnegative and finite"
);
checked_scalar!(
    /// A finite angle strictly between negative and positive half-pi radians.
    SlopeAngle, value, value.abs() < std::f64::consts::FRAC_PI_2,
    "SlopeAngle must be finite and strictly between -pi/2 and pi/2"
);
checked_scalar!(
    /// A finite angle strictly between zero and pi radians.
    InteriorAngle, value, value > 0.0 && value < std::f64::consts::PI,
    "InteriorAngle must be finite and strictly between zero and pi"
);
checked_scalar!(
    /// A positive finite angle in canonical radians.
    PositiveAngle, value, value > 0.0, "PositiveAngle must be positive and finite"
);
checked_scalar!(
    /// A finite nonzero signed angle in canonical radians.
    NonZeroAngle, value, value != 0.0, "NonZeroAngle must be finite and nonzero"
);
checked_scalar!(
    /// A finite dimensionless scalar.
    FiniteReal, value, true, "FiniteReal must be finite"
);
checked_scalar!(
    /// A positive finite dimensionless scalar.
    PositiveReal, value, value > 0.0, "PositiveReal must be positive and finite"
);
checked_scalar!(
    /// A nonnegative finite dimensionless scalar.
    NonNegativeReal, value, value >= 0.0, "NonNegativeReal must be nonnegative and finite"
);

checked_scalar!(
    /// A finite nonzero signed dimensionless scalar.
    NonZeroReal, value, value != 0.0, "NonZeroReal must be finite and nonzero"
);
checked_scalar!(
    /// A finite fraction in the closed interval from zero to one.
    Fraction, value, value >= 0.0 && value <= 1.0, "Fraction must be between zero and one"
);
checked_scalar!(
    /// A finite dimensionless scale of at least one. The product of a nonzero
    /// value and this scale has a magnitude not below the value's, so it is
    /// not zero.
    Magnification, value, value >= 1.0, "Magnification must be finite and at least one"
);

impl Length {
    /// Zero in canonical units.
    pub const ZERO: Self = Self(0.0);
}

impl Angle {
    /// One full turn in radians.
    pub const FULL_TURN: Self = Self(std::f64::consts::TAU);
    /// One quarter turn in radians.
    pub const QUARTER_TURN: Self = Self(std::f64::consts::FRAC_PI_2);
    /// One half turn in radians.
    pub const HALF_TURN: Self = Self(std::f64::consts::PI);
    /// Three quarter turns in radians.
    pub const THREE_QUARTER_TURN: Self = Self(3.0 * std::f64::consts::FRAC_PI_2);
    /// Zero in canonical units.
    pub const ZERO: Self = Self(0.0);
}

impl NonNegativeLength {
    /// Zero in canonical units.
    pub const ZERO: Self = Self(0.0);

    /// The length times the magnitude of the sine of `angle`.
    ///
    /// The sine of a finite angle is finite, and a sine computed within one
    /// unit in the last place has magnitude at most one. The product is
    /// therefore finite, nonnegative and not above the length, so it is
    /// admitted without a check.
    #[must_use]
    pub fn scaled_by_sine(self, angle: Angle) -> Self {
        Self(self.0 * angle.0.sin().abs())
    }

    /// Assign the length family to a dimensionless nonnegative value in
    /// canonical millimeters.
    ///
    /// The caller calls this route only where its own context states that
    /// the value is a length, for example a tolerance that bounds model-space
    /// distances. Both domains admit every finite nonnegative value, so the
    /// assignment keeps the value and cannot refuse.
    #[must_use]
    pub const fn from_assigned_real(value: NonNegativeReal) -> Self {
        Self(value.0)
    }

    /// The length times `scale`.
    ///
    /// A positive scale keeps the sign of a nonnegative value: zero stays
    /// zero, and rounding does not change a sign. The product is refused only
    /// when it overflows.
    #[must_use]
    pub fn scaled(self, scale: PositiveReal) -> Option<Self> {
        let value = self.0 * scale.0;
        value.is_finite().then_some(Self(value))
    }
}

impl PositiveReal {
    /// Unit scalar value.
    pub const ONE: Self = Self(1.0);
}

impl NonNegativeReal {
    /// The value times `scale`.
    ///
    /// A positive scale keeps the sign of a nonnegative value: zero stays
    /// zero, and rounding does not change a sign. The product is refused only
    /// when it overflows.
    #[must_use]
    pub fn scaled(self, scale: PositiveReal) -> Option<Self> {
        let value = self.0 * scale.0;
        value.is_finite().then_some(Self(value))
    }
}

impl Magnification {
    /// Millimeters per meter.
    pub const MILLIMETERS_PER_METER: Self = Self(1000.0);
}

impl SlopeAngle {
    /// Zero in canonical radians.
    pub const ZERO: Self = Self(0.0);
}

impl PositiveAngle {
    /// One full turn in radians.
    pub const FULL_TURN: Self = Self(std::f64::consts::TAU);
    /// One quarter turn in radians.
    pub const QUARTER_TURN: Self = Self(std::f64::consts::FRAC_PI_2);
    /// One half turn in radians.
    pub const HALF_TURN: Self = Self(std::f64::consts::PI);
    /// Three quarter turns in radians.
    pub const THREE_QUARTER_TURN: Self = Self(3.0 * std::f64::consts::FRAC_PI_2);
}

/// State one subset edge inside a quantity family.
///
/// `$from` accepts a subset of what `$to` accepts, so the widening carries the
/// stored value without checking it again. The restriction back to `$from`
/// runs `$from`'s own admission, so it is `TryFrom` and `$error` states the
/// condition a refused value failed.
///
/// A pair whose domains only overlap has no subset direction and therefore no
/// declaration here; such a conversion is a check its caller spells out.
macro_rules! scalar_subset {
    ($from:ident => $to:ident, $error:literal) => {
        impl From<$from> for $to {
            fn from(value: $from) -> Self {
                Self(value.get())
            }
        }

        impl TryFrom<$to> for $from {
            type Error = &'static str;

            fn try_from(value: $to) -> Result<Self, Self::Error> {
                Self::new(value.get()).ok_or($error)
            }
        }
    };
}

scalar_subset!(PositiveLength => NonZeroLength, "length must be positive");
scalar_subset!(PositiveLength => NonNegativeLength, "length must be positive");
scalar_subset!(PositiveLength => Length, "length must be positive");
scalar_subset!(NonZeroLength => Length, "length must be nonzero");
scalar_subset!(NonNegativeLength => Length, "length must be nonnegative");

scalar_subset!(InteriorAngle => PositiveAngle, "angle must be strictly less than pi");
scalar_subset!(InteriorAngle => NonZeroAngle, "angle must be strictly between zero and pi");
scalar_subset!(InteriorAngle => Angle, "angle must be strictly between zero and pi");
scalar_subset!(PositiveAngle => NonZeroAngle, "angle must be positive");
scalar_subset!(PositiveAngle => Angle, "angle must be positive");
scalar_subset!(NonZeroAngle => Angle, "angle must be nonzero");
scalar_subset!(SlopeAngle => Angle, "angle must be strictly between -pi/2 and pi/2");

scalar_subset!(PositiveReal => NonZeroReal, "value must be positive");
scalar_subset!(PositiveReal => NonNegativeReal, "value must be positive");
scalar_subset!(PositiveReal => FiniteReal, "value must be positive");
scalar_subset!(NonZeroReal => FiniteReal, "value must be nonzero");
scalar_subset!(NonNegativeReal => FiniteReal, "value must be nonnegative");
scalar_subset!(Fraction => NonNegativeReal, "value must be between zero and one");
scalar_subset!(Fraction => FiniteReal, "value must be between zero and one");
scalar_subset!(Magnification => PositiveReal, "value must be at least one");

impl Length {
    /// Reverse the sign.
    #[must_use]
    pub const fn negated(self) -> Self {
        Self(-self.0)
    }

    /// The length in canonical millimeters as a dimensionless real, for a
    /// product with a dimensionless factor. A length is finite, so nothing is
    /// checked.
    #[must_use]
    pub const fn magnitude(self) -> FiniteReal {
        FiniteReal(self.0)
    }

    /// The magnitude. The magnitude of a finite value is finite.
    #[must_use]
    pub const fn abs(self) -> Self {
        Self(self.0.abs())
    }

    /// Assign the length family to a dimensionless value in canonical
    /// millimeters.
    ///
    /// A `FiniteReal` carries no quantity family, so no conversion gives it
    /// one. The caller calls this route only where its own context states that
    /// the value is a length, for example a parameter definition that declares
    /// a length kind. Both domains admit every finite value, so the assignment
    /// keeps the value and cannot refuse.
    #[must_use]
    pub const fn from_assigned_real(value: FiniteReal) -> Self {
        Self(value.0)
    }
}

impl PositiveLength {
    /// The length in canonical millimeters as a dimensionless real, for a
    /// quotient of lengths. A positive length is finite, so nothing is
    /// checked.
    #[must_use]
    pub const fn magnitude(self) -> FiniteReal {
        FiniteReal(self.0)
    }

    /// Two values, the first not below the second, each times `scale`, as
    /// positive lengths. The caller establishes that order.
    ///
    /// Rounding is monotone, so the products keep the order; they can round
    /// to one value, which the order admits. The first product is admitted
    /// positive and finite. The second is not above the first, so it is
    /// finite when the first is, and only its sign is tested. A refusal gives
    /// the index of the refused value.
    pub(crate) fn scale_ordered_pair(
        values: [f64; 2],
        scale: PositiveReal,
    ) -> Result<[Self; 2], usize> {
        let [first, second] = values.map(|value| value * scale.0);
        if !first.is_finite() || first <= 0.0 {
            return Err(0);
        }
        if second <= 0.0 {
            return Err(1);
        }
        Ok([Self(first), Self(second)])
    }
}

impl NonZeroLength {
    /// The magnitude. The magnitude of a finite nonzero value is finite and
    /// positive.
    #[must_use]
    pub const fn abs(self) -> PositiveLength {
        PositiveLength(self.0.abs())
    }

    /// The length times `factor`.
    ///
    /// The exact product has a magnitude not below the length's, and rounding
    /// is monotone, so the rounded product is not zero. The product is refused
    /// only when it overflows.
    #[must_use]
    pub fn magnified(self, factor: Magnification) -> Option<Self> {
        let value = self.0 * factor.0;
        value.is_finite().then_some(Self(value))
    }
}

impl Angle {
    /// Reverse the sign.
    #[must_use]
    pub const fn negated(self) -> Self {
        Self(-self.0)
    }

    /// The magnitude. The magnitude of a finite value is finite.
    #[must_use]
    pub const fn abs(self) -> Self {
        Self(self.0.abs())
    }

    /// Assign the angle family to a dimensionless value in canonical radians.
    ///
    /// A `FiniteReal` carries no quantity family, so no conversion gives it
    /// one. The caller calls this route only where its own context states that
    /// the value is an angle, for example a parameter definition or a relation
    /// that declares an angle kind. Both domains admit every finite value, so
    /// the assignment keeps the value and cannot refuse.
    #[must_use]
    pub const fn from_assigned_real(value: FiniteReal) -> Self {
        Self(value.0)
    }
}

impl FiniteReal {
    /// Unit scalar value.
    pub const ONE: Self = Self(1.0);
    /// Zero.
    pub const ZERO: Self = Self(0.0);
    /// One half.
    pub const HALF: Self = Self(0.5);
    /// Two.
    pub(crate) const TWO: Self = Self(2.0);

    /// Reverse the sign.
    #[must_use]
    pub const fn negated(self) -> Self {
        Self(-self.0)
    }

    /// The magnitude. The magnitude of a finite value is finite.
    #[must_use]
    pub const fn abs(self) -> Self {
        Self(self.0.abs())
    }

    /// The value `units` times the least positive subnormal magnitude,
    /// negated when `negative` is set.
    ///
    /// A `u64` converts to at most `2^64`, and `2^64` times `2^-1074` is
    /// `2^-1010`, so the product is finite for every argument and nothing is
    /// checked. Up to `2^53` units the conversion and the product are exact,
    /// so the value is the one whose bit pattern is the sign bit and `units`.
    #[must_use]
    pub(crate) fn subnormal_units(negative: bool, units: u64) -> Self {
        let magnitude = units as f64 * f64::from_bits(1);
        Self(if negative { -magnitude } else { magnitude })
    }

    /// The four-quadrant arctangent of `self / x`, an angle in `[-π, π]`.
    /// The arctangent of two finite values is finite, so nothing is checked.
    #[must_use]
    pub fn atan2(self, x: Self) -> Self {
        Self(self.0.atan2(x.0))
    }

    /// An index as a real. Every `usize` converts to a finite `f64`.
    #[must_use]
    pub(crate) fn from_index(index: usize) -> Self {
        Self(index as f64)
    }

    /// An integer as a real. Every `i64` converts to a finite `f64`.
    #[must_use]
    pub(crate) fn from_integer(value: i64) -> Self {
        Self(value as f64)
    }

    /// The turns from `start` to `self`: `(self - start) / τ`, rounded once
    /// from the exact difference.
    ///
    /// The exact difference of two finite values is at most `2 * f64::MAX`
    /// in magnitude and `τ` exceeds two, so the quotient is finite and nothing
    /// is checked. The route lives beside [`FiniteReal`] because only this
    /// module constructs one.
    #[must_use]
    pub(crate) fn turns_from(self, start: Self) -> Self {
        use crate::math::sum::{ExactSignedSum, ScaledValue};
        let mut difference = ExactSignedSum::default();
        difference.add_product(self.0, 1.0);
        difference.add_product(start.0, -1.0);
        let turn = ScaledValue::of_nonzero(NonZeroReal(std::f64::consts::TAU));
        Self(difference.finish().map_or(0.0, |difference| {
            difference.quotient_shifted_product(turn, 0)
        }))
    }

    /// Where `self` lies against the segment from `start` to `end`: outside
    /// it, on a segment of zero width, or at the fraction `(self - start) /
    /// (end - start)`, rounded once from the exact differences.
    ///
    /// A value within the segment lies no farther from `start` than `end`
    /// does, and on the same side, so the fraction lies in `[0, 1]` and
    /// nothing is checked. The route lives beside [`FiniteReal`] because only
    /// this module constructs one.
    pub(crate) fn segment_position(self, start: Self, end: Self) -> SegmentPosition {
        use crate::math::sum::ExactSignedSum;
        if !((self >= start && self <= end) || (self <= start && self >= end)) {
            return SegmentPosition::Outside;
        }
        let mut width = ExactSignedSum::default();
        width.add_product(end.0, 1.0);
        width.add_product(start.0, -1.0);
        let Some(width) = width.finish() else {
            return SegmentPosition::Degenerate;
        };
        let mut distance = ExactSignedSum::default();
        distance.add_product(self.0, 1.0);
        distance.add_product(start.0, -1.0);
        SegmentPosition::Within(Self(
            distance
                .finish()
                .map_or(0.0, |distance| distance.quotient_shifted_product(width, 0)),
        ))
    }

    /// `exp(-|self|)` and `(1 + exp(-|self|)²)²`, the tail and the squared
    /// denominator of the hyperbolic tangent's derivative. The tail lies in
    /// `[0, 1]`, so the denominator lies in `[1, 4]` and nothing is checked.
    /// The route lives beside [`FiniteReal`] because only this module
    /// constructs a nonzero value.
    pub(crate) fn hyperbolic_tail(self) -> (f64, NonZeroReal) {
        let tail = (-self.0.abs()).exp();
        (tail, NonZeroReal((1.0 + tail * tail).powi(2)))
    }

    /// The value as a magnitude above one, or none where its magnitude is at
    /// most one.
    pub(crate) fn beyond_unit(self) -> Option<BeyondUnit> {
        (self.0.abs() > 1.0).then_some(BeyondUnit(self.0))
    }

    /// Half the value. Half a finite value is finite.
    #[must_use]
    pub(crate) fn halved(self) -> Self {
        Self(self.0 * 0.5)
    }

    /// The midpoint of `self` and `other`. The midpoint of two finite values
    /// is finite.
    #[must_use]
    pub(crate) fn midpoint(self, other: Self) -> Self {
        Self(self.0.midpoint(other.0))
    }

    /// Admit a normal value or a zero. Every normal value and every zero is
    /// finite, so the one predicate states the whole admission.
    pub(crate) fn normal_or_zero(value: f64) -> Option<Self> {
        (value.is_normal() || value == 0.0).then_some(Self(value))
    }

    /// Admit every value of each lane, or none of them.
    pub(crate) fn lanes<const N: usize>(lanes: [Vec<f64>; N]) -> Option<[Vec<Self>; N]> {
        lanes
            .iter()
            .flatten()
            .all(|value| value.is_finite())
            .then(|| lanes.map(|lane| lane.into_iter().map(Self).collect()))
    }

    /// Admit every present value, or none of them.
    pub(crate) fn optional<const N: usize>(values: [Option<f64>; N]) -> Option<[Option<Self>; N]> {
        values
            .iter()
            .flatten()
            .all(|value| value.is_finite())
            .then(|| values.map(|value| value.map(Self)))
    }

    /// Admit every value, or none of them.
    pub(crate) fn array<const N: usize>(values: [f64; N]) -> Option<[Self; N]> {
        values
            .iter()
            .all(|value| value.is_finite())
            .then(|| values.map(Self))
    }

    /// Admit every value of the lane, or none of them.
    pub(crate) fn lane(values: Vec<f64>) -> Option<Vec<Self>> {
        values
            .iter()
            .all(|value| value.is_finite())
            .then(|| values.into_iter().map(Self).collect())
    }

    /// Admit every value of each row, or none of them.
    pub(crate) fn rows<const N: usize>(rows: Vec<[f64; N]>) -> Option<Vec<[Self; N]>> {
        rows.iter()
            .flatten()
            .all(|value| value.is_finite())
            .then(|| rows.into_iter().map(|row| row.map(Self)).collect())
    }

    /// Admit every value of the grid, or none of them.
    pub(crate) fn grid<const N: usize, const M: usize>(
        grid: [[f64; M]; N],
    ) -> Option<[[Self; M]; N]> {
        grid.iter()
            .flatten()
            .all(|value| value.is_finite())
            .then(|| grid.map(|row| row.map(Self)))
    }

    /// Admit every present value of the grid, or none of them.
    pub(crate) fn optional_grid<const N: usize, const M: usize>(
        grid: [[Option<f64>; M]; N],
    ) -> Option<[[Option<Self>; M]; N]> {
        grid.iter()
            .flatten()
            .flatten()
            .all(|value| value.is_finite())
            .then(|| grid.map(|row| row.map(|value| value.map(Self))))
    }

    /// The raw values of the array, for a reader that writes or edits them.
    #[must_use]
    pub fn raw_array<const N: usize>(values: [Self; N]) -> [f64; N] {
        values.map(Self::get)
    }

    /// The raw present values, for a reader that writes or edits them.
    #[must_use]
    pub fn raw_optional<const N: usize>(values: [Option<Self>; N]) -> [Option<f64>; N] {
        values.map(|value| value.map(Self::get))
    }

    /// The raw values of the lane, for a reader that writes or edits them.
    #[must_use]
    pub fn raw_lane(values: &[Self]) -> Vec<f64> {
        values.iter().map(|value| value.0).collect()
    }

    /// The raw values of each row, for a reader that writes or edits them.
    #[must_use]
    pub fn raw_rows<const N: usize>(rows: &[[Self; N]]) -> Vec<[f64; N]> {
        rows.iter().map(|row| row.map(Self::get)).collect()
    }

    /// The raw values of the grid, for a reader that writes or edits them.
    #[must_use]
    pub fn raw_grid<const N: usize, const M: usize>(grid: [[Self; M]; N]) -> [[f64; M]; N] {
        grid.map(|row| row.map(Self::get))
    }

    /// The raw present values of the grid, for a reader that writes or edits
    /// them.
    #[must_use]
    pub fn raw_optional_grid<const N: usize, const M: usize>(
        grid: [[Option<Self>; M]; N],
    ) -> [[Option<f64>; M]; N] {
        grid.map(Self::raw_optional)
    }

    /// The raw values of each lane, for a reader that writes or edits them.
    #[must_use]
    pub fn raw_lanes<const N: usize>(lanes: &[Vec<Self>; N]) -> [Vec<f64>; N] {
        std::array::from_fn(|index| lanes[index].iter().map(|value| value.0).collect())
    }
}

impl crate::topology::IncreasingParameterInterval {
    /// Return the endpoints as finite reals. The interval admits only finite
    /// endpoints, so nothing is checked. The route lives beside
    /// [`FiniteReal`] because only this module constructs one.
    #[must_use]
    pub const fn finite_endpoints(self) -> [FiniteReal; 2] {
        let [lower, upper] = self.endpoints();
        [FiniteReal(lower), FiniteReal(upper)]
    }
}

impl crate::topology::ParameterInterval {
    /// Return the endpoints as finite reals. The interval admits only finite
    /// endpoints, so nothing is checked. The route lives beside
    /// [`FiniteReal`] because only this module constructs one.
    #[must_use]
    pub const fn finite_endpoints(self) -> [FiniteReal; 2] {
        let [start, end] = self.endpoints();
        [FiniteReal(start), FiniteReal(end)]
    }

    /// The point of the interval nearest `value`: `value` itself inside the
    /// interval, its nearer endpoint outside. The endpoints are finite and
    /// ordered, so the nearest point is finite and nothing is checked. The
    /// route lives beside [`FiniteReal`] because only this module constructs
    /// one.
    #[must_use]
    pub(crate) fn project(self, value: ExtendedReal) -> FiniteReal {
        let [start, end] = self.endpoints();
        FiniteReal(value.0.clamp(start, end))
    }
}

impl<const N: usize> crate::units::FiniteVector<N> {
    /// The components as finite reals. The vector admits only finite
    /// components, so nothing is checked. The route lives beside
    /// [`FiniteReal`] because only this module constructs one.
    #[must_use]
    pub fn finite_components(self) -> [FiniteReal; N] {
        self.get().map(FiniteReal)
    }
}

impl crate::geometry::nurbs::KnotVector {
    /// The knot at `index` as a finite real, absent past the last knot. The
    /// vector admits only finite knots, so nothing is checked. The route
    /// lives beside [`FiniteReal`] because only this module constructs one.
    #[must_use]
    pub fn finite_knot(&self, index: usize) -> Option<FiniteReal> {
        self.as_slice().get(index).map(|knot| FiniteReal(*knot))
    }

    /// The knots as finite reals, in order. The vector admits only finite
    /// knots, so nothing is checked. The route lives beside [`FiniteReal`]
    /// because only this module constructs one.
    pub fn finite_knots(
        &self,
    ) -> impl DoubleEndedIterator<Item = FiniteReal> + ExactSizeIterator + '_ {
        self.as_slice().iter().map(|knot| FiniteReal(*knot))
    }
}

impl crate::geometry::DirectedParameterRange {
    /// Return the directed endpoints as finite reals. The range admits only
    /// finite endpoints, so nothing is checked. The route lives beside
    /// [`FiniteReal`] because only this module constructs one.
    #[must_use]
    pub const fn finite_endpoints(self) -> [FiniteReal; 2] {
        let [start, end] = self.endpoints();
        [FiniteReal(start), FiniteReal(end)]
    }
}

impl crate::geometry::RecordBounds {
    /// The present values as finite reals. The quartet admits only finite
    /// values, so nothing is checked. The route lives beside [`FiniteReal`]
    /// because only this module constructs one.
    #[must_use]
    pub(crate) fn finite_values(self) -> [Option<FiniteReal>; 4] {
        self.get().map(|value| value.map(FiniteReal))
    }
}

impl crate::features::FinitePoint3 {
    /// Return the coordinates as finite reals. The point admits only finite
    /// coordinates, so nothing is checked. The route lives beside
    /// [`FiniteReal`] because only this module constructs one.
    #[must_use]
    pub const fn coordinates(self) -> [FiniteReal; 3] {
        let point = self.get();
        [
            FiniteReal(point.x),
            FiniteReal(point.y),
            FiniteReal(point.z),
        ]
    }
}

impl crate::features::FiniteVector3 {
    /// Return the components as finite reals. The vector admits only finite
    /// components, so nothing is checked. The route lives beside
    /// [`FiniteReal`] because only this module constructs one.
    #[must_use]
    pub const fn components(self) -> [FiniteReal; 3] {
        let vector = self.get();
        [
            FiniteReal(vector.x),
            FiniteReal(vector.y),
            FiniteReal(vector.z),
        ]
    }

    /// The components charted by the binade of the largest magnitude, and the
    /// chart's exponent: each component times `2^-exponent`, where
    /// `2^exponent` is the least power of two above the largest magnitude. A
    /// zero vector has no chart.
    ///
    /// Every magnitude is at most the largest, which lies below `2^exponent`,
    /// and a power-of-two scaling does not raise a magnitude past the
    /// product's own bound, so each charted component lies in `(-1, 1)` and
    /// nothing is checked. The route lives beside [`FiniteReal`] because only
    /// this module constructs one.
    #[must_use]
    pub(crate) fn binade_chart(self) -> Option<([FiniteReal; 3], i32)> {
        let vector = self.get();
        let largest = vector.x.abs().max(vector.y.abs()).max(vector.z.abs());
        let exponent = crate::math::power_of_two_bound(largest)?;
        let chart = |value: f64| FiniteReal(crate::math::power_of_two_product(value, -exponent));
        Some((
            [chart(vector.x), chart(vector.y), chart(vector.z)],
            exponent,
        ))
    }

    /// Unit direction for every nonzero vector, including subnormals, and
    /// none for the zero vector. Callers that impose a geometric length
    /// threshold must check it separately.
    ///
    /// The vector is charted by the binade of its largest component
    /// ([`Self::binade_chart`]), so the norm stays in range: the largest
    /// charted magnitude is at least one half and none reaches one, so the
    /// chart's norm lies in `[1/2, √3)`. Each unit component is a charted
    /// component over that norm, at most one in magnitude. A charted component
    /// below the normal range may already be rounded or zero; its quotient is
    /// formed from the original component instead, rounded once. Nothing is
    /// checked. The route lives beside [`FiniteReal`] because only this module
    /// constructs the nonzero norm.
    #[must_use]
    pub fn unit_nonzero(self) -> Option<crate::math::Vector3> {
        use crate::math::sum::ScaledValue;
        let ([x, y, z], exponent) = self.binade_chart()?;
        let length = crate::math::Vector3::new(x.0, y.0, z.0).norm();
        let scaled_length = ScaledValue::of_nonzero(NonZeroReal(length));
        let component = |value: f64, chart: FiniteReal| match NonZeroReal::new(value) {
            Some(value) if chart.0.abs() < f64::MIN_POSITIVE => {
                ScaledValue::of_nonzero(value).quotient_shifted_product(scaled_length, -exponent)
            }
            _ => chart.0 / length,
        };
        let vector = self.get();
        Some(crate::math::Vector3::new(
            component(vector.x, x),
            component(vector.y, y),
            component(vector.z, z),
        ))
    }

    /// The unit direction, and the factors `[scale, scale, scale, length³]`
    /// of the cubed length, where `scale` is the largest component magnitude
    /// and `length` the norm of the components over it; none for the zero
    /// vector.
    ///
    /// The scale of a nonzero vector is nonzero and finite. Each component
    /// over it lies in `[-1, 1]` and the largest is `±1` exactly, so `length`
    /// lies in `[1, √3]` and its cube in `[1, 3√3]`: nothing is checked.
    #[must_use]
    pub(crate) fn unit_with_cubed_length(self) -> Option<(crate::math::Vector3, [NonZeroReal; 4])> {
        let unit = self.unit_nonzero()?;
        let vector = self.get();
        let scale = vector.x.abs().max(vector.y.abs()).max(vector.z.abs());
        let length = (vector.x / scale)
            .hypot(vector.y / scale)
            .hypot(vector.z / scale);
        let scale = NonZeroReal(scale);
        Some((
            unit,
            [scale, scale, scale, NonZeroReal(length * length * length)],
        ))
    }
}

impl crate::units::FinitePoint2 {
    /// Return the coordinates as finite reals. The point admits only finite
    /// coordinates, so nothing is checked. The route lives beside
    /// [`FiniteReal`] because only this module constructs one.
    #[must_use]
    pub const fn coordinates(self) -> [FiniteReal; 2] {
        let point = self.get();
        [FiniteReal(point.u), FiniteReal(point.v)]
    }
}

/// A real value or an infinity, never NaN.
///
/// The hypotenuse of two finite values, a finite value or an infinity over a
/// nonzero finite length, and a finite length subtracted from one of these
/// are all finite or infinite. The four-quadrant arctangent of two such values is
/// finite, so it needs no check.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ExtendedReal(f64);

impl ExtendedReal {
    /// A finite value.
    pub(crate) const fn from_finite(value: FiniteReal) -> Self {
        Self(value.0)
    }

    /// Admit a value that is not NaN.
    pub(crate) fn new(value: f64) -> Option<Self> {
        (!value.is_nan()).then_some(Self(value))
    }

    /// `origin - scale * step`. The product of two finite values is finite
    /// or infinite, and a finite value less it is finite or infinite.
    pub(crate) fn stepped(origin: FiniteReal, scale: FiniteReal, step: FiniteReal) -> Self {
        Self(origin.0 - scale.0 * step.0)
    }

    /// The hypotenuse `hypot(x, y)` of two finite values: finite or `+inf`.
    pub(crate) fn hypot(x: FiniteReal, y: FiniteReal) -> Self {
        Self(x.0.hypot(y.0))
    }

    /// `self - value`: an infinity less a finite length keeps its infinity.
    #[must_use]
    pub(crate) fn minus(self, value: Length) -> Self {
        Self(self.0 - value.0)
    }

    /// `self / divisor`: over a nonzero finite divisor, a finite value is
    /// finite or infinite and an infinity keeps its infinity.
    #[must_use]
    pub(crate) fn over(self, divisor: NonZeroLength) -> Self {
        Self(self.0 / divisor.0)
    }

    /// The four-quadrant arctangent of `self / x`, an angle in `[-π, π]`.
    pub(crate) fn atan2(self, x: Self) -> FiniteReal {
        FiniteReal(self.0.atan2(x.0))
    }
}

/// Where a value lies against a segment of two finite bounds.
pub(crate) enum SegmentPosition {
    /// The value lies outside the segment.
    Outside,
    /// The bounds are equal: the segment has zero width.
    Degenerate,
    /// The value lies at this fraction of the segment, in `[0, 1]`.
    Within(FiniteReal),
}

/// A finite value whose magnitude exceeds one.
#[derive(Clone, Copy)]
pub(crate) struct BeyondUnit(f64);

impl BeyondUnit {
    /// `dividend / self`. The quotient's magnitude is below the dividend's,
    /// so it is finite and nothing is checked.
    pub(crate) fn quotient(self, dividend: FiniteReal) -> FiniteReal {
        FiniteReal(dividend.0 / self.0)
    }

    /// `((self - 1) / self) * ((self + 1) / self)`, which is `1 - 1/self²`.
    ///
    /// For `self` in `(1, 2]` the difference `self - 1` is exact and at least
    /// `2^-52`, and beyond two it is at least one; the sum `self + 1` is at
    /// least two. Each quotient lies in `(0, 2)`, and their product is
    /// positive, finite and at least `2^-53`; the negative side is the mirror
    /// image. Nothing is checked.
    pub(crate) fn square_complement(self) -> NonZeroReal {
        let x = self.0;
        NonZeroReal(((x - 1.0) / x) * ((x + 1.0) / x))
    }

    /// The inverse hyperbolic cotangent, `atanh(1 / self)`. The reciprocal's
    /// magnitude is below one, so the value is finite and nothing is checked.
    pub(crate) fn arcoth(self) -> FiniteReal {
        FiniteReal((1.0 / self.0).atanh())
    }
}

impl PositiveReal {
    /// `sqrt((1 - self) * (1 + self))` for a value below one, and none
    /// otherwise.
    ///
    /// Below one each factor is at least `2^-53`: `1 - self` is exact from
    /// one half on and at least one half before it. The product is normal and
    /// at most one, so the root is positive and finite and nothing is
    /// checked. The route lives beside [`FiniteReal`] because only this
    /// module constructs one.
    pub(crate) fn unit_complement_root(self) -> Option<Self> {
        (self.0 < 1.0).then(|| Self(((1.0 - self.0) * (1.0 + self.0)).sqrt()))
    }
}

impl NonZeroReal {
    /// Unit scalar value.
    pub const ONE: Self = Self(1.0);
    /// One over the square root of two.
    pub const FRAC_1_SQRT_2: Self = Self(std::f64::consts::FRAC_1_SQRT_2);
}
#[cfg(test)]
mod tests;
