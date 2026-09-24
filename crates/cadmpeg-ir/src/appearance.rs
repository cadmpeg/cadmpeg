// SPDX-License-Identifier: Apache-2.0
//! Material and visual-appearance assets plus topology bindings.

use std::collections::BTreeMap;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::{AppearanceId, BodyId, EdgeId, FaceId, VertexId};
use crate::scalar::{Angle, FiniteReal, Length};
use crate::topology::Color;
use cadmpeg_core::text::NonBlankString;

/// A decoded appearance/material asset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Appearance {
    /// Stable arena id.
    pub id: AppearanceId,
    /// Display/preset name stored in the source, when present.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_name"
    )]
    pub name: Option<String>,
    /// Asset GUID stored in the Protein record.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_asset_guid"
    )]
    pub asset_guid: Option<String>,
    /// External library holding the preset named by `name`: a GUID for a shipped
    /// library, a path for a user library.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_library_id"
    )]
    pub library_id: Option<String>,
    /// Visual asset GUID stored in the source, when present.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_visual_guid"
    )]
    pub visual_guid: Option<String>,
    /// Physical-material token stored in the source, when present.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_physical_token"
    )]
    pub physical_token: Option<String>,
    /// Source schema family, such as `GenericSchema` or `PrismOpaqueSchema`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_schema"
    )]
    pub schema: Option<String>,
    /// Source material classification, when stored in the asset catalog.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_category"
    )]
    pub category: Option<String>,
    /// Resolved diffuse/albedo color.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_base_color"
    )]
    pub base_color: Option<Color>,
    /// Additional byte-decoded shader scalars keyed by schema property name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    #[cfg_attr(feature = "schema", schemars(with = "BTreeMap<NonBlankString, f64>"))]
    pub properties: BTreeMap<NonBlankString, FiniteReal>,
    /// Texture assets connected to shader input slots.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub textures: Vec<TextureRef>,
}

/// One texture asset connected to an appearance shader slot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_urn"
    )]
    pub urn: Option<String>,
    /// Two-dimensional texture-coordinate mapping.
    pub mapping: TextureMap2d,
    /// Bump/normal interpretation for a bump texture.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_bump"
    )]
    pub bump: Option<BumpMap>,
}

/// Neutral two-dimensional texture-coordinate mapping.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TextureMap2d {
    /// Source mapping channel.
    pub map_channel: u32,
    /// Source UVW mapping mode.
    pub uvw_source: u32,
    /// U-coordinate offset.
    #[cfg_attr(feature = "schema", schemars(with = "f64"))]
    pub u_offset: FiniteReal,
    /// V-coordinate offset.
    #[cfg_attr(feature = "schema", schemars(with = "f64"))]
    pub v_offset: FiniteReal,
    /// U-coordinate scale.
    #[cfg_attr(feature = "schema", schemars(with = "f64"))]
    pub u_scale: FiniteReal,
    /// V-coordinate scale.
    #[cfg_attr(feature = "schema", schemars(with = "f64"))]
    pub v_scale: FiniteReal,
    /// Counterclockwise texture rotation in radians.
    #[cfg_attr(feature = "schema", schemars(with = "f64"))]
    pub rotation: Angle,
    /// Whether the texture repeats along U.
    pub repeat_u: bool,
    /// Whether the texture repeats along V.
    pub repeat_v: bool,
    /// Real-world X offset in millimetres.
    #[cfg_attr(feature = "schema", schemars(with = "f64"))]
    pub real_world_offset_x: Length,
    /// Real-world Y offset in millimetres.
    #[cfg_attr(feature = "schema", schemars(with = "f64"))]
    pub real_world_offset_y: Length,
    /// Real-world X scale in millimetres.
    #[cfg_attr(feature = "schema", schemars(with = "f64"))]
    pub real_world_scale_x: Length,
    /// Real-world Y scale in millimetres.
    #[cfg_attr(feature = "schema", schemars(with = "f64"))]
    pub real_world_scale_y: Length,
}

/// Bump-map interpretation and amplitudes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BumpMap {
    /// Whether the bitmap stores tangent-space normals instead of heights.
    pub normal_map: bool,
    /// Height-map depth in millimetres.
    #[cfg_attr(feature = "schema", schemars(with = "f64"))]
    pub depth: Length,
    /// Unitless normal-map amplitude.
    #[cfg_attr(feature = "schema", schemars(with = "f64"))]
    pub normal_scale: FiniteReal,
}

/// A topology entity which receives an appearance.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct AppearanceBinding {
    /// Globally unique deterministic assignment identity.
    pub id: crate::ids::AppearanceBindingId,
    /// Assigned topology entity.
    pub target: AppearanceTarget,
    /// Referenced appearance asset.
    pub appearance: AppearanceId,
    /// Fusion design-entity id, such as `0_985`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_source_entity_id"
    )]
    pub source_entity_id: Option<String>,
    /// Design `MetaStream` object type, such as `Body`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_object_type"
    )]
    pub object_type: Option<String>,
    /// Whether this presentation binding is visible; `None` means that the
    /// source does not provide an explicit binding-level visibility value.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_visible"
    )]
    pub visible: Option<bool>,
    /// ACT change-version channel GUIDs for this assigned entity.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub channels: BTreeMap<NonBlankString, String>,
}

#[cfg(test)]
mod tests;

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_name, String, "name");
cadmpeg_core::named_optional_field!(deserialize_asset_guid, String, "asset_guid");
cadmpeg_core::named_optional_field!(deserialize_library_id, String, "library_id");
cadmpeg_core::named_optional_field!(deserialize_visual_guid, String, "visual_guid");
cadmpeg_core::named_optional_field!(deserialize_physical_token, String, "physical_token");
cadmpeg_core::named_optional_field!(deserialize_schema, String, "schema");
cadmpeg_core::named_optional_field!(deserialize_category, String, "category");
cadmpeg_core::named_optional_field!(deserialize_base_color, Color, "base_color");
cadmpeg_core::named_optional_field!(deserialize_urn, String, "urn");
cadmpeg_core::named_optional_field!(deserialize_bump, BumpMap, "bump");
cadmpeg_core::named_optional_field!(deserialize_source_entity_id, String, "source_entity_id");
cadmpeg_core::named_optional_field!(deserialize_object_type, String, "object_type");
cadmpeg_core::named_optional_field!(deserialize_visible, bool, "visible");
