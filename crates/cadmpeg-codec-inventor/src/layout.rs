// SPDX-License-Identifier: Apache-2.0
//! Byte-offset and value constants generated from `docs/layouts/inventor.toml`.
//!
//! Do not edit by hand. Regenerate with:
//! `UPDATE_LAYOUT_CODE=1 cargo test -p cadmpeg --test layout_tables`.

/// Byte offsets for the `pm_dc_751ade_label_owner_body_v25` record.
/// Spec §13. Record length 36 B.
///
/// ```text
/// Offsets start after the versioned state content header in segment major version 25.
/// ```
pub(crate) mod pm_dc_751ade_label_owner_body_v25 {
    /// Record length in bytes. Spec §13.
    pub(crate) const LEN: usize = 36;
    /// Offset of `state` (`i32`, little-endian). Spec §13.
    pub(crate) const STATE: usize = 0;
    /// Stated value of `state` (`i32`). Spec §13.
    pub(crate) const STATE_VALUE: i32 = -1;
    /// Offset of `first_reference` (`u32`, little-endian). Spec §13.
    pub(crate) const FIRST_REFERENCE: usize = 4;
    /// Offset of `work_plane` (`u32`, little-endian). Spec §13.
    pub(crate) const WORK_PLANE: usize = 8;
    /// Offset of `references` (`u32[5]`, little-endian). Spec §13.
    pub(crate) const REFERENCES: usize = 16;
}

/// Byte offsets for the `pm_dc_751ade_label_owner_body_v28` record.
/// Spec §13. Record length 44 B.
///
/// ```text
/// Offsets start after the versioned state content header in segment major version 28.
/// ```
pub(crate) mod pm_dc_751ade_label_owner_body_v28 {
    /// Record length in bytes. Spec §13.
    pub(crate) const LEN: usize = 44;
    /// Offset of `state` (`i32`, little-endian). Spec §13.
    pub(crate) const STATE: usize = 0;
    /// Offset of `first_reference` (`u32`, little-endian). Spec §13.
    pub(crate) const FIRST_REFERENCE: usize = 4;
    /// Offset of `work_plane` (`u32`, little-endian). Spec §13.
    pub(crate) const WORK_PLANE: usize = 8;
    /// Offset of `empty_list_marker` (`u32`, little-endian). Spec §13.
    pub(crate) const EMPTY_LIST_MARKER: usize = 16;
    /// Stated value of `empty_list_marker` (`u32`). Spec §13.
    pub(crate) const EMPTY_LIST_MARKER_VALUE: u32 = 0x3000_0002;
    /// Offset of `empty_list_count` (`u32`, little-endian). Spec §13.
    pub(crate) const EMPTY_LIST_COUNT: usize = 20;
    /// Stated value of `empty_list_count` (`u32`). Spec §13.
    pub(crate) const EMPTY_LIST_COUNT_VALUE: u32 = 0x0000_0000;
    /// Offset of `references` (`u32[5]`, little-endian). Spec §13.
    pub(crate) const REFERENCES: usize = 24;
}

/// Byte offsets for the `pm_dc_derived_source_field` record.
/// Spec §13.4. Record length 4 B.
///
/// ```text
/// Offset starts after the versioned content header.
/// ```
pub(crate) mod pm_dc_derived_source_field {
    /// Record length in bytes. Spec §13.4.
    pub(crate) const LEN: usize = 4;
}

/// Byte offsets for the `pm_dc_derived_component_fields` record.
/// Spec §13.4. Record length 32 B.
///
/// ```text
/// Offsets start after the state content header in segment major versions 19, 23, 25, and 31.
/// ```
pub(crate) mod pm_dc_derived_component_fields {
    /// Record length in bytes. Spec §13.4.
    pub(crate) const LEN: usize = 32;
    /// Offset of `state` (`i32`, little-endian). Spec §13.4.
    #[cfg(test)]
    pub(crate) const STATE: usize = 0;
    /// Stated value of `state` (`i32`). Spec §13.4.
    pub(crate) const STATE_VALUE: i32 = -1;
    /// Offset of `references` (`u32[4]`, little-endian). Spec §13.4.
    #[cfg(test)]
    pub(crate) const REFERENCES: usize = 4;
    /// Offset of `marker` (`u32`, little-endian). Spec §13.4.
    #[cfg(test)]
    pub(crate) const MARKER: usize = 20;
    /// Stated value of `marker` (`u32`). Spec §13.4.
    pub(crate) const MARKER_VALUE: u32 = 0x3000_0004;
    /// Offset of `count` (`u32`, little-endian). Spec §13.4.
    #[cfg(test)]
    pub(crate) const COUNT: usize = 24;
    /// Stated value of `count` (`u32`). Spec §13.4.
    pub(crate) const COUNT_VALUE: u32 = 0x0000_0000;
}

/// Byte offsets for the `pm_dc_derived_source_stamp` record.
/// Spec §13.4. Record length 4 B.
///
/// ```text
/// Offset of each stamp starts after the content header, 22 bytes before segment major version 28 and 26 bytes from it.
/// ```
pub(crate) mod pm_dc_derived_source_stamp {
    /// Record length in bytes. Spec §13.4.
    pub(crate) const LEN: usize = 4;
}

/// Byte offsets for the `pm_dc_derived_component_suffix` record.
/// Spec §13.4. Record length 4 B.
///
/// ```text
/// Offsets start after the component terminal reference in segment major versions 23, 25, and 31.
/// ```
pub(crate) mod pm_dc_derived_component_suffix {
    /// Record length in bytes. Spec §13.4.
    pub(crate) const LEN: usize = 4;
    /// Stated value of `state` (`u32`). Spec §13.4.
    pub(crate) const STATE_VALUE: u32 = 0x0000_0000;
}

/// Byte offsets for the `pm_dc_content_header` record.
/// Spec §12. Record length 22 B.
pub(crate) mod pm_dc_content_header {
    /// Record length in bytes. Spec §12.
    pub(crate) const LEN: usize = 22;
    /// Offset of `flags` (`u32`, little-endian). Spec §12.
    #[cfg(test)]
    pub(crate) const FLAGS: usize = 10;
}

/// Byte offsets for the `pm_dc_content_header_v28` record.
/// Spec §12. Record length 26 B.
pub(crate) mod pm_dc_content_header_v28 {
    /// Record length in bytes. Spec §12.
    pub(crate) const LEN: usize = 26;
}

/// Byte offsets for the `bulk_envelope` record.
/// Spec §4. Record length 18 B.
pub(crate) mod bulk_envelope {
    /// Record length in bytes. Spec §4.
    pub(crate) const LEN: usize = 18;
    /// Offset of `prefix` (`bytes[16]`). Spec §4.
    pub(crate) const PREFIX: usize = 0;
    /// Offset of `form` (`u8`). Spec §4.
    pub(crate) const FORM: usize = 16;
}

/// Byte offsets for the `meta_body_prefix` record.
/// Spec §3. Record length 14 B.
pub(crate) mod meta_body_prefix {
    /// Record length in bytes. Spec §3.
    pub(crate) const LEN: usize = 14;
}

/// Byte offsets for the `meta_type_descriptor` record.
/// Spec §3. Record length 28 B.
pub(crate) mod meta_type_descriptor {
    /// Record length in bytes. Spec §3.
    pub(crate) const LEN: usize = 28;
}

/// Byte offsets for the `kernel_carrier_header` record.
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
/// Spec §7. Record length 4 B.
///
/// ```text
/// Inventor compound-stream envelope around a Protein ZIP. The Protein page format is tabulated in `docs/layouts/protein.toml`.
/// ```
pub(crate) mod protein_header {
    /// Record length in bytes. Spec §7.
    pub(crate) const LEN: usize = 4;
}

/// Byte offsets for the `meta_segment_pair` record.
/// Spec §13.4. Record length 32 B.
pub(crate) mod meta_segment_pair {
    /// Record length in bytes. Spec §13.4.
    pub(crate) const LEN: usize = 32;
    /// Offset of `segment_id` (`bytes[16]`). Spec §13.4.
    #[cfg(test)]
    pub(crate) const SEGMENT_ID: usize = 0;
}

/// Byte offsets for the `meta_external_revision` record.
/// Spec §13.4. Record length 20 B.
pub(crate) mod meta_external_revision {
    /// Record length in bytes. Spec §13.4.
    pub(crate) const LEN: usize = 20;
    /// Offset of `segment_pair_index` (`u16`, little-endian). Spec §13.4.
    pub(crate) const SEGMENT_PAIR_INDEX: usize = 16;
    /// Offset of `reference_count` (`u16`, little-endian). Spec §13.4.
    #[cfg(test)]
    pub(crate) const REFERENCE_COUNT: usize = 18;
}

/// Byte offsets for the `meta_external_reference` record.
/// Spec §13.4. Record length 8 B.
pub(crate) mod meta_external_reference {
    /// Record length in bytes. Spec §13.4.
    pub(crate) const LEN: usize = 8;
    /// Offset of `revision_index` (`u32`, little-endian). Spec §13.4.
    #[cfg(test)]
    pub(crate) const REVISION_INDEX: usize = 0;
    /// Offset of `key_extension` (`u16`, little-endian). Spec §13.4.
    #[cfg(test)]
    pub(crate) const KEY_EXTENSION: usize = 4;
}

/// Byte offsets for the `kernel_entity_link` record.
/// Spec §13. Record length 23 B.
pub(crate) mod kernel_entity_link {
    /// Offset of `header_id` (`u16`, little-endian). Spec §13.
    #[cfg(test)]
    pub(crate) const HEADER_ID: usize = 4;
    /// Offset of `selection_source` (`u32`, little-endian). Spec §13.
    #[cfg(test)]
    pub(crate) const SELECTION_SOURCE: usize = 6;
    /// Offset of `body_reference` (`u32`, little-endian). Spec §13.
    #[cfg(test)]
    pub(crate) const BODY_REFERENCE: usize = 11;
    /// Offset of `entity_id` (`u32`, little-endian). Spec §13.
    #[cfg(test)]
    pub(crate) const ENTITY_ID: usize = 15;
}

/// Byte offsets for the `pm_dc_spline_curve_prefix` record.
/// Spec §12. Record length 28 B.
///
/// ```text
/// Offsets are relative to each spline curve, after the spline record's variable lists.
/// ```
pub(crate) mod pm_dc_spline_curve_prefix {
    /// Stated value of `scalar_width` (`u32`). Spec §12.
    pub(crate) const SCALAR_WIDTH_VALUE: u32 = 0x0000_0008;
}

/// Byte offsets for the `pm_dc_sheet_metal_style_operand_prefix` record.
/// Spec §13.5. Record length 90 B.
///
/// ```text
/// Prefix of the segment-major-29 and segment-major-31 sheet-metal style record; the record continues after this prefix.
/// ```
pub(crate) mod pm_dc_sheet_metal_style_operand_prefix {
    /// Offset of `thickness` (`u32`, little-endian). Spec §13.5.
    pub(crate) const THICKNESS: usize = 30;
}

/// Byte offsets for the `pm_dc_sheet_metal_style_operand_prefix_legacy` record.
/// Spec §13.5. Record length 86 B.
///
/// ```text
/// Prefix of the segment-major-20 and segment-major-23 sheet-metal style record; the record continues after this prefix.
/// ```
pub(crate) mod pm_dc_sheet_metal_style_operand_prefix_legacy {
    /// Offset of `thickness` (`u32`, little-endian). Spec §13.5.
    pub(crate) const THICKNESS: usize = 26;
}

/// Byte offsets for the `pm_dc_sheet_metal_style_operand_prefix_v19` record.
/// Spec §13.5. Record length 106 B.
///
/// ```text
/// Prefix of the segment-major-19 sheet-metal style record; the record continues after this prefix.
/// ```
pub(crate) mod pm_dc_sheet_metal_style_operand_prefix_v19 {
    /// Offset of `thickness` (`u32`, little-endian). Spec §13.5.
    pub(crate) const THICKNESS: usize = 26;
}

/// Byte offsets for the `pm_dc_feature_dimension_tail` record.
/// Spec §13. Record length 9 B.
///
/// ```text
/// The tail ends the record after the annotation list data.
/// ```
pub(crate) mod pm_dc_feature_dimension_tail {
    /// Record length in bytes. Spec §13.
    pub(crate) const LEN: usize = 9;
}

/// Byte offsets for the `meta_reference_header` record.
/// Spec §3. Record length 3 B.
pub(crate) mod meta_reference_header {
    /// Record length in bytes. Spec §3.
    pub(crate) const LEN: usize = 3;
}

/// Byte offsets for the `drawing_sheet_prefix` record.
/// Spec §15. Record length 30 B.
pub(crate) mod drawing_sheet_prefix {
    /// Offset of `identifier` (`bytes[16]`). Spec §15.
    pub(crate) const IDENTIFIER: usize = 14;
}

/// Byte offsets for the `drawing_view_prefix` record.
/// Spec §15. Record length 30 B.
pub(crate) mod drawing_view_prefix {
    /// Record length in bytes. Spec §15.
    pub(crate) const LEN: usize = 30;
}

/// Byte offsets for the `drawing_sheet_controls` record.
/// Spec §15. Record length 24 B.
pub(crate) mod drawing_sheet_controls {
    /// Record length in bytes. Spec §15.
    pub(crate) const LEN: usize = 24;
}

/// Byte offsets for the `drawing_model_reference_prefix` record.
/// Spec §15. Record length 49 B.
pub(crate) mod drawing_model_reference_prefix {
    /// Record length in bytes. Spec §15.
    pub(crate) const LEN: usize = 49;
}

/// Byte offsets for the `drawing_cached_view_prefix` record.
/// Spec §15. Record length 32 B.
pub(crate) mod drawing_cached_view_prefix {
    /// Offset of `view_reference` (`u32`, little-endian). Spec §15.
    pub(crate) const VIEW_REFERENCE: usize = 28;
}

/// Byte offsets for the `drawing_sheet_property_list` record.
/// Spec §15. Record length 16 B.
pub(crate) mod drawing_sheet_property_list {
    /// Stated value of `marker` (`u16`). Spec §15.
    pub(crate) const MARKER_VALUE: u16 = 0x0002;
    /// Stated value of `count` (`u32`). Spec §15.
    pub(crate) const COUNT_VALUE: u32 = 0x0000_0006;
}

/// Byte offsets for the `drawing_view_operand_list` record.
/// Spec §15. Record length 16 B.
pub(crate) mod drawing_view_operand_list {
    /// Stated value of `count` (`u32`). Spec §15.
    pub(crate) const COUNT_VALUE: u32 = 0x0000_0013;
}

/// Byte offsets for the `meta_reference_identifier` record.
/// Spec §3. Record length 16 B.
pub(crate) mod meta_reference_identifier {
    /// Record length in bytes. Spec §3.
    pub(crate) const LEN: usize = 16;
}

/// Byte offsets for the `meta_reference_pair` record.
/// Spec §3. Record length 8 B.
pub(crate) mod meta_reference_pair {
    /// Record length in bytes. Spec §3.
    pub(crate) const LEN: usize = 8;
}

/// Byte offsets for the `drawing_sheet_tail_prefix` record.
/// Spec §15. Record length 14 B.
pub(crate) mod drawing_sheet_tail_prefix {
    /// Record length in bytes. Spec §15.
    pub(crate) const LEN: usize = 14;
}

/// Byte offsets for the `drawing_sheet_terminal` record.
/// Spec §15. Record length 8 B.
pub(crate) mod drawing_sheet_terminal {
    /// Record length in bytes. Spec §15.
    pub(crate) const LEN: usize = 8;
}

/// Byte offsets for the `drawing_main_view_fixed` record.
/// Spec §15. Record length 136 B.
pub(crate) mod drawing_main_view_fixed {
    /// Record length in bytes. Spec §15.
    pub(crate) const LEN: usize = 136;
}

/// Byte offsets for the `drawing_alternate_view_fixed` record.
/// Spec §15. Record length 140 B.
pub(crate) mod drawing_alternate_view_fixed {
    /// Record length in bytes. Spec §15.
    pub(crate) const LEN: usize = 140;
}

/// Byte offsets for the `pm_dc_mirror_selection_tail` record.
/// Spec §13. Record length 8 B.
///
/// ```text
/// The zero-option mirror form. Offsets are relative to the end of the versioned content header and stamps.
/// ```
pub(crate) mod pm_dc_mirror_selection_tail {
    /// Record length in bytes. Spec §13.
    pub(crate) const LEN: usize = 8;
    /// Stated value of `control` (`u32`). Spec §13.
    pub(crate) const CONTROL_VALUE: u32 = 0x3000_0004;
    /// Stated value of `value` (`u32`). Spec §13.
    pub(crate) const VALUE_VALUE: u32 = 0x0000_0000;
}

/// Byte offsets for the `pm_dc_mirror_options_tail` record.
/// Spec §13. Record length 9 B.
///
/// ```text
/// The zero-option mirror form. Offsets are relative to the end of the versioned content header and stamps.
/// ```
pub(crate) mod pm_dc_mirror_options_tail {
    /// Record length in bytes. Spec §13.
    pub(crate) const LEN: usize = 9;
    /// Stated value of `options` (`bytes[9]`). Spec §13.
    pub(crate) const OPTIONS_VALUE: [u8; 9] =
        [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
}

/// Byte offsets for the `pm_dc_mirror_instance_map_single_tail_v19` record.
/// Spec §13. Record length 56 B.
///
/// ```text
/// Offsets start after the segment-major-19 content header.
/// ```
pub(crate) mod pm_dc_mirror_instance_map_single_tail_v19 {
    /// Record length in bytes. Spec §13.
    pub(crate) const LEN: usize = 56;
    /// Offset of `state` (`i32`, little-endian). Spec §13.
    #[cfg(test)]
    pub(crate) const STATE: usize = 0;
    /// Stated value of `state` (`i32`). Spec §13.
    pub(crate) const STATE_VALUE: i32 = -1;
    /// Offset of `map_marker` (`u16`, little-endian). Spec §13.
    #[cfg(test)]
    pub(crate) const MAP_MARKER: usize = 4;
    /// Stated value of `map_marker` (`u16`). Spec §13.
    pub(crate) const MAP_MARKER_VALUE: u16 = 0x0006;
    /// Offset of `map_signature` (`u16`, little-endian). Spec §13.
    #[cfg(test)]
    pub(crate) const MAP_SIGNATURE: usize = 6;
    /// Stated value of `map_signature` (`u16`). Spec §13.
    pub(crate) const MAP_SIGNATURE_VALUE: u16 = 0x3000;
    /// Offset of `map_count` (`u32`, little-endian). Spec §13.
    #[cfg(test)]
    pub(crate) const MAP_COUNT: usize = 8;
    /// Stated value of `map_count` (`u32`). Spec §13.
    pub(crate) const MAP_COUNT_VALUE: u32 = 0x0000_0001;
    /// Offset of `map_value` (`u32`, little-endian). Spec §13.
    #[cfg(test)]
    pub(crate) const MAP_VALUE: usize = 12;
    /// Stated value of `map_value` (`u32`). Spec §13.
    pub(crate) const MAP_VALUE_VALUE: u32 = 0x0000_0000;
    /// Offset of `source_tag` (`u32`, little-endian). Spec §13.
    #[cfg(test)]
    pub(crate) const SOURCE_TAG: usize = 16;
    /// Stated value of `source_tag` (`u32`). Spec §13.
    pub(crate) const SOURCE_TAG_VALUE: u32 = 0x3000_0002;
    /// Offset of `source` (`u32`, little-endian). Spec §13.
    #[cfg(test)]
    pub(crate) const SOURCE: usize = 20;
    /// Offset of `result_marker` (`u16`, little-endian). Spec §13.
    #[cfg(test)]
    pub(crate) const RESULT_MARKER: usize = 24;
    /// Stated value of `result_marker` (`u16`). Spec §13.
    pub(crate) const RESULT_MARKER_VALUE: u16 = 0x0002;
    /// Offset of `result_signature` (`u16`, little-endian). Spec §13.
    #[cfg(test)]
    pub(crate) const RESULT_SIGNATURE: usize = 26;
    /// Stated value of `result_signature` (`u16`). Spec §13.
    pub(crate) const RESULT_SIGNATURE_VALUE: u16 = 0x3000;
    /// Offset of `result_count` (`u32`, little-endian). Spec §13.
    #[cfg(test)]
    pub(crate) const RESULT_COUNT: usize = 28;
    /// Stated value of `result_count` (`u32`). Spec §13.
    pub(crate) const RESULT_COUNT_VALUE: u32 = 0x0000_0001;
    /// Offset of `result_metadata0` (`u32`, little-endian). Spec §13.
    #[cfg(test)]
    pub(crate) const RESULT_METADATA0: usize = 32;
    /// Stated value of `result_metadata0` (`u32`). Spec §13.
    pub(crate) const RESULT_METADATA0_VALUE: u32 = 0x0000_0001;
    /// Offset of `result_metadata1` (`u32`, little-endian). Spec §13.
    #[cfg(test)]
    pub(crate) const RESULT_METADATA1: usize = 36;
    /// Stated value of `result_metadata1` (`u32`). Spec §13.
    pub(crate) const RESULT_METADATA1_VALUE: u32 = 0x0000_0000;
    /// Offset of `result` (`u32`, little-endian). Spec §13.
    #[cfg(test)]
    pub(crate) const RESULT: usize = 40;
    /// Offset of `terminal_value` (`u32`, little-endian). Spec §13.
    #[cfg(test)]
    pub(crate) const TERMINAL_VALUE: usize = 44;
    /// Stated value of `terminal_value` (`u32`). Spec §13.
    pub(crate) const TERMINAL_VALUE_VALUE: u32 = 0x0000_0002;
    /// Offset of `owner_count` (`u32`, little-endian). Spec §13.
    #[cfg(test)]
    pub(crate) const OWNER_COUNT: usize = 48;
    /// Stated value of `owner_count` (`u32`). Spec §13.
    pub(crate) const OWNER_COUNT_VALUE: u32 = 0x0000_0001;
    /// Offset of `owner` (`u32`, little-endian). Spec §13.
    #[cfg(test)]
    pub(crate) const OWNER: usize = 52;
}

/// Byte offsets for the `pm_dc_mirror_plane_reference_tail_v19` record.
/// Spec §13. Record length 18 B.
///
/// ```text
/// Offsets start after the segment-major-19 content header.
/// ```
pub(crate) mod pm_dc_mirror_plane_reference_tail_v19 {
    /// Record length in bytes. Spec §13.
    pub(crate) const LEN: usize = 18;
    /// Stated value of `state` (`i32`). Spec §13.
    pub(crate) const STATE_VALUE: i32 = -1;
    /// Stated value of `marker0` (`u8`). Spec §13.
    pub(crate) const MARKER0_VALUE: u8 = 1;
    /// Stated value of `marker1` (`u8`). Spec §13.
    pub(crate) const MARKER1_VALUE: u8 = 5;
}

/// Byte offsets for the `pm_dc_mirror_generated_body_feature_v19` record.
/// Spec §13. Record length 66 B.
pub(crate) mod pm_dc_mirror_generated_body_feature_v19 {
    /// Record length in bytes. Spec §13.
    pub(crate) const LEN: usize = 66;
    /// Stated value of `flags` (`u32`). Spec §13.
    pub(crate) const FLAGS_VALUE: u32 = 0x0000_6200;
    /// Stated value of `marker` (`u16`). Spec §13.
    pub(crate) const MARKER_VALUE: u16 = 0x0002;
    /// Stated value of `metadata0` (`u32`). Spec §13.
    pub(crate) const METADATA0_VALUE: u32 = 0x0000_0004;
    /// Stated value of `metadata1` (`u32`). Spec §13.
    pub(crate) const METADATA1_VALUE: u32 = 0x0000_0000;
    /// Stated value of `value` (`u32`). Spec §13.
    pub(crate) const VALUE_VALUE: u32 = 0x0000_0000;
}

/// Byte offsets for the `pm_dc_pattern_selection_control_body` record.
/// Spec §13.6. Record length 9 B.
///
/// ```text
/// Offsets are relative to the end of the versioned content header.
/// ```
pub(crate) mod pm_dc_pattern_selection_control_body {
    /// Record length in bytes. Spec §13.6.
    pub(crate) const LEN: usize = 9;
}

/// Byte offsets for the `pm_dc_pattern_cached_edge_body` record.
/// Spec §13.6. Record length 103 B.
///
/// ```text
/// Selection form 1 with curve form 23. Offsets are relative to the end of the content header without stamps.
/// ```
pub(crate) mod pm_dc_pattern_cached_edge_body {
    /// Record length in bytes. Spec §13.6.
    pub(crate) const LEN: usize = 103;
    /// Stated value of `selection_form` (`u32`). Spec §13.6.
    pub(crate) const SELECTION_FORM_VALUE: u32 = 0x0000_0001;
    /// Stated value of `zero` (`u16`). Spec §13.6.
    pub(crate) const ZERO_VALUE: u16 = 0x0000;
    /// Stated value of `curve_form` (`u32`). Spec §13.6.
    pub(crate) const CURVE_FORM_VALUE: u32 = 0x0000_0017;
    /// Stated value of `line_state` (`u8`). Spec §13.6.
    pub(crate) const LINE_STATE_VALUE: u8 = 1;
}

/// Byte offsets for the `pm_dc_pattern_cached_line_body` record.
/// Spec §13.6. Record length 99 B.
///
/// ```text
/// Selection form 1 with curve form 23. Offsets are relative to the end of the content header without stamps.
/// ```
pub(crate) mod pm_dc_pattern_cached_line_body {
    /// Record length in bytes. Spec §13.6.
    pub(crate) const LEN: usize = 99;
}

/// Byte offsets for the `pm_dc_pattern_suppression_prefix` record.
/// Spec §13.6. Record length 8 B.
///
/// ```text
/// Offsets are relative to the end of the versioned content header.
/// ```
pub(crate) mod pm_dc_pattern_suppression_prefix {
    /// Record length in bytes. Spec §13.6.
    pub(crate) const LEN: usize = 8;
    /// Stated value of `marker` (`u16`). Spec §13.6.
    pub(crate) const MARKER_VALUE: u16 = 0x0004;
    /// Stated value of `marker_kind` (`u16`). Spec §13.6.
    pub(crate) const MARKER_KIND_VALUE: u16 = 0x3000;
}

/// Byte offsets for the `pm_dc_pattern_suppression_pair` record.
/// Spec §13.6. Record length 8 B.
pub(crate) mod pm_dc_pattern_suppression_pair {
    /// Record length in bytes. Spec §13.6.
    pub(crate) const LEN: usize = 8;
}

/// Byte offsets for the `pm_dc_work_point_fields` record.
/// Spec §13. Record length 36 B.
///
/// ```text
/// Offsets start after the segment-major-19 content header.
/// ```
pub(crate) mod pm_dc_work_point_fields {
    /// Record length in bytes. Spec §13.
    pub(crate) const LEN: usize = 36;
}

/// Byte offsets for the `pm_dc_open_cubic_curve_prefix` record.
/// Spec §12. Record length 28 B.
///
/// ```text
/// Offsets are relative to each curve array of the endpoint-bound open cubic form. The scalar lanes and fixed tail share the spline curve layout.
/// ```
pub(crate) mod pm_dc_open_cubic_curve_prefix {
    /// Stated value of `form` (`u32`). Spec §12.
    pub(crate) const FORM_VALUE: u32 = 0x0000_0000;
    /// Stated value of `degree` (`u32`). Spec §12.
    pub(crate) const DEGREE_VALUE: u32 = 0x0000_0003;
}

/// Byte offsets for the `pm_dc_open_cubic_pole_prefix` record.
/// Spec §12. Record length 24 B.
///
/// ```text
/// Offsets are relative to the byte after the counted knot lane of the endpoint-bound open cubic form.
/// ```
pub(crate) mod pm_dc_open_cubic_pole_prefix {
    /// Stated value of `pole_value` (`u32`). Spec §12.
    pub(crate) const POLE_VALUE_VALUE: u32 = 0x0000_0008;
}

/// Byte offsets for the `pm_dc_box_group_body` record.
/// Spec §12. Record length 32 B.
///
/// ```text
/// Offsets start after the segment-major-19 content header.
/// ```
pub(crate) mod pm_dc_box_group_body {
    /// Record length in bytes. Spec §12.
    pub(crate) const LEN: usize = 32;
    /// Offset of `value` (`u32`, little-endian). Spec §12.
    pub(crate) const VALUE: usize = 0;
    /// Offset of `references` (`u32[7]`, little-endian). Spec §12.
    pub(crate) const REFERENCES: usize = 4;
}

/// Byte offsets for the `pm_dc_box_frame_fixed` record.
/// Spec §12. Record length 44 B.
///
/// ```text
/// Offsets start after the segment-major-19 content header. The explicit placement cells follow this part.
/// ```
pub(crate) mod pm_dc_box_frame_fixed {
    /// Record length in bytes. Spec §12.
    pub(crate) const LEN: usize = 44;
    /// Offset of `sketch_reference` (`u32`, little-endian). Spec §12.
    pub(crate) const SKETCH_REFERENCE: usize = 0;
    /// Offset of `width` (`f64`, little-endian). Spec §12.
    pub(crate) const WIDTH: usize = 4;
    /// Offset of `height` (`f64`, little-endian). Spec §12.
    pub(crate) const HEIGHT: usize = 12;
    /// Offset of `corner_x` (`f64`, little-endian). Spec §12.
    pub(crate) const CORNER_X: usize = 20;
    /// Offset of `corner_y` (`f64`, little-endian). Spec §12.
    pub(crate) const CORNER_Y: usize = 28;
    /// Offset of `value` (`u32`, little-endian). Spec §12.
    pub(crate) const VALUE: usize = 36;
    /// Offset of `value_mask` (`u16`, little-endian). Spec §12.
    pub(crate) const VALUE_MASK: usize = 40;
    /// Offset of `zero_mask` (`u16`, little-endian). Spec §12.
    pub(crate) const ZERO_MASK: usize = 42;
}

/// Byte offsets for the `pm_dc_box_frame_tail` record.
/// Spec §12. Record length 15 B.
///
/// ```text
/// Offsets start after the explicit placement cells of the box frame.
/// ```
pub(crate) mod pm_dc_box_frame_tail {
    /// Record length in bytes. Spec §12.
    pub(crate) const LEN: usize = 15;
    /// Offset of `tail_value` (`u32`, little-endian). Spec §12.
    pub(crate) const TAIL_VALUE: usize = 0;
    /// Offset of `tail_flags` (`u16`, little-endian). Spec §12.
    pub(crate) const TAIL_FLAGS: usize = 4;
    /// Offset of `tail_scale` (`f64`, little-endian). Spec §12.
    pub(crate) const TAIL_SCALE: usize = 6;
    /// Offset of `tail_state` (`u8`). Spec §12.
    pub(crate) const TAIL_STATE: usize = 14;
}

/// Byte offsets for the `pm_dc_frame_row_v25` record.
/// Spec §13.11. Record length 82 B.
///
/// ```text
/// A feature-control-frame row in segment major version 25.
/// ```
pub(crate) mod pm_dc_frame_row_v25 {
    /// Record length in bytes. Spec §13.11.
    pub(crate) const LEN: usize = 82;
    /// Offset of `characteristic` (`u32`, little-endian). Spec §13.11.
    pub(crate) const CHARACTERISTIC: usize = 6;
    /// Offset of `magnitude` (`u32`, little-endian). Spec §13.11.
    pub(crate) const MAGNITUDE: usize = 10;
    /// Offset of `zone_modifiers` (`u32[3]`, little-endian). Spec §13.11.
    pub(crate) const ZONE_MODIFIERS: usize = 14;
    /// Offset of `second_value` (`u32`, little-endian). Spec §13.11.
    pub(crate) const SECOND_VALUE: usize = 26;
    /// Offset of `third_value` (`u32`, little-endian). Spec §13.11.
    pub(crate) const THIRD_VALUE: usize = 30;
    /// Offset of `datums` (`u32[3]`, little-endian). Spec §13.11.
    pub(crate) const DATUMS: usize = 34;
    /// Offset of `datum_modifiers` (`u8[36]`). Spec §13.11.
    pub(crate) const DATUM_MODIFIERS: usize = 46;
}

/// Byte offsets for the `pm_dc_frame_row_v28` record.
/// Spec §13.11. Record length 102 B.
///
/// ```text
/// A feature-control-frame row in segment major version 28.
/// ```
pub(crate) mod pm_dc_frame_row_v28 {
    /// Record length in bytes. Spec §13.11.
    pub(crate) const LEN: usize = 102;
    /// Offset of `frame_reference` (`u32`, little-endian). Spec §13.11.
    pub(crate) const FRAME_REFERENCE: usize = 98;
}

/// Byte offsets for the `pm_dc_naming_table_header` record.
/// Spec §13.11. Record length 18 B.
///
/// ```text
/// The entries follow the header.
/// ```
pub(crate) mod pm_dc_naming_table_header {
    /// Record length in bytes. Spec §13.11.
    pub(crate) const LEN: usize = 18;
    /// Offset of `count` (`u32`, little-endian). Spec §13.11.
    pub(crate) const COUNT: usize = 14;
}

/// Byte offsets for the `pm_dc_annotation_value_block` record.
/// Spec §13.11. Record length 76 B.
///
/// ```text
/// The block between the fit texts and the parameter references of a value record.
/// ```
pub(crate) mod pm_dc_annotation_value_block {
    /// Record length in bytes. Spec §13.11.
    pub(crate) const LEN: usize = 76;
}

/// Byte offsets for the `pm_dc_hole_note_flags` record.
/// Spec §13.11. Record length 16 B.
///
/// ```text
/// The flag words that follow the four hole parameters of a plain hole note.
/// ```
pub(crate) mod pm_dc_hole_note_flags {
    /// Stated value of `word_a` (`u32`). Spec §13.11.
    pub(crate) const WORD_A_VALUE: u32 = 0x0000_0001;
    /// Stated value of `word_b` (`u32`). Spec §13.11.
    pub(crate) const WORD_B_VALUE: u32 = 0x0000_0000;
    /// Stated value of `word_c` (`u32`). Spec §13.11.
    pub(crate) const WORD_C_VALUE: u32 = 0x0100_0000;
    /// Stated value of `word_d` (`u32`). Spec §13.11.
    pub(crate) const WORD_D_VALUE: u32 = 0x0000_0000;
}

/// Byte offsets for the `pm_dc_generated_name_tail` record.
/// Spec §13.11. Record length 16 B.
///
/// ```text
/// The tail of a generated name record of the generated and sweep families.
/// ```
pub(crate) mod pm_dc_generated_name_tail {
    /// Offset of `lead` (`u32`, little-endian). Spec §13.11.
    pub(crate) const LEAD: usize = 0;
    /// Stated value of `lead` (`u32`). Spec §13.11.
    pub(crate) const LEAD_VALUE: u32 = 0x0000_0000;
    /// Offset of `second` (`u32`, little-endian). Spec §13.11.
    pub(crate) const SECOND: usize = 4;
    /// Stated value of `second` (`u32`). Spec §13.11.
    pub(crate) const SECOND_VALUE: u32 = 0x0000_0000;
    /// Offset of `associative_id` (`u32`, little-endian). Spec §13.11.
    pub(crate) const ASSOCIATIVE_ID: usize = 8;
    /// Offset of `role` (`u32`, little-endian). Spec §13.11.
    pub(crate) const ROLE: usize = 12;
}

/// Byte offsets for the `pm_dc_hole_name_tail` record.
/// Spec §13.11. Record length 10 B.
///
/// ```text
/// The tail of a hole name record.
/// ```
pub(crate) mod pm_dc_hole_name_tail {
    /// Offset of `lead` (`u32`, little-endian). Spec §13.11.
    pub(crate) const LEAD: usize = 0;
    /// Stated value of `lead` (`u32`). Spec §13.11.
    pub(crate) const LEAD_VALUE: u32 = 0x0000_0000;
    /// Offset of `first` (`u32`, little-endian). Spec §13.11.
    pub(crate) const FIRST: usize = 4;
    /// Offset of `role` (`u16`, little-endian). Spec §13.11.
    pub(crate) const ROLE: usize = 8;
}

/// Byte offsets for the `pm_dc_blend_name_tail` record.
/// Spec §13.11. Record length 13 B.
///
/// ```text
/// The tail of a blend name record.
/// ```
pub(crate) mod pm_dc_blend_name_tail {
    /// Offset of `lead` (`u32`, little-endian). Spec §13.11.
    pub(crate) const LEAD: usize = 0;
    /// Stated value of `lead` (`u32`). Spec §13.11.
    pub(crate) const LEAD_VALUE: u32 = 0x0000_0000;
    /// Offset of `first` (`u8`). Spec §13.11.
    pub(crate) const FIRST: usize = 4;
    /// Offset of `second` (`u8`). Spec §13.11.
    pub(crate) const SECOND: usize = 5;
    /// Offset of `third` (`u32`, little-endian). Spec §13.11.
    pub(crate) const THIRD: usize = 9;
}
