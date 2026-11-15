// SPDX-License-Identifier: Apache-2.0
//! Types that more than one operation family shares.

use cadmpeg_core::text::NonBlankString;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::design::construction::{kinds, Construction};
use crate::design::dimension::Dimension;
use crate::design::options::LimitOffset;
use crate::design::selection::SetSelection;

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
    AtOrigin,
    /// A signed distance or angle from the origin.
    Amount(Dimension<Q>),
    /// Through every target body; in a revolve, a full turn.
    ThroughAll,
    /// At the first face the swept profile meets.
    Next,
    /// At the last face the swept profile meets.
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
    Linear,
    /// One smooth surface passes through every section.
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
    Round,
    /// Sharp intersection: the sides extend to meet.
    Sharp,
    /// A source join whose meaning is not established.
    Native(NonBlankString),
}

cadmpeg_core::named_optional_field!(deserialize_extend, Dimension<bool>, "extend");
cadmpeg_core::named_optional_field!(deserialize_far, Dimension<bool>, "far");
cadmpeg_core::named_optional_field!(deserialize_offset, LimitOffset, "offset");
