// SPDX-License-Identifier: Apache-2.0
//! Leads, plunge, links, and cycle settings of an operation (§7.3).

use cadmpeg_core::text::NonBlankString;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::cycle::CycleKind;
use super::tool::{PAngle, PLength};
use super::{Choice, ProcessDimension};
use crate::design::units::Duration;
use crate::scalar::{Angle, Length};

/// How the tool enters and leaves each cut.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Leads {
    /// The lead into the cut.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_entry"
    )]
    pub entry: Option<Lead>,
    /// The lead out of the cut, which mirrors the entry.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_exit"
    )]
    pub exit: Option<Lead>,
}

/// A lead: a straight move tangent to an arc in the cutting plane, then a
/// quarter arc in the vertical plane that lands the tool on the cutting level
/// along the cut. A radius or length of 0 omits its motion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Lead {
    /// Whether the lead is made; `Some(false)` makes no lead.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_enabled"
    )]
    pub enabled: Option<ProcessDimension<bool>>,
    /// The radius of the arc in the cutting plane.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_arc_radius"
    )]
    pub arc_radius: PLength,
    /// The sweep of the arc in the cutting plane.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_arc_sweep"
    )]
    pub arc_sweep: PAngle,
    /// The length of the straight move.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_line_length"
    )]
    pub line_length: PLength,
    /// The radius of the quarter arc in the vertical plane.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_vertical_radius"
    )]
    pub vertical_radius: PLength,
    /// The straight move's direction.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_line_turn"
    )]
    pub line_turn: Option<Choice<LineTurn>>,
    /// Whether the entry starts on the axis of the hole or circle the pass
    /// cuts.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_from_axis"
    )]
    pub from_axis: Option<ProcessDimension<bool>>,
    /// Whether the lead arcs rise along the tool axis; `Some(false)` keeps
    /// them in the cutting plane.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_helical"
    )]
    pub helical: Option<ProcessDimension<bool>>,
}

/// The direction of a lead's straight move.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LineTurn {
    /// The tangent line, in line with what follows it.
    Tangent {},
    /// A line that meets what follows it at a right angle.
    Perpendicular {},
    /// A line at an undirected angle to a reference line.
    Angle {
        /// The angle to the reference line: 0° along it and 90° square to
        /// it.
        angle: ProcessDimension<Angle>,
        /// The reference line. `None` does not state it.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_from"
        )]
        from: Option<Choice<LeadAngleFrom>>,
        /// The sense of the move, read in the order the tool runs it and in
        /// the terms of `from`. `None` does not state it.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_along"
        )]
        along: Option<LeadAlong>,
    },
}

/// The reference line of a lead's angle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum LeadAngleFrom {
    /// The cut's tangent where the lead joins it.
    Cut,
    /// The spindle axis of a turning operation.
    SpindleAxis,
}

/// The sense of a lead's angled move.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum LeadAlong {
    /// With the cut's direction of travel, or toward the front along the
    /// spindle axis.
    Forward,
    /// Against the cut's direction of travel, or toward the back along the
    /// spindle axis.
    Backward,
}

/// How the tool enters each level.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PlungeStrategy {
    /// The entry move.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_plunge_strategy_kind"
    )]
    pub kind: Option<Choice<PlungeKind>>,
    /// The descent angle above the horizontal.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_angle"
    )]
    pub angle: PAngle,
    /// The helix diameter, measured at the tool centre.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_helix_diameter"
    )]
    pub helix_diameter: PLength,
    /// The angle from the tool axis at which a helical ramp narrows
    /// downward, stated only with `kind` `Helix`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_helix_taper"
    )]
    pub helix_taper: PAngle,
    /// The height above the top height at which the first entry starts.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_start_above"
    )]
    pub start_above: PLength,
}

/// The move by which the tool enters a level.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum PlungeKind {
    /// Along the tool axis.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    ToolAxis,
    /// Down a helix.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Helix,
    /// Along the cut.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Ramp,
    /// A move that no arm states, by the source's name.
    Native(NonBlankString),
}

/// How the tool moves between two passes on one level.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Linking {
    /// Whether the tool may link at depth where the strategy finds a link
    /// clear of uncut material.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_stay_down"
    )]
    pub stay_down: Option<ProcessDimension<bool>>,
    /// The longest link at depth.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_stay_down_distance"
    )]
    pub stay_down_distance: PLength,
    /// The height by which a link that stays down is raised above the
    /// cutting level.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_lift"
    )]
    pub lift: PLength,
    /// How high a retracting link lifts.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_retract"
    )]
    pub retract: Option<Choice<RetractPolicy>>,
    /// How far a turning roughing pass leaves the material at its end, at
    /// 45°.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_pull_out"
    )]
    pub pull_out: PLength,
    /// The start of a turning roughing pass.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_pass_clearance"
    )]
    pub pass_clearance: Option<PassClearance>,
}

/// The start of a turning roughing pass.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PassClearance {
    /// The distance short of the pass's radius at which the rapid stops
    /// where the pass starts against material.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_radial"
    )]
    pub radial: PLength,
    /// The distance in front of the pass's front limit at which the pass
    /// starts, the tool's nose included.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_axial"
    )]
    pub axial: PLength,
}

/// How high a retracting link lifts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum RetractPolicy {
    /// To the retract height or above.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Full,
    /// As high as the strategy needs, possibly below the retract height.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Minimum,
    /// A policy that no arm states, by the source's name.
    Native(NonBlankString),
}

/// The cycle values from which a drilling operation generates its cycles,
/// or the touch rule of a probing operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CycleSettings {
    /// The cycle kind.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_cycle_settings_kind"
    )]
    pub kind: Option<Choice<CycleKind>>,
    /// The peck increment.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_increment"
    )]
    pub increment: PLength,
    /// The reduction of each peck after the first.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_increment_reduction"
    )]
    pub increment_reduction: PLength,
    /// The shortest peck.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_minimum_increment"
    )]
    pub minimum_increment: PLength,
    /// The plunging depth after which the tool retracts fully.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_full_retract_after"
    )]
    pub full_retract_after: PLength,
    /// The distance a chip-breaking cycle retracts after each peck.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_chip_break_retract"
    )]
    pub chip_break_retract: PLength,
    /// The dwell at the bottom.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_dwell"
    )]
    pub dwell: Option<ProcessDimension<Duration>>,
    /// How far a drilling cycle goes below the operation's `Bottom` height.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_depth"
    )]
    pub depth: Option<HoleDepth>,
    /// Where a probing cycle touches.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_probe_at"
    )]
    pub probe_at: Option<Choice<ProbeAt>>,
}

/// Where a probing cycle touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ProbeAt {
    /// At the feature's nominal point.
    Nominal,
    /// At the point picked on the face.
    Picked,
}

/// How far a drilling cycle goes below the operation's `Bottom` height.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HoleDepth {
    /// Whether the cycle goes deeper by the tip length, so the full diameter
    /// reaches the bottom.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tip_through"
    )]
    pub tip_through: Option<ProcessDimension<bool>>,
    /// The further depth past the bottom.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_break_through"
    )]
    pub break_through: PLength,
}

cadmpeg_core::named_optional_field!(deserialize_along, LeadAlong, "along");
cadmpeg_core::named_optional_field!(deserialize_angle, ProcessDimension<Angle>, "angle");
cadmpeg_core::named_optional_field!(
    deserialize_arc_radius,
    ProcessDimension<Length>,
    "arc_radius"
);
cadmpeg_core::named_optional_field!(deserialize_arc_sweep, ProcessDimension<Angle>, "arc_sweep");
cadmpeg_core::named_optional_field!(deserialize_axial, ProcessDimension<Length>, "axial");
cadmpeg_core::named_optional_field!(
    deserialize_break_through,
    ProcessDimension<Length>,
    "break_through"
);
cadmpeg_core::named_optional_field!(
    deserialize_chip_break_retract,
    ProcessDimension<Length>,
    "chip_break_retract"
);
cadmpeg_core::named_optional_field!(deserialize_cycle_settings_kind, Choice<CycleKind>, "kind");
cadmpeg_core::named_optional_field!(deserialize_depth, HoleDepth, "depth");
cadmpeg_core::named_optional_field!(deserialize_dwell, ProcessDimension<Duration>, "dwell");
cadmpeg_core::named_optional_field!(deserialize_enabled, ProcessDimension<bool>, "enabled");
cadmpeg_core::named_optional_field!(deserialize_entry, Lead, "entry");
cadmpeg_core::named_optional_field!(deserialize_exit, Lead, "exit");
cadmpeg_core::named_optional_field!(deserialize_from, Choice<LeadAngleFrom>, "from");
cadmpeg_core::named_optional_field!(deserialize_from_axis, ProcessDimension<bool>, "from_axis");
cadmpeg_core::named_optional_field!(
    deserialize_full_retract_after,
    ProcessDimension<Length>,
    "full_retract_after"
);
cadmpeg_core::named_optional_field!(deserialize_helical, ProcessDimension<bool>, "helical");
cadmpeg_core::named_optional_field!(
    deserialize_helix_diameter,
    ProcessDimension<Length>,
    "helix_diameter"
);
cadmpeg_core::named_optional_field!(
    deserialize_helix_taper,
    ProcessDimension<Angle>,
    "helix_taper"
);
cadmpeg_core::named_optional_field!(deserialize_increment, ProcessDimension<Length>, "increment");
cadmpeg_core::named_optional_field!(
    deserialize_increment_reduction,
    ProcessDimension<Length>,
    "increment_reduction"
);
cadmpeg_core::named_optional_field!(deserialize_lift, ProcessDimension<Length>, "lift");
cadmpeg_core::named_optional_field!(
    deserialize_line_length,
    ProcessDimension<Length>,
    "line_length"
);
cadmpeg_core::named_optional_field!(deserialize_line_turn, Choice<LineTurn>, "line_turn");
cadmpeg_core::named_optional_field!(
    deserialize_minimum_increment,
    ProcessDimension<Length>,
    "minimum_increment"
);
cadmpeg_core::named_optional_field!(deserialize_pass_clearance, PassClearance, "pass_clearance");
cadmpeg_core::named_optional_field!(deserialize_plunge_strategy_kind, Choice<PlungeKind>, "kind");
cadmpeg_core::named_optional_field!(deserialize_probe_at, Choice<ProbeAt>, "probe_at");
cadmpeg_core::named_optional_field!(deserialize_pull_out, ProcessDimension<Length>, "pull_out");
cadmpeg_core::named_optional_field!(deserialize_radial, ProcessDimension<Length>, "radial");
cadmpeg_core::named_optional_field!(deserialize_retract, Choice<RetractPolicy>, "retract");
cadmpeg_core::named_optional_field!(
    deserialize_start_above,
    ProcessDimension<Length>,
    "start_above"
);
cadmpeg_core::named_optional_field!(deserialize_stay_down, ProcessDimension<bool>, "stay_down");
cadmpeg_core::named_optional_field!(
    deserialize_stay_down_distance,
    ProcessDimension<Length>,
    "stay_down_distance"
);
cadmpeg_core::named_optional_field!(
    deserialize_tip_through,
    ProcessDimension<bool>,
    "tip_through"
);
cadmpeg_core::named_optional_field!(
    deserialize_vertical_radius,
    ProcessDimension<Length>,
    "vertical_radius"
);
