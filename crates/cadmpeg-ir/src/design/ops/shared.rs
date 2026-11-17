// SPDX-License-Identifier: Apache-2.0
//! Types that more than one operation family shares.

use cadmpeg_core::text::NonBlankString;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::design::construction::{kinds, Construction, FrameAxis, ScalarDimension};
use crate::design::dimension::Dimension;
use crate::design::expr::Expression;
use crate::design::options::{LimitOffset, NativeOptions, PathRef, SymmetricMeasure};
use crate::design::selection::{OrderedSelection, SetSelection};
use crate::features::ParameterId;
use crate::scalar::{
    Angle, FiniteReal, Fraction, Length, NonNegativeLength, NonNegativeReal, PositiveReal,
};

/// A size measured on the radius or on the diameter, as the source states
/// it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound(
        serialize = "Q: Serialize, P: Serialize",
        deserialize = "Q: Deserialize<'de>, P: Deserialize<'de>"
    )
)]
pub enum RadialDimension<Q, P = ParameterId> {
    /// The size on the radius.
    Radius(Dimension<Q, P>),
    /// The size on the diameter.
    Diameter(Dimension<Q, P>),
}

/// Material placed along a direction or about an axis, measured from the
/// operation's origin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound(serialize = "Q: Serialize", deserialize = "Q: Deserialize<'de>")
)]
pub enum Span<Q> {
    /// From `start` to `end`. `end` is measured along the direction and
    /// `start` against it.
    Between {
        /// The limit against the direction.
        start: Box<Limit<Q>>,
        /// The limit along the direction.
        end: Box<Limit<Q>>,
    },
    /// Both sides of the origin equally.
    Symmetric {
        /// The amount.
        amount: Dimension<Q>,
        /// Whether `amount` is the extent of each side or the total.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_measure"
        )]
        measure: Option<SymmetricMeasure>,
    },
}

/// One end of a span.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound(serialize = "Q: Serialize", deserialize = "Q: Deserialize<'de>")
)]
pub enum Limit<Q> {
    /// At the operation's origin.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    AtOrigin,
    /// A signed distance or angle from the origin.
    Amount(Dimension<Q>),
    /// Through every target body; in a revolve, a full turn.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    ThroughAll,
    /// At the first face the swept profile meets.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Next,
    /// At the last face the swept profile meets.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Last,
    /// At faces, surfaces, or bodies.
    ToGeometry {
        /// The faces, surfaces, or bodies.
        target: SetSelection,
        /// The offset of the limit from the target.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_offset"
        )]
        offset: Option<LimitOffset>,
        /// Whether the target faces extend until the profile meets them.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_extend"
        )]
        extend: Option<Dimension<bool>>,
        /// Where `target` picks bodies, whether the limit is the far side of
        /// the body the profile meets.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_far"
        )]
        far: Option<Dimension<bool>>,
    },
    /// At a plane.
    ToPlane(Construction<kinds::Plane>),
    /// Where the swept profile reaches a point.
    ToPoint(Construction<kinds::Point>),
    /// A source limit kind whose meaning is not established.
    Native(NonBlankString),
}

/// A function of a quantity along a path parameter from 0 to 1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound(serialize = "Q: Serialize", deserialize = "Q: Deserialize<'de>")
)]
pub enum Law<Q> {
    /// A constant.
    Constant(Dimension<Q>),
    /// A linear ramp.
    Linear {
        /// The value at the path's start.
        start: Dimension<Q>,
        /// The value at the path's end.
        end: Dimension<Q>,
    },
    /// A law curve that the source selects.
    Curve(OrderedSelection),
    /// An expression in the law parameter whose result has the quantity.
    Expression(Expression),
    /// A source law whose meaning is not established.
    Native(NonBlankString),
}

/// How a value or a section changes between consecutive stations or
/// sections.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Interpolation {
    /// Each consecutive pair blends linearly, which leaves an edge at each
    /// intermediate section.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Linear,
    /// One smooth surface passes through every section.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Smooth,
    /// A source interpolation whose meaning is not established.
    Native(NonBlankString),
}

/// The corner law between offset faces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum OffsetJoin {
    /// Rounded arcs.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Round,
    /// Sharp intersection: the sides extend to meet.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Sharp,
    /// A source join whose meaning is not established.
    Native(NonBlankString),
}

/// The faces of a sheet-metal wall that a cut's side faces are normal to, the
/// cut being made as if the part were flat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
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

/// Where a pattern's instances lie. Every direction, axis, and plane is a
/// construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum PatternLayout {
    /// One or two runs, each along its own direction.
    Linear {
        /// The first run.
        first: LinearRun,
        /// The second run.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_second"
        )]
        second: Option<LinearRun>,
        /// Whether the second run repeats only the seed, not the first run's
        /// instances: the instances that then exist are `[i, 0]` and
        /// `[0, j]`. It occurs only with a `second` run.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_seed_only"
        )]
        seed_only: Option<Dimension<bool>>,
    },
    /// An angular run about `axis` and an optional radial run outward from
    /// it.
    Circular {
        /// The axis of the angular run.
        axis: Construction<kinds::Axis>,
        /// The run about the axis.
        angular: PatternRun<Angle>,
        /// The run outward from the axis.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_radial"
        )]
        radial: Option<PatternRun<Length>>,
        /// Whether an instance keeps the seed's orientation or turns with the
        /// layout.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_orientation"
        )]
        orientation: Option<InstanceOrientation>,
    },
    /// Instances along a path, `run` stating their spacing by arc length.
    Path {
        /// The path.
        path: PathRef,
        /// The spacing of the instances by arc length.
        run: PatternRun<Length>,
        /// Whether an instance keeps the seed's orientation, turns with the
        /// path, or follows its frame.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_orientation"
        )]
        orientation: Option<InstanceOrientation>,
    },
    /// One instance at each picked point, in pick order.
    Points {
        /// The points.
        points: OrderedSelection,
        /// The point of the seed that lands on each picked point.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_reference"
        )]
        reference: Option<Locus>,
    },
    /// Runs that each increment seed dimensions. The instance placement
    /// follows from the incremented placement dimensions, so there is no
    /// direction.
    Dimensions(Vec<DimensionRun>),
    /// One row per instance, with a value per table dimension.
    Table(DimensionTable),
    /// The seeds reflected across `plane`. Its one instance is the
    /// reflection.
    Mirror {
        /// The mirror plane.
        plane: Construction<kinds::Plane>,
        /// The tolerance with which the mirror stitches the reflection to its
        /// seed, whatever the seeds are.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_stitch_tolerance"
        )]
        stitch_tolerance: Option<Dimension<Length>>,
    },
    /// Stages applied in order, each to every instance the earlier stages
    /// produced, except that a `Scale` stage after the first splits the
    /// instances made so far into `count` consecutive slices of equal length
    /// and scales every instance of slice j by slice j's factor about that
    /// instance's own transformed centre. No stage is a `Composite`.
    Composite(Vec<PatternLayout>),
    /// `count` instances, the seed included: instance i, for i from 0 to
    /// count − 1, is the seed scaled uniformly by 1 + i (factor − 1)/(count −
    /// 1) about `center`, so the last is scaled by `factor`.
    Scale {
        /// The scale of the last instance.
        factor: Dimension<PositiveReal>,
        /// The number of instances, the seed included.
        count: Dimension<u32>,
        /// The point the instances are scaled about.
        center: Locus,
    },
    /// The instance set and indices of another pattern, which its one pick
    /// names: one instance per instance index of that pattern other than the
    /// seed's, with that index, each the seeds rebuilt against that pattern's
    /// instance.
    Follow(OrderedSelection),
    /// Components along a path that lies in one plane, each instance keeping
    /// the seed's relation to that plane. It occurs only in a component
    /// pattern.
    Chain(ChainLayout),
    /// A layout whose meaning is not established.
    Native(NonBlankString),
}

/// Components placed along a planar path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ChainLayout {
    /// The path, which lies in one plane.
    pub path: PathRef,
    /// How the seeds link along the path.
    pub links: ChainLinks,
}

/// How the seeds of a chain link along its path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ChainLinks {
    /// One link per seed, in the order the seeds alternate. Each link's first
    /// pin axis is the previous link's second, both pin axes of every link
    /// lie on the path, and consecutive pin axes are therefore a chord of the
    /// path as long as the link's own pin distance.
    Connected(Vec<ChainLink>),
    /// A linkage whose meaning is not established.
    Native {
        /// The source's form.
        kind: NonBlankString,
        /// The entities of the seeds it picks.
        picks: OrderedSelection,
    },
}

/// One link of a connected chain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ChainLink {
    /// Two axes of the seed's own geometry, both on the path.
    pub pins: [Construction<kinds::Axis>; 2],
}

/// A run along a direction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct LinearRun {
    /// The direction of the run.
    pub direction: Construction<kinds::Direction>,
    /// The instances along it.
    pub run: PatternRun<Length>,
}

/// The instances along one run of a pattern.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    deny_unknown_fields,
    bound(serialize = "Q: Serialize", deserialize = "Q: Deserialize<'de>")
)]
pub struct PatternRun<Q> {
    /// How the run spaces its instances.
    pub spacing: Spacing<Q>,
    /// Where the run places its instances. `None` states nothing.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_sides"
    )]
    pub sides: Option<RunSides<Q>>,
    /// The seed's step count from the run's first instance: the run's
    /// instances then span coordinates −`seed` to count − 1 − `seed`. `None`
    /// places the seed first.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_seed"
    )]
    pub seed: Option<Dimension<u32>>,
    /// Parameters of the seed definition that change by `step` per
    /// coordinate along the run.
    pub increments: Vec<Increment>,
}

/// Where a run places its instances.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound(serialize = "Q: Serialize", deserialize = "Q: Deserialize<'de>")
)]
pub enum RunSides<Q> {
    /// Along the run's direction from the seed.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    One,
    /// Against the run's direction from the seed, so the instances'
    /// coordinates are negative. It occurs only in a `Path` run.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Against,
    /// The run mirrored onto the reverse side.
    Symmetric {
        /// How the stored count is read. `None` means not read.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_count"
        )]
        count: Option<SymmetricMeasure>,
        /// How the stored distance of a `Span` or `PitchSpan` is read. `None`
        /// means not read.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_extent"
        )]
        extent: Option<SymmetricMeasure>,
    },
    /// A second arm against the run's direction with its own spacing.
    Reverse(Box<Spacing<Q>>),
}

/// The spacing of a run. Every count includes the seed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound(serialize = "Q: Serialize", deserialize = "Q: Deserialize<'de>")
)]
pub enum Spacing<Q> {
    /// Count and spacing between neighbours.
    Pitch {
        /// The number of instances.
        count: Dimension<u32>,
        /// The spacing between neighbours.
        pitch: Dimension<Q>,
    },
    /// Count and the distance or angle from the first instance to the last.
    Span {
        /// The number of instances.
        count: Dimension<u32>,
        /// The distance or angle from the first instance to the last.
        span: Dimension<Q>,
    },
    /// Spacing and total extent, with the count derived.
    PitchSpan {
        /// The spacing between neighbours.
        pitch: Dimension<Q>,
        /// The total extent.
        span: Dimension<Q>,
    },
    /// `count` instances spread over the whole path or the whole turn.
    Fit {
        /// The number of instances.
        count: Dimension<u32>,
    },
    /// A linear run filled up to a reference.
    UpTo {
        /// The reference the run fills up to.
        reference: Box<OrderedSelection>,
        /// The count or the spacing.
        fill: UpToFill<Q>,
        /// The distance short of the reference at which the run stops.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_offset"
        )]
        offset: Option<Dimension<Q>>,
        /// The locus on the seed the run is measured from.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_from"
        )]
        from: Option<Locus>,
    },
    /// The spacing between each neighbour pair, in order, so the count is its
    /// length plus one.
    List(Vec<Dimension<Q>>),
    /// A spacing whose meaning is not established.
    Native(NonBlankString),
}

/// How an `UpTo` run fills its extent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound(serialize = "Q: Serialize", deserialize = "Q: Deserialize<'de>")
)]
pub enum UpToFill<Q> {
    /// A number of instances.
    Count(Dimension<u32>),
    /// A spacing between neighbours.
    Pitch(Dimension<Q>),
}

/// A point on a seed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Locus {
    /// The seed's centroid.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    Centroid,
    /// A constructed point.
    Point(Box<Construction<kinds::Point>>),
}

/// Whether a circular or path instance keeps the seed's orientation or turns
/// with the layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum InstanceOrientation {
    /// Keeps the seed's orientation.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Fixed,
    /// Turns with the layout: about the axis, or tangent to the path.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Follow,
    /// Follows the path's Frenet frame.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Frenet,
    /// Puts the instance's x axis on the path tangent and its
    /// `reference_axis` on the `reference` direction made orthogonal to the
    /// tangent, the third axis completing a right-handed frame, after turning
    /// the seed so that its `seed_axis`, or its x axis where that is `None`,
    /// lies on its x axis.
    Rolled {
        /// The direction the reference axis lies on.
        reference: PathReference,
        /// The axis of the instance, `Y` or `Z`, that lies on `reference`.
        reference_axis: FrameAxis,
        /// The axis of the seed turned onto the instance's x axis.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_seed_axis"
        )]
        seed_axis: Option<Box<Construction<kinds::Direction>>>,
    },
    /// An orientation whose meaning is not established.
    Native(NonBlankString),
}

/// The direction a rolled instance's reference axis lies on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PathReference {
    /// The normal of the path's plane on the side of the z axis of the path's
    /// placement, and the definition's +Z for a path that is not planar.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    PlaneNormal,
    /// A constructed direction.
    Direction(Box<Construction<kinds::Direction>>),
}

/// A change of a seed parameter per coordinate along a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Increment {
    /// The parameter of the seed definition.
    pub parameter: ParameterId,
    /// The change per coordinate. It is a dimension, so a parameter can
    /// drive it.
    pub step: ScalarDimension,
}

/// One run of a dimension pattern.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DimensionRun {
    /// The number of instances.
    pub count: Dimension<u32>,
    /// The seed dimensions it increments.
    pub increments: Vec<Increment>,
}

/// A table pattern: one row per instance, with a value per table dimension.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DimensionTable {
    /// The table dimensions.
    pub parameters: Vec<ParameterId>,
    /// The rows, one per instance.
    pub rows: Vec<TableRow>,
}

/// One instance of a table pattern.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TableRow {
    /// The instance's coordinate.
    pub instance: u32,
    /// A value per table dimension, `None` where the row keeps the seed's
    /// value.
    pub values: Vec<Option<ScalarDimension>>,
}

/// What the source does with the tools of an operation. Which bodies the
/// current geometry holds is stated by the current states, never derived from
/// this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PairElement {
    /// The first member.
    First,
    /// The second member.
    Second,
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
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Aligned,
    /// Opposite senses: z₁ = −z₀.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Opposed,
    /// The source fixes no sense and the solver keeps the nearer one; the
    /// solved sense is in the connectors' resolved frames.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
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
        axes: Option<Box<Construction<kinds::Frame>>>,
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
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
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
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Parallel,
    /// The elements are perpendicular.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
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
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
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
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
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
/// `stiffness · (target_position − position) + damping · (target_velocity −
/// velocity)`, limited to `max_force`, with positions in radians or
/// millimetres, velocities per second, and every coefficient in canonical
/// units.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DriveMode {
    /// As a force or a torque.
    Force,
    /// As an acceleration independent of the bodies' masses.
    Acceleration,
}

/// Whether a tangent joint's elements touch from outside or from inside.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
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
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Centered,
    /// Free to move between them.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
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
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Free,
    /// At the middle of the slot's centre path.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
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
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    Free,
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
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    Free,
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
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    Free,
    /// One axis held along a constructed direction.
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

cadmpeg_core::named_optional_field!(deserialize_alignment<T>, T, "alignment");
cadmpeg_core::named_optional_field!(deserialize_angle<T>, T, "angle");
cadmpeg_core::named_optional_field!(deserialize_axes<T>, T, "axes");
cadmpeg_core::named_optional_field!(deserialize_contact<T>, T, "contact");
cadmpeg_core::named_optional_field!(deserialize_count<T>, T, "count");
cadmpeg_core::named_optional_field!(deserialize_damping<T>, T, "damping");
cadmpeg_core::named_optional_field!(deserialize_distance<T>, T, "distance");
cadmpeg_core::named_optional_field!(deserialize_end<T>, T, "end");
cadmpeg_core::named_optional_field!(deserialize_extend, Dimension<bool>, "extend");
cadmpeg_core::named_optional_field!(deserialize_extent<T>, T, "extent");
cadmpeg_core::named_optional_field!(deserialize_far, Dimension<bool>, "far");
cadmpeg_core::named_optional_field!(deserialize_flip<T>, T, "flip");
cadmpeg_core::named_optional_field!(deserialize_from<T>, T, "from");
cadmpeg_core::named_optional_field!(deserialize_from_end<T>, T, "from_end");
cadmpeg_core::named_optional_field!(deserialize_hand<T>, T, "hand");
cadmpeg_core::named_optional_field!(deserialize_joints<T>, T, "joints");
cadmpeg_core::named_optional_field!(deserialize_law<T>, T, "law");
cadmpeg_core::named_optional_field!(deserialize_limits<T>, T, "limits");
cadmpeg_core::named_optional_field!(deserialize_lock_rotation<T>, T, "lock_rotation");
cadmpeg_core::named_optional_field!(deserialize_max<T>, T, "max");
cadmpeg_core::named_optional_field!(deserialize_max_force<T>, T, "max_force");
cadmpeg_core::named_optional_field!(deserialize_measure, SymmetricMeasure, "measure");
cadmpeg_core::named_optional_field!(deserialize_min<T>, T, "min");
cadmpeg_core::named_optional_field!(deserialize_mode<T>, T, "mode");
cadmpeg_core::named_optional_field!(deserialize_offset<T>, T, "offset");
cadmpeg_core::named_optional_field!(deserialize_orientation<T>, T, "orientation");
cadmpeg_core::named_optional_field!(deserialize_pitch<T>, T, "pitch");
cadmpeg_core::named_optional_field!(deserialize_pitch_radius<T>, T, "pitch_radius");
cadmpeg_core::named_optional_field!(deserialize_pitch_yaw<T>, T, "pitch_yaw");
cadmpeg_core::named_optional_field!(deserialize_position<T>, T, "position");
cadmpeg_core::named_optional_field!(deserialize_radial<T>, T, "radial");
cadmpeg_core::named_optional_field!(deserialize_ratio<T>, T, "ratio");
cadmpeg_core::named_optional_field!(deserialize_reference<T>, T, "reference");
cadmpeg_core::named_optional_field!(deserialize_rest<T>, T, "rest");
cadmpeg_core::named_optional_field!(deserialize_roll<T>, T, "roll");
cadmpeg_core::named_optional_field!(deserialize_second<T>, T, "second");
cadmpeg_core::named_optional_field!(deserialize_seed<T>, T, "seed");
cadmpeg_core::named_optional_field!(deserialize_seed_axis<T>, T, "seed_axis");
cadmpeg_core::named_optional_field!(deserialize_seed_only<T>, T, "seed_only");
cadmpeg_core::named_optional_field!(deserialize_sides<T>, T, "sides");
cadmpeg_core::named_optional_field!(deserialize_start<T>, T, "start");
cadmpeg_core::named_optional_field!(deserialize_stiffness<T>, T, "stiffness");
cadmpeg_core::named_optional_field!(deserialize_stitch_tolerance<T>, T, "stitch_tolerance");
cadmpeg_core::named_optional_field!(deserialize_taper<T>, T, "taper");
cadmpeg_core::named_optional_field!(deserialize_target_position<T>, T, "target_position");
cadmpeg_core::named_optional_field!(deserialize_target_velocity<T>, T, "target_velocity");
cadmpeg_core::named_optional_field!(deserialize_x<T>, T, "x");
cadmpeg_core::named_optional_field!(deserialize_y<T>, T, "y");
cadmpeg_core::named_optional_field!(deserialize_z<T>, T, "z");

#[cfg(test)]
pub(super) mod tests;
