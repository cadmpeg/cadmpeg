// SPDX-License-Identifier: Apache-2.0
//! Constructions: how a point, direction, axis, plane, frame, or curve is
//! built from references.
//!
//! Each input constrains one subject of the result by one relation to its
//! reference, with an optional value. The steps of `after` act on the result
//! after the inputs, in the source's order.

use std::collections::BTreeMap;
use std::fmt::Debug;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::dimension::Dimension;
use super::names::{CodecId, NativeNameValue};
use super::options::NativeOptions;
use super::read::{self, Ceiling};
use super::selection::OrderedSelection;
use crate::features::{ConfigurationId, FinitePoint3};
use crate::scalar::{Angle, FiniteReal, Length, PositiveLength};
use crate::transform::Transform;
use crate::units::UnitVector3;

/// The kind of element that a construction builds. Its `Value` is the
/// construction's resolved value.
pub trait ConstructionKind {
    /// The value that a construction of this kind resolves to.
    type Value: Debug + Clone + PartialEq + Serialize + DeserializeOwned;
}

/// The construction kinds: marker types, distinct from the geometry records
/// of the same names.
pub mod kinds {
    #[cfg(feature = "schema")]
    use schemars::JsonSchema;

    use super::{AxisValue, ConstructionKind, CurveValue, FrameValue, PlaneValue};
    use crate::features::FinitePoint3;
    use crate::units::UnitVector3;

    macro_rules! kinds {
        ($($(#[$meta:meta])* $name:ident => $value:ty),* $(,)?) => {$(
            $(#[$meta])*
            #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
            #[cfg_attr(feature = "schema", derive(JsonSchema))]
            pub struct $name;

            impl ConstructionKind for $name {
                type Value = $value;
            }
        )*};
    }

    kinds!(
        /// A point, which resolves to a `FinitePoint3`.
        Point => FinitePoint3,
        /// A direction, which resolves to a `UnitVector3`.
        Direction => UnitVector3,
        /// An axis, which resolves to an [`AxisValue`].
        Axis => AxisValue,
        /// A plane, which resolves to a [`PlaneValue`].
        Plane => PlaneValue,
        /// A coordinate frame, which resolves to a [`FrameValue`].
        Frame => FrameValue,
        /// A curve, which resolves to a [`CurveValue`].
        Curve => CurveValue,
    );
}

/// How an element is built from references. A construction inside an
/// operand has no identity, and its references anchor where the operand's
/// do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(
        bound = "K: JsonSchema, K::Value: JsonSchema",
        with = "ConstructionWire<K>"
    )
)]
#[serde(
    bound = "",
    rename(serialize = "ConstructionWire"),
    try_from = "ConstructionWire<K>"
)]
pub struct Construction<K: ConstructionKind> {
    /// The constraints the source stores, in the source's priority order.
    pub inputs: Vec<ConstructionInput>,
    /// The source's own steps on the result after the inputs, in the
    /// source's order.
    pub after: Vec<PostStep>,
    /// The selector the source stores for its choice among the solutions
    /// that the inputs admit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub solution: Option<u32>,
    /// The source's name for its construction method, with the method
    /// options the source stores beside it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<SourceMode>,
    /// The construction's value in its anchored state, for the active
    /// configuration, after the steps of `after`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved: Option<K::Value>,
    /// The construction's value in each other configuration for which the
    /// source stores one.
    pub resolved_in: BTreeMap<ConfigurationId, K::Value>,
}

impl<K: ConstructionKind> Default for Construction<K> {
    fn default() -> Self {
        Self {
            inputs: Vec::new(),
            after: Vec::new(),
            solution: None,
            mode: None,
            resolved: None,
            resolved_in: BTreeMap::new(),
        }
    }
}

/// The wire form of [`Construction`]. Each construction nested in its
/// inputs has passed the construction ceiling; reading admits this one
/// level above the deepest of them.
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "K: JsonSchema, K::Value: JsonSchema")
)]
#[serde(deny_unknown_fields, bound = "")]
struct ConstructionWire<K: ConstructionKind> {
    /// The constraints the source stores, in the source's priority order.
    inputs: Vec<ConstructionInput>,
    /// The source's own steps on the result after the inputs, in the
    /// source's order.
    after: Vec<PostStep>,
    /// The selector the source stores for its choice among the solutions
    /// that the inputs admit.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_solution"
    )]
    solution: Option<u32>,
    /// The source's name for its construction method, with the method
    /// options the source stores beside it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_mode"
    )]
    mode: Option<SourceMode>,
    /// The construction's value in its anchored state, for the active
    /// configuration, after the steps of `after`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_resolved"
    )]
    resolved: Option<K::Value>,
    /// The construction's value in each other configuration for which the
    /// source stores one.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    resolved_in: BTreeMap<ConfigurationId, K::Value>,
}

impl<K: ConstructionKind> TryFrom<ConstructionWire<K>> for Construction<K> {
    type Error = String;

    /// Admits a construction nested no deeper than the construction ceiling.
    fn try_from(wire: ConstructionWire<K>) -> Result<Self, Self::Error> {
        let ConstructionWire {
            inputs,
            after,
            solution,
            mode,
            resolved,
            resolved_in,
        } = wire;
        let construction = Self {
            inputs,
            after,
            solution,
            mode,
            resolved,
            resolved_in,
        };
        read::within(Ceiling::Construction, construction.depth())?;
        Ok(construction)
    }
}

impl<K: ConstructionKind> Construction<K> {
    /// The levels of this construction and the constructions nested in its
    /// inputs.
    fn depth(&self) -> u64 {
        let below = self
            .inputs
            .iter()
            .filter_map(|input| match &input.reference {
                Some(InputReference::Built(nested)) => Some(nested.depth()),
                Some(InputReference::Picks(_)) | None => None,
            })
            .max()
            .unwrap_or(0);
        below + 1
    }
}

/// One constraint of a construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ConstructionInput {
    /// The subject and the relation. `None` states a reference or value
    /// whose role the decoder did not determine.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_role"
    )]
    pub role: Option<InputRole>,
    /// The reference.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_reference"
    )]
    pub reference: Option<InputReference>,
    /// The value.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_value"
    )]
    pub value: Option<ScalarDimension>,
    /// Whether the subject points opposite to the reference's direction, or
    /// an `AtParameter` value is measured from the curve's end.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_reversed"
    )]
    pub reversed: Option<bool>,
}

/// The reference of a construction input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum InputReference {
    /// A selection.
    Picks(OrderedSelection),
    /// A nested construction. It occurs only with a relation that takes one
    /// pick.
    Built(Box<AnyConstruction>),
}

/// A construction of any kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AnyConstruction {
    /// A point construction.
    Point(Construction<kinds::Point>),
    /// A direction construction.
    Direction(Construction<kinds::Direction>),
    /// An axis construction.
    Axis(Construction<kinds::Axis>),
    /// A plane construction.
    Plane(Construction<kinds::Plane>),
    /// A frame construction.
    Frame(Construction<kinds::Frame>),
    /// A curve construction.
    Curve(Construction<kinds::Curve>),
}

impl AnyConstruction {
    fn depth(&self) -> u64 {
        match self {
            Self::Point(construction) => construction.depth(),
            Self::Direction(construction) => construction.depth(),
            Self::Axis(construction) => construction.depth(),
            Self::Plane(construction) => construction.depth(),
            Self::Frame(construction) => construction.depth(),
            Self::Curve(construction) => construction.depth(),
        }
    }
}

/// The subject and the relation of a construction input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct InputRole {
    /// The part of the result that the input constrains.
    pub subject: Subject,
    /// How the input constrains it.
    pub relation: Relation,
}

/// The part of a construction's result that an input constrains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Subject {
    /// The result itself; for a frame, the frame's XY plane.
    Whole,
    /// The origin of a plane's or frame's local frame.
    Origin,
    /// The X axis of a plane's or frame's local frame.
    XAxis,
    /// The Y axis of a plane's or frame's local frame.
    YAxis,
    /// The Z axis of a frame.
    ZAxis,
}

/// How an input constrains its subject.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Relation {
    /// Incidence: the lower-dimensional of subject and reference lies in the
    /// other.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Coincident,
    /// The subject's direction is parallel to the reference's.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Parallel,
    /// The subject's direction is perpendicular to the reference's.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Perpendicular,
    /// The subject touches the reference tangentially.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Tangent,
    /// The subject lies at a signed distance from the reference.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Offset,
    /// The subject makes an angle with the reference.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Angle,
    /// The subject lies midway between the references, or at the midpoint
    /// of one curve.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Midway,
    /// The subject contains the reference's point at the value.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    AtParameter,
    /// The subject's coordinates in the reference frame.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Coordinates,
    /// At the center of the reference.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Center,
    /// At the centroid of the references.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Centroid,
    /// Incident with the axis of a surface of revolution or circle.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    AxisOf,
    /// The reference projected onto the element the other inputs fix.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Projection,
    /// Along the reference curve's principal normal.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    PrincipalNormal,
    /// Along the reference curve's binormal.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Binormal,
    /// The subject, a circle or arc, has the radius of the value, or passes
    /// through the reference.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Radius,
    /// One end of the subject curve.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Limit,
    /// A frame on a strip of a sheet folded along two lines.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Fold,
    /// A source relation whose meaning is not established, written
    /// `codec:relation`.
    Native(NonBlankString),
}

/// The value of a construction input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ScalarDimension {
    /// A length.
    Length(Dimension<Length>),
    /// An angle.
    Angle(Dimension<Angle>),
    /// A fraction of a curve's arc length, or a ratio between references.
    Ratio(Dimension<FiniteReal>),
    /// A position on a face.
    Uv {
        /// The u parameter.
        u: Dimension<FiniteReal>,
        /// The v parameter.
        v: Dimension<FiniteReal>,
    },
    /// The coordinates of a point or origin.
    Position {
        /// The x coordinate.
        x: Dimension<Length>,
        /// The y coordinate.
        y: Dimension<Length>,
        /// The z coordinate.
        z: Dimension<Length>,
    },
    /// The components of a direction or axis.
    Components {
        /// The x component.
        x: Dimension<FiniteReal>,
        /// The y component.
        y: Dimension<FiniteReal>,
        /// The z component.
        z: Dimension<FiniteReal>,
    },
    /// The coefficients of a x + b y + c z = d, as the source binds them.
    PlaneEquation {
        /// The coefficient of x.
        a: Dimension<FiniteReal>,
        /// The coefficient of y.
        b: Dimension<FiniteReal>,
        /// The coefficient of z.
        c: Dimension<FiniteReal>,
        /// The constant term.
        d: Dimension<Length>,
    },
    /// A fraction of a curve's own parameter range.
    ParameterRatio(Dimension<FiniteReal>),
}

/// A step on a construction's result after the inputs. Each step acts in the
/// result's frame as the earlier steps leave it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum PostStep {
    /// A half turn about the frame's Y axis where the value is true.
    Flip(Dimension<bool>),
    /// A displacement along one of the frame's axes.
    Translate {
        /// The axis.
        axis: FrameAxis,
        /// The distance.
        distance: Dimension<Length>,
    },
    /// A rotation about one of the frame's axes.
    Rotate {
        /// The axis.
        axis: FrameAxis,
        /// The angle.
        angle: Dimension<Angle>,
    },
    /// A turn counterclockwise about the axis through the frame's origin
    /// with the stated components.
    AxisRotate {
        /// The axis components in the frame, not all zero.
        axis: [Dimension<FiniteReal>; 3],
        /// The angle.
        angle: Dimension<Angle>,
    },
    /// A matrix that the source stores.
    Fixed(Transform),
}

/// An axis of a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum FrameAxis {
    /// The X axis.
    X,
    /// The Y axis.
    Y,
    /// The Z axis.
    Z,
}

/// The source's name for its construction method, tagged with its codec,
/// with the method options the source stores beside it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SourceMode {
    /// The codec.
    pub codec: CodecId,
    /// The method.
    pub method: NativeNameValue,
    /// Options whose effect is not established.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Options that select how the source computes a value that the inputs
    /// and `resolved` state.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// An axis: a point on the line and its direction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct AxisValue {
    /// A point on the line.
    pub origin: FinitePoint3,
    /// The direction.
    pub direction: UnitVector3,
}

/// A plane: a point, the normal, and an in-plane x axis perpendicular to the
/// normal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PlaneValue {
    /// A point on the plane.
    pub origin: FinitePoint3,
    /// The normal.
    pub normal: UnitVector3,
    /// The in-plane x axis.
    pub x_axis: UnitVector3,
}

/// A right-handed orthonormal frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FrameValue {
    /// The origin.
    pub origin: FinitePoint3,
    /// The x axis.
    pub x_axis: UnitVector3,
    /// The y axis.
    pub y_axis: UnitVector3,
    /// The z axis.
    pub z_axis: UnitVector3,
}

/// A curve that a construction resolves to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CurveValue {
    /// A line segment.
    Segment {
        /// The start point.
        start: FinitePoint3,
        /// The end point.
        end: FinitePoint3,
    },
    /// An arc that runs counterclockwise about `normal` from `start` to
    /// `end`.
    Arc {
        /// The center.
        center: FinitePoint3,
        /// The normal.
        normal: UnitVector3,
        /// The radius.
        radius: PositiveLength,
        /// The start point.
        start: FinitePoint3,
        /// The end point.
        end: FinitePoint3,
    },
    /// A circle.
    Circle {
        /// The center.
        center: FinitePoint3,
        /// The normal.
        normal: UnitVector3,
        /// The radius.
        radius: PositiveLength,
    },
}

cadmpeg_core::named_optional_field!(deserialize_mode, SourceMode, "mode");
cadmpeg_core::named_optional_field!(deserialize_reference, InputReference, "reference");
cadmpeg_core::named_optional_field!(deserialize_resolved<V>, V, "resolved");
cadmpeg_core::named_optional_field!(deserialize_reversed, bool, "reversed");
cadmpeg_core::named_optional_field!(deserialize_role, InputRole, "role");
cadmpeg_core::named_optional_field!(deserialize_solution, u32, "solution");
cadmpeg_core::named_optional_field!(deserialize_value, ScalarDimension, "value");

#[cfg(test)]
mod tests;
