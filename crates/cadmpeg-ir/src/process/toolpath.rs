// SPDX-License-Identifier: Apache-2.0
//! Toolpaths (§7.8).
//!
//! A toolpath is the evaluation of an operation (ISO 14649 cutter-location
//! trajectory). Motion coordinates are in the operation's work coordinate
//! system.

use cadmpeg_core::text::NonBlankString;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::cycle::Cycle;
use super::technology::{Coolant, SpindleDirection};
use crate::design::evaluation::EvaluationStatus;
use crate::design::ids::{MachiningFeatureId, OperationId, ToolpathId};
use crate::design::units::{AngularSpeed, Duration, Feed};
use crate::features::{ConfigurationId, FinitePoint3};
use crate::scalar::{FiniteReal, Fraction};
use crate::transform::Transform;
use crate::units::UnitVector3;

/// The evaluation of an operation, or of an operation and one of its
/// features where the source stores one toolpath per pair.
///
/// A toolpath's key is (`operation`, `feature`, `configuration`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Toolpath {
    /// The toolpath's identity.
    pub id: ToolpathId,
    /// The operation the toolpath evaluates.
    pub operation: OperationId,
    /// The feature of the operation the toolpath machines, where the source
    /// stores one toolpath per operation and feature.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_feature"
    )]
    pub feature: Option<MachiningFeatureId>,
    /// The configuration of the setup's scope the toolpath is of. `None`
    /// states the toolpath of every configuration.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_configuration"
    )]
    pub configuration: Option<ConfigurationId>,
    /// The status the source records for the toolpath.
    pub status: EvaluationStatus,
    /// The toolpath's motions, or its native record.
    pub content: ToolpathContent,
}

/// The content of a toolpath.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ToolpathContent {
    /// Decoded motions.
    Motions {
        /// The placement of the operation's work coordinate system in setup
        /// space: the operation's resolved frame, a cache for the toolpath.
        frame: Transform,
        /// The motions, in order.
        motions: Vec<Motion>,
    },
    /// The native record of a toolpath whose encoding the decoder does not
    /// decode.
    Native(String),
}

/// One motion or event of a toolpath.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Motion {
    /// A rapid move.
    Rapid {
        /// The target point.
        to: FinitePoint3,
        /// The tool axis of a multi-axis motion; `None` is the frame's z
        /// axis.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_axis"
        )]
        axis: Option<UnitVector3>,
        /// A rate the source states on the rapid: the rapid does not move at
        /// it, and a later motion with no feed takes it.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_feed"
        )]
        feed: Option<Feed>,
    },
    /// A linear move.
    Linear {
        /// The target point.
        to: FinitePoint3,
        /// The tool axis of a multi-axis motion; `None` is the frame's z
        /// axis.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_axis"
        )]
        axis: Option<UnitVector3>,
        /// The feed, per time; `None` means the motion states none and the
        /// previous one holds.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_feed"
        )]
        feed: Option<Feed>,
    },
    /// An arc move. An arc whose `to` leaves the plane through its start
    /// normal to `normal` is helical.
    Arc {
        /// The target point.
        to: FinitePoint3,
        /// Any point on the arc's axis.
        center: FinitePoint3,
        /// The arc's axis direction; the radius is measured perpendicular to
        /// it.
        normal: UnitVector3,
        /// Whether the arc turns clockwise about `normal`.
        clockwise: bool,
        /// The feed, per time; `None` means the motion states none and the
        /// previous one holds.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_feed"
        )]
        feed: Option<Feed>,
    },
    /// The movement class of the motions after it until the next.
    Class(MotionClass),
    /// A drilling or probing cycle.
    Cycle(Box<Cycle>),
    /// A tool change.
    ToolChange {
        /// The tool number.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_number"
        )]
        number: Option<u32>,
    },
    /// A spindle command.
    Spindle {
        /// The spindle direction.
        direction: SpindleDirection,
        /// The spindle speed.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_speed"
        )]
        speed: Option<AngularSpeed>,
    },
    /// A coolant command.
    Coolant(Coolant),
    /// A dwell.
    Dwell(Duration),
    /// A comment.
    Comment(String),
    /// A typed value the source states between motions, by its source key.
    Attribute {
        /// The source key.
        key: NonBlankString,
        /// The value.
        value: MotionValue,
    },
    /// Sets a heater to `celsius` degrees Celsius, and with `wait` holds the
    /// motion until it is reached.
    Temperature {
        /// The heater.
        heater: Heater,
        /// The temperature in degrees Celsius.
        celsius: FiniteReal,
        /// Whether the motion holds until the temperature is reached.
        wait: bool,
    },
    /// Sets the part-cooling fan to the fraction `speed` of full.
    Fan {
        /// The fraction of full speed.
        speed: Fraction,
    },
    /// The pierce that starts a jet cut, before its lead-in.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Pierce,
    /// A motion whose target the stored path does not determine in full,
    /// with its words.
    Native(NonBlankString),
}

/// A heater of an additive machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Heater {
    /// An extruder.
    Extruder {
        /// The extruder's index.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_index"
        )]
        index: Option<u32>,
    },
    /// The bed.
    Bed {},
}

/// The movement class of the motions after it until the next.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct MotionClass {
    /// The movement class.
    pub class: MovementClass,
    /// The source's code, which a writer to the same codec re-emits.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native"
    )]
    pub native: Option<NonBlankString>,
}

/// A movement class. A linear or arc motion with no feed takes the feed of
/// its class's role from the operation's technology.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum MovementClass {
    /// An approach, at the `Entry` feed.
    Approach,
    /// A cut, at the `Cutting` feed.
    Cutting,
    /// A finishing cut, at the `Finishing` feed.
    Finishing,
    /// A retreat, at the `Exit` feed.
    Retreat,
    /// A link that may leave the cutting level, at the `Transition` feed, or
    /// the `Cutting` feed where the technology states no `Transition` feed.
    Transition,
    /// A link between passes that keeps the cutting level, at the `Link`
    /// feed.
    Link,
    /// A ramp, at the `Ramp` feed.
    Ramp,
    /// A plunge, at the `Plunge` feed.
    Plunge,
    /// A reduced-feed move, at the `Reduced` feed.
    Reduced,
    /// A class that no arm states.
    Unclassified,
}

/// A typed value of a toolpath attribute or cycle parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum MotionValue {
    /// An integer.
    Integer(i64),
    /// A real number.
    Real(FiniteReal),
    /// Text.
    Text(String),
    /// A point.
    Point(FinitePoint3),
}

cadmpeg_core::named_optional_field!(deserialize_axis, UnitVector3, "axis");
cadmpeg_core::named_optional_field!(deserialize_configuration, ConfigurationId, "configuration");
cadmpeg_core::named_optional_field!(deserialize_feature, MachiningFeatureId, "feature");
cadmpeg_core::named_optional_field!(deserialize_feed, Feed, "feed");
cadmpeg_core::named_optional_field!(deserialize_index, u32, "index");
cadmpeg_core::named_optional_field!(deserialize_native, NonBlankString, "native");
cadmpeg_core::named_optional_field!(deserialize_number, u32, "number");
cadmpeg_core::named_optional_field!(deserialize_speed, AngularSpeed, "speed");
