// SPDX-License-Identifier: Apache-2.0
//! Types that more than one family of the feature vocabulary uses.
//!
//! The layouts of patterns, which part patterns (§6.5) and component patterns
//! (§6.7) share, with their runs, spacings, and instance orientations, and the
//! element of a pair that sheet metal (§6.6) and joints (§6.7) name.

use cadmpeg_core::text::NonBlankString;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::design::construction::{kinds, Construction, FrameAxis, ScalarDimension};
use crate::design::dimension::Dimension;
use crate::design::options::{PathRef, SymmetricMeasure};
use crate::design::selection::OrderedSelection;
use crate::features::ParameterId;
use crate::scalar::{Angle, Length, PositiveReal};

/// One of the two elements of a pair: the first or the second.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PairElement {
    /// The first element.
    First,
    /// The second element.
    Second,
}

/// Where a pattern places its instances. Every direction, axis, and plane is a
/// construction (§5.9); a reversed direction is a `Flip` step of the
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
        /// The second run, where the source stores one.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_second"
        )]
        second: Option<LinearRun>,
        /// Whether the second run repeats only the seed, not the first run's
        /// instances, so the instances that exist are `[i, 0]` and `[0, j]`.
        /// It occurs only with a `second` run.
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
        /// The axis.
        axis: Construction<kinds::Axis>,
        /// The angular run.
        angular: PatternRun<Angle>,
        /// The radial run outward from the axis.
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
        /// The spacing by arc length.
        run: PatternRun<Length>,
        /// Whether an instance keeps the seed's orientation, turns tangent to
        /// the path, or follows the path's frame.
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
        /// The point of the seed that lands on each.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_reference"
        )]
        reference: Option<Locus>,
    },
    /// A dimension pattern: each run has a count and the seed dimensions it
    /// increments, so there is no direction.
    Dimensions(Vec<DimensionRun>),
    /// A table pattern: one row per instance, with a value per table
    /// dimension.
    Table(DimensionTable),
    /// The seeds reflected across `plane`. Its one instance is the
    /// reflection.
    Mirror {
        /// The mirror plane.
        plane: Construction<kinds::Plane>,
        /// The tolerance with which the mirror stitches the reflection to its
        /// seed.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_stitch_tolerance"
        )]
        stitch_tolerance: Option<Dimension<Length>>,
    },
    /// Stages applied in order, each to every instance the earlier stages
    /// produced. No stage is a `Composite`.
    Composite(Vec<PatternLayout>),
    /// `count` instances, the seed included, scaled uniformly about `center`
    /// so that the last is scaled by `factor`.
    Scale {
        /// The scale of the last instance.
        factor: Dimension<PositiveReal>,
        /// The number of instances.
        count: Dimension<u32>,
        /// The centre of the scaling.
        center: Locus,
    },
    /// The instance set and indices of another pattern, which its one pick
    /// names.
    Follow(OrderedSelection),
    /// Components along a planar path. It occurs only in a component pattern.
    Chain(ChainLayout),
    /// A layout whose meaning is not established. Its parameters go to
    /// `native_options` under their source names.
    Native(NonBlankString),
}

/// Components placed along a path that lies in one plane, each instance
/// keeping the seed's relation to that plane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ChainLayout {
    /// The planar path.
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
    /// The seeds in turn, one link per seed in the order the seeds alternate.
    Connected(Vec<ChainLink>),
    /// A linkage whose meaning is not established, with its values as the
    /// pattern's native options.
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
    /// Two axes of the seed's own geometry, both on the path. Each link's
    /// first pin axis is the previous link's second.
    pub pins: [Construction<kinds::Axis>; 2],
}

/// A linear run along its own direction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct LinearRun {
    /// The direction of the run.
    pub direction: Construction<kinds::Direction>,
    /// The spacing of the run.
    pub run: PatternRun<Length>,
}

/// A run of instances.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    deny_unknown_fields,
    bound(serialize = "Q: Serialize", deserialize = "Q: Deserialize<'de>")
)]
pub struct PatternRun<Q> {
    /// The spacing. Every count includes the seed.
    pub spacing: Spacing<Q>,
    /// Where the run places its instances. `None` states nothing.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_sides"
    )]
    pub sides: Option<RunSides<Q>>,
    /// The seed's step count from the run's first instance, so the run's
    /// instances span coordinates −`seed` to count − 1 − `seed`. `None` places
    /// the seed first.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_seed"
    )]
    pub seed: Option<Dimension<u32>>,
    /// Changes of listed parameters of the seed definition by `step` per
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
    One,
    /// Against the run's direction from the seed, so the instances'
    /// coordinates are negative. It occurs only in a `Path` run.
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
    Centroid {},
    /// A constructed point.
    Point(Box<Construction<kinds::Point>>),
}

/// Whether a circular or path instance keeps the seed's orientation, turns
/// with the layout, or follows the path's frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum InstanceOrientation {
    /// The instance keeps the seed's orientation.
    Fixed,
    /// The instance turns with the layout: about the axis, or tangent to the
    /// path.
    Follow,
    /// The instance follows the path's Frenet frame.
    Frenet,
    /// The instance's x axis on the path tangent and its `reference_axis` on
    /// the `reference` direction made orthogonal to the tangent, the third
    /// axis completing a right-handed frame, after turning the seed so that
    /// its `seed_axis`, or its x axis where that is `None`, lies on its x
    /// axis.
    Rolled {
        /// The direction the `reference_axis` lies on.
        reference: PathReference,
        /// The axis of the instance on `reference`: `Y` or `Z`.
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

/// The direction a rolled path instance's reference axis lies on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PathReference {
    /// The normal of the path's plane on the side of the z axis of the path's
    /// placement, and the definition's +Z for a path that is not planar.
    PlaneNormal {},
    /// A constructed direction.
    Direction(Box<Construction<kinds::Direction>>),
}

/// A change of a seed parameter by `step` per coordinate along a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Increment {
    /// The parameter of the seed definition.
    pub parameter: ParameterId,
    /// The change per coordinate. It is a dimension, so a parameter can drive
    /// it.
    pub step: ScalarDimension,
}

/// One run of a dimension pattern.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DimensionRun {
    /// The number of instances.
    pub count: Dimension<u32>,
    /// The seed dimensions the run increments.
    pub increments: Vec<Increment>,
}

/// The table of a table pattern.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DimensionTable {
    /// The table dimensions.
    pub parameters: Vec<ParameterId>,
    /// One row per instance.
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

cadmpeg_core::named_optional_field!(deserialize_count<T>, T, "count");
cadmpeg_core::named_optional_field!(deserialize_extent<T>, T, "extent");
cadmpeg_core::named_optional_field!(deserialize_from<T>, T, "from");
cadmpeg_core::named_optional_field!(deserialize_offset<T>, T, "offset");
cadmpeg_core::named_optional_field!(deserialize_orientation<T>, T, "orientation");
cadmpeg_core::named_optional_field!(deserialize_radial<T>, T, "radial");
cadmpeg_core::named_optional_field!(deserialize_reference<T>, T, "reference");
cadmpeg_core::named_optional_field!(deserialize_second<T>, T, "second");
cadmpeg_core::named_optional_field!(deserialize_seed<T>, T, "seed");
cadmpeg_core::named_optional_field!(deserialize_seed_axis<T>, T, "seed_axis");
cadmpeg_core::named_optional_field!(deserialize_seed_only<T>, T, "seed_only");
cadmpeg_core::named_optional_field!(deserialize_sides<T>, T, "sides");
cadmpeg_core::named_optional_field!(deserialize_stitch_tolerance<T>, T, "stitch_tolerance");
