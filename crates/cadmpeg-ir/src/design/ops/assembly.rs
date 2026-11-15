// SPDX-License-Identifier: Apache-2.0
//! Assembly operations (§6.7).
//!
//! An assembly operation is a feature of the scope that holds an assembly's
//! design: the scope of the assembly definition, or a Fusion root scope with
//! `Feature.component` naming the component. It may name an occurrence owned
//! by any definition its scope spans. The operations place, connect, repeat,
//! mirror, and modify occurrences: [`Place`], [`PlaceSet`], [`Joint`],
//! [`ComponentPattern`], [`MirrorComponents`], and the assembly feature, whose
//! [`ContextEffect`] states what it changes. A [`RigidGroup`] holds occurrences
//! in their relative placement.

use std::collections::BTreeSet;

use cadmpeg_core::text::NonBlankString;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::shared::{PairElement, PatternLayout};
use crate::design::construction::{kinds, Construction, FrameAxis};
use crate::design::dimension::Dimension;
use crate::design::names::InstanceIndex;
use crate::design::options::NativeOptions;
use crate::design::selection::{OccurrencePath, OrderedSelection};
use crate::design::wire;
use crate::ids::OccurrenceId;
use crate::scalar::{
    Angle, FiniteReal, Fraction, Length, NonNegativeLength, NonNegativeReal, PositiveReal,
};

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

/// A relation between two connectors: a mate, an assembly constraint, a
/// joint, a USD physics joint, or a STEP kinematic pair.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Joint {
    /// The two connectors the joint relates.
    pub connectors: [JointConnector; 2],
    /// What the joint holds.
    pub kind: JointKind,
    /// The relative sense of the connectors' z axes or normals.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_alignment"
    )]
    pub alignment: Option<Alignment>,
    /// Source options whose meaning is not established, and the values of
    /// typed facts whose carrier is unread.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options with no design meaning.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// A construction of the element a joint relates. Its picks select geometry
/// through occurrence contexts; a connector on the world is a construction
/// with no inputs and a resolved value.
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
    /// faces a rigid or native mate fixes, or a pick whose geometric kind the
    /// decoder cannot read.
    Entity(OrderedSelection),
}

/// The relative sense of a joint's connectors' z axes or normals. It is the
/// only home of the sense.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Alignment {
    /// The senses agree.
    Aligned,
    /// The senses are opposite: `z₁ = −z₀`.
    Opposed,
    /// The source fixes no sense and the solver keeps the nearer one; the
    /// solved sense is in the connectors' resolved frames.
    Nearest,
    /// A sense whose meaning is not established.
    Native(NonBlankString),
}

/// What a joint holds.
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
        /// The degrees of freedom.
        motion: JointMotion,
        /// The frame whose axes the motion, its limits, and its drives name,
        /// where the motion's axis is no axis of the first connector's frame.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_axes"
        )]
        axes: Option<Box<Construction<kinds::Frame>>>,
        /// The displacement of the second connector from the first.
        offset: JointOffset,
        /// The limits of the degrees of freedom the motion leaves.
        limits: Vec<AxisLimits>,
        /// The motors and springs on degrees of freedom of the motion.
        drives: Vec<JointDrive>,
    },
    /// The second element on the first.
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
    /// Two circles coaxial with coincident or offset planes.
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
        /// The bounds of a limit mate.
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
        /// The bounds of a limit mate.
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
    /// The first point at `distance` from the second axis.
    PointOnAxis {
        /// The distance from the axis.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_distance"
        )]
        distance: Option<Dimension<Length>>,
    },
    /// The two elements are mirror images in `plane`.
    Symmetric {
        /// The plane of symmetry.
        plane: Construction<kinds::Plane>,
    },
    /// The second element held between the two elements of `pair`.
    Width {
        /// The two elements that bound the held element.
        pair: Box<[JointConnector; 2]>,
        /// Where the held element lies between them. `None` centres it.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_mode"
        )]
        mode: Option<WidthMode>,
    },
    /// A pin in a slot: the axis of the second element stays on the slot's
    /// centre path between the first element's side faces, free to slide
    /// along it and to turn about itself.
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
    /// A point of the second element kept on a path, the chain its first
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
    /// The rotations or translations of the two connectors related.
    Coupling {
        /// The relation.
        coupling: Coupling,
        /// The two other joints the coupling acts on, as `Feature` targets,
        /// gear or pinion first.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_joints"
        )]
        joints: Option<OrderedSelection>,
    },
    /// A kind whose meaning is not established.
    Native(NonBlankString),
}

/// The degrees of freedom a motion joint leaves, each axis an axis of the
/// first connector's frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum JointMotion {
    /// None: the current relative placement of the two connectors is fixed.
    Rigid,
    /// Rotation about `axis`.
    Revolute {
        /// The axis of rotation.
        axis: FrameAxis,
    },
    /// Translation along `axis`.
    Slider {
        /// The axis of translation.
        axis: FrameAxis,
    },
    /// Rotation about and translation along `axis`.
    Cylindrical {
        /// The axis.
        axis: FrameAxis,
    },
    /// Translation in the plane normal to `axis` and rotation about it.
    Planar {
        /// The normal of the plane.
        axis: FrameAxis,
    },
    /// Rotation about the origin.
    Ball,
    /// Rotation about `axis` and translation along `slide`, another axis of
    /// the same frame.
    PinSlot {
        /// The axis of rotation.
        axis: FrameAxis,
        /// The axis of translation.
        slide: FrameAxis,
    },
    /// A motion whose meaning is not established.
    Native(NonBlankString),
}

/// The displacement of a motion joint's second connector from the first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct JointOffset {
    /// The displacement along x.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_x"
    )]
    pub x: Option<Dimension<Length>>,
    /// The displacement along y.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_y"
    )]
    pub y: Option<Dimension<Length>>,
    /// The displacement along z.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_z"
    )]
    pub z: Option<Dimension<Length>>,
    /// The rotation about z.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_angle"
    )]
    pub angle: Option<Dimension<Angle>>,
}

/// The limits of one degree of freedom a motion leaves, at most once per
/// axis. It states `limits`, `rest`, or both.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AxisLimits {
    /// Rotation about an axis.
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
        /// The joint coordinate at which the degree of freedom rests within
        /// its range.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_rest"
        )]
        rest: Option<Dimension<Angle>>,
    },
    /// Translation along an axis.
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
        /// The joint coordinate at which the degree of freedom rests within
        /// its range.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_rest"
        )]
        rest: Option<Dimension<Length>>,
    },
}

/// A range. It holds at least one bound.
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

/// A relation of the rotations or translations of two connectors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Coupling {
    /// A gear pair.
    Gear {
        /// The second member's rotation per rotation of the first, as a
        /// magnitude whose sign the frames give.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_ratio"
        )]
        ratio: Option<Dimension<FiniteReal>>,
        /// The radii the source states instead.
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
        /// The radii the source states instead.
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
        /// The pitch radius the source states instead.
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
    /// A coupling whose meaning is not established.
    Native(NonBlankString),
}

/// A motor or spring on one degree of freedom of a `Motion` joint. Its output
/// is `stiffness · (target_position − position) + damping · (target_velocity −
/// velocity)`, limited to `max_force`, with positions in radians or
/// millimetres, velocities per second, and every coefficient in canonical
/// units.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct JointDrive {
    /// The degree of freedom.
    pub axis: DriveAxis,
    /// How the output applies.
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
    /// The coefficient of the position error.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_stiffness"
    )]
    pub stiffness: Option<NonNegativeReal>,
    /// The coefficient of the velocity error.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_damping"
    )]
    pub damping: Option<NonNegativeReal>,
    /// The limit of the output, unlimited where `None`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_max_force"
    )]
    pub max_force: Option<PositiveReal>,
}

/// The degree of freedom a drive acts on: a rotation about or a translation
/// along an axis of the first connector's frame.
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

/// Whether a `Tangent` joint's elements touch from outside or from inside.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TangentContact {
    /// From outside.
    Outside,
    /// From inside.
    Inside,
}

/// Where a `Width` joint holds its element between the two elements of its
/// pair.
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
    /// The distance from the `from` element of the pair to the held element's
    /// face that faces it is `distance`.
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
    /// A mode whose meaning is not established.
    Native(NonBlankString),
}

/// Where a `Slot` joint's pin lies along the slot.
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
    /// A position whose meaning is not established.
    Native(NonBlankString),
}

/// Where a `Path` joint's point lies along the path.
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
        /// Whether the distance is measured from the path's end rather than
        /// its start.
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
        /// Whether the fraction is measured from the path's end rather than
        /// its start.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_from_end"
        )]
        from_end: Option<Dimension<bool>>,
    },
}

/// The axis of a `Path` joint's second element held tangent to the path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PathAxisControl {
    /// No axis is held.
    Free {},
    /// One axis of the element's definition frame held tangent to the path.
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

/// The axis of a `Path` joint's second element held along a constructed
/// direction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PathRollControl {
    /// No axis is held.
    Free {},
    /// One axis held along `vector`.
    UpVector {
        /// The direction.
        vector: Box<Construction<kinds::Direction>>,
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

/// Occurrences held in the placement they have relative to one another, so
/// that they move as one. The group states no pose: a member's pose in the
/// group is its `Occurrence.transform`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RigidGroup {
    /// The members, as paths from the definition that owns the group; the
    /// empty path is that definition's own origin.
    #[serde(with = "wire::set")]
    #[cfg_attr(feature = "schema", schemars(with = "Vec<OccurrencePath>"))]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

/// What an assembly feature changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    Instances,
    /// The participants' definitions: each changed definition's scope holds a
    /// resulting feature with `driven_by` naming the assembly feature.
    Definitions,
    /// An effect whose meaning is not established.
    Native(NonBlankString),
}

cadmpeg_core::named_optional_field!(deserialize_about<T>, T, "about");
cadmpeg_core::named_optional_field!(deserialize_alignment<T>, T, "alignment");
cadmpeg_core::named_optional_field!(deserialize_angle<T>, T, "angle");
cadmpeg_core::named_optional_field!(deserialize_axes<T>, T, "axes");
cadmpeg_core::named_optional_field!(deserialize_base<T>, T, "base");
cadmpeg_core::named_optional_field!(deserialize_contact<T>, T, "contact");
cadmpeg_core::named_optional_field!(deserialize_damping<T>, T, "damping");
cadmpeg_core::named_optional_field!(deserialize_distance<T>, T, "distance");
cadmpeg_core::named_optional_field!(deserialize_flip<T>, T, "flip");
cadmpeg_core::named_optional_field!(deserialize_from<T>, T, "from");
cadmpeg_core::named_optional_field!(deserialize_from_end<T>, T, "from_end");
cadmpeg_core::named_optional_field!(deserialize_joints<T>, T, "joints");
cadmpeg_core::named_optional_field!(deserialize_limits<T>, T, "limits");
cadmpeg_core::named_optional_field!(deserialize_lock_rotation<T>, T, "lock_rotation");
cadmpeg_core::named_optional_field!(deserialize_max<T>, T, "max");
cadmpeg_core::named_optional_field!(deserialize_max_force<T>, T, "max_force");
cadmpeg_core::named_optional_field!(deserialize_min<T>, T, "min");
cadmpeg_core::named_optional_field!(deserialize_mode<T>, T, "mode");
cadmpeg_core::named_optional_field!(deserialize_offset<T>, T, "offset");
cadmpeg_core::named_optional_field!(deserialize_orientation<T>, T, "orientation");
cadmpeg_core::named_optional_field!(deserialize_packaged<T>, T, "packaged");
cadmpeg_core::named_optional_field!(deserialize_pitch<T>, T, "pitch");
cadmpeg_core::named_optional_field!(deserialize_pitch_radius<T>, T, "pitch_radius");
cadmpeg_core::named_optional_field!(deserialize_pitch_yaw<T>, T, "pitch_yaw");
cadmpeg_core::named_optional_field!(deserialize_placement<T>, T, "placement");
cadmpeg_core::named_optional_field!(deserialize_position<T>, T, "position");
cadmpeg_core::named_optional_field!(deserialize_ratio<T>, T, "ratio");
cadmpeg_core::named_optional_field!(deserialize_rest<T>, T, "rest");
cadmpeg_core::named_optional_field!(deserialize_roll<T>, T, "roll");
cadmpeg_core::named_optional_field!(deserialize_stiffness<T>, T, "stiffness");
cadmpeg_core::named_optional_field!(deserialize_target_position<T>, T, "target_position");
cadmpeg_core::named_optional_field!(deserialize_target_velocity<T>, T, "target_velocity");
cadmpeg_core::named_optional_field!(deserialize_x<T>, T, "x");
cadmpeg_core::named_optional_field!(deserialize_y<T>, T, "y");
cadmpeg_core::named_optional_field!(deserialize_z<T>, T, "z");
