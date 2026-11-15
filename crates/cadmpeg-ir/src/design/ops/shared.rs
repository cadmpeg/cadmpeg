// SPDX-License-Identifier: Apache-2.0
//! Types that several feature families share.
//!
//! A type that more than one family of the feature vocabulary names is defined
//! here once, and every family imports it.

use cadmpeg_core::text::NonBlankString;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::design::construction::{kinds, Construction, FrameAxis};
use crate::design::dimension::Dimension;
use crate::design::options::{Hand, Law, NativeOptions, RadialDimension};
use crate::design::selection::{OrderedSelection, SetSelection};
use crate::scalar::{
    Angle, FiniteReal, Fraction, Length, NonNegativeLength, NonNegativeReal, NonZeroLength,
    PositiveLength, PositiveReal,
};

/// The content of a sketch text or a text curve, with its font, size, and
/// layout of the glyphs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TextContent {
    /// The text.
    pub content: NonBlankString,
    /// The font name.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_font"
    )]
    pub font: Option<NonBlankString>,
    /// The cap height: the distance from the baseline to the top of a flat
    /// capital letter. `None` where the stored size is not established against
    /// a glyph outline.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_height"
    )]
    pub height: Option<Dimension<Length>>,
    /// The scale of the nominal glyph width.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_width_factor"
    )]
    pub width_factor: Option<Dimension<FiniteReal>>,
    /// The rotation of the text from the sketch x axis.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_angle"
    )]
    pub angle: Option<Dimension<Angle>>,
    /// The horizontal alignment, against the box `horizontal_box` names.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_horizontal"
    )]
    pub horizontal: Option<TextAlign>,
    /// The vertical alignment, against the box `vertical_box` names.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_vertical"
    )]
    pub vertical: Option<TextAlign>,
    /// The box the horizontal alignment is measured against.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_horizontal_box"
    )]
    pub horizontal_box: Option<TextHorizontalBox>,
    /// The box the vertical alignment is measured against. With `CapHeight`,
    /// `vertical` is not `Baseline`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_vertical_box"
    )]
    pub vertical_box: Option<TextVerticalBox>,
    /// The distance added between consecutive characters.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tracking"
    )]
    pub tracking: Option<Dimension<Length>>,
    /// The shear of the glyphs: x' = x + y tan(`oblique`).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_oblique"
    )]
    pub oblique: Option<Dimension<Angle>>,
    /// `Some(false)` makes the glyphs wires, not faces.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_filled"
    )]
    pub filled: Option<Dimension<bool>>,
}

/// The alignment of text against its box.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum TextAlign {
    /// At the box's start; vertically, against `CapHeight`, the baseline.
    Start,
    /// At the box's centre.
    Center,
    /// At the box's end.
    End,
    /// On the baseline.
    Baseline,
    /// A source alignment whose meaning is not established.
    Native(NonBlankString),
}

/// The box horizontal text alignment is measured against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TextHorizontalBox {
    /// The ink's extent.
    Ink,
    /// The pen's extent from its origin, leading space and side bearing
    /// included.
    Pen,
}

/// The box vertical text alignment is measured against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TextVerticalBox {
    /// The ink's extent.
    Ink,
    /// The band from the baseline to the cap height.
    CapHeight,
}

/// How a swept result passes a sharp corner of its path, where the path's
/// tangent turns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum CornerTransition {
    /// The pieces swept along the two adjacent segments extend until they meet,
    /// and their intersection is the joint.
    Mitre,
    /// The section turns about the axis through the corner point normal to both
    /// adjacent tangents, so a rounded piece joins the two.
    Round,
    /// The pieces swept along the two adjacent segments stay as they are, with
    /// no surface joining them at the corner.
    Open,
    /// No joint piece: the section crosses the corner without turning, each
    /// later segment's frame law premultiplied by the constant rotation that
    /// carries the frame at that segment's start onto the frame at the previous
    /// segment's end.
    Carried,
    /// A source transition whose meaning is not established.
    Native(NonBlankString),
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

/// What the source does with the tools of an operation. Which bodies the
/// current geometry holds is stated by the current states, never derived from
/// this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolDisposition {
    /// The tools stay as bodies.
    Kept,
    /// The tools are removed.
    Consumed,
    /// The tool body moves under the operation with its history editable.
    Absorbed,
}

/// One member of a pair of elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PairElement {
    /// The first member.
    First,
    /// The second member.
    Second,
}

/// The faces of a sheet-metal wall that a cut's side faces are normal to, the
/// cut being made as if the part were flat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum SheetCutNormal {
    /// The face the wall is built from.
    Defining,
    /// The other face.
    Opposite,
    /// A cut through the thickness in the flat state.
    Both,
}

/// A joint: a relation between two connectors, a design feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Joint {
    /// The two connectors the joint relates.
    pub connectors: [JointConnector; 2],
    /// What the joint does.
    pub kind: JointKind,
    /// The relative sense of the connectors' z axes or normals; its only home.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_alignment"
    )]
    pub alignment: Option<Alignment>,
    /// Source options whose effect is not established.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options that select how the source computes the result.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// A connector: a construction of the element a joint relates, or the picked
/// entity itself. Its resolved value is in the coordinates of the definition
/// its picks resolve in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum JointConnector {
    /// A frame.
    Frame(Construction<kinds::Frame>),
    /// A plane.
    Plane(Construction<kinds::Plane>),
    /// An axis.
    Axis(Construction<kinds::Axis>),
    /// A point.
    Point(Construction<kinds::Point>),
    /// The picked entity itself: a face whose surface the relation uses, the
    /// faces a rigid or native joint fixes, or a pick whose geometric kind the
    /// decoder cannot read.
    Entity(OrderedSelection),
}

/// The relative sense of a joint's connectors' z axes or normals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Alignment {
    /// The same sense.
    Aligned,
    /// Opposite senses: z₁ = −z₀.
    Opposed,
    /// The source fixes no sense and the solver keeps the nearer one; the
    /// solved sense is in the connectors' resolved frames.
    Nearest,
    /// A source alignment whose meaning is not established.
    Native(NonBlankString),
}

/// What a joint does to its connectors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum JointKind {
    /// The degrees of freedom the joint leaves.
    Motion {
        /// The freedoms.
        motion: JointMotion,
        /// The frame whose axes the motion, its limits, and its drives name,
        /// where the motion's axis is no axis of the first connector's frame.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_axes"
        )]
        axes: Option<Construction<kinds::Frame>>,
        /// The displacement of the second connector from the first.
        offset: JointOffset,
        /// The limits of the freedoms, at most one entry per axis.
        limits: Vec<AxisLimits>,
        /// The motors and springs on the freedoms.
        drives: Vec<JointDrive>,
    },
    /// The second element lies on the first.
    Coincident {
        /// The offset, signed along the first element's normal.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_offset"
        )]
        offset: Option<Dimension<Length>>,
    },
    /// Two axes coincide.
    Concentric,
    /// Two circles are coaxial, with coincident or offset planes.
    Insert {
        /// The offset of the planes.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_offset"
        )]
        offset: Option<Dimension<Length>>,
    },
    /// An unsigned separation.
    Distance {
        /// The separation.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_distance"
        )]
        distance: Option<Dimension<Length>>,
        /// The limits of the separation.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_limits"
        )]
        limits: Option<Bounds<Length>>,
    },
    /// An angle between the elements.
    Angle {
        /// The angle.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_angle"
        )]
        angle: Option<Dimension<Angle>>,
        /// The limits of the angle.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_limits"
        )]
        limits: Option<Bounds<Angle>>,
    },
    /// The elements are parallel.
    Parallel,
    /// The elements are perpendicular.
    Perpendicular,
    /// The elements touch tangentially.
    Tangent {
        /// Whether the elements touch from outside or from inside.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_contact"
        )]
        contact: Option<TangentContact>,
    },
    /// The first point lies at a distance from the second axis.
    PointOnAxis {
        /// The distance.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_distance"
        )]
        distance: Option<Dimension<Length>>,
    },
    /// The two elements are mirror images in a plane.
    Symmetric {
        /// The plane of symmetry.
        plane: Construction<kinds::Plane>,
    },
    /// The second element is held between the two elements of `pair`.
    Width {
        /// The two elements that hold it.
        pair: Box<[JointConnector; 2]>,
        /// Where it is held. `None` centres it.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_mode"
        )]
        mode: Option<WidthMode>,
    },
    /// A pin in a slot: the axis of the second element stays on the slot's
    /// centre path between the first element's side faces, free to slide along
    /// it and to turn about itself. Its connectors are `Entity` picks.
    Slot {
        /// Where the pin lies along the slot.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_position"
        )]
        position: Option<SlotPosition>,
        /// Whether the pin is stopped from turning.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_lock_rotation"
        )]
        lock_rotation: Option<Dimension<bool>>,
    },
    /// A point of the second element stays on a path, the chain the first
    /// connector picks.
    Path {
        /// Where the point lies along the path.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_position"
        )]
        position: Option<PathPosition>,
        /// The axis of the second element's definition frame held tangent to
        /// the path.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_pitch_yaw"
        )]
        pitch_yaw: Option<PathAxisControl>,
        /// The axis held along a constructed direction, never the axis
        /// `pitch_yaw` holds.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_roll"
        )]
        roll: Option<PathRollControl>,
    },
    /// A coupling of the rotations or translations of the two connectors.
    Coupling {
        /// The coupling.
        coupling: Coupling,
        /// The two other joints the coupling acts on, as `Feature` picks, gear
        /// or pinion first.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_joints"
        )]
        joints: Option<OrderedSelection>,
    },
    /// A source joint kind whose meaning is not established.
    Native(NonBlankString),
}

/// The degrees of freedom a motion joint leaves. An axis is an axis of the
/// first connector's frame, or of `axes` where that is stated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum JointMotion {
    /// None: the current relative placement of the two connectors is fixed. It
    /// needs no frame, so its connectors may be any variant.
    Rigid,
    /// Rotation about `axis`.
    Revolute {
        /// The axis.
        axis: FrameAxis,
    },
    /// Translation along `axis`.
    Slider {
        /// The axis.
        axis: FrameAxis,
    },
    /// Rotation about and translation along `axis`.
    Cylindrical {
        /// The axis.
        axis: FrameAxis,
    },
    /// Translation in the plane normal to `axis` and rotation about it.
    Planar {
        /// The axis.
        axis: FrameAxis,
    },
    /// Rotation about the origin.
    Ball,
    /// Rotation about `axis` and translation along `slide`, another axis of the
    /// same frame.
    PinSlot {
        /// The rotation axis.
        axis: FrameAxis,
        /// The translation axis.
        slide: FrameAxis,
    },
    /// A source motion whose meaning is not established.
    Native(NonBlankString),
}

/// The displacement of a motion joint's second connector from the first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct JointOffset {
    /// Along x.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_x"
    )]
    pub x: Option<Dimension<Length>>,
    /// Along y.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_y"
    )]
    pub y: Option<Dimension<Length>>,
    /// Along z.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_z"
    )]
    pub z: Option<Dimension<Length>>,
    /// About z.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_angle"
    )]
    pub angle: Option<Dimension<Angle>>,
}

/// The limits of one degree of freedom a motion leaves; it states `limits`,
/// `rest`, or both.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AxisLimits {
    /// A rotation about an axis.
    Rotation {
        /// The axis.
        axis: FrameAxis,
        /// The range.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_limits"
        )]
        limits: Option<Bounds<Angle>>,
        /// The joint coordinate at which the freedom rests within its range.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_rest"
        )]
        rest: Option<Dimension<Angle>>,
    },
    /// A translation along an axis.
    Translation {
        /// The axis.
        axis: FrameAxis,
        /// The range.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_limits"
        )]
        limits: Option<Bounds<Length>>,
        /// The joint coordinate at which the freedom rests within its range.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_rest"
        )]
        rest: Option<Dimension<Length>>,
    },
}

/// A range; it holds at least one bound.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    deny_unknown_fields,
    bound(serialize = "Q: Serialize", deserialize = "Q: Deserialize<'de>")
)]
pub struct Bounds<Q> {
    /// The lower bound.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_min"
    )]
    pub min: Option<Dimension<Q>>,
    /// The upper bound.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_max"
    )]
    pub max: Option<Dimension<Q>>,
}

/// A coupling of the rotations or translations of two members.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Coupling {
    /// A gear.
    Gear {
        /// The second member's rotation per rotation of the first, as a
        /// magnitude whose sign the frames give.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_ratio"
        )]
        ratio: Option<Dimension<FiniteReal>>,
        /// The radii the source states instead of the ratio.
        radii: [Option<Dimension<Length>>; 2],
    },
    /// A belt.
    Belt {
        /// The second member's rotation per rotation of the first, as a
        /// magnitude whose sign the frames give.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_ratio"
        )]
        ratio: Option<Dimension<FiniteReal>>,
        /// The radii the source states instead of the ratio.
        radii: [Option<Dimension<Length>>; 2],
    },
    /// A rack and pinion.
    RackPinion {
        /// The pinion rotation in radians per millimetre of rack travel, as a
        /// magnitude whose sign the frames give.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_ratio"
        )]
        ratio: Option<Dimension<FiniteReal>>,
        /// The pitch radius the source states instead of the ratio.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_pitch_radius"
        )]
        pitch_radius: Option<Dimension<Length>>,
    },
    /// A screw.
    Screw {
        /// The pitch.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_pitch"
        )]
        pitch: Option<Dimension<Length>>,
    },
    /// A source coupling whose meaning is not established.
    Native(NonBlankString),
}

/// A motor or spring on one degree of freedom of a motion joint. Its output is
/// stiffness · (target_position − position) + damping · (target_velocity −
/// velocity), limited to `max_force`, with positions in radians or millimetres,
/// velocities per second, and every coefficient in canonical units.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct JointDrive {
    /// The freedom it drives.
    pub axis: DriveAxis,
    /// How its output applies.
    pub mode: DriveMode,
    /// The target position.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_target_position"
    )]
    pub target_position: Option<FiniteReal>,
    /// The target velocity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_target_velocity"
    )]
    pub target_velocity: Option<FiniteReal>,
    /// The stiffness.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_stiffness"
    )]
    pub stiffness: Option<NonNegativeReal>,
    /// The damping.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_damping"
    )]
    pub damping: Option<NonNegativeReal>,
    /// The limit of the output; `None` is unlimited.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_max_force"
    )]
    pub max_force: Option<PositiveReal>,
}

/// The degree of freedom a drive acts on, an axis of the first connector's
/// frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DriveAxis {
    /// A rotation about the axis.
    Rotation {
        /// The axis.
        axis: FrameAxis,
    },
    /// A translation along the axis.
    Translation {
        /// The axis.
        axis: FrameAxis,
    },
}

/// How a drive's output applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DriveMode {
    /// As a force or a torque.
    Force,
    /// As an acceleration independent of the bodies' masses.
    Acceleration,
}

/// Whether a tangent joint's elements touch from outside or from inside.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TangentContact {
    /// From outside.
    Outside,
    /// From inside.
    Inside,
}

/// Where a width joint holds its element between the pair.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum WidthMode {
    /// Centred, so the two gaps are equal.
    Centered,
    /// Free to move between them.
    Free,
    /// At a distance from one element of the pair: the distance from the `from`
    /// element to the held element's face that faces it.
    Gap {
        /// The distance.
        distance: Dimension<NonNegativeLength>,
        /// The element of the pair the distance is measured from. `None` does
        /// not state which.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_from"
        )]
        from: Option<PairElement>,
    },
    /// A source width mode whose meaning is not established.
    Native(NonBlankString),
}

/// Where a slot joint's pin lies along the slot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SlotPosition {
    /// Anywhere.
    Free,
    /// At the middle of the slot's centre path.
    Centered,
    /// A source slot position whose meaning is not established.
    Native(NonBlankString),
}

/// Where a path joint's point lies along the path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PathPosition {
    /// Anywhere.
    Free {},
    /// At a distance along the path.
    Distance {
        /// The distance.
        distance: Dimension<NonNegativeLength>,
        /// Whether the distance is measured from the path's end, not its start.
        /// It decides how the value is read.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_from_end"
        )]
        from_end: Option<Dimension<bool>>,
    },
    /// At a fraction of the path's length.
    Percent {
        /// The fraction.
        fraction: Dimension<Fraction>,
        /// Whether the fraction is measured from the path's end, not its start.
        /// It decides how the value is read.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_from_end"
        )]
        from_end: Option<Dimension<bool>>,
    },
}

/// How a path joint holds an axis of the second element against the path's
/// tangent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PathAxisControl {
    /// Not held.
    Free {},
    /// One axis of the second element's definition frame held tangent to the
    /// path.
    FollowPath {
        /// The axis.
        axis: FrameAxis,
        /// Whether the axis is reversed.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_flip"
        )]
        flip: Option<Dimension<bool>>,
    },
}

/// How a path joint holds an axis of the second element along a direction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PathRollControl {
    /// Not held.
    Free {},
    /// One axis held along a constructed direction.
    UpVector {
        /// The direction.
        vector: Construction<kinds::Direction>,
        /// The axis.
        axis: FrameAxis,
        /// Whether the axis is reversed.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_flip"
        )]
        flip: Option<Dimension<bool>>,
    },
}

cadmpeg_core::named_optional_field!(deserialize_alignment<T>, T, "alignment");
cadmpeg_core::named_optional_field!(deserialize_angle<T>, T, "angle");
cadmpeg_core::named_optional_field!(deserialize_axes<T>, T, "axes");
cadmpeg_core::named_optional_field!(deserialize_contact<T>, T, "contact");
cadmpeg_core::named_optional_field!(deserialize_damping<T>, T, "damping");
cadmpeg_core::named_optional_field!(deserialize_distance<T>, T, "distance");
cadmpeg_core::named_optional_field!(deserialize_end<T>, T, "end");
cadmpeg_core::named_optional_field!(deserialize_filled<T>, T, "filled");
cadmpeg_core::named_optional_field!(deserialize_flip<T>, T, "flip");
cadmpeg_core::named_optional_field!(deserialize_font<T>, T, "font");
cadmpeg_core::named_optional_field!(deserialize_from<T>, T, "from");
cadmpeg_core::named_optional_field!(deserialize_from_end<T>, T, "from_end");
cadmpeg_core::named_optional_field!(deserialize_hand<T>, T, "hand");
cadmpeg_core::named_optional_field!(deserialize_height<T>, T, "height");
cadmpeg_core::named_optional_field!(deserialize_horizontal<T>, T, "horizontal");
cadmpeg_core::named_optional_field!(deserialize_horizontal_box<T>, T, "horizontal_box");
cadmpeg_core::named_optional_field!(deserialize_joints<T>, T, "joints");
cadmpeg_core::named_optional_field!(deserialize_law<T>, T, "law");
cadmpeg_core::named_optional_field!(deserialize_limits<T>, T, "limits");
cadmpeg_core::named_optional_field!(deserialize_lock_rotation<T>, T, "lock_rotation");
cadmpeg_core::named_optional_field!(deserialize_max<T>, T, "max");
cadmpeg_core::named_optional_field!(deserialize_max_force<T>, T, "max_force");
cadmpeg_core::named_optional_field!(deserialize_min<T>, T, "min");
cadmpeg_core::named_optional_field!(deserialize_mode<T>, T, "mode");
cadmpeg_core::named_optional_field!(deserialize_oblique<T>, T, "oblique");
cadmpeg_core::named_optional_field!(deserialize_offset<T>, T, "offset");
cadmpeg_core::named_optional_field!(deserialize_pitch<T>, T, "pitch");
cadmpeg_core::named_optional_field!(deserialize_pitch_radius<T>, T, "pitch_radius");
cadmpeg_core::named_optional_field!(deserialize_pitch_yaw<T>, T, "pitch_yaw");
cadmpeg_core::named_optional_field!(deserialize_position<T>, T, "position");
cadmpeg_core::named_optional_field!(deserialize_ratio<T>, T, "ratio");
cadmpeg_core::named_optional_field!(deserialize_rest<T>, T, "rest");
cadmpeg_core::named_optional_field!(deserialize_roll<T>, T, "roll");
cadmpeg_core::named_optional_field!(deserialize_start<T>, T, "start");
cadmpeg_core::named_optional_field!(deserialize_stiffness<T>, T, "stiffness");
cadmpeg_core::named_optional_field!(deserialize_taper<T>, T, "taper");
cadmpeg_core::named_optional_field!(deserialize_target_position<T>, T, "target_position");
cadmpeg_core::named_optional_field!(deserialize_target_velocity<T>, T, "target_velocity");
cadmpeg_core::named_optional_field!(deserialize_tracking<T>, T, "tracking");
cadmpeg_core::named_optional_field!(deserialize_vertical<T>, T, "vertical");
cadmpeg_core::named_optional_field!(deserialize_vertical_box<T>, T, "vertical_box");
cadmpeg_core::named_optional_field!(deserialize_width_factor<T>, T, "width_factor");
cadmpeg_core::named_optional_field!(deserialize_x<T>, T, "x");
cadmpeg_core::named_optional_field!(deserialize_y<T>, T, "y");
cadmpeg_core::named_optional_field!(deserialize_z<T>, T, "z");

#[cfg(test)]
pub(super) mod tests;
