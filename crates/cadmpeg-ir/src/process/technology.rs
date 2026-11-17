// SPDX-License-Identifier: Apache-2.0
//! Technology: feeds, speeds, allowances, cut mode, steps, and passes
//! (§7.5).
//!
//! Operations and presets each hold one. An operation value left `None`
//! takes its preset's value. A preset value left `None` is unstated, and a
//! writer uses the target default.

use std::collections::BTreeMap;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{Choice, ProcessConstruction, ProcessDimension};
use crate::design::construction::kinds;
use crate::design::dimension::ParamRef;
use crate::design::ops::shared::RadialDimension;
use crate::design::options::Hand;
use crate::design::units::{AngularSpeed, Feed};
use crate::scalar::{Angle, Length};

/// The feeds, speeds, allowances, and cut mode of an operation or a preset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Technology {
    /// The feeds by role, each in the form the source authors.
    #[serde(with = "cadmpeg_core::distinct_keys::pairs")]
    #[cfg_attr(
        feature = "schema",
        schemars(with = "std::collections::BTreeSet<(FeedRole, FeedRate)>")
    )]
    pub feeds: BTreeMap<FeedRole, FeedRate>,
    /// The spindle speed.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_spindle"
    )]
    pub spindle: Option<Spindle>,
    /// The spindle's turning direction.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_spindle_direction"
    )]
    pub spindle_direction: Option<Choice<SpindleDirection>>,
    /// The coolant mode.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_coolant"
    )]
    pub coolant: Option<Choice<Coolant>>,
    /// The distance between adjacent passes, normal to the pass direction.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_stepover"
    )]
    pub stepover: Option<ProcessDimension<Length>>,
    /// The largest step between cutting levels.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_stepdown"
    )]
    pub stepdown: Option<ProcessDimension<Length>>,
    /// The radial stock to leave.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_stock_to_leave_radial"
    )]
    pub stock_to_leave_radial: Option<ProcessDimension<Length>>,
    /// The axial stock to leave.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_stock_to_leave_axial"
    )]
    pub stock_to_leave_axial: Option<ProcessDimension<Length>>,
    /// The machining tolerance.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tolerance"
    )]
    pub tolerance: Option<ProcessDimension<Length>>,
    /// The side of the tool the material is cut on as it follows a contour.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_cut_mode"
    )]
    pub cut_mode: Option<Choice<CutMode>>,
    /// How the cutting levels are placed from the top height down to the
    /// bottom.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_stepdown_rule"
    )]
    pub stepdown_rule: Option<Choice<StepRule>>,
    /// How the passes are spaced across the width.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_stepover_rule"
    )]
    pub stepover_rule: Option<Choice<StepRule>>,
    /// Whether the operation cuts several levels; `Some(false)` cuts one.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_multiple_depths"
    )]
    pub multiple_depths: Option<ProcessDimension<bool>>,
    /// The finishing levels at the bottom.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_finishing_stepdown"
    )]
    pub finishing_stepdown: Option<FinishingSteps>,
    /// The finishing passes, cut from the outermost in.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_finishing_stepover"
    )]
    pub finishing_stepover: Option<FinishingSteps>,
    /// The side of the drive contour that the finishing pass runs on.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_compensation"
    )]
    pub compensation: Option<Choice<Compensation>>,
    /// Whether the final pass is cut a second time on the same path.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_repeat_final_pass"
    )]
    pub repeat_final_pass: Option<ProcessDimension<bool>>,
    /// The length by which a closed finishing pass continues past its start.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_closed_pass_overlap"
    )]
    pub closed_pass_overlap: Option<ProcessDimension<Length>>,
    /// How the junctions inside a cutting pass are cut.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_corners"
    )]
    pub corners: Option<Choice<CornerMode>>,
    /// Whether the roughing passes are cut.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_roughing_passes"
    )]
    pub roughing_passes: Option<ProcessDimension<bool>>,
    /// Whether the finishing passes are cut.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_finishing_passes"
    )]
    pub finishing_passes: Option<ProcessDimension<bool>>,
    /// The move from one pass to the next.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_pass_transition"
    )]
    pub pass_transition: Option<Choice<PassTransition>>,
    /// Whether the strategy may cut moves at the `Reduced` feed;
    /// `Some(false)` cuts none.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_reduce_feed"
    )]
    pub reduce_feed: Option<ProcessDimension<bool>>,
    /// The direction of parallel passes in the operation frame's XY plane,
    /// from its X axis.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_pass_angle"
    )]
    pub pass_angle: Option<ProcessDimension<Angle>>,
    /// The number of roughing passes on each level, spaced by `stepover`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_roughing_pass_count"
    )]
    pub roughing_pass_count: Option<ProcessDimension<u32>>,
    /// The bridges of material left under a profile.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tabs"
    )]
    pub tabs: Option<Tabs>,
    /// A chamfer cut along the machined edge with a tapered tool.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_chamfer"
    )]
    pub chamfer: Option<ChamferCut>,
    /// How a jet operation cuts.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_jet_cut"
    )]
    pub jet_cut: Option<Choice<JetCut>>,
    /// The material an earlier stage left, to which the cut is limited.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_rest"
    )]
    pub rest: Option<Choice<RestSource>>,
    /// A helical pass's rise per turn.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_helix_rise"
    )]
    pub helix_rise: Option<Choice<HelixRise>>,
    /// The thread's hand.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_thread_hand"
    )]
    pub thread_hand: Option<Choice<Hand>>,
    /// The thread's depth, on the radius or on the diameter.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_thread_depth"
    )]
    pub thread_depth: Option<RadialDimension<Length, ParamRef>>,
    /// The angle that sets each turning thread pass's start further back
    /// along the axis by its radial infeed times the angle's tangent.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_thread_infeed_angle"
    )]
    pub thread_infeed_angle: Option<ProcessDimension<Angle>>,
    /// The angle between neighbouring radial passes about their centre.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_angular_stepover"
    )]
    pub angular_stepover: Option<ProcessDimension<Angle>>,
    /// The move of the pass along the tool axis, positive away from the
    /// part, after any chamfer depth.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_axial_offset"
    )]
    pub axial_offset: Option<ProcessDimension<Length>>,
    /// The spacing of the levels added between roughing levels.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_intermediate_stepdown"
    )]
    pub intermediate_stepdown: Option<ProcessDimension<Length>>,
    /// The number of levels above the final one, each `stepdown` above the
    /// next.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_stepdown_count"
    )]
    pub stepdown_count: Option<ProcessDimension<u32>>,
    /// Whether the levels are cut from the lowest up.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_bottom_up"
    )]
    pub bottom_up: Option<ProcessDimension<bool>>,
    /// Whether a turning pass cuts the outside of the part or the inside.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_turning_side"
    )]
    pub turning_side: Option<Choice<TurningSide>>,
}

/// The role of a feed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum FeedRole {
    /// The cutting feed.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Cutting,
    /// The plunge feed.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Plunge,
    /// The ramp feed.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Ramp,
    /// The feed into the cut.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Entry,
    /// The feed out of the cut.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Exit,
    /// The retract feed.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Retract,
    /// The approach feed.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Approach,
    /// The departure feed.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Departure,
    /// The feed of a direct link between passes that keeps the cutting
    /// level.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Link,
    /// The feed of a link that may leave the cutting level. A technology
    /// that states none feeds those links at its `Cutting` feed.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Transition,
    /// The feed of the finishing movement class.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Finishing,
    /// The feed of the reduced movement class.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Reduced,
    /// A role that no arm states, by the source's name.
    Native(NonBlankString),
}

/// A feed: per time, per spindle revolution, or per tooth.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum FeedRate {
    /// A feed per time.
    PerTime(ProcessDimension<Feed>),
    /// A feed per spindle revolution.
    PerRevolution(ProcessDimension<Length>),
    /// A feed per tooth.
    PerTooth(ProcessDimension<Length>),
}

/// The spindle speed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Spindle {
    /// A spindle speed.
    Rotation(ProcessDimension<AngularSpeed>),
    /// A constant cutting speed with an optional speed limit.
    SurfaceSpeed {
        /// The cutting speed.
        speed: ProcessDimension<Feed>,
        /// The spindle speed limit.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_limit"
        )]
        limit: Option<ProcessDimension<AngularSpeed>>,
    },
}

/// The spindle's turning direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum SpindleDirection {
    /// Clockwise.
    Clockwise,
    /// Counterclockwise.
    CounterClockwise,
    /// Stopped.
    Off,
}

/// The coolant mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Coolant {
    /// No coolant.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Off,
    /// Flood coolant.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Flood,
    /// Mist coolant.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Mist,
    /// Air.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Air,
    /// Coolant through the tool.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    ThroughTool,
    /// A mode that no arm states, by the source's name.
    Native(NonBlankString),
}

/// The side of the tool the material is cut on as it follows a contour.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum CutMode {
    /// The material on the tool's right with a clockwise spindle, and on its
    /// left with a counterclockwise spindle.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Climb,
    /// The other way from `Climb`.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Conventional,
    /// Successive passes in opposite directions.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Both,
    /// A mode that no arm states, by the source's name.
    Native(NonBlankString),
}

/// How steps are placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum StepRule {
    /// Full steps, the remainder last.
    Fixed,
    /// The fewest equal steps no larger than the step.
    Even,
    /// Passes spaced so that the tool's engagement in material holds at the
    /// stepover.
    ConstantEngagement,
}

/// Finishing levels or passes: `count` of them, `step` apart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FinishingSteps {
    /// The number of finishing steps.
    pub count: ProcessDimension<u32>,
    /// The distance between finishing steps.
    pub step: ProcessDimension<Length>,
}

/// The side of the drive contour that the finishing pass runs on, seen along
/// the travel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Compensation {
    /// The tool centre offset to the left by the tool radius plus the radial
    /// stock to leave.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Left,
    /// The tool centre offset to the right by the tool radius plus the
    /// radial stock to leave.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Right,
    /// On the contour.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Center,
    /// A compensation that no arm states, by the source's name.
    Native(NonBlankString),
}

/// How the junctions inside a cutting pass are cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CornerMode {
    /// Left as corners.
    Sharp,
    /// Rounded tangent.
    Rounded,
}

/// The move from one pass to the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PassTransition {
    /// An arc.
    Arc,
    /// A line.
    Line,
    /// No transition move: the tool lifts and moves in rapid.
    None,
}

/// Bridges of material left under a profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Tabs {
    /// The height above the floor of the lowest level to which the tool
    /// rises over each tab.
    pub height: ProcessDimension<Length>,
    /// The tab's width.
    pub width: ProcessDimension<Length>,
    /// The tab's shape.
    pub shape: Choice<TabShape>,
    /// Where the tabs are put.
    pub placement: TabPlacement,
}

/// The shape of a tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TabShape {
    /// The tool rises vertically and stays raised over the width plus the
    /// tool diameter.
    Rectangular,
    /// A triangle of material of base `width` and height `height`.
    Triangular,
}

/// Where tabs are put.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum TabPlacement {
    /// A number of tabs on each closed contour, evenly spaced.
    PerContour(ProcessDimension<u32>),
    /// One tab per point, centred where the tool centre passes the point.
    AtPoints(Vec<ProcessConstruction<kinds::Point>>),
    /// Tabs on each straight edge at a spacing, with equal gaps and a half
    /// gap at each end.
    Spacing(ProcessDimension<Length>),
    /// A placement that no arm states, by the source's name.
    Native(NonBlankString),
}

/// A chamfer cut along the machined edge with a tapered tool, whose tip runs
/// `width` times the cotangent of the tool's taper, plus `tip_offset`, below
/// the edge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ChamferCut {
    /// The chamfer's width.
    pub width: ProcessDimension<Length>,
    /// The further depth of the tool's tip.
    pub tip_offset: ProcessDimension<Length>,
}

/// How a jet operation cuts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum JetCut {
    /// Pierces the sheet before each cut.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Through,
    /// Marks the surface without piercing.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Mark,
    /// A mode that no arm states, by the source's name.
    Native(NonBlankString),
}

/// The material an earlier stage left, to which the cut is limited.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum RestSource {
    /// The material every earlier operation of the setup left on the setup's
    /// stock.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    EarlierOperations,
    /// The setup's stock with no operation removed.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    SetupStock,
    /// A source that no arm states, by the source's name.
    Native(NonBlankString),
}

/// A helical pass's rise per turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum HelixRise {
    /// The rise per turn.
    Pitch(ProcessDimension<Length>),
    /// The rise as (2r + D)·tan(angle), r the helix radius at the tool's
    /// centre and D the tool's diameter.
    Angle(ProcessDimension<Angle>),
}

/// The side of the part a turning pass cuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TurningSide {
    /// The outside of the part.
    Outside,
    /// The inside of the part: a bore.
    Inside,
}

cadmpeg_core::named_optional_field!(
    deserialize_angular_stepover,
    ProcessDimension<Angle>,
    "angular_stepover"
);
cadmpeg_core::named_optional_field!(
    deserialize_axial_offset,
    ProcessDimension<Length>,
    "axial_offset"
);
cadmpeg_core::named_optional_field!(deserialize_bottom_up, ProcessDimension<bool>, "bottom_up");
cadmpeg_core::named_optional_field!(deserialize_chamfer, ChamferCut, "chamfer");
cadmpeg_core::named_optional_field!(
    deserialize_closed_pass_overlap,
    ProcessDimension<Length>,
    "closed_pass_overlap"
);
cadmpeg_core::named_optional_field!(
    deserialize_compensation,
    Choice<Compensation>,
    "compensation"
);
cadmpeg_core::named_optional_field!(deserialize_coolant, Choice<Coolant>, "coolant");
cadmpeg_core::named_optional_field!(deserialize_corners, Choice<CornerMode>, "corners");
cadmpeg_core::named_optional_field!(deserialize_cut_mode, Choice<CutMode>, "cut_mode");
cadmpeg_core::named_optional_field!(
    deserialize_finishing_passes,
    ProcessDimension<bool>,
    "finishing_passes"
);
cadmpeg_core::named_optional_field!(
    deserialize_finishing_stepdown,
    FinishingSteps,
    "finishing_stepdown"
);
cadmpeg_core::named_optional_field!(
    deserialize_finishing_stepover,
    FinishingSteps,
    "finishing_stepover"
);
cadmpeg_core::named_optional_field!(deserialize_helix_rise, Choice<HelixRise>, "helix_rise");
cadmpeg_core::named_optional_field!(
    deserialize_intermediate_stepdown,
    ProcessDimension<Length>,
    "intermediate_stepdown"
);
cadmpeg_core::named_optional_field!(deserialize_jet_cut, Choice<JetCut>, "jet_cut");
cadmpeg_core::named_optional_field!(deserialize_limit, ProcessDimension<AngularSpeed>, "limit");
cadmpeg_core::named_optional_field!(
    deserialize_multiple_depths,
    ProcessDimension<bool>,
    "multiple_depths"
);
cadmpeg_core::named_optional_field!(
    deserialize_pass_angle,
    ProcessDimension<Angle>,
    "pass_angle"
);
cadmpeg_core::named_optional_field!(
    deserialize_pass_transition,
    Choice<PassTransition>,
    "pass_transition"
);
cadmpeg_core::named_optional_field!(
    deserialize_reduce_feed,
    ProcessDimension<bool>,
    "reduce_feed"
);
cadmpeg_core::named_optional_field!(
    deserialize_repeat_final_pass,
    ProcessDimension<bool>,
    "repeat_final_pass"
);
cadmpeg_core::named_optional_field!(deserialize_rest, Choice<RestSource>, "rest");
cadmpeg_core::named_optional_field!(
    deserialize_roughing_pass_count,
    ProcessDimension<u32>,
    "roughing_pass_count"
);
cadmpeg_core::named_optional_field!(
    deserialize_roughing_passes,
    ProcessDimension<bool>,
    "roughing_passes"
);
cadmpeg_core::named_optional_field!(deserialize_spindle, Spindle, "spindle");
cadmpeg_core::named_optional_field!(
    deserialize_spindle_direction,
    Choice<SpindleDirection>,
    "spindle_direction"
);
cadmpeg_core::named_optional_field!(deserialize_stepdown, ProcessDimension<Length>, "stepdown");
cadmpeg_core::named_optional_field!(
    deserialize_stepdown_count,
    ProcessDimension<u32>,
    "stepdown_count"
);
cadmpeg_core::named_optional_field!(deserialize_stepdown_rule, Choice<StepRule>, "stepdown_rule");
cadmpeg_core::named_optional_field!(deserialize_stepover, ProcessDimension<Length>, "stepover");
cadmpeg_core::named_optional_field!(deserialize_stepover_rule, Choice<StepRule>, "stepover_rule");
cadmpeg_core::named_optional_field!(
    deserialize_stock_to_leave_axial,
    ProcessDimension<Length>,
    "stock_to_leave_axial"
);
cadmpeg_core::named_optional_field!(
    deserialize_stock_to_leave_radial,
    ProcessDimension<Length>,
    "stock_to_leave_radial"
);
cadmpeg_core::named_optional_field!(deserialize_tabs, Tabs, "tabs");
cadmpeg_core::named_optional_field!(deserialize_thread_depth, RadialDimension<Length, ParamRef>, "thread_depth");
cadmpeg_core::named_optional_field!(deserialize_thread_hand, Choice<Hand>, "thread_hand");
cadmpeg_core::named_optional_field!(
    deserialize_thread_infeed_angle,
    ProcessDimension<Angle>,
    "thread_infeed_angle"
);
cadmpeg_core::named_optional_field!(deserialize_tolerance, ProcessDimension<Length>, "tolerance");
cadmpeg_core::named_optional_field!(
    deserialize_turning_side,
    Choice<TurningSide>,
    "turning_side"
);
