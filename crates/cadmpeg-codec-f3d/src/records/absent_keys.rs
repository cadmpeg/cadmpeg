// SPDX-License-Identifier: Apache-2.0
//! The named readers every optional record key is declared with.
//!
//! The declarations live beside their records rather than among them: the
//! record file is at the production-size limit `docs/source-policy.md` states.

use super::{
    BodyId, ConstructionRecipeSelector, DesignMeshSceneBoundsWire, DesignMeshUuid,
    DesignSketchVisibility, MeshAffineTransform, Point2, SketchCurveGeometry, SketchLinkSense,
    SketchPointClosureSerde, XrefPlacementTransform,
};

cadmpeg_core::named_optional_field!(pub(super) deserialize_sense, SketchLinkSense, "sense");
cadmpeg_core::named_optional_field!(pub(super) deserialize_record_index_offset, u64, "record_index_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_design_id, String, "design_id");
cadmpeg_core::named_optional_field!(pub(super) deserialize_design_id_offset, u64, "design_id_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_design_selector, ConstructionRecipeSelector, "design_selector");
cadmpeg_core::named_optional_field!(pub(super) deserialize_record_index, i32, "record_index");
cadmpeg_core::named_optional_field!(pub(super) deserialize_family_discriminator, u64, "family_discriminator");
cadmpeg_core::named_optional_field!(pub(super) deserialize_family_discriminator_offset, u64, "family_discriminator_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_owner_record_index, u32, "owner_record_index");
cadmpeg_core::named_optional_field!(pub(super) deserialize_unit, String, "unit");
cadmpeg_core::named_optional_field!(pub(super) deserialize_unit_offset, u64, "unit_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_variant, u8, "variant");
cadmpeg_core::named_optional_field!(pub(super) deserialize_payload_byte_offset, u64, "payload_byte_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_payload_byte_length, u64, "payload_byte_length");
cadmpeg_core::named_optional_field!(pub(super) deserialize_opaque_index, u32, "opaque_index");
cadmpeg_core::named_optional_field!(pub(super) deserialize_opaque_index_offset, u64, "opaque_index_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_companion_record_index, u32, "companion_record_index");
cadmpeg_core::named_optional_field!(pub(super) deserialize_scope_record_index, u32, "scope_record_index");
cadmpeg_core::named_optional_field!(pub(super) deserialize_visibility, DesignSketchVisibility, "visibility");
cadmpeg_core::named_optional_field!(pub(super) deserialize_transform_offset, u64, "transform_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_physical_token, String, "physical_token");
cadmpeg_core::named_optional_field!(pub(super) deserialize_physical_token_offset, u64, "physical_token_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_visual_preset, String, "visual_preset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_visual_preset_offset, u64, "visual_preset_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_base_type_guid, String, "base_type_guid");
cadmpeg_core::named_optional_field!(pub(super) deserialize_base_type_guid_offset, u64, "base_type_guid_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_module, String, "module");
cadmpeg_core::named_optional_field!(pub(super) deserialize_record_reference, u32, "record_reference");
cadmpeg_core::named_optional_field!(pub(super) deserialize_record_reference_offset, u64, "record_reference_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_declared_reference_count, usize, "declared_reference_count");
cadmpeg_core::named_optional_field!(pub(super) deserialize_scene_state_bounds, DesignMeshSceneBoundsWire, "scene_state_bounds");
cadmpeg_core::named_optional_field!(pub(super) deserialize_scene_node_bounds, DesignMeshSceneBoundsWire, "scene_node_bounds");
cadmpeg_core::named_optional_field!(pub(super) deserialize_scene_node_transform, MeshAffineTransform, "scene_node_transform");
cadmpeg_core::named_optional_field!(pub(super) deserialize_scene_node_transform_offset, u64, "scene_node_transform_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_container_mesh_uuid, DesignMeshUuid, "container_mesh_uuid");
cadmpeg_core::named_optional_field!(pub(super) deserialize_tessellation_id, String, "tessellation_id");
cadmpeg_core::named_optional_field!(pub(super) deserialize_rectangular_counted_reference_count, u32, "rectangular_counted_reference_count");
cadmpeg_core::named_optional_field!(pub(super) deserialize_entity_genesis, u64, "entity_genesis");
cadmpeg_core::named_optional_field!(pub(super) deserialize_persistent_id, u64, "persistent_id");
cadmpeg_core::named_optional_field!(pub(super) deserialize_base_id, u64, "base_id");
cadmpeg_core::named_optional_field!(pub(super) deserialize_width_factor, f64, "width_factor");
cadmpeg_core::named_optional_field!(pub(super) deserialize_anchor, Point2, "anchor");
cadmpeg_core::named_optional_field!(pub(super) deserialize_rotation, f64, "rotation");
cadmpeg_core::named_optional_field!(pub(super) deserialize_horizontal_alignment, u32, "horizontal_alignment");
cadmpeg_core::named_optional_field!(pub(super) deserialize_vertical_alignment, u32, "vertical_alignment");
cadmpeg_core::named_optional_field!(pub(super) deserialize_first_reference, u32, "first_reference");
cadmpeg_core::named_optional_field!(pub(super) deserialize_second_reference, u32, "second_reference");
cadmpeg_core::named_optional_field!(pub(super) deserialize_owner_reference, u32, "owner_reference");
cadmpeg_core::named_optional_field!(pub(super) deserialize_closure, SketchPointClosureSerde, "closure");
cadmpeg_core::named_optional_field!(pub(super) deserialize_geometry, SketchCurveGeometry, "geometry");
cadmpeg_core::named_optional_field!(pub(super) deserialize_body, BodyId, "body");
cadmpeg_core::named_optional_field!(pub(super) deserialize_table_record_index_offset, u64, "table_record_index_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_channel_record_index_offset, u64, "channel_record_index_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_table_entity_id_offset, u64, "table_entity_id_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_channel_entity_id_offset, u64, "channel_entity_id_offset");
cadmpeg_core::named_optional_field!(pub(super) deserialize_channel_class_tag, String, "channel_class_tag");
cadmpeg_core::named_optional_field!(pub(super) deserialize_transform, XrefPlacementTransform, "transform");
