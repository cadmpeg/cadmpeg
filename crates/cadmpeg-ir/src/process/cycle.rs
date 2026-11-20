// SPDX-License-Identifier: Apache-2.0
//! Drilling and probing cycles of a toolpath (§7.8).
//!
//! One cycle holds one point; a writer joins consecutive cycles with equal
//! parameters. A cycle's levels are coordinates along the z axis of the
//! motion frame, and its point a point of that frame.

use std::collections::BTreeMap;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::technology::FeedRole;
use super::toolpath::{Motion, MotionValue};
use crate::design::construction::FrameAxis;
use crate::design::units::{Duration, Feed};
use crate::features::FinitePoint3;
use crate::scalar::{Angle, Length};

/// A drilling or probing cycle at one point.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Cycle {
    /// The cycle's kind.
    pub kind: CycleKind,
    /// The cycle point: the hole's x and y, and as z the level of the hole's
    /// bottom, where the cycle stops feeding. For one probed face or a
    /// corner, the start point `probe_clearance` plus the tip radius from
    /// the face.
    pub at: FinitePoint3,
    /// The level the tool is at when the cycle starts, which
    /// [`CycleReturn::Initial`] returns to. `None` where the motion before
    /// the cycle holds it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_clearance"
    )]
    pub clearance: Option<Length>,
    /// The level the tool rapids to before it feeds, which
    /// [`CycleReturn::Retract`] returns to.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_retract"
    )]
    pub retract: Option<Length>,
    /// The level of the stock's top at the hole.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_stock"
    )]
    pub stock: Option<Length>,
    /// The hole's depth below `stock`, a positive distance, where the source
    /// stores it beside the cycle point. It equals `stock` minus the z of
    /// `at`, and is a cache of the two.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_depth"
    )]
    pub depth: Option<Length>,
    /// The cycle's feeds by role.
    #[serde(with = "cadmpeg_core::distinct_keys::pairs")]
    #[cfg_attr(
        feature = "schema",
        schemars(with = "std::collections::BTreeSet<(FeedRole, Feed)>")
    )]
    pub feeds: BTreeMap<FeedRole, Feed>,
    /// The dwell at the bottom.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_dwell"
    )]
    pub dwell: Option<Duration>,
    /// A pecking cycle's peck depth, a distance.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_increment"
    )]
    pub increment: Option<Length>,
    /// The amount by which each peck after the first is shorter than the one
    /// before.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_increment_reduction"
    )]
    pub increment_reduction: Option<Length>,
    /// The length that no peck is shorter than.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_minimum_increment"
    )]
    pub minimum_increment: Option<Length>,
    /// The total plunging depth after which the tool retracts fully to the
    /// retract level.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_full_retract_after"
    )]
    pub full_retract_after: Option<Length>,
    /// The distance a chip-breaking cycle retracts after each peck.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_chip_break_retract"
    )]
    pub chip_break_retract: Option<Length>,
    /// The level the cycle returns to.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_return_to"
    )]
    pub return_to: Option<CycleReturn>,
    /// The distance from the face at which a probe starts, less the tip
    /// radius.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_probe_clearance"
    )]
    pub probe_clearance: Option<Length>,
    /// How far past the expected contact a probe travels before it reports
    /// no contact.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_probe_overtravel"
    )]
    pub probe_overtravel: Option<Length>,
    /// Every other value of the cycle, by the source's name.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub parameters: BTreeMap<NonBlankString, MotionValue>,
    /// The expanded motion the source stores beside the cycle, a cache of
    /// the cycle.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_expansion"
    )]
    pub expansion: Option<Vec<Motion>>,
}

/// The kind of a cycle.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum CycleKind {
    /// A drilling cycle.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Drill,
    /// A pecking cycle with full retract.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Deep,
    /// A pecking cycle without full retract.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    ChipBreak,
    /// A boring cycle that feeds out.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Bore,
    /// A reaming cycle.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Ream,
    /// A tapping cycle.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Tap,
    /// A turning groove cycle.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Groove,
    /// A probing cycle, with the nominal faces it measures.
    Probe(ProbeTarget),
    /// A cycle of a kind no arm states, by the source's name.
    Native(NonBlankString),
}

/// The nominal faces that a probing cycle measures.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ProbeTarget {
    /// One face along `axis`, approached along it, or against it where
    /// `reversed`.
    Face {
        /// The axis the face is along.
        axis: FrameAxis,
        /// Whether the probe approaches against the axis.
        reversed: bool,
    },
    /// Two outward faces `width` apart across `axis`.
    Wall {
        /// The axis across the faces.
        axis: FrameAxis,
        /// The distance between the faces.
        width: Length,
    },
    /// Two inward faces `width` apart across `axis`, over a raised island
    /// where `island`.
    Channel {
        /// The axis across the faces.
        axis: FrameAxis,
        /// The distance between the faces.
        width: Length,
        /// Whether a raised island stands between the faces.
        island: bool,
    },
    /// Two pairs of inward faces, `widths` along x and y, over a raised
    /// island where `island`.
    RectangularPocket {
        /// The distances between the faces along x and y.
        widths: [Length; 2],
        /// Whether a raised island stands in the pocket.
        island: bool,
    },
    /// Two pairs of outward faces, `widths` along x and y.
    RectangularBoss {
        /// The distances between the faces along x and y.
        widths: [Length; 2],
    },
    /// An inward cylinder of `diameter`, over a raised island where
    /// `island`.
    CircularPocket {
        /// The cylinder's diameter.
        diameter: Length,
        /// Whether a raised island stands in the pocket.
        island: bool,
    },
    /// An outward cylinder of `diameter`.
    CircularBoss {
        /// The cylinder's diameter.
        diameter: Length,
    },
    /// An inner or an outer corner of two faces, approached along x and y,
    /// or against them where `reversed`.
    Corner {
        /// Whether the corner is inner.
        inner: bool,
        /// Whether the probe approaches against x and against y.
        reversed: [bool; 2],
    },
    /// One face touched twice, approached along `axis`, or against it where
    /// `reversed`, on two lines `spacing` apart across the approach. It
    /// measures the angle of the line through the two touches from +x,
    /// counter-clockwise positive, against `nominal`.
    FaceAngle {
        /// The axis of approach.
        axis: PlaneAxis,
        /// Whether the probe approaches against the axis.
        reversed: bool,
        /// The distance between the two touch lines, half of it to each side
        /// of the cycle point along the other axis of the frame's xy plane.
        spacing: Length,
        /// The nominal angle; `None` is 90° for `X` and 0° for `Y`.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_nominal"
        )]
        nominal: Option<Angle>,
    },
    /// An inward circle of `diameter` about the cycle point, touched at
    /// three points in the directions `angles` from +x, counter-clockwise
    /// positive, over a raised island where `island`.
    PartialCircularPocket {
        /// The circle's diameter.
        diameter: Length,
        /// Whether a raised island stands in the pocket.
        island: bool,
        /// The directions of the three touches.
        angles: [Angle; 3],
    },
    /// An outward circle of `diameter` about the cycle point, touched at
    /// three points in the directions `angles` from +x, counter-clockwise
    /// positive.
    PartialCircularBoss {
        /// The circle's diameter.
        diameter: Length,
        /// The directions of the three touches.
        angles: [Angle; 3],
    },
    /// A probing kind no arm states, by the source's name; its values are in
    /// the cycle's `parameters`.
    Native(NonBlankString),
}

/// An axis of the motion frame's xy plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PlaneAxis {
    /// The x axis.
    X,
    /// The y axis.
    Y,
}

/// The level a cycle returns to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CycleReturn {
    /// The cycle's clearance level (G98).
    Initial,
    /// The cycle's retract level (G99).
    Retract,
}

cadmpeg_core::named_optional_field!(deserialize_chip_break_retract, Length, "chip_break_retract");
cadmpeg_core::named_optional_field!(deserialize_clearance, Length, "clearance");
cadmpeg_core::named_optional_field!(deserialize_depth, Length, "depth");
cadmpeg_core::named_optional_field!(deserialize_dwell, Duration, "dwell");
cadmpeg_core::named_optional_field!(deserialize_expansion, Vec<Motion>, "expansion");
cadmpeg_core::named_optional_field!(deserialize_full_retract_after, Length, "full_retract_after");
cadmpeg_core::named_optional_field!(deserialize_increment, Length, "increment");
cadmpeg_core::named_optional_field!(
    deserialize_increment_reduction,
    Length,
    "increment_reduction"
);
cadmpeg_core::named_optional_field!(deserialize_minimum_increment, Length, "minimum_increment");
cadmpeg_core::named_optional_field!(deserialize_nominal, Angle, "nominal");
cadmpeg_core::named_optional_field!(deserialize_probe_clearance, Length, "probe_clearance");
cadmpeg_core::named_optional_field!(deserialize_probe_overtravel, Length, "probe_overtravel");
cadmpeg_core::named_optional_field!(deserialize_retract, Length, "retract");
cadmpeg_core::named_optional_field!(deserialize_return_to, CycleReturn, "return_to");
cadmpeg_core::named_optional_field!(deserialize_stock, Length, "stock");
