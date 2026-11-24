// SPDX-License-Identifier: Apache-2.0
//! Operations and modifiers, their classes, tooling, tool axis control, and
//! geometry (§7.3).
//!
//! An operation is one machining step with one tool, or a modifier of other
//! operations' toolpaths. Its limits are in [`super::limits`], and its leads,
//! plunge, links, and cycle settings in [`super::settings`].

use std::collections::BTreeMap;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::limits::{AxialLimit, AxialRole, Height, HeightRole, RadialLimit, RadiusRole};
use super::settings::{CycleSettings, Leads, Linking, PlungeStrategy};
use super::stock::Wcs;
use super::technology::Technology;
use super::{Choice, ProcessConstruction, ProcessDimension, WorkpieceSelection};
use crate::design::construction::{kinds, Construction};
use crate::design::ids::{MachiningFeatureId, OperationId, PresetId, ProcessGroupId, ToolId};
use crate::design::names::NativeName;
use crate::design::ops::sketch::BoundedCurve;
use crate::design::selection::OrderedSelection;
use crate::features::ConfigurationId;

/// One machining step with one tool, or a modifier of other operations'
/// toolpaths.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Operation {
    /// The operation's identity.
    pub id: OperationId,
    /// The source's name.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_name"
    )]
    pub name: Option<String>,
    /// The source's comment.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_comment"
    )]
    pub comment: Option<String>,
    /// A machining operation with its class, or a modifier with its inputs.
    pub kind: OperationKind,
    /// The source's operation type, verbatim.
    pub native_kind: NativeName,
    /// The tool or preset the operation uses. `None` is an operation with no
    /// tool; a modifier with no tooling uses its first input's.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tooling"
    )]
    pub tooling: Option<Tooling>,
    /// The machining features the operation machines.
    pub features: Vec<MachiningFeatureId>,
    /// The groups the operation inherits from, at most one of each kind.
    pub groups: Vec<ProcessGroupId>,
    /// What the operation machines or avoids, by role.
    pub geometry: Vec<ProcessGeometry>,
    /// The levels along the tool axis of the operation's frame, by role.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub heights: BTreeMap<HeightRole, Height>,
    /// The radial limits of a turning operation about the spindle axis, by
    /// role.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub radii: BTreeMap<RadiusRole, Choice<RadialLimit>>,
    /// The axial limits of a turning operation along the spindle axis, by
    /// role.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub axial: BTreeMap<AxialRole, AxialLimit>,
    /// The feeds, speeds, allowances, and cut mode.
    pub technology: Technology,
    /// The operation's own tool orientation, a frame rule of the setup WCS's
    /// form. `None` uses the setup's WCS.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tool_frame"
    )]
    pub tool_frame: Option<Wcs>,
    /// How the tool axis is controlled through the operation. `None` is a
    /// fixed axis, the operation's tool frame.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tool_axis"
    )]
    pub tool_axis: Option<ToolAxisControl>,
    /// How the tool enters and leaves each cut.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_leads"
    )]
    pub leads: Option<Leads>,
    /// How the tool enters each level.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_plunge"
    )]
    pub plunge: Option<PlungeStrategy>,
    /// How the tool moves between two passes on one level.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_linking"
    )]
    pub linking: Option<Linking>,
    /// The cycle values from which a drilling or probing operation generates
    /// its cycles.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_cycle"
    )]
    pub cycle: Option<CycleSettings>,
    /// Whether the operation is suppressed.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_suppressed"
    )]
    pub suppressed: Option<ProcessDimension<bool>>,
    /// The suppression of the operation per configuration of the setup's
    /// scope; `true` is suppressed.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub configuration_states: BTreeMap<ConfigurationId, bool>,
    /// The source's optional-operation flag.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional"
    )]
    pub optional: Option<ProcessDimension<bool>>,
    /// The source's own reference to its record.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// A machining operation or a modifier.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum OperationKind {
    /// A machining operation.
    Machining {
        /// The ISO 14649 operation class.
        class: OperationClass,
        /// The rough and finish split, `None` where the source does not
        /// state it.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_phase"
        )]
        phase: Option<Phase>,
    },
    /// A modifier: it takes the toolpaths of its inputs, in order, and
    /// produces its own.
    Modifier {
        /// What the modifier does.
        modifier: ModifierKind,
        /// The operations whose toolpaths the modifier takes, in order. An
        /// input may be a modifier.
        inputs: Vec<OperationId>,
    },
}

/// The rough and finish split of an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Phase {
    /// Roughing.
    Rough,
    /// Semi-finishing.
    SemiFinish,
    /// Finishing.
    Finish,
}

/// The operation class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum OperationClass {
    /// A milling operation.
    Milling(MillingClass),
    /// A drilling operation.
    Drilling(DrillingClass),
    /// A turning operation.
    Turning(TurningClass),
    /// Touch probing.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Probing,
    /// Thread milling.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    ThreadMilling,
    /// Engraving.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Engraving,
    /// Chamfering.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Chamfering,
    /// Jet cutting.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Jet,
    /// Wire EDM.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    WireEdm,
    /// Additive manufacturing.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Additive,
    /// A class that no arm states; the operation's `native_kind` holds the
    /// source's name.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Unclassified,
}

/// The ISO 14649 milling classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum MillingClass {
    /// Plane milling.
    Plane,
    /// Side milling.
    Side,
    /// Bottom and side milling.
    BottomAndSide,
    /// Freeform milling.
    Freeform,
}

/// The ISO 14649 drilling classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DrillingClass {
    /// Drilling.
    Drilling,
    /// Center drilling.
    CenterDrilling,
    /// Countersinking.
    Countersinking,
    /// Multistep drilling.
    Multistep,
    /// Boring.
    Boring,
    /// Reaming.
    Reaming,
    /// Back boring.
    BackBoring,
    /// Tapping.
    Tapping,
    /// Thread drilling.
    ThreadDrilling,
}

/// The ISO 14649 turning classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TurningClass {
    /// Facing.
    Facing,
    /// Contouring.
    Contouring,
    /// Grooving.
    Grooving,
    /// Cutting in.
    CuttingIn,
    /// Threading.
    Threading,
    /// Knurling.
    Knurling,
}

/// What a modifier does to its inputs' toolpaths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ModifierKind {
    /// Adds leads in and out.
    LeadInOut,
    /// Adds ramps.
    Ramp,
    /// Adds tags.
    Tags,
    /// Adds dogbone corners.
    Dogbone,
    /// Trims the toolpaths.
    Trim,
    /// Bounds the toolpaths.
    Boundary,
    /// Repeats the toolpaths in a pattern.
    Pattern,
    /// Transforms the toolpaths.
    Transform,
    /// Maps the toolpaths onto another axis.
    AxisMap,
    /// Adapts the toolpaths to a drag knife.
    DragKnife,
    /// Corrects the toolpaths by probing.
    ProbeCorrection,
    /// A modifier that no arm states; the operation's `native_kind` holds
    /// the source's name.
    Unclassified,
}

/// The tool an operation uses.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Tooling {
    /// A tool used with no stated preset.
    Tool(ToolId),
    /// A preset or tool controller, which names its tool.
    Preset(PresetId),
}

/// How the tool axis is controlled through an operation.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ToolAxisControl {
    /// The tool axis stays parallel to the picked plane, so the rotary axis
    /// is the plane's normal.
    InPlane {
        /// The plane.
        plane: WorkpieceSelection,
    },
    /// Every position's tool axis meets the picked line at the point of the
    /// line nearest the tool's contact point on the part.
    ThroughLine {
        /// The line.
        line: WorkpieceSelection,
    },
    /// The tool axis is the picked surfaces' normal at the contact point.
    NormalToSurface {
        /// The surfaces.
        surfaces: WorkpieceSelection,
    },
    /// The tool's side runs along the rulings of the picked ruled surfaces,
    /// its axis one tool radius off them.
    AlongRuling {
        /// The ruled surfaces.
        surfaces: WorkpieceSelection,
    },
    /// A control that no arm states, by the source's instruction codes.
    Native(NonBlankString),
}

/// Geometry that an operation machines or avoids, by role.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ProcessGeometry {
    /// What the geometry is for.
    pub role: GeometryRole,
    /// The geometry.
    pub source: GeometrySource,
    /// The source's input name where it has several of one role. A
    /// `Reference` geometry is named by its slot.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_slot"
    )]
    pub slot: Option<NonBlankString>,
    /// Whether the geometry replaces or extends its group's geometry of the
    /// same role. `None` states that the source does not say which.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_combine"
    )]
    pub combine: Option<GeometryCombine>,
}

/// The role of an operation's geometry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum GeometryRole {
    /// The drive geometry: contours, pockets, faces, holes.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Machine,
    /// A boundary of the cut.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Boundary,
    /// Geometry the tool avoids.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Check,
    /// Picks the operation removes from its drive geometry.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Exclude,
    /// A start point the operation uses.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Start,
    /// Geometry that orients the tool.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Orientation,
    /// Geometry that a limit, a height, or a radial base names by `slot`.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Reference,
    /// A role that no arm states, by the source's name.
    Native(NonBlankString),
}

/// Whether an operation's geometry replaces or extends its group's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum GeometryCombine {
    /// Replaces the group's geometry of the role.
    Replace,
    /// Adds to the group's geometry of the role.
    Extend,
}

/// The geometry of a [`ProcessGeometry`].
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum GeometrySource {
    /// One selection per workpiece selected.
    Selection(Vec<WorkpieceSelection>),
    /// Points, each a process construction; a fixed point is a construction
    /// with no inputs.
    Points(Vec<ProcessConstruction<kinds::Point>>),
    /// A direction.
    Direction(ProcessConstruction<kinds::Direction>),
    /// An axis.
    Axis(ProcessConstruction<kinds::Axis>),
    /// Planar geometry the operation owns, as solved curves with no
    /// constraints.
    Profile {
        /// The profile's plane.
        plane: Construction<kinds::Frame>,
        /// The profile's curves.
        curves: Vec<ProfileCurve>,
    },
}

/// A solved curve of an operation's profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ProfileCurve {
    /// The curve. `None` where the decoder did not read its carrier.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_geometry"
    )]
    pub geometry: Option<BoundedCurve>,
    /// The model entities the curve was taken from, where the source records
    /// them.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_source"
    )]
    pub source: Option<OrderedSelection>,
}

cadmpeg_core::named_optional_field!(deserialize_combine, GeometryCombine, "combine");
cadmpeg_core::named_optional_field!(deserialize_comment, String, "comment");
cadmpeg_core::named_optional_field!(deserialize_cycle, CycleSettings, "cycle");
cadmpeg_core::named_optional_field!(deserialize_geometry, BoundedCurve, "geometry");
cadmpeg_core::named_optional_field!(deserialize_leads, Leads, "leads");
cadmpeg_core::named_optional_field!(deserialize_linking, Linking, "linking");
cadmpeg_core::named_optional_field!(deserialize_name, String, "name");
cadmpeg_core::named_optional_field!(deserialize_native_ref, String, "native_ref");
cadmpeg_core::named_optional_field!(deserialize_optional, ProcessDimension<bool>, "optional");
cadmpeg_core::named_optional_field!(deserialize_phase, Phase, "phase");
cadmpeg_core::named_optional_field!(deserialize_plunge, PlungeStrategy, "plunge");
cadmpeg_core::named_optional_field!(deserialize_slot, NonBlankString, "slot");
cadmpeg_core::named_optional_field!(deserialize_source, OrderedSelection, "source");
cadmpeg_core::named_optional_field!(deserialize_suppressed, ProcessDimension<bool>, "suppressed");
cadmpeg_core::named_optional_field!(deserialize_tool_axis, ToolAxisControl, "tool_axis");
cadmpeg_core::named_optional_field!(deserialize_tool_frame, Wcs, "tool_frame");
cadmpeg_core::named_optional_field!(deserialize_tooling, Tooling, "tooling");
