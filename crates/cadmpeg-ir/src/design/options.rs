// SPDX-License-Identifier: Apache-2.0
//! Option types that the operations of the feature vocabulary share.

use std::collections::BTreeMap;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::construction::{kinds, Construction};
use super::dimension::Dimension;
use super::names::CodecName;
use super::selection::{OrderedSelection, SetSelection};
use crate::scalar::{Angle, FiniteReal, Length, PositiveLength, PositiveReal};

/// Source options that Rule N leaves native, by key. A key is `codec:name`.
pub type NativeOptions = BTreeMap<CodecName, NativeOption>;

pub(crate) fn rewrite_options(
    options: NativeOptions,
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    map: &mut crate::schema::rewrite::typed::IdentityMap<'_>,
) -> Result<NativeOptions, cadmpeg_core::CodecError> {
    use crate::schema::rewrite::typed::RewriteIdentities;
    let _depth = ctx.enter_nested("native option values")?;
    options
        .into_iter()
        .map(|(key, value)| Ok((key, value.rewrite_identities(ctx, map)?)))
        .collect()
}

/// The value of a native option or a source setting. A value that a
/// parameter can drive keeps its binding.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum NativeOption {
    /// A flag.
    Bool(Dimension<bool>),
    /// An integer.
    Integer(Dimension<i64>),
    /// A real.
    Real(Dimension<FiniteReal>),
    /// A length.
    Length(Dimension<Length>),
    /// An angle.
    Angle(Dimension<Angle>),
    /// A text.
    Text(Dimension<NonBlankString>),
    /// A text the source stores with white space only, or empty.
    Blank(BlankText),
    /// A selection with no order.
    Selection(SetSelection),
    /// A selection that keeps source order.
    OrderedSelection(OrderedSelection),
}

/// A text of white space only, the empty text included.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct BlankText(String);

impl BlankText {
    /// `text`, where it holds white space only.
    #[must_use]
    pub fn new(text: String) -> Option<Self> {
        text.chars().all(char::is_whitespace).then_some(Self(text))
    }

    /// The text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for BlankText {
    type Error = &'static str;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::new(text).ok_or("a blank text holds a character other than white space")
    }
}

impl From<BlankText> for String {
    fn from(text: BlankText) -> Self {
        text.0
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for BlankText {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "BlankText".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({ "type": "string", "pattern": "^\\s*$" })
    }
}

/// The turning sense of a helix, a thread, or a tool about its axis
/// direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Hand {
    /// Turns by the right-hand rule as it advances along the axis.
    Right,
    /// Turns opposite to the right-hand rule.
    Left,
}

/// A profile: a selection, each pick one of its sources.
pub type ProfileRef = OrderedSelection;

/// A path: a selection whose picks form one connected chain, in path order.
pub type PathRef = OrderedSelection;

/// The rule by which an extrude or revolve turns its profile's closed loops
/// into regions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum LoopNesting {
    /// Loops nest alternately: a loop inside a region's outer loop bounds a
    /// hole, and a loop inside that hole bounds an island.
    Alternate,
    /// A loop inside an outer loop is a hole, and an island inside a hole
    /// makes the operation fail.
    OneLevel,
    /// Every closed loop is a region with no holes.
    Independent,
    /// Every band between consecutive nested loops is a region.
    EveryLevel,
}

/// What a profile-driven operation makes and how it changes existing bodies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SolidOutput {
    /// The form of the result. `None` means the decoder did not read it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_body"
    )]
    pub body: Option<BodyForm>,
    /// How the result combines with existing bodies. `None` means the
    /// decoder did not read it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_boolean"
    )]
    pub boolean: Option<Boolean>,
}

/// The form of an operation's result.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BodyForm {
    /// A solid.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    Solid,
    /// A solid wall about the profile curves.
    Thin(ThinWall),
    /// A surface result.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    Sheet,
}

/// A solid wall about the profile curves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ThinWall {
    /// The wall's thickness and where it lies.
    pub thickness: Thickness,
    /// How the wall ends where the profile is open.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_ends"
    )]
    pub ends: Option<ThinEnds>,
    /// The wall's corners.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_corner_round"
    )]
    pub corner_round: Option<CornerRound>,
}

/// The corners of a thin wall.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum CornerRound {
    /// Sharp corners.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Sharp,
    /// Corners rounded to the radius.
    Round(Dimension<PositiveLength>),
}

/// A wall's thickness and where it lies.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Thickness {
    /// The whole thickness on one side.
    OneSide {
        /// The thickness.
        value: Dimension<PositiveLength>,
        /// The side.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_side"
        )]
        side: Option<WallSide>,
    },
    /// The thickness split equally on both sides.
    Symmetric(Dimension<PositiveLength>),
    /// A thickness on each side.
    Offsets {
        /// The thickness outside.
        outside: Dimension<Length>,
        /// The thickness inside.
        inside: Dimension<Length>,
    },
    /// A source thickness whose meaning is not established.
    Native(NonBlankString),
}

/// The side a wall lies on.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum WallSide {
    /// Into the material that a closed profile or a face bounds; for a face
    /// offset, against the face normal.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Inside,
    /// Away from the material that a closed profile or a face bounds; for a
    /// face offset, along the face normal.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Outside,
    /// The left side of an open profile, seen looking against the profile
    /// normal.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Left,
    /// The right side of an open profile, seen looking against the profile
    /// normal.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Right,
    /// A source side rule that is not established.
    Native(NonBlankString),
}

/// How a thin wall ends where the profile is open.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ThinEnds {
    /// The ends are capped with the thickness.
    Capped(Dimension<PositiveLength>),
    /// The ends are merged.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Merged,
}

/// How an operation's result combines with existing bodies.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Boolean {
    /// A body with no combination.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    NewBody,
    /// The result combines with existing bodies.
    Apply {
        /// The combination.
        op: BooleanOp,
        /// The bodies the result combines with. `None` for a member of a
        /// `Body` container, which acts on that container's body.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_targets"
        )]
        targets: Option<Targets>,
    },
}

/// A Boolean combination.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum BooleanOp {
    /// Union.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Join,
    /// Subtraction.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Cut,
    /// Intersection.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Intersect,
    /// Adds a tool body by its own polarity. It occurs only in `Combine`.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Assemble,
    /// Sews the result into the sheets it meets. It occurs only in an
    /// output.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Sew,
    /// Joins with no material interaction. It occurs only in `Combine`.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Compound,
    /// Joins the target and the tool, then removes each piece of one operand
    /// that lies outside the other operand's material and that the other
    /// operand encloses in some plane.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    UnionTrim,
    /// A source combination whose meaning is not established.
    Native(NonBlankString),
}

/// The bodies an operation combines with.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Targets {
    /// Every body.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    All,
    /// The selected bodies.
    Selected(SetSelection),
}

/// What an operation that builds sheets makes and how it changes existing
/// bodies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SheetOutput {
    /// Whether a closed result becomes a solid.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_solid"
    )]
    pub solid: Option<Solidify>,
    /// How the result combines with existing bodies.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_boolean"
    )]
    pub boolean: Option<Boolean>,
}

/// Whether a closed sheet result becomes a solid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Solidify {
    /// The result stays a sheet.
    Sheet,
    /// The result forms a solid where it closes.
    IfClosed,
    /// The result also caps planar openings.
    CapPlanarOpenings,
}

/// What the amount of a symmetric span measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum SymmetricMeasure {
    /// The extent of each side.
    PerSide,
    /// The total extent.
    Total,
}

/// The offset of a limit from its target. A positive value moves the limit
/// further in the direction the limit is measured.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum LimitOffset {
    /// The target offset along its own normals by the distance.
    Distance(Dimension<Length>),
    /// The target moved rigidly along the span's direction by the distance.
    Translated(Dimension<Length>),
    /// The target turned by the angle, in a limit about an axis.
    Angle(Dimension<Angle>),
}

/// How a generated surface meets a loft section, a loft guide, or a patch
/// boundary.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SectionCondition {
    /// Position only.
    Free {
        /// Faces the section lies on that the condition does not use.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_support"
        )]
        support: Option<Support>,
    },
    /// The source's explicit default.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Default,
    /// The surface leaves the section along a direction.
    Direction {
        /// The direction.
        direction: Construction<kinds::Direction>,
        /// The angle from the direction.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_angle"
        )]
        angle: Option<Dimension<Angle>>,
        /// The source's tangent magnitude.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_weight"
        )]
        weight: Option<Dimension<PositiveReal>>,
    },
    /// The surface leaves normal to the section's plane.
    NormalToProfile {
        /// The angle from the normal.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_angle"
        )]
        angle: Option<Dimension<Angle>>,
        /// The source's tangent magnitude.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_weight"
        )]
        weight: Option<Dimension<PositiveReal>>,
    },
    /// G1 continuity with the support.
    Tangent {
        /// The faces the condition refers to.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_support"
        )]
        support: Option<Support>,
        /// The source's tangent magnitude.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_weight"
        )]
        weight: Option<Dimension<PositiveReal>>,
        /// Whether the tangent side is reversed.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_flip"
        )]
        flip: Option<Dimension<bool>>,
    },
    /// G2 continuity with the support.
    Curvature {
        /// The faces the condition refers to.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_support"
        )]
        support: Option<Support>,
        /// The source's tangent magnitude.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_weight"
        )]
        weight: Option<Dimension<PositiveReal>>,
        /// Whether the tangent side is reversed.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_flip"
        )]
        flip: Option<Dimension<bool>>,
    },
    /// A source condition whose meaning is not established.
    Native(NonBlankString),
}

/// The faces a section condition refers to.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Support {
    /// The selected faces.
    Faces(SetSelection),
    /// An automatically computed support.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    Computed,
}

/// How a curve reaches a face.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ProjectionDirection {
    /// Normal to the receiving surface.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    SurfaceNormal,
    /// Normal to the curve's sketch.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    SketchNormal,
    /// Along a constructed direction.
    Along(Construction<kinds::Direction>),
    /// A source direction whose meaning is not established.
    Native(NonBlankString),
}

cadmpeg_core::named_optional_field!(deserialize_angle, Dimension<Angle>, "angle");
cadmpeg_core::named_optional_field!(deserialize_body, BodyForm, "body");
cadmpeg_core::named_optional_field!(deserialize_boolean, Boolean, "boolean");
cadmpeg_core::named_optional_field!(deserialize_corner_round, CornerRound, "corner_round");
cadmpeg_core::named_optional_field!(deserialize_ends, ThinEnds, "ends");
cadmpeg_core::named_optional_field!(deserialize_flip, Dimension<bool>, "flip");
cadmpeg_core::named_optional_field!(deserialize_side, WallSide, "side");
cadmpeg_core::named_optional_field!(deserialize_solid, Solidify, "solid");
cadmpeg_core::named_optional_field!(deserialize_support, Support, "support");
cadmpeg_core::named_optional_field!(deserialize_targets, Targets, "targets");
cadmpeg_core::named_optional_field!(deserialize_weight, Dimension<PositiveReal>, "weight");
