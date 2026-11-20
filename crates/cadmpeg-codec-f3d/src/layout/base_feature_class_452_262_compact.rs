// SPDX-License-Identifier: Apache-2.0
/// Record length in bytes. Spec §3.1.
pub(crate) const LEN: usize = 348;
/// Offset of `zero_run_8` (`bytes[8]`). Spec §3.1.
pub(crate) const ZERO_RUN_8: usize = 11;
/// Offset of `body_count_marker` (`u8`). Spec §3.1.
pub(crate) const BODY_COUNT_MARKER: usize = 19;
/// Stated value of `body_count_marker` (`u8`). Spec §3.1.
pub(crate) const BODY_COUNT_MARKER_VALUE: u8 = 1;
/// Offset of `body_count` (`u32`, little-endian). Spec §3.1.
pub(crate) const BODY_COUNT: usize = 20;
/// Stated value of `body_count` (`u32`). Spec §3.1.
pub(crate) const BODY_COUNT_VALUE: u32 = 0x0000_0001;
/// Offset of `body_entity_reference_marker` (`u8`). Spec §3.1.
pub(crate) const BODY_ENTITY_REFERENCE_MARKER: usize = 24;
/// Stated value of `body_entity_reference_marker` (`u8`). Spec §3.1.
pub(crate) const BODY_ENTITY_REFERENCE_MARKER_VALUE: u8 = 1;
/// Offset of `body_entity_suffix` (`u64`, little-endian). Spec §3.1.
pub(crate) const BODY_ENTITY_SUFFIX: usize = 25;
/// Offset of `body_entity_reference_field` (`bytes[6]`). Spec §3.1.
pub(crate) const BODY_ENTITY_REFERENCE_FIELD: usize = 33;
/// Offset of `tag_body_based_on_faces_marker` (`u8`). Spec §3.1.
pub(crate) const TAG_BODY_BASED_ON_FACES_MARKER: usize = 39;
/// Stated value of `tag_body_based_on_faces_marker` (`u8`). Spec §3.1.
#[cfg(test)]
pub(crate) const TAG_BODY_BASED_ON_FACES_MARKER_VALUE: u8 = 1;
/// Offset of `tag_body_based_on_faces_value` (`u16`, little-endian). Spec §3.1.
pub(crate) const TAG_BODY_BASED_ON_FACES_VALUE: usize = 92;
/// Offset of `mode` (`u8`). Spec §3.1.
pub(crate) const MODE: usize = 94;
/// Offset of `parameter_body_count` (`u8`). Spec §3.1.
pub(crate) const PARAMETER_BODY_COUNT: usize = 95;
/// Stated value of `parameter_body_count` (`u8`). Spec §3.1.
pub(crate) const PARAMETER_BODY_COUNT_VALUE: u8 = 1;
/// Offset of `parameter_body_zero_run` (`bytes[3]`). Spec §3.1.
pub(crate) const PARAMETER_BODY_ZERO_RUN: usize = 96;
/// Offset of `parameter_body_reference_marker` (`u8`). Spec §3.1.
pub(crate) const PARAMETER_BODY_REFERENCE_MARKER: usize = 99;
/// Stated value of `parameter_body_reference_marker` (`u8`). Spec §3.1.
pub(crate) const PARAMETER_BODY_REFERENCE_MARKER_VALUE: u8 = 1;
/// Offset of `parameter_body_record` (`u64`, little-endian). Spec §3.1.
pub(crate) const PARAMETER_BODY_RECORD: usize = 100;
/// Offset of `parameter_body_reference_field` (`bytes[3]`). Spec §3.1.
pub(crate) const PARAMETER_BODY_REFERENCE_FIELD: usize = 108;
/// Offset of `scope_reference_marker` (`u8`). Spec §3.1.
pub(crate) const SCOPE_REFERENCE_MARKER: usize = 111;
/// Stated value of `scope_reference_marker` (`u8`). Spec §3.1.
pub(crate) const SCOPE_REFERENCE_MARKER_VALUE: u8 = 1;
/// Offset of `scope_reference` (`u64`, little-endian). Spec §3.1.
pub(crate) const SCOPE_REFERENCE: usize = 112;
/// Offset of `scope_reference_field` (`bytes[2]`). Spec §3.1.
pub(crate) const SCOPE_REFERENCE_FIELD: usize = 120;
/// Offset of `auxiliary_group_marker` (`u8`). Spec §3.1.
pub(crate) const AUXILIARY_GROUP_MARKER: usize = 122;
/// Stated value of `auxiliary_group_marker` (`u8`). Spec §3.1.
pub(crate) const AUXILIARY_GROUP_MARKER_VALUE: u8 = 1;
/// Offset of `auxiliary_group_zero_run` (`bytes[3]`). Spec §3.1.
pub(crate) const AUXILIARY_GROUP_ZERO_RUN: usize = 123;
/// Offset of `auxiliary_reference_marker` (`u8`). Spec §3.1.
pub(crate) const AUXILIARY_REFERENCE_MARKER: usize = 126;
/// Stated value of `auxiliary_reference_marker` (`u8`). Spec §3.1.
pub(crate) const AUXILIARY_REFERENCE_MARKER_VALUE: u8 = 1;
/// Offset of `auxiliary_record` (`u64`, little-endian). Spec §3.1.
pub(crate) const AUXILIARY_RECORD: usize = 127;
/// Offset of `auxiliary_reference_field` (`bytes[10]`). Spec §3.1.
pub(crate) const AUXILIARY_REFERENCE_FIELD: usize = 135;
/// Offset of `envelope_guid_code_unit_count` (`u32`, little-endian). Spec §3.1.
pub(crate) const ENVELOPE_GUID_CODE_UNIT_COUNT: usize = 145;
/// Offset of `envelope_guid` (`bytes[72]`). Spec §3.1.
pub(crate) const ENVELOPE_GUID: usize = 149;
/// Offset of `zero_run_after_guid` (`bytes[3]`). Spec §3.1.
pub(crate) const ZERO_RUN_AFTER_GUID: usize = 221;
/// Offset of `reference_count` (`u32`, little-endian). Spec §3.1.
pub(crate) const REFERENCE_COUNT: usize = 224;
/// Offset of `generic_scope_reference_marker` (`u8`). Spec §3.1.
pub(crate) const GENERIC_SCOPE_REFERENCE_MARKER: usize = 228;
/// Offset of `generic_scope_reference_record` (`u32`, little-endian). Spec §3.1.
pub(crate) const GENERIC_SCOPE_REFERENCE_RECORD: usize = 229;
/// Offset of `generic_scope_reference_field` (`bytes[6]`). Spec §3.1.
pub(crate) const GENERIC_SCOPE_REFERENCE_FIELD: usize = 233;
/// Offset of `history_state_id` (`u32`, little-endian). Spec §3.1.
pub(crate) const HISTORY_STATE_ID: usize = 239;
/// Offset of `kind_length` (`u32`, little-endian). Spec §3.1.
pub(crate) const KIND_LENGTH: usize = 243;
/// Offset of `kind` (`bytes[24]`). Spec §3.1.
pub(crate) const KIND: usize = 247;
/// Offset of `feature_ordinal` (`u32`, little-endian). Spec §3.1.
pub(crate) const FEATURE_ORDINAL: usize = 271;
/// Offset of `previous_history_state_id` (`u32`, little-endian). Spec §3.1.
pub(crate) const PREVIOUS_HISTORY_STATE_ID: usize = 302;
