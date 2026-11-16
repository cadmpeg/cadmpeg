// SPDX-License-Identifier: Apache-2.0
//! Types that more than one family of the feature vocabulary uses.
//!
//! The status of solved values, the link of a frozen copy to its source, and
//! the spacing and sides of a pattern run.

use cadmpeg_core::text::NonBlankString;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::design::construction::{kinds, Construction};
use crate::design::dimension::Dimension;
use crate::design::options::SymmetricMeasure;
use crate::design::selection::{DefinitionRef, OrderedSelection, SetSelection};
use crate::scalar::FiniteReal;
use crate::transform::Transform;

/// The status of stored solved values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum EvaluationStatus {
    /// The source does not flag the values.
    Current,
    /// The values predate an edit of what they depend on.
    Stale,
    /// The source records a failure to compute the values.
    Failed,
}

/// The link of a frozen copy of geometry to the geometry it copies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Link {
    /// The linked entities. Its picks' contexts state the source definition,
    /// the assembly the link runs through, and the configuration each level
    /// reads, and its anchor the state it reads.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_source"
    )]
    pub source: Option<SetSelection>,
    /// The source definition, and the configuration of it the link reads,
    /// only where `source` is absent or unread.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_document"
    )]
    pub document: Option<DefinitionRef>,
    /// A frozen placement of the copy.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_transform"
    )]
    pub transform: Option<Transform>,
    /// A uniform scale of the copy.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_scale"
    )]
    pub scale: Option<FiniteReal>,
    /// Whether the copy follows its source.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_associative"
    )]
    pub associative: Option<bool>,
    /// The source's record of the link's health.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_state"
    )]
    pub state: Option<LinkState>,
    /// The native record that holds the link.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// The source's record of a link's associativity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum LinkState {
    /// The copy matches its source.
    Current,
    /// The source changed after the copy was made.
    OutOfDate,
    /// The source does not resolve.
    Broken,
    /// The copy does not follow its source.
    Locked,
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
    Centroid {},
    /// A constructed point.
    Point(Box<Construction<kinds::Point>>),
}

cadmpeg_core::named_optional_field!(deserialize_associative<T>, T, "associative");
cadmpeg_core::named_optional_field!(deserialize_count<T>, T, "count");
cadmpeg_core::named_optional_field!(deserialize_document<T>, T, "document");
cadmpeg_core::named_optional_field!(deserialize_extent<T>, T, "extent");
cadmpeg_core::named_optional_field!(deserialize_from<T>, T, "from");
cadmpeg_core::named_optional_field!(deserialize_native_ref<T>, T, "native_ref");
cadmpeg_core::named_optional_field!(deserialize_offset<T>, T, "offset");
cadmpeg_core::named_optional_field!(deserialize_scale<T>, T, "scale");
cadmpeg_core::named_optional_field!(deserialize_source<T>, T, "source");
cadmpeg_core::named_optional_field!(deserialize_state<T>, T, "state");
cadmpeg_core::named_optional_field!(deserialize_transform<T>, T, "transform");
