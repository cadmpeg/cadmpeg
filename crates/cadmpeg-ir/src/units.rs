// SPDX-License-Identifier: Apache-2.0
//! Canonical units and tolerances.
//!
//! Stored lengths and coordinates use millimeters. Angular quantities use
//! radians.

use crate::math::sum::ScaledValue;
use crate::math::{Point2, Vector3};
use crate::scalar::{PositiveAngle, PositiveLength};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub(crate) struct CanonicalUnitsWire {
    length: CanonicalLengthUnitWire,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
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

impl<const N: usize> From<[crate::scalar::FiniteReal; N]> for FiniteVector<N> {
    /// Carry finite scalars as coordinates. Every scalar is finite, so
    /// nothing is checked.
    fn from(value: [crate::scalar::FiniteReal; N]) -> Self {
        Self(value.map(crate::scalar::FiniteReal::get))
    }
}

impl<const N: usize> std::ops::Deref for FiniteVector<N> {
    type Target = [f64; N];
    fn deref(&self) -> &[f64; N] {
        &self.0
    }
}

impl<const N: usize> IntoIterator for FiniteVector<N> {
    type Item = f64;
    type IntoIter = std::array::IntoIter<f64, N>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a, const N: usize> IntoIterator for &'a FiniteVector<N> {
    type Item = &'a f64;
    type IntoIter = std::slice::Iter<'a, f64>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl From<crate::topology::ParameterInterval> for FiniteVector<2> {
    /// Carry an interval's endpoints. The interval admits only finite
    /// endpoints, so nothing is checked.
    fn from(value: crate::topology::ParameterInterval) -> Self {
        Self(value.endpoints())
    }
}

impl From<crate::topology::IncreasingParameterInterval> for FiniteVector<2> {
    /// Carry an increasing interval's endpoints. The interval admits only
    /// finite endpoints, so nothing is checked.
    fn from(value: crate::topology::IncreasingParameterInterval) -> Self {
        Self(value.endpoints())
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

crate::units::named_field!(deserialize_linear, PositiveLength, "linear");

crate::units::named_field!(deserialize_angular, PositiveAngle, "angular");

const EPS_DOCUMENT_LINEAR_MM: f64 = 1.0e-6;
const EPS_DOCUMENT_ANGULAR_RADIANS: f64 = 1.0e-10;
// Admission runs during constant evaluation. An invalid policy literal fails
// compilation; these branches are not runtime panic paths.
const DEFAULT_LINEAR_TOLERANCE: PositiveLength = match PositiveLength::new(EPS_DOCUMENT_LINEAR_MM) {
    Some(value) => value,
    None => panic!("the default linear tolerance must be positive and finite"),
};
const DEFAULT_ANGULAR_TOLERANCE: PositiveAngle =
    match PositiveAngle::new(EPS_DOCUMENT_ANGULAR_RADIANS) {
        Some(value) => value,
        None => panic!("the default angular tolerance must be positive and finite"),
    };

/// Document-wide linear and angular tolerances.
///
/// The field types carry the units. A linear tolerance cannot be stored in
/// `angular`, and the document policy for a file that states no tolerance of
/// its own is `1.0e-6` millimetres and `1.0e-10` radians.
///
/// Moving each stated tolerance into its own field compiles:
///
/// ```
/// let stated = cadmpeg_ir::units::Tolerances::new(1.0e-3, 1.0e-4)
///     .expect("positive finite tolerances");
/// let moved = cadmpeg_ir::units::Tolerances {
///     linear: stated.linear,
///     angular: stated.angular,
/// };
/// assert_eq!(moved, stated);
/// ```
///
/// Exchanging them does not:
///
/// ```compile_fail
/// let stated = cadmpeg_ir::units::Tolerances::new(1.0e-3, 1.0e-4)
///     .expect("positive finite tolerances");
/// let swapped = cadmpeg_ir::units::Tolerances {
///     linear: stated.angular,
///     angular: stated.linear,
/// };
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Tolerances {
    /// Linear tolerance in millimeters.
    #[serde(deserialize_with = "deserialize_linear")]
    pub linear: PositiveLength,
    /// Angular tolerance in radians.
    #[serde(deserialize_with = "deserialize_angular")]
    pub angular: PositiveAngle,
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
            linear: PositiveLength::new(linear)
                .ok_or_else(|| "linear tolerance must be positive and finite".to_owned())?,
            angular: PositiveAngle::new(angular)
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
    /// The unit +x direction.
    pub const X_AXIS: Self = Self(Vector3 {
        x: 1.0,
        y: 0.0,
        z: 0.0,
    });
    /// The unit +y direction.
    pub const Y_AXIS: Self = Self(Vector3 {
        x: 0.0,
        y: 1.0,
        z: 0.0,
    });
    /// The unit +z direction.
    pub const Z_AXIS: Self = Self(Vector3 {
        x: 0.0,
        y: 0.0,
        z: 1.0,
    });
    /// Admit a unit direction.
    pub fn new(value: Vector3) -> Option<Self> {
        ((value.norm() - 1.0).abs() <= EPS_UNIT_FRAME).then_some(Self(value))
    }
    /// The unit direction of `value`, as [`Vector3::unit`] computes it.
    ///
    /// [`Vector3::unit`] divides finite components, charted so the largest
    /// magnitude is in `[0.5, 1)`, by their finite nonzero norm. Each quotient
    /// is finite and within a few rounding errors of the exact unit
    /// direction, so the norm is within rounding of one and the result keeps
    /// the admission of [`Self::new`].
    #[must_use]
    pub fn normalized(value: Vector3) -> Option<Self> {
        value.unit().map(Self)
    }
    /// The unit direction of `value`: each component divided by the largest
    /// component magnitude, then by the `hypot` length of the quotients. The
    /// direction is absent when a component is not finite or every component
    /// is zero.
    ///
    /// The largest quotient has magnitude one and the others at most one, so
    /// the `hypot` length is in `[1, √3]`, finite and nonzero. The final
    /// quotients have a norm within rounding of one, which keeps the
    /// admission of [`Self::new`].
    #[must_use]
    pub fn normalized_by_largest_component(value: Vector3) -> Option<Self> {
        if !value.is_finite() {
            return None;
        }
        let largest = value.x.abs().max(value.y.abs()).max(value.z.abs());
        (largest != 0.0).then(|| Self(divided_by_largest_component(value, largest)))
    }
    /// Normalize three exact sums, each rescaled into the frame of the largest
    /// exponent, and reverse them when `reversed` is set.
    ///
    /// The largest sum rescales to its mantissa, whose magnitude is in
    /// `[0.5, 1)`, and every other sum to a magnitude below one. The squared
    /// length is therefore in `[0.25, 3)`, its square root is finite and
    /// nonzero, and the three quotients have a norm within rounding of one,
    /// which keeps the admission of [`Self::new`]. The result is absent only
    /// when every sum is zero.
    pub(crate) fn from_exact_sums(
        values: [Option<ScaledValue>; 3],
        reversed: bool,
    ) -> Option<Self> {
        let scale_exponent = values
            .iter()
            .filter_map(|value| value.map(|value| value.exponent))
            .max()?;
        let orientation = if reversed { -1.0 } else { 1.0 };
        let scaled = values
            .map(|value| value.map_or(0.0, |value| orientation * value.scaled_by(scale_exponent)));
        let length = scaled.iter().map(|value| value * value).sum::<f64>().sqrt();
        Some(Self(Vector3::new(
            scaled[0] / length,
            scaled[1] / length,
            scaled[2] / length,
        )))
    }
    /// Borrow the direction.
    pub const fn as_raw(&self) -> &Vector3 {
        &self.0
    }
    /// The direction divided by its length.
    ///
    /// The admission holds the length within `1e-9` of one and does not
    /// rescale the direction. The length is therefore finite and nonzero,
    /// and the quotients have unit length to rounding, which keeps the
    /// admission. A reader that needs unit length to rounding, such as a
    /// rotation about the direction or a matrix read at printed precision,
    /// uses this value.
    #[must_use]
    pub fn to_unit_length(self) -> Self {
        let length = self.0.norm();
        Self(Vector3::new(
            self.0.x / length,
            self.0.y / length,
            self.0.z / length,
        ))
    }
    /// The direction divided by its length with the arithmetic of
    /// [`Vector3::unit_nonzero`], for a reader whose output bits follow that
    /// arithmetic.
    ///
    /// `unit_nonzero` multiplies each component by the power of two that
    /// puts the largest magnitude in `[0.5, 1)` and divides the result by
    /// the `hypot` length of the three results. A component whose result is
    /// below the normal range is divided from its original value instead:
    /// the quotient of the two significands is rounded once, then the power
    /// of two is applied with one more rounding.
    ///
    /// The admission holds the length within `1e-9` of one, so the largest
    /// magnitude is in `[0.5, 2)` and the power of two is one below one and
    /// one half from one. The multiplied components are finite, the length
    /// is in `[0.5, √3)`, and every quotient is finite: no step refuses.
    /// Multiplying a small component by `2^64` is exact and keeps it and its
    /// quotient by the length normal, so that quotient rounds the same
    /// significand quotient. One multiplication by the remaining power of
    /// two then gives the same final rounding. The quotients have unit
    /// length to rounding, which keeps the admission.
    #[must_use]
    pub fn to_unit_length_charted(self) -> Self {
        let value = self.0;
        let largest = value.x.abs().max(value.y.abs()).max(value.z.abs());
        let exponent = if largest < 1.0 { 0 } else { 1 };
        let chart_scale = 2.0_f64.powi(-exponent);
        let chart = Vector3::new(
            value.x * chart_scale,
            value.y * chart_scale,
            value.z * chart_scale,
        );
        let length = chart.norm();
        let component = |original: f64, charted: f64| {
            if original != 0.0 && charted.abs() < f64::MIN_POSITIVE {
                original * 2.0_f64.powi(64) / length * 2.0_f64.powi(-64 - exponent)
            } else {
                charted / length
            }
        };
        Self(Vector3::new(
            component(value.x, chart.x),
            component(value.y, chart.y),
            component(value.z, chart.z),
        ))
    }
    /// Reverse the direction.
    #[must_use]
    pub fn reversed(self) -> Self {
        Self(Vector3::new(-self.0.x, -self.0.y, -self.0.z))
    }
    /// Place a planar direction in the xy plane, as `[first, second, 0]`.
    ///
    /// `hypot(h, 0)` is `|h|`, so the placed direction's norm is the planar
    /// direction's `hypot` length bit for bit and keeps its admission.
    #[must_use]
    fn in_xy_plane(planar: UnitVector2) -> Self {
        let [first, second] = planar.0;
        Self(Vector3::new(first, second, 0.0))
    }
    /// Place a planar direction in the xz plane, as `[first, 0, second]`.
    ///
    /// `hypot(first, 0)` is `|first|`, and `hypot` does not depend on the
    /// sign of an argument, so the placed direction's norm is the planar
    /// direction's `hypot` length bit for bit and keeps its admission.
    #[must_use]
    fn in_xz_plane(planar: UnitVector2) -> Self {
        let [first, second] = planar.0;
        Self(Vector3::new(first, 0.0, second))
    }
}
/// Divide `value` by `largest`, its nonzero largest component magnitude, and
/// then by the `hypot` length of the quotients.
fn divided_by_largest_component(value: Vector3, largest: f64) -> Vector3 {
    let scaled = [value.x / largest, value.y / largest, value.z / largest];
    let length = scaled[0].hypot(scaled[1]).hypot(scaled[2]);
    Vector3::new(scaled[0] / length, scaled[1] / length, scaled[2] / length)
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

/// A planar direction whose `hypot` length is one within the analytic frame
/// tolerance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnitVector2([f64; 2]);
impl UnitVector2 {
    /// Admit a planar unit direction.
    pub fn new(value: [f64; 2]) -> Option<Self> {
        ((value[0].hypot(value[1]) - 1.0).abs() <= EPS_UNIT_FRAME).then_some(Self(value))
    }
    /// Return the direction components.
    pub const fn get(self) -> [f64; 2] {
        self.0
    }
    /// Turn the direction a quarter turn, to `[-second, first]`. `hypot`
    /// does not depend on the order or the sign of its arguments, so the
    /// length stays the same bit for bit.
    #[must_use]
    pub fn quarter_turn(self) -> Self {
        Self([-self.0[1], self.0[0]])
    }
    /// Turn the direction a quarter turn the other way, to
    /// `[second, -first]`. The length stays the same bit for bit.
    #[must_use]
    pub fn reverse_quarter_turn(self) -> Self {
        Self([self.0[1], -self.0[0]])
    }
}

const EPS_RIGHT_HANDED_FRAME: f64 = 1.0e-12;
const EPS_RIGHT_HANDED_FRAME_DISTANCE_SQUARED: f64 = 4.0e-24;
const EPS_RIGHT_HANDED_FRAME_1E12_DISTANCE_SQUARED: f64 = 1.0e-24;

/// Two perpendicular unit directions within the analytic frame tolerance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrthonormalFrame3 {
    axis: UnitVector3,
    reference: UnitVector3,
}
impl OrthonormalFrame3 {
    /// The model coordinate frame: first direction +z, second direction +x.
    pub const IDENTITY: Self = Self {
        axis: UnitVector3::Z_AXIS,
        reference: UnitVector3::X_AXIS,
    };
    /// The frame with first direction +x and second direction +y.
    pub const X_AXIS_Y_REFERENCE: Self = Self {
        axis: UnitVector3::X_AXIS,
        reference: UnitVector3::Y_AXIS,
    };
    /// Build the frame of a planar direction in the xy plane: the first
    /// direction is `[first, second, 0]` and the second is its quarter turn
    /// `[-second, first, 0]`. Each has the planar direction's `hypot` length
    /// bit for bit, so each keeps the planar admission. The two are
    /// perpendicular exactly: the dot product is
    /// `first·(-second) + second·first`, which is zero.
    #[must_use]
    pub fn in_xy_plane(axis: UnitVector2) -> Self {
        Self {
            axis: UnitVector3::in_xy_plane(axis),
            reference: UnitVector3::in_xy_plane(axis.quarter_turn()),
        }
    }
    /// Build the frame with first direction +y and a planar second direction
    /// placed in the xz plane as `[first, 0, second]`. The placed direction has
    /// the planar direction's `hypot` length bit for bit, so it keeps the
    /// planar admission. It has no y component, so the two are perpendicular
    /// exactly.
    #[must_use]
    pub fn about_y_axis(reference: UnitVector2) -> Self {
        Self {
            axis: UnitVector3::Y_AXIS,
            reference: UnitVector3::in_xz_plane(reference),
        }
    }
    /// Admit two perpendicular unit directions.
    pub fn new(axis: Vector3, reference: Vector3) -> Option<Self> {
        Self::from_units(UnitVector3::new(axis)?, UnitVector3::new(reference)?)
    }
    /// Admit two admitted unit directions that are perpendicular. Only the
    /// perpendicularity is checked.
    pub fn from_units(axis: UnitVector3, reference: UnitVector3) -> Option<Self> {
        (axis.0.dot(reference.0).abs() <= EPS_UNIT_FRAME).then_some(Self { axis, reference })
    }
    /// Admit the frame of `axis` and `reference` when the three directions
    /// form a right-handed orthonormal frame to `1e-12`: the plain dot
    /// product of `reference` and `binormal` is at most `1e-12` in
    /// magnitude, and each component of the cross product
    /// `reference × binormal`, a difference of two plain products, is within
    /// `1e-12` of the same component of `axis`.
    ///
    /// The cross-product condition holds the perpendicularity of `axis` and
    /// `reference`, so it is not measured again.
    /// `reference · (reference × binormal)` is zero for the exact cross
    /// product. `|reference · axis|` is therefore at most the
    /// length of `reference`, which is within `1e-9` of one, times the
    /// euclidean deviation, which is at most `√3·1e-12`, plus the rounding of
    /// the computed cross product, which is below `1e-15` for unit
    /// directions. The sum is below `2e-12`, inside the `1e-9` frame
    /// tolerance of [`Self::from_units`].
    pub fn right_handed(
        axis: UnitVector3,
        reference: UnitVector3,
        binormal: UnitVector3,
    ) -> Option<Self> {
        let (first, second) = (reference.0, binormal.0);
        let cross = [
            first.y * second.z - first.z * second.y,
            first.z * second.x - first.x * second.z,
            first.x * second.y - first.y * second.x,
        ];
        ((first.x * second.x + first.y * second.y + first.z * second.z).abs()
            <= EPS_RIGHT_HANDED_FRAME
            && cross
                .iter()
                .zip([axis.0.x, axis.0.y, axis.0.z])
                .all(|(cross, axis)| (cross - axis).abs() <= EPS_RIGHT_HANDED_FRAME))
        .then_some(Self { axis, reference })
    }
    /// Admit the frame of `axis` and `reference` when the three directions
    /// form a right-handed orthonormal frame to a euclidean distance of
    /// `2e-12`: the plain dot product of `reference` and `binormal` is at
    /// most `1e-12` in magnitude, and the squared euclidean distance of the
    /// cross product `reference × binormal`, as [`Vector3::cross`] computes
    /// it, from `axis` is at most `4e-24`.
    ///
    /// The cross-product condition holds the perpendicularity of `axis` and
    /// `reference`, so it is not measured again. `|reference · axis|` is at
    /// most the length of `reference`, which is within `1e-9` of one, times
    /// the distance, which is at most `2e-12`, plus the rounding of the
    /// computed cross product, which is below `1e-15` for unit directions.
    /// The sum is below `3e-12`, inside the `1e-9` frame tolerance of
    /// [`Self::from_units`].
    pub fn right_handed_euclidean(
        axis: UnitVector3,
        reference: UnitVector3,
        binormal: UnitVector3,
    ) -> Option<Self> {
        Self::right_handed_within(
            axis,
            reference,
            binormal,
            EPS_RIGHT_HANDED_FRAME_DISTANCE_SQUARED,
        )
    }
    /// Admit the frame of `axis` and `reference` when the three directions
    /// form a right-handed orthonormal frame to a euclidean distance of
    /// `1e-12`: the plain dot product of `reference` and `binormal` is at
    /// most `1e-12` in magnitude, and the squared euclidean distance of the
    /// cross product `reference × binormal`, as [`Vector3::cross`] computes
    /// it, from `axis` is at most `1e-24`.
    ///
    /// The distance bound is half the bound of
    /// [`Self::right_handed_euclidean`], so the same argument holds
    /// `|reference · axis|` below `2e-12`, inside the `1e-9` frame tolerance
    /// of [`Self::from_units`].
    pub fn right_handed_euclidean_1e12(
        axis: UnitVector3,
        reference: UnitVector3,
        binormal: UnitVector3,
    ) -> Option<Self> {
        Self::right_handed_within(
            axis,
            reference,
            binormal,
            EPS_RIGHT_HANDED_FRAME_1E12_DISTANCE_SQUARED,
        )
    }
    fn right_handed_within(
        axis: UnitVector3,
        reference: UnitVector3,
        binormal: UnitVector3,
        distance_squared: f64,
    ) -> Option<Self> {
        let (first, second) = (reference.0, binormal.0);
        let cross = first.cross(second);
        let deviation = [cross.x - axis.0.x, cross.y - axis.0.y, cross.z - axis.0.z];
        ((first.x * second.x + first.y * second.y + first.z * second.z).abs()
            <= EPS_RIGHT_HANDED_FRAME
            && deviation[0] * deviation[0]
                + deviation[1] * deviation[1]
                + deviation[2] * deviation[2]
                <= distance_squared)
            .then_some(Self { axis, reference })
    }
    /// Admit the frame whose second direction is `reference` and whose first
    /// direction is the cross product `reference × transverse` divided by its
    /// length, when the dot product of `reference` and `transverse` is at most
    /// `1e-12` in magnitude.
    ///
    /// The squared length of the cross product is
    /// `|reference|²·|transverse|² − (reference · transverse)²`. Both lengths
    /// are within `1e-9` of one, so the length is within about `3e-9` of one,
    /// finite and nonzero, and the quotients have a norm within rounding of
    /// one. The exact cross product is perpendicular to `reference`; the
    /// computed one differs from it by rounding, so the first direction is
    /// perpendicular to `reference` within about `1e-15`, inside the `1e-9`
    /// frame tolerance of [`Self::from_units`].
    pub fn completing(reference: UnitVector3, transverse: UnitVector3) -> Option<Self> {
        if reference.0.dot(transverse.0).abs() > EPS_RIGHT_HANDED_FRAME {
            return None;
        }
        let cross = reference.0.cross(transverse.0);
        let length = cross.norm();
        Some(Self {
            axis: UnitVector3(Vector3::new(
                cross.x / length,
                cross.y / length,
                cross.z / length,
            )),
            reference,
        })
    }
    /// Admit the frame whose second direction is `reference` and whose first
    /// direction is the unit direction of the cross product
    /// `reference × transverse`, as
    /// [`UnitVector3::normalized_by_largest_component`] computes it, when the
    /// plain dot product of `reference` and `transverse` is at most `1e-12`
    /// in magnitude.
    ///
    /// The cross product is nonzero and finite by the argument of
    /// [`Self::completing`], so the normalization cannot refuse. The exact
    /// cross product is perpendicular to `reference`; the computed one and
    /// its normalization differ from it by rounding, so the first direction
    /// is perpendicular to `reference` within about `1e-15`, inside the
    /// `1e-9` frame tolerance of [`Self::from_units`].
    pub fn completing_by_largest_component(
        reference: UnitVector3,
        transverse: UnitVector3,
    ) -> Option<Self> {
        let (first, second) = (reference.0, transverse.0);
        ((first.x * second.x + first.y * second.y + first.z * second.z).abs()
            <= EPS_RIGHT_HANDED_FRAME)
            .then(|| {
                let cross = first.cross(second);
                let largest = cross.x.abs().max(cross.y.abs()).max(cross.z.abs());
                Self {
                    axis: UnitVector3(divided_by_largest_component(cross, largest)),
                    reference,
                }
            })
    }
    /// Admit the frame whose second direction is `reference` and whose first
    /// direction is the unit direction of `reference × +z`, as
    /// [`Vector3::unit`] computes it. The computed cross product is exactly
    /// `[reference.y, −reference.x, ±0]`, so the two directions are
    /// perpendicular to the rounding of the unit direction. The frame is
    /// absent when the cross product is not longer than [`f64::EPSILON`],
    /// where `reference` is parallel to +z to rounding.
    pub fn about_horizontal_normal(reference: UnitVector3) -> Option<Self> {
        let normal = reference.0.cross(Vector3::new(0.0, 0.0, 1.0)).unit()?;
        Some(Self {
            axis: UnitVector3(normal),
            reference,
        })
    }
    /// The unit direction of `axis × reference`, which completes the
    /// right-handed frame.
    ///
    /// The squared length of the cross product is
    /// `|axis|²·|reference|² − (axis · reference)²`. Both lengths are within
    /// `1e-9` of one and every route into the type holds
    /// `|axis · reference|` at most `1e-9`, so the length is within about
    /// `3e-9` of one, finite and nonzero, and the quotients have a norm
    /// within rounding of one, which keeps the admission of
    /// [`UnitVector3::new`].
    #[must_use]
    pub fn binormal(&self) -> UnitVector3 {
        let cross = self.axis.0.cross(self.reference.0);
        let length = cross.norm();
        UnitVector3(Vector3::new(
            cross.x / length,
            cross.y / length,
            cross.z / length,
        ))
    }
    /// Borrow the admitted first direction. A caller that moves it into
    /// another model object keeps the unit-length guarantee and performs no
    /// new admission.
    pub const fn axis(&self) -> &UnitVector3 {
        &self.axis
    }
    /// Borrow the admitted second direction. A caller that moves it into
    /// another model object keeps the unit-length guarantee and performs no
    /// new admission.
    pub const fn reference(&self) -> &UnitVector3 {
        &self.reference
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
    /// Return the point.
    pub const fn get(self) -> Point2 {
        self.0
    }
    /// Borrow the point.
    pub const fn as_raw(&self) -> &Point2 {
        &self.0
    }
    /// Reflect through the parameter-space origin. Negation keeps every
    /// coordinate finite, so the result stays admitted.
    #[must_use]
    pub fn negated(self) -> Self {
        Self(Point2::new(-self.0.u, -self.0.v))
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
impl crate::geometry::pcurve::PcurveNurbs {
    /// Control points in parameter order. [`Self::new`] and
    /// [`Self::edit_control_points`] admit every pole finite, so each pole is
    /// carried without a check. The route lives beside [`FinitePoint2`]
    /// because only this module constructs one.
    #[must_use]
    pub fn control_points(&self) -> Vec<FinitePoint2> {
        self.pole_rows()
            .points()
            .into_iter()
            .map(FinitePoint2)
            .collect()
    }
}
impl std::ops::Deref for FinitePoint2 {
    type Target = Point2;
    fn deref(&self) -> &Point2 {
        &self.0
    }
}
impl PartialEq<Point2> for FinitePoint2 {
    fn eq(&self, other: &Point2) -> bool {
        self.0 == *other
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
    /// The opposite direction. Negation keeps the coordinates finite and the
    /// squared norm unchanged, so nothing is checked.
    #[must_use]
    pub fn reversed(self) -> Self {
        Self(Point2::new(-self.0.u, -self.0.v))
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
impl From<NonzeroPoint2> for FinitePoint2 {
    /// Carry an admitted direction as a point. The nonzero admission
    /// requires finite coordinates, so no admission can refuse it.
    fn from(value: NonzeroPoint2) -> Self {
        Self(value.0)
    }
}

impl FiniteVector<2> {
    /// Reverse coordinate order and signs.
    #[must_use]
    pub const fn reversed_negated(self) -> Self {
        Self([-self.0[1], -self.0[0]])
    }
}

#[cfg(test)]
mod tests {
    use super::{FiniteVector, NonzeroVector, Tolerances, UnitVector3};
    use crate::math::Vector3;
    use crate::scalar::PositiveReal;
    use crate::scalar::{FiniteReal, NonNegativeReal};

    #[test]
    fn finite_scalars_carry_into_finite_coordinates_unchanged() {
        let scalars =
            [-0.0, f64::MAX, 5.0e-324].map(|value| FiniteReal::new(value).expect("finite scalar"));
        let coordinates = FiniteVector::from(scalars);
        assert_eq!(
            coordinates.get().map(f64::to_bits),
            [-0.0, f64::MAX, 5.0e-324].map(f64::to_bits)
        );
        assert_eq!(
            FiniteVector::new([-0.0, f64::MAX, 5.0e-324]),
            Some(coordinates)
        );
    }

    #[test]
    fn an_admitted_direction_at_unit_length_is_divided_by_its_length() {
        for (admitted, expected) in [
            (
                Vector3::new(0.0, 0.0, 1.0 + 5.0e-10),
                Vector3::new(0.0, 0.0, 1.0),
            ),
            (
                Vector3::new(-1.0 + 8.0e-10, 0.0, 0.0),
                Vector3::new(-1.0, 0.0, 0.0),
            ),
            (Vector3::new(0.6, 0.8, 0.0), Vector3::new(0.6, 0.8, 0.0)),
        ] {
            let direction = UnitVector3::new(admitted).expect("admitted direction");
            let unit = direction.to_unit_length();
            assert!((unit.as_raw().x - expected.x).abs() <= f64::EPSILON);
            assert!((unit.as_raw().y - expected.y).abs() <= f64::EPSILON);
            assert!((unit.as_raw().z - expected.z).abs() <= f64::EPSILON);
            assert!((unit.as_raw().norm() - 1.0).abs() <= 2.0 * f64::EPSILON);
            assert_eq!(UnitVector3::new(*unit.as_raw()), Some(unit));
        }
    }

    #[test]
    fn a_charted_unit_direction_has_the_bits_of_unit_nonzero() {
        for admitted in [
            Vector3::new(
                f64::from_bits(0xbff0_0000_0003_9c52),
                f64::from_bits(0x0000_a645_6bb3_8f02),
                f64::from_bits(0x169e_ea5d_0b4d_3dca),
            ),
            Vector3::new(0.6, 0.8, 0.0),
            Vector3::new(0.0, -1.0 - 8.0e-10, 0.0),
            Vector3::new(1.0 + 5.0e-10, 3.0e-310, -1.5 * f64::MIN_POSITIVE),
            Vector3::new(-0.0, 0.999_999_999_5, 5.0e-324),
            Vector3::new(0.577_350_269_2, -0.577_350_269_2, 0.577_350_269_2),
        ] {
            let direction = UnitVector3::new(admitted).expect("admitted direction");
            let expected = admitted.unit_nonzero().expect("finite nonzero direction");
            let charted = *direction.to_unit_length_charted().as_raw();
            assert_eq!(
                [charted.x, charted.y, charted.z].map(f64::to_bits),
                [expected.x, expected.y, expected.z].map(f64::to_bits)
            );
        }
    }

    #[test]
    fn a_normalized_direction_keeps_the_unit_admission() {
        for value in [
            Vector3::new(3.0, 4.0, 0.0),
            Vector3::new(1.0e300, -1.0e300, 1.0e300),
            Vector3::new(1.0e-10, 3.0e-10, -2.0e-310),
            Vector3::new(f64::MAX, f64::MAX, f64::MAX),
            Vector3::new(1.0, 1.0e-310, 0.0),
        ] {
            let unit = UnitVector3::normalized(value).expect("normalizable direction");
            assert_eq!(Some(*unit.as_raw()), value.unit());
            assert_eq!(UnitVector3::new(*unit.as_raw()), Some(unit));
        }
        for value in [
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0e-300, 0.0, 0.0),
            Vector3::new(f64::NAN, 1.0, 0.0),
            Vector3::new(f64::INFINITY, 0.0, 0.0),
        ] {
            assert_eq!(UnitVector3::normalized(value), None);
            assert_eq!(value.unit(), None);
        }
    }

    #[test]
    fn scalar_admission_matches_each_numeric_contract() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(FiniteReal::new(value).is_none());
            assert!(PositiveReal::new(value).is_none());
            assert!(NonNegativeReal::new(value).is_none());
        }
        for value in [-1.0, -0.0, 0.0] {
            assert!(PositiveReal::new(value).is_none());
            assert_eq!(
                FiniteReal::new(value).expect("finite").get().to_bits(),
                value.to_bits()
            );
        }
        assert!(NonNegativeReal::new(-1.0).is_none());
        for value in [-0.0, 0.0, f64::MIN_POSITIVE, f64::MAX] {
            let admitted = NonNegativeReal::new(value).expect("nonnegative");
            let wire = serde_json::to_string(&admitted).expect("serialize");
            let decoded: NonNegativeReal = serde_json::from_str(&wire).expect("deserialize");
            assert_eq!(decoded.get().to_bits(), value.to_bits());
        }
        assert!(serde_json::from_str::<PositiveReal>("0").is_err());
        assert!(serde_json::from_str::<NonNegativeReal>("-1").is_err());
    }

    #[test]
    fn vectors_preserve_finite_nonunit_coordinates_and_reject_invalid_norms() {
        assert!(FiniteVector::new([f64::NAN, 0.0]).is_none());
        assert!(NonzeroVector::new([0.0, 0.0, 0.0]).is_none());
        assert!(NonzeroVector::new([f64::EPSILON, 0.0, 0.0]).is_none());
        assert!(serde_json::from_str::<NonzeroVector<3>>("[0,0,0]").is_err());
        let coordinates = [2.0, -3.0, 4.0];
        let value = NonzeroVector::new(coordinates).expect("nonzero");
        assert_eq!(value.0, coordinates);
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

    #[test]
    fn unit_axis_constants_and_the_identity_frame_are_the_admitted_literals() {
        use super::{OrthonormalFrame3, UnitVector3};
        use crate::math::Vector3;

        for (constant, literal) in [
            (UnitVector3::X_AXIS, Vector3::new(1.0, 0.0, 0.0)),
            (UnitVector3::Y_AXIS, Vector3::new(0.0, 1.0, 0.0)),
            (UnitVector3::Z_AXIS, Vector3::new(0.0, 0.0, 1.0)),
        ] {
            assert_eq!(UnitVector3::new(literal), Some(constant));
            assert_eq!(
                [
                    constant.as_raw().x,
                    constant.as_raw().y,
                    constant.as_raw().z
                ]
                .map(f64::to_bits),
                [literal.x, literal.y, literal.z].map(f64::to_bits)
            );
        }
        assert_eq!(
            OrthonormalFrame3::new(Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0)),
            Some(OrthonormalFrame3::IDENTITY)
        );
        assert_eq!(*OrthonormalFrame3::IDENTITY.axis(), UnitVector3::Z_AXIS);
        assert_eq!(
            *OrthonormalFrame3::IDENTITY.reference(),
            UnitVector3::X_AXIS
        );
    }

    #[test]
    fn planar_directions_keep_their_admission_when_turned_and_placed() {
        use super::{UnitVector2, UnitVector3};

        let scale = 1.0 + 9.9e-10;
        for value in [[0.6 * scale, -0.8 * scale], [-scale, 0.0], [0.0, scale]] {
            let planar = UnitVector2::new(value).expect("planar direction inside the band");
            for turned in [planar, planar.quarter_turn(), planar.reverse_quarter_turn()] {
                let [first, second] = turned.get();
                let length = first.hypot(second);
                assert_eq!(length.to_bits(), value[0].hypot(value[1]).to_bits());
                for placed in [
                    UnitVector3::in_xy_plane(turned),
                    UnitVector3::in_xz_plane(turned),
                ] {
                    assert_eq!(placed.as_raw().norm().to_bits(), length.to_bits());
                    assert_eq!(UnitVector3::new(*placed.as_raw()), Some(placed));
                }
            }
        }
        let [first, second] = UnitVector2::new([0.6, -0.8])
            .expect("unit planar direction")
            .quarter_turn()
            .get();
        assert_eq!([first, second], [0.8, 0.6]);
        let placed = UnitVector3::in_xz_plane(UnitVector2::new([0.6, 0.8]).expect("unit"));
        assert_eq!(
            [placed.as_raw().x, placed.as_raw().y, placed.as_raw().z].map(f64::to_bits),
            [0.6_f64, 0.0, 0.8].map(f64::to_bits)
        );
        for rejected in [
            [1.0 + 2.0e-9, 0.0],
            [f64::NAN, 0.0],
            [f64::INFINITY, 0.0],
            [0.0, 0.0],
        ] {
            assert!(UnitVector2::new(rejected).is_none());
        }
    }

    #[test]
    fn planar_frames_are_admitted_and_perpendicular_by_construction() {
        use super::{OrthonormalFrame3, UnitVector2, UnitVector3};
        use crate::math::Vector3;

        let bits = |value: &Vector3| [value.x, value.y, value.z].map(f64::to_bits);
        let scale = 1.0 + 9.9e-10;
        for value in [
            [0.6 * scale, -0.8 * scale],
            [-scale, 0.0],
            [0.0, scale],
            [0.28, 0.96],
        ] {
            let planar = UnitVector2::new(value).expect("planar direction inside the band");
            let [first, second] = planar.get();
            let xy = OrthonormalFrame3::in_xy_plane(planar);
            assert_eq!(
                bits(xy.axis().as_raw()),
                [first, second, 0.0].map(f64::to_bits)
            );
            assert_eq!(
                bits(xy.reference().as_raw()),
                [-second, first, 0.0].map(f64::to_bits)
            );
            let about_y = OrthonormalFrame3::about_y_axis(planar);
            assert_eq!(*about_y.axis(), UnitVector3::Y_AXIS);
            assert_eq!(
                bits(about_y.reference().as_raw()),
                [first, 0.0, second].map(f64::to_bits)
            );
            for frame in [xy, about_y] {
                assert_eq!(frame.axis().as_raw().dot(*frame.reference().as_raw()), 0.0);
                assert_eq!(
                    OrthonormalFrame3::new(*frame.axis().as_raw(), *frame.reference().as_raw()),
                    Some(frame)
                );
            }
        }
        assert_eq!(
            OrthonormalFrame3::new(Vector3::new(1.0, 0.0, 0.0), Vector3::new(0.0, 1.0, 0.0)),
            Some(OrthonormalFrame3::X_AXIS_Y_REFERENCE)
        );
    }

    #[test]
    fn frames_from_admitted_units_check_only_perpendicularity() {
        use super::{OrthonormalFrame3, UnitVector3};
        use crate::math::Vector3;

        assert_eq!(
            OrthonormalFrame3::from_units(UnitVector3::Z_AXIS, UnitVector3::X_AXIS),
            Some(OrthonormalFrame3::IDENTITY)
        );
        assert!(OrthonormalFrame3::from_units(UnitVector3::X_AXIS, UnitVector3::X_AXIS).is_none());
        for (tilt, admitted) in [(5.0e-10, true), (2.0e-9, false)] {
            let reference = Vector3::new(tilt, 0.0, 1.0).unit().expect("nonzero");
            let reference = UnitVector3::new(reference).expect("unit reference");
            let frame = OrthonormalFrame3::from_units(UnitVector3::X_AXIS, reference);
            assert_eq!(frame.is_some(), admitted);
            assert_eq!(
                frame,
                OrthonormalFrame3::new(Vector3::new(1.0, 0.0, 0.0), *reference.as_raw())
            );
        }
    }

    #[test]
    fn right_handed_frames_admit_the_cross_product_band_and_hold_perpendicular_directions() {
        use super::{OrthonormalFrame3, UnitVector3};
        use crate::math::Vector3;

        let unit = |x: f64, y: f64, z: f64| UnitVector3::new(Vector3::new(x, y, z)).expect("unit");
        let frame = OrthonormalFrame3::right_handed(
            UnitVector3::Z_AXIS,
            UnitVector3::X_AXIS,
            UnitVector3::Y_AXIS,
        );
        assert_eq!(frame, Some(OrthonormalFrame3::IDENTITY));
        assert!(OrthonormalFrame3::right_handed(
            UnitVector3::Z_AXIS.reversed(),
            UnitVector3::X_AXIS,
            UnitVector3::Y_AXIS,
        )
        .is_none());
        let tilted = unit(1.0, 0.0, 2.0e-12);
        assert!(
            OrthonormalFrame3::right_handed(UnitVector3::Z_AXIS, tilted, UnitVector3::Y_AXIS)
                .is_none()
        );
        // The cross product is +z exactly, but the reference and the binormal
        // are not perpendicular to 1e-12.
        let skewed = unit(2.0e-12, 1.0, 0.0);
        assert_eq!(
            UnitVector3::X_AXIS.as_raw().cross(*skewed.as_raw()),
            Vector3::new(0.0, 0.0, 1.0)
        );
        assert!(
            OrthonormalFrame3::right_handed(UnitVector3::Z_AXIS, UnitVector3::X_AXIS, skewed)
                .is_none()
        );

        for axis in [unit(0.0, 1.0e-12, 1.0), unit(1.0e-12, 1.0e-12, 1.0)] {
            let frame =
                OrthonormalFrame3::right_handed(axis, UnitVector3::X_AXIS, UnitVector3::Y_AXIS)
                    .expect("each cross-product component is within 1e-12 of the axis");
            assert_eq!(
                (*frame.axis(), *frame.reference()),
                (axis, UnitVector3::X_AXIS)
            );
            assert_eq!(
                OrthonormalFrame3::from_units(axis, UnitVector3::X_AXIS),
                Some(frame)
            );
        }
        assert!(OrthonormalFrame3::right_handed(
            unit(2.0e-12, 0.0, 1.0),
            UnitVector3::X_AXIS,
            UnitVector3::Y_AXIS,
        )
        .is_none());

        let long = 1.0 + 4.0e-13;
        let binormal = unit(0.0, long, 0.0);
        let axis = unit(0.0, 1.0e-12, long);
        let frame = OrthonormalFrame3::right_handed(axis, UnitVector3::X_AXIS, binormal)
            .expect("the cross product is within the band");
        assert!(binormal.as_raw().dot(*axis.as_raw()).abs() > 1.0e-12);
        assert!(frame.axis().as_raw().dot(*frame.reference().as_raw()).abs() <= 2.0e-12);
    }

    #[test]
    fn euclidean_right_handed_frames_admit_the_distance_band_and_hold_perpendicular_directions() {
        use super::{OrthonormalFrame3, UnitVector3};
        use crate::math::Vector3;

        let unit = |x: f64, y: f64, z: f64| UnitVector3::new(Vector3::new(x, y, z)).expect("unit");
        assert_eq!(
            OrthonormalFrame3::right_handed_euclidean(
                UnitVector3::Z_AXIS,
                UnitVector3::X_AXIS,
                UnitVector3::Y_AXIS,
            ),
            Some(OrthonormalFrame3::IDENTITY)
        );
        assert!(OrthonormalFrame3::right_handed_euclidean(
            UnitVector3::Z_AXIS.reversed(),
            UnitVector3::X_AXIS,
            UnitVector3::Y_AXIS,
        )
        .is_none());
        // The cross product is +z exactly, but the reference and the binormal
        // are not perpendicular to 1e-12.
        let skewed = unit(2.0e-12, 1.0, 0.0);
        assert!(OrthonormalFrame3::right_handed_euclidean(
            UnitVector3::Z_AXIS,
            UnitVector3::X_AXIS,
            skewed
        )
        .is_none());

        // One component 1.5e-12 from the cross product: the componentwise
        // route refuses it and the euclidean route admits it.
        let axis = unit(0.0, 1.5e-12, 1.0);
        assert!(
            OrthonormalFrame3::right_handed(axis, UnitVector3::X_AXIS, UnitVector3::Y_AXIS)
                .is_none()
        );
        let frame = OrthonormalFrame3::right_handed_euclidean(
            axis,
            UnitVector3::X_AXIS,
            UnitVector3::Y_AXIS,
        )
        .expect("the cross product is within 2e-12 of the axis");
        assert_eq!(
            (*frame.axis(), *frame.reference()),
            (axis, UnitVector3::X_AXIS)
        );
        assert_eq!(
            OrthonormalFrame3::from_units(axis, UnitVector3::X_AXIS),
            Some(frame)
        );
        // Two components 1.5e-12 from the cross product: the distance is
        // about 2.1e-12.
        assert!(OrthonormalFrame3::right_handed_euclidean(
            unit(1.5e-12, 1.5e-12, 1.0),
            UnitVector3::X_AXIS,
            UnitVector3::Y_AXIS,
        )
        .is_none());
    }

    #[test]
    fn completed_frames_take_the_unit_cross_product_of_perpendicular_directions() {
        use super::{OrthonormalFrame3, UnitVector3};
        use crate::math::Vector3;

        let unit = |x: f64, y: f64, z: f64| UnitVector3::new(Vector3::new(x, y, z)).expect("unit");
        assert_eq!(
            OrthonormalFrame3::completing(UnitVector3::X_AXIS, UnitVector3::Y_AXIS),
            Some(OrthonormalFrame3::IDENTITY)
        );
        let (cosine, sine) = (0.696_706_709_347_165_3_f64, 0.717_356_090_899_522_8_f64);
        let reference = unit(cosine, -sine, 0.0);
        let transverse = unit(sine, cosine, -0.0);
        let frame = OrthonormalFrame3::completing(reference, transverse)
            .expect("perpendicular unit directions");
        assert_eq!(*frame.reference(), reference);
        let axis = frame.axis().as_raw();
        assert!((axis.norm() - 1.0).abs() <= 4.0 * f64::EPSILON);
        assert!(axis.dot(*reference.as_raw()).abs() <= 4.0 * f64::EPSILON);
        assert!((axis.z - 1.0).abs() <= 4.0 * f64::EPSILON);
        assert_eq!(
            OrthonormalFrame3::from_units(*frame.axis(), reference),
            Some(frame)
        );

        for (skew, admitted) in [(1.0e-12, true), (2.0e-12, false)] {
            let transverse = unit(skew, 1.0, 0.0);
            assert_eq!(
                OrthonormalFrame3::completing(UnitVector3::X_AXIS, transverse).is_some(),
                admitted
            );
        }
    }

    #[test]
    fn a_direction_normalized_by_its_largest_component_keeps_the_unit_admission() {
        let divided = |value: Vector3| {
            let largest = value.x.abs().max(value.y.abs()).max(value.z.abs());
            let scaled = [value.x / largest, value.y / largest, value.z / largest];
            let length = scaled[0].hypot(scaled[1]).hypot(scaled[2]);
            Vector3::new(scaled[0] / length, scaled[1] / length, scaled[2] / length)
        };
        for value in [
            Vector3::new(3.0, 4.0, 0.0),
            Vector3::new(-2.0, 0.5, 7.0),
            Vector3::new(1.0e300, -1.0e300, 1.0e300),
            Vector3::new(1.0e-10, 3.0e-10, -2.0e-310),
            Vector3::new(f64::MAX, f64::MAX, f64::MAX),
            Vector3::new(1.0e-300, 0.0, 0.0),
        ] {
            let unit = UnitVector3::normalized_by_largest_component(value)
                .expect("finite nonzero direction");
            assert_eq!(*unit.as_raw(), divided(value));
            assert_eq!(UnitVector3::new(*unit.as_raw()), Some(unit));
        }
        for value in [
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(-0.0, 0.0, -0.0),
            Vector3::new(f64::NAN, 1.0, 0.0),
            Vector3::new(1.0, f64::INFINITY, 0.0),
        ] {
            assert_eq!(UnitVector3::normalized_by_largest_component(value), None);
        }
    }

    #[test]
    fn right_handed_frames_within_1e12_admit_half_the_euclidean_band() {
        use super::OrthonormalFrame3;

        let unit = |x: f64, y: f64, z: f64| UnitVector3::new(Vector3::new(x, y, z)).expect("unit");
        assert_eq!(
            OrthonormalFrame3::right_handed_euclidean_1e12(
                UnitVector3::Z_AXIS,
                UnitVector3::X_AXIS,
                UnitVector3::Y_AXIS,
            ),
            Some(OrthonormalFrame3::IDENTITY)
        );
        assert!(OrthonormalFrame3::right_handed_euclidean_1e12(
            UnitVector3::Z_AXIS.reversed(),
            UnitVector3::X_AXIS,
            UnitVector3::Y_AXIS,
        )
        .is_none());
        let skewed = unit(2.0e-12, 1.0, 0.0);
        assert!(OrthonormalFrame3::right_handed_euclidean_1e12(
            UnitVector3::Z_AXIS,
            UnitVector3::X_AXIS,
            skewed
        )
        .is_none());

        // One component 0.9e-12 from the cross product is inside the band.
        let near = unit(0.0, 0.9e-12, 1.0);
        let frame = OrthonormalFrame3::right_handed_euclidean_1e12(
            near,
            UnitVector3::X_AXIS,
            UnitVector3::Y_AXIS,
        )
        .expect("the cross product is within 1e-12 of the axis");
        assert_eq!(
            (*frame.axis(), *frame.reference()),
            (near, UnitVector3::X_AXIS)
        );
        assert_eq!(
            OrthonormalFrame3::from_units(near, UnitVector3::X_AXIS),
            Some(frame)
        );
        // One component 1.5e-12 from the cross product: the 2e-12 route
        // admits it and this route refuses it.
        let far = unit(0.0, 1.5e-12, 1.0);
        assert!(OrthonormalFrame3::right_handed_euclidean(
            far,
            UnitVector3::X_AXIS,
            UnitVector3::Y_AXIS
        )
        .is_some());
        assert!(OrthonormalFrame3::right_handed_euclidean_1e12(
            far,
            UnitVector3::X_AXIS,
            UnitVector3::Y_AXIS
        )
        .is_none());
    }

    #[test]
    fn frames_completed_by_the_largest_component_normalize_the_cross_product() {
        use super::OrthonormalFrame3;

        let unit = |x: f64, y: f64, z: f64| UnitVector3::new(Vector3::new(x, y, z)).expect("unit");
        assert_eq!(
            OrthonormalFrame3::completing_by_largest_component(
                UnitVector3::X_AXIS,
                UnitVector3::Y_AXIS
            ),
            Some(OrthonormalFrame3::IDENTITY)
        );
        let (cosine, sine) = (0.696_706_709_347_165_3_f64, 0.717_356_090_899_522_8_f64);
        let reference = unit(cosine, -sine, 0.0);
        let transverse = unit(sine * 0.6, cosine * 0.6, 0.8);
        let frame = OrthonormalFrame3::completing_by_largest_component(reference, transverse)
            .expect("perpendicular unit directions");
        assert_eq!(*frame.reference(), reference);
        assert_eq!(
            Some(*frame.axis()),
            UnitVector3::normalized_by_largest_component(
                reference.as_raw().cross(*transverse.as_raw())
            )
        );
        assert!(frame.axis().as_raw().dot(*reference.as_raw()).abs() <= 4.0 * f64::EPSILON);
        assert_eq!(
            OrthonormalFrame3::from_units(*frame.axis(), reference),
            Some(frame)
        );

        for (skew, admitted) in [(1.0e-12, true), (2.0e-12, false)] {
            let transverse = unit(skew, 1.0, 0.0);
            assert_eq!(
                OrthonormalFrame3::completing_by_largest_component(UnitVector3::X_AXIS, transverse)
                    .is_some(),
                admitted
            );
        }
    }

    #[test]
    fn a_frame_binormal_is_the_unit_cross_product_of_its_directions() {
        use super::OrthonormalFrame3;

        assert_eq!(OrthonormalFrame3::IDENTITY.binormal(), UnitVector3::Y_AXIS);
        let (cosine, sine) = (0.696_706_709_347_165_3_f64, 0.717_356_090_899_522_8_f64);
        let axis = UnitVector3::new(Vector3::new(sine * 0.6, cosine * 0.6, 0.8)).expect("unit");
        let reference = UnitVector3::new(Vector3::new(cosine, -sine, 0.0)).expect("unit");
        let frame = OrthonormalFrame3::from_units(axis, reference).expect("perpendicular");
        let binormal = frame.binormal();
        let cross = axis.as_raw().cross(*reference.as_raw());
        assert_eq!(
            *binormal.as_raw(),
            Vector3::new(
                cross.x / cross.norm(),
                cross.y / cross.norm(),
                cross.z / cross.norm()
            )
        );
        assert_eq!(UnitVector3::new(*binormal.as_raw()), Some(binormal));
        assert!(binormal.as_raw().dot(*axis.as_raw()).abs() <= 4.0 * f64::EPSILON);
        assert!(binormal.as_raw().dot(*reference.as_raw()).abs() <= 4.0 * f64::EPSILON);
        // A frame at the 1e-9 perpendicularity limit of from_units.
        let skewed = UnitVector3::new(Vector3::new(1.0e-9, 1.0, 0.0)).expect("unit");
        let binormal = OrthonormalFrame3::from_units(UnitVector3::X_AXIS, skewed)
            .expect("perpendicular to 1e-9")
            .binormal();
        assert_eq!(UnitVector3::new(*binormal.as_raw()), Some(binormal));
    }

    #[test]
    fn increasing_intervals_and_unit_directions_carry_into_finite_vectors() {
        use crate::features::FiniteVector3;
        use crate::topology::IncreasingParameterInterval;

        let interval = IncreasingParameterInterval::new([-2.0, 0.5]).expect("increasing");
        assert_eq!(FiniteVector::<2>::from(interval).get(), [-2.0, 0.5]);
        let direction = UnitVector3::new(Vector3::new(0.0, -0.6, 0.8)).expect("unit");
        assert_eq!(
            FiniteVector3::from(direction),
            FiniteVector3::new(Vector3::new(0.0, -0.6, 0.8)).expect("finite")
        );
    }

    #[test]
    fn horizontal_normal_frames_hold_the_reference_and_the_unit_normal_below_it() {
        use super::{OrthonormalFrame3, UnitVector3};
        use crate::math::Vector3;

        let unit = |x: f64, y: f64, z: f64| UnitVector3::new(Vector3::new(x, y, z)).expect("unit");
        let reference = unit(-0.500_000_000_000_000_2, -0.866_025_403_784_438_7, -0.0);
        let frame =
            OrthonormalFrame3::about_horizontal_normal(reference).expect("a horizontal reference");
        assert_eq!(*frame.reference(), reference);
        assert_eq!(
            *frame.axis().as_raw(),
            reference
                .as_raw()
                .cross(Vector3::new(0.0, 0.0, 1.0))
                .unit()
                .expect("nonzero cross product")
        );
        assert_eq!(
            OrthonormalFrame3::from_units(*frame.axis(), reference),
            Some(frame)
        );
        let tilted = unit(1.0, 0.0, 1.0e-9);
        let frame = OrthonormalFrame3::about_horizontal_normal(tilted).expect("nearly horizontal");
        assert_eq!(frame.axis().as_raw(), &Vector3::new(0.0, -1.0, 0.0));
        assert!(OrthonormalFrame3::about_horizontal_normal(UnitVector3::Z_AXIS).is_none());
    }

    #[test]
    fn the_document_tolerance_defaults_are_values_their_own_admission_accepts() {
        use crate::scalar::{PositiveAngle, PositiveLength};

        let Tolerances { linear, angular } = Tolerances::default();
        assert_eq!(linear.get(), 1.0e-6);
        assert_eq!(angular.get(), 1.0e-10);
        assert_eq!(PositiveLength::new(linear.get()), Some(linear));
        assert_eq!(PositiveAngle::new(angular.get()), Some(angular));
    }
}
