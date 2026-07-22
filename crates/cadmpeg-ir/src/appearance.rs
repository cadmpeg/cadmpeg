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
    /// Texture assets connected to shader input slots.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub textures: Vec<TextureRef>,
}

/// One texture asset connected to an appearance shader slot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TextureRef {
    /// Stable source asset GUID.
    pub asset_guid: String,
    /// Shader property receiving this texture, such as `generic_diffuse`.
    pub slot: String,
    /// Texture schema family, such as `UnifiedBitmapSchema`.
    pub schema: String,
    /// Ordered library resource paths stored by the texture asset.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    /// External asset-library URN, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub urn: Option<String>,
    /// Two-dimensional texture-coordinate mapping.
    pub mapping: TextureMap2d,
    /// Bump/normal interpretation for a bump texture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bump: Option<BumpMap>,
}

/// Neutral two-dimensional texture-coordinate mapping.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TextureMap2d {
    /// Source mapping channel.
    pub map_channel: u32,
    /// Source UVW mapping mode.
    pub uvw_source: u32,
    /// U-coordinate offset.
    pub u_offset: f64,
    /// V-coordinate offset.
    pub v_offset: f64,
    /// U-coordinate scale.
    pub u_scale: f64,
    /// V-coordinate scale.
    pub v_scale: f64,
    /// Counterclockwise texture rotation in radians.
    pub rotation: f64,
    /// Whether the texture repeats along U.
    pub repeat_u: bool,
    /// Whether the texture repeats along V.
    pub repeat_v: bool,
    /// Real-world X offset in millimetres.
    pub real_world_offset_x: f64,
    /// Real-world Y offset in millimetres.
    pub real_world_offset_y: f64,
    /// Real-world X scale in millimetres.
    pub real_world_scale_x: f64,
    /// Real-world Y scale in millimetres.
    pub real_world_scale_y: f64,
}

/// Bump-map interpretation and amplitudes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BumpMap {
    /// Whether the bitmap stores tangent-space normals instead of heights.
    pub normal_map: bool,
    /// Height-map depth in millimetres.
    pub depth: f64,
    /// Unitless normal-map amplitude.
    pub normal_scale: f64,
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
