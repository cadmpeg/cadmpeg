// SPDX-License-Identifier: Apache-2.0
//! Canonical units and tolerances.
//!
//! Stored lengths and coordinates use millimeters. Angular quantities use
//! radians.

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
    /// Borrow the direction.
    pub const fn as_raw(&self) -> &Vector3 {
        &self.0
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
    /// Return the admitted first direction. A caller that moves it into
    /// another model object keeps the unit-length guarantee and performs no
    /// new admission.
    pub const fn unit_axis(&self) -> UnitVector3 {
        self.axis
    }
    /// Return the admitted second direction. A caller that moves it into
    /// another model object keeps the unit-length guarantee and performs no
    /// new admission.
    pub const fn unit_reference(&self) -> UnitVector3 {
        self.reference
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
    #[must_use]
    pub const fn reversed_negated(self) -> Self {
        Self([-self.0[1], -self.0[0]])
    }
}

#[cfg(test)]
mod tests {
    use super::{FiniteVector, NonzeroVector, Tolerances};
    use crate::scalar::PositiveReal;
    use crate::scalar::{FiniteReal, NonNegativeReal};

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
        assert_eq!(OrthonormalFrame3::IDENTITY.unit_axis(), UnitVector3::Z_AXIS);
        assert_eq!(
            OrthonormalFrame3::IDENTITY.unit_reference(),
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
            assert_eq!(bits(xy.axis()), [first, second, 0.0].map(f64::to_bits));
            assert_eq!(
                bits(xy.reference()),
                [-second, first, 0.0].map(f64::to_bits)
            );
            let about_y = OrthonormalFrame3::about_y_axis(planar);
            assert_eq!(about_y.unit_axis(), UnitVector3::Y_AXIS);
            assert_eq!(
                bits(about_y.reference()),
                [first, 0.0, second].map(f64::to_bits)
            );
            for frame in [xy, about_y] {
                assert_eq!(frame.axis().dot(*frame.reference()), 0.0);
                assert_eq!(
                    OrthonormalFrame3::new(*frame.axis(), *frame.reference()),
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
    fn the_document_tolerance_defaults_are_values_their_own_admission_accepts() {
        use crate::scalar::{PositiveAngle, PositiveLength};

        let Tolerances { linear, angular } = Tolerances::default();
        assert_eq!(linear.get(), 1.0e-6);
        assert_eq!(angular.get(), 1.0e-10);
        assert_eq!(PositiveLength::new(linear.get()), Some(linear));
        assert_eq!(PositiveAngle::new(angular.get()), Some(angular));
    }
}
