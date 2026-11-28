// SPDX-License-Identifier: Apache-2.0
//! Surfacing, wireframe, subdivision forms, and meshes.
//!
//! These operations build and edit sheets and wires, own subdivision cages, and
//! edit meshes. Surfaces built through sections are lofts, an offset surface is
//! an offset with a copy result, trimming and splitting sheets is a split, and
//! an explicit sweep with a positioned profile is a sweep.

use cadmpeg_core::text::NonBlankString;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::shared::{Law, ToolDisposition};
use super::sketch::TextContent;
use super::swept::{CornerTransition, Helix, SpineLimits};
use crate::design::construction::{kinds, Construction};
use crate::design::dimension::Dimension;
use crate::design::options::{
    Boolean, BooleanOp, NativeOptions, PathRef, ProfileRef, ProjectionDirection, SectionCondition,
    SheetOutput, SolidOutput,
};
use crate::design::selection::{Continuity, OrderedSelection, SetSelection};
use crate::document::Topology;
use crate::scalar::{Angle, Fraction, Length, NonNegativeLength, PositiveLength, PositiveReal};
use crate::sketches::SketchId;

/// A patch: a sheet spanning a closed boundary, optionally through passing
/// elements, around inner boundaries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Patch {
    /// The boundary.
    pub bounds: PatchBounds,
    /// The curves and points the patch passes through.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_passing"
    )]
    pub passing: Option<SetSelection>,
    /// Whether the patch is the plane through its boundary.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_planar"
    )]
    pub planar: Option<Dimension<bool>>,
    /// `Some(true)` makes the patch one degree-3 surface bounded by its own
    /// edges, not cut out of a larger surface by parameter-space curves:
    /// bicubic over a four-sided boundary, cubic by linear between the two
    /// curves of a two-sided boundary, and a plane over a planar boundary.
    /// `Some(false)` lets the source fit its default patch.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_simplified"
    )]
    pub simplified: Option<Dimension<bool>>,
    /// Whether a closed result becomes a solid, and how the patch combines with
    /// the sheets it meets.
    pub output: SheetOutput,
    /// Source options whose effect is not established.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options that select how the source computes the result.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// The boundary of a patch.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PatchBounds {
    /// Sketch regions or faces, with the regions' holes as inner boundaries and
    /// no edge conditions.
    Profile(ProfileRef),
    /// Boundary segments in traversal order, each with its own condition.
    Loops {
        /// The outer boundary.
        outer: Vec<PatchEdge>,
        /// The inner boundaries.
        inner: Vec<Vec<PatchEdge>>,
    },
}

/// One segment of a patch boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PatchEdge {
    /// The boundary curves.
    pub curve: PathRef,
    /// How the patch meets the segment: `Free` is contact, and `Tangent` and
    /// `Curvature` meet the support faces.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_condition"
    )]
    pub condition: Option<SectionCondition>,
}

/// A knit: sheets, wires, or solids joined along shared boundaries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Knit {
    /// The elements, in source order; the first is the primary.
    pub elements: OrderedSelection,
    /// The largest gap the knit closes.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tolerance"
    )]
    pub tolerance: Option<Dimension<NonNegativeLength>>,
    /// Whether coincident faces and edges of the result merge.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_simplify"
    )]
    pub simplify: Option<Dimension<bool>>,
    /// The deformation of the elements that closes their gaps.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_healing"
    )]
    pub healing: Option<Healing>,
    /// A group of elements of the result, so that a later pick of any one
    /// reaches the group: a rule selection over the result whose seeds the
    /// source names. `None` states no group.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_federation"
    )]
    pub federation: Option<SetSelection>,
    /// Whether a closed result becomes a solid, and how it combines with
    /// existing bodies.
    pub output: SheetOutput,
    /// Source options whose effect is not established.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options that select how the source computes the result.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// A deformation of a knit's elements so that gaps up to the knit's tolerance
/// close. The deformation is the source's, and the result is its evaluation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Healing {
    /// The largest gap the healing leaves.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_objective"
    )]
    pub objective: Option<Dimension<NonNegativeLength>>,
    /// The largest tangent discontinuity the healing closes.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tangency"
    )]
    pub tangency: Option<Dimension<Angle>>,
    /// The largest tangent discontinuity the healing leaves.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tangency_objective"
    )]
    pub tangency_objective: Option<Dimension<Angle>>,
    /// The elements that are not deformed.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_frozen"
    )]
    pub frozen: Option<SetSelection>,
}

/// An unsew by edges: a sheet cut in two along `edges`. The faces on the two
/// sides become two sheet bodies, each with its own copy of the picked edges,
/// the input body keeping one side and a new body holding the other.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Unsew {
    /// The edges that divide the sheet.
    pub edges: SetSelection,
    /// Source options whose effect is not established.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options that select how the source computes the result.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// An unsew by faces: each picked face cut out of its body as a sheet body of
/// its own, with its own copies of its edges. The faces left behind keep the
/// input body, open where the picked faces were.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct UnsewFaces {
    /// The faces cut out.
    pub faces: SetSelection,
    /// Source options whose effect is not established.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options that select how the source computes the result.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// An extension of sheets or wires beyond their free boundary edges.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Extend {
    /// The free boundary edges of sheets; a source that extends whole faces
    /// uses an `Of { kind: Edge }` term with face seeds.
    pub edges: SetSelection,
    /// How far the extension goes.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_limit"
    )]
    pub limit: Option<ExtendLimit>,
    /// How the surface continues.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_method"
    )]
    pub method: Option<ExtendMethod>,
    /// Whether the result merges with the original (`Apply` with `Sew`) or is a
    /// body of its own (`NewBody`).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_output"
    )]
    pub output: Option<SheetOutput>,
    /// Source options whose effect is not established.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options that select how the source computes the result.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// How far an extension goes.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ExtendLimit {
    /// A distance.
    Distance(Dimension<Length>),
    /// Up to a point or a surface.
    UpTo(SetSelection),
    /// To the carrier's natural boundary: the face covers its surface's
    /// untrimmed domain, its holes and outer trims removed, or the loops
    /// `edges` picks only, and a wire's edge covers its curve.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Natural,
    /// A source limit whose meaning is not established.
    Native(NonBlankString),
}

/// How an extended surface continues.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ExtendMethod {
    /// It continues the underlying surface.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Natural,
    /// It extends linearly.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Tangent,
    /// It extrapolates with curvature continuity.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Curvature,
    /// It extends normal to the boundary.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Perpendicular,
    /// A source method whose meaning is not established.
    Native(NonBlankString),
}

/// A sweep whose section is a line, a circle, or a conic that the operation
/// builds at each spine station from guides, supports, directions, and laws.
/// The section at each station lies in the plane normal to the spine there, and
/// each guide, surface, or direction acts through its point or trace in that
/// plane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ImplicitSweep {
    /// The section.
    pub section: ImplicitSection,
    /// The spine. `None` states that the source names no spine.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_spine"
    )]
    pub spine: Option<PathRef>,
    /// The relimiting elements on the spine.
    pub limits: SpineLimits,
    /// Which of the solutions the source enumerates.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_solution"
    )]
    pub solution: Option<u32>,
    /// How the result passes a sharp corner of its guide chain.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_corner"
    )]
    pub corner: Option<CornerTransition>,
    /// What the sweep makes and how it changes existing bodies; a sheet unless
    /// `body` says otherwise.
    pub output: SolidOutput,
    /// Source options whose effect is not established.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options that select how the source computes the result.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// The section an implicit sweep builds at each spine station.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ImplicitSection {
    /// A line.
    Line {
        /// How the line is built.
        mode: LineMode,
        /// The line's extents on either side of its first point.
        lengths: [Option<LineLength>; 2],
    },
    /// A circle.
    Circle(CircleMode),
    /// The conic arc from the first guide's point to the last's through the
    /// points of the guides between, two to five guides in order.
    Conic {
        /// The guides, in order.
        guides: Vec<PathRef>,
        /// The surface each end is tangent to, where one is stated.
        supports: [Option<SetSelection>; 2],
        /// The ratio OM/OT that places a passing point M on the line from the
        /// midpoint O of the chord to the apex T of the end tangents: 0.5 a
        /// parabola, below it an ellipse, above it a hyperbola.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_parameter"
        )]
        parameter: Option<Law<Fraction>>,
    },
    /// A source section whose meaning is not established.
    Native(NonBlankString),
}

/// How an implicit sweep builds its line section.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum LineMode {
    /// The segment between the two guides' points.
    TwoGuides {
        /// The first guide.
        first: PathRef,
        /// The second guide.
        second: PathRef,
    },
    /// The segment from the guide's point whose midpoint is the middle curve's
    /// point.
    Middle {
        /// The guide curve.
        guide: PathRef,
        /// The middle curve.
        middle: PathRef,
    },
    /// The line at `angle` from the reference surface's tangent or normal plane
    /// along the guide.
    Surface {
        /// The guide curve.
        guide: PathRef,
        /// The reference surface.
        reference: FaceReference,
        /// The plane the angle is measured from. It decides how the angle is
        /// read.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_from"
        )]
        from: Option<SurfaceRelation>,
        /// The angle.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_angle"
        )]
        angle: Option<Law<Angle>>,
    },
    /// The line through the guide's point at `angle` from the direction `from`
    /// names.
    Curve {
        /// The guide curve.
        guide: PathRef,
        /// The reference curve.
        curve: PathRef,
        /// The direction the angle is measured from. It decides how the angle
        /// is read.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_from"
        )]
        from: Option<CurveRelation>,
        /// The angle.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_angle"
        )]
        angle: Option<Law<Angle>>,
    },
    /// The line at `angle` from a draft direction.
    Direction {
        /// The guide curve.
        guide: PathRef,
        /// The draft direction.
        direction: Construction<kinds::Direction>,
        /// How the angle relates to the direction.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_from"
        )]
        from: Option<DirectionRelation>,
        /// The angle.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_angle"
        )]
        angle: Option<Law<Angle>>,
    },
    /// The line from the guide's point tangent to `surface`.
    TangentTo {
        /// The guide curve.
        guide: PathRef,
        /// The surface.
        surface: SetSelection,
    },
    /// The line tangent to both surfaces.
    TwoTangents {
        /// The first surface.
        first: SetSelection,
        /// The second surface.
        second: SetSelection,
    },
    /// A source line mode whose meaning is not established.
    Native(NonBlankString),
}

/// The reference surface of a line section.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FaceReference {
    /// The picked faces.
    Faces(SetSelection),
    /// The face adjacent to the guide edge.
    Adjacent {
        /// Whether the second face at the guide edge is taken.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_alternate"
        )]
        alternate: Option<Dimension<bool>>,
    },
}

/// The plane of a reference surface that a line section's angle is measured
/// from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum SurfaceRelation {
    /// The tangent plane.
    Tangent,
    /// The normal plane.
    Normal,
}

/// How a line section's angle relates to its draft direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DirectionRelation {
    /// Measured from the direction.
    Parallel,
    /// Measured from the plane normal to the direction.
    Perpendicular,
}

/// The direction a section's angle is measured from against a reference curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CurveRelation {
    /// The direction toward the reference curve's point.
    TowardReference,
}

/// One extent of a line section.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum LineLength {
    /// A length law.
    Law(Law<Length>),
    /// Up to an element.
    UpTo(SetSelection),
    /// A source extent whose meaning is not established.
    Native(NonBlankString),
}

/// How an implicit sweep builds its circle section.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum CircleMode {
    /// The circle through the three guides' points; `solution` chooses which of
    /// its arcs.
    ThreeGuides {
        /// The guides.
        guides: [PathRef; 3],
    },
    /// The circle of `radius` through the two guides' points.
    TwoGuidesRadius {
        /// The guides.
        guides: [PathRef; 2],
        /// The radius.
        radius: Law<Length>,
    },
    /// The circle centred at the center curve's point through the reference
    /// curve's point, between the two `angles` measured from the direction
    /// `from` names; the whole circle where both are `None`.
    CenterAngles {
        /// The center curve.
        center: PathRef,
        /// The reference curve.
        reference: PathRef,
        /// The direction the angles are measured from. It decides how the
        /// angles are read.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_from"
        )]
        from: Option<CurveRelation>,
        /// The two angles.
        angles: [Option<Law<Angle>>; 2],
    },
    /// The circle of `radius` centred at the center curve's point.
    CenterRadius {
        /// The center curve.
        center: PathRef,
        /// The radius.
        radius: Law<Length>,
    },
    /// The circle through the `passing` guide's point that touches `surface`
    /// tangentially along the `limit` guide.
    TangentTwoGuides {
        /// The guide the circle passes through.
        passing: PathRef,
        /// The guide along which the circle touches the surface.
        limit: PathRef,
        /// The tangency surface.
        surface: SetSelection,
    },
    /// The circle of `radius` through the `passing` guide's point that touches
    /// `surface` tangentially.
    TangentGuideRadius {
        /// The guide the circle passes through.
        passing: PathRef,
        /// The tangency surface.
        surface: SetSelection,
        /// The radius.
        radius: Law<Length>,
        /// Whether the tangency surface is trimmed by the sweep and assembled
        /// with it, keeping the portion that makes the result tangent.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_trim"
        )]
        trim: Option<Dimension<bool>>,
    },
    /// The circle of `radius` that touches `surface` along the `limit` curve,
    /// which lies on it: its centre lies on the surface's normal at the limit
    /// curve's point, at `radius` from it, between the two `angles` measured
    /// from the surface's tangent plane; the whole circle where both are
    /// `None`.
    TangentLimitRadius {
        /// The limit curve.
        limit: PathRef,
        /// The tangency surface.
        surface: SetSelection,
        /// The radius.
        radius: Law<Length>,
        /// The two angles.
        angles: [Option<Law<Angle>>; 2],
    },
    /// A source circle mode whose meaning is not established.
    Native(NonBlankString),
}

/// A boundary fill: tools partition space into cells, and the kept cells become
/// bodies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BoundaryFill {
    /// The tools.
    pub tools: SetSelection,
    /// The kept cells, each selected by a `Containing(point)` pick.
    pub cells: Vec<SetSelection>,
    /// Which cells the tools make.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_regions"
    )]
    pub regions: Option<RegionKind>,
    /// Whether the planar openings of sheet tools are closed first.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_cap_planar"
    )]
    pub cap_planar: Option<Dimension<bool>>,
    /// `Some(true)` makes touching kept cells one body, and `Some(false)` each
    /// kept cell a body of its own.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_merge"
    )]
    pub merge: Option<Dimension<bool>>,
    /// What becomes of the tools.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tools_after"
    )]
    pub tools_after: Option<ToolDisposition>,
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

/// Which cells a boundary fill's tools make.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RegionKind {
    /// The closed volumes that the union or the intersection of the tools
    /// bounds.
    Intersecting,
    /// The closed hollow volumes inside them.
    Internal,
    /// Both.
    Both,
}

/// A curve that is design geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CurveFeature {
    /// How the curve is made.
    pub definition: CurveDefinition,
    /// Source options whose effect is not established.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options that select how the source computes the result.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// How a curve feature makes its curve.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum CurveDefinition {
    /// A spline through constructed points.
    Spline(SplineCurve),
    /// A helix.
    Helix(Helix),
    /// Curves and points projected onto faces.
    Projection {
        /// The curves and points.
        source: SetSelection,
        /// The faces.
        onto: SetSelection,
        /// The projection direction.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_direction"
        )]
        direction: Option<ProjectionDirection>,
    },
    /// The intersection of two sets of surfaces, curves, or planes.
    Intersection {
        /// The first set.
        first: SetSelection,
        /// The second set.
        second: SetSelection,
    },
    /// A curve joining two curves at points, with an end continuity each.
    Bridge {
        /// The two ends.
        ends: [BridgeEnd; 2],
    },
    /// The outlines of a text.
    Text(TextCurve),
    /// A line segment, arc, or circle built from references by a construction.
    /// Its subjects are `Whole` and `Origin`: the origin of a circle or arc is
    /// its center, of a line segment its start.
    Constructed(Construction<kinds::Curve>),
    /// A corner of two elements.
    Corner {
        /// The arc of the corner's radius tangent to both elements, on a
        /// support where it has one.
        arc: Construction<kinds::Curve>,
        /// `Some(true)` makes the result one wire: the first element up to its
        /// tangency point, the arc, and the second element from its tangency
        /// point, each element keeping the piece that makes the wire
        /// tangent-continuous there. `Some(false)` makes the result the arc
        /// alone.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_trim"
        )]
        trim: Option<Dimension<bool>>,
    },
    /// The closed wire of a regular polygon of `sides` vertices on the circle
    /// of `circumradius` about the frame's z axis in its xy plane.
    Polygon {
        /// The frame.
        placement: Construction<kinds::Frame>,
        /// The number of vertices.
        sides: Dimension<u32>,
        /// The radius of the circle through the vertices.
        circumradius: Dimension<PositiveLength>,
    },
    /// A wire body that holds one free vertex at the constructed point.
    Point(Construction<kinds::Point>),
    /// The curve whose point at each `t` from 0 to 1 has the `coordinates` in
    /// `frame`.
    Equation {
        /// The frame.
        frame: Construction<kinds::Frame>,
        /// The coordinates, each a law over the curve parameter.
        coordinates: EquationCoordinates,
    },
    /// The curve where the extrusions of two curves meet.
    Combined {
        /// The first curve.
        first: CombineOperand,
        /// The second curve.
        second: CombineOperand,
        /// `Some(true)` keeps, of several solutions, the one nearest the first
        /// curve.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_nearest"
        )]
        nearest: Option<Dimension<bool>>,
    },
    /// The curve on `support` each of whose points lies at the offset from
    /// `curve`.
    Offset {
        /// The curve.
        curve: OrderedSelection,
        /// The support.
        support: SetSelection,
        /// The offset.
        offset: CurveOffset,
        /// How the distance is measured.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_measure"
        )]
        measure: Option<OffsetMeasure>,
        /// How a convex corner closes: sharp (`Mitre`) or by the arc a ball
        /// rolling on the curve leaves (`Round`).
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_corner"
        )]
        corner: Option<CornerTransition>,
        /// The continuity the result is made to have.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_smoothing"
        )]
        smoothing: Option<Continuity>,
    },
    /// A curve within `max_deviation` of `curve` whose discontinuities below
    /// its thresholds are smoothed. The fit is the source's, and the curve is
    /// its evaluation.
    Smoothed {
        /// The curve.
        curve: OrderedSelection,
        /// The radius of the pipe about the curve the result fits in.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_max_deviation"
        )]
        max_deviation: Option<Dimension<NonNegativeLength>>,
        /// The tangency discontinuity below which a corner is smoothed.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_tangency_threshold"
        )]
        tangency_threshold: Option<Dimension<Angle>>,
        /// The ratio r below which a curvature discontinuity is smoothed: where
        /// ‖ρ₁ − ρ₂‖ / ‖ρ₂‖ < (1 − r)/r.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_curvature_ratio"
        )]
        curvature_ratio: Option<Dimension<Fraction>>,
        /// The continuity the source forces everywhere; `None` smooths by the
        /// thresholds.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_continuity_field"
        )]
        continuity: Option<Continuity>,
    },
    /// A source curve whose meaning is not established.
    Native(NonBlankString),
}

/// The coordinates of an equation curve, each a law over the curve parameter
/// `t`.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum EquationCoordinates {
    /// Cartesian coordinates.
    Cartesian {
        /// x.
        x: Law<Length>,
        /// y.
        y: Law<Length>,
        /// z.
        z: Law<Length>,
    },
    /// Cylindrical coordinates.
    Cylindrical {
        /// The radius.
        r: Law<Length>,
        /// The angle about z from x toward y.
        theta: Law<Angle>,
        /// The height.
        z: Law<Length>,
    },
    /// Spherical coordinates: the point is rho·(sin theta cos phi, sin theta
    /// sin phi, cos theta).
    Spherical {
        /// The radius.
        rho: Law<Length>,
        /// The polar angle from the frame's +z axis.
        theta: Law<Angle>,
        /// The azimuth about z from x toward y.
        phi: Law<Angle>,
    },
    /// A source coordinate kind whose meaning is not established.
    Native(NonBlankString),
}

/// A spline through constructed points with optional tangent directions and
/// magnitudes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SplineCurve {
    /// The points, in order.
    pub points: Vec<SplinePoint>,
    /// Whether the spline closes.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_closed"
    )]
    pub closed: Option<Dimension<bool>>,
    /// The support the spline lies on.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_support"
    )]
    pub support: Option<SetSelection>,
}

/// A point of a spline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SplinePoint {
    /// The point.
    pub point: Construction<kinds::Point>,
    /// The tangent direction at the point.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tangent"
    )]
    pub tangent: Option<Construction<kinds::Direction>>,
    /// The tangent magnitude.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tension"
    )]
    pub tension: Option<Dimension<PositiveReal>>,
}

/// One end of a bridge curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BridgeEnd {
    /// The curve the bridge joins.
    pub curve: OrderedSelection,
    /// The connection point, where the source states one other than the curve
    /// end.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_at"
    )]
    pub at: Option<Construction<kinds::Point>>,
    /// The continuity at the end.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_continuity_field"
    )]
    pub continuity: Option<Continuity>,
    /// The tangent magnitude.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tension"
    )]
    pub tension: Option<Dimension<PositiveReal>>,
    /// Whether the end's direction is reversed.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_reversed"
    )]
    pub reversed: Option<Dimension<bool>>,
}

/// The outlines of a text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TextCurve {
    /// The content, font, and size, in the sketch text form.
    pub text: TextContent,
    /// Where the text lies.
    pub placement: TextPlacement,
}

/// Where a text curve lies.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TextPlacement {
    /// In the xy plane of a frame.
    Plane(Construction<kinds::Frame>),
    /// Along a curve.
    OnCurve {
        /// The curve.
        path: PathRef,
    },
    /// On faces, along a curve.
    OnFace {
        /// The faces.
        faces: SetSelection,
        /// The curve.
        path: PathRef,
    },
}

/// One curve of a combined curve, with its extrusion direction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CombineOperand {
    /// The curve.
    pub curve: OrderedSelection,
    /// The direction it is extruded along; `None` is normal to its own plane.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_direction"
    )]
    pub direction: Option<Construction<kinds::Direction>>,
}

/// The offset of an offset curve.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum CurveOffset {
    /// A constant or a law along the curve.
    Law(Law<Length>),
    /// Through a point.
    Through(Construction<kinds::Point>),
}

/// How the distance of an offset curve is measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum OffsetMeasure {
    /// The straight distance, whatever the support.
    Euclidean,
    /// Measured on the support.
    Geodesic,
}

/// A subdivision form. Its control cages are its definition, not a state; its
/// B-rep evaluation is the state after the feature, with the limit-surface
/// carriers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Form {
    /// The control cages, as a topology that holds subdivision records only.
    pub cage: Topology,
    /// Source options whose effect is not established.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options that select how the source computes the result.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// A mesh operation: an edit of mesh bodies, the tessellations with no body
/// that a mesh import contributes and that earlier mesh operations make. It is
/// not a B-rep operation with a mesh operand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct MeshFeature {
    /// The mesh bodies the operation takes, in the state before the feature.
    pub source: SetSelection,
    /// What the operation does to each.
    pub kind: MeshKind,
    /// Source options whose effect is not established.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options that select how the source computes the result.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// What a mesh operation does.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum MeshKind {
    /// The curves where the mesh bodies meet a plane, put into a sketch. It
    /// changes no body.
    Section {
        /// The sketch.
        sketch: SketchId,
    },
    /// Each mesh body cut with a plane.
    PlaneCut {
        /// The plane.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_plane"
        )]
        plane: Option<Construction<kinds::Plane>>,
        /// Which pieces are kept.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_cut"
        )]
        cut: Option<MeshCut>,
        /// How a kept piece is closed at the cut.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_fill"
        )]
        fill: Option<MeshFill>,
    },
    /// Each mesh body's triangle count lowered.
    Reduce {
        /// The quantity that bounds the result.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_target"
        )]
        target: Option<ReduceTarget>,
    },
    /// Each mesh body repaired.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Repair,
    /// A new triangulation of each mesh body.
    Remesh {
        /// The source's edge length.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_edge_length"
        )]
        edge_length: Option<Dimension<PositiveLength>>,
        /// The source's shape preservation, a fraction from 0 to 1.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_shape"
        )]
        shape: Option<Dimension<Fraction>>,
    },
    /// Each mesh body smoothed.
    Smooth {
        /// The smoothness, a fraction from 0 to 1.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_smoothness"
        )]
        smoothness: Option<Dimension<Fraction>>,
    },
    /// The mesh bodies of `source`, the targets, combined with those of
    /// `tools`.
    Combine {
        /// The tool mesh bodies, in source order.
        tools: OrderedSelection,
        /// The combination: `Join`, `Cut`, or `Intersect`.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_op"
        )]
        op: Option<BooleanOp>,
    },
    /// The mesh bodies removed from the state.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Remove,
    /// The orientation of each mesh body reversed: every triangle's winding and
    /// every normal, so that its outward side becomes its inward side.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    ReverseNormals,
    /// A B-rep body made of each mesh body.
    Convert {
        /// How the body is made.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_method"
        )]
        method: Option<MeshConversion>,
    },
    /// A source mesh operation whose meaning or operands are not established.
    Native(NonBlankString),
}

/// Which pieces a mesh plane cut keeps.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MeshCut {
    /// The piece on the side the plane's oriented normal points to, or the
    /// other piece where `flip` is true.
    Trim {
        /// Whether the other piece is kept.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_flip"
        )]
        flip: Option<bool>,
    },
    /// Both pieces, each a mesh body of its own.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    SplitBodies,
}

/// How a kept piece of a mesh plane cut is closed at the cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum MeshFill {
    /// The cut is left open.
    Open,
    /// The cut is closed with the fewest triangles that cover, in the plane,
    /// the region the cut bounds: triangles between the cut's own vertices.
    Minimal,
}

/// The quantity that bounds a reduced mesh.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ReduceTarget {
    /// The largest distance of the result from the source mesh.
    Deviation(Dimension<NonNegativeLength>),
    /// The fraction of the source's triangles the result keeps.
    Proportion(Dimension<Fraction>),
    /// The result's triangle count.
    FaceCount(Dimension<u32>),
}

/// How a mesh conversion makes its B-rep body.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum MeshConversion {
    /// Each facet of the mesh becomes a planar face of the body.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Faceted,
    /// A source method whose meaning is not established.
    Native(NonBlankString),
}

cadmpeg_core::named_optional_field!(deserialize_alternate<T>, T, "alternate");
cadmpeg_core::named_optional_field!(deserialize_angle<T>, T, "angle");
cadmpeg_core::named_optional_field!(deserialize_at<T>, T, "at");
cadmpeg_core::named_optional_field!(deserialize_boolean<T>, T, "boolean");
cadmpeg_core::named_optional_field!(deserialize_cap_planar<T>, T, "cap_planar");
cadmpeg_core::named_optional_field!(deserialize_closed<T>, T, "closed");
cadmpeg_core::named_optional_field!(deserialize_condition<T>, T, "condition");
cadmpeg_core::named_optional_field!(deserialize_continuity_field<T>, T, "continuity");
cadmpeg_core::named_optional_field!(deserialize_corner<T>, T, "corner");
cadmpeg_core::named_optional_field!(deserialize_curvature_ratio<T>, T, "curvature_ratio");
cadmpeg_core::named_optional_field!(deserialize_cut<T>, T, "cut");
cadmpeg_core::named_optional_field!(deserialize_direction<T>, T, "direction");
cadmpeg_core::named_optional_field!(deserialize_edge_length<T>, T, "edge_length");
cadmpeg_core::named_optional_field!(deserialize_federation<T>, T, "federation");
cadmpeg_core::named_optional_field!(deserialize_fill<T>, T, "fill");
cadmpeg_core::named_optional_field!(deserialize_flip<T>, T, "flip");
cadmpeg_core::named_optional_field!(deserialize_from<T>, T, "from");
cadmpeg_core::named_optional_field!(deserialize_frozen<T>, T, "frozen");
cadmpeg_core::named_optional_field!(deserialize_healing<T>, T, "healing");
cadmpeg_core::named_optional_field!(deserialize_limit<T>, T, "limit");
cadmpeg_core::named_optional_field!(deserialize_max_deviation<T>, T, "max_deviation");
cadmpeg_core::named_optional_field!(deserialize_measure<T>, T, "measure");
cadmpeg_core::named_optional_field!(deserialize_merge<T>, T, "merge");
cadmpeg_core::named_optional_field!(deserialize_method<T>, T, "method");
cadmpeg_core::named_optional_field!(deserialize_nearest<T>, T, "nearest");
cadmpeg_core::named_optional_field!(deserialize_objective<T>, T, "objective");
cadmpeg_core::named_optional_field!(deserialize_op<T>, T, "op");
cadmpeg_core::named_optional_field!(deserialize_output<T>, T, "output");
cadmpeg_core::named_optional_field!(deserialize_parameter<T>, T, "parameter");
cadmpeg_core::named_optional_field!(deserialize_passing<T>, T, "passing");
cadmpeg_core::named_optional_field!(deserialize_planar<T>, T, "planar");
cadmpeg_core::named_optional_field!(deserialize_plane<T>, T, "plane");
cadmpeg_core::named_optional_field!(deserialize_regions<T>, T, "regions");
cadmpeg_core::named_optional_field!(deserialize_reversed<T>, T, "reversed");
cadmpeg_core::named_optional_field!(deserialize_shape<T>, T, "shape");
cadmpeg_core::named_optional_field!(deserialize_simplified<T>, T, "simplified");
cadmpeg_core::named_optional_field!(deserialize_simplify<T>, T, "simplify");
cadmpeg_core::named_optional_field!(deserialize_smoothing<T>, T, "smoothing");
cadmpeg_core::named_optional_field!(deserialize_smoothness<T>, T, "smoothness");
cadmpeg_core::named_optional_field!(deserialize_solution<T>, T, "solution");
cadmpeg_core::named_optional_field!(deserialize_spine<T>, T, "spine");
cadmpeg_core::named_optional_field!(deserialize_support<T>, T, "support");
cadmpeg_core::named_optional_field!(deserialize_tangency<T>, T, "tangency");
cadmpeg_core::named_optional_field!(deserialize_tangency_objective<T>, T, "tangency_objective");
cadmpeg_core::named_optional_field!(deserialize_tangency_threshold<T>, T, "tangency_threshold");
cadmpeg_core::named_optional_field!(deserialize_tangent<T>, T, "tangent");
cadmpeg_core::named_optional_field!(deserialize_target<T>, T, "target");
cadmpeg_core::named_optional_field!(deserialize_tension<T>, T, "tension");
cadmpeg_core::named_optional_field!(deserialize_tolerance<T>, T, "tolerance");
cadmpeg_core::named_optional_field!(deserialize_tools_after<T>, T, "tools_after");
cadmpeg_core::named_optional_field!(deserialize_trim<T>, T, "trim");

#[cfg(test)]
mod tests;
