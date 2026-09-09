// SPDX-License-Identifier: Apache-2.0
//! Canonical units and tolerances.
//!
//! Stored lengths and coordinates use millimeters. Angular quantities use
//! radians.

use crate::math::{Point2, Vector3};
pub use crate::scalar::{
    FiniteReal as FiniteScalar, NonNegativeReal as NonNegativeScalar,
    PositiveReal as PositiveScalar,
};
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

const DEFAULT_LINEAR_TOLERANCE: PositiveScalar = PositiveScalar::new(1.0e-6).unwrap();
const DEFAULT_ANGULAR_TOLERANCE: PositiveScalar = PositiveScalar::new(1.0e-10).unwrap();

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

    /// Borrow the coordinates.
    pub const fn as_raw(&self) -> &[f64; N] {
        &self.0
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

/// Define the concrete shim required by serde's field deserializer path.
macro_rules! named_field {
    ($name:ident, $value:ty, $field:literal) => {
        fn $name<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<$value, D::Error> {
            $crate::units::deserialize_named(deserializer, $field)
        }
    };
}
pub(crate) use named_field;

pub(crate) fn deserialize_named<'de, D, T>(deserializer: D, field: &str) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer)
        .map_err(|error| serde::de::Error::custom(format_args!("{field}: {error}")))
}

crate::units::named_field!(deserialize_linear, PositiveScalar, "linear");

crate::units::named_field!(deserialize_angular, PositiveScalar, "angular");

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
            linear: DEFAULT_LINEAR_TOLERANCE,
            angular: DEFAULT_ANGULAR_TOLERANCE,
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

const EPS_UNIT_FRAME: f64 = 1.0e-9;

/// A direction with unit length within the analytic frame tolerance.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "Vector3", into = "Vector3")]
pub struct UnitVector3(Vector3);

impl UnitVector3 {
    /// Admit a unit direction.
    pub fn new(value: Vector3) -> Option<Self> {
        ((value.norm() - 1.0).abs() <= EPS_UNIT_FRAME).then_some(Self(value))
    }
    /// Borrow the direction.
    pub const fn as_raw(&self) -> &Vector3 {
        &self.0
    }
    /// Reverse the direction.
    pub fn reversed(self) -> Self {
        Self(Vector3::new(-self.0.x, -self.0.y, -self.0.z))
    }
}
impl TryFrom<Vector3> for UnitVector3 {
    type Error = &'static str;
    fn try_from(value: Vector3) -> Result<Self, Self::Error> {
        Self::new(value).ok_or("direction must have unit length")
    }
}
impl From<UnitVector3> for Vector3 {
    fn from(value: UnitVector3) -> Self {
        value.0
    }
}

/// Two perpendicular unit directions within the analytic frame tolerance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrthonormalFrame3 {
    axis: UnitVector3,
    reference: UnitVector3,
}
impl OrthonormalFrame3 {
    /// Admit two perpendicular unit directions.
    pub fn new(axis: Vector3, reference: Vector3) -> Option<Self> {
        let axis = UnitVector3::new(axis)?;
        let reference = UnitVector3::new(reference)?;
        (axis.0.dot(reference.0).abs() <= EPS_UNIT_FRAME).then_some(Self { axis, reference })
    }
    /// Borrow the first direction.
    pub const fn axis(&self) -> &Vector3 {
        self.axis.as_raw()
    }
    /// Borrow the second direction.
    pub const fn reference(&self) -> &Vector3 {
        self.reference.as_raw()
    }
    /// Reverse the first direction.
    pub fn reverse_axis(&mut self) {
        self.axis = self.axis.reversed();
    }
    /// Reverse the second direction.
    pub fn reverse_reference(&mut self) {
        self.reference = self.reference.reversed();
    }
}

/// A parameter-space point with finite coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "Point2", into = "Point2")]
pub struct FinitePoint2(Point2);
impl FinitePoint2 {
    /// The parameter-space origin.
    pub const ZERO: Self = Self(Point2 { u: 0.0, v: 0.0 });
    /// Admit finite coordinates.
    pub fn new(value: Point2) -> Option<Self> {
        FiniteVector::new([value.u, value.v]).map(|_| Self(value))
    }
    /// Borrow the point.
    pub const fn as_raw(&self) -> &Point2 {
        &self.0
    }
}
impl TryFrom<Point2> for FinitePoint2 {
    type Error = &'static str;
    fn try_from(value: Point2) -> Result<Self, Self::Error> {
        Self::new(value).ok_or("coordinates must be finite")
    }
}
impl From<FinitePoint2> for Point2 {
    fn from(value: FinitePoint2) -> Self {
        value.0
    }
}

/// A finite parameter-space direction whose squared norm exceeds machine epsilon.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "Point2", into = "Point2")]
pub struct NonzeroPoint2(Point2);
impl NonzeroPoint2 {
    /// The unit-u direction.
    pub const U_AXIS: Self = Self(Point2 { u: 1.0, v: 0.0 });
    /// Admit the shared nonzero-vector contract.
    pub fn new(value: Point2) -> Option<Self> {
        NonzeroVector::new([value.u, value.v]).map(|_| Self(value))
    }
    /// Borrow the direction.
    pub const fn as_raw(&self) -> &Point2 {
        &self.0
    }
}
impl TryFrom<Point2> for NonzeroPoint2 {
    type Error = &'static str;
    fn try_from(value: Point2) -> Result<Self, Self::Error> {
        Self::new(value).ok_or("direction must be finite with squared norm greater than epsilon")
    }
}
impl From<NonzeroPoint2> for Point2 {
    fn from(value: NonzeroPoint2) -> Self {
        value.0
    }
}

impl FiniteVector<2> {
    /// Reverse coordinate order and signs.
    pub const fn reversed_negated(self) -> Self {
        Self([-self.0[1], -self.0[0]])
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
