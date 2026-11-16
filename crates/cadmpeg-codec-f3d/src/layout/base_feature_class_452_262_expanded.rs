// SPDX-License-Identifier: Apache-2.0
/// Record length in bytes. Spec §3.1.
pub(crate) const LEN: usize = 395;
/// Offset of `zero_run_8` (`bytes[8]`). Spec §3.1.
pub(crate) const ZERO_RUN_8: usize = 11;
/// Offset of `body_count_marker` (`u8`). Spec §3.1.
pub(crate) const BODY_COUNT_MARKER: usize = 19;
/// Stated value of `body_count_marker` (`u8`). Spec §3.1.
pub(crate) const BODY_COUNT_MARKER_VALUE: u8 = 1;
/// Offset of `body_count` (`u32`, little-endian). Spec §3.1.
pub(crate) const BODY_COUNT: usize = 20;
/// Stated value of `body_count` (`u32`). Spec §3.1.
pub(crate) const BODY_COUNT_VALUE: u32 = 0x0000_0002;
/// Offset of `body_entity_one_marker` (`u8`). Spec §3.1.
pub(crate) const BODY_ENTITY_ONE_MARKER: usize = 24;
/// Stated value of `body_entity_one_marker` (`u8`). Spec §3.1.
pub(crate) const BODY_ENTITY_ONE_MARKER_VALUE: u8 = 1;
/// Offset of `body_entity_one_suffix` (`u64`, little-endian). Spec §3.1.
pub(crate) const BODY_ENTITY_ONE_SUFFIX: usize = 25;
/// Offset of `body_entity_one_field` (`bytes[6]`). Spec §3.1.
pub(crate) const BODY_ENTITY_ONE_FIELD: usize = 33;
/// Offset of `body_entity_two_marker` (`u8`). Spec §3.1.
pub(crate) const BODY_ENTITY_TWO_MARKER: usize = 39;
/// Stated value of `body_entity_two_marker` (`u8`). Spec §3.1.
pub(crate) const BODY_ENTITY_TWO_MARKER_VALUE: u8 = 1;
/// Offset of `body_entity_two_suffix` (`u64`, little-endian). Spec §3.1.
pub(crate) const BODY_ENTITY_TWO_SUFFIX: usize = 40;
/// Offset of `body_entity_two_field` (`bytes[6]`). Spec §3.1.
pub(crate) const BODY_ENTITY_TWO_FIELD: usize = 48;
/// Offset of `tag_body_based_on_faces_marker` (`u8`). Spec §3.1.
pub(crate) const TAG_BODY_BASED_ON_FACES_MARKER: usize = 54;
/// Offset of `tag_body_based_on_faces_value` (`u16`, little-endian). Spec §3.1.
pub(crate) const TAG_BODY_BASED_ON_FACES_VALUE: usize = 107;
/// Offset of `parameter_body_group_marker` (`u8`). Spec §3.1.
pub(crate) const PARAMETER_BODY_GROUP_MARKER: usize = 109;
/// Stated value of `parameter_body_group_marker` (`u8`). Spec §3.1.
pub(crate) const PARAMETER_BODY_GROUP_MARKER_VALUE: u8 = 1;
/// Offset of `parameter_body_count` (`u32`, little-endian). Spec §3.1.
pub(crate) const PARAMETER_BODY_COUNT: usize = 110;
/// Stated value of `parameter_body_count` (`u32`). Spec §3.1.
pub(crate) const PARAMETER_BODY_COUNT_VALUE: u32 = 0x0000_0002;
/// Offset of `parameter_body_one_marker` (`u8`). Spec §3.1.
pub(crate) const PARAMETER_BODY_ONE_MARKER: usize = 114;
/// Offset of `parameter_body_one_record` (`u32`, little-endian). Spec §3.1.
pub(crate) const PARAMETER_BODY_ONE_RECORD: usize = 115;
/// Offset of `parameter_body_one_field` (`bytes[6]`). Spec §3.1.
pub(crate) const PARAMETER_BODY_ONE_FIELD: usize = 119;
/// Offset of `parameter_body_two_marker` (`u8`). Spec §3.1.
pub(crate) const PARAMETER_BODY_TWO_MARKER: usize = 125;
/// Offset of `parameter_body_two_record` (`u32`, little-endian). Spec §3.1.
pub(crate) const PARAMETER_BODY_TWO_RECORD: usize = 126;
/// Offset of `parameter_body_two_field` (`bytes[6]`). Spec §3.1.
pub(crate) const PARAMETER_BODY_TWO_FIELD: usize = 130;
/// Offset of `parameter_body_separator` (`u8`). Spec §3.1.
pub(crate) const PARAMETER_BODY_SEPARATOR: usize = 136;
/// Stated value of `parameter_body_separator` (`u8`). Spec §3.1.
pub(crate) const PARAMETER_BODY_SEPARATOR_VALUE: u8 = 0;
/// Offset of `scope_reference_marker` (`u8`). Spec §3.1.
pub(crate) const SCOPE_REFERENCE_MARKER: usize = 137;
/// Offset of `scope_reference` (`u32`, little-endian). Spec §3.1.
pub(crate) const SCOPE_REFERENCE: usize = 138;
/// Offset of `scope_reference_field` (`bytes[6]`). Spec §3.1.
pub(crate) const SCOPE_REFERENCE_FIELD: usize = 142;
/// Offset of `auxiliary_body_count` (`u32`, little-endian). Spec §3.1.
pub(crate) const AUXILIARY_BODY_COUNT: usize = 148;
/// Stated value of `auxiliary_body_count` (`u32`). Spec §3.1.
pub(crate) const AUXILIARY_BODY_COUNT_VALUE: u32 = 0x0000_0002;
/// Offset of `auxiliary_body_one_marker` (`u8`). Spec §3.1.
pub(crate) const AUXILIARY_BODY_ONE_MARKER: usize = 152;
/// Offset of `auxiliary_body_one_record` (`u32`, little-endian). Spec §3.1.
pub(crate) const AUXILIARY_BODY_ONE_RECORD: usize = 153;
/// Offset of `auxiliary_body_one_field` (`bytes[6]`). Spec §3.1.
pub(crate) const AUXILIARY_BODY_ONE_FIELD: usize = 157;
/// Offset of `auxiliary_body_two_marker` (`u8`). Spec §3.1.
pub(crate) const AUXILIARY_BODY_TWO_MARKER: usize = 163;
/// Offset of `auxiliary_body_two_record` (`u32`, little-endian). Spec §3.1.
pub(crate) const AUXILIARY_BODY_TWO_RECORD: usize = 164;
/// Offset of `auxiliary_body_two_field` (`bytes[6]`). Spec §3.1.
pub(crate) const AUXILIARY_BODY_TWO_FIELD: usize = 168;
/// Offset of `auxiliary_body_zero_run` (`bytes[8]`). Spec §3.1.
pub(crate) const AUXILIARY_BODY_ZERO_RUN: usize = 174;
/// Offset of `envelope_guid_code_unit_count` (`u32`, little-endian). Spec §3.1.
pub(crate) const ENVELOPE_GUID_CODE_UNIT_COUNT: usize = 182;
/// Offset of `envelope_guid` (`bytes[72]`). Spec §3.1.
pub(crate) const ENVELOPE_GUID: usize = 186;
/// Offset of `zero_run_after_guid` (`bytes[3]`). Spec §3.1.
pub(crate) const ZERO_RUN_AFTER_GUID: usize = 258;
/// Offset of `reference_count` (`u32`, little-endian). Spec §3.1.
pub(crate) const REFERENCE_COUNT: usize = 261;
/// Offset of `generic_scope_reference_marker` (`u8`). Spec §3.1.
pub(crate) const GENERIC_SCOPE_REFERENCE_MARKER: usize = 265;
/// Offset of `generic_scope_reference_record` (`u32`, little-endian). Spec §3.1.
pub(crate) const GENERIC_SCOPE_REFERENCE_RECORD: usize = 266;
/// Offset of `generic_scope_reference_field` (`bytes[6]`). Spec §3.1.
pub(crate) const GENERIC_SCOPE_REFERENCE_FIELD: usize = 270;
/// Offset of `history_state_id` (`u32`, little-endian). Spec §3.1.
pub(crate) const HISTORY_STATE_ID: usize = 276;
/// Offset of `kind_length` (`u32`, little-endian). Spec §3.1.
pub(crate) const KIND_LENGTH: usize = 280;
/// Offset of `kind` (`bytes[24]`). Spec §3.1.
pub(crate) const KIND: usize = 284;
/// Offset of `feature_ordinal` (`u32`, little-endian). Spec §3.1.
pub(crate) const FEATURE_ORDINAL: usize = 308;
/// Offset of `previous_history_state_id` (`u32`, little-endian). Spec §3.1.
pub(crate) const PREVIOUS_HISTORY_STATE_ID: usize = 349;
