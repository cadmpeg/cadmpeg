// SPDX-License-Identifier: Apache-2.0
//! Canonical units and tolerances.
//!
//! Stored lengths and coordinates use millimeters. Angular quantities use
//! radians.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub(crate) struct CanonicalUnitsWire {
    length: CanonicalLengthUnitWire,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
enum CanonicalLengthUnitWire {
    #[default]
    Millimeter,
}

/// Maximum distance, in the document's length unit, between an evaluated
/// carrier point and the vertex position it must coincide with. Exact carriers
/// agree to rational-weight rounding (well under `1e-3` mm); real mismatches
/// from decoder defects start orders of magnitude above this bound. A decoder
/// that binds a carrier to topology applies the same bound, so a binding it
/// accepts is one the topology contract also accepts.
pub const COINCIDENCE_TOLERANCE: f64 = 0.01;

const DEFAULT_LINEAR_TOLERANCE: f64 = 1.0e-6;
const DEFAULT_ANGULAR_TOLERANCE: f64 = 1.0e-10;

macro_rules! checked_scalar {
    ($name:ident, $doc:literal, $value:ident, $valid:expr, $error:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize)]
        #[cfg_attr(feature = "schema", derive(JsonSchema))]
        #[serde(transparent)]
        pub struct $name(f64);

        impl $name {
            /// Construct a value that satisfies this scalar's numeric contract.
            pub const fn new($value: f64) -> Option<Self> {
                if $valid {
                    Some(Self($value))
                } else {
                    None
                }
            }

            /// Return the numeric value.
            pub const fn get(self) -> f64 {
                self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                Self::new(f64::deserialize(deserializer)?)
                    .ok_or_else(|| serde::de::Error::custom($error))
            }
        }
    };
}

checked_scalar!(
    FiniteScalar,
    "A finite signed scalar.",
    value,
    value.is_finite(),
    "value must be finite"
);
checked_scalar!(
    PositiveScalar,
    "A positive finite scalar.",
    value,
    value.is_finite() && value > 0.0,
    "value must be positive and finite"
);
checked_scalar!(
    NonNegativeScalar,
    "A nonnegative finite scalar.",
    value,
    value.is_finite() && value >= 0.0,
    "value must be nonnegative and finite"
);

/// An array of finite coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FiniteVector<const N: usize>([f64; N]);

impl<const N: usize> FiniteVector<N> {
    /// Construct finite coordinates.
    pub fn new(value: [f64; N]) -> Option<Self> {
        value
            .iter()
            .all(|component| component.is_finite())
            .then_some(Self(value))
    }

    /// Return the coordinates.
    pub const fn get(self) -> [f64; N] {
        self.0
    }
}

/// Finite coordinates whose squared norm exceeds machine epsilon.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NonzeroVector<const N: usize>([f64; N]);

impl<const N: usize> NonzeroVector<N> {
    /// Construct finite coordinates with a nonzero accepted norm.
    pub fn new(value: [f64; N]) -> Option<Self> {
        (value.iter().all(|component| component.is_finite())
            && value
                .iter()
                .map(|component| component * component)
                .sum::<f64>()
                > f64::EPSILON)
            .then_some(Self(value))
    }

    /// Return the coordinates without normalization.
    pub const fn get(self) -> [f64; N] {
        self.0
    }
}

macro_rules! vector_wire {
    ($name:ident, $error:literal) => {
        impl<const N: usize> Serialize for $name<N>
        where
            [f64; N]: Serialize,
        {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                self.0.serialize(serializer)
            }
        }
        impl<'de, const N: usize> Deserialize<'de> for $name<N>
        where
            [f64; N]: Deserialize<'de>,
        {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                Self::new(<[f64; N]>::deserialize(deserializer)?)
                    .ok_or_else(|| serde::de::Error::custom($error))
            }
        }
        #[cfg(feature = "schema")]
        impl<const N: usize> JsonSchema for $name<N>
        where
            [f64; N]: JsonSchema,
        {
            fn schema_name() -> std::borrow::Cow<'static, str> {
                format!("{}{}", stringify!($name), N).into()
            }
            fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
                <[f64; N]>::json_schema(generator)
            }
        }
    };
}

vector_wire!(FiniteVector, "coordinates must be finite");
vector_wire!(
    NonzeroVector,
    "coordinates must be finite with squared norm greater than epsilon"
);

pub(crate) fn deserialize_named<'de, D, T>(deserializer: D, field: &str) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer)
        .map_err(|error| serde::de::Error::custom(format_args!("{field}: {error}")))
}

fn deserialize_linear<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<PositiveScalar, D::Error> {
    deserialize_named(deserializer, "linear")
}

fn deserialize_angular<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<PositiveScalar, D::Error> {
    deserialize_named(deserializer, "angular")
}

/// Document-wide linear and angular tolerances.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Tolerances {
    /// Linear tolerance in millimeters.
    #[serde(deserialize_with = "deserialize_linear")]
    pub linear: PositiveScalar,
    /// Angular tolerance in radians.
    #[serde(deserialize_with = "deserialize_angular")]
    pub angular: PositiveScalar,
}

impl Default for Tolerances {
    fn default() -> Self {
        Tolerances {
            linear: PositiveScalar(DEFAULT_LINEAR_TOLERANCE),
            angular: PositiveScalar(DEFAULT_ANGULAR_TOLERANCE),
        }
    }
}

impl Tolerances {
    /// Construct positive finite document tolerances.
    pub fn new(linear: f64, angular: f64) -> Result<Self, String> {
        Ok(Self {
            linear: PositiveScalar::new(linear)
                .ok_or_else(|| "linear tolerance must be positive and finite".to_owned())?,
            angular: PositiveScalar::new(angular)
                .ok_or_else(|| "angular tolerance must be positive and finite".to_owned())?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_admission_matches_each_numeric_contract() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(FiniteScalar::new(value).is_none());
            assert!(PositiveScalar::new(value).is_none());
            assert!(NonNegativeScalar::new(value).is_none());
        }
        for value in [-1.0, -0.0, 0.0] {
            assert!(PositiveScalar::new(value).is_none());
            assert_eq!(
                FiniteScalar::new(value).expect("finite").get().to_bits(),
                value.to_bits()
            );
        }
        assert!(NonNegativeScalar::new(-1.0).is_none());
        for value in [-0.0, 0.0, f64::MIN_POSITIVE, f64::MAX] {
            let admitted = NonNegativeScalar::new(value).expect("nonnegative");
            let wire = serde_json::to_string(&admitted).expect("serialize");
            let decoded: NonNegativeScalar = serde_json::from_str(&wire).expect("deserialize");
            assert_eq!(decoded.get().to_bits(), value.to_bits());
        }
        assert!(serde_json::from_str::<PositiveScalar>("0").is_err());
        assert!(serde_json::from_str::<NonNegativeScalar>("-1").is_err());
    }

    #[test]
    fn vectors_preserve_finite_nonunit_coordinates_and_reject_invalid_norms() {
        assert!(FiniteVector::new([f64::NAN, 0.0]).is_none());
        assert!(NonzeroVector::new([0.0, 0.0, 0.0]).is_none());
        assert!(NonzeroVector::new([f64::EPSILON, 0.0, 0.0]).is_none());
        assert!(serde_json::from_str::<NonzeroVector<3>>("[0,0,0]").is_err());
        let coordinates = [2.0, -3.0, 4.0];
        let value = NonzeroVector::new(coordinates).expect("nonzero");
        assert_eq!(value.get(), coordinates);
        let wire = serde_json::to_string(&value).expect("serialize");
        assert_eq!(wire, "[2.0,-3.0,4.0]");
        assert_eq!(
            serde_json::from_str::<NonzeroVector<3>>(&wire).expect("deserialize"),
            value
        );
        assert!(NonzeroVector::new([f64::MAX, 0.0, 0.0]).is_some());
    }

    #[test]
    fn tolerance_wire_errors_name_the_rejected_field() {
        for (wire, field) in [
            (r#"{"linear":0,"angular":1}"#, "linear"),
            (r#"{"linear":1,"angular":-1}"#, "angular"),
        ] {
            let error = serde_json::from_str::<Tolerances>(wire).expect_err("invalid tolerance");
            assert!(error.to_string().contains(field));
        }
        let value = Tolerances::new(0.25, 0.5).expect("positive finite");
        assert_eq!(
            serde_json::to_string(&value).expect("serialize"),
            r#"{"linear":0.25,"angular":0.5}"#
        );
    }
}
