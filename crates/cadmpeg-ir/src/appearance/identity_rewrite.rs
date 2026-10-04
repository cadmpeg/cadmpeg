// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{Appearance, AppearanceBinding, AppearanceTarget, BumpMap, TextureMap2d, TextureRef};

rewrite_record!(Appearance, []; {id, name, asset_guid, library_id, visual_guid, physical_token, schema, category, base_color, properties, textures});
rewrite_record!(AppearanceBinding, []; {id, target, appearance, source_entity_id, object_type, visible, channels});
rewrite_enum!(AppearanceTarget, []; {
    Body(field0),
    Face(field0),
    Edge(field0),
    Vertex(field0),
    Surface(field0),
    Curve(field0),
    Point(field0),
    Tessellation(field0),
    Source {source_id},
});
rewrite_record!(BumpMap, []; {normal_map, depth, normal_scale});
rewrite_record!(TextureMap2d, []; {map_channel, uvw_source, u_offset, v_offset, u_scale, v_scale, rotation, repeat_u, repeat_v, real_world_offset_x, real_world_offset_y, real_world_scale_x, real_world_scale_y});
rewrite_record!(TextureRef, []; {asset_guid, slot, schema, paths, urn, mapping, bump});
