// SPDX-License-Identifier: Apache-2.0
//! Units, unit systems, quantities, and the quantity types that v11 adds.
//!
//! Every stored value and every cache is in canonical units: millimetres,
//! radians, seconds, and kilograms. Expressions keep authored units in their
//! literals.

use std::num::NonZeroI8;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A unit: a product of symbols with powers. An empty unit is dimensionless.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct Unit(pub Vec<UnitFactor>);

/// One factor of a unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct UnitFactor {
    /// The unit symbol.
    pub symbol: UnitSymbol,
    /// The power of the symbol.
    pub power: NonZeroI8,
}

/// A unit symbol. Its wire name is the symbol that canonical expression
/// text writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum UnitSymbol {
    /// Nanometre.
    Nm,
    /// Micrometre.
    Um,
    /// Millimetre.
    Mm,
    /// Centimetre.
    Cm,
    /// Metre.
    M,
    /// Kilometre.
    Km,
    /// Inch.
    In,
    /// Foot.
    Ft,
    /// Yard.
    Yd,
    /// Mile.
    Mi,
    /// Radian.
    Rad,
    /// Degree.
    Deg,
    /// Gradian.
    Grad,
    /// Minute of arc.
    Arcmin,
    /// Second of arc.
    Arcsec,
    /// Revolution: an angle of 2π radians.
    Rev,
    /// Second.
    S,
    /// Minute.
    Min,
    /// Hour.
    H,
    /// Gram.
    G,
    /// Kilogram.
    Kg,
    /// Pound.
    Lb,
}

impl UnitSymbol {
    /// Every unit symbol, in declared order.
    pub const ALL: [Self; 22] = [
        Self::Nm,
        Self::Um,
        Self::Mm,
        Self::Cm,
        Self::M,
        Self::Km,
        Self::In,
        Self::Ft,
        Self::Yd,
        Self::Mi,
        Self::Rad,
        Self::Deg,
        Self::Grad,
        Self::Arcmin,
        Self::Arcsec,
        Self::Rev,
        Self::S,
        Self::Min,
        Self::H,
        Self::G,
        Self::Kg,
        Self::Lb,
    ];

    /// The factor from the symbol to the canonical unit of its quantity, and
    /// that quantity.
    #[must_use]
    pub fn canonical(self) -> (f64, Quantity) {
        use std::f64::consts::PI;
        const MASS: Quantity = Quantity::of(0, 1, 0, 0);
        const TIME: Quantity = Quantity::of(0, 0, 0, 1);
        match self {
            Self::Nm => (1.0 / 1_000_000.0, Quantity::LENGTH),
            Self::Um => (1.0 / 1_000.0, Quantity::LENGTH),
            Self::Mm => (1.0, Quantity::LENGTH),
            Self::Cm => (10.0, Quantity::LENGTH),
            Self::M => (1_000.0, Quantity::LENGTH),
            Self::Km => (1_000_000.0, Quantity::LENGTH),
            Self::In => (25.4, Quantity::LENGTH),
            Self::Ft => (304.8, Quantity::LENGTH),
            Self::Yd => (914.4, Quantity::LENGTH),
            Self::Mi => (1_609_344.0, Quantity::LENGTH),
            Self::Rad => (1.0, Quantity::ANGLE),
            Self::Deg => (PI / 180.0, Quantity::ANGLE),
            Self::Grad => (PI / 200.0, Quantity::ANGLE),
            Self::Arcmin => (PI / 10_800.0, Quantity::ANGLE),
            Self::Arcsec => (PI / 648_000.0, Quantity::ANGLE),
            Self::Rev => (2.0 * PI, Quantity::ANGLE),
            Self::S => (1.0, TIME),
            Self::Min => (60.0, TIME),
            Self::H => (3_600.0, TIME),
            Self::G => (1.0 / 1_000.0, MASS),
            Self::Kg => (1.0, MASS),
            Self::Lb => (0.453_592_37, MASS),
        }
    }

    /// The wire name of the symbol, which canonical expression text also
    /// writes.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Nm => "nm",
            Self::Um => "um",
            Self::Mm => "mm",
            Self::Cm => "cm",
            Self::M => "m",
            Self::Km => "km",
            Self::In => "in",
            Self::Ft => "ft",
            Self::Yd => "yd",
            Self::Mi => "mi",
            Self::Rad => "rad",
            Self::Deg => "deg",
            Self::Grad => "grad",
            Self::Arcmin => "arcmin",
            Self::Arcsec => "arcsec",
            Self::Rev => "rev",
            Self::S => "s",
            Self::Min => "min",
            Self::H => "h",
            Self::G => "g",
            Self::Kg => "kg",
            Self::Lb => "lb",
        }
    }
}

/// The authoring unit system of a definition: the units the source document
/// displays and types values in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct UnitSystem {
    /// The length unit.
    pub length: UnitSymbol,
    /// The angle unit.
    pub angle: UnitSymbol,
    /// The mass unit.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_mass"
    )]
    pub mass: Option<UnitSymbol>,
    /// The time unit.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_time"
    )]
    pub time: Option<UnitSymbol>,
}

/// The declared kind of a parameter's value.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ValueKind {
    /// A quantity.
    Quantity(Quantity),
    /// An integer.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Integer,
    /// A Boolean.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Boolean,
    /// A text.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Text,
    /// A kind that the vocabulary does not type.
    Native(NonBlankString),
}

/// The exponents of length, mass, angle, and time of a quantity. Its
/// canonical unit is the product of millimetre, kilogram, radian, and second
/// to those exponents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Quantity {
    /// The exponent of length.
    pub length: i8,
    /// The exponent of mass.
    pub mass: i8,
    /// The exponent of angle.
    pub angle: i8,
    /// The exponent of time.
    pub time: i8,
}

impl Quantity {
    /// A length, (1, 0, 0, 0).
    pub const LENGTH: Self = Self::of(1, 0, 0, 0);
    /// An angle, (0, 0, 1, 0).
    pub const ANGLE: Self = Self::of(0, 0, 1, 0);
    /// A dimensionless real, (0, 0, 0, 0).
    pub const REAL: Self = Self::of(0, 0, 0, 0);
    /// An area, (2, 0, 0, 0).
    pub const AREA: Self = Self::of(2, 0, 0, 0);

    /// The quantity with these exponents of length, mass, angle, and time.
    #[must_use]
    pub const fn of(length: i8, mass: i8, angle: i8, time: i8) -> Self {
        Self {
            length,
            mass,
            angle,
            time,
        }
    }

    /// The factor that converts a value of this quantity from a unit system
    /// of `millimetres`, `kilograms`, `radians`, and `seconds` per unit to
    /// the canonical units.
    #[must_use]
    pub fn factor(&self, millimetres: f64, kilograms: f64, radians: f64, seconds: f64) -> f64 {
        millimetres.powi(i32::from(self.length))
            * kilograms.powi(i32::from(self.mass))
            * radians.powi(i32::from(self.angle))
            * seconds.powi(i32::from(self.time))
    }
}

macro_rules! checked_quantity {
    ($(#[$attribute:meta])* $name:ident, $value:ident, $condition:expr, $error:literal) => {
        $(#[$attribute])*
        #[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
        #[cfg_attr(feature = "schema", derive(JsonSchema))]
        #[serde(into = "f64", try_from = "f64")]
        pub struct $name(f64);

        impl $name {
            /// Admits a value within the quantity's domain. A number has one
            /// zero: −0 is admitted as 0.
            #[must_use]
            pub const fn new($value: f64) -> Option<Self> {
                if $value.is_finite() && $condition {
                    Some(Self($value + 0.0))
                } else {
                    None
                }
            }

            /// Returns the value in canonical units.
            #[must_use]
            pub const fn get(self) -> f64 {
                self.0
            }
        }

        impl TryFrom<f64> for $name {
            type Error = &'static str;

            /// Admits a value within the quantity's domain.
            fn try_from($value: f64) -> Result<Self, Self::Error> {
                Self::new($value).ok_or($error)
            }
        }

        impl From<$name> for f64 {
            fn from(quantity: $name) -> Self {
                quantity.0
            }
        }
    };
}

checked_quantity!(
    /// A finite duration in seconds.
    Duration, value, true, "a Duration is finite"
);

checked_quantity!(
    /// A finite linear feed in millimetres per second.
    Feed, value, true, "a Feed is finite"
);

checked_quantity!(
    /// A finite angular speed in radians per second.
    AngularSpeed, value, true, "an AngularSpeed is finite"
);

checked_quantity!(
    /// A positive finite area in square millimetres.
    PositiveArea, value, value > 0.0, "a PositiveArea is finite and positive"
);

cadmpeg_core::named_optional_field!(deserialize_mass, UnitSymbol, "mass");
cadmpeg_core::named_optional_field!(deserialize_time, UnitSymbol, "time");
