// SPDX-License-Identifier: Apache-2.0
//! Stock rules and the work coordinate system (§7.2).
//!
//! A source states stock and WCS as a rule over the model, as a plain
//! placement, or both. The stock solid is the process state `Stock(setup)`,
//! not part of the rule.

use cadmpeg_core::text::NonBlankString;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{Choice, ProcessConstruction, ProcessDimension};
use crate::design::construction::{kinds, FrameAxis};
use crate::design::ids::SetupId;
use crate::design::selection::SetSelection;
use crate::scalar::Length;
use crate::transform::Transform;

/// The rule that states a setup's stock.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum StockRule {
    /// A box around the model bounds, grown by six offsets.
    Relative(BoxOffsets),
    /// A cylinder about an axis, as long as the model's extent along it and
    /// as wide as the smallest such cylinder that holds the model, grown by
    /// the offsets.
    RelativeCylinder(CylinderOffsets),
    /// A box, cylinder, or tube of stated size, aligned per axis to the model
    /// bounds or placed by a transform.
    Fixed {
        /// The stock's shape and size.
        shape: StockShape,
        /// The stock's place.
        placement: StockPlacement,
    },
    /// A selected design body.
    Solid(SetSelection),
    /// The result of a preceding setup.
    PreviousSetup(SetupId),
    /// A rule that no arm states, by the source's mode name.
    Native(NonBlankString),
}

/// The offsets of a relative box stock, per axis of the setup: `low` at the
/// −x, −y, −z sides and `high` at the +x, +y, +z sides.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BoxOffsets {
    /// The offsets at the low sides.
    pub low: [Option<ProcessDimension<Length>>; 3],
    /// The offsets at the high sides.
    pub high: [Option<ProcessDimension<Length>>; 3],
}

/// The offsets of a relative cylinder stock.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CylinderOffsets {
    /// The cylinder's axis. `None` is the setup's z axis through the centre
    /// of the model's bounds.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_axis"
    )]
    pub axis: Option<ProcessConstruction<kinds::Axis>>,
    /// The growth of the radius.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_radial"
    )]
    pub radial: Option<ProcessDimension<Length>>,
    /// The growth at the +z end.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_front"
    )]
    pub front: Option<ProcessDimension<Length>>,
    /// The growth at the −z end.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_back"
    )]
    pub back: Option<ProcessDimension<Length>>,
}

/// The shape and size of a fixed stock.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StockShape {
    /// A box.
    Box {
        /// The box's size along x, y, and z.
        size: [ProcessDimension<Length>; 3],
    },
    /// A cylinder.
    Cylinder {
        /// The cylinder's diameter.
        diameter: ProcessDimension<Length>,
        /// The cylinder's length.
        length: ProcessDimension<Length>,
    },
    /// A tube.
    Tube {
        /// The tube's outer diameter.
        outer: ProcessDimension<Length>,
        /// The tube's inner diameter.
        inner: ProcessDimension<Length>,
        /// The tube's length.
        length: ProcessDimension<Length>,
    },
}

/// The place of a fixed stock.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum StockPlacement {
    /// Aligned to the model bounds along x, y, and z.
    Aligned([BoxAlignment; 3]),
    /// Placed by a transform.
    Transform(Transform),
}

/// The alignment of a fixed stock to the model bounds along one axis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BoxAlignment {
    /// The side of the model bounds the stock aligns to.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_side"
    )]
    pub side: Option<BoxSide>,
    /// The offset from that side.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_offset"
    )]
    pub offset: Option<ProcessDimension<Length>>,
}

/// A side of a box along one axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum BoxSide {
    /// The low side.
    Low,
    /// The centre.
    Center,
    /// The high side.
    High,
}

/// A work coordinate system: an orientation rule, an origin rule, and the
/// resulting frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Wcs {
    /// The orientation rule.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_orientation"
    )]
    pub orientation: Option<Choice<WcsOrientation>>,
    /// The origin rule.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_origin"
    )]
    pub origin: Option<Choice<WcsOrigin>>,
    /// The WCS frame in setup space: the rules' cache where rules exist, and
    /// the only statement where the source stores a frame without a rule.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_placement"
    )]
    pub placement: Option<Transform>,
}

/// The orientation rule of a work coordinate system.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum WcsOrientation {
    /// The model's orientation.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Model,
    /// Two axes, each set along a direction, with a flip per axis.
    Axes {
        /// The first axis set.
        primary: Box<AxisSpec>,
        /// The second axis set.
        secondary: Box<AxisSpec>,
        /// The flips of the x, y, and z axes.
        flips: [Option<ProcessDimension<bool>>; 3],
    },
    /// A coordinate frame.
    Frame(ProcessConstruction<kinds::Frame>),
    /// A rule that no arm states, by the source's mode name.
    Native(NonBlankString),
}

/// One axis of a work coordinate system set along a direction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct AxisSpec {
    /// The axis that is set.
    pub axis: FrameAxis,
    /// The direction the axis takes.
    pub direction: ProcessConstruction<kinds::Direction>,
}

/// The origin rule of a work coordinate system.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum WcsOrigin {
    /// The model's origin.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    ModelOrigin,
    /// The origin of the `Frame` orientation.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    FrameOrigin,
    /// A point of the stock's bounding box.
    StockPoint(BoxPoint),
    /// A point of the model's bounding box.
    ModelPoint(BoxPoint),
    /// A constructed point.
    Point(ProcessConstruction<kinds::Point>),
    /// A rule that no arm states, by the source's mode name.
    Native(NonBlankString),
}

/// A point of a box, by its side along each axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BoxPoint {
    /// The side along x.
    pub x: BoxSide,
    /// The side along y.
    pub y: BoxSide,
    /// The side along z.
    pub z: BoxSide,
}

cadmpeg_core::named_optional_field!(deserialize_axis, ProcessConstruction<kinds::Axis>, "axis");
cadmpeg_core::named_optional_field!(deserialize_back, ProcessDimension<Length>, "back");
cadmpeg_core::named_optional_field!(deserialize_front, ProcessDimension<Length>, "front");
cadmpeg_core::named_optional_field!(deserialize_offset, ProcessDimension<Length>, "offset");
cadmpeg_core::named_optional_field!(
    deserialize_orientation,
    Choice<WcsOrientation>,
    "orientation"
);
cadmpeg_core::named_optional_field!(deserialize_origin, Choice<WcsOrigin>, "origin");
cadmpeg_core::named_optional_field!(deserialize_placement, Transform, "placement");
cadmpeg_core::named_optional_field!(deserialize_radial, ProcessDimension<Length>, "radial");
cadmpeg_core::named_optional_field!(deserialize_side, BoxSide, "side");
