// SPDX-License-Identifier: Apache-2.0
//! Heights, radial limits, and axial limits of an operation (§7.3).
//!
//! A height is a level along the tool axis of the operation's frame. A
//! radial limit bounds a turning operation about the spindle axis, and an
//! axial limit bounds it along the spindle axis. Each is a reference plus an
//! offset, and the reference is a [`Choice`] so that the source's mode
//! parameter binds it.

use std::fmt;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{Choice, ProcessDimension};
use crate::design::dimension::ParamRef;
use crate::design::ops::shared::RadialDimension;
use crate::scalar::Length;

/// The role of an operation's height.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum HeightRole {
    /// The level the tool moves at in rapid between operations and above
    /// every obstacle.
    Clearance,
    /// The level the tool rapids to between passes and from which it moves
    /// down to start a pass.
    Retract,
    /// The level at which the tool changes from rapid to cutting feed as it
    /// approaches the material, which a drilling cycle also retracts to
    /// between pecks.
    Feed,
    /// The top of the material the operation machines.
    Top,
    /// The operation's final depth.
    Bottom,
}

/// The role's wire name, as a key of `Operation.heights`.
impl fmt::Display for HeightRole {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Clearance => "clearance",
            Self::Retract => "retract",
            Self::Feed => "feed",
            Self::Top => "top",
            Self::Bottom => "bottom",
        })
    }
}

/// A height: a reference level plus an offset, along the operation's own
/// tool axis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Height {
    /// The reference level.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_from"
    )]
    pub from: Option<Choice<HeightFrom>>,
    /// The offset from the reference level.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_offset"
    )]
    pub offset: Option<ProcessDimension<Length>>,
}

/// The role of a turning operation's radial limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RadiusRole {
    /// The outermost radius the cut reaches.
    Outer,
    /// The innermost radius the cut reaches.
    Inner,
    /// The radius the tool's rapids keep outside of.
    OuterClearance,
    /// The radius the tool's rapids keep inside of.
    InnerClearance,
    /// The radius a lead-out rises to.
    OuterRetract,
}

/// The role's wire name, as a key of `Operation.radii`.
impl fmt::Display for RadiusRole {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Outer => "outer",
            Self::Inner => "inner",
            Self::OuterClearance => "outer_clearance",
            Self::InnerClearance => "inner_clearance",
            Self::OuterRetract => "outer_retract",
        })
    }
}

/// A radial limit of a turning operation.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum RadialLimit {
    /// The limit's own size about the axis, on the radius or on the diameter.
    Absolute(RadialDimension<Length, ParamRef>),
    /// A base plus an offset.
    From {
        /// The base.
        base: RadiusFrom,
        /// The offset from the base.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_offset"
        )]
        offset: Option<ProcessDimension<Length>>,
    },
}

/// The base of a radial limit.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum RadiusFrom {
    /// The stock's outer radius.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    StockOuter,
    /// The stock's inner radius; the spindle axis for a solid stock.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    StockInner,
    /// The least radius of the `Reference` geometry whose slot it names.
    SelectionMin(NonBlankString),
    /// The greatest radius of the `Reference` geometry whose slot it names.
    SelectionMax(NonBlankString),
    /// One cut at each point of the `Reference` geometry whose slot it names,
    /// the insert on the side of the point that `side` states.
    Positions {
        /// The slot of the points' geometry.
        slot: NonBlankString,
        /// The side of each point the insert lies on.
        side: Choice<AxialRole>,
    },
    /// The limit of another role of the same operation.
    Limit(RadiusRole),
    /// A base that no arm states, by the source's mode.
    Native(NonBlankString),
}

/// The role of a turning operation's axial limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AxialRole {
    /// The limit at the front.
    Front,
    /// The limit at the back.
    Back,
}

/// The role's wire name, as a key of `Operation.axial`.
impl fmt::Display for AxialRole {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Front => "front",
            Self::Back => "back",
        })
    }
}

/// An axial limit of a turning operation: a base plus an offset along the
/// spindle axis, positive toward the front.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct AxialLimit {
    /// The base.
    pub base: Choice<AxialFrom>,
    /// The offset from the base, positive toward the front.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_offset"
    )]
    pub offset: Option<ProcessDimension<Length>>,
    /// The point of the tool that ends on the limit. `None` does not state
    /// it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_point"
    )]
    pub point: Option<Choice<LimitPoint>>,
}

/// The base of an axial limit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum AxialFrom {
    /// The frontmost point along the axis of the model's solids turned in
    /// the setup.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    ModelFront,
    /// The backmost point along the axis of the model's solids turned in the
    /// setup.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    ModelBack,
    /// The stock's front face.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    StockFront,
    /// A base that no arm states, by the source's mode.
    Native(NonBlankString),
}

/// The point of the tool that ends on an axial limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum LimitPoint {
    /// The tool's contact with the cut surface.
    ContactPoint,
    /// The point beyond which no point of the tool passes.
    CuttingEdge,
}

/// The reference level of a height.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum HeightFrom {
    /// The work coordinate system's origin: an absolute height.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Wcs,
    /// The stock's top.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    StockTop,
    /// The stock's bottom.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    StockBottom,
    /// The highest point of the setup's model along the tool axis.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    ModelTop,
    /// The lowest point of the setup's model along the tool axis.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    ModelBottom,
    /// Another height of the same operation.
    Height(HeightRole),
    /// The `Reference` geometry of the same operation whose slot it names.
    Selection(NonBlankString),
    /// The operation's `Machine` geometry.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Geometry,
    /// A reference that no arm states, by the source's mode.
    Native(NonBlankString),
}

cadmpeg_core::named_optional_field!(deserialize_from, Choice<HeightFrom>, "from");
cadmpeg_core::named_optional_field!(deserialize_offset, ProcessDimension<Length>, "offset");
cadmpeg_core::named_optional_field!(deserialize_point, Choice<LimitPoint>, "point");
