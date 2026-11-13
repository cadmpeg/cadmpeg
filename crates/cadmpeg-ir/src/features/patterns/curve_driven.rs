// SPDX-License-Identifier: Apache-2.0

use super::{PathRef, PositiveLength};
use crate::scalar::FiniteReal;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[doc = "Independently decoded inputs and controls of a curve-driven pattern."]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CurveDrivenPatternParameters {
    #[doc = "Ordered pattern path."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_path"
    )]
    pub path: Option<PathRef>,
    #[doc = "Total instance count, including the seed."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_count"
    )]
    pub count: Option<u32>,
    #[doc = "Arc-length spacing between adjacent instances."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_spacing"
    )]
    pub spacing: Option<PositiveLength>,
    #[doc = "Source distance before conversion from extent to adjacent spacing."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_distance"
    )]
    pub distance: Option<PositiveLength>,
    #[doc = "Whether distance spans from the seed to the last instance."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_total_extent"
    )]
    pub total_extent: Option<bool>,
    #[doc = "Whether instances follow the reverse path direction."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_reversed"
    )]
    pub reversed: Option<bool>,
    #[doc = "Whether each instance turns with the path direction."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_path_oriented"
    )]
    pub path_oriented: Option<bool>,
    #[doc = "Source path parameter at the pattern start."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_start_parameter"
    )]
    pub start_parameter: Option<FiniteReal>,
}

cadmpeg_core::named_optional_field!(deserialize_path, PathRef, "path");
cadmpeg_core::named_optional_field!(deserialize_count, u32, "count");
cadmpeg_core::named_optional_field!(deserialize_spacing, PositiveLength, "spacing");
cadmpeg_core::named_optional_field!(deserialize_distance, PositiveLength, "distance");
cadmpeg_core::named_optional_field!(deserialize_total_extent, bool, "total_extent");
cadmpeg_core::named_optional_field!(deserialize_reversed, bool, "reversed");
cadmpeg_core::named_optional_field!(deserialize_path_oriented, bool, "path_oriented");
cadmpeg_core::named_optional_field!(deserialize_start_parameter, FiniteReal, "start_parameter");
