// SPDX-License-Identifier: Apache-2.0
//! Option types that the operations of the feature vocabulary share.

use std::collections::BTreeMap;

use cadmpeg_core::text::NonBlankString;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::construction::{kinds, Construction};
use super::dimension::Dimension;
use super::expr::Expression;
use super::selection::{OrderedSelection, SetSelection};
use crate::features::ParameterId;
use crate::scalar::{Angle, FiniteReal, Length, PositiveLength, PositiveReal};

/// Source options that Rule N leaves native, by key. A key is `codec:name`.
pub type NativeOptions = BTreeMap<NonBlankString, NativeOption>;

/// The value of a native option or a source setting. A value that a
/// parameter can drive keeps its binding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// A selection with no order.
    Selection(SetSelection),
    /// A selection that keeps source order.
    OrderedSelection(OrderedSelection),
}

/// A size measured on the radius or on the diameter, as the source states
/// it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound(
        serialize = "Q: Serialize, P: Serialize",
        deserialize = "Q: Deserialize<'de>, P: Deserialize<'de>"
    )
)]
pub enum RadialDimension<Q, P = ParameterId> {
    /// The size on the radius.
    Radius(Dimension<Q, P>),
    /// The size on the diameter.
    Diameter(Dimension<Q, P>),
}

/// The turning sense of a helix, a thread, or a tool about its axis
/// direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
        deserialize_with = "super::wire::present"
    )]
    pub body: Option<BodyForm>,
    /// How the result combines with existing bodies. `None` means the
    /// decoder did not read it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::wire::present"
    )]
    pub boolean: Option<Boolean>,
}

/// The form of an operation's result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BodyForm {
    /// A solid.
    Solid,
    /// A solid wall about the profile curves.
    Thin(ThinWall),
    /// A surface result.
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
        deserialize_with = "super::wire::present"
    )]
    pub ends: Option<ThinEnds>,
    /// The wall's corners.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::wire::present"
    )]
    pub corner_round: Option<CornerRound>,
}

/// The corners of a thin wall.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum CornerRound {
    /// Sharp corners.
    Sharp,
    /// Corners rounded to the radius.
    Round(Dimension<PositiveLength>),
}

/// A wall's thickness and where it lies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
            deserialize_with = "super::wire::present"
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    Inside,
    /// Away from the material that a closed profile or a face bounds; for a
    /// face offset, along the face normal.
    Outside,
    /// The left side of an open profile, seen looking against the profile
    /// normal.
    Left,
    /// The right side of an open profile, seen looking against the profile
    /// normal.
    Right,
    /// A source side rule that is not established.
    Native(NonBlankString),
}

/// How a thin wall ends where the profile is open.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    Merged,
}

/// How an operation's result combines with existing bodies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Boolean {
    /// A body with no combination.
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
            deserialize_with = "super::wire::present"
        )]
        targets: Option<Targets>,
    },
}

/// A Boolean combination.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum BooleanOp {
    /// Union.
    Join,
    /// Subtraction.
    Cut,
    /// Intersection.
    Intersect,
    /// Adds a tool body by its own polarity. It occurs only in `Combine`.
    Assemble,
    /// Sews the result into the sheets it meets. It occurs only in an
    /// output.
    Sew,
    /// Joins with no material interaction. It occurs only in `Combine`.
    Compound,
    /// A union that trims.
    UnionTrim,
    /// A source combination whose meaning is not established.
    Native(NonBlankString),
}

/// The bodies an operation combines with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Targets {
    /// Every body.
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
        deserialize_with = "super::wire::present"
    )]
    pub solid: Option<Solidify>,
    /// How the result combines with existing bodies.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::wire::present"
    )]
    pub boolean: Option<Boolean>,
}

/// Whether a closed sheet result becomes a solid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

/// Material placed along a direction or about an axis, measured from the
/// operation's origin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound(serialize = "Q: Serialize", deserialize = "Q: Deserialize<'de>")
)]
pub enum Span<Q> {
    /// From `start` to `end`. `end` is measured along the direction and
    /// `start` against it.
    Between {
        /// The limit against the direction.
        start: Box<Limit<Q>>,
        /// The limit along the direction.
        end: Box<Limit<Q>>,
    },
    /// Both sides of the origin equally.
    Symmetric {
        /// The amount.
        amount: Dimension<Q>,
        /// Whether `amount` is the extent of each side or the total.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::wire::present"
        )]
        measure: Option<SymmetricMeasure>,
    },
}

/// What the amount of a symmetric span measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum SymmetricMeasure {
    /// The extent of each side.
    PerSide,
    /// The total extent.
    Total,
}

/// One end of a span.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound(serialize = "Q: Serialize", deserialize = "Q: Deserialize<'de>")
)]
pub enum Limit<Q> {
    /// At the operation's origin.
    AtOrigin,
    /// A signed distance or angle from the origin.
    Amount(Dimension<Q>),
    /// Through every target body; in a revolve, a full turn.
    ThroughAll,
    /// At the first face the swept profile meets.
    Next,
    /// At the last face the swept profile meets.
    Last,
    /// At faces, surfaces, or bodies.
    ToGeometry {
        /// The faces, surfaces, or bodies.
        target: SetSelection,
        /// The offset of the limit from the target.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::wire::present"
        )]
        offset: Option<LimitOffset>,
        /// Whether the target faces extend until the profile meets them.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::wire::present"
        )]
        extend: Option<Dimension<bool>>,
        /// Where `target` picks bodies, whether the limit is the far side of
        /// the body the profile meets.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::wire::present"
        )]
        far: Option<Dimension<bool>>,
    },
    /// At a plane.
    ToPlane(Construction<kinds::Plane>),
    /// Where the swept profile reaches a point.
    ToPoint(Construction<kinds::Point>),
    /// A source limit kind whose meaning is not established.
    Native(NonBlankString),
}

/// The offset of a limit from its target. A positive value moves the limit
/// further in the direction the limit is measured.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

/// A function of a quantity along a path parameter from 0 to 1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound(serialize = "Q: Serialize", deserialize = "Q: Deserialize<'de>")
)]
pub enum Law<Q> {
    /// A constant.
    Constant(Dimension<Q>),
    /// A linear ramp.
    Linear {
        /// The value at the path's start.
        start: Dimension<Q>,
        /// The value at the path's end.
        end: Dimension<Q>,
    },
    /// A law curve that the source selects.
    Curve(OrderedSelection),
    /// An expression in the law parameter whose result has the quantity.
    Expression(Expression),
    /// A source law whose meaning is not established.
    Native(NonBlankString),
}

/// How a generated surface meets a loft section, a loft guide, or a patch
/// boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
            deserialize_with = "super::wire::present"
        )]
        support: Option<Support>,
    },
    /// The source's explicit default.
    Default,
    /// The surface leaves the section along a direction.
    Direction {
        /// The direction.
        direction: Construction<kinds::Direction>,
        /// The angle from the direction.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::wire::present"
        )]
        angle: Option<Dimension<Angle>>,
        /// The source's tangent magnitude.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::wire::present"
        )]
        weight: Option<Dimension<PositiveReal>>,
    },
    /// The surface leaves normal to the section's plane.
    NormalToProfile {
        /// The angle from the normal.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::wire::present"
        )]
        angle: Option<Dimension<Angle>>,
        /// The source's tangent magnitude.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::wire::present"
        )]
        weight: Option<Dimension<PositiveReal>>,
    },
    /// G1 continuity with the support.
    Tangent {
        /// The faces the condition refers to.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::wire::present"
        )]
        support: Option<Support>,
        /// The source's tangent magnitude.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::wire::present"
        )]
        weight: Option<Dimension<PositiveReal>>,
        /// Whether the tangent side is reversed.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::wire::present"
        )]
        flip: Option<Dimension<bool>>,
    },
    /// G2 continuity with the support.
    Curvature {
        /// The faces the condition refers to.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::wire::present"
        )]
        support: Option<Support>,
        /// The source's tangent magnitude.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::wire::present"
        )]
        weight: Option<Dimension<PositiveReal>>,
        /// Whether the tangent side is reversed.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::wire::present"
        )]
        flip: Option<Dimension<bool>>,
    },
    /// A source condition whose meaning is not established.
    Native(NonBlankString),
}

/// The faces a section condition refers to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Support {
    /// The selected faces.
    Faces(SetSelection),
    /// An automatically computed support.
    Computed,
}

/// How a curve reaches a face.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ProjectionDirection {
    /// Normal to the receiving surface.
    SurfaceNormal,
    /// Normal to the curve's sketch.
    SketchNormal,
    /// Along a constructed direction.
    Along(Construction<kinds::Direction>),
    /// A source direction whose meaning is not established.
    Native(NonBlankString),
}
