// SPDX-License-Identifier: Apache-2.0
//! Assembly operations (§6.7).
//!
//! An assembly operation is a feature of the scope that holds an assembly's
//! design: the scope of the assembly definition, or a Fusion root scope with
//! `Feature.component` naming the component. It may name an occurrence owned
//! by any definition its scope spans. The operations place, connect, repeat,
//! mirror, and modify occurrences: [`Place`], [`PlaceSet`], [`Joint`],
//! [`ComponentPattern`], [`MirrorComponents`], and [`AssemblyFeature`], whose
//! [`ContextEffect`] states what it changes. A [`RigidGroup`] holds occurrences
//! in their relative placement.

use std::collections::BTreeSet;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::body::ScaleFactors;
use super::shared::{Joint, PatternLayout};
use crate::design::construction::{kinds, Construction};
use crate::design::feature::FeatureOperation;
use crate::design::names::InstanceIndex;
use crate::design::options::NativeOptions;
use crate::design::selection::{OccurrencePath, OrderedSelection, SetSelection};
use crate::ids::OccurrenceId;

#[cfg(test)]
mod tests;

/// The placement of one occurrence, where the source orders a component's
/// placement with its features: a Creo component feature, a Fusion new
/// component, Component Insert, `CopyPaste`, `CutPaste`, or `DerivedInstance`
/// item, or an `App::Link`. The occurrence is a member of the `occurrences`
/// of a definition the scope spans.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Place {
    /// The occurrence the feature places.
    pub occurrence: OccurrenceId,
    /// The occurrence's base placement where the source stores a placement
    /// that can be bound, as a construction whose steps can be bound. `None`,
    /// with `base: None`, where the source stores no such placement; the value
    /// is then `Occurrence.transform` alone.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_placement"
    )]
    pub placement: Option<Construction<kinds::Frame>>,
    /// The scale the source composes with `placement` on the occurrence,
    /// applied in the placed definition's own axes before `placement`, with
    /// factors an expression can bind. A `PerAxis` scale states no `frame`.
    /// `None` where the source stores no scale, or a scale of one on every
    /// axis.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_scale"
    )]
    pub scale: Option<ScaleFactors>,
    /// What `placement` is relative to.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_base"
    )]
    pub base: Option<PlacementBase>,
    /// The constraints the source stores inside the component's own record.
    /// Each has at least one connector that picks into the placed occurrence.
    pub constraints: Vec<Joint>,
    /// Whether the source positions the occurrence by transform alone and
    /// leaves it free to move.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_packaged"
    )]
    pub packaged: Option<bool>,
    /// Source options whose meaning is not established, and the values of
    /// typed facts whose carrier is unread.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options with no design meaning.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// What a `Place` feature's `placement` is relative to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PlacementBase {
    /// The occurrence's placement in its parent's frame.
    Parent,
    /// A placement composed after the transform of the placed definition's
    /// own occurrence in the scope, so the occurrence's base placement is
    /// `placement` · that transform and follows an edit of it.
    OwnOccurrence,
}

/// Several occurrences placed in one feature, each as a `Place` with no
/// `placement`, `base`, or `constraints` would place it: a Fusion Component
/// Insert or `DerivedInstance` item that inserts more than one occurrence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PlaceSet {
    /// The occurrences, in source order, each once. Each is a member of the
    /// `occurrences` of a definition the scope spans, and
    /// `Occurrence.transform` holds its placement.
    pub occurrences: Vec<OccurrenceId>,
    /// Source options whose meaning is not established, and the values of
    /// typed facts whose carrier is unread.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options with no design meaning.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// Occurrences held in the placement they have relative to one another, so
/// that they move as one. The group states no pose: a member's pose in the
/// group is its `Occurrence.transform`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RigidGroup {
    /// The members, as paths from the definition that owns the group; the
    /// empty path is that definition's own origin.
    #[serde(with = "cadmpeg_core::distinct_keys::set")]
    #[cfg_attr(feature = "schema", schemars(with = "BTreeSet<OccurrencePath>"))]
    pub members: BTreeSet<OccurrencePath>,
    /// Source options whose meaning is not established, and the values of
    /// typed facts whose carrier is unread.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options with no design meaning.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// Components repeated along a part-level layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ComponentPattern {
    /// The seeds, each a `Component` pick.
    pub seeds: OrderedSelection,
    /// The layout. A component pattern that follows a pattern is `Follow`.
    pub layout: PatternLayout,
    /// The occurrences the pattern creates. The seed position is not an
    /// instance, and an index the layout counts and `instances` omits is a
    /// skipped instance.
    pub instances: Vec<ComponentInstance>,
    /// Source options whose meaning is not established, and the values of
    /// typed facts whose carrier is unread.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options with no design meaning.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// One instance of a component pattern.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ComponentInstance {
    /// The instance coordinates.
    pub index: InstanceIndex,
    /// The occurrences of the instance: `occurrences[k]` is the copy of
    /// `seeds[k]`.
    pub occurrences: Vec<OccurrenceId>,
}

/// Components mirrored across a plane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct MirrorComponents {
    /// The seeds, each a `Component` pick.
    pub seeds: OrderedSelection,
    /// The mirror plane.
    pub plane: Construction<kinds::Plane>,
    /// The results: `results[k]` is the mirror of `seeds[k]`.
    pub results: Vec<MirrorResult>,
    /// Source options whose meaning is not established, and the values of
    /// typed facts whose carrier is unread.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options with no design meaning.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// The mirror of one seed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct MirrorResult {
    /// The resulting occurrence.
    pub occurrence: OccurrenceId,
    /// The result's orientation. `None` is the opposite-hand result, the
    /// seed's placement reflected across the plane.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_orientation"
    )]
    pub orientation: Option<MirrorOrientation>,
}

/// The orientation of a mirror result that is a copy of its seed.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum MirrorOrientation {
    /// A copy of the seed, not its mirror image: the seed's placement
    /// reflected across the plane, then the seed reversed along `reversed`, an
    /// axis of the seed's own frame, through the point `about` names.
    SeedAxes {
        /// The reversed axis, or all three.
        reversed: MirrorReversal,
        /// The point the reversal is through. `None`, not stated.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_about"
        )]
        about: Option<MirrorPivot>,
    },
    /// A mode the design does not construct, with the result's stored
    /// placement.
    Native(NonBlankString),
}

/// The axis of the seed's own frame a mirror copy reverses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum MirrorReversal {
    /// The X axis.
    X,
    /// The Y axis.
    Y,
    /// The Z axis.
    Z,
    /// All three axes.
    All,
}

/// The point a mirror copy's reversal is through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum MirrorPivot {
    /// The centre of the seed model's box in the seed's own frame.
    ExtentCentre,
    /// The seed's centre of mass.
    CentreOfMass,
    /// The seed's origin.
    Origin,
}

/// An operation (an extrude or revolve cut, a hole, a split, a fillet) run in
/// the assembly over the occurrences, or bodies through occurrence contexts,
/// that it modifies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct AssemblyFeature {
    /// What the feature modifies, its picks carrying occurrence contexts: the
    /// only statement of it, since the operation states no body operand of
    /// its own.
    pub participants: SetSelection,
    /// What the feature changes. `None`, not stated.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_effect"
    )]
    pub effect: Option<ContextEffect>,
    /// The operation the feature runs.
    pub operation: Box<FeatureOperation>,
}

/// What an assembly feature changes.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ContextEffect {
    /// Only the participating instances: the result is `instance_bodies` of
    /// the assembly definition at each participant's path.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Instances,
    /// The participants' definitions: each changed definition's scope holds a
    /// resulting feature with `driven_by` naming the assembly feature.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Definitions,
    /// An effect whose meaning is not established.
    Native(NonBlankString),
}

cadmpeg_core::named_optional_field!(deserialize_about<T>, T, "about");
cadmpeg_core::named_optional_field!(deserialize_base<T>, T, "base");
cadmpeg_core::named_optional_field!(deserialize_effect<T>, T, "effect");
cadmpeg_core::named_optional_field!(deserialize_orientation<T>, T, "orientation");
cadmpeg_core::named_optional_field!(deserialize_packaged<T>, T, "packaged");
cadmpeg_core::named_optional_field!(deserialize_placement<T>, T, "placement");
cadmpeg_core::named_optional_field!(deserialize_scale<T>, T, "scale");
