// SPDX-License-Identifier: Apache-2.0
//! Byte-offset and value constants generated from `docs/layouts/inventor.toml`.
//!
//! Do not edit by hand. Regenerate with:
//! `UPDATE_LAYOUT_CODE=1 cargo test -p cadmpeg --test layout_tables`.

#![allow(dead_code)] // Not every generated constant is referenced yet.

/// Byte offsets for the `pm_dc_content_header` record.
///
/// Spec §12. Record length 22 B.
pub(crate) mod pm_dc_content_header {
    /// Record length in bytes. Spec §12.
    pub(crate) const LEN: usize = 22;
    /// Offset of `header_value` (`u32`, little-endian). Spec §12.
    pub(crate) const HEADER_VALUE: usize = 0;
    /// Offset of `header_id` (`u16`, little-endian). Spec §12.
    pub(crate) const HEADER_ID: usize = 4;
    /// Offset of `next_reference` (`u32`, little-endian). Spec §12.
    pub(crate) const NEXT_REFERENCE: usize = 6;
    /// Offset of `flags` (`u32`, little-endian). Spec §12.
    pub(crate) const FLAGS: usize = 10;
    /// Offset of `context_reference` (`u32`, little-endian). Spec §12.
    pub(crate) const CONTEXT_REFERENCE: usize = 14;
    /// Offset of `source_index` (`u32`, little-endian). Spec §12.
    pub(crate) const SOURCE_INDEX: usize = 18;
}

/// Byte offsets for the `pm_dc_sketch_entity_header` record.
///
/// Spec §12. Record length 30 B.
pub(crate) mod pm_dc_sketch_entity_header {
    /// Record length in bytes. Spec §12.
    pub(crate) const LEN: usize = 30;
    /// Offset of `content_header` (`bytes[22]`). Spec §12.
    pub(crate) const CONTENT_HEADER: usize = 0;
    /// Offset of `entity_flags` (`u32`, little-endian). Spec §12.
    pub(crate) const ENTITY_FLAGS: usize = 22;
    /// Offset of `sketch_reference` (`u32`, little-endian). Spec §12.
    pub(crate) const SKETCH_REFERENCE: usize = 26;
}

/// Byte offsets for the `pm_dc_reference_list_prefix` record.
///
/// Spec §12. Record length 8 B.
pub(crate) mod pm_dc_reference_list_prefix {
    /// Record length in bytes. Spec §12.
    pub(crate) const LEN: usize = 8;
    /// Offset of `marker` (`u16[2]`, little-endian). Spec §12.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `item_count` (`u32`, little-endian). Spec §12.
    pub(crate) const ITEM_COUNT: usize = 4;
}

/// Byte offsets for the `pm_dc_reference_array_prefix` record.
///
/// Spec §12. Record length 8 B.
pub(crate) mod pm_dc_reference_array_prefix {
    /// Record length in bytes. Spec §12.
    pub(crate) const LEN: usize = 8;
    /// Offset of `marker` (`u16[2]`, little-endian). Spec §12.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `item_count` (`u32`, little-endian). Spec §12.
    pub(crate) const ITEM_COUNT: usize = 4;
}

/// Byte offsets for the `pm_dc_constraint_prefix` record.
///
/// Spec §12. Record length 30 B.
pub(crate) mod pm_dc_constraint_prefix {
    /// Record length in bytes. Spec §12.
    pub(crate) const LEN: usize = 30;
    /// Offset of `content_header` (`bytes[22]`). Spec §12.
    pub(crate) const CONTENT_HEADER: usize = 0;
    /// Offset of `state` (`i32`, little-endian). Spec §12.
    pub(crate) const STATE: usize = 22;
    /// Offset of `group_reference` (`u32`, little-endian). Spec §12.
    pub(crate) const GROUP_REFERENCE: usize = 26;
}

/// Byte offsets for the `pm_dc_constraint_v15_v16_header` record.
///
/// Spec §12. Record length 34 B.
pub(crate) mod pm_dc_constraint_v15_v16_header {
    /// Record length in bytes. Spec §12.
    pub(crate) const LEN: usize = 34;
    /// Offset of `constraint_prefix` (`bytes[30]`). Spec §12.
    pub(crate) const CONSTRAINT_PREFIX: usize = 0;
    /// Offset of `parameter_reference` (`u32`, little-endian). Spec §12.
    pub(crate) const PARAMETER_REFERENCE: usize = 30;
}

/// Byte offsets for the `pm_dc_constraint_map_prefix` record.
///
/// Spec §12. Record length 8 B.
pub(crate) mod pm_dc_constraint_map_prefix {
    /// Record length in bytes. Spec §12.
    pub(crate) const LEN: usize = 8;
    /// Offset of `marker` (`u16[2]`, little-endian). Spec §12.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `item_count` (`u32`, little-endian). Spec §12.
    pub(crate) const ITEM_COUNT: usize = 4;
}

/// Byte offsets for the `pm_dc_feature_prefix` record.
///
/// Spec §13. Record length 38 B.
pub(crate) mod pm_dc_feature_prefix {
    /// Record length in bytes. Spec §13.
    pub(crate) const LEN: usize = 38;
    /// Offset of `content_header` (`bytes[22]`). Spec §13.
    pub(crate) const CONTENT_HEADER: usize = 0;
    /// Offset of `state` (`i32`, little-endian). Spec §13.
    pub(crate) const STATE: usize = 22;
    /// Offset of `outline_value` (`u32`, little-endian). Spec §13.
    pub(crate) const OUTLINE_VALUE: usize = 26;
    /// Offset of `property_list_prefix` (`bytes[8]`). Spec §13.
    pub(crate) const PROPERTY_LIST_PREFIX: usize = 30;
}

/// Byte offsets for the `pm_dc_feature_terminator` record.
///
/// Spec §13. Record length 26 B.
pub(crate) mod pm_dc_feature_terminator {
    /// Record length in bytes. Spec §13.
    pub(crate) const LEN: usize = 26;
    /// Offset of `content_header` (`bytes[22]`). Spec §13.
    pub(crate) const CONTENT_HEADER: usize = 0;
    /// Offset of `state` (`i32`, little-endian). Spec §13.
    pub(crate) const STATE: usize = 22;
}

/// Byte offsets for the `pm_dc_feature_enumeration` record.
///
/// Spec §13. Record length 26 B.
pub(crate) mod pm_dc_feature_enumeration {
    /// Record length in bytes. Spec §13.
    pub(crate) const LEN: usize = 26;
    /// Offset of `content_header` (`bytes[22]`). Spec §13.
    pub(crate) const CONTENT_HEADER: usize = 0;
    /// Offset of `type_value` (`i16`, little-endian). Spec §13.
    pub(crate) const TYPE_VALUE: usize = 22;
    /// Offset of `value` (`u16`, little-endian). Spec §13.
    pub(crate) const VALUE: usize = 24;
}

/// Byte offsets for the `pm_dc_chamfer_enumeration` record.
///
/// Spec §13. Record length 30 B.
pub(crate) mod pm_dc_chamfer_enumeration {
    /// Record length in bytes. Spec §13.
    pub(crate) const LEN: usize = 30;
    /// Offset of `content_header` (`bytes[22]`). Spec §13.
    pub(crate) const CONTENT_HEADER: usize = 0;
    /// Offset of `type_value` (`i16`, little-endian). Spec §13.
    pub(crate) const TYPE_VALUE: usize = 22;
    /// Offset of `value` (`u16`, little-endian). Spec §13.
    pub(crate) const VALUE: usize = 24;
    /// Offset of `terminal_value` (`u32`, little-endian). Spec §13.
    pub(crate) const TERMINAL_VALUE: usize = 26;
}

/// Byte offsets for the `pm_dc_fillet_edge_selection` record.
///
/// Spec §13. Record length 30 B.
pub(crate) mod pm_dc_fillet_edge_selection {
    /// Record length in bytes. Spec §13.
    pub(crate) const LEN: usize = 30;
    /// Offset of `content_header` (`bytes[22]`). Spec §13.
    pub(crate) const CONTENT_HEADER: usize = 0;
    /// Offset of `type_value` (`u32`, little-endian). Spec §13.
    pub(crate) const TYPE_VALUE: usize = 22;
    /// Offset of `value` (`u32`, little-endian). Spec §13.
    pub(crate) const VALUE: usize = 26;
}

/// Byte offsets for the `pm_dc_linked_element_header` record.
///
/// Spec §13. Record length 26 B.
pub(crate) mod pm_dc_linked_element_header {
    /// Record length in bytes. Spec §13.
    pub(crate) const LEN: usize = 26;
    /// Offset of `header_value` (`u32`, little-endian). Spec §13.
    pub(crate) const HEADER_VALUE: usize = 0;
    /// Offset of `header_id` (`u16`, little-endian). Spec §13.
    pub(crate) const HEADER_ID: usize = 4;
    /// Offset of `values` (`u32[2]`, little-endian). Spec §13.
    pub(crate) const VALUES: usize = 6;
    /// Offset of `owner_reference` (`u32`, little-endian). Spec §13.
    pub(crate) const OWNER_REFERENCE: usize = 14;
    /// Offset of `parent_reference` (`u32`, little-endian). Spec §13.
    pub(crate) const PARENT_REFERENCE: usize = 18;
    /// Offset of `next_reference` (`u32`, little-endian). Spec §13.
    pub(crate) const NEXT_REFERENCE: usize = 22;
}

/// Byte offsets for the `pm_dc_parameter_prefix` record.
///
/// Spec §11. Record length 26 B.
///
/// ```text
/// The counted UTF-16LE parameter name starts at byte 22.
/// ```
pub(crate) mod pm_dc_parameter_prefix {
    /// Record length in bytes. Spec §11.
    pub(crate) const LEN: usize = 26;
    /// Offset of `header_value` (`u32`, little-endian). Spec §11.
    pub(crate) const HEADER_VALUE: usize = 0;
    /// Offset of `header_id` (`u16`, little-endian). Spec §11.
    pub(crate) const HEADER_ID: usize = 4;
    /// Offset of `next_reference` (`u32`, little-endian). Spec §11.
    pub(crate) const NEXT_REFERENCE: usize = 6;
    /// Offset of `flags` (`u32`, little-endian). Spec §11.
    pub(crate) const FLAGS: usize = 10;
    /// Offset of `context_reference` (`u32`, little-endian). Spec §11.
    pub(crate) const CONTEXT_REFERENCE: usize = 14;
    /// Offset of `source_index` (`u32`, little-endian). Spec §11.
    pub(crate) const SOURCE_INDEX: usize = 18;
    /// Offset of `name_code_units` (`u32`, little-endian). Spec §11.
    pub(crate) const NAME_CODE_UNITS: usize = 22;
}

/// Byte offsets for the `pm_dc_expression_header` record.
///
/// Spec §11. Record length 10 B.
pub(crate) mod pm_dc_expression_header {
    /// Record length in bytes. Spec §11.
    pub(crate) const LEN: usize = 10;
    /// Offset of `header_value` (`u32`, little-endian). Spec §11.
    pub(crate) const HEADER_VALUE: usize = 0;
    /// Offset of `header_id` (`u16`, little-endian). Spec §11.
    pub(crate) const HEADER_ID: usize = 4;
    /// Offset of `unit_reference` (`u32`, little-endian). Spec §11.
    pub(crate) const UNIT_REFERENCE: usize = 6;
}

/// Byte offsets for the `pm_dc_unit_array_prefix` record.
///
/// Spec §11. Record length 8 B.
pub(crate) mod pm_dc_unit_array_prefix {
    /// Record length in bytes. Spec §11.
    pub(crate) const LEN: usize = 8;
    /// Offset of `marker` (`u16[2]`, little-endian). Spec §11.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `item_count` (`u32`, little-endian). Spec §11.
    pub(crate) const ITEM_COUNT: usize = 4;
}

/// Byte offsets for the `pm_dc_base_unit` record.
///
/// Spec §11. Record length 22 B.
pub(crate) mod pm_dc_base_unit {
    /// Record length in bytes. Spec §11.
    pub(crate) const LEN: usize = 22;
    /// Offset of `header_value` (`u32`, little-endian). Spec §11.
    pub(crate) const HEADER_VALUE: usize = 0;
    /// Offset of `header_id` (`u16`, little-endian). Spec §11.
    pub(crate) const HEADER_ID: usize = 4;
    /// Offset of `magnitude` (`f64`, little-endian). Spec §11.
    pub(crate) const MAGNITUDE: usize = 6;
    /// Offset of `factor` (`f64`, little-endian). Spec §11.
    pub(crate) const FACTOR: usize = 14;
}

/// Byte offsets for the `rse_database_prefix` record.
///
/// Spec §2. Record length 56 B.
pub(crate) mod rse_database_prefix {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 56;
    /// Offset of `database_id` (`bytes[16]`). Spec §2.
    pub(crate) const DATABASE_ID: usize = 0;
    /// Offset of `schema` (`u32`, little-endian). Spec §2.
    pub(crate) const SCHEMA: usize = 16;
    /// Offset of `created_by` (`bytes[8]`). Spec §2.
    pub(crate) const CREATED_BY: usize = 20;
    /// Offset of `created_filetime` (`u64`, little-endian). Spec §2.
    pub(crate) const CREATED_FILETIME: usize = 28;
    /// Offset of `saved_by` (`bytes[8]`). Spec §2.
    pub(crate) const SAVED_BY: usize = 36;
    /// Offset of `saved_filetime` (`u64`, little-endian). Spec §2.
    pub(crate) const SAVED_FILETIME: usize = 44;
    /// Offset of `note_code_units` (`u32`, little-endian). Spec §2.
    pub(crate) const NOTE_CODE_UNITS: usize = 52;
}

/// Byte offsets for the `bulk_envelope` record.
///
/// Spec §4. Record length 18 B.
pub(crate) mod bulk_envelope {
    /// Record length in bytes. Spec §4.
    pub(crate) const LEN: usize = 18;
    /// Offset of `prefix` (`bytes[16]`). Spec §4.
    pub(crate) const PREFIX: usize = 0;
    /// Offset of `form` (`u16`, little-endian). Spec §4.
    pub(crate) const FORM: usize = 16;
}

/// Byte offsets for the `meta_body_prefix` record.
///
/// Spec §3. Record length 14 B.
pub(crate) mod meta_body_prefix {
    /// Record length in bytes. Spec §3.
    pub(crate) const LEN: usize = 14;
    /// Offset of `values` (`u16[7]`, little-endian). Spec §3.
    pub(crate) const VALUES: usize = 0;
}

/// Byte offsets for the `meta_type_descriptor` record.
///
/// Spec §3. Record length 28 B.
pub(crate) mod meta_type_descriptor {
    /// Record length in bytes. Spec §3.
    pub(crate) const LEN: usize = 28;
    /// Offset of `type_id` (`bytes[16]`). Spec §3.
    pub(crate) const TYPE_ID: usize = 0;
    /// Offset of `field_0_kind` (`u16`, little-endian). Spec §3.
    pub(crate) const FIELD_0_KIND: usize = 16;
    /// Offset of `field_0_value` (`u32`, little-endian). Spec §3.
    pub(crate) const FIELD_0_VALUE: usize = 18;
    /// Offset of `field_1_kind` (`u16`, little-endian). Spec §3.
    pub(crate) const FIELD_1_KIND: usize = 22;
    /// Offset of `field_1_value` (`u32`, little-endian). Spec §3.
    pub(crate) const FIELD_1_VALUE: usize = 24;
}

/// Byte offsets for the `kernel_carrier_header` record.
///
/// Spec §5. Record length 14 B.
pub(crate) mod kernel_carrier_header {
    /// Record length in bytes. Spec §5.
    pub(crate) const LEN: usize = 14;
    /// Offset of `header_state` (`u32`, little-endian). Spec §5.
    pub(crate) const HEADER_STATE: usize = 0;
    /// Offset of `header_kind` (`u16`, little-endian). Spec §5.
    pub(crate) const HEADER_KIND: usize = 4;
    /// Offset of `header_value` (`u32`, little-endian). Spec §5.
    pub(crate) const HEADER_VALUE: usize = 6;
    /// Offset of `schema` (`u32`, little-endian). Spec §5.
    pub(crate) const SCHEMA: usize = 10;
}

/// Byte offsets for the `protein_header` record.
///
/// Spec §7. Record length 4 B.
///
/// ```text
/// Inventor compound-stream envelope around a Protein ZIP. The Protein page format is tabulated in `docs/layouts/protein.toml`.
/// ```
pub(crate) mod protein_header {
    /// Record length in bytes. Spec §7.
    pub(crate) const LEN: usize = 4;
    /// Offset of `payload_len` (`u32`, little-endian). Spec §7.
    pub(crate) const PAYLOAD_LEN: usize = 0;
}

/// Byte offsets for the `ufrx_header` record.
///
/// Spec §8. Record length 4 B.
pub(crate) mod ufrx_header {
    /// Record length in bytes. Spec §8.
    pub(crate) const LEN: usize = 4;
    /// Offset of `schema` (`u16`, little-endian). Spec §8.
    pub(crate) const SCHEMA: usize = 0;
    /// Offset of `section_version_count` (`u16`, little-endian). Spec §8.
    pub(crate) const SECTION_VERSION_COUNT: usize = 2;
}

/// Byte offsets for the `pm_app_default_style_current` record.
///
/// Spec §10. Record length 55 B.
pub(crate) mod pm_app_default_style_current {
    /// Record length in bytes. Spec §10.
    pub(crate) const LEN: usize = 55;
    /// Offset of `header_value` (`u32`, little-endian). Spec §10.
    pub(crate) const HEADER_VALUE: usize = 0;
    /// Offset of `header_id` (`u16`, little-endian). Spec §10.
    pub(crate) const HEADER_ID: usize = 4;
    /// Offset of `material_reference` (`u32`, little-endian). Spec §10.
    pub(crate) const MATERIAL_REFERENCE: usize = 6;
    /// Offset of `rendering_style_reference` (`u32`, little-endian). Spec §10.
    pub(crate) const RENDERING_STYLE_REFERENCE: usize = 10;
    /// Offset of `related_references` (`u32[7]`, little-endian). Spec §10.
    pub(crate) const RELATED_REFERENCES: usize = 14;
    /// Offset of `state` (`u8`). Spec §10.
    pub(crate) const STATE: usize = 42;
    /// Offset of `terminal_reference` (`u32`, little-endian). Spec §10.
    pub(crate) const TERMINAL_REFERENCE: usize = 43;
    /// Offset of `padding` (`bytes[8]`). Spec §10.
    pub(crate) const PADDING: usize = 47;
}

/// Byte offsets for the `pm_app_rendering_style_current_prefix` record.
///
/// Spec §10. Record length 27 B.
pub(crate) mod pm_app_rendering_style_current_prefix {
    /// Record length in bytes. Spec §10.
    pub(crate) const LEN: usize = 27;
    /// Offset of `header_value` (`u32`, little-endian). Spec §10.
    pub(crate) const HEADER_VALUE: usize = 0;
    /// Offset of `header_id` (`u16`, little-endian). Spec §10.
    pub(crate) const HEADER_ID: usize = 4;
    /// Offset of `state` (`u8`). Spec §10.
    pub(crate) const STATE: usize = 6;
    /// Offset of `flags` (`u16`, little-endian). Spec §10.
    pub(crate) const FLAGS: usize = 7;
    /// Offset of `padding` (`u16`, little-endian). Spec §10.
    pub(crate) const PADDING: usize = 9;
    /// Offset of `values` (`u16[2]`, little-endian). Spec §10.
    pub(crate) const VALUES: usize = 11;
    /// Offset of `default_state` (`u32`, little-endian). Spec §10.
    pub(crate) const DEFAULT_STATE: usize = 15;
    /// Offset of `value` (`u32`, little-endian). Spec §10.
    pub(crate) const VALUE: usize = 19;
    /// Offset of `name_reference` (`u32`, little-endian). Spec §10.
    pub(crate) const NAME_REFERENCE: usize = 23;
}

/// Byte offsets for the `pm_graphics_face_current_prefix` record.
///
/// Spec §10. Record length 26 B.
///
/// ```text
/// The variable edge-reference list starts at byte 26. Its exact end selects the fixed visibility, bounds, key, and values tail.
/// ```
pub(crate) mod pm_graphics_face_current_prefix {
    /// Record length in bytes. Spec §10.
    pub(crate) const LEN: usize = 26;
    /// Offset of `header_value` (`u32`, little-endian). Spec §10.
    pub(crate) const HEADER_VALUE: usize = 0;
    /// Offset of `header_id` (`u16`, little-endian). Spec §10.
    pub(crate) const HEADER_ID: usize = 4;
    /// Offset of `flags` (`u32`, little-endian). Spec §10.
    pub(crate) const FLAGS: usize = 6;
    /// Offset of `styles_reference` (`u32`, little-endian). Spec §10.
    pub(crate) const STYLES_REFERENCE: usize = 10;
    /// Offset of `surface_reference` (`u32`, little-endian). Spec §10.
    pub(crate) const SURFACE_REFERENCE: usize = 14;
    /// Offset of `parent_reference` (`u32`, little-endian). Spec §10.
    pub(crate) const PARENT_REFERENCE: usize = 18;
    /// Offset of `state` (`u32`, little-endian). Spec §10.
    pub(crate) const STATE: usize = 22;
}

/// Byte offsets for the `pm_graphics_list_prefix` record.
///
/// Spec §10. Record length 8 B.
///
/// ```text
/// A nonempty list continues with two u32 metadata values and its counted items.
/// ```
pub(crate) mod pm_graphics_list_prefix {
    /// Record length in bytes. Spec §10.
    pub(crate) const LEN: usize = 8;
    /// Offset of `marker` (`u16[2]`, little-endian). Spec §10.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `item_count` (`u32`, little-endian). Spec §10.
    pub(crate) const ITEM_COUNT: usize = 4;
}

/// Byte offsets for the `pm_graphics_primary_color_style_current` record.
///
/// Spec §10. Record length 94 B.
pub(crate) mod pm_graphics_primary_color_style_current {
    /// Record length in bytes. Spec §10.
    pub(crate) const LEN: usize = 94;
    /// Offset of `header_value` (`u32`, little-endian). Spec §10.
    pub(crate) const HEADER_VALUE: usize = 0;
    /// Offset of `controls` (`u16[7]`, little-endian). Spec §10.
    pub(crate) const CONTROLS: usize = 4;
    /// Offset of `color_header` (`u8[2]`). Spec §10.
    pub(crate) const COLOR_HEADER: usize = 18;
    /// Offset of `colors` (`f32[16]`, little-endian). Spec §10.
    pub(crate) const COLORS: usize = 20;
    /// Offset of `color_tail` (`u16[2]`, little-endian). Spec §10.
    pub(crate) const COLOR_TAIL: usize = 84;
    /// Offset of `state` (`u8`). Spec §10.
    pub(crate) const STATE: usize = 88;
    /// Offset of `values` (`u16[2]`, little-endian). Spec §10.
    pub(crate) const VALUES: usize = 89;
    /// Offset of `terminal_state` (`u8`). Spec §10.
    pub(crate) const TERMINAL_STATE: usize = 93;
}

/// Byte offsets for the `ufrx_occurrence_prefix` record.
///
/// Spec §8. Record length 20 B.
pub(crate) mod ufrx_occurrence_prefix {
    /// Record length in bytes. Spec §8.
    pub(crate) const LEN: usize = 20;
    /// Offset of `end_string_flag` (`u32`, little-endian). Spec §8.
    pub(crate) const END_STRING_FLAG: usize = 0;
    /// Offset of `file_reference_id` (`u32`, little-endian). Spec §8.
    pub(crate) const FILE_REFERENCE_ID: usize = 4;
    /// Offset of `occurrence_id` (`u32`, little-endian). Spec §8.
    pub(crate) const OCCURRENCE_ID: usize = 8;
    /// Offset of `header_value` (`u32`, little-endian). Spec §8.
    pub(crate) const HEADER_VALUE: usize = 12;
    /// Offset of `title_form_or_count` (`u32`, little-endian). Spec §8.
    pub(crate) const TITLE_FORM_OR_COUNT: usize = 16;
}

/// Byte offsets for the `assembly_occurrence_prefix` record.
///
/// Spec §9. Record length 50 B.
pub(crate) mod assembly_occurrence_prefix {
    /// Record length in bytes. Spec §9.
    pub(crate) const LEN: usize = 50;
    /// Offset of `header_value` (`u32`, little-endian). Spec §9.
    pub(crate) const HEADER_VALUE: usize = 0;
    /// Offset of `header_id` (`u16`, little-endian). Spec §9.
    pub(crate) const HEADER_ID: usize = 4;
    /// Offset of `next_reference` (`u32`, little-endian). Spec §9.
    pub(crate) const NEXT_REFERENCE: usize = 6;
    /// Offset of `flags` (`u32`, little-endian). Spec §9.
    pub(crate) const FLAGS: usize = 10;
    /// Offset of `owner_reference` (`u32`, little-endian). Spec §9.
    pub(crate) const OWNER_REFERENCE: usize = 14;
    /// Offset of `node_index` (`u32`, little-endian). Spec §9.
    pub(crate) const NODE_INDEX: usize = 18;
    /// Offset of `state` (`i32[2]`, little-endian). Spec §9.
    pub(crate) const STATE: usize = 22;
    /// Offset of `relation_marker` (`u32`, little-endian). Spec §9.
    pub(crate) const RELATION_MARKER: usize = 30;
    /// Offset of `relation_count` (`u32`, little-endian). Spec §9.
    pub(crate) const RELATION_COUNT: usize = 34;
    /// Offset of `ordinal_key` (`u32`, little-endian). Spec §9.
    pub(crate) const ORDINAL_KEY: usize = 38;
    /// Offset of `related_marker` (`u32`, little-endian). Spec §9.
    pub(crate) const RELATED_MARKER: usize = 42;
    /// Offset of `related_count` (`u32`, little-endian). Spec §9.
    pub(crate) const RELATED_COUNT: usize = 46;
}
