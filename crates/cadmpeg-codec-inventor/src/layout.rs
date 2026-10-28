// SPDX-License-Identifier: Apache-2.0
//! Byte-offset and value constants generated from `docs/layouts/inventor.toml`.
//!
//! Do not edit by hand. Regenerate with:
//! `UPDATE_LAYOUT_CODE=1 cargo test -p cadmpeg --test layout_tables`.

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
