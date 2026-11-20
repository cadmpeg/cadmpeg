// SPDX-License-Identifier: Apache-2.0
//! Overrides of the occurrences below an occurrence.

use cadmpeg_core::text::NonBlankString;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::design::selection::OccurrencePath;
use crate::transform::Transform;

/// An override of one occurrence below an occurrence, by its path from the
/// occurrence. For one path, a field that a configuration's entry states wins
/// over its parent's resolved entry, which wins over the base list's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct NestedOverride {
    /// The nested occurrence's path. It states no step configuration.
    pub path: OccurrencePath,
    /// The nested occurrence's placement.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_transform"
    )]
    pub transform: Option<Transform>,
    /// Whether the nested occurrence is suppressed.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_suppressed"
    )]
    pub suppressed: Option<bool>,
    /// Whether the nested occurrence is visible.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_visible"
    )]
    pub visible: Option<bool>,
    /// The representation of its prototype that the nested occurrence shows.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_representation"
    )]
    pub representation: Option<NonBlankString>,
}

cadmpeg_core::named_optional_field!(deserialize_transform, Transform, "transform");
cadmpeg_core::named_optional_field!(deserialize_suppressed, bool, "suppressed");
cadmpeg_core::named_optional_field!(deserialize_visible, bool, "visible");
cadmpeg_core::named_optional_field!(deserialize_representation, NonBlankString, "representation");
