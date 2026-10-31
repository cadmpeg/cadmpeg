// SPDX-License-Identifier: Apache-2.0
//! Byte-offset and value constants generated from `docs/layouts/inventor.toml`.
//!
//! Do not edit by hand. Regenerate with:
//! `UPDATE_LAYOUT_CODE=1 cargo test -p cadmpeg --test layout_tables`.

/// Byte offsets for the `pm_dc_derived_source_field` record.
///
/// Spec §13.4. Record length 4 B.
///
/// ```text
/// Offset starts after the segment-major-19 content header.
/// ```
pub(crate) mod pm_dc_derived_source_field {
    /// Record length in bytes. Spec §13.4.
    pub(crate) const LEN: usize = 4;
}

/// Byte offsets for the `pm_dc_derived_component_fields` record.
///
/// Spec §13.4. Record length 32 B.
///
/// ```text
/// Offsets start after the segment-major-19 state content header.
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

/// Byte offsets for the `pm_dc_content_header` record.
///
/// Spec §12. Record length 22 B.
pub(crate) mod pm_dc_content_header {
    /// Record length in bytes. Spec §12.
    pub(crate) const LEN: usize = 22;
    /// Offset of `flags` (`u32`, little-endian). Spec §12.
    #[cfg(test)]
    pub(crate) const FLAGS: usize = 10;
}

/// Byte offsets for the `pm_dc_content_header_v28` record.
///
/// Spec §12. Record length 26 B.
pub(crate) mod pm_dc_content_header_v28 {
    /// Record length in bytes. Spec §12.
    pub(crate) const LEN: usize = 26;
}

/// Byte offsets for the `bulk_envelope` record.
///
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
///
/// Spec §3. Record length 14 B.
pub(crate) mod meta_body_prefix {
    /// Record length in bytes. Spec §3.
    pub(crate) const LEN: usize = 14;
}

/// Byte offsets for the `meta_type_descriptor` record.
///
/// Spec §3. Record length 28 B.
pub(crate) mod meta_type_descriptor {
    /// Record length in bytes. Spec §3.
    pub(crate) const LEN: usize = 28;
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
}

/// Byte offsets for the `meta_segment_pair` record.
///
/// Spec §13.4. Record length 32 B.
pub(crate) mod meta_segment_pair {
    /// Record length in bytes. Spec §13.4.
    pub(crate) const LEN: usize = 32;
    /// Offset of `segment_id` (`bytes[16]`). Spec §13.4.
    #[cfg(test)]
    pub(crate) const SEGMENT_ID: usize = 0;
}

/// Byte offsets for the `meta_external_revision` record.
///
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
///
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
///
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

/// Byte offsets for the `pm_dc_sheet_metal_style_operand_prefix` record.
///
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
///
/// Spec §13.5. Record length 86 B.
///
/// ```text
/// Prefix of the segment-major-20 and segment-major-23 sheet-metal style record; the record continues after this prefix.
/// ```
pub(crate) mod pm_dc_sheet_metal_style_operand_prefix_legacy {
    /// Offset of `thickness` (`u32`, little-endian). Spec §13.5.
    pub(crate) const THICKNESS: usize = 26;
}

/// Byte offsets for the `pm_dc_feature_dimension_tail` record.
///
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
///
/// Spec §3. Record length 3 B.
pub(crate) mod meta_reference_header {
    /// Record length in bytes. Spec §3.
    pub(crate) const LEN: usize = 3;
}

/// Byte offsets for the `drawing_sheet_prefix` record.
///
/// Spec §15. Record length 30 B.
pub(crate) mod drawing_sheet_prefix {
    /// Offset of `identifier` (`bytes[16]`). Spec §15.
    pub(crate) const IDENTIFIER: usize = 14;
}

/// Byte offsets for the `drawing_view_prefix` record.
///
/// Spec §15. Record length 30 B.
pub(crate) mod drawing_view_prefix {
    /// Record length in bytes. Spec §15.
    pub(crate) const LEN: usize = 30;
}

/// Byte offsets for the `drawing_sheet_controls` record.
///
/// Spec §15. Record length 24 B.
pub(crate) mod drawing_sheet_controls {
    /// Record length in bytes. Spec §15.
    pub(crate) const LEN: usize = 24;
}

/// Byte offsets for the `drawing_model_reference_prefix` record.
///
/// Spec §15. Record length 49 B.
pub(crate) mod drawing_model_reference_prefix {
    /// Record length in bytes. Spec §15.
    pub(crate) const LEN: usize = 49;
}

/// Byte offsets for the `drawing_cached_view_prefix` record.
///
/// Spec §15. Record length 32 B.
pub(crate) mod drawing_cached_view_prefix {
    /// Offset of `view_reference` (`u32`, little-endian). Spec §15.
    pub(crate) const VIEW_REFERENCE: usize = 28;
}

/// Byte offsets for the `drawing_sheet_property_list` record.
///
/// Spec §15. Record length 16 B.
pub(crate) mod drawing_sheet_property_list {
    /// Stated value of `marker` (`u16`). Spec §15.
    pub(crate) const MARKER_VALUE: u16 = 0x0002;
    /// Stated value of `count` (`u32`). Spec §15.
    pub(crate) const COUNT_VALUE: u32 = 0x0000_0006;
}

/// Byte offsets for the `drawing_view_operand_list` record.
///
/// Spec §15. Record length 16 B.
pub(crate) mod drawing_view_operand_list {
    /// Stated value of `count` (`u32`). Spec §15.
    pub(crate) const COUNT_VALUE: u32 = 0x0000_0013;
}

/// Byte offsets for the `meta_reference_identifier` record.
///
/// Spec §3. Record length 16 B.
pub(crate) mod meta_reference_identifier {
    /// Record length in bytes. Spec §3.
    pub(crate) const LEN: usize = 16;
}

/// Byte offsets for the `meta_reference_pair` record.
///
/// Spec §3. Record length 8 B.
pub(crate) mod meta_reference_pair {
    /// Record length in bytes. Spec §3.
    pub(crate) const LEN: usize = 8;
}

/// Byte offsets for the `drawing_sheet_tail_prefix` record.
///
/// Spec §15. Record length 14 B.
pub(crate) mod drawing_sheet_tail_prefix {
    /// Record length in bytes. Spec §15.
    pub(crate) const LEN: usize = 14;
}

/// Byte offsets for the `drawing_sheet_terminal` record.
///
/// Spec §15. Record length 8 B.
pub(crate) mod drawing_sheet_terminal {
    /// Record length in bytes. Spec §15.
    pub(crate) const LEN: usize = 8;
}

/// Byte offsets for the `drawing_main_view_fixed` record.
///
/// Spec §15. Record length 136 B.
pub(crate) mod drawing_main_view_fixed {
    /// Record length in bytes. Spec §15.
    pub(crate) const LEN: usize = 136;
}

/// Byte offsets for the `drawing_alternate_view_fixed` record.
///
/// Spec §15. Record length 140 B.
pub(crate) mod drawing_alternate_view_fixed {
    /// Record length in bytes. Spec §15.
    pub(crate) const LEN: usize = 140;
}

/// Byte offsets for the `pm_dc_mirror_selection_tail` record.
///
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
///
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

/// Byte offsets for the `pm_dc_pattern_selection_control_body` record.
///
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
///
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
///
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
///
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
///
/// Spec §13.6. Record length 8 B.
pub(crate) mod pm_dc_pattern_suppression_pair {
    /// Record length in bytes. Spec §13.6.
    pub(crate) const LEN: usize = 8;
}

/// Byte offsets for the `pm_dc_work_point_fields` record.
///
/// Spec §13. Record length 36 B.
///
/// ```text
/// Offsets start after the segment-major-19 content header.
/// ```
pub(crate) mod pm_dc_work_point_fields {
    /// Record length in bytes. Spec §13.
    pub(crate) const LEN: usize = 36;
}
