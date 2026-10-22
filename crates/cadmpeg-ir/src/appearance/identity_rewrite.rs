// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{Appearance, AppearanceBinding, AppearanceTarget, BumpMap, TextureMap2d, TextureRef};

rewrite_record!(Appearance, []; {id, native_ref, name, asset_guid, library_id, visual_guid, physical_token, schema, category, base_color, properties, textures});
rewrite_record!(AppearanceBinding, []; {id, native_ref, target, appearance, source_entity_id, object_type, visible, channels});
impl crate::schema::rewrite::typed::RewriteIdentities for AppearanceTarget {
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>>(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut crate::schema::rewrite::typed::IdentityMap<'_, F>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        <crate::attributes::AttributeTarget as crate::schema::rewrite::typed::RewriteIdentities>::rewrite_native_value(ctx, value, map)
    }

    fn visit_identity_references(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        match self {
            Self::Body(id) => visitor(id.as_str()),
            Self::Occurrence(id) => visitor(id.as_str()),
            Self::Face(id) => visitor(id.as_str()),
            Self::Edge(id) => visitor(id.as_str()),
            Self::Vertex(id) => visitor(id.as_str()),
            Self::Surface(id) => visitor(id.as_str()),
            Self::Curve(id) => visitor(id.as_str()),
            Self::Point(id) => visitor(id.as_str()),
            Self::Tessellation(id) | Self::Record { id, .. } => visitor(id),
            Self::Source { source_id } => source_id.visit_identity_references(_ctx, visitor),
        }
    }

    fn rewrite_identities<F: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>>(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        map: &mut crate::schema::rewrite::typed::IdentityMap<'_, F>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        Ok(match self {
            Self::Body(id) => Self::Body(id.rewrite_identities(ctx, map)?),
            Self::Occurrence(id) => Self::Occurrence(id.rewrite_identities(ctx, map)?),
            Self::Face(id) => Self::Face(id.rewrite_identities(ctx, map)?),
            Self::Edge(id) => Self::Edge(id.rewrite_identities(ctx, map)?),
            Self::Vertex(id) => Self::Vertex(id.rewrite_identities(ctx, map)?),
            Self::Surface(id) => Self::Surface(id.rewrite_identities(ctx, map)?),
            Self::Curve(id) => Self::Curve(id.rewrite_identities(ctx, map)?),
            Self::Point(id) => Self::Point(id.rewrite_identities(ctx, map)?),
            Self::Tessellation(id) => Self::Tessellation(map.identity(&id)?),
            Self::Source { source_id } => Self::Source {
                source_id: source_id.rewrite_identities(ctx, map)?,
            },
            Self::Record { arena, id } => Self::Record {
                arena,
                id: map.identity(&id)?,
            },
        })
    }
}
rewrite_record!(BumpMap, []; {normal_map, depth, normal_scale});
rewrite_record!(TextureMap2d, []; {map_channel, uvw_source, u_offset, v_offset, u_scale, v_scale, rotation, repeat_u, repeat_v, real_world_offset_x, real_world_offset_y, real_world_scale_x, real_world_scale_y});
rewrite_record!(TextureRef, []; {asset_guid, slot, schema, paths, urn, mapping, bump});
