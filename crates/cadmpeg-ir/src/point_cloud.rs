// SPDX-License-Identifier: Apache-2.0
//! Point clouds: sampled or measured points, each with optional per-point
//! values, in the frame of a product definition.

use cadmpeg_core::text::NonBlankString;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::features::geometry::{FinitePoint3, FiniteVector3};
use crate::ids::{PointCloudId, ProductDefinitionId};
use crate::scalar::FiniteReal;
use crate::topology::Color;

/// One point cloud.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PointCloud {
    /// Stable point cloud identity.
    pub id: PointCloudId,
    /// Product definition that holds the point cloud, in whose frame the
    /// points lie.
    pub definition: ProductDefinitionId,
    /// Source name.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_name"
    )]
    pub name: Option<NonBlankString>,
    /// Point positions in document length units.
    pub positions: Vec<FinitePoint3>,
    /// The normal vector the source stores at each point, or none. It need
    /// not be a unit vector.
    pub normals: Vec<FiniteVector3>,
    /// One color for each point, or none.
    pub colors: Vec<Color>,
    /// Named scalar values, one for each point in each channel.
    pub channels: Vec<PointChannel>,
    /// Identifier of the full-fidelity source record.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// One named per-point scalar, such as intensity, confidence, or a
/// classification code.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PointChannel {
    /// Channel name, unique in its point cloud.
    pub name: NonBlankString,
    /// One value for each point, in point order.
    pub values: Vec<FiniteReal>,
}

cadmpeg_core::named_optional_field!(deserialize_name, NonBlankString, "name");
cadmpeg_core::named_optional_field!(deserialize_native_ref, String, "native_ref");

mod identity_rewrite;
