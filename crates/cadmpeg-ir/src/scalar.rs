// SPDX-License-Identifier: Apache-2.0
//! Checked finite scalar domains.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
    /// Zero in canonical units.
    pub const ZERO: Self = Self(0.0);
}

impl SlopeAngle {
    /// Zero in canonical radians.
    pub const ZERO: Self = Self(0.0);
}

impl PositiveAngle {
    /// One full turn in radians.
    pub const FULL_TURN: Self = Self(std::f64::consts::TAU);
}

macro_rules! scalar_conversion {
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

scalar_conversion!(PositiveLength => Length, "length must be positive");
scalar_conversion!(NonZeroLength => Length, "length must be nonzero");
scalar_conversion!(NonNegativeLength => Length, "length must be nonnegative");
scalar_conversion!(SlopeAngle => Angle, "angle must be strictly between -pi/2 and pi/2");
scalar_conversion!(InteriorAngle => Angle, "angle must be strictly between zero and pi");
scalar_conversion!(PositiveAngle => Angle, "angle must be positive");

impl FiniteReal {
    /// Unit scalar value.
    pub const ONE: Self = Self(1.0);

    pub(crate) const fn as_raw(&self) -> &f64 {
        &self.0
    }
    /// Reverse the sign.
    #[must_use]
    pub const fn negated(self) -> Self {
        Self(-self.0)
    }
}
impl PositiveReal {
    pub(crate) const fn as_raw(&self) -> &f64 {
        &self.0
    }
}
impl NonNegativeReal {
    pub(crate) const fn as_raw(&self) -> &f64 {
        &self.0
    }
}

#[cfg(test)]
mod tests;
