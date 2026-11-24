// SPDX-License-Identifier: Apache-2.0
//! Patterns, template instances, and body operations (§6.5).
//!
//! A [`Pattern`] repeats seeds along a layout, and a mirror is a pattern whose
//! layout is one reflection. A [`TemplateInstance`] is a group of features
//! created from a template. The body operations combine, split, delete,
//! extract, move, and copy bodies, edit faces directly, graft a sheet onto a
//! body, and flex bodies: [`Combine`], [`Split`], [`DeleteBodies`],
//! [`Extract`], [`TransformBodies`], [`MoveFaces`], [`DeleteFaces`],
//! [`ReplaceFaces`], [`Graft`], and [`Flex`].

use std::collections::BTreeMap;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::design::construction::{kinds, Construction};
use crate::design::dimension::Dimension;
use crate::design::expr::Expression;
use crate::design::names::InstanceIndex;
use crate::design::ops::shared::{
    Alignment, JointConnector, JointKind, Locus, PatternLayout, ToolDisposition,
};
use crate::design::options::{BooleanOp, NativeOptions, ProjectionDirection};
use crate::design::selection::{OrderedSelection, SetSelection};
use crate::features::{FeatureId, ParameterId};
use crate::ids::OccurrenceId;
use crate::products::ExternalDocument;
use crate::scalar::{Angle, FiniteReal, Length, NonNegativeLength, NonZeroReal, PositiveLength};
use crate::transform::Transform;

#[cfg(test)]
mod tests;

/// A repetition of seeds along a layout. A mirror is a pattern whose layout
/// is one reflection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Pattern {
    /// What is patterned.
    pub seeds: PatternSeeds,
    /// Where the instances lie.
    pub layout: PatternLayout,
    /// Whether the instances exist only as the pattern's result or are
    /// features of their own.
    pub instances: PatternInstances,
    /// Source options whose meaning is not established, and the values of
    /// typed facts whose carrier is unread.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options with no design meaning.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// What a pattern repeats.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PatternSeeds {
    /// The seed features, each pick `FeatureResult(f)`, in source order.
    /// Each instance applies the seed features' definitions at the instance's
    /// placement.
    Features {
        /// The seed features.
        features: OrderedSelection,
        /// How each instance is built.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_method"
        )]
        method: Option<PatternMethod>,
    },
    /// Seed faces, copied into their body at each instance.
    Faces(SetSelection),
    /// Whole bodies, copied as new bodies.
    Bodies {
        /// The seed bodies.
        bodies: SetSelection,
        /// Whether each copy merges with its seed.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_join"
        )]
        join: Option<bool>,
    },
}

/// How a feature pattern builds its instances.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum PatternMethod {
    /// Copies the seed's result, its intersections with the model included,
    /// to each instance.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Identical,
    /// Builds each instance at its placement and solves its terminations and
    /// intersections against the model.
    Adjust {
        /// Which model. `None` does not state which.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_against"
        )]
        against: Option<AdjustAgainst>,
    },
    /// A method whose meaning is not established.
    Native(NonBlankString),
}

/// The model an adjusting pattern builds and intersects its instances
/// against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AdjustAgainst {
    /// The model as the instances before it left it, each instance built and
    /// intersected in turn.
    ModelSoFar,
    /// The model as it stood before the pattern, every instance built against
    /// it and all of them intersected with it at once.
    ModelBefore,
}

/// The form of a pattern's instances. An instance is named by its
/// [`InstanceIndex`].
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum PatternInstances {
    /// Instances that exist only as the pattern's result, each override
    /// applying to one instance.
    Implicit(Vec<InstanceOverride>),
    /// Instances that are features of their own. An index the layout counts
    /// and the list omits is a skipped instance.
    Members(Vec<PatternMember>),
}

/// A change to one implicit instance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct InstanceOverride {
    /// The instance.
    pub index: InstanceIndex,
    /// The change.
    pub change: InstanceChange,
}

/// How an override changes its instance.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InstanceChange {
    /// Leaves the instance out. The other instances keep their coordinates.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    Skipped,
    /// Replaces the expression of seed parameters within the instance.
    Varied {
        /// The expression of each seed parameter within the instance.
        #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
        parameters: BTreeMap<ParameterId, Expression>,
    },
}

/// An instance that is features of its own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PatternMember {
    /// The instance.
    pub index: InstanceIndex,
    /// One feature per seed feature, in seed order.
    pub features: Vec<FeatureId>,
}

/// A group of features created from a template, with the template's inputs
/// bound to selections and its parameters to design parameters. The instance
/// adds no geometry of its own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TemplateInstance {
    /// The template.
    pub template: Template,
    /// The template's inputs in the template's order.
    pub inputs: Vec<TemplateInput>,
    /// The binding of each template parameter to a design parameter of the
    /// scope.
    pub parameters: Vec<TemplateParameter>,
    /// The features the instance created. They follow the instance
    /// contiguously in its container.
    pub members: Vec<FeatureId>,
    /// Source options whose meaning is not established, and the values of
    /// typed facts whose carrier is unread.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options with no design meaning.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// The template an instance was created from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Template {
    /// The template's name.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_name"
    )]
    pub name: Option<NonBlankString>,
    /// The library document the template came from.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_library"
    )]
    pub library: Option<ExternalDocument>,
    /// The row of a template family table.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_variant"
    )]
    pub variant: Option<NonBlankString>,
}

/// One input of a template instance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TemplateInput {
    /// The source's role name, where it has one.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_role"
    )]
    pub role: Option<NonBlankString>,
    /// The selection the input is bound to.
    pub reference: OrderedSelection,
}

/// The binding of one template parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TemplateParameter {
    /// The source's role name, where it has one.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_role"
    )]
    pub role: Option<NonBlankString>,
    /// The design parameter it binds.
    pub parameter: ParameterId,
}

/// A Boolean combination of bodies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Combine {
    /// The body the result keeps the identity of, anchored `Input`. `None`
    /// states that the source names no target.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_target"
    )]
    pub target: Option<SetSelection>,
    /// The other operands, in source order.
    pub tools: OrderedSelection,
    /// The combination.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_op"
    )]
    pub op: Option<BooleanOp>,
    /// What the source does with the tools.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tools_after"
    )]
    pub tools_after: Option<ToolDisposition>,
    /// The new component the feature makes, which the result goes into.
    /// `None` states that the result stays with the target.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_new_component"
    )]
    pub new_component: Option<NewComponent>,
    /// Source options whose meaning is not established, and the values of
    /// typed facts whose carrier is unread.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options with no design meaning.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// A new component that a combine makes and puts its result into.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct NewComponent {
    /// The occurrence of the new component's definition, which the feature
    /// creates. That definition owns the result bodies in the state after
    /// the feature.
    pub occurrence: OccurrenceId,
}

/// One operation that splits bodies and faces, trims them, and cuts bodies
/// with a surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Split {
    /// What cuts what.
    pub mode: SplitMode,
    /// Source options whose meaning is not established, and the values of
    /// typed facts whose carrier is unread.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options with no design meaning.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// What a split cuts, and with what.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SplitMode {
    /// Cuts `targets` with `tools` and leaves the tools unchanged.
    ByTools {
        /// The bodies or faces cut. `None` where the definition states
        /// `assembly`, whose participants are cut.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_targets"
        )]
        targets: Option<SetSelection>,
        /// The tools that cut them.
        tools: SplitTool,
        /// How the tools meet the targets.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_method"
        )]
        method: Option<SplitMethod>,
        /// The pieces kept, with one side flag for the tools. `None` where
        /// the decoder did not read the kept side.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_kept"
        )]
        kept: Option<Kept<Dimension<bool>>>,
    },
    /// Trims each element by the others and joins the kept pieces.
    Mutual {
        /// The elements.
        elements: OrderedSelection,
        /// The pieces kept, with one side flag per element in element order,
        /// each `None` where the source does not state it.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_kept"
        )]
        kept: Option<Kept<Vec<Option<Dimension<bool>>>>>,
    },
}

/// The tools of a split.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SplitTool {
    /// A datum, an origin plane, or a constructed plane.
    Plane(Box<Construction<kinds::Plane>>),
    /// Faces, sheet or solid bodies, curves, or sketches.
    Geometry(SetSelection),
}

/// How a split's tools meet its targets.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SplitMethod {
    /// Tool surfaces cross the targets.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Intersect,
    /// A curve tool reaches the targets along a projection.
    Project(Box<ProjectionDirection>),
}

/// The pieces a split keeps. `S` is the side flag of the tools, or one per
/// element.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound(serialize = "S: Serialize", deserialize = "S: Deserialize<'de>")
)]
pub enum Kept<S> {
    /// Every piece.
    All {
        /// The form of the pieces. `None` means not read.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_form"
        )]
        form: Option<PieceForm>,
    },
    /// For each side-defining tool or element, the side its oriented normal
    /// points to, or the other side where the flag is true.
    Side {
        /// The flags.
        side: S,
    },
    /// The kept or removed pieces. A piece pick targets the input face or
    /// body that the split cuts and locates the piece by its domain.
    Pieces {
        /// The kept pieces.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_keep"
        )]
        keep: Option<Box<SetSelection>>,
        /// The removed pieces.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_remove"
        )]
        remove: Option<Box<SetSelection>>,
    },
    /// A source choice whose meaning is not established.
    Native(NonBlankString),
}

/// The form of the pieces a split keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PieceForm {
    /// Each piece a body of its own.
    Bodies,
    /// The pieces in one body as separate regions.
    Regions,
    /// The pieces as faces of their body.
    Faces,
}

/// A removal of bodies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DeleteBodies {
    /// The picked bodies.
    pub bodies: SetSelection,
    /// Whether the picked bodies or every other body are removed.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_retain"
    )]
    pub retain: Option<Retention>,
    /// Source options whose meaning is not established, and the values of
    /// typed facts whose carrier is unread.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options with no design meaning.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// Which bodies a removal removes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Retention {
    /// The picked bodies.
    DeleteSelected,
    /// Every other body of the anchored state, which at a current state's
    /// position is that state's extent.
    KeepSelected,
}

/// New geometry that copies the picked entities: bodies as bodies, faces as
/// one sheet body, edges and curves as a wire body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Extract {
    /// The copied entities.
    pub source: SetSelection,
    /// Whether the copy's orientation is complemented: every face normal and
    /// edge direction reversed, and a solid's material side exchanged.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_reversed"
    )]
    pub reversed: Option<bool>,
    /// Source options whose meaning is not established, and the values of
    /// typed facts whose carrier is unread.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options with no design meaning.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// A motion or copy of bodies by a transformation. It always has a step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TransformBodies {
    /// The picked bodies.
    pub bodies: SetSelection,
    /// The transformation.
    pub transformation: Transformation,
    /// Whether the bodies move or are copied.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_mode"
    )]
    pub mode: Option<TransformMode>,
    /// Source options whose meaning is not established, and the values of
    /// typed facts whose carrier is unread.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options with no design meaning.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// Whether a transformation moves its bodies or copies them.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TransformMode {
    /// Transforms the picked bodies in place, and they keep their identity.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    Move,
    /// Leaves the bodies and adds `count` copies: copy k, for k from 1 to
    /// `count`, applies the per-copy motion k times.
    Copy {
        /// The number of copies.
        count: Dimension<u32>,
        /// The divisor of each step's distance or angle that gives the
        /// per-copy motion. `None` means not read.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_divisions"
        )]
        divisions: Option<Dimension<u32>>,
    },
}

/// A transformation: steps applied in order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Transformation {
    /// The steps, in order.
    pub steps: Vec<TransformStep>,
    /// The composed transform in the state the steps are anchored in: a cache
    /// of the steps.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_resolved"
    )]
    pub resolved: Option<Transform>,
}

/// One step of a transformation. A reflection is not a step.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum TransformStep {
    /// A move along a direction by a distance.
    Translate {
        /// The direction.
        direction: Construction<kinds::Direction>,
        /// The distance.
        distance: Dimension<Length>,
    },
    /// A move by three distances along a frame's axes.
    Components {
        /// The frame.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_frame"
        )]
        frame: Option<Construction<kinds::Frame>>,
        /// The distances along the frame's x, y, and z axes.
        delta: [Dimension<Length>; 3],
    },
    /// A move of one point onto another.
    PointToPoint {
        /// The point moved.
        from: Construction<kinds::Point>,
        /// The point it moves onto.
        to: Construction<kinds::Point>,
    },
    /// A turn about an axis.
    Rotate {
        /// The axis.
        axis: Construction<kinds::Axis>,
        /// The angle.
        angle: Dimension<Angle>,
    },
    /// A move of one frame onto another.
    FrameToFrame {
        /// The frame moved.
        from: Construction<kinds::Frame>,
        /// The frame it moves onto.
        to: Construction<kinds::Frame>,
    },
    /// A scale about a locus.
    Scale {
        /// The locus.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_center"
        )]
        center: Option<Locus>,
        /// The factors.
        factors: ScaleFactors,
    },
    /// A transform the source stores without any construction.
    Fixed(Transform),
    /// A placement of the moved bodies where its mates hold.
    Constrained {
        /// The mates.
        mates: Vec<BodyMate>,
    },
    /// A step whose meaning is not established.
    Native(NonBlankString),
}

/// A relation of a connector on a moved body to fixed geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BodyMate {
    /// The two connectors the mate relates.
    pub connectors: [JointConnector; 2],
    /// The relation: one of the joint kinds `Coincident`, `Parallel`,
    /// `Perpendicular`, `Tangent`, `Concentric`, `Distance`, or `Angle`.
    pub kind: JointKind,
    /// The relative sense of the connectors.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_alignment"
    )]
    pub alignment: Option<Alignment>,
}

/// The factors of a scale step.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ScaleFactors {
    /// One factor for every axis.
    Uniform(Dimension<NonZeroReal>),
    /// One factor per axis of a frame.
    PerAxis {
        /// The factors along the x, y, and z axes.
        factors: [Dimension<NonZeroReal>; 3],
        /// The frame.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_frame"
        )]
        frame: Option<Box<Construction<kinds::Frame>>>,
    },
}

/// Faces moved rigidly by a transformation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct MoveFaces {
    /// The faces.
    pub faces: SetSelection,
    /// The transformation.
    pub transformation: Transformation,
    /// Source options whose meaning is not established, and the values of
    /// typed facts whose carrier is unread.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options with no design meaning.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// A removal of faces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DeleteFaces {
    /// The faces.
    pub faces: SetSelection,
    /// How the gap closes.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_heal"
    )]
    pub heal: Option<Heal>,
    /// Source options whose meaning is not established, and the values of
    /// typed facts whose carrier is unread.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options with no design meaning.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// How the gap that deleted faces leave closes.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Heal {
    /// Leaves the gap open, so the result is a sheet.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Open,
    /// Closes the gap by extending the neighbouring faces.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Extend,
    /// Closes the gap with one new face bounded by the gap's edges.
    Fill {
        /// Whether the new face is tangent to each neighbouring face rather
        /// than meeting it with contact.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_tangent"
        )]
        tangent: Option<Dimension<bool>>,
    },
    /// A healing whose meaning is not established.
    Native(NonBlankString),
}

/// Faces replaced with the faces of `replacement`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ReplaceFaces {
    /// The faces replaced.
    pub faces: SetSelection,
    /// The faces that replace them.
    pub replacement: SetSelection,
    /// Source options whose meaning is not established, and the values of
    /// typed facts whose carrier is unread.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options with no design meaning.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// A replacement of part of a body's boundary by a sheet whose boundary lies
/// on the body: material is added where the sheet lies outside the body and
/// removed where it lies inside.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Graft {
    /// The body.
    pub body: SetSelection,
    /// The sheet.
    pub sheet: SetSelection,
    /// Whether the result's material lies on the side the sheet's normal
    /// points away from, rather than the side it points to.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_flip"
    )]
    pub flip: Option<Dimension<bool>>,
    /// Whether connected faces that share a surface merge.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_simplify"
    )]
    pub simplify: Option<Dimension<bool>>,
    /// Source options whose meaning is not established, and the values of
    /// typed facts whose carrier is unread.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options with no design meaning.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// A deformation of bodies between two trim planes normal to the z axis of
/// `frame`, which changes only the region between them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Flex {
    /// The bodies.
    pub bodies: SetSelection,
    /// The frame of the deformation.
    pub frame: Construction<kinds::Frame>,
    /// How far each trim plane moves inward from the bodies' extent along the
    /// z axis.
    pub trims: [Option<Dimension<NonNegativeLength>>; 2],
    /// The deformation.
    pub kind: FlexKind,
    /// Source options whose meaning is not established, and the values of
    /// typed facts whose carrier is unread.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options with no design meaning.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// The deformation of a flex.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum FlexKind {
    /// Bends about the frame's x axis with its neutral plane the frame's xz
    /// plane, keeping the arc length between the trim planes along that
    /// plane, so that length is `angle` times the bend radius.
    Bend {
        /// The angle.
        angle: Dimension<Angle>,
        /// The bend radius: a cache of the angle and the arc length, where
        /// the source stores it.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_radius"
        )]
        radius: Option<Dimension<PositiveLength>>,
    },
    /// Turns about the z axis by `angle`.
    Twist {
        /// The angle.
        angle: Dimension<Angle>,
    },
    /// Tapers along the z axis by `factor`.
    Taper {
        /// The factor.
        factor: Dimension<FiniteReal>,
    },
    /// Stretches along the z axis by `distance`.
    Stretch {
        /// The distance.
        distance: Dimension<Length>,
    },
    /// A deformation whose meaning is not established.
    Native(NonBlankString),
}

cadmpeg_core::named_optional_field!(deserialize_against<T>, T, "against");
cadmpeg_core::named_optional_field!(deserialize_alignment<T>, T, "alignment");
cadmpeg_core::named_optional_field!(deserialize_center<T>, T, "center");
cadmpeg_core::named_optional_field!(deserialize_divisions<T>, T, "divisions");
cadmpeg_core::named_optional_field!(deserialize_flip<T>, T, "flip");
cadmpeg_core::named_optional_field!(deserialize_form<T>, T, "form");
cadmpeg_core::named_optional_field!(deserialize_frame<T>, T, "frame");
cadmpeg_core::named_optional_field!(deserialize_heal<T>, T, "heal");
cadmpeg_core::named_optional_field!(deserialize_join<T>, T, "join");
cadmpeg_core::named_optional_field!(deserialize_keep<T>, T, "keep");
cadmpeg_core::named_optional_field!(deserialize_kept<T>, T, "kept");
cadmpeg_core::named_optional_field!(deserialize_library<T>, T, "library");
cadmpeg_core::named_optional_field!(deserialize_method<T>, T, "method");
cadmpeg_core::named_optional_field!(deserialize_mode<T>, T, "mode");
cadmpeg_core::named_optional_field!(deserialize_name<T>, T, "name");
cadmpeg_core::named_optional_field!(deserialize_new_component<T>, T, "new_component");
cadmpeg_core::named_optional_field!(deserialize_op<T>, T, "op");
cadmpeg_core::named_optional_field!(deserialize_radius<T>, T, "radius");
cadmpeg_core::named_optional_field!(deserialize_remove<T>, T, "remove");
cadmpeg_core::named_optional_field!(deserialize_resolved<T>, T, "resolved");
cadmpeg_core::named_optional_field!(deserialize_retain<T>, T, "retain");
cadmpeg_core::named_optional_field!(deserialize_reversed<T>, T, "reversed");
cadmpeg_core::named_optional_field!(deserialize_role<T>, T, "role");
cadmpeg_core::named_optional_field!(deserialize_simplify<T>, T, "simplify");
cadmpeg_core::named_optional_field!(deserialize_tangent<T>, T, "tangent");
cadmpeg_core::named_optional_field!(deserialize_target<T>, T, "target");
cadmpeg_core::named_optional_field!(deserialize_targets<T>, T, "targets");
cadmpeg_core::named_optional_field!(deserialize_tools_after<T>, T, "tools_after");
cadmpeg_core::named_optional_field!(deserialize_variant<T>, T, "variant");
