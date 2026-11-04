// SPDX-License-Identifier: Apache-2.0
//! Point clouds: sampled or measured points, each with optional per-point
//! values, in the frame of a product definition.

use cadmpeg_core::text::NonBlankString;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::features::FinitePoint3;
use crate::ids::{PointCloudId, ProductDefinitionId};
use crate::scalar::FiniteReal;
use crate::topology::Color;
use crate::transform::Transform;
use crate::units::UnitVector3;

/// One point cloud.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PointCloud {
    /// Stable point cloud identity.
    pub id: PointCloudId,
    /// Product definition whose frame holds the points.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_product_definition"
    )]
    pub product_definition: Option<ProductDefinitionId>,
    /// Source name.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_name"
    )]
    pub name: Option<String>,
    /// Pose of the capturing sensor in the frame of the points.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_sensor"
    )]
    pub sensor: Option<Transform>,
    /// Point positions in document length units.
    pub positions: Vec<FinitePoint3>,
    /// One unit normal for each point, or none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub normals: Vec<UnitVector3>,
    /// One color for each point, or none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub colors: Vec<Color>,
    /// Named scalar values, one for each point in each channel.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
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

cadmpeg_core::named_optional_field!(
    deserialize_product_definition,
    ProductDefinitionId,
    "product_definition"
);
cadmpeg_core::named_optional_field!(deserialize_name, String, "name");
cadmpeg_core::named_optional_field!(deserialize_sensor, Transform, "sensor");
cadmpeg_core::named_optional_field!(deserialize_native_ref, String, "native_ref");

mod identity_rewrite;
