// SPDX-License-Identifier: Apache-2.0
//! Byte-offset and value constants generated from `docs/layouts/f3d.toml`.
//!
//! Do not edit by hand. Regenerate with:
//! `UPDATE_LAYOUT_CODE=1 cargo test -p cadmpeg --test layout_tables`.

#![allow(dead_code)] // Not every generated constant is referenced yet.

/// Byte offsets for the `indexed_design_record_header` record.
///
/// Spec §3.1. Record length 11 B.
///
/// ```text
/// The 11-byte size is the spec's own "eleven-byte indexed header". §3.1 states the segment's integers are little-endian ("a nonempty contiguous sequence of little-endian i32 values").
/// ```
pub(crate) mod indexed_design_record_header {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 11;
    /// Offset of `class_tag_length` (`u32`, little-endian). Spec §3.1.
    pub(crate) const CLASS_TAG_LENGTH: usize = 0;
    /// Offset of `class_tag` (`bytes[3]`). Spec §3.1.
    pub(crate) const CLASS_TAG: usize = 4;
    /// Offset of `record_index` (`u32`, little-endian). Spec §3.1.
    pub(crate) const RECORD_INDEX: usize = 7;
}

/// Byte offsets for the `sketch_container_visibility_member_prefix` record.
///
/// Spec §3.1. Record length 37 B.
///
/// ```text
/// Offsets are relative to the typed Geometry member's indexed header.
/// ```
pub(crate) mod sketch_container_visibility_member_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 37;
    /// Offset of `class_tag_length` (`u32`, little-endian). Spec §3.1.
    pub(crate) const CLASS_TAG_LENGTH: usize = 0;
    /// Offset of `class_tag` (`bytes[3]`). Spec §3.1.
    pub(crate) const CLASS_TAG: usize = 4;
    /// Offset of `entity_suffix` (`u64`, little-endian). Spec §3.1.
    pub(crate) const ENTITY_SUFFIX: usize = 7;
    /// Offset of `zero_run` (`bytes[4]`). Spec §3.1.
    pub(crate) const ZERO_RUN: usize = 15;
    /// Offset of `owner_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const OWNER_REFERENCE: usize = 19;
    /// Offset of `stream_ordinal` (`u32`, little-endian). Spec §3.1.
    pub(crate) const STREAM_ORDINAL: usize = 30;
    /// Offset of `reserved_zero` (`u8`). Spec §3.1.
    pub(crate) const RESERVED_ZERO: usize = 34;
    /// Offset of `visible` (`u8`). Spec §3.1.
    pub(crate) const VISIBLE: usize = 35;
    /// Offset of `tail_marker` (`u8`). Spec §3.1.
    pub(crate) const TAIL_MARKER: usize = 36;
}

/// Byte offsets for the `design_decal_scope_prefix` record.
///
/// Spec §3.1. Record length 44 B.
///
/// ```text
/// Offsets are relative to the Decal scope's primary indexed header. The remaining scope payload follows this fixed prefix.
/// ```
pub(crate) mod design_decal_scope_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 44;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_10` (`bytes[10]`). Spec §3.1.
    pub(crate) const ZERO_RUN_10: usize = 11;
    /// Offset of `asset_reference` (`bytes[5]`). Spec §3.1.
    pub(crate) const ASSET_REFERENCE: usize = 21;
    /// Offset of `asset_reference_zero_run` (`bytes[6]`). Spec §3.1.
    pub(crate) const ASSET_REFERENCE_ZERO_RUN: usize = 26;
    /// Offset of `mapping_mode` (`u8`). Spec §3.1.
    pub(crate) const MAPPING_MODE: usize = 32;
    /// Offset of `target_group_reference` (`bytes[5]`). Spec §3.1.
    pub(crate) const TARGET_GROUP_REFERENCE: usize = 33;
    /// Offset of `target_reference_zero_run` (`bytes[6]`). Spec §3.1.
    pub(crate) const TARGET_REFERENCE_ZERO_RUN: usize = 38;
}

/// Byte offsets for the `design_decal_image_asset_record` record.
///
/// Spec §3.1. Record length 30 B.
///
/// ```text
/// Complete primary Decal image-asset record. The image-name record begins at byte 30.
/// ```
pub(crate) mod design_decal_image_asset_record {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 30;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_8` (`bytes[8]`). Spec §3.1.
    pub(crate) const ZERO_RUN_8: usize = 11;
    /// Offset of `design_entity_suffix_reference` (`bytes[5]`). Spec §3.1.
    pub(crate) const DESIGN_ENTITY_SUFFIX_REFERENCE: usize = 19;
    /// Offset of `zero_run_6` (`bytes[6]`). Spec §3.1.
    pub(crate) const ZERO_RUN_6: usize = 24;
}

/// Byte offsets for the `design_decal_image_name_prefix` record.
///
/// Spec §3.1. Record length 25 B.
///
/// ```text
/// Fixed prefix through the LP-UTF16 code-unit count. The variable UTF-16LE basename starts at byte 25.
/// ```
pub(crate) mod design_decal_image_name_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 25;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_10` (`bytes[10]`). Spec §3.1.
    pub(crate) const ZERO_RUN_10: usize = 11;
    /// Offset of `asset_name_code_unit_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const ASSET_NAME_CODE_UNIT_COUNT: usize = 21;
}

/// Byte offsets for the `design_parameter_legacy_287_prefix` record.
///
/// Spec §3.1. Record length 45 B.
///
/// ```text
/// Offsets are relative to the class-287 parameter header through the compact expression length. The variable expression is followed by the exact five-byte trailer 00 00 00 00 00 or 00 00 00 01 00.
/// ```
pub(crate) mod design_parameter_legacy_287_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 45;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_15` (`bytes[15]`). Spec §3.1.
    pub(crate) const ZERO_RUN_15: usize = 11;
    /// Offset of `source_ordinal` (`u32`, little-endian). Spec §3.1.
    pub(crate) const SOURCE_ORDINAL: usize = 26;
    /// Offset of `owner_marker` (`u8`). Spec §3.1.
    pub(crate) const OWNER_MARKER: usize = 30;
    /// Stated value of `owner_marker` (`u8`). Spec §3.1.
    pub(crate) const OWNER_MARKER_VALUE: u8 = 1;
    /// Offset of `owner_record_index` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OWNER_RECORD_INDEX: usize = 31;
    /// Offset of `zero_run_6` (`bytes[6]`). Spec §3.1.
    pub(crate) const ZERO_RUN_6: usize = 35;
    /// Offset of `expression_length` (`u32`, little-endian). Spec §3.1.
    pub(crate) const EXPRESSION_LENGTH: usize = 41;
}

/// Byte offsets for the `design_parameter_legacy_287_tail` record.
///
/// Spec §3.1. Record length 12 B.
///
/// ```text
/// This tail is relative to the end of the variable LP-UTF16 name and evaluated scalar.
/// ```
pub(crate) mod design_parameter_legacy_287_tail {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 12;
    /// Offset of `tail_prefix` (`bytes[2]`). Spec §3.1.
    pub(crate) const TAIL_PREFIX: usize = 0;
    /// Stated value of `tail_prefix` (`bytes[2]`). Spec §3.1.
    pub(crate) const TAIL_PREFIX_VALUE: [u8; 2] = [0x00, 0x01];
    /// Offset of `family_marker` (`u8`). Spec §3.1.
    pub(crate) const FAMILY_MARKER: usize = 2;
    /// Stated value of `family_marker` (`u8`). Spec §3.1.
    pub(crate) const FAMILY_MARKER_VALUE: u8 = 175;
    /// Offset of `zero_run_9` (`bytes[9]`). Spec §3.1.
    pub(crate) const ZERO_RUN_9: usize = 3;
}

/// Byte offsets for the `design_parameter_owner_prefix` record.
///
/// Spec §3.1. Record length 39 B.
///
/// ```text
/// Offsets are relative to the parameter-owner primary header. The selected scalar envelope starts at offset 39.
/// ```
pub(crate) mod design_parameter_owner_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 39;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_8` (`bytes[8]`). Spec §3.1.
    pub(crate) const ZERO_RUN_8: usize = 11;
    /// Offset of `one_marker` (`u8`). Spec §3.1.
    pub(crate) const ONE_MARKER: usize = 19;
    /// Offset of `one_value` (`u32`, little-endian). Spec §3.1.
    pub(crate) const ONE_VALUE: usize = 20;
    /// Offset of `scope_marker` (`u8`). Spec §3.1.
    pub(crate) const SCOPE_MARKER: usize = 24;
    /// Offset of `scope_record_index` (`u32`, little-endian). Spec §3.1.
    pub(crate) const SCOPE_RECORD_INDEX: usize = 25;
    /// Offset of `zero_run_6` (`bytes[6]`). Spec §3.1.
    pub(crate) const ZERO_RUN_6: usize = 29;
    /// Offset of `local_ordinal` (`u32`, little-endian). Spec §3.1.
    pub(crate) const LOCAL_ORDINAL: usize = 35;
}

/// Byte offsets for the `design_parameter_owner_legacy_68` record.
///
/// Spec §3.1. Record length 68 B.
///
/// ```text
/// Offsets are relative to the legacy parameter-owner primary header. The scope and scalar lanes are absent.
/// ```
pub(crate) mod design_parameter_owner_legacy_68 {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 68;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_8` (`bytes[8]`). Spec §3.1.
    pub(crate) const ZERO_RUN_8: usize = 11;
    /// Offset of `first_marker` (`u8`). Spec §3.1.
    pub(crate) const FIRST_MARKER: usize = 19;
    /// Offset of `zero_run_13` (`bytes[13]`). Spec §3.1.
    pub(crate) const ZERO_RUN_13: usize = 20;
    /// Offset of `parameter_marker` (`u8`). Spec §3.1.
    pub(crate) const PARAMETER_MARKER: usize = 33;
    /// Offset of `parameter_record_index` (`u32`, little-endian). Spec §3.1.
    pub(crate) const PARAMETER_RECORD_INDEX: usize = 34;
    /// Offset of `zero_run_6` (`bytes[6]`). Spec §3.1.
    pub(crate) const ZERO_RUN_6: usize = 38;
    /// Offset of `owned_ordinal` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OWNED_ORDINAL: usize = 44;
    /// Offset of `zero_run_7` (`bytes[7]`). Spec §3.1.
    pub(crate) const ZERO_RUN_7: usize = 48;
    /// Offset of `companion_marker` (`u8`). Spec §3.1.
    pub(crate) const COMPANION_MARKER: usize = 55;
    /// Offset of `companion_record_index` (`u32`, little-endian). Spec §3.1.
    pub(crate) const COMPANION_RECORD_INDEX: usize = 56;
    /// Offset of `zero_run_8_tail` (`bytes[8]`). Spec §3.1.
    pub(crate) const ZERO_RUN_8_TAIL: usize = 60;
}

/// Byte offsets for the `design_parameter_owner_legacy_88` record.
///
/// Spec §3.1. Record length 88 B.
///
/// ```text
/// Offsets are relative to the legacy parameter-owner primary header. The scalar and local-ordinal lanes are absent.
/// ```
pub(crate) mod design_parameter_owner_legacy_88 {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 88;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_8` (`bytes[8]`). Spec §3.1.
    pub(crate) const ZERO_RUN_8: usize = 11;
    /// Offset of `first_marker` (`u8`). Spec §3.1.
    pub(crate) const FIRST_MARKER: usize = 19;
    /// Offset of `zero_run_13` (`bytes[13]`). Spec §3.1.
    pub(crate) const ZERO_RUN_13: usize = 20;
    /// Offset of `parameter_marker` (`u8`). Spec §3.1.
    pub(crate) const PARAMETER_MARKER: usize = 33;
    /// Offset of `parameter_record_index` (`u32`, little-endian). Spec §3.1.
    pub(crate) const PARAMETER_RECORD_INDEX: usize = 34;
    /// Offset of `zero_run_6` (`bytes[6]`). Spec §3.1.
    pub(crate) const ZERO_RUN_6: usize = 38;
    /// Offset of `owned_ordinal` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OWNED_ORDINAL: usize = 44;
    /// Offset of `zero_run_4` (`bytes[4]`). Spec §3.1.
    pub(crate) const ZERO_RUN_4: usize = 48;
    /// Offset of `scope_marker` (`u8`). Spec §3.1.
    pub(crate) const SCOPE_MARKER: usize = 52;
    /// Offset of `scope_record_index` (`u32`, little-endian). Spec §3.1.
    pub(crate) const SCOPE_RECORD_INDEX: usize = 53;
    /// Offset of `zero_run_8_between_scopes` (`bytes[8]`). Spec §3.1.
    pub(crate) const ZERO_RUN_8_BETWEEN_SCOPES: usize = 57;
    /// Offset of `companion_marker` (`u8`). Spec §3.1.
    pub(crate) const COMPANION_MARKER: usize = 65;
    /// Offset of `companion_record_index` (`u32`, little-endian). Spec §3.1.
    pub(crate) const COMPANION_RECORD_INDEX: usize = 66;
    /// Offset of `zero_run_7` (`bytes[7]`). Spec §3.1.
    pub(crate) const ZERO_RUN_7: usize = 70;
    /// Offset of `repeated_scope_marker` (`u8`). Spec §3.1.
    pub(crate) const REPEATED_SCOPE_MARKER: usize = 77;
    /// Offset of `repeated_scope_record_index` (`u32`, little-endian). Spec §3.1.
    pub(crate) const REPEATED_SCOPE_RECORD_INDEX: usize = 78;
    /// Offset of `zero_run_6_tail` (`bytes[6]`). Spec §3.1.
    pub(crate) const ZERO_RUN_6_TAIL: usize = 82;
}

/// Byte offsets for the `thicken_class_347_scope_frame` record.
///
/// Spec §3.1. Record length 291 B.
///
/// ```text
/// Offsets are relative to the primary indexed header. The paired class-258 header begins at offset 291; the class pair and frame length admit this form.
/// ```
pub(crate) mod thicken_class_347_scope_frame {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 291;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_10` (`bytes[10]`). Spec §3.1.
    pub(crate) const ZERO_RUN_10: usize = 11;
    /// Offset of `feature_form` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FEATURE_FORM: usize = 21;
    /// Stated value of `feature_form` (`u32`). Spec §3.1.
    pub(crate) const FEATURE_FORM_VALUE: u32 = 0x0000_0004;
    /// Offset of `group_form` (`u32`, little-endian). Spec §3.1.
    pub(crate) const GROUP_FORM: usize = 25;
    /// Stated value of `group_form` (`u32`). Spec §3.1.
    pub(crate) const GROUP_FORM_VALUE: u32 = 0x0000_0001;
    /// Offset of `group_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const GROUP_REFERENCE: usize = 29;
    /// Offset of `scalar_prefix` (`bytes[2]`). Spec §3.1.
    pub(crate) const SCALAR_PREFIX: usize = 40;
    /// Stated value of `scalar_prefix` (`bytes[2]`). Spec §3.1.
    pub(crate) const SCALAR_PREFIX_VALUE: [u8; 2] = [0x01, 0x01];
    /// Offset of `scalar_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const SCALAR_REFERENCE: usize = 42;
    /// Offset of `auxiliary_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const AUXILIARY_COUNT: usize = 53;
    /// Stated value of `auxiliary_count` (`u32`). Spec §3.1.
    pub(crate) const AUXILIARY_COUNT_VALUE: u32 = 0x0000_0001;
    /// Offset of `auxiliary_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const AUXILIARY_REFERENCE: usize = 57;
    /// Offset of `zero_run_8` (`bytes[8]`). Spec §3.1.
    pub(crate) const ZERO_RUN_8: usize = 68;
    /// Offset of `guid_code_unit_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const GUID_CODE_UNIT_COUNT: usize = 76;
    /// Stated value of `guid_code_unit_count` (`u32`). Spec §3.1.
    pub(crate) const GUID_CODE_UNIT_COUNT_VALUE: u32 = 0x0000_0024;
    /// Offset of `guid` (`bytes[72]`). Spec §3.1.
    pub(crate) const GUID: usize = 80;
    /// Offset of `zero_run_3` (`bytes[3]`). Spec §3.1.
    pub(crate) const ZERO_RUN_3: usize = 152;
    /// Offset of `reference_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const REFERENCE_COUNT: usize = 155;
    /// Stated value of `reference_count` (`u32`). Spec §3.1.
    pub(crate) const REFERENCE_COUNT_VALUE: u32 = 0x0000_0003;
    /// Offset of `group_reference_entry` (`bytes[11]`). Spec §3.1.
    pub(crate) const GROUP_REFERENCE_ENTRY: usize = 159;
    /// Offset of `member_reference_entry` (`bytes[11]`). Spec §3.1.
    pub(crate) const MEMBER_REFERENCE_ENTRY: usize = 170;
    /// Offset of `scalar_reference_entry` (`bytes[11]`). Spec §3.1.
    pub(crate) const SCALAR_REFERENCE_ENTRY: usize = 181;
    /// Offset of `history_state_id` (`u32`, little-endian). Spec §3.1.
    pub(crate) const HISTORY_STATE_ID: usize = 192;
    /// Offset of `kind_code_unit_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const KIND_CODE_UNIT_COUNT: usize = 196;
    /// Stated value of `kind_code_unit_count` (`u32`). Spec §3.1.
    pub(crate) const KIND_CODE_UNIT_COUNT_VALUE: u32 = 0x0000_0007;
    /// Offset of `kind` (`bytes[14]`). Spec §3.1.
    pub(crate) const KIND: usize = 200;
    /// Offset of `feature_ordinal` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FEATURE_ORDINAL: usize = 214;
    /// Offset of `previous_history_state_id` (`u32`, little-endian). Spec §3.1.
    pub(crate) const PREVIOUS_HISTORY_STATE_ID: usize = 245;
}

/// Byte offsets for the `design_mirror_scope_class413_tail` record.
///
/// Spec §3.1. Record length 77 B.
///
/// ```text
/// Offsets are relative to the first byte after the UTF-16LE Mirror kind. The variable class-413 reference-table prefix precedes this fixed tail.
/// ```
pub(crate) mod design_mirror_scope_class413_tail {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 77;
    /// Offset of `feature_ordinal` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FEATURE_ORDINAL: usize = 0;
    /// Offset of `previous_history_state` (`u32`, little-endian). Spec §3.1.
    pub(crate) const PREVIOUS_HISTORY_STATE: usize = 31;
    /// Offset of `scalar_marker` (`u32`, little-endian). Spec §3.1.
    pub(crate) const SCALAR_MARKER: usize = 35;
    /// Stated value of `scalar_marker` (`u32`). Spec §3.1.
    pub(crate) const SCALAR_MARKER_VALUE: u32 = 0x0000_0059;
    /// Offset of `stitch_tolerance` (`f64`, little-endian). Spec §3.1.
    pub(crate) const STITCH_TOLERANCE: usize = 39;
    /// Offset of `repeated_scalar_marker` (`u32`, little-endian). Spec §3.1.
    pub(crate) const REPEATED_SCALAR_MARKER: usize = 47;
    /// Stated value of `repeated_scalar_marker` (`u32`). Spec §3.1.
    pub(crate) const REPEATED_SCALAR_MARKER_VALUE: u32 = 0x0000_0059;
    /// Offset of `first_reference` (`bytes[13]`). Spec §3.1.
    pub(crate) const FIRST_REFERENCE: usize = 51;
    /// Offset of `second_reference` (`bytes[13]`). Spec §3.1.
    pub(crate) const SECOND_REFERENCE: usize = 64;
}

/// Byte offsets for the `design_draft_scope_class318_compact` record.
///
/// Spec §3.1. Record length 336 B.
///
/// ```text
/// Offsets are relative to the primary Draft indexed header. The paired indexed header begins at offset 336.
/// ```
pub(crate) mod design_draft_scope_class318_compact {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 336;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `reference_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const REFERENCE_COUNT: usize = 171;
    /// Offset of `references` (`bytes[66]`). Spec §3.1.
    pub(crate) const REFERENCES: usize = 175;
    /// Offset of `current_history_state` (`u32`, little-endian). Spec §3.1.
    pub(crate) const CURRENT_HISTORY_STATE: usize = 241;
    /// Offset of `kind_code_unit_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const KIND_CODE_UNIT_COUNT: usize = 245;
    /// Offset of `kind` (`bytes[10]`). Spec §3.1.
    pub(crate) const KIND: usize = 249;
    /// Offset of `feature_ordinal` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FEATURE_ORDINAL: usize = 259;
    /// Offset of `previous_history_state` (`u32`, little-endian). Spec §3.1.
    pub(crate) const PREVIOUS_HISTORY_STATE: usize = 290;
}

/// Byte offsets for the `design_draft_scope_class318_shifted` record.
///
/// Spec §3.1. Record length 340 B.
///
/// ```text
/// Offsets are relative to the primary Draft indexed header. The paired indexed header begins at offset 340.
/// ```
pub(crate) mod design_draft_scope_class318_shifted {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 340;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `reserved_zero` (`u32`, little-endian). Spec §3.1.
    pub(crate) const RESERVED_ZERO: usize = 171;
    /// Stated value of `reserved_zero` (`u32`). Spec §3.1.
    pub(crate) const RESERVED_ZERO_VALUE: u32 = 0x0000_0000;
    /// Offset of `reference_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const REFERENCE_COUNT: usize = 175;
    /// Offset of `references` (`bytes[66]`). Spec §3.1.
    pub(crate) const REFERENCES: usize = 179;
    /// Offset of `current_history_state` (`u32`, little-endian). Spec §3.1.
    pub(crate) const CURRENT_HISTORY_STATE: usize = 245;
    /// Offset of `kind_code_unit_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const KIND_CODE_UNIT_COUNT: usize = 249;
    /// Offset of `kind` (`bytes[10]`). Spec §3.1.
    pub(crate) const KIND: usize = 253;
    /// Offset of `feature_ordinal` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FEATURE_ORDINAL: usize = 263;
    /// Offset of `previous_history_state` (`u32`, little-endian). Spec §3.1.
    pub(crate) const PREVIOUS_HISTORY_STATE: usize = 294;
}

/// Byte offsets for the `design_draft_scope_class318_legacy` record.
///
/// Spec §3.1. Record length 373 B.
///
/// ```text
/// Offsets are relative to the primary Draft indexed header. The paired indexed header begins at offset 373.
/// ```
pub(crate) mod design_draft_scope_class318_legacy {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 373;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `reserved_zero` (`u32`, little-endian). Spec §3.1.
    pub(crate) const RESERVED_ZERO: usize = 171;
    /// Stated value of `reserved_zero` (`u32`). Spec §3.1.
    pub(crate) const RESERVED_ZERO_VALUE: u32 = 0x0000_0000;
    /// Offset of `reference_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const REFERENCE_COUNT: usize = 175;
    /// Offset of `references` (`bytes[99]`). Spec §3.1.
    pub(crate) const REFERENCES: usize = 179;
    /// Offset of `current_history_state` (`u32`, little-endian). Spec §3.1.
    pub(crate) const CURRENT_HISTORY_STATE: usize = 278;
    /// Offset of `kind_code_unit_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const KIND_CODE_UNIT_COUNT: usize = 282;
    /// Offset of `kind` (`bytes[10]`). Spec §3.1.
    pub(crate) const KIND: usize = 286;
    /// Offset of `feature_ordinal` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FEATURE_ORDINAL: usize = 296;
    /// Offset of `previous_history_state` (`u32`, little-endian). Spec §3.1.
    pub(crate) const PREVIOUS_HISTORY_STATE: usize = 327;
}

/// Byte offsets for the `design_draft_scope_class372` record.
///
/// Spec §3.1. Record length 340 B.
///
/// ```text
/// Offsets are relative to the primary Draft indexed header. The paired indexed header begins at offset 340.
/// ```
pub(crate) mod design_draft_scope_class372 {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 340;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `reserved_zero` (`u32`, little-endian). Spec §3.1.
    pub(crate) const RESERVED_ZERO: usize = 171;
    /// Stated value of `reserved_zero` (`u32`). Spec §3.1.
    pub(crate) const RESERVED_ZERO_VALUE: u32 = 0x0000_0000;
    /// Offset of `reference_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const REFERENCE_COUNT: usize = 175;
    /// Offset of `references` (`bytes[66]`). Spec §3.1.
    pub(crate) const REFERENCES: usize = 179;
    /// Offset of `current_history_state` (`u32`, little-endian). Spec §3.1.
    pub(crate) const CURRENT_HISTORY_STATE: usize = 245;
    /// Offset of `kind_code_unit_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const KIND_CODE_UNIT_COUNT: usize = 249;
    /// Offset of `kind` (`bytes[10]`). Spec §3.1.
    pub(crate) const KIND: usize = 253;
    /// Offset of `feature_ordinal` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FEATURE_ORDINAL: usize = 263;
    /// Offset of `previous_history_state` (`u32`, little-endian). Spec §3.1.
    pub(crate) const PREVIOUS_HISTORY_STATE: usize = 294;
}

/// Byte offsets for the `design_draft_scope_class393` record.
///
/// Spec §3.1. Record length 339 B.
///
/// ```text
/// Offsets are relative to the primary Draft indexed header. The paired indexed header begins at offset 339.
/// ```
pub(crate) mod design_draft_scope_class393 {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 339;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `reserved_zero` (`u32`, little-endian). Spec §3.1.
    pub(crate) const RESERVED_ZERO: usize = 171;
    /// Stated value of `reserved_zero` (`u32`). Spec §3.1.
    pub(crate) const RESERVED_ZERO_VALUE: u32 = 0x0000_0000;
    /// Offset of `reference_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const REFERENCE_COUNT: usize = 175;
    /// Offset of `references` (`bytes[66]`). Spec §3.1.
    pub(crate) const REFERENCES: usize = 179;
    /// Offset of `current_history_state` (`u32`, little-endian). Spec §3.1.
    pub(crate) const CURRENT_HISTORY_STATE: usize = 245;
    /// Offset of `kind_code_unit_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const KIND_CODE_UNIT_COUNT: usize = 249;
    /// Offset of `kind` (`bytes[10]`). Spec §3.1.
    pub(crate) const KIND: usize = 253;
    /// Offset of `feature_ordinal` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FEATURE_ORDINAL: usize = 263;
    /// Offset of `previous_history_state` (`u32`, little-endian). Spec §3.1.
    pub(crate) const PREVIOUS_HISTORY_STATE: usize = 293;
}

/// Byte offsets for the `design_draft_scope_class448` record.
///
/// Spec §3.1. Record length 340 B.
///
/// ```text
/// Offsets are relative to the primary Draft indexed header. The paired indexed header begins at offset 340.
/// ```
pub(crate) mod design_draft_scope_class448 {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 340;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `reserved_zero` (`u32`, little-endian). Spec §3.1.
    pub(crate) const RESERVED_ZERO: usize = 171;
    /// Stated value of `reserved_zero` (`u32`). Spec §3.1.
    pub(crate) const RESERVED_ZERO_VALUE: u32 = 0x0000_0000;
    /// Offset of `reference_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const REFERENCE_COUNT: usize = 175;
    /// Offset of `references` (`bytes[66]`). Spec §3.1.
    pub(crate) const REFERENCES: usize = 179;
    /// Offset of `current_history_state` (`u32`, little-endian). Spec §3.1.
    pub(crate) const CURRENT_HISTORY_STATE: usize = 245;
    /// Offset of `kind_code_unit_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const KIND_CODE_UNIT_COUNT: usize = 249;
    /// Offset of `kind` (`bytes[10]`). Spec §3.1.
    pub(crate) const KIND: usize = 253;
    /// Offset of `feature_ordinal` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FEATURE_ORDINAL: usize = 263;
    /// Offset of `previous_history_state` (`u32`, little-endian). Spec §3.1.
    pub(crate) const PREVIOUS_HISTORY_STATE: usize = 294;
}

/// Byte offsets for the `design_hole_point_data_v1_prefix` record.
///
/// Spec §3.1. Record length 97 B.
///
/// ```text
/// Offsets are relative to the version-one Hole point-data primary indexed header. The counted non-null reference run follows the fixed prefix.
/// ```
pub(crate) mod design_hole_point_data_v1_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 97;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_8` (`bytes[8]`). Spec §3.1.
    pub(crate) const ZERO_RUN_8: usize = 11;
    /// Offset of `leading_block_presence` (`u8`). Spec §3.1.
    pub(crate) const LEADING_BLOCK_PRESENCE: usize = 19;
    /// Offset of `property_block_presence` (`u8`). Spec §3.1.
    pub(crate) const PROPERTY_BLOCK_PRESENCE: usize = 20;
    /// Offset of `bounding_box_index` (`u32`, little-endian). Spec §3.1.
    pub(crate) const BOUNDING_BOX_INDEX: usize = 21;
    /// Offset of `position` (`f64[3]`, little-endian). Spec §3.1.
    pub(crate) const POSITION: usize = 25;
    /// Offset of `direction` (`f64[3]`, little-endian). Spec §3.1.
    pub(crate) const DIRECTION: usize = 49;
    /// Offset of `point_parameters` (`f64[2]`, little-endian). Spec §3.1.
    pub(crate) const POINT_PARAMETERS: usize = 73;
    /// Offset of `reference_type` (`u32`, little-endian). Spec §3.1.
    pub(crate) const REFERENCE_TYPE: usize = 89;
    /// Offset of `input_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const INPUT_COUNT: usize = 93;
}

/// Byte offsets for the `design_hole_point_data_v4_prefix` record.
///
/// Spec §3.1. Record length 122 B.
///
/// ```text
/// Offsets are relative to the version-four Hole point-data primary indexed header. The counted non-null reference run follows the fixed prefix.
/// ```
pub(crate) mod design_hole_point_data_v4_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 122;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_8` (`bytes[8]`). Spec §3.1.
    pub(crate) const ZERO_RUN_8: usize = 11;
    /// Offset of `leading_block_presence` (`u8`). Spec §3.1.
    pub(crate) const LEADING_BLOCK_PRESENCE: usize = 19;
    /// Offset of `property_block_presence` (`u8`). Spec §3.1.
    pub(crate) const PROPERTY_BLOCK_PRESENCE: usize = 20;
    /// Offset of `bounding_box_index` (`u32`, little-endian). Spec §3.1.
    pub(crate) const BOUNDING_BOX_INDEX: usize = 21;
    /// Offset of `position` (`f64[3]`, little-endian). Spec §3.1.
    pub(crate) const POSITION: usize = 25;
    /// Offset of `direction` (`f64[3]`, little-endian). Spec §3.1.
    pub(crate) const DIRECTION: usize = 49;
    /// Offset of `point_parameters` (`f64[2]`, little-endian). Spec §3.1.
    pub(crate) const POINT_PARAMETERS: usize = 73;
    /// Offset of `reference_type` (`u32`, little-endian). Spec §3.1.
    pub(crate) const REFERENCE_TYPE: usize = 89;
    /// Offset of `tangent_prefix` (`u8`). Spec §3.1.
    pub(crate) const TANGENT_PREFIX: usize = 93;
    /// Offset of `tangent_point_data` (`f64[3]`, little-endian). Spec §3.1.
    pub(crate) const TANGENT_POINT_DATA: usize = 94;
    /// Offset of `input_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const INPUT_COUNT: usize = 118;
}

/// Byte offsets for the `design_hole_direct_selection_prefix` record.
///
/// Spec §3.1. Record length 40 B.
///
/// ```text
/// Fixed prefix through the variable asset UUID. The context UUID and nested indexed records follow.
/// ```
pub(crate) mod design_hole_direct_selection_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 40;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_10` (`bytes[10]`). Spec §3.1.
    pub(crate) const ZERO_RUN_10: usize = 11;
    /// Offset of `nested_selection_marker` (`u8`). Spec §3.1.
    pub(crate) const NESTED_SELECTION_MARKER: usize = 21;
    /// Stated value of `nested_selection_marker` (`u8`). Spec §3.1.
    pub(crate) const NESTED_SELECTION_MARKER_VALUE: u8 = 1;
    /// Offset of `nested_record_index` (`u32`, little-endian). Spec §3.1.
    pub(crate) const NESTED_RECORD_INDEX: usize = 22;
    /// Offset of `zero_run_6` (`bytes[6]`). Spec §3.1.
    pub(crate) const ZERO_RUN_6: usize = 26;
    /// Offset of `asset_presence` (`u32`, little-endian). Spec §3.1.
    pub(crate) const ASSET_PRESENCE: usize = 32;
    /// Stated value of `asset_presence` (`u32`). Spec §3.1.
    pub(crate) const ASSET_PRESENCE_VALUE: u32 = 0x0000_0001;
    /// Offset of `asset_uuid_code_unit_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const ASSET_UUID_CODE_UNIT_COUNT: usize = 36;
}

/// Byte offsets for the `scale_modern_operation_prefix` record.
///
/// Spec §3.1. Record length 79 B.
///
/// ```text
/// Offsets are relative to the modern Scale scope's primary indexed header. The ordered-reference tail continues after this fixed operation prefix.
/// ```
pub(crate) mod scale_modern_operation_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 79;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `factor_kind` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FACTOR_KIND: usize = 20;
    /// Offset of `factor` (`f64`, little-endian). Spec §3.1.
    pub(crate) const FACTOR: usize = 25;
    /// Offset of `center_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const CENTER_REFERENCE: usize = 33;
    /// Offset of `factor_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const FACTOR_REFERENCE: usize = 44;
    /// Offset of `factor_tail_one` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FACTOR_TAIL_ONE: usize = 55;
    /// Offset of `body_group_one` (`u32`, little-endian). Spec §3.1.
    pub(crate) const BODY_GROUP_ONE: usize = 60;
    /// Offset of `body_group_kind` (`u32`, little-endian). Spec §3.1.
    pub(crate) const BODY_GROUP_KIND: usize = 64;
    /// Offset of `body_group_marker` (`bytes[11]`). Spec §3.1.
    pub(crate) const BODY_GROUP_MARKER: usize = 68;
}

/// Byte offsets for the `scale_legacy_operation_prefix` record.
///
/// Spec §3.1. Record length 75 B.
///
/// ```text
/// Offsets are relative to the legacy Scale scope's primary indexed header. The frame tail carries the ordered-reference members.
/// ```
pub(crate) mod scale_legacy_operation_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 75;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `factor` (`f64`, little-endian). Spec §3.1.
    pub(crate) const FACTOR: usize = 21;
    /// Offset of `center_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const CENTER_REFERENCE: usize = 29;
    /// Offset of `factor_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const FACTOR_REFERENCE: usize = 40;
    /// Offset of `factor_kind` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FACTOR_KIND: usize = 51;
    /// Offset of `zero_byte` (`u8`). Spec §3.1.
    pub(crate) const ZERO_BYTE: usize = 55;
    /// Offset of `tail_one` (`u32`, little-endian). Spec §3.1.
    pub(crate) const TAIL_ONE: usize = 56;
    /// Offset of `body_group_one` (`u32`, little-endian). Spec §3.1.
    pub(crate) const BODY_GROUP_ONE: usize = 60;
    /// Offset of `body_group_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const BODY_GROUP_REFERENCE: usize = 64;
}

/// Byte offsets for the `design_body_map_prefix_10` record.
///
/// Spec §3.1. Record length 25 B.
///
/// ```text
/// Ten-reserved-byte variant. Offsets are relative to the typed body-map indexed header. The first selector/entity pair starts at offset 25.
/// ```
pub(crate) mod design_body_map_prefix_10 {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 25;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `reserved_zero_run` (`bytes[10]`). Spec §3.1.
    pub(crate) const RESERVED_ZERO_RUN: usize = 11;
    /// Offset of `pair_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const PAIR_COUNT: usize = 21;
}

/// Byte offsets for the `design_body_map_prefix_11` record.
///
/// Spec §3.1. Record length 26 B.
///
/// ```text
/// Eleven-reserved-byte variant. Offsets are relative to the typed body-map indexed header. The first selector/entity pair starts at offset 26.
/// ```
pub(crate) mod design_body_map_prefix_11 {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 26;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `reserved_zero_run` (`bytes[11]`). Spec §3.1.
    pub(crate) const RESERVED_ZERO_RUN: usize = 11;
    /// Offset of `pair_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const PAIR_COUNT: usize = 22;
}

/// Byte offsets for the `paramesh_entry_name_prefix` record.
///
/// Spec §3.1. Record length 32 B.
///
/// ```text
/// Offsets are relative to the entry-name record's indexed header. The variable u32-count UTF-16LE entry name starts at offset 32 and ends at the primary-record boundary.
/// ```
pub(crate) mod paramesh_entry_name_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 32;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_10` (`bytes[10]`). Spec §3.1.
    pub(crate) const ZERO_RUN_10: usize = 11;
    /// Offset of `guid_record_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const GUID_RECORD_REFERENCE: usize = 21;
}

/// Byte offsets for the `paramesh_guid_join_prefix` record.
///
/// Spec §3.1. Record length 83 B.
///
/// ```text
/// Offsets are relative to the container-GUID record's indexed header. A type-specific tail can follow the fixed join prefix.
/// ```
pub(crate) mod paramesh_guid_join_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 83;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_21` (`bytes[21]`). Spec §3.1.
    pub(crate) const ZERO_RUN_21: usize = 11;
    /// Offset of `fusion_uuid` (`bytes[40]`). Spec §3.1.
    pub(crate) const FUSION_UUID: usize = 32;
    /// Offset of `entry_name_backlink` (`bytes[11]`). Spec §3.1.
    pub(crate) const ENTRY_NAME_BACKLINK: usize = 72;
}

/// Byte offsets for the `paramesh_mesh_body_join_prefix` record.
///
/// Spec §3.1. Record length 564 B.
///
/// ```text
/// Offsets are relative to the mesh-body primary indexed header. Presentation fields occupy the unstated spans, and the primary record can continue after this fixed prefix.
/// ```
pub(crate) mod paramesh_mesh_body_join_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 564;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_10` (`bytes[10]`). Spec §3.1.
    pub(crate) const ZERO_RUN_10: usize = 11;
    /// Offset of `first_transform` (`f64[16]`, little-endian). Spec §3.1.
    pub(crate) const FIRST_TRANSFORM: usize = 42;
    /// Offset of `second_transform` (`f64[16]`, little-endian). Spec §3.1.
    pub(crate) const SECOND_TRANSFORM: usize = 171;
    /// Offset of `feature_scope_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const FEATURE_SCOPE_REFERENCE: usize = 508;
    /// Offset of `wrapper_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const WRAPPER_REFERENCE: usize = 519;
    /// Offset of `body_owner_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const BODY_OWNER_REFERENCE: usize = 530;
    /// Offset of `container_guid_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const CONTAINER_GUID_REFERENCE: usize = 541;
    /// Offset of `scene_node_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const SCENE_NODE_REFERENCE: usize = 553;
}

/// Byte offsets for the `paramesh_mesh_collection_prefix` record.
///
/// Spec §3.1. Record length 38 B.
///
/// ```text
/// Offsets are relative to the mesh-collection indexed header. The nested CommonData record starts at the end of this prefix.
/// ```
pub(crate) mod paramesh_mesh_collection_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 38;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_10` (`bytes[10]`). Spec §3.1.
    pub(crate) const ZERO_RUN_10: usize = 11;
    /// Offset of `body_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const BODY_COUNT: usize = 21;
    /// Offset of `constant_01_01` (`bytes[2]`). Spec §3.1.
    pub(crate) const CONSTANT_01_01: usize = 25;
    /// Offset of `texture_table_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const TEXTURE_TABLE_REFERENCE: usize = 27;
}

/// Byte offsets for the `paramesh_mesh_collection_base_prefix` record.
///
/// Spec §3.1. Record length 24 B.
///
/// ```text
/// Offsets are relative to the nested CommonData indexed header at collection offset 38. The variable body-reference list starts at nested offset 24, which is collection offset 62.
/// ```
pub(crate) mod paramesh_mesh_collection_base_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 24;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_9` (`bytes[9]`). Spec §3.1.
    pub(crate) const ZERO_RUN_9: usize = 11;
    /// Offset of `body_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const BODY_COUNT: usize = 20;
}

/// Byte offsets for the `paramesh_texture_table_prefix` record.
///
/// Spec §3.1. Record length 25 B.
///
/// ```text
/// Offsets are relative to the texture-table indexed header. The first variable flags-map entry starts at offset 25.
/// ```
pub(crate) mod paramesh_texture_table_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 25;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_10` (`bytes[10]`). Spec §3.1.
    pub(crate) const ZERO_RUN_10: usize = 11;
    /// Offset of `flags_map_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FLAGS_MAP_COUNT: usize = 21;
}

/// Byte offsets for the `paramesh_texture_filename_prefix` record.
///
/// Spec §3.1. Record length 25 B.
///
/// ```text
/// Offsets are relative to the filename-record indexed header. UTF-16LE code units start at offset 25 and continue to the primary-record boundary.
/// ```
pub(crate) mod paramesh_texture_filename_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 25;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_10` (`bytes[10]`). Spec §3.1.
    pub(crate) const ZERO_RUN_10: usize = 11;
    /// Offset of `basename_code_unit_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const BASENAME_CODE_UNIT_COUNT: usize = 21;
}

/// Byte offsets for the `paramesh_body_wrapper` record.
///
/// Spec §3.1. Record length 40 B.
pub(crate) mod paramesh_body_wrapper {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 40;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_10` (`bytes[10]`). Spec §3.1.
    pub(crate) const ZERO_RUN_10: usize = 11;
    /// Offset of `body_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const BODY_REFERENCE: usize = 21;
    /// Offset of `zero_tail_8` (`bytes[8]`). Spec §3.1.
    pub(crate) const ZERO_TAIL_8: usize = 32;
}

/// Byte offsets for the `paramesh_feature_scope_prefix` record.
///
/// Spec §3.1. Record length 25 B.
///
/// ```text
/// Offsets are relative to the `Base Mesh Feature` indexed header. The ordered body-reference list starts at offset 25.
/// ```
pub(crate) mod paramesh_feature_scope_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 25;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_10` (`bytes[10]`). Spec §3.1.
    pub(crate) const ZERO_RUN_10: usize = 11;
    /// Offset of `body_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const BODY_COUNT: usize = 21;
}

/// Byte offsets for the `paramesh_feature_scope_base` record.
///
/// Spec §3.1. Record length 30 B.
pub(crate) mod paramesh_feature_scope_base {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 30;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_8` (`bytes[8]`). Spec §3.1.
    pub(crate) const ZERO_RUN_8: usize = 11;
    /// Offset of `scope_owner_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const SCOPE_OWNER_REFERENCE: usize = 19;
}

/// Byte offsets for the `paramesh_scene_state` record.
///
/// Spec §3.1. Record length 95 B.
pub(crate) mod paramesh_scene_state {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 95;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_34` (`bytes[34]`). Spec §3.1.
    pub(crate) const ZERO_RUN_34: usize = 11;
    /// Offset of `footer_marker` (`u8`). Spec §3.1.
    pub(crate) const FOOTER_MARKER: usize = 45;
    /// Offset of `footer_mask` (`bytes[49]`). Spec §3.1.
    pub(crate) const FOOTER_MASK: usize = 46;
}

/// Byte offsets for the `paramesh_scene_node` record.
///
/// Spec §3.1. Record length 133 B.
pub(crate) mod paramesh_scene_node {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 133;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_14` (`bytes[14]`). Spec §3.1.
    pub(crate) const ZERO_RUN_14: usize = 11;
    /// Offset of `constant_two_a` (`u32`, little-endian). Spec §3.1.
    pub(crate) const CONSTANT_TWO_A: usize = 25;
    /// Offset of `constant_two_b` (`u32`, little-endian). Spec §3.1.
    pub(crate) const CONSTANT_TWO_B: usize = 29;
    /// Offset of `scene_state_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const SCENE_STATE_REFERENCE: usize = 33;
    /// Offset of `constant_three` (`u32`, little-endian). Spec §3.1.
    pub(crate) const CONSTANT_THREE: usize = 44;
    /// Offset of `auxiliary_record_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const AUXILIARY_RECORD_REFERENCE: usize = 48;
    /// Offset of `zero_run_24` (`bytes[24]`). Spec §3.1.
    pub(crate) const ZERO_RUN_24: usize = 59;
    /// Offset of `footer_marker` (`u8`). Spec §3.1.
    pub(crate) const FOOTER_MARKER: usize = 83;
    /// Offset of `footer_mask` (`bytes[49]`). Spec §3.1.
    pub(crate) const FOOTER_MASK: usize = 84;
}

/// Byte offsets for the `paramesh_collection_owner_backlink_prefix` record.
///
/// Spec §3.1. Record length 273 B.
///
/// ```text
/// Offsets are relative to the collection-owner indexed header. The record can continue after this fixed backlink prefix.
/// ```
pub(crate) mod paramesh_collection_owner_backlink_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 273;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `collection_backlink` (`bytes[11]`). Spec §3.1.
    pub(crate) const COLLECTION_BACKLINK: usize = 262;
}

/// Byte offsets for the `assembly_operand_path_locator_reference_run` record.
///
/// Spec §Assembly operands. Record length 26 B.
///
/// ```text
/// Offsets are relative to the count. The run starts at scope offset 47 in the 399-byte As-built form, offset 362 in the 627-, 637-, and 692-byte forms, and offset 358 in the 633- and 732-byte forms.
/// ```
pub(crate) mod assembly_operand_path_locator_reference_run {
    /// Record length in bytes. Spec §Assembly operands.
    pub(crate) const LEN: usize = 26;
    /// Offset of `locator_count` (`u32`, little-endian). Spec §Assembly operands.
    pub(crate) const LOCATOR_COUNT: usize = 0;
    /// Offset of `first_locator_reference` (`bytes[11]`). Spec §Assembly operands.
    pub(crate) const FIRST_LOCATOR_REFERENCE: usize = 4;
    /// Offset of `second_locator_reference` (`bytes[11]`). Spec §Assembly operands.
    pub(crate) const SECOND_LOCATOR_REFERENCE: usize = 15;
}

/// Byte offsets for the `assembly_operand_path_locator` record.
///
/// Spec §Assembly operands. Record length 190 B.
///
/// ```text
/// Offsets are relative to the locator's indexed header. The variable-length occurrence-path record starts immediately after this record.
/// ```
pub(crate) mod assembly_operand_path_locator {
    /// Record length in bytes. Spec §Assembly operands.
    pub(crate) const LEN: usize = 190;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §Assembly operands.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_10` (`bytes[10]`). Spec §Assembly operands.
    pub(crate) const ZERO_RUN_10: usize = 11;
    /// Offset of `nonzero_record_reference` (`bytes[11]`). Spec §Assembly operands.
    pub(crate) const NONZERO_RECORD_REFERENCE: usize = 21;
    /// Offset of `zero_32` (`u8`). Spec §Assembly operands.
    pub(crate) const ZERO_32: usize = 32;
    /// Offset of `transform` (`f64[16]`, little-endian). Spec §Assembly operands.
    pub(crate) const TRANSFORM: usize = 33;
    /// Offset of `zero_161` (`u8`). Spec §Assembly operands.
    pub(crate) const ZERO_161: usize = 161;
    /// Offset of `scope_backlink` (`bytes[11]`). Spec §Assembly operands.
    pub(crate) const SCOPE_BACKLINK: usize = 162;
    /// Offset of `wrapper_reference` (`bytes[11]`). Spec §Assembly operands.
    pub(crate) const WRAPPER_REFERENCE: usize = 173;
    /// Offset of `constant_two` (`u32`, little-endian). Spec §Assembly operands.
    pub(crate) const CONSTANT_TWO: usize = 184;
    /// Offset of `zero_tail_2` (`bytes[2]`). Spec §Assembly operands.
    pub(crate) const ZERO_TAIL_2: usize = 188;
}

/// Byte offsets for the `assembly_operand_path_wrapper` record.
///
/// Spec §Assembly operands. Record length 37 B.
///
/// ```text
/// Offsets are relative to the wrapper's indexed header. The next indexed record starts at the end of this fixed record.
/// ```
pub(crate) mod assembly_operand_path_wrapper {
    /// Record length in bytes. Spec §Assembly operands.
    pub(crate) const LEN: usize = 37;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §Assembly operands.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_10` (`bytes[10]`). Spec §Assembly operands.
    pub(crate) const ZERO_RUN_10: usize = 11;
    /// Offset of `constant_one_byte` (`u8`). Spec §Assembly operands.
    pub(crate) const CONSTANT_ONE_BYTE: usize = 21;
    /// Offset of `constant_one_word` (`u32`, little-endian). Spec §Assembly operands.
    pub(crate) const CONSTANT_ONE_WORD: usize = 22;
    /// Offset of `path_reference` (`bytes[11]`). Spec §Assembly operands.
    pub(crate) const PATH_REFERENCE: usize = 26;
}

/// Byte offsets for the `assembly_axial_construction_carrier` record.
///
/// Spec §Assembly operands. Record length 391 B.
///
/// ```text
/// Offsets are relative to the construction carrier's primary indexed header. The uninterpreted gaps retain bytes outside the settled transform and axis-reference fields.
/// ```
pub(crate) mod assembly_axial_construction_carrier {
    /// Record length in bytes. Spec §Assembly operands.
    pub(crate) const LEN: usize = 391;
    /// Offset of `primary_indexed_header` (`bytes[11]`). Spec §Assembly operands.
    pub(crate) const PRIMARY_INDEXED_HEADER: usize = 0;
    /// Offset of `operand_transform` (`f64[16]`, little-endian). Spec §Assembly operands.
    pub(crate) const OPERAND_TRANSFORM: usize = 48;
    /// Offset of `first_axis_record_reference` (`bytes[11]`). Spec §Assembly operands.
    pub(crate) const FIRST_AXIS_RECORD_REFERENCE: usize = 192;
    /// Offset of `second_axis_record_reference` (`bytes[11]`). Spec §Assembly operands.
    pub(crate) const SECOND_AXIS_RECORD_REFERENCE: usize = 208;
    /// Offset of `paired_indexed_header` (`bytes[11]`). Spec §Assembly operands.
    pub(crate) const PAIRED_INDEXED_HEADER: usize = 380;
}

/// Byte offsets for the `assembly_axial_selector_prefix` record.
///
/// Spec §Assembly operands. Record length 37 B.
///
/// ```text
/// Offsets are relative to the selector record's primary indexed header. The two variable LP-UTF16 selector GUIDs follow this prefix.
/// ```
pub(crate) mod assembly_axial_selector_prefix {
    /// Record length in bytes. Spec §Assembly operands.
    pub(crate) const LEN: usize = 37;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §Assembly operands.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_11` (`bytes[11]`). Spec §Assembly operands.
    pub(crate) const ZERO_RUN_11: usize = 11;
    /// Offset of `nested_record_reference` (`bytes[11]`). Spec §Assembly operands.
    pub(crate) const NESTED_RECORD_REFERENCE: usize = 22;
    /// Offset of `constant_one` (`u32`, little-endian). Spec §Assembly operands.
    pub(crate) const CONSTANT_ONE: usize = 33;
}

/// Byte offsets for the `assembly_axial_role_prefix` record.
///
/// Spec §Assembly operands. Record length 29 B.
///
/// ```text
/// Offsets are relative to the role record's indexed header. The 36-code-unit UTF-16 role payload follows this fixed prefix.
/// ```
pub(crate) mod assembly_axial_role_prefix {
    /// Record length in bytes. Spec §Assembly operands.
    pub(crate) const LEN: usize = 29;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §Assembly operands.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_10` (`bytes[10]`). Spec §Assembly operands.
    pub(crate) const ZERO_RUN_10: usize = 11;
    /// Offset of `constant_one` (`u32`, little-endian). Spec §Assembly operands.
    pub(crate) const CONSTANT_ONE: usize = 21;
    /// Offset of `role_code_unit_count` (`u32`, little-endian). Spec §Assembly operands.
    pub(crate) const ROLE_CODE_UNIT_COUNT: usize = 25;
}

/// Byte offsets for the `grouped_recipe_reference_prefix` record.
///
/// Spec §3.1. Record length 18 B.
///
/// ```text
/// Offsets are relative to the recipe prefix. Five variable-length counted operand groups and one final u32 zero follow this fixed header.
/// ```
pub(crate) mod grouped_recipe_reference_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 18;
    /// Offset of `zero_run_10` (`bytes[10]`). Spec §3.1.
    pub(crate) const ZERO_RUN_10: usize = 0;
    /// Offset of `constant_one` (`u32`, little-endian). Spec §3.1.
    pub(crate) const CONSTANT_ONE: usize = 10;
    /// Offset of `group_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const GROUP_COUNT: usize = 14;
}

/// Byte offsets for the `combine_standard_operation_prefix` record.
///
/// Spec §3.1. Record length 33 B.
///
/// ```text
/// Offsets are relative to the primary indexed scope header. The variable scope body follows this fixed prologue.
/// ```
pub(crate) mod combine_standard_operation_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 33;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_9` (`bytes[9]`). Spec §3.1.
    pub(crate) const ZERO_RUN_9: usize = 11;
    /// Offset of `operation` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OPERATION: usize = 20;
    /// Offset of `zero_flag` (`u8`). Spec §3.1.
    pub(crate) const ZERO_FLAG: usize = 24;
    /// Offset of `keep_tools` (`u8`). Spec §3.1.
    pub(crate) const KEEP_TOOLS: usize = 25;
    /// Offset of `zero_run_7` (`bytes[7]`). Spec §3.1.
    pub(crate) const ZERO_RUN_7: usize = 26;
}

/// Byte offsets for the `combine_compact_operation_prefix` record.
///
/// Spec §3.1. Record length 46 B.
///
/// ```text
/// Offsets are relative to the class-387 primary indexed scope header. The variable scope body follows this fixed prologue.
/// ```
pub(crate) mod combine_compact_operation_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 46;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_10` (`bytes[10]`). Spec §3.1.
    pub(crate) const ZERO_RUN_10: usize = 11;
    /// Offset of `operation` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OPERATION: usize = 21;
    /// Offset of `keep_tools` (`u8`). Spec §3.1.
    pub(crate) const KEEP_TOOLS: usize = 25;
    /// Offset of `zero_run_3` (`bytes[3]`). Spec §3.1.
    pub(crate) const ZERO_RUN_3: usize = 26;
    /// Offset of `reference_form` (`bytes[2]`). Spec §3.1.
    pub(crate) const REFERENCE_FORM: usize = 29;
    /// Offset of `constant_one` (`u32`, little-endian). Spec §3.1.
    pub(crate) const CONSTANT_ONE: usize = 31;
    /// Offset of `reference_marker` (`u8`). Spec §3.1.
    pub(crate) const REFERENCE_MARKER: usize = 35;
    /// Offset of `reference_value` (`u64`, little-endian). Spec §3.1.
    pub(crate) const REFERENCE_VALUE: usize = 36;
    /// Offset of `reference_tail` (`bytes[2]`). Spec §3.1.
    pub(crate) const REFERENCE_TAIL: usize = 44;
}

/// Byte offsets for the `combine_extended_reference_operation_prefix` record.
///
/// Spec §3.1. Record length 46 B.
///
/// ```text
/// Offsets are relative to the class-329 primary indexed scope header. The variable scope body follows this fixed prologue.
/// ```
pub(crate) mod combine_extended_reference_operation_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 46;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_18` (`bytes[18]`). Spec §3.1.
    pub(crate) const ZERO_RUN_18: usize = 11;
    /// Offset of `form_marker` (`u8`). Spec §3.1.
    pub(crate) const FORM_MARKER: usize = 29;
    /// Offset of `keep_tools` (`u8`). Spec §3.1.
    pub(crate) const KEEP_TOOLS: usize = 30;
    /// Offset of `operation` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OPERATION: usize = 31;
    /// Offset of `reference_marker` (`u8`). Spec §3.1.
    pub(crate) const REFERENCE_MARKER: usize = 35;
    /// Offset of `reference_value` (`u64`, little-endian). Spec §3.1.
    pub(crate) const REFERENCE_VALUE: usize = 36;
    /// Offset of `reference_tail` (`bytes[2]`). Spec §3.1.
    pub(crate) const REFERENCE_TAIL: usize = 44;
}

/// Byte offsets for the `combine_external_selector_prefix` record.
///
/// Spec §3.1. Record length 40 B.
///
/// ```text
/// Offsets are relative to the tool body-selection header. The variable LP-UTF16 selector asset GUID starts at offset 40.
/// ```
pub(crate) mod combine_external_selector_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 40;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_14` (`bytes[14]`). Spec §3.1.
    pub(crate) const ZERO_RUN_14: usize = 11;
    /// Offset of `nested_reference_marker` (`u8`). Spec §3.1.
    pub(crate) const NESTED_REFERENCE_MARKER: usize = 25;
    /// Offset of `nested_record_index` (`u64`, little-endian). Spec §3.1.
    pub(crate) const NESTED_RECORD_INDEX: usize = 26;
    /// Offset of `nested_reference_tail` (`bytes[2]`). Spec §3.1.
    pub(crate) const NESTED_REFERENCE_TAIL: usize = 34;
    /// Offset of `constant_one` (`u32`, little-endian). Spec §3.1.
    pub(crate) const CONSTANT_ONE: usize = 36;
}

/// Byte offsets for the `combine_external_selector_tail` record.
///
/// Spec §3.1. Record length 62 B.
///
/// ```text
/// Offsets are relative to the first byte after the cross-document reference. The same-index paired header starts at offset 62.
/// ```
pub(crate) mod combine_external_selector_tail {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 62;
    /// Offset of `constant_nine` (`u32`, little-endian). Spec §3.1.
    pub(crate) const CONSTANT_NINE: usize = 0;
    /// Offset of `constant_two` (`u16`, little-endian). Spec §3.1.
    pub(crate) const CONSTANT_TWO: usize = 4;
    /// Offset of `tail_value_0` (`u64`, little-endian). Spec §3.1.
    pub(crate) const TAIL_VALUE_0: usize = 6;
    /// Offset of `constant_forty_eight` (`u32`, little-endian). Spec §3.1.
    pub(crate) const CONSTANT_FORTY_EIGHT: usize = 14;
    /// Offset of `tail_value_1` (`u64`, little-endian). Spec §3.1.
    pub(crate) const TAIL_VALUE_1: usize = 18;
    /// Offset of `nested_two_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const NESTED_TWO_REFERENCE: usize = 26;
    /// Offset of `zero_run_2` (`bytes[2]`). Spec §3.1.
    pub(crate) const ZERO_RUN_2: usize = 37;
    /// Offset of `nested_one_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const NESTED_ONE_REFERENCE: usize = 39;
    /// Offset of `zero_flag` (`u8`). Spec §3.1.
    pub(crate) const ZERO_FLAG: usize = 50;
    /// Offset of `scope_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const SCOPE_REFERENCE: usize = 51;
}

/// Byte offsets for the `indexed_companion_record_prefix` record.
///
/// Spec §3.1. Record length 58 B.
///
/// ```text
/// The stated field list tiles the stated 58-byte total exactly. The timestamp is a wall-clock value recorded at a parameter authoring event.
/// ```
pub(crate) mod indexed_companion_record_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 58;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_20` (`bytes[20]`). Spec §3.1.
    pub(crate) const ZERO_RUN_20: usize = 11;
    /// Offset of `owner_marker` (`u8`). Spec §3.1.
    pub(crate) const OWNER_MARKER: usize = 31;
    /// Offset of `owner_record_index` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OWNER_RECORD_INDEX: usize = 32;
    /// Offset of `zero_run_6` (`bytes[6]`). Spec §3.1.
    pub(crate) const ZERO_RUN_6: usize = 36;
    /// Offset of `timestamp_micros` (`u64`, little-endian). Spec §3.1.
    pub(crate) const TIMESTAMP_MICROS: usize = 42;
    /// Offset of `zero_run_8` (`bytes[8]`). Spec §3.1.
    pub(crate) const ZERO_RUN_8: usize = 50;
}

/// Byte offsets for the `named_solid_primitive_prologue` record.
///
/// Spec §3.1. Record length 26 B.
///
/// ```text
/// Offsets are relative to the primary indexed scope header. The ordered parameter-owner references and the paired header follow this fixed prologue.
/// ```
pub(crate) mod named_solid_primitive_prologue {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 26;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_9` (`bytes[9]`). Spec §3.1.
    pub(crate) const ZERO_RUN_9: usize = 11;
    /// Offset of `operation` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OPERATION: usize = 20;
    /// Offset of `zero_flag` (`u8`). Spec §3.1.
    pub(crate) const ZERO_FLAG: usize = 24;
    /// Offset of `form_marker` (`u8`). Spec §3.1.
    pub(crate) const FORM_MARKER: usize = 25;
}

/// Byte offsets for the `shifted_cylinder_primitive_352_frame` record.
///
/// Spec §3.1. Record length 352 B.
///
/// ```text
/// Offsets are relative to the primary indexed scope header. The generic scope reference table begins at offset 174; the paired header follows the complete scope frame.
/// ```
pub(crate) mod shifted_cylinder_primitive_352_frame {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 352;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_10` (`bytes[10]`). Spec §3.1.
    pub(crate) const ZERO_RUN_10: usize = 11;
    /// Offset of `form_marker` (`u8`). Spec §3.1.
    pub(crate) const FORM_MARKER: usize = 21;
    /// Stated value of `form_marker` (`u8`). Spec §3.1.
    pub(crate) const FORM_MARKER_VALUE: u8 = 1;
    /// Offset of `operation` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OPERATION: usize = 22;
    /// Offset of `first_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const FIRST_REFERENCE: usize = 26;
    /// Offset of `reference_gap` (`bytes[1]`). Spec §3.1.
    pub(crate) const REFERENCE_GAP: usize = 37;
    /// Offset of `second_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const SECOND_REFERENCE: usize = 38;
    /// Offset of `third_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const THIRD_REFERENCE: usize = 49;
    /// Offset of `fourth_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const FOURTH_REFERENCE: usize = 60;
    /// Offset of `compact_tail_marker` (`u8`). Spec §3.1.
    pub(crate) const COMPACT_TAIL_MARKER: usize = 71;
    /// Offset of `compact_tail_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const COMPACT_TAIL_COUNT: usize = 72;
    /// Stated value of `compact_tail_count` (`u32`). Spec §3.1.
    pub(crate) const COMPACT_TAIL_COUNT_VALUE: u32 = 0x0000_0001;
    /// Offset of `compact_tail_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const COMPACT_TAIL_REFERENCE: usize = 76;
    /// Offset of `compact_tail_zero_run_8` (`bytes[8]`). Spec §3.1.
    pub(crate) const COMPACT_TAIL_ZERO_RUN_8: usize = 87;
    /// Offset of `guid_code_unit_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const GUID_CODE_UNIT_COUNT: usize = 95;
    /// Stated value of `guid_code_unit_count` (`u32`). Spec §3.1.
    pub(crate) const GUID_CODE_UNIT_COUNT_VALUE: u32 = 0x0000_0024;
    /// Offset of `guid` (`bytes[72]`). Spec §3.1.
    pub(crate) const GUID: usize = 99;
    /// Offset of `zero_run_3_after_guid` (`bytes[3]`). Spec §3.1.
    pub(crate) const ZERO_RUN_3_AFTER_GUID: usize = 171;
    /// Offset of `reference_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const REFERENCE_COUNT: usize = 174;
    /// Offset of `references` (`bytes[55]`). Spec §3.1.
    pub(crate) const REFERENCES: usize = 178;
    /// Offset of `history_state_id` (`u32`, little-endian). Spec §3.1.
    pub(crate) const HISTORY_STATE_ID: usize = 233;
    /// Offset of `kind_code_unit_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const KIND_CODE_UNIT_COUNT: usize = 237;
    /// Stated value of `kind_code_unit_count` (`u32`). Spec §3.1.
    pub(crate) const KIND_CODE_UNIT_COUNT_VALUE: u32 = 0x0000_0011;
    /// Offset of `kind` (`bytes[34]`). Spec §3.1.
    pub(crate) const KIND: usize = 241;
    /// Offset of `feature_ordinal` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FEATURE_ORDINAL: usize = 275;
    /// Offset of `previous_history_state_id` (`u32`, little-endian). Spec §3.1.
    pub(crate) const PREVIOUS_HISTORY_STATE_ID: usize = 306;
}

/// Byte offsets for the `shifted_cylinder_primitive_502_frame` record.
///
/// Spec §3.1. Record length 502 B.
///
/// ```text
/// Offsets are relative to the primary indexed scope header. The generic scope reference table begins at offset 302; the paired header follows the complete scope frame.
/// ```
pub(crate) mod shifted_cylinder_primitive_502_frame {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 502;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_10` (`bytes[10]`). Spec §3.1.
    pub(crate) const ZERO_RUN_10: usize = 11;
    /// Offset of `form_marker` (`u8`). Spec §3.1.
    pub(crate) const FORM_MARKER: usize = 21;
    /// Stated value of `form_marker` (`u8`). Spec §3.1.
    pub(crate) const FORM_MARKER_VALUE: u8 = 1;
    /// Offset of `operation` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OPERATION: usize = 22;
    /// Offset of `first_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const FIRST_REFERENCE: usize = 26;
    /// Offset of `reference_gap` (`bytes[1]`). Spec §3.1.
    pub(crate) const REFERENCE_GAP: usize = 37;
    /// Offset of `second_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const SECOND_REFERENCE: usize = 38;
    /// Offset of `third_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const THIRD_REFERENCE: usize = 49;
    /// Offset of `fourth_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const FOURTH_REFERENCE: usize = 60;
    /// Offset of `zero_before_matrix` (`bytes[1]`). Spec §3.1.
    pub(crate) const ZERO_BEFORE_MATRIX: usize = 71;
    /// Offset of `matrix` (`f64[16]`, little-endian). Spec §3.1.
    pub(crate) const MATRIX: usize = 72;
    /// Offset of `zero_run_8_after_matrix` (`bytes[8]`). Spec §3.1.
    pub(crate) const ZERO_RUN_8_AFTER_MATRIX: usize = 200;
    /// Offset of `construction_reference` (`bytes[15]`). Spec §3.1.
    pub(crate) const CONSTRUCTION_REFERENCE: usize = 208;
    /// Offset of `guid_code_unit_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const GUID_CODE_UNIT_COUNT: usize = 223;
    /// Stated value of `guid_code_unit_count` (`u32`). Spec §3.1.
    pub(crate) const GUID_CODE_UNIT_COUNT_VALUE: u32 = 0x0000_0024;
    /// Offset of `guid` (`bytes[72]`). Spec §3.1.
    pub(crate) const GUID: usize = 227;
    /// Offset of `zero_run_3_after_guid` (`bytes[3]`). Spec §3.1.
    pub(crate) const ZERO_RUN_3_AFTER_GUID: usize = 299;
    /// Offset of `reference_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const REFERENCE_COUNT: usize = 302;
    /// Offset of `references` (`bytes[77]`). Spec §3.1.
    pub(crate) const REFERENCES: usize = 306;
    /// Offset of `history_state_id` (`u32`, little-endian). Spec §3.1.
    pub(crate) const HISTORY_STATE_ID: usize = 383;
    /// Offset of `kind_code_unit_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const KIND_CODE_UNIT_COUNT: usize = 387;
    /// Stated value of `kind_code_unit_count` (`u32`). Spec §3.1.
    pub(crate) const KIND_CODE_UNIT_COUNT_VALUE: u32 = 0x0000_0011;
    /// Offset of `kind` (`bytes[34]`). Spec §3.1.
    pub(crate) const KIND: usize = 391;
    /// Offset of `feature_ordinal` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FEATURE_ORDINAL: usize = 425;
    /// Offset of `previous_history_state_id` (`u32`, little-endian). Spec §3.1.
    pub(crate) const PREVIOUS_HISTORY_STATE_ID: usize = 456;
}

/// Byte offsets for the `compact_loft_operation_prefix` record.
///
/// Spec §3.1. Record length 45 B.
///
/// ```text
/// Offsets are relative to the primary indexed header. The variable scope body follows this fixed prefix.
/// ```
pub(crate) mod compact_loft_operation_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 45;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_10` (`bytes[10]`). Spec §3.1.
    pub(crate) const ZERO_RUN_10: usize = 11;
    /// Offset of `one_run_4` (`bytes[4]`). Spec §3.1.
    pub(crate) const ONE_RUN_4: usize = 21;
    /// Offset of `operation` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OPERATION: usize = 25;
    /// Offset of `zero_flag` (`u8`). Spec §3.1.
    pub(crate) const ZERO_FLAG: usize = 29;
    /// Offset of `all_ones` (`bytes[4]`). Spec §3.1.
    pub(crate) const ALL_ONES: usize = 30;
    /// Offset of `zero_run_11` (`bytes[11]`). Spec §3.1.
    pub(crate) const ZERO_RUN_11: usize = 34;
}

/// Byte offsets for the `sketch_profile_region_selection_prefix` record.
///
/// Spec §3.1. Record length 40 B.
///
/// ```text
/// Offsets are relative to the N+3 selection header. The ordered variable-length region run starts at offset 40.
/// ```
pub(crate) mod sketch_profile_region_selection_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 40;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_10` (`bytes[10]`). Spec §3.1.
    pub(crate) const ZERO_RUN_10: usize = 11;
    /// Offset of `profile_reference_marker` (`u8`). Spec §3.1.
    pub(crate) const PROFILE_REFERENCE_MARKER: usize = 21;
    /// Offset of `profile_record_index` (`u32`, little-endian). Spec §3.1.
    pub(crate) const PROFILE_RECORD_INDEX: usize = 22;
    /// Offset of `zero_run_6` (`bytes[6]`). Spec §3.1.
    pub(crate) const ZERO_RUN_6: usize = 26;
    /// Offset of `format_version` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FORMAT_VERSION: usize = 32;
    /// Offset of `region_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const REGION_COUNT: usize = 36;
}

/// Byte offsets for the `sketch_profile_region_member` record.
///
/// Spec §3.1. Record length 40 B.
///
/// ```text
/// The member repeats within each selected region. Region and member counts and the later-region marker are outside this fixed member frame.
/// ```
pub(crate) mod sketch_profile_region_member {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 40;
    /// Offset of `kind` (`u32`, little-endian). Spec §3.1.
    pub(crate) const KIND: usize = 0;
    /// Offset of `curve_primary_id` (`u32`, little-endian). Spec §3.1.
    pub(crate) const CURVE_PRIMARY_ID: usize = 4;
    /// Offset of `zero_words_3` (`u32[3]`, little-endian). Spec §3.1.
    pub(crate) const ZERO_WORDS_3: usize = 8;
    /// Offset of `incidence_words` (`u32[3]`, little-endian). Spec §3.1.
    pub(crate) const INCIDENCE_WORDS: usize = 20;
    /// Offset of `zero_words_2` (`u32[2]`, little-endian). Spec §3.1.
    pub(crate) const ZERO_WORDS_2: usize = 32;
}

/// Byte offsets for the `base_feature_class_377_prefix` record.
///
/// Spec §3.1. Record length 363 B.
///
/// ```text
/// Offsets are relative to the class-377 primary indexed header; the generic scope suffix and paired header remain part of the fixed frame.
/// ```
pub(crate) mod base_feature_class_377_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 363;
    /// Offset of `zero_run_8` (`bytes[8]`). Spec §3.1.
    pub(crate) const ZERO_RUN_8: usize = 11;
    /// Offset of `body_reference_count_marker` (`u8`). Spec §3.1.
    pub(crate) const BODY_REFERENCE_COUNT_MARKER: usize = 19;
    /// Stated value of `body_reference_count_marker` (`u8`). Spec §3.1.
    pub(crate) const BODY_REFERENCE_COUNT_MARKER_VALUE: u8 = 1;
    /// Offset of `body_reference_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const BODY_REFERENCE_COUNT: usize = 20;
    /// Stated value of `body_reference_count` (`u32`). Spec §3.1.
    pub(crate) const BODY_REFERENCE_COUNT_VALUE: u32 = 0x0000_0002;
    /// Offset of `parameter_body_reference_marker` (`u8`). Spec §3.1.
    pub(crate) const PARAMETER_BODY_REFERENCE_MARKER: usize = 24;
    /// Stated value of `parameter_body_reference_marker` (`u8`). Spec §3.1.
    pub(crate) const PARAMETER_BODY_REFERENCE_MARKER_VALUE: u8 = 1;
    /// Offset of `parameter_body_record` (`u32`, little-endian). Spec §3.1.
    pub(crate) const PARAMETER_BODY_RECORD: usize = 25;
    /// Offset of `parameter_body_reference_field` (`bytes[10]`). Spec §3.1.
    pub(crate) const PARAMETER_BODY_REFERENCE_FIELD: usize = 29;
    /// Offset of `body_entity_reference_marker` (`u8`). Spec §3.1.
    pub(crate) const BODY_ENTITY_REFERENCE_MARKER: usize = 39;
    /// Stated value of `body_entity_reference_marker` (`u8`). Spec §3.1.
    pub(crate) const BODY_ENTITY_REFERENCE_MARKER_VALUE: u8 = 1;
    /// Offset of `body_entity_suffix` (`u32`, little-endian). Spec §3.1.
    pub(crate) const BODY_ENTITY_SUFFIX: usize = 40;
    /// Offset of `body_entity_reference_field` (`bytes[10]`). Spec §3.1.
    pub(crate) const BODY_ENTITY_REFERENCE_FIELD: usize = 44;
    /// Offset of `tag_body_based_on_faces_marker` (`u8`). Spec §3.1.
    pub(crate) const TAG_BODY_BASED_ON_FACES_MARKER: usize = 54;
    /// Stated value of `tag_body_based_on_faces_marker` (`u8`). Spec §3.1.
    pub(crate) const TAG_BODY_BASED_ON_FACES_MARKER_VALUE: u8 = 1;
    /// Offset of `tag_body_based_on_faces_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const TAG_BODY_BASED_ON_FACES_COUNT: usize = 55;
    /// Stated value of `tag_body_based_on_faces_count` (`u32`). Spec §3.1.
    pub(crate) const TAG_BODY_BASED_ON_FACES_COUNT_VALUE: u32 = 0x0000_0001;
    /// Offset of `tag_body_based_on_faces_key_length` (`u32`, little-endian). Spec §3.1.
    pub(crate) const TAG_BODY_BASED_ON_FACES_KEY_LENGTH: usize = 59;
    /// Stated value of `tag_body_based_on_faces_key_length` (`u32`). Spec §3.1.
    pub(crate) const TAG_BODY_BASED_ON_FACES_KEY_LENGTH_VALUE: u32 = 0x0000_0013;
    /// Offset of `tag_body_based_on_faces_key` (`bytes[19]`). Spec §3.1.
    pub(crate) const TAG_BODY_BASED_ON_FACES_KEY: usize = 63;
    /// Offset of `tag_body_based_on_faces_type_length` (`u32`, little-endian). Spec §3.1.
    pub(crate) const TAG_BODY_BASED_ON_FACES_TYPE_LENGTH: usize = 82;
    /// Stated value of `tag_body_based_on_faces_type_length` (`u32`). Spec §3.1.
    pub(crate) const TAG_BODY_BASED_ON_FACES_TYPE_LENGTH_VALUE: u32 = 0x0000_0015;
    /// Offset of `tag_body_based_on_faces_type` (`bytes[21]`). Spec §3.1.
    pub(crate) const TAG_BODY_BASED_ON_FACES_TYPE: usize = 86;
    /// Offset of `tag_body_based_on_faces_value` (`u16`, little-endian). Spec §3.1.
    pub(crate) const TAG_BODY_BASED_ON_FACES_VALUE: usize = 107;
    /// Stated value of `tag_body_based_on_faces_value` (`u16`). Spec §3.1.
    pub(crate) const TAG_BODY_BASED_ON_FACES_VALUE_VALUE: u16 = 0x0001;
    /// Offset of `parameter_reference_group_marker` (`u8`). Spec §3.1.
    pub(crate) const PARAMETER_REFERENCE_GROUP_MARKER: usize = 109;
    /// Stated value of `parameter_reference_group_marker` (`u8`). Spec §3.1.
    pub(crate) const PARAMETER_REFERENCE_GROUP_MARKER_VALUE: u8 = 1;
    /// Offset of `parameter_reference_group_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const PARAMETER_REFERENCE_GROUP_COUNT: usize = 110;
    /// Stated value of `parameter_reference_group_count` (`u32`). Spec §3.1.
    pub(crate) const PARAMETER_REFERENCE_GROUP_COUNT_VALUE: u32 = 0x0000_0001;
    /// Offset of `parameter_reference_marker` (`u8`). Spec §3.1.
    pub(crate) const PARAMETER_REFERENCE_MARKER: usize = 114;
    /// Stated value of `parameter_reference_marker` (`u8`). Spec §3.1.
    pub(crate) const PARAMETER_REFERENCE_MARKER_VALUE: u8 = 1;
    /// Offset of `parameter_reference_record` (`u32`, little-endian). Spec §3.1.
    pub(crate) const PARAMETER_REFERENCE_RECORD: usize = 115;
    /// Offset of `parameter_reference_field` (`bytes[7]`). Spec §3.1.
    pub(crate) const PARAMETER_REFERENCE_FIELD: usize = 119;
    /// Offset of `scope_reference_member_marker` (`u8`). Spec §3.1.
    pub(crate) const SCOPE_REFERENCE_MEMBER_MARKER: usize = 126;
    /// Stated value of `scope_reference_member_marker` (`u8`). Spec §3.1.
    pub(crate) const SCOPE_REFERENCE_MEMBER_MARKER_VALUE: u8 = 1;
    /// Offset of `scope_reference_member_record` (`u32`, little-endian). Spec §3.1.
    pub(crate) const SCOPE_REFERENCE_MEMBER_RECORD: usize = 127;
    /// Offset of `scope_reference_member_field` (`bytes[6]`). Spec §3.1.
    pub(crate) const SCOPE_REFERENCE_MEMBER_FIELD: usize = 131;
    /// Offset of `auxiliary_group_marker` (`u8`). Spec §3.1.
    pub(crate) const AUXILIARY_GROUP_MARKER: usize = 137;
    /// Stated value of `auxiliary_group_marker` (`u8`). Spec §3.1.
    pub(crate) const AUXILIARY_GROUP_MARKER_VALUE: u8 = 1;
    /// Offset of `auxiliary_group_zero_run` (`bytes[3]`). Spec §3.1.
    pub(crate) const AUXILIARY_GROUP_ZERO_RUN: usize = 138;
    /// Offset of `auxiliary_reference_marker` (`u8`). Spec §3.1.
    pub(crate) const AUXILIARY_REFERENCE_MARKER: usize = 141;
    /// Stated value of `auxiliary_reference_marker` (`u8`). Spec §3.1.
    pub(crate) const AUXILIARY_REFERENCE_MARKER_VALUE: u8 = 1;
    /// Offset of `auxiliary_record` (`u32`, little-endian). Spec §3.1.
    pub(crate) const AUXILIARY_RECORD: usize = 142;
    /// Offset of `auxiliary_reference_field` (`bytes[14]`). Spec §3.1.
    pub(crate) const AUXILIARY_REFERENCE_FIELD: usize = 146;
    /// Offset of `envelope_guid_code_unit_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const ENVELOPE_GUID_CODE_UNIT_COUNT: usize = 160;
    /// Stated value of `envelope_guid_code_unit_count` (`u32`). Spec §3.1.
    pub(crate) const ENVELOPE_GUID_CODE_UNIT_COUNT_VALUE: u32 = 0x0000_0024;
    /// Offset of `envelope_guid` (`bytes[72]`). Spec §3.1.
    pub(crate) const ENVELOPE_GUID: usize = 164;
    /// Offset of `zero_run_3` (`bytes[3]`). Spec §3.1.
    pub(crate) const ZERO_RUN_3: usize = 236;
    /// Offset of `reference_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const REFERENCE_COUNT: usize = 239;
    /// Stated value of `reference_count` (`u32`). Spec §3.1.
    pub(crate) const REFERENCE_COUNT_VALUE: u32 = 0x0000_0001;
    /// Offset of `generic_scope_reference_marker` (`u8`). Spec §3.1.
    pub(crate) const GENERIC_SCOPE_REFERENCE_MARKER: usize = 243;
    /// Stated value of `generic_scope_reference_marker` (`u8`). Spec §3.1.
    pub(crate) const GENERIC_SCOPE_REFERENCE_MARKER_VALUE: u8 = 1;
    /// Offset of `generic_scope_reference_record` (`u32`, little-endian). Spec §3.1.
    pub(crate) const GENERIC_SCOPE_REFERENCE_RECORD: usize = 244;
    /// Offset of `generic_scope_reference_field` (`bytes[6]`). Spec §3.1.
    pub(crate) const GENERIC_SCOPE_REFERENCE_FIELD: usize = 248;
    /// Offset of `history_state_id` (`u32`, little-endian). Spec §3.1.
    pub(crate) const HISTORY_STATE_ID: usize = 254;
    /// Offset of `kind_length` (`u32`, little-endian). Spec §3.1.
    pub(crate) const KIND_LENGTH: usize = 258;
    /// Stated value of `kind_length` (`u32`). Spec §3.1.
    pub(crate) const KIND_LENGTH_VALUE: u32 = 0x0000_000c;
    /// Offset of `kind` (`bytes[24]`). Spec §3.1.
    pub(crate) const KIND: usize = 262;
    /// Offset of `feature_ordinal` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FEATURE_ORDINAL: usize = 286;
    /// Offset of `previous_history_state_id` (`u32`, little-endian). Spec §3.1.
    pub(crate) const PREVIOUS_HISTORY_STATE_ID: usize = 317;
}

/// Byte offsets for the `base_feature_result_body_prefix` record.
///
/// Spec §3.1. Record length 24 B.
///
/// ```text
/// Offsets are relative to the primary indexed header. The two parallel 15-byte body-entry runs begin at offset 24.
/// ```
pub(crate) mod base_feature_result_body_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 24;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_8` (`bytes[8]`). Spec §3.1.
    pub(crate) const ZERO_RUN_8: usize = 11;
    /// Offset of `body_count_marker` (`u8`). Spec §3.1.
    pub(crate) const BODY_COUNT_MARKER: usize = 19;
    /// Offset of `combined_body_reference_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const COMBINED_BODY_REFERENCE_COUNT: usize = 20;
}

/// Byte offsets for the `base_feature_legacy_zero_body` record.
///
/// Spec §3.1. Record length 55 B.
///
/// ```text
/// Offsets are relative to the class-409 primary indexed header. The shared metadata record is the only retained body-snapshot reference.
/// ```
pub(crate) mod base_feature_legacy_zero_body {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 55;
    /// Offset of `zero_run_9` (`bytes[9]`). Spec §3.1.
    pub(crate) const ZERO_RUN_9: usize = 11;
    /// Offset of `zero_body_marker` (`u8`). Spec §3.1.
    pub(crate) const ZERO_BODY_MARKER: usize = 20;
    /// Offset of `zero_run_11` (`bytes[11]`). Spec §3.1.
    pub(crate) const ZERO_RUN_11: usize = 21;
    /// Offset of `shared_metadata_marker` (`u8`). Spec §3.1.
    pub(crate) const SHARED_METADATA_MARKER: usize = 32;
    /// Offset of `shared_metadata_record` (`u64`, little-endian). Spec §3.1.
    pub(crate) const SHARED_METADATA_RECORD: usize = 33;
    /// Offset of `shared_metadata_field` (`bytes[6]`). Spec §3.1.
    pub(crate) const SHARED_METADATA_FIELD: usize = 41;
    /// Offset of `zero_padding_8` (`bytes[8]`). Spec §3.1.
    pub(crate) const ZERO_PADDING_8: usize = 47;
}

/// Byte offsets for the `base_feature_legacy_444_zero_body` record.
///
/// Spec §3.1. Record length 157 B.
///
/// ```text
/// Offsets are relative to the class-444 primary indexed header. The fixed prefix ends at the 12-code-unit kind length; the generic scope tail and paired header follow it.
/// ```
pub(crate) mod base_feature_legacy_444_zero_body {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 157;
    /// Offset of `zero_run_9` (`bytes[9]`). Spec §3.1.
    pub(crate) const ZERO_RUN_9: usize = 11;
    /// Offset of `zero_body_marker` (`u8`). Spec §3.1.
    pub(crate) const ZERO_BODY_MARKER: usize = 20;
    /// Stated value of `zero_body_marker` (`u8`). Spec §3.1.
    pub(crate) const ZERO_BODY_MARKER_VALUE: u8 = 1;
    /// Offset of `zero_run_11` (`bytes[11]`). Spec §3.1.
    pub(crate) const ZERO_RUN_11: usize = 21;
    /// Offset of `shared_metadata_marker` (`u8`). Spec §3.1.
    pub(crate) const SHARED_METADATA_MARKER: usize = 32;
    /// Stated value of `shared_metadata_marker` (`u8`). Spec §3.1.
    pub(crate) const SHARED_METADATA_MARKER_VALUE: u8 = 1;
    /// Offset of `shared_metadata_record` (`u64`, little-endian). Spec §3.1.
    pub(crate) const SHARED_METADATA_RECORD: usize = 33;
    /// Offset of `shared_metadata_zero_tail` (`bytes[14]`). Spec §3.1.
    pub(crate) const SHARED_METADATA_ZERO_TAIL: usize = 41;
    /// Offset of `guid_code_unit_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const GUID_CODE_UNIT_COUNT: usize = 55;
    /// Stated value of `guid_code_unit_count` (`u32`). Spec §3.1.
    pub(crate) const GUID_CODE_UNIT_COUNT_VALUE: u32 = 0x0000_0024;
    /// Offset of `guid` (`bytes[72]`). Spec §3.1.
    pub(crate) const GUID: usize = 59;
    /// Offset of `zero_run_3` (`bytes[3]`). Spec §3.1.
    pub(crate) const ZERO_RUN_3: usize = 131;
    /// Offset of `reference_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const REFERENCE_COUNT: usize = 134;
    /// Stated value of `reference_count` (`u32`). Spec §3.1.
    pub(crate) const REFERENCE_COUNT_VALUE: u32 = 0x0000_0001;
    /// Offset of `scope_reference_marker` (`u8`). Spec §3.1.
    pub(crate) const SCOPE_REFERENCE_MARKER: usize = 138;
    /// Stated value of `scope_reference_marker` (`u8`). Spec §3.1.
    pub(crate) const SCOPE_REFERENCE_MARKER_VALUE: u8 = 1;
    /// Offset of `scope_reference_record` (`u32`, little-endian). Spec §3.1.
    pub(crate) const SCOPE_REFERENCE_RECORD: usize = 139;
    /// Offset of `scope_reference_field` (`bytes[6]`). Spec §3.1.
    pub(crate) const SCOPE_REFERENCE_FIELD: usize = 143;
    /// Offset of `history_state_id` (`u32`, little-endian). Spec §3.1.
    pub(crate) const HISTORY_STATE_ID: usize = 149;
    /// Offset of `kind_length` (`u32`, little-endian). Spec §3.1.
    pub(crate) const KIND_LENGTH: usize = 153;
    /// Stated value of `kind_length` (`u32`). Spec §3.1.
    pub(crate) const KIND_LENGTH_VALUE: u32 = 0x0000_000c;
}

/// Byte offsets for the `base_feature_result_body_entry` record.
///
/// Spec §3.1. Record length 15 B.
///
/// ```text
/// This layout repeats for each body entity suffix and each passive body-reference record, with entry bases at `24 + 15i` and `24 + 15N + 15i`.
/// ```
pub(crate) mod base_feature_result_body_entry {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 15;
    /// Offset of `reference_marker` (`u8`). Spec §3.1.
    pub(crate) const REFERENCE_MARKER: usize = 0;
    /// Offset of `reference_value` (`u64`, little-endian). Spec §3.1.
    pub(crate) const REFERENCE_VALUE: usize = 1;
    /// Offset of `reference_field` (`bytes[6]`). Spec §3.1.
    pub(crate) const REFERENCE_FIELD: usize = 9;
}

/// Byte offsets for the `base_feature_compact_result_body_count` record.
///
/// Spec §3.1. Record length 11 B.
///
/// ```text
/// Offsets are relative to the byte after the two parallel 15-byte body-entry runs. The class-420 and class-452 compact forms use repeat marker 1; the class-444 form uses repeat marker 0.
/// ```
pub(crate) mod base_feature_compact_result_body_count {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 11;
    /// Offset of `count_marker` (`u8`). Spec §3.1.
    pub(crate) const COUNT_MARKER: usize = 0;
    /// Offset of `zero_run_5` (`bytes[5]`). Spec §3.1.
    pub(crate) const ZERO_RUN_5: usize = 1;
    /// Offset of `repeat_marker` (`u8`). Spec §3.1.
    pub(crate) const REPEAT_MARKER: usize = 6;
    /// Offset of `body_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const BODY_COUNT: usize = 7;
}

/// Byte offsets for the `base_feature_compact_repeated_body_entry` record.
///
/// Spec §3.1. Record length 11 B.
///
/// ```text
/// This layout repeats for `N` entries immediately after the compact result-body count field.
/// ```
pub(crate) mod base_feature_compact_repeated_body_entry {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 11;
    /// Offset of `body_marker` (`u8`). Spec §3.1.
    pub(crate) const BODY_MARKER: usize = 0;
    /// Offset of `body_entity_suffix` (`u32`, little-endian). Spec §3.1.
    pub(crate) const BODY_ENTITY_SUFFIX: usize = 1;
    /// Offset of `body_field` (`bytes[6]`). Spec §3.1.
    pub(crate) const BODY_FIELD: usize = 5;
}

/// Byte offsets for the `base_feature_compact_metadata_tail` record.
///
/// Spec §3.1. Record length 16 B.
///
/// ```text
/// Offsets are relative to the byte after the compact repeated body-entry run. The result-record run begins at offset 16.
/// ```
pub(crate) mod base_feature_compact_metadata_tail {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 16;
    /// Offset of `separator` (`u8`). Spec §3.1.
    pub(crate) const SEPARATOR: usize = 0;
    /// Offset of `metadata_marker` (`u8`). Spec §3.1.
    pub(crate) const METADATA_MARKER: usize = 1;
    /// Offset of `metadata_record` (`u64`, little-endian). Spec §3.1.
    pub(crate) const METADATA_RECORD: usize = 2;
    /// Offset of `metadata_field` (`bytes[2]`). Spec §3.1.
    pub(crate) const METADATA_FIELD: usize = 10;
    /// Offset of `result_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const RESULT_COUNT: usize = 12;
}

/// Byte offsets for the `base_feature_result_body_result_entry` record.
///
/// Spec §3.1. Record length 11 B.
///
/// ```text
/// This layout repeats for `N` result-record entries after the result count.
/// ```
pub(crate) mod base_feature_result_body_result_entry {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 11;
    /// Offset of `result_marker` (`u8`). Spec §3.1.
    pub(crate) const RESULT_MARKER: usize = 0;
    /// Offset of `result_record` (`u32`, little-endian). Spec §3.1.
    pub(crate) const RESULT_RECORD: usize = 1;
    /// Offset of `result_field` (`bytes[6]`). Spec §3.1.
    pub(crate) const RESULT_FIELD: usize = 5;
}

/// Byte offsets for the `base_feature_body_snapshot_prefix` record.
///
/// Spec §3.1. Record length 24 B.
///
/// ```text
/// Offsets are relative to the primary indexed header. The repeated body-entry run begins at offset 24 and has 15 bytes per body.
/// ```
pub(crate) mod base_feature_body_snapshot_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 24;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_8` (`bytes[8]`). Spec §3.1.
    pub(crate) const ZERO_RUN_8: usize = 11;
    /// Offset of `body_count_marker` (`u8`). Spec §3.1.
    pub(crate) const BODY_COUNT_MARKER: usize = 19;
    /// Offset of `body_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const BODY_COUNT: usize = 20;
}

/// Byte offsets for the `base_feature_body_snapshot_body_entry` record.
///
/// Spec §3.1. Record length 15 B.
///
/// ```text
/// This layout repeats once for each body, with the entry base at primary-scope offset `24 + 15i`.
/// ```
pub(crate) mod base_feature_body_snapshot_body_entry {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 15;
    /// Offset of `body_marker` (`u8`). Spec §3.1.
    pub(crate) const BODY_MARKER: usize = 0;
    /// Offset of `body_entity_suffix` (`u64`, little-endian). Spec §3.1.
    pub(crate) const BODY_ENTITY_SUFFIX: usize = 1;
    /// Offset of `body_entity_field` (`bytes[6]`). Spec §3.1.
    pub(crate) const BODY_ENTITY_FIELD: usize = 9;
}

/// Byte offsets for the `base_feature_body_snapshot_compact_preamble` record.
///
/// Spec §3.1. Record length 8 B.
///
/// ```text
/// The compact body-snapshot preamble occupies the eight bytes immediately after the repeated body-entry run.
/// ```
pub(crate) mod base_feature_body_snapshot_compact_preamble {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 8;
    /// Offset of `preamble` (`bytes[8]`). Spec §3.1.
    pub(crate) const PREAMBLE: usize = 0;
}

/// Byte offsets for the `base_feature_body_snapshot_expanded_preamble` record.
///
/// Spec §3.1. Record length 9 B.
///
/// ```text
/// The expanded body-snapshot preamble occupies the nine bytes immediately after the repeated body-entry run. Its final zero byte is also the first byte of the linkage tail.
/// ```
pub(crate) mod base_feature_body_snapshot_expanded_preamble {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 9;
    /// Offset of `preamble` (`bytes[9]`). Spec §3.1.
    pub(crate) const PREAMBLE: usize = 0;
}

/// Byte offsets for the `base_feature_body_snapshot_linkage_tail` record.
///
/// Spec §3.1. Record length 57 B.
///
/// ```text
/// Offsets are relative to the linkage-tail anchor `A = 184 + 15N`. The third GUID starts at offset 57; in the expanded preamble, the second GUID ends at `A + 1` and shares its final zero byte with the tail at `A`.
/// ```
pub(crate) mod base_feature_body_snapshot_linkage_tail {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 57;
    /// Offset of `zero_run_2` (`bytes[2]`). Spec §3.1.
    pub(crate) const ZERO_RUN_2: usize = 0;
    /// Offset of `envelope_prefix` (`bytes[5]`). Spec §3.1.
    pub(crate) const ENVELOPE_PREFIX: usize = 2;
    /// Offset of `first_body_marker` (`u8`). Spec §3.1.
    pub(crate) const FIRST_BODY_MARKER: usize = 7;
    /// Offset of `first_body_entity_suffix` (`u64`, little-endian). Spec §3.1.
    pub(crate) const FIRST_BODY_ENTITY_SUFFIX: usize = 8;
    /// Offset of `zero_run_3` (`bytes[3]`). Spec §3.1.
    pub(crate) const ZERO_RUN_3: usize = 16;
    /// Offset of `linkage_marker` (`u8`). Spec §3.1.
    pub(crate) const LINKAGE_MARKER: usize = 19;
    /// Offset of `linkage_record` (`u64`, little-endian). Spec §3.1.
    pub(crate) const LINKAGE_RECORD: usize = 20;
    /// Offset of `zero_run_6` (`bytes[6]`). Spec §3.1.
    pub(crate) const ZERO_RUN_6: usize = 28;
    /// Offset of `relation_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const RELATION_COUNT: usize = 34;
    /// Offset of `auxiliary_marker` (`u8`). Spec §3.1.
    pub(crate) const AUXILIARY_MARKER: usize = 38;
    /// Offset of `auxiliary_record` (`u64`, little-endian). Spec §3.1.
    pub(crate) const AUXILIARY_RECORD: usize = 39;
    /// Offset of `trailing_zero_run_6` (`bytes[6]`). Spec §3.1.
    pub(crate) const TRAILING_ZERO_RUN_6: usize = 47;
    /// Offset of `trailing_zero_run_4` (`bytes[4]`). Spec §3.1.
    pub(crate) const TRAILING_ZERO_RUN_4: usize = 53;
}

/// Byte offsets for the `base_feature_body_snapshot_guid` record.
///
/// Spec §3.1. Record length 76 B.
///
/// ```text
/// Each GUID occupies a u32 code-unit count followed by 72 UTF-16LE payload bytes. The layout repeats for the two initial GUIDs and the third GUID after the linkage tail.
/// ```
pub(crate) mod base_feature_body_snapshot_guid {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 76;
    /// Offset of `code_unit_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const CODE_UNIT_COUNT: usize = 0;
    /// Offset of `guid_utf16` (`bytes[72]`). Spec §3.1.
    pub(crate) const GUID_UTF16: usize = 4;
}

/// Byte offsets for the `base_feature_body_snapshot_scope_prefix` record.
///
/// Spec §3.1. Record length 26 B.
///
/// ```text
/// Offsets are relative to `T = A + 133`, after the third GUID. The LP-UTF16 kind payload follows this prefix at offset 26.
/// ```
pub(crate) mod base_feature_body_snapshot_scope_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 26;
    /// Offset of `zero_run_3` (`bytes[3]`). Spec §3.1.
    pub(crate) const ZERO_RUN_3: usize = 0;
    /// Offset of `reference_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const REFERENCE_COUNT: usize = 3;
    /// Offset of `reference_marker` (`u8`). Spec §3.1.
    pub(crate) const REFERENCE_MARKER: usize = 7;
    /// Offset of `reference_member` (`u32`, little-endian). Spec §3.1.
    pub(crate) const REFERENCE_MEMBER: usize = 8;
    /// Offset of `reference_zero_run` (`bytes[6]`). Spec §3.1.
    pub(crate) const REFERENCE_ZERO_RUN: usize = 12;
    /// Offset of `history_state_id` (`u32`, little-endian). Spec §3.1.
    pub(crate) const HISTORY_STATE_ID: usize = 18;
    /// Offset of `kind_code_unit_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const KIND_CODE_UNIT_COUNT: usize = 22;
}

/// Byte offsets for the `split_face_class_418_prefix` record.
///
/// Spec §3.1. Record length 32 B.
///
/// ```text
/// Offsets are relative to the primary SplitFace indexed header. The first marked construction reference follows this prefix at offset 32.
/// ```
pub(crate) mod split_face_class_418_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 32;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_11` (`bytes[11]`). Spec §3.1.
    pub(crate) const ZERO_RUN_11: usize = 11;
    /// Offset of `first_marker` (`u8`). Spec §3.1.
    pub(crate) const FIRST_MARKER: usize = 22;
    /// Offset of `zero_run_4` (`bytes[4]`). Spec §3.1.
    pub(crate) const ZERO_RUN_4: usize = 23;
    /// Offset of `marker_pair` (`bytes[2]`). Spec §3.1.
    pub(crate) const MARKER_PAIR: usize = 27;
    /// Offset of `zero_run_3` (`bytes[3]`). Spec §3.1.
    pub(crate) const ZERO_RUN_3: usize = 29;
}

/// Byte offsets for the `form_legacy_one_cage_owner` record.
///
/// Spec §1.1.1. Record length 81 B.
///
/// ```text
/// Offsets are relative to the primary indexed header. The owner/paired/nested class triples are 335/262/328, 395/264/329, 448/258/276, and 295/258/274.
/// ```
pub(crate) mod form_legacy_one_cage_owner {
    /// Record length in bytes. Spec §1.1.1.
    pub(crate) const LEN: usize = 81;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §1.1.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_14` (`bytes[14]`). Spec §1.1.1.
    pub(crate) const ZERO_RUN_14: usize = 11;
    /// Offset of `owner_marker` (`u8`). Spec §1.1.1.
    pub(crate) const OWNER_MARKER: usize = 25;
    /// Offset of `owner_scope_record_index` (`u64`, little-endian). Spec §1.1.1.
    pub(crate) const OWNER_SCOPE_RECORD_INDEX: usize = 26;
    /// Offset of `zero_run_24` (`bytes[24]`). Spec §1.1.1.
    pub(crate) const ZERO_RUN_24: usize = 34;
    /// Offset of `nested_marker` (`u8`). Spec §1.1.1.
    pub(crate) const NESTED_MARKER: usize = 58;
    /// Offset of `nested_record_index` (`u64`, little-endian). Spec §1.1.1.
    pub(crate) const NESTED_RECORD_INDEX: usize = 59;
    /// Offset of `nested_zero_run` (`bytes[3]`). Spec §1.1.1.
    pub(crate) const NESTED_ZERO_RUN: usize = 67;
    /// Offset of `owner_repeat_marker` (`u8`). Spec §1.1.1.
    pub(crate) const OWNER_REPEAT_MARKER: usize = 70;
    /// Offset of `owner_repeat_scope` (`u64`, little-endian). Spec §1.1.1.
    pub(crate) const OWNER_REPEAT_SCOPE: usize = 71;
    /// Offset of `tail_zero_run` (`bytes[2]`). Spec §1.1.1.
    pub(crate) const TAIL_ZERO_RUN: usize = 79;
}

/// Byte offsets for the `form_compact_one_cage_list` record.
///
/// Spec §1.1.1. Record length 100 B.
///
/// ```text
/// Offsets are relative to the primary indexed header. The class tags are dynamic; the frame length and fixed fields select the compact one-cage form.
/// ```
pub(crate) mod form_compact_one_cage_list {
    /// Record length in bytes. Spec §1.1.1.
    pub(crate) const LEN: usize = 100;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §1.1.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_10` (`bytes[10]`). Spec §1.1.1.
    pub(crate) const ZERO_RUN_10: usize = 11;
    /// Offset of `owner_marker` (`u8`). Spec §1.1.1.
    pub(crate) const OWNER_MARKER: usize = 21;
    /// Offset of `owner_scope_record_index` (`u64`, little-endian). Spec §1.1.1.
    pub(crate) const OWNER_SCOPE_RECORD_INDEX: usize = 22;
    /// Offset of `zero_run_2` (`bytes[2]`). Spec §1.1.1.
    pub(crate) const ZERO_RUN_2: usize = 30;
    /// Offset of `cage_count` (`u32`, little-endian). Spec §1.1.1.
    pub(crate) const CAGE_COUNT: usize = 32;
    /// Offset of `member_marker` (`u8`). Spec §1.1.1.
    pub(crate) const MEMBER_MARKER: usize = 36;
    /// Offset of `cage_object_record_index` (`u64`, little-endian). Spec §1.1.1.
    pub(crate) const CAGE_OBJECT_RECORD_INDEX: usize = 37;
    /// Offset of `member_zero` (`u16`, little-endian). Spec §1.1.1.
    pub(crate) const MEMBER_ZERO: usize = 45;
    /// Offset of `member_flags` (`u16`, little-endian). Spec §1.1.1.
    pub(crate) const MEMBER_FLAGS: usize = 47;
}

/// Byte offsets for the `form_class_325_cage_table` record.
///
/// Spec §1.1.1. Record length 1850 B.
///
/// ```text
/// Offsets are relative to the class-325 Form primary indexed header. The 32-entry run starts at offset 41; each entry is 30 bytes. The class-325 frame length is 890 plus 30 times its cage count.
/// ```
pub(crate) mod form_class_325_cage_table {
    /// Record length in bytes. Spec §1.1.1.
    pub(crate) const LEN: usize = 1850;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §1.1.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_9` (`bytes[9]`). Spec §1.1.1.
    pub(crate) const ZERO_RUN_9: usize = 11;
    /// Offset of `list_marker` (`u8`). Spec §1.1.1.
    pub(crate) const LIST_MARKER: usize = 20;
    /// Stated value of `list_marker` (`u8`). Spec §1.1.1.
    pub(crate) const LIST_MARKER_VALUE: u8 = 1;
    /// Offset of `zero_run_5` (`bytes[5]`). Spec §1.1.1.
    pub(crate) const ZERO_RUN_5: usize = 21;
    /// Offset of `owner_marker` (`u8`). Spec §1.1.1.
    pub(crate) const OWNER_MARKER: usize = 26;
    /// Stated value of `owner_marker` (`u8`). Spec §1.1.1.
    pub(crate) const OWNER_MARKER_VALUE: u8 = 1;
    /// Offset of `owner_result_record_index` (`u64`, little-endian). Spec §1.1.1.
    pub(crate) const OWNER_RESULT_RECORD_INDEX: usize = 27;
    /// Offset of `zero_run_2` (`bytes[2]`). Spec §1.1.1.
    pub(crate) const ZERO_RUN_2: usize = 35;
    /// Offset of `cage_count` (`u32`, little-endian). Spec §1.1.1.
    pub(crate) const CAGE_COUNT: usize = 37;
    /// Stated value of `cage_count` (`u32`). Spec §1.1.1.
    pub(crate) const CAGE_COUNT_VALUE: u32 = 0x0000_0020;
    /// Offset of `cage_entries` (`bytes[960]`). Spec §1.1.1.
    pub(crate) const CAGE_ENTRIES: usize = 41;
}

/// Byte offsets for the `form_class_325_cage_entry` record.
///
/// Spec §1.1.1. Record length 30 B.
///
/// ```text
/// Offsets are relative to one entry's base; the entry repeats every 30 bytes from class-325 offset 41.
/// ```
pub(crate) mod form_class_325_cage_entry {
    /// Record length in bytes. Spec §1.1.1.
    pub(crate) const LEN: usize = 30;
    /// Offset of `cage_object_marker` (`u8`). Spec §1.1.1.
    pub(crate) const CAGE_OBJECT_MARKER: usize = 0;
    /// Stated value of `cage_object_marker` (`u8`). Spec §1.1.1.
    pub(crate) const CAGE_OBJECT_MARKER_VALUE: u8 = 1;
    /// Offset of `cage_object_record_index` (`u64`, little-endian). Spec §1.1.1.
    pub(crate) const CAGE_OBJECT_RECORD_INDEX: usize = 1;
    /// Offset of `cage_object_zero` (`u16`, little-endian). Spec §1.1.1.
    pub(crate) const CAGE_OBJECT_ZERO: usize = 9;
    /// Stated value of `cage_object_zero` (`u16`). Spec §1.1.1.
    pub(crate) const CAGE_OBJECT_ZERO_VALUE: u16 = 0x0000;
    /// Offset of `type_discriminator` (`u64`, little-endian). Spec §1.1.1.
    pub(crate) const TYPE_DISCRIMINATOR: usize = 11;
    /// Offset of `companion_marker` (`u8`). Spec §1.1.1.
    pub(crate) const COMPANION_MARKER: usize = 19;
    /// Stated value of `companion_marker` (`u8`). Spec §1.1.1.
    pub(crate) const COMPANION_MARKER_VALUE: u8 = 1;
    /// Offset of `companion_record_index` (`u64`, little-endian). Spec §1.1.1.
    pub(crate) const COMPANION_RECORD_INDEX: usize = 20;
    /// Offset of `companion_zero` (`u16`, little-endian). Spec §1.1.1.
    pub(crate) const COMPANION_ZERO: usize = 28;
    /// Stated value of `companion_zero` (`u16`). Spec §1.1.1.
    pub(crate) const COMPANION_ZERO_VALUE: u16 = 0x0000;
}

/// Byte offsets for the `form_serializer_frame_132` record.
///
/// Spec §1.1.1. Record length 132 B.
///
/// ```text
/// Offsets are relative to the serializer's primary indexed header. The LP-UTF16 entry-name span and the marked surface reference are variable-length fields within the fixed frame.
/// ```
pub(crate) mod form_serializer_frame_132 {
    /// Record length in bytes. Spec §1.1.1.
    pub(crate) const LEN: usize = 132;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §1.1.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_10` (`bytes[10]`). Spec §1.1.1.
    pub(crate) const ZERO_RUN_10: usize = 11;
    /// Offset of `entry_name_length` (`u32`, little-endian). Spec §1.1.1.
    pub(crate) const ENTRY_NAME_LENGTH: usize = 21;
}

/// Byte offsets for the `extrude_selection_member_fixed_frame` record.
///
/// Spec §3.1. Record length 190 B.
///
/// ```text
/// Offsets are relative to the member's indexed header. The UUID payloads occupy 36 UTF-16 code units each; the following indexed header is absent only at the stream-end boundary.
/// ```
pub(crate) mod extrude_selection_member_fixed_frame {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 190;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_10` (`bytes[10]`). Spec §3.1.
    pub(crate) const ZERO_RUN_10: usize = 11;
    /// Offset of `local_identity` (`u64`, little-endian). Spec §3.1.
    pub(crate) const LOCAL_IDENTITY: usize = 21;
    /// Offset of `asset_uuid_length` (`u32`, little-endian). Spec §3.1.
    pub(crate) const ASSET_UUID_LENGTH: usize = 29;
    /// Offset of `asset_uuid_utf16` (`bytes[72]`). Spec §3.1.
    pub(crate) const ASSET_UUID_UTF16: usize = 33;
    /// Offset of `context_uuid_length` (`u32`, little-endian). Spec §3.1.
    pub(crate) const CONTEXT_UUID_LENGTH: usize = 105;
    /// Offset of `context_uuid_utf16` (`bytes[72]`). Spec §3.1.
    pub(crate) const CONTEXT_UUID_UTF16: usize = 109;
    /// Offset of `tail_kind` (`u32`, little-endian). Spec §3.1.
    pub(crate) const TAIL_KIND: usize = 181;
    /// Offset of `tail_slot_marker` (`u8`). Spec §3.1.
    pub(crate) const TAIL_SLOT_MARKER: usize = 185;
    /// Offset of `tail_slot_value` (`u32`, little-endian). Spec §3.1.
    pub(crate) const TAIL_SLOT_VALUE: usize = 186;
}

/// Byte offsets for the `coil_compact_scope_discriminators` record.
///
/// Spec §3.1. Record length 111 B.
///
/// ```text
/// Offsets are relative to the primary indexed scope header. The ordered reference table and scope trailer follow this fixed discriminator block.
/// ```
pub(crate) mod coil_compact_scope_discriminators {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 111;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `operation` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OPERATION: usize = 20;
    /// Offset of `clockwise` (`u8`). Spec §3.1.
    pub(crate) const CLOCKWISE: usize = 24;
    /// Offset of `structural_constant` (`u32`, little-endian). Spec §3.1.
    pub(crate) const STRUCTURAL_CONSTANT: usize = 26;
    /// Offset of `extent` (`u32`, little-endian). Spec §3.1.
    pub(crate) const EXTENT: usize = 30;
    /// Offset of `section_placement` (`u32`, little-endian). Spec §3.1.
    pub(crate) const SECTION_PLACEMENT: usize = 92;
    /// Offset of `section_shape` (`u32`, little-endian). Spec §3.1.
    pub(crate) const SECTION_SHAPE: usize = 107;
}

/// Byte offsets for the `coil_long_scope_fixed_prologue` record.
///
/// Spec §3.1. Record length 52 B.
///
/// ```text
/// Offsets are relative to the primary indexed scope header. The two marked references repeat ordered-reference ordinals four and eight; their target records are dynamic indexed records.
/// ```
pub(crate) mod coil_long_scope_fixed_prologue {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 52;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_11` (`bytes[11]`). Spec §3.1.
    pub(crate) const ZERO_RUN_11: usize = 11;
    /// Offset of `operation` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OPERATION: usize = 22;
    /// Offset of `structural_constant` (`u32`, little-endian). Spec §3.1.
    pub(crate) const STRUCTURAL_CONSTANT: usize = 26;
    /// Offset of `fifth_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const FIFTH_REFERENCE: usize = 30;
    /// Offset of `ninth_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const NINTH_REFERENCE: usize = 41;
}

/// Byte offsets for the `coil_long_scope_matrix` record.
///
/// Spec §3.1. Record length 128 B.
///
/// ```text
/// The block begins at primary indexed scope offset 77. Its final row is `(0, 0, 0, 1)`; the 572-byte form carries Boolean operations and the 578-byte form carries new body.
/// ```
pub(crate) mod coil_long_scope_matrix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 128;
    /// Offset of `matrix` (`f64[16]`, little-endian). Spec §3.1.
    pub(crate) const MATRIX: usize = 0;
}

/// Byte offsets for the `coil_compact_persistent_selection_prefix` record.
///
/// Spec §3.1. Record length 40 B.
///
/// ```text
/// Offsets are relative to the first placement selection header. The asset and context UUID payloads follow the fixed UTF-16 length fields and therefore have variable length.
/// ```
pub(crate) mod coil_compact_persistent_selection_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 40;
    /// Offset of `nested_selection_marker` (`u8`). Spec §3.1.
    pub(crate) const NESTED_SELECTION_MARKER: usize = 21;
    /// Offset of `nested_record_index` (`u32`, little-endian). Spec §3.1.
    pub(crate) const NESTED_RECORD_INDEX: usize = 22;
    /// Offset of `asset_presence` (`u32`, little-endian). Spec §3.1.
    pub(crate) const ASSET_PRESENCE: usize = 32;
    /// Offset of `asset_uuid_length` (`u32`, little-endian). Spec §3.1.
    pub(crate) const ASSET_UUID_LENGTH: usize = 36;
}

/// Byte offsets for the `work_point_sketch_point_identity` record.
///
/// Spec §3.1. Record length 41 B.
///
/// ```text
/// Offsets are relative to the identity record's indexed header. The following indexed record begins at offset 41.
/// ```
pub(crate) mod work_point_sketch_point_identity {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 41;
    /// Offset of `presence` (`u8`). Spec §3.1.
    pub(crate) const PRESENCE: usize = 20;
    /// Offset of `sketch_record_index` (`u32`, little-endian). Spec §3.1.
    pub(crate) const SKETCH_RECORD_INDEX: usize = 25;
    /// Offset of `point_persistent_id` (`u32`, little-endian). Spec §3.1.
    pub(crate) const POINT_PERSISTENT_ID: usize = 33;
}

/// Byte offsets for the `class_338_sketch_curve_identity` record.
///
/// Spec §3.1. Record length 49 B.
///
/// ```text
/// Offsets are relative to the class-`361` identity record's indexed header. The following indexed record begins at offset 49.
/// ```
pub(crate) mod class_338_sketch_curve_identity {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 49;
    /// Offset of `zero_prefix` (`bytes[9]`). Spec §3.1.
    pub(crate) const ZERO_PREFIX: usize = 11;
    /// Offset of `presence` (`u8`). Spec §3.1.
    pub(crate) const PRESENCE: usize = 20;
    /// Offset of `owner_record_index` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OWNER_RECORD_INDEX: usize = 33;
    /// Offset of `owner_high_zero` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OWNER_HIGH_ZERO: usize = 37;
    /// Offset of `curve_persistent_id` (`u32`, little-endian). Spec §3.1.
    pub(crate) const CURVE_PERSISTENT_ID: usize = 41;
    /// Offset of `curve_high_zero` (`u32`, little-endian). Spec §3.1.
    pub(crate) const CURVE_HIGH_ZERO: usize = 45;
}

/// Byte offsets for the `coil_modern_selection_prefix` record.
///
/// Spec §3.1. Record length 41 B.
///
/// ```text
/// Offsets are relative to the class-286 first placement selection header. The asset and context UUID payloads follow the fixed UTF-16 length fields and therefore have variable length.
/// ```
pub(crate) mod coil_modern_selection_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 41;
    /// Offset of `nested_selection_marker` (`u8`). Spec §3.1.
    pub(crate) const NESTED_SELECTION_MARKER: usize = 22;
    /// Offset of `nested_record_index` (`u32`, little-endian). Spec §3.1.
    pub(crate) const NESTED_RECORD_INDEX: usize = 23;
    /// Offset of `asset_presence` (`u32`, little-endian). Spec §3.1.
    pub(crate) const ASSET_PRESENCE: usize = 33;
    /// Offset of `asset_uuid_length` (`u32`, little-endian). Spec §3.1.
    pub(crate) const ASSET_UUID_LENGTH: usize = 37;
}

/// Byte offsets for the `coil_compact_face_selection_prefix` record.
///
/// Spec §3.1. Record length 42 B.
///
/// ```text
/// Offsets are relative to the first placement selection header. The asset and context UUID payloads follow the fixed UTF-16 length fields and therefore have variable length.
/// ```
pub(crate) mod coil_compact_face_selection_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 42;
    /// Offset of `nested_selection_marker` (`u8`). Spec §3.1.
    pub(crate) const NESTED_SELECTION_MARKER: usize = 23;
    /// Offset of `nested_record_index` (`u32`, little-endian). Spec §3.1.
    pub(crate) const NESTED_RECORD_INDEX: usize = 24;
    /// Offset of `asset_presence` (`u8`). Spec §3.1.
    pub(crate) const ASSET_PRESENCE: usize = 34;
    /// Offset of `asset_uuid_length` (`u32`, little-endian). Spec §3.1.
    pub(crate) const ASSET_UUID_LENGTH: usize = 38;
}

/// Byte offsets for the `coil_legacy_placement_identity_frame` record.
///
/// Spec §3.1. Record length 186 B.
///
/// ```text
/// Offsets are relative to the class-395 second placement carrier's primary indexed header. The carrier is the identity form of the legacy eight-reference Coil scope.
/// ```
pub(crate) mod coil_legacy_placement_identity_frame {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 186;
    /// Offset of `leading_reference_marker` (`u8`). Spec §3.1.
    pub(crate) const LEADING_REFERENCE_MARKER: usize = 48;
    /// Offset of `leading_reference_index` (`u32`, little-endian). Spec §3.1.
    pub(crate) const LEADING_REFERENCE_INDEX: usize = 49;
    /// Offset of `prologue_value` (`u32`, little-endian). Spec §3.1.
    pub(crate) const PROLOGUE_VALUE: usize = 76;
    /// Offset of `prologue_flag` (`u32`, little-endian). Spec §3.1.
    pub(crate) const PROLOGUE_FLAG: usize = 84;
    /// Offset of `selection_reference_marker` (`u8`). Spec §3.1.
    pub(crate) const SELECTION_REFERENCE_MARKER: usize = 88;
    /// Offset of `selection_record_index` (`u32`, little-endian). Spec §3.1.
    pub(crate) const SELECTION_RECORD_INDEX: usize = 89;
    /// Offset of `selection_flag` (`u32`, little-endian). Spec §3.1.
    pub(crate) const SELECTION_FLAG: usize = 101;
    /// Offset of `auxiliary_reference_marker` (`u8`). Spec §3.1.
    pub(crate) const AUXILIARY_REFERENCE_MARKER: usize = 105;
    /// Offset of `auxiliary_record_index` (`u32`, little-endian). Spec §3.1.
    pub(crate) const AUXILIARY_RECORD_INDEX: usize = 106;
    /// Offset of `tail_value` (`u32`, little-endian). Spec §3.1.
    pub(crate) const TAIL_VALUE: usize = 120;
    /// Offset of `intermediate_selector` (`u32`, little-endian). Spec §3.1.
    pub(crate) const INTERMEDIATE_SELECTOR: usize = 134;
    /// Offset of `carrier_scalar` (`f64`, little-endian). Spec §3.1.
    pub(crate) const CARRIER_SCALAR: usize = 138;
    /// Offset of `tail_selector` (`u32`, little-endian). Spec §3.1.
    pub(crate) const TAIL_SELECTOR: usize = 146;
    /// Offset of `successor_reference_marker` (`u8`). Spec §3.1.
    pub(crate) const SUCCESSOR_REFERENCE_MARKER: usize = 150;
    /// Offset of `successor_record_index` (`u32`, little-endian). Spec §3.1.
    pub(crate) const SUCCESSOR_RECORD_INDEX: usize = 151;
    /// Offset of `predecessor_reference_marker` (`u8`). Spec §3.1.
    pub(crate) const PREDECESSOR_REFERENCE_MARKER: usize = 163;
    /// Offset of `predecessor_record_index` (`u32`, little-endian). Spec §3.1.
    pub(crate) const PREDECESSOR_RECORD_INDEX: usize = 164;
    /// Offset of `owner_reference_marker` (`u8`). Spec §3.1.
    pub(crate) const OWNER_REFERENCE_MARKER: usize = 175;
    /// Offset of `owner_scope_record_index` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OWNER_SCOPE_RECORD_INDEX: usize = 176;
}

/// Byte offsets for the `coil_compact_placement_identity_frame` record.
///
/// Spec §3.1. Record length 213 B.
///
/// ```text
/// Offsets are relative to the second ordered placement carrier's indexed header. The identity form omits the matrix block.
/// ```
pub(crate) mod coil_compact_placement_identity_frame {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 213;
    /// Offset of `placement_marker` (`u8`). Spec §3.1.
    pub(crate) const PLACEMENT_MARKER: usize = 55;
    /// Offset of `identity_zero_run` (`bytes[9]`). Spec §3.1.
    pub(crate) const IDENTITY_ZERO_RUN: usize = 56;
    /// Offset of `identity_marker` (`u8`). Spec §3.1.
    pub(crate) const IDENTITY_MARKER: usize = 65;
}

/// Byte offsets for the `coil_modern_placement_matrix_frame` record.
///
/// Spec §3.1. Record length 315 B.
///
/// ```text
/// Offsets are relative to the class-450 second placement carrier's primary indexed header. The matrix is row-major and its translation values are in centimetres.
/// ```
pub(crate) mod coil_modern_placement_matrix_frame {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 315;
    /// Offset of `matrix` (`f64[16]`, little-endian). Spec §3.1.
    pub(crate) const MATRIX: usize = 50;
    /// Offset of `constant_512` (`u32`, little-endian). Spec §3.1.
    pub(crate) const CONSTANT_512: usize = 204;
    /// Offset of `constant_256` (`u32`, little-endian). Spec §3.1.
    pub(crate) const CONSTANT_256: usize = 212;
    /// Offset of `selection_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const SELECTION_REFERENCE: usize = 217;
    /// Offset of `selection_flag` (`u32`, little-endian). Spec §3.1.
    pub(crate) const SELECTION_FLAG: usize = 230;
    /// Offset of `auxiliary_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const AUXILIARY_REFERENCE: usize = 234;
    /// Offset of `constant_1024` (`u64`, little-endian). Spec §3.1.
    pub(crate) const CONSTANT_1024: usize = 248;
    /// Offset of `identity_lane_prefix` (`u64`, little-endian). Spec §3.1.
    pub(crate) const IDENTITY_LANE_PREFIX: usize = 256;
    /// Offset of `identity_lane` (`u64`, little-endian). Spec §3.1.
    pub(crate) const IDENTITY_LANE: usize = 268;
    /// Offset of `successor_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const SUCCESSOR_REFERENCE: usize = 279;
    /// Offset of `predecessor_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const PREDECESSOR_REFERENCE: usize = 292;
    /// Offset of `owner_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const OWNER_REFERENCE: usize = 304;
}

/// Byte offsets for the `coil_compact_placement_owner_identity_frame` record.
///
/// Spec §3.1. Record length 233 B.
///
/// ```text
/// Offsets are relative to the second ordered placement carrier's indexed header. The owner reference closes the carrier to its containing Coil scope.
/// ```
pub(crate) mod coil_compact_placement_owner_identity_frame {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 233;
    /// Offset of `owner_reference_marker` (`u8`). Spec §3.1.
    pub(crate) const OWNER_REFERENCE_MARKER: usize = 222;
    /// Offset of `owner_scope_record_index` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OWNER_SCOPE_RECORD_INDEX: usize = 223;
    /// Offset of `owner_reference_tail` (`bytes[6]`). Spec §3.1.
    pub(crate) const OWNER_REFERENCE_TAIL: usize = 227;
}

/// Byte offsets for the `work_plane_legacy_class_290_matrix_frame` record.
///
/// Spec §3.1. Record length 325 B.
///
/// ```text
/// Offsets are relative to the class-290 primary indexed placement header paired with class 262. The four-byte marker distinguishes this prefix from the zero-prefix 325-byte placement forms.
/// ```
pub(crate) mod work_plane_legacy_class_290_matrix_frame {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 325;
    /// Offset of `prefix_marker` (`bytes[4]`). Spec §3.1.
    pub(crate) const PREFIX_MARKER: usize = 45;
    /// Offset of `matrix` (`f64[16]`, little-endian). Spec §3.1.
    pub(crate) const MATRIX: usize = 49;
}

/// Byte offsets for the `work_plane_legacy_325_matrix_frame` record.
///
/// Spec §3.1. Record length 325 B.
///
/// ```text
/// Offsets are relative to any of the class-308, class-320, class-364, class-380, or class-431 primary indexed placement headers. Their paired classes are 257, 258, 263, 262, and 257 respectively.
/// ```
pub(crate) mod work_plane_legacy_325_matrix_frame {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 325;
    /// Offset of `matrix` (`f64[16]`, little-endian). Spec §3.1.
    pub(crate) const MATRIX: usize = 49;
}

/// Byte offsets for the `work_plane_legacy_class_256_matrix_frame` record.
///
/// Spec §3.1. Record length 325 B.
///
/// ```text
/// Offsets are relative to the class-256 primary indexed placement header paired with class 262. The two-byte lane before the final zero pair is opaque.
/// ```
pub(crate) mod work_plane_legacy_class_256_matrix_frame {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 325;
    /// Offset of `opaque_u16` (`u16`, little-endian). Spec §3.1.
    pub(crate) const OPAQUE_U16: usize = 45;
    /// Offset of `zero_pair` (`bytes[2]`). Spec §3.1.
    pub(crate) const ZERO_PAIR: usize = 47;
    /// Offset of `matrix` (`f64[16]`, little-endian). Spec §3.1.
    pub(crate) const MATRIX: usize = 49;
}

/// Byte offsets for the `work_plane_legacy_class_337_325_matrix_frame` record.
///
/// Spec §3.1. Record length 325 B.
///
/// ```text
/// Offsets are relative to the class-337 primary indexed placement header paired with class 266. The two-byte lane before the final zero pair is opaque.
/// ```
pub(crate) mod work_plane_legacy_class_337_325_matrix_frame {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 325;
    /// Offset of `opaque_u16` (`u16`, little-endian). Spec §3.1.
    pub(crate) const OPAQUE_U16: usize = 45;
    /// Offset of `zero_pair` (`bytes[2]`). Spec §3.1.
    pub(crate) const ZERO_PAIR: usize = 47;
    /// Offset of `matrix` (`f64[16]`, little-endian). Spec §3.1.
    pub(crate) const MATRIX: usize = 49;
}

/// Byte offsets for the `work_plane_legacy_321_opaque_matrix_frame` record.
///
/// Spec §3.1. Record length 321 B.
///
/// ```text
/// Offsets are relative to the class-341 primary indexed placement header paired with class 261, or the class-346 primary indexed placement header paired with class 262. The two-byte lane before the final zero pair is opaque.
/// ```
pub(crate) mod work_plane_legacy_321_opaque_matrix_frame {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 321;
    /// Offset of `opaque_u16` (`u16`, little-endian). Spec §3.1.
    pub(crate) const OPAQUE_U16: usize = 45;
    /// Offset of `zero_pair` (`bytes[2]`). Spec §3.1.
    pub(crate) const ZERO_PAIR: usize = 47;
    /// Offset of `matrix` (`f64[16]`, little-endian). Spec §3.1.
    pub(crate) const MATRIX: usize = 49;
}

/// Byte offsets for the `work_plane_legacy_337_matrix_frame` record.
///
/// Spec §3.1. Record length 337 B.
///
/// ```text
/// Offsets are relative to either the class-350 or class-409 primary indexed placement header paired with class 258. The placement carriers share a 39-byte zero prefix and matrix offset.
/// ```
pub(crate) mod work_plane_legacy_337_matrix_frame {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 337;
    /// Offset of `matrix` (`f64[16]`, little-endian). Spec §3.1.
    pub(crate) const MATRIX: usize = 50;
}

/// Byte offsets for the `work_plane_legacy_class_400_matrix_frame` record.
///
/// Spec §3.1. Record length 345 B.
///
/// ```text
/// Offsets are relative to the first ordered placement member's class-400 indexed header. The class-400 tail retains the construction references after the solved matrix.
/// ```
pub(crate) mod work_plane_legacy_class_400_matrix_frame {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 345;
    /// Offset of `matrix` (`f64[16]`, little-endian). Spec §3.1.
    pub(crate) const MATRIX: usize = 49;
}

/// Byte offsets for the `work_axis_direct_carrier_class_297` record.
///
/// Spec §3.1. Record length 215 B.
///
/// ```text
/// Offsets are relative to the class-297 primary indexed axis carrier paired with class 262.
/// ```
pub(crate) mod work_axis_direct_carrier_class_297 {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 215;
    /// Offset of `value_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const VALUE_COUNT: usize = 21;
    /// Offset of `axis_values` (`f64[8]`, little-endian). Spec §3.1.
    pub(crate) const AXIS_VALUES: usize = 25;
    /// Offset of `reference_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const REFERENCE_COUNT: usize = 89;
    /// Offset of `reference_preamble` (`u32`, little-endian). Spec §3.1.
    pub(crate) const REFERENCE_PREAMBLE: usize = 93;
}

/// Byte offsets for the `work_axis_direct_carrier_class_335` record.
///
/// Spec §3.1. Record length 195 B.
///
/// ```text
/// Offsets are relative to the class-335 primary indexed axis carrier paired with class 258.
/// ```
pub(crate) mod work_axis_direct_carrier_class_335 {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 195;
    /// Offset of `value_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const VALUE_COUNT: usize = 21;
    /// Offset of `axis_values` (`f64[8]`, little-endian). Spec §3.1.
    pub(crate) const AXIS_VALUES: usize = 25;
    /// Offset of `reference_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const REFERENCE_COUNT: usize = 89;
    /// Offset of `reference_preamble` (`u32`, little-endian). Spec §3.1.
    pub(crate) const REFERENCE_PREAMBLE: usize = 93;
}

/// Byte offsets for the `coil_compact_placement_matrix_frame` record.
///
/// Spec §3.1. Record length 341 B.
///
/// ```text
/// Offsets are relative to the second ordered placement carrier's indexed header. The matrix is row-major and its translation values are in centimetres.
/// ```
pub(crate) mod coil_compact_placement_matrix_frame {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 341;
    /// Offset of `placement_marker` (`u8`). Spec §3.1.
    pub(crate) const PLACEMENT_MARKER: usize = 55;
    /// Offset of `explicit_zero_run` (`bytes[9]`). Spec §3.1.
    pub(crate) const EXPLICIT_ZERO_RUN: usize = 56;
    /// Offset of `explicit_form_marker` (`u8`). Spec §3.1.
    pub(crate) const EXPLICIT_FORM_MARKER: usize = 65;
    /// Offset of `matrix` (`f64[16]`, little-endian). Spec §3.1.
    pub(crate) const MATRIX: usize = 66;
}

/// Byte offsets for the `class_403_revolve_scope_frame` record.
///
/// Spec §3.1. Record length 387 B.
///
/// ```text
/// Offsets are relative to the primary indexed header. The frame has eight ordered references; its fixed operation prefix and angle-owner reference are typed, and the remaining marked references retain their source envelope.
/// ```
pub(crate) mod class_403_revolve_scope_frame {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 387;
    /// Offset of `operation` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OPERATION: usize = 21;
    /// Offset of `extent_kind` (`u32`, little-endian). Spec §3.1.
    pub(crate) const EXTENT_KIND: usize = 25;
    /// Offset of `direction_kind` (`u8`). Spec §3.1.
    pub(crate) const DIRECTION_KIND: usize = 29;
    /// Offset of `envelope_marker` (`u8`). Spec §3.1.
    pub(crate) const ENVELOPE_MARKER: usize = 30;
    /// Offset of `angle_reference_marker` (`u8`). Spec §3.1.
    pub(crate) const ANGLE_REFERENCE_MARKER: usize = 34;
    /// Offset of `angle_record_index` (`u32`, little-endian). Spec §3.1.
    pub(crate) const ANGLE_RECORD_INDEX: usize = 35;
    /// Offset of `guid_code_unit_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const GUID_CODE_UNIT_COUNT: usize = 107;
    /// Offset of `guid_utf16` (`bytes[72]`). Spec §3.1.
    pub(crate) const GUID_UTF16: usize = 111;
    /// Offset of `reference_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const REFERENCE_COUNT: usize = 186;
    /// Offset of `history_state_id` (`u32`, little-endian). Spec §3.1.
    pub(crate) const HISTORY_STATE_ID: usize = 278;
    /// Offset of `kind_code_unit_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const KIND_CODE_UNIT_COUNT: usize = 282;
    /// Offset of `kind_utf16` (`bytes[14]`). Spec §3.1.
    pub(crate) const KIND_UTF16: usize = 286;
    /// Offset of `feature_ordinal` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FEATURE_ORDINAL: usize = 300;
    /// Offset of `previous_history_state_id` (`u32`, little-endian). Spec §3.1.
    pub(crate) const PREVIOUS_HISTORY_STATE_ID: usize = 341;
}

/// Byte offsets for the `marker_one_revolve_prologue` record.
///
/// Spec §3.1. Record length 38 B.
///
/// ```text
/// Offsets are relative to the Revolve primary indexed header. Every marker-one class pair uses the same fixed prologue.
/// ```
pub(crate) mod marker_one_revolve_prologue {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 38;
    /// Offset of `marker` (`u8`). Spec §3.1.
    pub(crate) const MARKER: usize = 20;
    /// Offset of `zero_value` (`u32`, little-endian). Spec §3.1.
    pub(crate) const ZERO_VALUE: usize = 21;
    /// Offset of `operation` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OPERATION: usize = 25;
    /// Offset of `extent_kind` (`u32`, little-endian). Spec §3.1.
    pub(crate) const EXTENT_KIND: usize = 29;
    /// Offset of `direction_kind` (`u8`). Spec §3.1.
    pub(crate) const DIRECTION_KIND: usize = 33;
    /// Offset of `structural_constant` (`u32`, little-endian). Spec §3.1.
    pub(crate) const STRUCTURAL_CONSTANT: usize = 34;
}

/// Byte offsets for the `current_extrude_operation_fields` record.
///
/// Spec §3.1. Record length 42 B.
///
/// ```text
/// Offsets are relative to the result-operation u32 of a current reference-aware Extrude scope. The seven variable-width nullable reference slots follow at offset 42.
/// ```
pub(crate) mod current_extrude_operation_fields {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 42;
    /// Offset of `operation` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OPERATION: usize = 0;
    /// Offset of `direction` (`u32`, little-endian). Spec §3.1.
    pub(crate) const DIRECTION: usize = 4;
    /// Offset of `face_extend` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FACE_EXTEND: usize = 8;
    /// Offset of `direction_reversed` (`u8`). Spec §3.1.
    pub(crate) const DIRECTION_REVERSED: usize = 12;
    /// Offset of `geometry_kind` (`u8`). Spec §3.1.
    pub(crate) const GEOMETRY_KIND: usize = 13;
    /// Offset of `start_support` (`u8`). Spec §3.1.
    pub(crate) const START_SUPPORT: usize = 14;
    /// Offset of `zero_run_3` (`bytes[3]`). Spec §3.1.
    pub(crate) const ZERO_RUN_3: usize = 15;
    /// Offset of `profile_normal` (`f64[3]`, little-endian). Spec §3.1.
    pub(crate) const PROFILE_NORMAL: usize = 18;
}

/// Byte offsets for the `current_extrude_non_target_extent_pair` record.
///
/// Spec §3.1. Record length 17 B.
///
/// ```text
/// Offsets are relative to the first-side extent u32. This frame applies when the first-side extent is not the to-entity value 2.
/// ```
pub(crate) mod current_extrude_non_target_extent_pair {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 17;
    /// Offset of `first_side_extent` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FIRST_SIDE_EXTENT: usize = 0;
    /// Offset of `zero_run_9` (`bytes[9]`). Spec §3.1.
    pub(crate) const ZERO_RUN_9: usize = 4;
    /// Offset of `second_side_extent` (`u32`, little-endian). Spec §3.1.
    pub(crate) const SECOND_SIDE_EXTENT: usize = 13;
}

/// Byte offsets for the `current_extrude_shape_target_extent_prefix` record.
///
/// Spec §3.1. Record length 9 B.
///
/// ```text
/// Offsets are relative to the repeated target-group ordinal. The target payload follows the first-side extent; the second-side extent is four bytes before the scope reference-count field.
/// ```
pub(crate) mod current_extrude_shape_target_extent_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 9;
    /// Offset of `target_scope_reference_ordinal` (`u32`, little-endian). Spec §3.1.
    pub(crate) const TARGET_SCOPE_REFERENCE_ORDINAL: usize = 0;
    /// Offset of `zero_separator` (`u8`). Spec §3.1.
    pub(crate) const ZERO_SEPARATOR: usize = 4;
    /// Offset of `first_side_extent` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FIRST_SIDE_EXTENT: usize = 5;
}

/// Byte offsets for the `early_distance_extrude_absent_prefix` record.
///
/// Spec §3.1. Record length 34 B.
///
/// ```text
/// Offsets are relative to the early distance-only Extrude primary indexed header. The scope reference-count field is at offset 208.
/// ```
pub(crate) mod early_distance_extrude_absent_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 34;
    /// Offset of `absent_prefix` (`u8`). Spec §3.1.
    pub(crate) const ABSENT_PREFIX: usize = 20;
    /// Offset of `operation` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OPERATION: usize = 21;
    /// Offset of `extent_kind` (`u32`, little-endian). Spec §3.1.
    pub(crate) const EXTENT_KIND: usize = 25;
    /// Offset of `direction_reversed` (`u8`). Spec §3.1.
    pub(crate) const DIRECTION_REVERSED: usize = 29;
    /// Offset of `geometry_kind` (`u32`, little-endian). Spec §3.1.
    pub(crate) const GEOMETRY_KIND: usize = 30;
}

/// Byte offsets for the `early_distance_extrude_present_prefix` record.
///
/// Spec §3.1. Record length 38 B.
///
/// ```text
/// Offsets are relative to the early distance-only Extrude primary indexed header. The scope reference-count field is at offset 212.
/// ```
pub(crate) mod early_distance_extrude_present_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 38;
    /// Offset of `present_prefix_marker` (`u8`). Spec §3.1.
    pub(crate) const PRESENT_PREFIX_MARKER: usize = 20;
    /// Offset of `prefix_value` (`u32`, little-endian). Spec §3.1.
    pub(crate) const PREFIX_VALUE: usize = 21;
    /// Offset of `operation` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OPERATION: usize = 25;
    /// Offset of `extent_kind` (`u32`, little-endian). Spec §3.1.
    pub(crate) const EXTENT_KIND: usize = 29;
    /// Offset of `direction_reversed` (`u8`). Spec §3.1.
    pub(crate) const DIRECTION_REVERSED: usize = 33;
    /// Offset of `geometry_kind` (`u32`, little-endian). Spec §3.1.
    pub(crate) const GEOMETRY_KIND: usize = 34;
}

/// Byte offsets for the `shifted_extrude_prologue` record.
///
/// Spec §3.1. Record length 42 B.
///
/// ```text
/// Offsets are relative to the shifted Extrude primary indexed header. The operation fields end at the start-support byte; extent lanes and the ordered reference table follow in the enclosing scope.
/// ```
pub(crate) mod shifted_extrude_prologue {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 42;
    /// Offset of `prefix_constant` (`u32`, little-endian). Spec §3.1.
    pub(crate) const PREFIX_CONSTANT: usize = 20;
    /// Offset of `zero_run_3` (`bytes[3]`). Spec §3.1.
    pub(crate) const ZERO_RUN_3: usize = 24;
    /// Offset of `operation` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OPERATION: usize = 27;
    /// Offset of `direction` (`u32`, little-endian). Spec §3.1.
    pub(crate) const DIRECTION: usize = 31;
    /// Offset of `face_extend` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FACE_EXTEND: usize = 35;
    /// Offset of `direction_reversed` (`u8`). Spec §3.1.
    pub(crate) const DIRECTION_REVERSED: usize = 39;
    /// Offset of `geometry_kind` (`u8`). Spec §3.1.
    pub(crate) const GEOMETRY_KIND: usize = 40;
    /// Offset of `start_support` (`u8`). Spec §3.1.
    pub(crate) const START_SUPPORT: usize = 41;
}

/// Byte offsets for the `shifted_reference_aware_extrude_scope_prefix` record.
///
/// Spec §3.1. Record length 296 B.
///
/// ```text
/// Offsets are relative to the primary indexed header. The fixed prefix ends at the u32 reference count; the ordered reference table has 13 entries for the 538-byte class pairs 357/258, 275/262, 361/262, and 349/266, and 11 entries for class pair 323/263, and the scope tail follows it. The final zero high byte of the 36-code-unit GUID is shared with the second-side extent lane.
/// ```
pub(crate) mod shifted_reference_aware_extrude_scope_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 296;
    /// Offset of `prefix_constant` (`u32`, little-endian). Spec §3.1.
    pub(crate) const PREFIX_CONSTANT: usize = 20;
    /// Stated value of `prefix_constant` (`u32`). Spec §3.1.
    pub(crate) const PREFIX_CONSTANT_VALUE: u32 = 0x0000_0001;
    /// Offset of `zero_run_3` (`bytes[3]`). Spec §3.1.
    pub(crate) const ZERO_RUN_3: usize = 24;
    /// Offset of `operation` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OPERATION: usize = 27;
    /// Offset of `direction` (`u32`, little-endian). Spec §3.1.
    pub(crate) const DIRECTION: usize = 31;
    /// Stated value of `direction` (`u32`). Spec §3.1.
    pub(crate) const DIRECTION_VALUE: u32 = 0x0000_0002;
    /// Offset of `face_extend` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FACE_EXTEND: usize = 35;
    /// Stated value of `face_extend` (`u32`). Spec §3.1.
    pub(crate) const FACE_EXTEND_VALUE: u32 = 0x0000_0001;
    /// Offset of `direction_reversed` (`u8`). Spec §3.1.
    pub(crate) const DIRECTION_REVERSED: usize = 39;
    /// Offset of `geometry_kind` (`u8`). Spec §3.1.
    pub(crate) const GEOMETRY_KIND: usize = 40;
    /// Offset of `start_support` (`u8`). Spec §3.1.
    pub(crate) const START_SUPPORT: usize = 41;
    /// Offset of `zero_run_3_after_start` (`bytes[3]`). Spec §3.1.
    pub(crate) const ZERO_RUN_3_AFTER_START: usize = 42;
    /// Offset of `profile_normal` (`f64[3]`, little-endian). Spec §3.1.
    pub(crate) const PROFILE_NORMAL: usize = 45;
    /// Offset of `reference_slots` (`bytes[47]`). Spec §3.1.
    pub(crate) const REFERENCE_SLOTS: usize = 69;
    /// Offset of `first_side_extent` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FIRST_SIDE_EXTENT: usize = 116;
    /// Stated value of `first_side_extent` (`u32`). Spec §3.1.
    pub(crate) const FIRST_SIDE_EXTENT_VALUE: u32 = 0x0000_0002;
    /// Offset of `first_side_owner_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const FIRST_SIDE_OWNER_REFERENCE: usize = 120;
    /// Offset of `first_side_padding` (`bytes[4]`). Spec §3.1.
    pub(crate) const FIRST_SIDE_PADDING: usize = 131;
    /// Offset of `first_side_discriminant` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FIRST_SIDE_DISCRIMINANT: usize = 135;
    /// Stated value of `first_side_discriminant` (`u32`). Spec §3.1.
    pub(crate) const FIRST_SIDE_DISCRIMINANT_VALUE: u32 = 0x0000_0001;
    /// Offset of `first_side_payload` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FIRST_SIDE_PAYLOAD: usize = 139;
    /// Stated value of `first_side_payload` (`u32`). Spec §3.1.
    pub(crate) const FIRST_SIDE_PAYLOAD_VALUE: u32 = 0x0000_0002;
    /// Offset of `first_side_separator` (`u8`). Spec §3.1.
    pub(crate) const FIRST_SIDE_SEPARATOR: usize = 143;
    /// Stated value of `first_side_separator` (`u8`). Spec §3.1.
    pub(crate) const FIRST_SIDE_SEPARATOR_VALUE: u8 = 0;
    /// Offset of `second_side_offset_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const SECOND_SIDE_OFFSET_REFERENCE: usize = 144;
    /// Offset of `second_side_offset_padding` (`bytes[4]`). Spec §3.1.
    pub(crate) const SECOND_SIDE_OFFSET_PADDING: usize = 155;
    /// Offset of `second_side_taper_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const SECOND_SIDE_TAPER_REFERENCE: usize = 159;
    /// Offset of `second_side_taper_padding` (`bytes[5]`). Spec §3.1.
    pub(crate) const SECOND_SIDE_TAPER_PADDING: usize = 170;
    /// Offset of `profile_group_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const PROFILE_GROUP_COUNT: usize = 175;
    /// Stated value of `profile_group_count` (`u32`). Spec §3.1.
    pub(crate) const PROFILE_GROUP_COUNT_VALUE: u32 = 0x0000_0001;
    /// Offset of `profile_group_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const PROFILE_GROUP_REFERENCE: usize = 179;
    /// Offset of `profile_group_padding` (`bytes[8]`). Spec §3.1.
    pub(crate) const PROFILE_GROUP_PADDING: usize = 190;
    /// Offset of `body_group_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const BODY_GROUP_COUNT: usize = 198;
    /// Stated value of `body_group_count` (`u32`). Spec §3.1.
    pub(crate) const BODY_GROUP_COUNT_VALUE: u32 = 0x0000_0001;
    /// Offset of `body_group_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const BODY_GROUP_REFERENCE: usize = 202;
    /// Offset of `body_group_guid_prefix` (`bytes[75]`). Spec §3.1.
    pub(crate) const BODY_GROUP_GUID_PREFIX: usize = 213;
    /// Offset of `second_side_extent` (`u32`, little-endian). Spec §3.1.
    pub(crate) const SECOND_SIDE_EXTENT: usize = 288;
    /// Stated value of `second_side_extent` (`u32`). Spec §3.1.
    pub(crate) const SECOND_SIDE_EXTENT_VALUE: u32 = 0x0000_0000;
    /// Offset of `reference_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const REFERENCE_COUNT: usize = 292;
}

/// Byte offsets for the `shifted_reference_aware_extrude_class_323_tail` record.
///
/// Spec §3.1. Record length 288 B.
///
/// ```text
/// Offsets are relative to the primary indexed header. The class-specific tail moves the trailing-reference count and marked reference to +190 and +194, leaves zero padding at +205..+212, and keeps the 36-code-unit GUID prefix at +213.
/// ```
pub(crate) mod shifted_reference_aware_extrude_class_323_tail {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 288;
    /// Offset of `profile_group_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const PROFILE_GROUP_COUNT: usize = 175;
    /// Offset of `profile_group_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const PROFILE_GROUP_REFERENCE: usize = 179;
    /// Offset of `trailing_reference_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const TRAILING_REFERENCE_COUNT: usize = 190;
    /// Stated value of `trailing_reference_count` (`u32`). Spec §3.1.
    pub(crate) const TRAILING_REFERENCE_COUNT_VALUE: u32 = 0x0000_0001;
    /// Offset of `trailing_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const TRAILING_REFERENCE: usize = 194;
    /// Offset of `trailing_reference_padding` (`bytes[8]`). Spec §3.1.
    pub(crate) const TRAILING_REFERENCE_PADDING: usize = 205;
    /// Offset of `guid_prefix` (`bytes[75]`). Spec §3.1.
    pub(crate) const GUID_PREFIX: usize = 213;
}

/// Byte offsets for the `shifted_reference_aware_extrude_class_323_symmetric_prefix` record.
///
/// Spec §3.1. Record length 276 B.
///
/// ```text
/// Offsets are relative to the primary indexed header. This class-specific prefix stores both symmetric-through-all extent discriminators before the grouped-reference tail and ends at the u32 reference count.
/// ```
pub(crate) mod shifted_reference_aware_extrude_class_323_symmetric_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 276;
    /// Offset of `first_side_extent` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FIRST_SIDE_EXTENT: usize = 116;
    /// Stated value of `first_side_extent` (`u32`). Spec §3.1.
    pub(crate) const FIRST_SIDE_EXTENT_VALUE: u32 = 0x0000_0004;
    /// Offset of `first_side_padding` (`bytes[9]`). Spec §3.1.
    pub(crate) const FIRST_SIDE_PADDING: usize = 120;
    /// Offset of `second_side_extent` (`u32`, little-endian). Spec §3.1.
    pub(crate) const SECOND_SIDE_EXTENT: usize = 129;
    /// Stated value of `second_side_extent` (`u32`). Spec §3.1.
    pub(crate) const SECOND_SIDE_EXTENT_VALUE: u32 = 0x0000_0004;
    /// Offset of `second_side_padding` (`bytes[6]`). Spec §3.1.
    pub(crate) const SECOND_SIDE_PADDING: usize = 133;
    /// Offset of `symmetric_extent_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const SYMMETRIC_EXTENT_REFERENCE: usize = 139;
    /// Offset of `symmetric_extent_padding` (`bytes[5]`). Spec §3.1.
    pub(crate) const SYMMETRIC_EXTENT_PADDING: usize = 150;
    /// Offset of `profile_group_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const PROFILE_GROUP_COUNT: usize = 155;
    /// Stated value of `profile_group_count` (`u32`). Spec §3.1.
    pub(crate) const PROFILE_GROUP_COUNT_VALUE: u32 = 0x0000_0001;
    /// Offset of `profile_group_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const PROFILE_GROUP_REFERENCE: usize = 159;
    /// Offset of `profile_group_padding` (`bytes[8]`). Spec §3.1.
    pub(crate) const PROFILE_GROUP_PADDING: usize = 170;
    /// Offset of `trailing_reference_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const TRAILING_REFERENCE_COUNT: usize = 178;
    /// Stated value of `trailing_reference_count` (`u32`). Spec §3.1.
    pub(crate) const TRAILING_REFERENCE_COUNT_VALUE: u32 = 0x0000_0001;
    /// Offset of `trailing_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const TRAILING_REFERENCE: usize = 182;
    /// Offset of `guid_prefix` (`bytes[76]`). Spec §3.1.
    pub(crate) const GUID_PREFIX: usize = 193;
    /// Offset of `reference_count_padding` (`bytes[3]`). Spec §3.1.
    pub(crate) const REFERENCE_COUNT_PADDING: usize = 269;
    /// Offset of `reference_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const REFERENCE_COUNT: usize = 272;
    /// Stated value of `reference_count` (`u32`). Spec §3.1.
    pub(crate) const REFERENCE_COUNT_VALUE: u32 = 0x0000_000a;
}

/// Byte offsets for the `compact_shifted_extrude_prologue` record.
///
/// Spec §3.1. Record length 41 B.
///
/// ```text
/// Offsets are relative to the compact legacy Extrude primary indexed header. The operation fields end at the start-support byte; the one-sided extent lane and ordered reference table follow in the enclosing scope.
/// ```
pub(crate) mod compact_shifted_extrude_prologue {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 41;
    /// Offset of `prefix_constant` (`u32`, little-endian). Spec §3.1.
    pub(crate) const PREFIX_CONSTANT: usize = 20;
    /// Offset of `zero_run_2` (`bytes[2]`). Spec §3.1.
    pub(crate) const ZERO_RUN_2: usize = 24;
    /// Offset of `operation` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OPERATION: usize = 26;
    /// Offset of `direction` (`u32`, little-endian). Spec §3.1.
    pub(crate) const DIRECTION: usize = 30;
    /// Offset of `face_extend` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FACE_EXTEND: usize = 34;
    /// Offset of `direction_reversed` (`u8`). Spec §3.1.
    pub(crate) const DIRECTION_REVERSED: usize = 38;
    /// Offset of `geometry_kind` (`u8`). Spec §3.1.
    pub(crate) const GEOMETRY_KIND: usize = 39;
    /// Offset of `start_support` (`u8`). Spec §3.1.
    pub(crate) const START_SUPPORT: usize = 40;
}

/// Byte offsets for the `compact_shifted_extrude_extent_and_table_prefix` record.
///
/// Spec §3.1. Record length 255 B.
///
/// ```text
/// Offsets are relative to the compact legacy Extrude primary indexed header. The one-sided distance and symmetric-distance forms use the two extent values shown here; the ordered reference table begins at the final field.
/// ```
pub(crate) mod compact_shifted_extrude_extent_and_table_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 255;
    /// Offset of `first_side_extent` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FIRST_SIDE_EXTENT: usize = 105;
    /// Offset of `second_side_extent` (`u32`, little-endian). Spec §3.1.
    pub(crate) const SECOND_SIDE_EXTENT: usize = 109;
    /// Offset of `reference_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const REFERENCE_COUNT: usize = 251;
}

/// Byte offsets for the `compact_shifted_extrude_mixed_extent_and_table_prefix` record.
///
/// Spec §3.1. Record length 285 B.
///
/// ```text
/// Offsets are relative to the compact legacy Extrude primary indexed header. The mixed two-sided form uses the two extent values shown here; the ordered reference table begins at the final field.
/// ```
pub(crate) mod compact_shifted_extrude_mixed_extent_and_table_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 285;
    /// Offset of `first_side_extent` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FIRST_SIDE_EXTENT: usize = 124;
    /// Offset of `second_side_extent` (`u32`, little-endian). Spec §3.1.
    pub(crate) const SECOND_SIDE_EXTENT: usize = 128;
    /// Offset of `reference_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const REFERENCE_COUNT: usize = 281;
}

/// Byte offsets for the `marked_shifted_extrude_prologue` record.
///
/// Spec §3.1. Record length 43 B.
///
/// ```text
/// Offsets are relative to the marked shifted Extrude primary indexed header. The marker shifts the operation field run by one byte.
/// ```
pub(crate) mod marked_shifted_extrude_prologue {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 43;
    /// Offset of `prefix_constant` (`u32`, little-endian). Spec §3.1.
    pub(crate) const PREFIX_CONSTANT: usize = 20;
    /// Offset of `zero_run_3` (`bytes[3]`). Spec §3.1.
    pub(crate) const ZERO_RUN_3: usize = 24;
    /// Offset of `operation_prefix_marker` (`u8`). Spec §3.1.
    pub(crate) const OPERATION_PREFIX_MARKER: usize = 27;
    /// Offset of `operation` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OPERATION: usize = 28;
    /// Offset of `direction` (`u32`, little-endian). Spec §3.1.
    pub(crate) const DIRECTION: usize = 32;
    /// Offset of `face_extend` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FACE_EXTEND: usize = 36;
    /// Offset of `direction_reversed` (`u8`). Spec §3.1.
    pub(crate) const DIRECTION_REVERSED: usize = 40;
    /// Offset of `geometry_kind` (`u8`). Spec §3.1.
    pub(crate) const GEOMETRY_KIND: usize = 41;
    /// Offset of `start_support` (`u8`). Spec §3.1.
    pub(crate) const START_SUPPORT: usize = 42;
}

/// Byte offsets for the `legacy_class_415_symmetric_extrude_prefix` record.
///
/// Spec §3.1. Record length 292 B.
///
/// ```text
/// Offsets are relative to the class-415 primary indexed header. The primary/paired frame lengths are 447 B with five ordered references and 469 B with seven; the fixed prefix ends at the ordered reference-count field at offset 288.
/// ```
pub(crate) mod legacy_class_415_symmetric_extrude_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 292;
    /// Offset of `prefix_constant` (`u32`, little-endian). Spec §3.1.
    pub(crate) const PREFIX_CONSTANT: usize = 20;
    /// Stated value of `prefix_constant` (`u32`). Spec §3.1.
    pub(crate) const PREFIX_CONSTANT_VALUE: u32 = 0x0000_0001;
    /// Offset of `zero_run_3` (`bytes[3]`). Spec §3.1.
    pub(crate) const ZERO_RUN_3: usize = 24;
    /// Offset of `operation_prefix_marker` (`u8`). Spec §3.1.
    pub(crate) const OPERATION_PREFIX_MARKER: usize = 27;
    /// Stated value of `operation_prefix_marker` (`u8`). Spec §3.1.
    pub(crate) const OPERATION_PREFIX_MARKER_VALUE: u8 = 1;
    /// Offset of `operation` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OPERATION: usize = 28;
    /// Offset of `direction` (`u32`, little-endian). Spec §3.1.
    pub(crate) const DIRECTION: usize = 32;
    /// Stated value of `direction` (`u32`). Spec §3.1.
    pub(crate) const DIRECTION_VALUE: u32 = 0x0000_0003;
    /// Offset of `face_extend` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FACE_EXTEND: usize = 36;
    /// Stated value of `face_extend` (`u32`). Spec §3.1.
    pub(crate) const FACE_EXTEND_VALUE: u32 = 0x0000_0002;
    /// Offset of `direction_reversed` (`u8`). Spec §3.1.
    pub(crate) const DIRECTION_REVERSED: usize = 40;
    /// Offset of `geometry_kind` (`u8`). Spec §3.1.
    pub(crate) const GEOMETRY_KIND: usize = 41;
    /// Offset of `start_support` (`u8`). Spec §3.1.
    pub(crate) const START_SUPPORT: usize = 42;
    /// Offset of `zero_run_3_after_start` (`bytes[3]`). Spec §3.1.
    pub(crate) const ZERO_RUN_3_AFTER_START: usize = 43;
    /// Offset of `profile_normal` (`f64[3]`, little-endian). Spec §3.1.
    pub(crate) const PROFILE_NORMAL: usize = 46;
    /// Offset of `reference_slots` (`bytes[47]`). Spec §3.1.
    pub(crate) const REFERENCE_SLOTS: usize = 70;
    /// Offset of `first_side_extent` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FIRST_SIDE_EXTENT: usize = 117;
    /// Stated value of `first_side_extent` (`u32`). Spec §3.1.
    pub(crate) const FIRST_SIDE_EXTENT_VALUE: u32 = 0x0000_0001;
    /// Offset of `zero_run_9` (`bytes[9]`). Spec §3.1.
    pub(crate) const ZERO_RUN_9: usize = 121;
    /// Offset of `second_side_extent` (`u32`, little-endian). Spec §3.1.
    pub(crate) const SECOND_SIDE_EXTENT: usize = 130;
    /// Stated value of `second_side_extent` (`u32`). Spec §3.1.
    pub(crate) const SECOND_SIDE_EXTENT_VALUE: u32 = 0x0000_0001;
    /// Offset of `reference_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const REFERENCE_COUNT: usize = 288;
}

/// Byte offsets for the `legacy_class_415_one_sided_to_face_extrude_prefix` record.
///
/// Spec §3.1. Record length 282 B.
///
/// ```text
/// Offsets are relative to the class-415 primary indexed header. The paired frame begins at 481 B, the ordered reference count is at offset 278, and the fixed prefix ends after that count.
/// ```
pub(crate) mod legacy_class_415_one_sided_to_face_extrude_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 282;
    /// Offset of `prefix_constant` (`u32`, little-endian). Spec §3.1.
    pub(crate) const PREFIX_CONSTANT: usize = 20;
    /// Stated value of `prefix_constant` (`u32`). Spec §3.1.
    pub(crate) const PREFIX_CONSTANT_VALUE: u32 = 0x0000_0001;
    /// Offset of `zero_run_3` (`bytes[3]`). Spec §3.1.
    pub(crate) const ZERO_RUN_3: usize = 24;
    /// Offset of `operation_prefix_marker` (`u8`). Spec §3.1.
    pub(crate) const OPERATION_PREFIX_MARKER: usize = 27;
    /// Stated value of `operation_prefix_marker` (`u8`). Spec §3.1.
    pub(crate) const OPERATION_PREFIX_MARKER_VALUE: u8 = 1;
    /// Offset of `operation` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OPERATION: usize = 28;
    /// Offset of `direction` (`u32`, little-endian). Spec §3.1.
    pub(crate) const DIRECTION: usize = 32;
    /// Stated value of `direction` (`u32`). Spec §3.1.
    pub(crate) const DIRECTION_VALUE: u32 = 0x0000_0001;
    /// Offset of `face_extend` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FACE_EXTEND: usize = 36;
    /// Stated value of `face_extend` (`u32`). Spec §3.1.
    pub(crate) const FACE_EXTEND_VALUE: u32 = 0x0000_0001;
    /// Offset of `direction_reversed` (`u8`). Spec §3.1.
    pub(crate) const DIRECTION_REVERSED: usize = 40;
    /// Offset of `geometry_kind` (`u8`). Spec §3.1.
    pub(crate) const GEOMETRY_KIND: usize = 41;
    /// Offset of `start_support` (`u8`). Spec §3.1.
    pub(crate) const START_SUPPORT: usize = 42;
    /// Offset of `zero_run_3_after_start` (`bytes[3]`). Spec §3.1.
    pub(crate) const ZERO_RUN_3_AFTER_START: usize = 43;
    /// Offset of `profile_normal` (`f64[3]`, little-endian). Spec §3.1.
    pub(crate) const PROFILE_NORMAL: usize = 46;
    /// Offset of `first_side_extent` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FIRST_SIDE_EXTENT: usize = 107;
    /// Stated value of `first_side_extent` (`u32`). Spec §3.1.
    pub(crate) const FIRST_SIDE_EXTENT_VALUE: u32 = 0x0000_0002;
    /// Offset of `first_side_offset_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const FIRST_SIDE_OFFSET_REFERENCE: usize = 111;
    /// Offset of `second_side_extent` (`u32`, little-endian). Spec §3.1.
    pub(crate) const SECOND_SIDE_EXTENT: usize = 274;
    /// Stated value of `second_side_extent` (`u32`). Spec §3.1.
    pub(crate) const SECOND_SIDE_EXTENT_VALUE: u32 = 0x0000_0000;
    /// Offset of `reference_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const REFERENCE_COUNT: usize = 278;
    /// Stated value of `reference_count` (`u32`). Spec §3.1.
    pub(crate) const REFERENCE_COUNT_VALUE: u32 = 0x0000_0009;
}

/// Byte offsets for the `legacy_class_415_one_sided_distance_extrude_prefix` record.
///
/// Spec §3.1. Record length 272 B.
///
/// ```text
/// Offsets are relative to the class-415 primary indexed header. The paired frame begins at 449 B, the ordered reference count is at offset 268, and the fixed prefix ends after that count.
/// ```
pub(crate) mod legacy_class_415_one_sided_distance_extrude_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 272;
    /// Offset of `prefix_constant` (`u32`, little-endian). Spec §3.1.
    pub(crate) const PREFIX_CONSTANT: usize = 20;
    /// Stated value of `prefix_constant` (`u32`). Spec §3.1.
    pub(crate) const PREFIX_CONSTANT_VALUE: u32 = 0x0000_0001;
    /// Offset of `zero_run_3` (`bytes[3]`). Spec §3.1.
    pub(crate) const ZERO_RUN_3: usize = 24;
    /// Offset of `operation_prefix_marker` (`u8`). Spec §3.1.
    pub(crate) const OPERATION_PREFIX_MARKER: usize = 27;
    /// Stated value of `operation_prefix_marker` (`u8`). Spec §3.1.
    pub(crate) const OPERATION_PREFIX_MARKER_VALUE: u8 = 1;
    /// Offset of `operation` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OPERATION: usize = 28;
    /// Offset of `direction` (`u32`, little-endian). Spec §3.1.
    pub(crate) const DIRECTION: usize = 32;
    /// Stated value of `direction` (`u32`). Spec §3.1.
    pub(crate) const DIRECTION_VALUE: u32 = 0x0000_0001;
    /// Offset of `face_extend` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FACE_EXTEND: usize = 36;
    /// Stated value of `face_extend` (`u32`). Spec §3.1.
    pub(crate) const FACE_EXTEND_VALUE: u32 = 0x0000_0002;
    /// Offset of `direction_reversed` (`u8`). Spec §3.1.
    pub(crate) const DIRECTION_REVERSED: usize = 40;
    /// Offset of `geometry_kind` (`u8`). Spec §3.1.
    pub(crate) const GEOMETRY_KIND: usize = 41;
    /// Offset of `start_support` (`u8`). Spec §3.1.
    pub(crate) const START_SUPPORT: usize = 42;
    /// Offset of `zero_run_3_after_start` (`bytes[3]`). Spec §3.1.
    pub(crate) const ZERO_RUN_3_AFTER_START: usize = 43;
    /// Offset of `profile_normal` (`f64[3]`, little-endian). Spec §3.1.
    pub(crate) const PROFILE_NORMAL: usize = 46;
    /// Offset of `first_side_extent` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FIRST_SIDE_EXTENT: usize = 107;
    /// Stated value of `first_side_extent` (`u32`). Spec §3.1.
    pub(crate) const FIRST_SIDE_EXTENT_VALUE: u32 = 0x0000_0001;
    /// Offset of `second_side_extent` (`u32`, little-endian). Spec §3.1.
    pub(crate) const SECOND_SIDE_EXTENT: usize = 264;
    /// Stated value of `second_side_extent` (`u32`). Spec §3.1.
    pub(crate) const SECOND_SIDE_EXTENT_VALUE: u32 = 0x0000_0000;
    /// Offset of `reference_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const REFERENCE_COUNT: usize = 268;
    /// Stated value of `reference_count` (`u32`). Spec §3.1.
    pub(crate) const REFERENCE_COUNT_VALUE: u32 = 0x0000_0007;
}

/// Byte offsets for the `shifted_extrude_offset_profile_extent_lane` record.
///
/// Spec §3.1. Record length 134 B.
///
/// ```text
/// Offsets are relative to the shifted Extrude primary indexed header. The lane ends before the remaining scope envelope.
/// ```
pub(crate) mod shifted_extrude_offset_profile_extent_lane {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 134;
    /// Offset of `first_side_extent` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FIRST_SIDE_EXTENT: usize = 116;
    /// Offset of `second_side_extent` (`u32`, little-endian). Spec §3.1.
    pub(crate) const SECOND_SIDE_EXTENT: usize = 130;
}

/// Byte offsets for the `marked_shifted_extrude_symmetric_extent_lane` record.
///
/// Spec §3.1. Record length 135 B.
///
/// ```text
/// Offsets are relative to the marked shifted Extrude primary indexed header. Unselected fields in the intervening envelope have no extent semantics.
/// ```
pub(crate) mod marked_shifted_extrude_symmetric_extent_lane {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 135;
    /// Offset of `first_side_extent` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FIRST_SIDE_EXTENT: usize = 117;
    /// Offset of `second_side_extent` (`u32`, little-endian). Spec §3.1.
    pub(crate) const SECOND_SIDE_EXTENT: usize = 131;
}

/// Byte offsets for the `shifted_extrude_offset_283_two_sided_tail` record.
///
/// Spec §3.1. Record length 204 B.
///
/// ```text
/// Offsets are relative to the shifted Extrude primary indexed header. The tail ends immediately before the following LP-UTF16 GUID field.
/// ```
pub(crate) mod shifted_extrude_offset_283_two_sided_tail {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 204;
    /// Offset of `first_parameter_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const FIRST_PARAMETER_REFERENCE: usize = 139;
    /// Offset of `first_side_extent` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FIRST_SIDE_EXTENT: usize = 166;
    /// Offset of `second_parameter_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const SECOND_PARAMETER_REFERENCE: usize = 170;
    /// Offset of `second_side_extent` (`u32`, little-endian). Spec §3.1.
    pub(crate) const SECOND_SIDE_EXTENT: usize = 181;
    /// Offset of `trailing_entity_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const TRAILING_ENTITY_REFERENCE: usize = 185;
    /// Offset of `zero_run_8` (`bytes[8]`). Spec §3.1.
    pub(crate) const ZERO_RUN_8: usize = 196;
}

/// Byte offsets for the `thread_standard_scope_prefix` record.
///
/// Spec §3.1. Record length 38 B.
///
/// ```text
/// Direct-prefix offsets are relative to the primary Thread indexed header. The three LP-UTF16 fields begin at offset 38 and are outside this fixed prefix.
/// ```
pub(crate) mod thread_standard_scope_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 38;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_10` (`bytes[10]`). Spec §3.1.
    pub(crate) const ZERO_RUN_10: usize = 11;
    /// Offset of `fixed_scalar` (`f64`, little-endian). Spec §3.1.
    pub(crate) const FIXED_SCALAR: usize = 21;
    /// Offset of `standard_marker` (`bytes[5]`). Spec §3.1.
    pub(crate) const STANDARD_MARKER: usize = 29;
    /// Offset of `standard_prefix_tail` (`bytes[4]`). Spec §3.1.
    pub(crate) const STANDARD_PREFIX_TAIL: usize = 34;
}

/// Byte offsets for the `thread_compact_scope_prefix` record.
///
/// Spec §3.1. Record length 38 B.
///
/// ```text
/// The direct compact prefix has the same fixed width as the direct standard prefix and starts the same three LP-UTF16 fields at offset 38.
/// ```
pub(crate) mod thread_compact_scope_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 38;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `fixed_scalar` (`f64`, little-endian). Spec §3.1.
    pub(crate) const FIXED_SCALAR: usize = 21;
    /// Offset of `compact_marker` (`bytes[5]`). Spec §3.1.
    pub(crate) const COMPACT_MARKER: usize = 29;
    /// Offset of `compact_prefix_tail` (`bytes[4]`). Spec §3.1.
    pub(crate) const COMPACT_PREFIX_TAIL: usize = 34;
}

/// Byte offsets for the `thread_owner_marked_scope_prefix` record.
///
/// Spec §3.1. Record length 42 B.
///
/// ```text
/// Offsets are relative to the primary Thread indexed header. The three LP-UTF16 fields begin at offset 42 and are outside this fixed prefix.
/// ```
pub(crate) mod thread_owner_marked_scope_prefix {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 42;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `zero_run_9` (`bytes[9]`). Spec §3.1.
    pub(crate) const ZERO_RUN_9: usize = 11;
    /// Offset of `owner_marker` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OWNER_MARKER: usize = 20;
    /// Offset of `separator` (`u8`). Spec §3.1.
    pub(crate) const SEPARATOR: usize = 24;
    /// Offset of `fixed_scalar` (`f64`, little-endian). Spec §3.1.
    pub(crate) const FIXED_SCALAR: usize = 25;
    /// Offset of `form_marker` (`bytes[5]`). Spec §3.1.
    pub(crate) const FORM_MARKER: usize = 33;
    /// Offset of `form_token` (`bytes[4]`). Spec §3.1.
    pub(crate) const FORM_TOKEN: usize = 38;
}

/// Byte offsets for the `thread_standard_construction_tail` record.
///
/// Spec §3.1. Record length 40 B.
///
/// ```text
/// Offsets are relative to the first byte after the third LP-UTF16 string.
/// ```
pub(crate) mod thread_standard_construction_tail {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 40;
    /// Offset of `construction_marker` (`bytes[5]`). Spec §3.1.
    pub(crate) const CONSTRUCTION_MARKER: usize = 0;
    /// Offset of `major_diameter` (`f64`, little-endian). Spec §3.1.
    pub(crate) const MAJOR_DIAMETER: usize = 5;
    /// Offset of `minor_diameter` (`f64`, little-endian). Spec §3.1.
    pub(crate) const MINOR_DIAMETER: usize = 13;
    /// Offset of `pitch_marker` (`u8`). Spec §3.1.
    pub(crate) const PITCH_MARKER: usize = 21;
    /// Offset of `pitch` (`f64`, little-endian). Spec §3.1.
    pub(crate) const PITCH: usize = 22;
    /// Offset of `pitch_diameter` (`f64`, little-endian). Spec §3.1.
    pub(crate) const PITCH_DIAMETER: usize = 30;
    /// Offset of `standard_trailer` (`bytes[2]`). Spec §3.1.
    pub(crate) const STANDARD_TRAILER: usize = 38;
}

/// Byte offsets for the `thread_compact_construction_tail` record.
///
/// Spec §3.1. Record length 42 B.
///
/// ```text
/// Offsets are relative to the first byte after the third LP-UTF16 string.
/// ```
pub(crate) mod thread_compact_construction_tail {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 42;
    /// Offset of `construction_marker` (`bytes[5]`). Spec §3.1.
    pub(crate) const CONSTRUCTION_MARKER: usize = 0;
    /// Offset of `compact_trailer` (`bytes[4]`). Spec §3.1.
    pub(crate) const COMPACT_TRAILER: usize = 38;
}

/// Byte offsets for the `edge_flange_fixed_operation_section` record.
///
/// Spec §3.1. Record length 79 B.
///
/// ```text
/// Offsets are relative to the section base `85 + S`, where `S` is the header shift. The section runs from the bend-position discriminator through the inside bend radius; the result-record run and the two closing group references follow it at variable offsets.
/// ```
pub(crate) mod edge_flange_fixed_operation_section {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 79;
    /// Offset of `bend_position` (`u32`, little-endian). Spec §3.1.
    pub(crate) const BEND_POSITION: usize = 0;
    /// Offset of `edge_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const EDGE_COUNT: usize = 4;
    /// Offset of `edge_wrapper_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const EDGE_WRAPPER_REFERENCE: usize = 8;
    /// Offset of `settings_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const SETTINGS_REFERENCE: usize = 19;
    /// Offset of `height_datum` (`u32`, little-endian). Spec §3.1.
    pub(crate) const HEIGHT_DATUM: usize = 30;
    /// Offset of `angle_owner_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const ANGLE_OWNER_REFERENCE: usize = 34;
    /// Offset of `height_owner_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const HEIGHT_OWNER_REFERENCE: usize = 45;
    /// Offset of `unsettled_side_reference` (`u32`, little-endian). Spec §3.1.
    pub(crate) const UNSETTLED_SIDE_REFERENCE: usize = 56;
    /// Offset of `inside_bend_radius` (`f64`, little-endian). Spec §3.1.
    pub(crate) const INSIDE_BEND_RADIUS: usize = 71;
}

/// Byte offsets for the `edge_flange_legacy_single_edge_fixed_operation` record.
///
/// Spec §3.1. Record length 218 B.
///
/// ```text
/// Offsets are relative to the primary scope header. The class-325/class-258 and class-334/class-257 forms have a 494-byte primary frame; the fixed operation fields close at the marked role-0x08 group reference.
/// ```
pub(crate) mod edge_flange_legacy_single_edge_fixed_operation {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 218;
    /// Offset of `bend_position` (`u32`, little-endian). Spec §3.1.
    pub(crate) const BEND_POSITION: usize = 76;
    /// Offset of `edge_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const EDGE_COUNT: usize = 80;
    /// Offset of `edge_wrapper_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const EDGE_WRAPPER_REFERENCE: usize = 84;
    /// Offset of `settings_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const SETTINGS_REFERENCE: usize = 95;
    /// Offset of `height_datum` (`u32`, little-endian). Spec §3.1.
    pub(crate) const HEIGHT_DATUM: usize = 106;
    /// Offset of `angle_owner_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const ANGLE_OWNER_REFERENCE: usize = 110;
    /// Offset of `height_owner_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const HEIGHT_OWNER_REFERENCE: usize = 121;
    /// Offset of `reference_side` (`u32`, little-endian). Spec §3.1.
    pub(crate) const REFERENCE_SIDE: usize = 132;
    /// Offset of `inside_bend_radius` (`f64`, little-endian). Spec §3.1.
    pub(crate) const INSIDE_BEND_RADIUS: usize = 138;
    /// Offset of `result_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const RESULT_COUNT: usize = 146;
    /// Offset of `result_one_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const RESULT_ONE_REFERENCE: usize = 150;
    /// Offset of `result_one_trailer` (`u32`, little-endian). Spec §3.1.
    pub(crate) const RESULT_ONE_TRAILER: usize = 161;
    /// Offset of `result_two_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const RESULT_TWO_REFERENCE: usize = 165;
    /// Offset of `result_two_trailer` (`u32`, little-endian). Spec §3.1.
    pub(crate) const RESULT_TWO_TRAILER: usize = 176;
    /// Offset of `result_separator` (`u32`, little-endian). Spec §3.1.
    pub(crate) const RESULT_SEPARATOR: usize = 180;
    /// Offset of `aggregate_group_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const AGGREGATE_GROUP_REFERENCE: usize = 184;
    /// Offset of `edge_group_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const EDGE_GROUP_REFERENCE: usize = 207;
}

/// Byte offsets for the `edge_flange_multi_edge_fixed_operation` record.
///
/// Spec §3.1. Record length 271 B.
///
/// ```text
/// Offsets are relative to the primary scope header. The fixed operation fields close at the second marked role-0x08 group reference; the paired header follows the 591-byte primary frame.
/// ```
pub(crate) mod edge_flange_multi_edge_fixed_operation {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 271;
    /// Offset of `bend_position` (`u32`, little-endian). Spec §3.1.
    pub(crate) const BEND_POSITION: usize = 92;
    /// Offset of `edge_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const EDGE_COUNT: usize = 96;
    /// Offset of `edge_wrapper_one_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const EDGE_WRAPPER_ONE_REFERENCE: usize = 100;
    /// Offset of `edge_wrapper_two_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const EDGE_WRAPPER_TWO_REFERENCE: usize = 111;
    /// Offset of `settings_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const SETTINGS_REFERENCE: usize = 122;
    /// Offset of `height_datum` (`u32`, little-endian). Spec §3.1.
    pub(crate) const HEIGHT_DATUM: usize = 133;
    /// Offset of `angle_owner_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const ANGLE_OWNER_REFERENCE: usize = 137;
    /// Offset of `height_owner_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const HEIGHT_OWNER_REFERENCE: usize = 148;
    /// Offset of `reference_side` (`u32`, little-endian). Spec §3.1.
    pub(crate) const REFERENCE_SIDE: usize = 159;
    /// Offset of `inside_bend_radius` (`f64`, little-endian). Spec §3.1.
    pub(crate) const INSIDE_BEND_RADIUS: usize = 165;
    /// Offset of `result_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const RESULT_COUNT: usize = 173;
    /// Offset of `result_one_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const RESULT_ONE_REFERENCE: usize = 177;
    /// Offset of `result_one_trailer` (`u32`, little-endian). Spec §3.1.
    pub(crate) const RESULT_ONE_TRAILER: usize = 188;
    /// Offset of `result_two_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const RESULT_TWO_REFERENCE: usize = 192;
    /// Offset of `result_two_trailer` (`u32`, little-endian). Spec §3.1.
    pub(crate) const RESULT_TWO_TRAILER: usize = 203;
    /// Offset of `result_three_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const RESULT_THREE_REFERENCE: usize = 207;
    /// Offset of `result_three_trailer` (`u32`, little-endian). Spec §3.1.
    pub(crate) const RESULT_THREE_TRAILER: usize = 218;
    /// Offset of `result_separator` (`u32`, little-endian). Spec §3.1.
    pub(crate) const RESULT_SEPARATOR: usize = 222;
    /// Offset of `aggregate_group_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const AGGREGATE_GROUP_REFERENCE: usize = 226;
    /// Offset of `edge_group_one_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const EDGE_GROUP_ONE_REFERENCE: usize = 249;
    /// Offset of `edge_group_two_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const EDGE_GROUP_TWO_REFERENCE: usize = 260;
}

/// Byte offsets for the `edge_flange_class286_single_edge_fixed_operation` record.
///
/// Spec §3.1. Record length 207 B.
///
/// ```text
/// Offsets are relative to the primary scope header. The fixed operation fields close at the marked role-0x08 group reference; the paired header follows the 483-byte primary frame.
/// ```
pub(crate) mod edge_flange_class286_single_edge_fixed_operation {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 207;
    /// Offset of `bend_position` (`u32`, little-endian). Spec §3.1.
    pub(crate) const BEND_POSITION: usize = 80;
    /// Offset of `edge_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const EDGE_COUNT: usize = 84;
    /// Offset of `edge_wrapper_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const EDGE_WRAPPER_REFERENCE: usize = 88;
    /// Offset of `settings_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const SETTINGS_REFERENCE: usize = 99;
    /// Offset of `height_datum` (`u32`, little-endian). Spec §3.1.
    pub(crate) const HEIGHT_DATUM: usize = 110;
    /// Offset of `angle_owner_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const ANGLE_OWNER_REFERENCE: usize = 114;
    /// Offset of `height_owner_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const HEIGHT_OWNER_REFERENCE: usize = 125;
    /// Offset of `reference_side` (`u32`, little-endian). Spec §3.1.
    pub(crate) const REFERENCE_SIDE: usize = 136;
    /// Offset of `inside_bend_radius` (`f64`, little-endian). Spec §3.1.
    pub(crate) const INSIDE_BEND_RADIUS: usize = 142;
    /// Offset of `result_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const RESULT_COUNT: usize = 150;
    /// Offset of `result_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const RESULT_REFERENCE: usize = 154;
    /// Offset of `result_trailer` (`u32`, little-endian). Spec §3.1.
    pub(crate) const RESULT_TRAILER: usize = 165;
    /// Offset of `result_separator` (`u32`, little-endian). Spec §3.1.
    pub(crate) const RESULT_SEPARATOR: usize = 169;
    /// Offset of `aggregate_group_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const AGGREGATE_GROUP_REFERENCE: usize = 173;
    /// Offset of `edge_group_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const EDGE_GROUP_REFERENCE: usize = 196;
}

/// Byte offsets for the `edge_flange_to_object_fixed_operation_section` record.
///
/// Spec §3.1. Record length 181 B.
///
/// ```text
/// Offsets are relative to the section base `85 + S`. This single-edge form closes at the marked role-`0x08` group reference; the paired header follows at `576 + S`.
/// ```
pub(crate) mod edge_flange_to_object_fixed_operation_section {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 181;
    /// Offset of `target_group_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const TARGET_GROUP_REFERENCE: usize = 94;
    /// Offset of `target_reference_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const TARGET_REFERENCE_COUNT: usize = 105;
    /// Offset of `inserted_reference_one` (`bytes[11]`). Spec §3.1.
    pub(crate) const INSERTED_REFERENCE_ONE: usize = 109;
    /// Offset of `inserted_reference_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const INSERTED_REFERENCE_COUNT: usize = 120;
    /// Offset of `inserted_reference_two` (`bytes[11]`). Spec §3.1.
    pub(crate) const INSERTED_REFERENCE_TWO: usize = 124;
    /// Offset of `aggregate_reference_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const AGGREGATE_REFERENCE_COUNT: usize = 139;
    /// Offset of `aggregate_group_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const AGGREGATE_GROUP_REFERENCE: usize = 143;
    /// Offset of `edge_reference_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const EDGE_REFERENCE_COUNT: usize = 166;
    /// Offset of `edge_group_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const EDGE_GROUP_REFERENCE: usize = 170;
}

/// Byte offsets for the `hem_gap_length_fixed_operation_section` record.
///
/// Spec §3.1. Record length 79 B.
///
/// ```text
/// Offsets are relative to the section base `85 + S`, where `S` is the header shift. The aggregate and role-`0x08` group references follow this fixed section at offsets 108 and 135.
/// ```
pub(crate) mod hem_gap_length_fixed_operation_section {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 79;
    /// Offset of `edge_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const EDGE_COUNT: usize = 4;
    /// Offset of `edge_wrapper_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const EDGE_WRAPPER_REFERENCE: usize = 8;
    /// Offset of `settings_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const SETTINGS_REFERENCE: usize = 19;
    /// Offset of `gap_owner_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const GAP_OWNER_REFERENCE: usize = 42;
    /// Offset of `length_owner_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const LENGTH_OWNER_REFERENCE: usize = 53;
    /// Offset of `inside_bend_radius` (`f64`, little-endian). Spec §3.1.
    pub(crate) const INSIDE_BEND_RADIUS: usize = 71;
}

/// Byte offsets for the `hem_rolled_fixed_operation_section` record.
///
/// Spec §3.1. Record length 79 B.
///
/// ```text
/// Offsets are relative to the section base `85 + S`. The angle owner precedes the radius owner in the fixed section; source kinds assign their semantic roles.
/// ```
pub(crate) mod hem_rolled_fixed_operation_section {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 79;
    /// Offset of `angle_owner_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const ANGLE_OWNER_REFERENCE: usize = 41;
    /// Offset of `radius_owner_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const RADIUS_OWNER_REFERENCE: usize = 54;
    /// Offset of `inside_bend_radius` (`f64`, little-endian). Spec §3.1.
    pub(crate) const INSIDE_BEND_RADIUS: usize = 71;
}

/// Byte offsets for the `hem_teardrop_fixed_operation_section` record.
///
/// Spec §3.1. Record length 89 B.
///
/// ```text
/// Offsets are relative to the section base `85 + S`; the aggregate and role-`0x08` group references lie at offsets 118 and 145 after the third owner slot.
/// ```
pub(crate) mod hem_teardrop_fixed_operation_section {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 89;
    /// Offset of `gap_owner_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const GAP_OWNER_REFERENCE: usize = 42;
    /// Offset of `length_owner_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const LENGTH_OWNER_REFERENCE: usize = 53;
    /// Offset of `radius_owner_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const RADIUS_OWNER_REFERENCE: usize = 64;
    /// Offset of `inside_bend_radius` (`f64`, little-endian). Spec §3.1.
    pub(crate) const INSIDE_BEND_RADIUS: usize = 81;
}

/// Byte offsets for the `move_transform_frame_253` record.
///
/// Spec §3.1. Record length 253 B.
///
/// ```text
/// Offsets are relative to the transform record's primary indexed header. The class tags are the admission discriminator; the class-447 form uses paired class 263; the same-index paired header follows at offset 253.
/// ```
pub(crate) mod move_transform_frame_253 {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 253;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `form` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FORM: usize = 43;
    /// Offset of `reserved_zero` (`u8`). Spec §3.1.
    pub(crate) const RESERVED_ZERO: usize = 47;
    /// Offset of `transform` (`f64[16]`, little-endian). Spec §3.1.
    pub(crate) const TRANSFORM: usize = 48;
}

/// Byte offsets for the `legacy_body_group_frame_123` record.
///
/// Spec §3.1. Record length 123 B.
///
/// ```text
/// Offsets are relative to the primary indexed header for the ordinary one-member, two-null-auxiliary, one-trailing-reference envelope. Primary/paired classes are 257/262, 323/262, 328/263, 338/261, 282/262, and 302/258; the tail discriminants are 01 01, 01 01, 01 01, 01 01, 00 01, and 00 01 respectively. The class-328 Move variant remains 123 bytes but uses a null auxiliary reference at +36, a present auxiliary reference to N+13 at +37, trailing count zero at +48, and a retained null trailing-slot byte at +52 before the role at +53; its tail has byte 0 at +98, discriminant 01 01 at +99, an unmarked N+1 reference at +101, and the owning-scope reference at +112. It has no counted trailing-reference run.
/// ```
pub(crate) mod legacy_body_group_frame_123 {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 123;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `member_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const MEMBER_COUNT: usize = 21;
    /// Offset of `member_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const MEMBER_REFERENCE: usize = 25;
    /// Offset of `role` (`u64`, little-endian). Spec §3.1.
    pub(crate) const ROLE: usize = 53;
    /// Offset of `opaque_ordinal` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OPAQUE_ORDINAL: usize = 71;
    /// Offset of `opaque_scalar` (`f64`, little-endian). Spec §3.1.
    pub(crate) const OPAQUE_SCALAR: usize = 75;
    /// Offset of `repeated_ordinal` (`u32`, little-endian). Spec §3.1.
    pub(crate) const REPEATED_ORDINAL: usize = 83;
    /// Offset of `n_plus_2_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const N_PLUS_2_REFERENCE: usize = 87;
    /// Offset of `tail_discriminant` (`bytes[2]`). Spec §3.1.
    pub(crate) const TAIL_DISCRIMINANT: usize = 98;
    /// Offset of `n_plus_1_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const N_PLUS_1_REFERENCE: usize = 100;
    /// Offset of `tail_zero` (`u8`). Spec §3.1.
    pub(crate) const TAIL_ZERO: usize = 111;
    /// Offset of `owning_scope_reference` (`bytes[11]`). Spec §3.1.
    pub(crate) const OWNING_SCOPE_REFERENCE: usize = 112;
}

/// Byte offsets for the `component_insert_identity_scope_296_263` record.
///
/// Spec §3.1. Record length 261 B.
///
/// ```text
/// Offsets are relative to the primary class-296 indexed header. The paired class-263 indexed header begins at offset 261.
/// ```
pub(crate) mod component_insert_identity_scope_296_263 {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 261;
    /// Offset of `indexed_header` (`bytes[11]`). Spec §3.1.
    pub(crate) const INDEXED_HEADER: usize = 0;
    /// Offset of `prologue_marker` (`u8`). Spec §3.1.
    pub(crate) const PROLOGUE_MARKER: usize = 20;
    /// Offset of `prologue_value` (`u32`, little-endian). Spec §3.1.
    pub(crate) const PROLOGUE_VALUE: usize = 21;
    /// Offset of `occurrence_identity` (`u64`, little-endian). Spec §3.1.
    pub(crate) const OCCURRENCE_IDENTITY: usize = 25;
    /// Offset of `relation_marker` (`u8`). Spec §3.1.
    pub(crate) const RELATION_MARKER: usize = 37;
    /// Offset of `relation_record_index` (`u32`, little-endian). Spec §3.1.
    pub(crate) const RELATION_RECORD_INDEX: usize = 38;
    /// Offset of `identity_markers` (`bytes[2]`). Spec §3.1.
    pub(crate) const IDENTITY_MARKERS: usize = 48;
    /// Offset of `opaque_code_unit_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OPAQUE_CODE_UNIT_COUNT: usize = 50;
    /// Offset of `opaque_utf16_payload` (`bytes[72]`). Spec §3.1.
    pub(crate) const OPAQUE_UTF16_PAYLOAD: usize = 54;
}

/// Byte offsets for the `component_insert_grouped_identity_carrier_382` record.
///
/// Spec §3.1. Record length 695 B.
///
/// ```text
/// Offsets are relative to the primary class-382 grouped identity carrier header. The relation header begins at offset 695. Every GUID field has 36 code units or ASCII bytes.
/// ```
pub(crate) mod component_insert_grouped_identity_carrier_382 {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 695;
    /// Offset of `carrier_marker` (`u8`). Spec §3.1.
    pub(crate) const CARRIER_MARKER: usize = 19;
    /// Offset of `carrier_value` (`u32`, little-endian). Spec §3.1.
    pub(crate) const CARRIER_VALUE: usize = 20;
    /// Offset of `identity_marker` (`u8`). Spec §3.1.
    pub(crate) const IDENTITY_MARKER: usize = 24;
    /// Offset of `occurrence_identity` (`u64`, little-endian). Spec §3.1.
    pub(crate) const OCCURRENCE_IDENTITY: usize = 25;
    /// Offset of `reference_marker` (`u8`). Spec §3.1.
    pub(crate) const REFERENCE_MARKER: usize = 33;
    /// Offset of `first_component_guid` (`bytes[76]`). Spec §3.1.
    pub(crate) const FIRST_COMPONENT_GUID: usize = 38;
    /// Offset of `first_component_separator` (`u8`). Spec §3.1.
    pub(crate) const FIRST_COMPONENT_SEPARATOR: usize = 114;
    /// Offset of `first_type_guid` (`bytes[40]`). Spec §3.1.
    pub(crate) const FIRST_TYPE_GUID: usize = 115;
    /// Offset of `first_role_guid` (`bytes[76]`). Spec §3.1.
    pub(crate) const FIRST_ROLE_GUID: usize = 155;
    /// Offset of `metadata_marker` (`bytes[10]`). Spec §3.1.
    pub(crate) const METADATA_MARKER: usize = 231;
    /// Offset of `metadata_guid_a` (`bytes[76]`). Spec §3.1.
    pub(crate) const METADATA_GUID_A: usize = 241;
    /// Offset of `metadata_guid_b` (`bytes[76]`). Spec §3.1.
    pub(crate) const METADATA_GUID_B: usize = 317;
    /// Offset of `placement_marker` (`bytes[15]`). Spec §3.1.
    pub(crate) const PLACEMENT_MARKER: usize = 393;
    /// Offset of `repeated_component_guid` (`bytes[76]`). Spec §3.1.
    pub(crate) const REPEATED_COMPONENT_GUID: usize = 408;
    /// Offset of `repeated_component_separator` (`u8`). Spec §3.1.
    pub(crate) const REPEATED_COMPONENT_SEPARATOR: usize = 484;
    /// Offset of `repeated_type_guid` (`bytes[40]`). Spec §3.1.
    pub(crate) const REPEATED_TYPE_GUID: usize = 485;
    /// Offset of `repeated_role_guid` (`bytes[76]`). Spec §3.1.
    pub(crate) const REPEATED_ROLE_GUID: usize = 525;
    /// Offset of `construction_marker` (`bytes[6]`). Spec §3.1.
    pub(crate) const CONSTRUCTION_MARKER: usize = 601;
    /// Offset of `final_role_guid` (`bytes[76]`). Spec §3.1.
    pub(crate) const FINAL_ROLE_GUID: usize = 607;
    /// Offset of `closure` (`bytes[12]`). Spec §3.1.
    pub(crate) const CLOSURE: usize = 683;
}
