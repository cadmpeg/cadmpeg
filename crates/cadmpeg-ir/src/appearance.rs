// SPDX-License-Identifier: Apache-2.0
//! Material and visual-appearance assets plus topology bindings.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::{AppearanceId, BodyId, EdgeId, FaceId, VertexId};
use crate::topology::Color;

/// A decoded appearance/material asset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Appearance {
    /// Stable arena id.
    pub id: AppearanceId,
    /// Display/preset name stored in the source, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Asset GUID stored in the Protein record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_guid: Option<String>,
    /// Visual asset GUID stored in the source, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visual_guid: Option<String>,
    /// Physical-material token stored in the source, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub physical_token: Option<String>,
    /// Source schema family, such as `GenericSchema` or `PrismOpaqueSchema`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// Source material classification, when stored in the asset catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Resolved diffuse/albedo color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_color: Option<Color>,
    /// Additional byte-decoded shader scalars keyed by schema property name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, f64>,
}

/// A topology entity which receives an appearance.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum AppearanceTarget {
    /// Whole-body appearance.
    Body(BodyId),
    /// Per-face appearance override.
    Face(FaceId),
    /// Per-edge line appearance.
    Edge(EdgeId),
    /// Per-vertex point appearance.
    Vertex(VertexId),
    /// Standalone surface geometry appearance.
    Surface(crate::ids::SurfaceId),
    /// Standalone curve geometry appearance.
    Curve(crate::ids::CurveId),
    /// Standalone point geometry appearance.
    Point(crate::ids::PointId),
    /// Tessellated geometry appearance.
    Tessellation(String),
    /// Native presentation carrier without a neutral geometry arena.
    Source {
        /// Native source entity identity.
        source_id: String,
    },
}

/// An explicit appearance assignment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AppearanceBinding {
    /// Globally unique deterministic assignment identity.
    pub id: String,
    /// Assigned topology entity.
    pub target: AppearanceTarget,
    /// Referenced appearance asset.
    pub appearance: AppearanceId,
    /// Fusion design-entity id, such as `0_985`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_entity_id: Option<String>,
    /// Design `MetaStream` object type, such as `Body`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_type: Option<String>,
    /// ACT change-version channel GUIDs for this assigned entity.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub channels: BTreeMap<String, String>,
}
