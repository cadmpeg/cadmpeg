// SPDX-License-Identifier: Apache-2.0
//! Swept solids and the loft.
//!
//! Extrude, revolve, sweep, helical sweep, rib, primitive, and loft build a
//! body from a profile or a stated shape. Each carries a [`SolidOutput`].

use std::num::NonZeroU32;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::design::construction::{kinds, Construction};
use crate::design::dimension::Dimension;
use crate::design::ops::shared::{
    Interpolation, Law, Limit, OffsetJoin, RadialDimension, SheetCutNormal, Span,
};
use crate::design::options::{
    Boolean, Hand, LoopNesting, NativeOptions, PathRef, ProfileRef, SectionCondition, SolidOutput,
    Thickness,
};
use crate::design::selection::{OrderedSelection, SetSelection};
use crate::design::units::PositiveArea;
use crate::scalar::{
    Angle, FiniteReal, Fraction, Length, NonNegativeLength, NonZeroLength, PositiveLength,
    PositiveReal,
};

/// An extrude: a profile swept along a direction over a span measured from
/// an origin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Extrude {
    /// The profile.
    pub profile: ProfileRef,
    /// How the profile's loops nest into regions.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_loops"
    )]
    pub loops: Option<LoopNesting>,
    /// The direction the profile is swept along.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_direction"
    )]
    pub direction: Option<ExtrudeDirection>,
    /// The plane the span is measured from. `None` is not stated; a writer
    /// uses the profile plane.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_origin"
    )]
    pub origin: Option<ExtrudeOrigin>,
    /// The span, measured from the origin along the direction.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_span"
    )]
    pub span: Option<Span<Length>>,
    /// The taper of the swept material. A draft the source stores as off is
    /// `None`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_draft"
    )]
    pub draft: Option<ExtrudeDraft>,
    /// The offset of the profile in its plane before the operation uses it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_fit"
    )]
    pub fit: Option<ProfileFit>,
    /// `Some(true)` sweeps the part of the profile plane outside the
    /// profile's regions, so a cut removes the material outside the profile
    /// within the span. `None` and `Some(false)` sweep the regions.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_outside"
    )]
    pub outside: Option<Dimension<bool>>,
    /// How a cut of a sheet-metal wall is made.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_sheet_cut"
    )]
    pub sheet_cut: Option<SheetCutNormal>,
    /// What the extrude makes and how it changes existing bodies.
    pub output: SolidOutput,
    /// Source options whose effect is not established.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options that select how the source computes the result.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// The direction an extrude sweeps its profile along.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExtrudeDirection {
    /// Along the normal of the profile plane.
    ProfileNormal {
        /// The geometric sense: true where the material lies against the
        /// profile normal.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_reversed"
        )]
        reversed: Option<Dimension<bool>>,
    },
    /// Along a constructed direction. Its reversal is a `Flip` step of the
    /// construction.
    Along {
        /// The direction.
        direction: Box<Construction<kinds::Direction>>,
        /// Whether `Amount` limits are measured along the profile normal.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_lengths_along_normal"
        )]
        lengths_along_normal: Option<Dimension<bool>>,
    },
}

/// The plane an extrude's span is measured from. Editing an origin offset
/// moves both ends of the span.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ExtrudeOrigin {
    /// The profile plane.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    ProfilePlane,
    /// A plane offset from the profile plane along the direction.
    Offset(Dimension<Length>),
    /// A constructed plane.
    Plane(Construction<kinds::Plane>),
    /// The plane through a point normal to the direction.
    Point(Construction<kinds::Point>),
    /// Faces, with an offset.
    Faces {
        /// The faces.
        faces: SetSelection,
        /// The offset from the faces.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_offset"
        )]
        offset: Option<Dimension<Length>>,
    },
}

/// The taper of an extrude. A positive angle narrows the profile's outer
/// loop away from the origin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ExtrudeDraft {
    /// The taper of the material from the origin to the span's end. On a
    /// symmetric span it tapers both sides.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_end"
    )]
    pub end: Option<Dimension<Angle>>,
    /// The taper of the material from the origin to the span's start. A
    /// symmetric span has none.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_start"
    )]
    pub start: Option<Dimension<Angle>>,
    /// The plane at which the cross-section equals the profile. `None` does
    /// not state it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_neutral"
    )]
    pub neutral: Option<DraftNeutral>,
    /// How the inner loops of a tapered profile's regions taper. `None`
    /// states nothing.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_holes"
    )]
    pub holes: Option<HoleDraft>,
}

/// The plane at which a drafted extrude's cross-section equals its profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DraftNeutral {
    /// At the extrude's origin; the tapers run from there.
    Origin,
    /// At the start limit: one taper runs from the start limit to the end
    /// limit, by the `end` angle, and `start` is absent.
    Start,
}

/// How the inner loops of a tapered profile's regions taper.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HoleDraft {
    /// Every boundary of a region moves by the same amount relative to its
    /// material, so a hole narrows where the outer loop widens.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    WithRegion,
    /// Each inner loop tapers as the outer loop is tapered, in its own
    /// outward sense, so a hole widens where the outer loop widens.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    WithOuter,
    /// Each inner loop drafts as a loop of its own at its own angle. A
    /// positive angle widens the hole away from the origin, and 0 leaves it
    /// straight.
    Angles {
        /// The angle toward the span's end.
        end: Dimension<Angle>,
        /// The angle toward the span's start.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_start"
        )]
        start: Option<Dimension<Angle>>,
    },
}

/// An offset of a profile in its plane before the operation uses it. A
/// positive value moves a loop outward from itself, so a region grows and its
/// holes grow. An offset of 0 states no fit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ProfileFit {
    /// The offset of each region's outer loop.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_outer"
    )]
    pub outer: Option<Dimension<Length>>,
    /// The offset of each inner loop.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_inner"
    )]
    pub inner: Option<Dimension<Length>>,
    /// The law at the outer loops' convex corners.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_outer_join"
    )]
    pub outer_join: Option<OffsetJoin>,
    /// The law at the inner loops' convex corners.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_inner_join"
    )]
    pub inner_join: Option<OffsetJoin>,
}

/// A revolve: a profile turned about an axis over an angular span. The
/// origin of the span is the profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Revolve {
    /// The profile.
    pub profile: ProfileRef,
    /// How the profile's loops nest into regions.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_loops"
    )]
    pub loops: Option<LoopNesting>,
    /// The axis. Its direction, after the construction's `Flip` steps, fixes
    /// the positive rotation by the right-hand rule.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_axis"
    )]
    pub axis: Option<Construction<kinds::Axis>>,
    /// The span, measured from the profile about the axis.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_span"
    )]
    pub span: Option<Span<Angle>>,
    /// The offset of the profile in its plane before the operation uses it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_fit"
    )]
    pub fit: Option<ProfileFit>,
    /// What the revolve makes and how it changes existing bodies.
    pub output: SolidOutput,
    /// Source options whose effect is not established.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options that select how the source computes the result.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// A sweep: one section moved along one path. Several sections along a path
/// are a loft with a spine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Sweep {
    /// The section.
    pub section: SweepSection,
    /// The path.
    pub path: PathRef,
    /// How the section is oriented, twisted, and scaled along the path.
    pub controls: SweepControls,
    /// Curves the section follows besides the path.
    pub guides: Vec<SweepGuide>,
    /// The fractions of the path swept on either side of the section.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_extent"
    )]
    pub extent: Option<PathExtent>,
    /// Where the profile is swept from.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_placement"
    )]
    pub placement: Option<ProfilePlacement>,
    /// Relimiting elements on the path.
    pub limits: SpineLimits,
    /// How the swept result passes a sharp corner of its path.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_transition"
    )]
    pub transition: Option<CornerTransition>,
    /// What the sweep makes and how it changes existing bodies.
    pub output: SolidOutput,
    /// Source options whose effect is not established.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options that select how the source computes the result.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// How a swept result passes a sharp corner of its path, where the path's
/// tangent turns.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum CornerTransition {
    /// The pieces swept along the two adjacent segments extend until they
    /// meet, and their intersection is the joint.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Mitre,
    /// The section turns about the axis through the corner point normal to
    /// both adjacent tangents, so a rounded piece joins the two.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Round,
    /// The pieces swept along the two adjacent segments stay as they are,
    /// with no surface joining them at the corner.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Open,
    /// No joint piece: the section crosses the corner without turning, each
    /// later segment's frame law premultiplied by the constant rotation that
    /// carries the frame at that segment's start onto the frame at the
    /// previous segment's end.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Carried,
    /// A source transition whose meaning is not established.
    Native(NonBlankString),
}

/// The section of a sweep.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SweepSection {
    /// A profile.
    Profile(ProfileRef),
    /// A shape the source states by size.
    Generated {
        /// The shape.
        shape: SectionShape,
        /// How a generated square or triangle turns about its path. `None`
        /// for a circle.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_turn"
        )]
        turn: Option<SectionTurn>,
    },
}

/// How a sweep's generated square or triangle turns about its path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum SectionTurn {
    /// The path lies in a planar sketch: the section is turned to the
    /// sketch's plane.
    PathPlane,
    /// Where the sweep starts (an open path's start; on a closed path the
    /// middle of its first curve) one side of a square section lies along the
    /// z axis of the definition's frame projected into the plane normal to
    /// the path. From there the section is carried with no rotation about the
    /// path's tangent.
    ZStart,
}

/// A section shape stated by size.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SectionShape {
    /// A circle.
    Circle {
        /// The outside diameter.
        diameter: Dimension<PositiveLength>,
        /// The inner diameter of a hollow section.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_inner_diameter"
        )]
        inner_diameter: Option<Dimension<PositiveLength>>,
    },
    /// A square.
    Square {
        /// The side.
        size: Dimension<PositiveLength>,
    },
    /// An equilateral triangle, which lies in the plane of the helix axis
    /// with one side parallel to that axis.
    Triangle {
        /// The altitude; the side is 2·size/√3.
        size: Dimension<PositiveLength>,
        /// Which way the vertex opposite the side parallel to the axis
        /// points. Only a coil's helix axis gives it a meaning.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_apex"
        )]
        apex: Option<Apex>,
    },
    /// A source section whose meaning is not established.
    Native(NonBlankString),
}

/// Which way a triangle section's apex points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Apex {
    /// Away from the axis.
    Outward,
    /// Toward the axis.
    Inward,
}

/// How a section is oriented, twisted, and scaled along a path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SweepControls {
    /// The law that orients the section along the path.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_orientation"
    )]
    pub orientation: Option<SweepOrientation>,
    /// The twist of the section along the path.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_twist"
    )]
    pub twist: Option<Twist>,
    /// The scaling of the section along the path.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_scaling"
    )]
    pub scaling: Option<ScalingLaw>,
}

/// The law that orients a section along a path.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SweepOrientation {
    /// Keeps the section's angle to the path tangent.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    FollowPath,
    /// A law the source documents as twist-minimizing.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    MinimumTwist,
    /// Follows the Frenet frame of the path.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Frenet,
    /// Keeps the section's orientation.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Fixed,
    /// Twists the intermediate sections with the guides: the angle between
    /// the section's in-plane x axis and a guide vector stays constant along
    /// the path.
    Guides(GuideFrame),
    /// Holds one section axis along a direction.
    Direction(Construction<kinds::Direction>),
    /// Holds one section axis normal to faces.
    Surface(SetSelection),
    /// Turns toward a point.
    Point(Construction<kinds::Point>),
    /// Follows an auxiliary curve.
    Curve {
        /// The auxiliary curve.
        path: PathRef,
        /// The auxiliary curve's tangent flag.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_tangent"
        )]
        tangent: Option<Dimension<bool>>,
        /// The auxiliary curve's curvilinear flag.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_curvilinear"
        )]
        curvilinear: Option<Dimension<bool>>,
    },
    /// A source orientation whose meaning is not established.
    Native(NonBlankString),
}

/// The guide vector that `SweepOrientation::Guides` holds at a constant
/// angle, in each section's plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum GuideFrame {
    /// From the path to the first guide.
    PathToFirst,
    /// From the first guide to the second.
    FirstToSecond,
}

/// The twist of a section along a path.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Twist {
    /// The total twist over the path.
    Angle(Dimension<Angle>),
    /// An angle law along the path.
    Law(Law<Angle>),
}

/// The scaling of a section along a path.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ScalingLaw {
    /// A draft angle of the section along the path.
    Taper(Dimension<Angle>),
    /// A scale law along the path.
    Law(Law<FiniteReal>),
    /// Scaling that follows an area.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Area,
    /// Scaling that follows a perimeter.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Perimeter,
    /// Scaling toward a point.
    Point(Construction<kinds::Point>),
    /// Scaling that follows a curve.
    Curve(PathRef),
    /// A source scaling whose meaning is not established.
    Native(NonBlankString),
}

/// A curve a sweep's section follows besides the path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SweepGuide {
    /// The guide curve.
    pub path: PathRef,
    /// The fractions of the guide swept on either side of the section.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_extent"
    )]
    pub extent: Option<PathExtent>,
}

/// The fractions of a path or guide swept on either side of the section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PathExtent {
    /// The fraction along the path.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_along"
    )]
    pub along: Option<Dimension<Fraction>>,
    /// The fraction against the path.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_against"
    )]
    pub against: Option<Dimension<Fraction>>,
}

/// Where a sweep's profile is swept from.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProfilePlacement {
    /// Where the profile lies.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    InPlace,
    /// Moved onto the path start.
    Positioned(Box<ProfilePosition>),
}

/// How a positioned profile is moved onto the path start: `origin` lies on
/// the path start and `direction` aligns with the first axis of the moving
/// frame there, then the profile turns by `angle`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ProfilePosition {
    /// The profile point placed on the path start.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_origin"
    )]
    pub origin: Option<Construction<kinds::Point>>,
    /// The profile direction aligned with the first axis of the moving frame.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_direction"
    )]
    pub direction: Option<Construction<kinds::Direction>>,
    /// The rotation after the alignment.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_angle"
    )]
    pub angle: Option<Dimension<Angle>>,
    /// Whether the frame axes are swapped.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_swap_axes"
    )]
    pub swap_axes: Option<Dimension<bool>>,
}

/// Relimiting elements on a path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SpineLimits {
    /// The element at the start.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_start"
    )]
    pub start: Option<SetSelection>,
    /// The element at the end.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_end"
    )]
    pub end: Option<SetSelection>,
}

/// A helical sweep: a section moved along a helix that the operation
/// defines. A coil is a helical sweep of a generated section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HelicalSweep {
    /// The section.
    pub section: HelicalSection,
    /// The helix.
    pub helix: Helix,
    /// What the sweep makes and how it changes existing bodies.
    pub output: SolidOutput,
    /// Source options whose effect is not established.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options that select how the source computes the result.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// The section of a helical sweep.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HelicalSection {
    /// A profile.
    Profile(ProfileRef),
    /// A shape the source states by size.
    Generated {
        /// The shape.
        shape: SectionShape,
        /// Where the section lies relative to the helix.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_placement"
        )]
        placement: Option<SectionPlacement>,
    },
}

/// Where a coil's generated section lies relative to its helix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum SectionPlacement {
    /// Inside the helix.
    Inside,
    /// On the helix; for a triangle, the midpoint of its altitude lies on the
    /// helix.
    Center,
    /// Outside the helix.
    Outside,
}

/// A helix or a planar spiral.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Helix {
    /// The frame whose z axis is the helix axis and its advance direction.
    pub placement: Construction<kinds::Frame>,
    /// Where the helix starts and its radius. `None` states that the swept
    /// profile fixes them.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_start"
    )]
    pub start: Option<HelixStart>,
    /// The shape.
    pub shape: HelixShape,
    /// The turning sense.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_hand"
    )]
    pub hand: Option<Dimension<Hand>>,
}

/// Where a helix starts and its radius.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HelixStart {
    /// On the placement's x axis rotated by `angle`.
    Radius {
        /// The radius.
        radius: RadialDimension<PositiveLength>,
        /// The rotation of the start about the axis.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_angle"
        )]
        angle: Option<Dimension<Angle>>,
    },
    /// At a constructed point, which fixes radius and start direction.
    Point(Construction<kinds::Point>),
    /// On a selected circle, which fixes both.
    Circle(OrderedSelection),
}

/// The shape of a helix.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum HelixShape {
    /// A helix.
    Helix {
        /// The two values the source drives; the third follows from them.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_law"
        )]
        law: Option<HelixLaw>,
        /// What makes the helix conical.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_taper"
        )]
        taper: Option<HelixTaper>,
    },
    /// A planar spiral.
    Spiral {
        /// The two values the source drives.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_law"
        )]
        law: Option<SpiralLaw>,
    },
    /// A source shape whose meaning is not established.
    Native(NonBlankString),
}

/// The two values of a helix that the source drives; the third follows from
/// them.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum HelixLaw {
    /// Pitch and height.
    PitchHeight {
        /// The pitch.
        pitch: Dimension<PositiveLength>,
        /// The height.
        height: Dimension<PositiveLength>,
    },
    /// Pitch and turns.
    PitchTurns {
        /// The pitch.
        pitch: Dimension<PositiveLength>,
        /// The number of turns.
        turns: Dimension<PositiveReal>,
    },
    /// Height and turns.
    HeightTurns {
        /// The height.
        height: Dimension<PositiveLength>,
        /// The number of turns.
        turns: Dimension<PositiveReal>,
    },
    /// A source law whose meaning is not established.
    Native(NonBlankString),
}

/// What makes a helix conical.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum HelixTaper {
    /// A cone angle.
    Angle(Dimension<Angle>),
    /// Radial growth per turn.
    Growth(Dimension<Length>),
    /// A radius law.
    Radius(Law<Length>),
}

/// The two values of a planar spiral that the source drives.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SpiralLaw {
    /// Pitch and turns.
    PitchTurns {
        /// The radial growth per turn.
        pitch: Dimension<NonZeroLength>,
        /// The number of turns.
        turns: Dimension<PositiveReal>,
    },
    /// End radius and turns.
    EndRadiusTurns {
        /// The end radius.
        end: Dimension<PositiveLength>,
        /// The number of turns.
        turns: Dimension<PositiveReal>,
    },
    /// End radius and pitch.
    EndRadiusPitch {
        /// The end radius.
        end: Dimension<PositiveLength>,
        /// The radial growth per turn.
        pitch: Dimension<NonZeroLength>,
    },
}

/// A rib: a wall of `thickness` grown about open profile curves until it
/// meets material. A web is a rib whose profile holds several curves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Rib {
    /// The profile curves.
    pub profile: ProfileRef,
    /// The wall's thickness and where it lies.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_thickness"
    )]
    pub thickness: Option<Thickness>,
    /// The direction the wall grows in.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_growth"
    )]
    pub growth: Option<RibGrowth>,
    /// Whether the wall grows toward the other side of the curves.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_flipped"
    )]
    pub flipped: Option<Dimension<bool>>,
    /// How far the wall grows: `Next`, an `Amount`, or a target. `None` is
    /// not stated; a writer grows the rib to the next material.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_extent"
    )]
    pub extent: Option<Limit<Length>>,
    /// The draft of the wall.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_draft"
    )]
    pub draft: Option<Dimension<Angle>>,
    /// At which end a drafted wall has the stated thickness. It applies only
    /// with `draft`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_thickness_at"
    )]
    pub thickness_at: Option<RibEnd>,
    /// The constant radius that rounds the edges where the rib's walls meet
    /// the material it grows to. `None` for none.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_root_fillet"
    )]
    pub root_fillet: Option<Dimension<PositiveLength>>,
    /// `Some(true)` extends the open ends of the profile curves along their
    /// end tangents until they meet material.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_extend_profile"
    )]
    pub extend_profile: Option<Dimension<bool>>,
    /// How the result combines with existing bodies.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_boolean"
    )]
    pub boolean: Option<Boolean>,
    /// Source options whose effect is not established.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options that select how the source computes the result.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// The direction a rib's wall grows in.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum RibGrowth {
    /// In the profile plane, with the thickness normal to that plane.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    InPlane,
    /// Along the profile normal, with the thickness in the plane.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Normal,
    /// Along a fixed direction.
    Along(Construction<kinds::Direction>),
    /// A source growth whose meaning is not established.
    Native(NonBlankString),
}

/// An end of a rib's wall.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RibEnd {
    /// Where the wall meets the material it grows to.
    Root,
    /// The wall's other end.
    Tip,
}

/// A primitive: a stated shape in a local frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Primitive {
    /// The shape.
    pub shape: PrimitiveShape,
    /// The local frame, the one home of the primitive's source placement. A
    /// placement the source stores as the identity with no binding is
    /// `None`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_placement"
    )]
    pub placement: Option<Construction<kinds::Frame>>,
    /// What the primitive makes and how it changes existing bodies.
    pub output: SolidOutput,
    /// Source options whose effect is not established.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options that select how the source computes the result.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// The shape of a primitive, in its frame. `angle`, `latitudes`, and
/// `longitude` bound partial primitives.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum PrimitiveShape {
    /// A box spanning the frame's positive octant.
    Box {
        /// The sizes along x, y, and z.
        size: [Dimension<PositiveLength>; 3],
    },
    /// A cylinder standing on the frame's xy plane about +z.
    Cylinder {
        /// The radius.
        radius: RadialDimension<PositiveLength>,
        /// The height.
        height: Dimension<PositiveLength>,
        /// The angle about z of a partial cylinder.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_angle"
        )]
        angle: Option<Dimension<Angle>>,
    },
    /// A cone standing on the frame's xy plane about +z.
    Cone {
        /// The radii at the base and at the top.
        radii: [RadialDimension<NonNegativeLength>; 2],
        /// The height.
        height: Dimension<PositiveLength>,
        /// The angle about z of a partial cone.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_angle"
        )]
        angle: Option<Dimension<Angle>>,
    },
    /// A sphere centred on the frame's origin.
    Sphere {
        /// The radius.
        radius: RadialDimension<PositiveLength>,
        /// The latitudes that bound a partial sphere.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_latitudes"
        )]
        latitudes: Option<[Dimension<Angle>; 2]>,
        /// The longitude that bounds a partial sphere.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_longitude"
        )]
        longitude: Option<Dimension<Angle>>,
    },
    /// A torus centred on the frame's origin, about z.
    Torus {
        /// The major and minor radii.
        radii: [RadialDimension<PositiveLength>; 2],
        /// The latitudes that bound a partial torus.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_latitudes"
        )]
        latitudes: Option<[Dimension<Angle>; 2]>,
        /// The longitude that bounds a partial torus.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_longitude"
        )]
        longitude: Option<Dimension<Angle>>,
    },
    /// The sphere of radius `radii[0]` scaled along the frame's y axis to
    /// `radii[1]` and along z to `radii[2]`.
    Ellipsoid {
        /// The radii along x, y, and z.
        radii: [Dimension<PositiveLength>; 3],
        /// The latitudes that bound it, as they bound the sphere before the
        /// scaling.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_latitudes"
        )]
        latitudes: Option<[Dimension<Angle>; 2]>,
        /// The longitude that bounds it, as it bounds the sphere before the
        /// scaling.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_longitude"
        )]
        longitude: Option<Dimension<Angle>>,
    },
    /// A regular polygon on a circle about z extruded along z. Its first
    /// vertex lies on the frame's x axis.
    Prism {
        /// The number of vertices, at least 3.
        sides: Dimension<u32>,
        /// The radius of the circle the vertices lie on.
        circumradius: Dimension<PositiveLength>,
        /// The extrusion along z.
        height: Dimension<PositiveLength>,
    },
    /// The solid between two rectangles normal to y.
    Wedge {
        /// The y of the base rectangle and of the top rectangle.
        y: [Dimension<Length>; 2],
        /// The base rectangle at `y[0]`: x from `base[0]` to `base[1]`, z
        /// from `base[2]` to `base[3]`.
        base: [Dimension<Length>; 4],
        /// The top rectangle at `y[1]`: x from `top[0]` to `top[1]`, z from
        /// `top[2]` to `top[3]`.
        top: [Dimension<Length>; 4],
    },
    /// The rectangular face x in [0, `size[0]`], y in [0, `size[1]`] of the
    /// frame's xy plane, a sheet.
    Plane {
        /// The sizes along x and y.
        size: [Dimension<PositiveLength>; 2],
    },
    /// A source shape whose meaning is not established.
    Native(NonBlankString),
}

/// A loft: a surface or solid built through sections, between optional point
/// ends, with an optional second direction of sections, an optional spine,
/// guides, correspondence, shape controls, and output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Loft {
    /// A point section at the start.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_start"
    )]
    pub start: Option<PointSection>,
    /// The sections, in loft order.
    pub sections: Vec<Section>,
    /// A point section at the end.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_end"
    )]
    pub end: Option<PointSection>,
    /// The sections of a second direction.
    pub cross: Vec<Section>,
    /// The spine. `None` states that the source has none.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_spine"
    )]
    pub spine: Option<Spine>,
    /// The guides.
    pub guides: Vec<Guide>,
    /// How the sections are coupled to each other.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_correspondence"
    )]
    pub correspondence: Option<Correspondence>,
    /// The shape controls.
    pub shape: LoftShape,
    /// What the loft makes and how it changes existing bodies.
    pub output: SolidOutput,
    /// Source options whose effect is not established.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options that select how the source computes the result.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// A section of a loft.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Section {
    /// The section's profile and where it lies.
    pub form: SectionForm,
    /// Whether the section's direction is flipped.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_flip"
    )]
    pub flip: Option<Dimension<bool>>,
    /// The section's seam.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_seam"
    )]
    pub seam: Option<Seam>,
    /// How the surface meets the section.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_condition"
    )]
    pub condition: Option<SectionCondition>,
    /// `Some(true)` splits the support that the condition names at the
    /// section's curve and makes the piece the curve bounds part of the
    /// result; `Some(false)` leaves the support out of the result.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_trim_support"
    )]
    pub trim_support: Option<Dimension<bool>>,
    /// The point of the spine the section belongs to, where the source states
    /// it; with none, the section belongs to the point where its plane meets
    /// the spine.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_spine_point"
    )]
    pub spine_point: Option<Construction<kinds::Point>>,
}

/// A loft section's profile and where it lies.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SectionForm {
    /// A profile where it lies.
    Profile(ProfileRef),
    /// A profile put at a constructed frame away from its sketch.
    Placed {
        /// The profile.
        profile: ProfileRef,
        /// The frame.
        placement: Box<Construction<kinds::Frame>>,
    },
}

/// The seam of a loft section.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Seam {
    /// At a selected vertex.
    Vertex(OrderedSelection),
    /// At a parameter of the section's curve.
    Parameter(FiniteReal),
}

/// A point section, which exists only at a loft's start or end.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PointSection {
    /// The point.
    pub point: Construction<kinds::Point>,
    /// How the surface meets the point.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_condition"
    )]
    pub condition: Option<PointCondition>,
}

/// How a loft surface meets a point section.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PointCondition {
    /// A sharp point.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    Sharp,
    /// A tangent point.
    Tangent {
        /// The source's tangent magnitude.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_weight"
        )]
        weight: Option<Dimension<PositiveReal>>,
    },
    /// Tangent to a plane.
    TangentToPlane {
        /// The plane.
        plane: Box<Construction<kinds::Plane>>,
        /// The source's tangent magnitude.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_weight"
        )]
        weight: Option<Dimension<PositiveReal>>,
    },
}

/// A guide of a loft.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Guide {
    /// The guide curve.
    pub path: PathRef,
    /// How the surface meets the guide.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tangency"
    )]
    pub tangency: Option<SectionCondition>,
    /// How far along each section's boundary the guide shapes the surface.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_influence"
    )]
    pub influence: Option<GuideInfluence>,
}

/// How far along each section's boundary, from the point where a guide meets
/// it, the guide shapes the surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum GuideInfluence {
    /// To the next guide.
    NextGuide,
    /// To the next sharp corner of the section, where two contiguous
    /// entities have no common tangent or equal curvature.
    NextSharp,
    /// To the end of the section entity the guide meets.
    NextEdge,
    /// The whole section.
    Global,
}

/// The spine of a loft.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Spine {
    /// A spine path, which carries the sweep controls.
    Path {
        /// The path.
        path: PathRef,
        /// The orientation, twist, and scaling along the path.
        controls: Box<SweepControls>,
    },
    /// A centerline.
    Centerline {
        /// The path.
        path: PathRef,
        /// The intermediate-section count.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_sections"
        )]
        sections: Option<Dimension<u32>>,
        /// Sections of a stated area placed along the centerline.
        stations: Vec<AreaStation>,
    },
    /// The source computes the spine.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    Computed,
}

/// A section of a stated area placed along a loft's centerline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct AreaStation {
    /// Where the station lies.
    pub at: CenterlineAt,
    /// The area.
    pub area: Dimension<PositiveArea>,
    /// `false` makes the area a cache of the area the loft has there.
    pub driving: bool,
}

/// Where an area station lies on a centerline.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum CenterlineAt {
    /// A fraction of the centerline's length between the first and the last
    /// section.
    Fraction(Dimension<Fraction>),
    /// A distance along the centerline from the first section.
    Distance(Dimension<NonNegativeLength>),
}

/// How a loft's sections are coupled to each other.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Correspondence {
    /// The coupling mode.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_mode"
    )]
    pub mode: Option<CouplingMode>,
    /// The couplings the user places.
    pub connectors: Vec<Connector>,
}

/// The mode that couples a loft's sections.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum CouplingMode {
    /// By ratio.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Ratio,
    /// By tangency.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Tangency,
    /// By tangency, then curvature.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    TangencyThenCurvature,
    /// By vertices.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Vertices,
    /// By arc length.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    ArcLength,
    /// By parameter: member k of one section joins member k of the next at
    /// equal parameter.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Parameter,
    /// By segment.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    BySegment,
    /// A source coupling whose meaning is not established.
    Native(NonBlankString),
}

/// A coupling the user places between a loft's sections.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Connector {
    /// The points the coupling joins, in section order.
    pub points: OrderedSelection,
}

/// The shape controls of a loft.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct LoftShape {
    /// How the surface follows its sections between them.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_style"
    )]
    pub style: Option<LoftStyle>,
    /// The loft degree.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_max_degree"
    )]
    pub max_degree: Option<Dimension<NonZeroU32>>,
    /// How the result approximates the exact loft.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_rebuild"
    )]
    pub rebuild: Option<Rebuild>,
    /// Whether the surface is ruled between its sections.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_ruled"
    )]
    pub ruled: Option<Dimension<bool>>,
    /// Whether the loft closes on itself.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_closed"
    )]
    pub closed: Option<Dimension<bool>>,
    /// `Some(true)` ends the result at the first and last section;
    /// `Some(false)` extends it along the spine and guides to their ends.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_relimit"
    )]
    pub relimit: Option<Dimension<bool>>,
    /// Whether result faces that meet tangentially merge into one face.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_merge_tangent_faces"
    )]
    pub merge_tangent_faces: Option<Dimension<bool>>,
    /// The tolerances within which the result departs from the exact loft.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_smoothing"
    )]
    pub smoothing: Option<LoftSmoothing>,
    /// How the section changes from one section to the next where a loft has
    /// more than two.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_section_law"
    )]
    pub section_law: Option<Interpolation>,
}

/// The tolerances within which a loft's result departs from the exact loft.
/// The result is the source's evaluation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct LoftSmoothing {
    /// The angular discontinuity of the spine's tangent or the guides' normal
    /// below which the moving frame is smoothed. `None` where it is off.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_angle"
    )]
    pub angle: Option<Dimension<Angle>>,
    /// The distance by which the surface may leave the guides. `None` where
    /// it is off.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_deviation"
    )]
    pub deviation: Option<Dimension<NonNegativeLength>>,
}

/// How a loft surface follows its sections between them.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum LoftStyle {
    /// The sections' control points are the surface's control points, so the
    /// surface passes through the first and last sections and approximates
    /// the others.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Loose,
    /// The surface's knot vectors are uniform.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Uniform,
    /// A degree-3 interpolation of the sections whose knots are spaced by the
    /// chord between consecutive compatible sections.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    ChordLength,
    /// A degree-3 interpolation of the sections whose knots are spaced by the
    /// square root of that chord.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    SqrtChordLength,
    /// A source style whose meaning is not established.
    Native(NonBlankString),
}

/// How a loft's result approximates the exact loft.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Rebuild {
    /// Rebuilt with that many control points.
    Points(u32),
    /// Fitted within `tolerance`.
    Refit {
        /// The tolerance.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_tolerance"
        )]
        tolerance: Option<Length>,
    },
    /// The degree of a separate simplification of the result.
    Degree(u32),
}

cadmpeg_core::named_optional_field!(deserialize_against<T>, T, "against");
cadmpeg_core::named_optional_field!(deserialize_along<T>, T, "along");
cadmpeg_core::named_optional_field!(deserialize_angle<T>, T, "angle");
cadmpeg_core::named_optional_field!(deserialize_apex<T>, T, "apex");
cadmpeg_core::named_optional_field!(deserialize_axis<T>, T, "axis");
cadmpeg_core::named_optional_field!(deserialize_boolean<T>, T, "boolean");
cadmpeg_core::named_optional_field!(deserialize_closed<T>, T, "closed");
cadmpeg_core::named_optional_field!(deserialize_condition<T>, T, "condition");
cadmpeg_core::named_optional_field!(deserialize_correspondence<T>, T, "correspondence");
cadmpeg_core::named_optional_field!(deserialize_curvilinear<T>, T, "curvilinear");
cadmpeg_core::named_optional_field!(deserialize_deviation<T>, T, "deviation");
cadmpeg_core::named_optional_field!(deserialize_direction<T>, T, "direction");
cadmpeg_core::named_optional_field!(deserialize_draft<T>, T, "draft");
cadmpeg_core::named_optional_field!(deserialize_end<T>, T, "end");
cadmpeg_core::named_optional_field!(deserialize_extend_profile<T>, T, "extend_profile");
cadmpeg_core::named_optional_field!(deserialize_extent<T>, T, "extent");
cadmpeg_core::named_optional_field!(deserialize_fit<T>, T, "fit");
cadmpeg_core::named_optional_field!(deserialize_flip<T>, T, "flip");
cadmpeg_core::named_optional_field!(deserialize_flipped<T>, T, "flipped");
cadmpeg_core::named_optional_field!(deserialize_growth<T>, T, "growth");
cadmpeg_core::named_optional_field!(deserialize_hand<T>, T, "hand");
cadmpeg_core::named_optional_field!(deserialize_holes<T>, T, "holes");
cadmpeg_core::named_optional_field!(deserialize_influence<T>, T, "influence");
cadmpeg_core::named_optional_field!(deserialize_inner<T>, T, "inner");
cadmpeg_core::named_optional_field!(deserialize_inner_diameter<T>, T, "inner_diameter");
cadmpeg_core::named_optional_field!(deserialize_inner_join<T>, T, "inner_join");
cadmpeg_core::named_optional_field!(deserialize_latitudes<T>, T, "latitudes");
cadmpeg_core::named_optional_field!(deserialize_law<T>, T, "law");
cadmpeg_core::named_optional_field!(
    deserialize_lengths_along_normal<T>,
    T,
    "lengths_along_normal"
);
cadmpeg_core::named_optional_field!(deserialize_longitude<T>, T, "longitude");
cadmpeg_core::named_optional_field!(deserialize_loops<T>, T, "loops");
cadmpeg_core::named_optional_field!(deserialize_max_degree<T>, T, "max_degree");
cadmpeg_core::named_optional_field!(deserialize_merge_tangent_faces<T>, T, "merge_tangent_faces");
cadmpeg_core::named_optional_field!(deserialize_mode<T>, T, "mode");
cadmpeg_core::named_optional_field!(deserialize_neutral<T>, T, "neutral");
cadmpeg_core::named_optional_field!(deserialize_offset<T>, T, "offset");
cadmpeg_core::named_optional_field!(deserialize_orientation<T>, T, "orientation");
cadmpeg_core::named_optional_field!(deserialize_origin<T>, T, "origin");
cadmpeg_core::named_optional_field!(deserialize_outer<T>, T, "outer");
cadmpeg_core::named_optional_field!(deserialize_outer_join<T>, T, "outer_join");
cadmpeg_core::named_optional_field!(deserialize_outside<T>, T, "outside");
cadmpeg_core::named_optional_field!(deserialize_placement<T>, T, "placement");
cadmpeg_core::named_optional_field!(deserialize_rebuild<T>, T, "rebuild");
cadmpeg_core::named_optional_field!(deserialize_relimit<T>, T, "relimit");
cadmpeg_core::named_optional_field!(deserialize_reversed<T>, T, "reversed");
cadmpeg_core::named_optional_field!(deserialize_root_fillet<T>, T, "root_fillet");
cadmpeg_core::named_optional_field!(deserialize_ruled<T>, T, "ruled");
cadmpeg_core::named_optional_field!(deserialize_scaling<T>, T, "scaling");
cadmpeg_core::named_optional_field!(deserialize_seam<T>, T, "seam");
cadmpeg_core::named_optional_field!(deserialize_section_law<T>, T, "section_law");
cadmpeg_core::named_optional_field!(deserialize_sections<T>, T, "sections");
cadmpeg_core::named_optional_field!(deserialize_sheet_cut<T>, T, "sheet_cut");
cadmpeg_core::named_optional_field!(deserialize_smoothing<T>, T, "smoothing");
cadmpeg_core::named_optional_field!(deserialize_span<T>, T, "span");
cadmpeg_core::named_optional_field!(deserialize_spine<T>, T, "spine");
cadmpeg_core::named_optional_field!(deserialize_spine_point<T>, T, "spine_point");
cadmpeg_core::named_optional_field!(deserialize_start<T>, T, "start");
cadmpeg_core::named_optional_field!(deserialize_style<T>, T, "style");
cadmpeg_core::named_optional_field!(deserialize_swap_axes<T>, T, "swap_axes");
cadmpeg_core::named_optional_field!(deserialize_tangency<T>, T, "tangency");
cadmpeg_core::named_optional_field!(deserialize_tangent<T>, T, "tangent");
cadmpeg_core::named_optional_field!(deserialize_taper<T>, T, "taper");
cadmpeg_core::named_optional_field!(deserialize_thickness<T>, T, "thickness");
cadmpeg_core::named_optional_field!(deserialize_thickness_at<T>, T, "thickness_at");
cadmpeg_core::named_optional_field!(deserialize_tolerance<T>, T, "tolerance");
cadmpeg_core::named_optional_field!(deserialize_transition<T>, T, "transition");
cadmpeg_core::named_optional_field!(deserialize_trim_support<T>, T, "trim_support");
cadmpeg_core::named_optional_field!(deserialize_turn<T>, T, "turn");
cadmpeg_core::named_optional_field!(deserialize_twist<T>, T, "twist");
cadmpeg_core::named_optional_field!(deserialize_weight<T>, T, "weight");

#[cfg(test)]
mod tests;
