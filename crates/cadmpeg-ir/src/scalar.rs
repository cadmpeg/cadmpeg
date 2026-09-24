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
}

impl PositiveReal {
    /// Unit scalar value.
    pub const ONE: Self = Self(1.0);
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

impl Length {
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

impl NonZeroLength {
    /// The magnitude. The magnitude of a finite nonzero value is finite and
    /// positive.
    #[must_use]
    pub const fn abs(self) -> PositiveLength {
        PositiveLength(self.0.abs())
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

impl crate::geometry::pcurve::PolarPcurveNurbs {
    /// Axial pole values in parameter order. [`Self::new`] admits every pole
    /// finite, so each value is carried without a check. The route lives
    /// beside [`FiniteReal`] because only this module constructs one.
    #[must_use]
    pub fn axial_control_values(&self) -> Vec<FiniteReal> {
        self.poles()
            .into_iter()
            .map(|pole| FiniteReal(pole.axial))
            .collect()
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

impl NonZeroReal {
    /// Unit scalar value.
    pub const ONE: Self = Self(1.0);
    /// One over the square root of two.
    pub const FRAC_1_SQRT_2: Self = Self(std::f64::consts::FRAC_1_SQRT_2);
}
#[cfg(test)]
mod tests;
