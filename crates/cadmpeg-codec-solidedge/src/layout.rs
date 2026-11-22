// SPDX-License-Identifier: Apache-2.0
//! Byte-offset and value constants generated from `docs/layouts/solidedge.toml`.
//!
//! Do not edit by hand. Regenerate with:
//! `UPDATE_LAYOUT_CODE=1 cargo test -p cadmpeg-layout-tables`.

/// Byte offsets for the `property_set_header` record.
///
/// Spec §2.1. Record length 16 B.
pub(crate) mod property_set_header {
    /// Record length in bytes. Spec §2.1.
    pub(crate) const LEN: usize = 16;
    /// Stated value of `tag` (`u16`). Spec §2.1.
    pub(crate) const TAG_VALUE: u16 = 0x5151;
    /// Offset of `index` (`u32`, little-endian). Spec §2.1.
    pub(crate) const INDEX: usize = 8;
    /// Offset of `count` (`u32`, little-endian). Spec §2.1.
    pub(crate) const COUNT: usize = 12;
}

/// Byte offsets for the `space_map_header` record.
///
/// Spec §3.1. Record length 12 B.
pub(crate) mod space_map_header {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 12;
    /// Stated value of `magic` (`bytes[4]`). Spec §3.1.
    pub(crate) const MAGIC_VALUE: [u8; 4] = *b"sseg";
    /// Offset of `item_count` (`u16`, little-endian). Spec §3.1.
    pub(crate) const ITEM_COUNT: usize = 4;
    /// Offset of `free_count` (`u16`, little-endian). Spec §3.1.
    pub(crate) const FREE_COUNT: usize = 10;
}

/// Byte offsets for the `space_map_item` record.
///
/// Spec §3.1. Record length 18 B.
pub(crate) mod space_map_item {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 18;
    /// Offset of `length` (`u16`, little-endian). Spec §3.1.
    pub(crate) const LENGTH: usize = 0;
    /// Offset of `object_offset` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OBJECT_OFFSET: usize = 2;
    /// Offset of `object_id` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OBJECT_ID: usize = 6;
    /// Offset of `form_word` (`u16`, little-endian). Spec §3.1.
    pub(crate) const FORM_WORD: usize = 10;
    /// Offset of `flags` (`u16`, little-endian). Spec §3.1.
    pub(crate) const FLAGS: usize = 16;
}

/// Byte offsets for the `cluster_header` record.
///
/// Spec §3.1. Record length 8 B.
pub(crate) mod cluster_header {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 8;
    /// Offset of `magic` (`u32`, little-endian). Spec §3.1.
    pub(crate) const MAGIC: usize = 0;
    /// Stated value of `magic` (`u32`). Spec §3.1.
    pub(crate) const MAGIC_VALUE: u32 = 0x6c90_f544;
    /// Offset of `object_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OBJECT_COUNT: usize = 4;
}

/// Byte offsets for the `object_frame` record.
///
/// Spec §3.1. Record length 8 B.
pub(crate) mod object_frame {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 8;
    /// Offset of `tag` (`u16`, little-endian). Spec §3.1.
    pub(crate) const TAG: usize = 0;
    /// Stated value of `tag` (`u16`). Spec §3.1.
    pub(crate) const TAG_VALUE: u16 = 0x5151;
    /// Offset of `form` (`u16`, little-endian). Spec §3.1.
    pub(crate) const FORM: usize = 2;
    /// Offset of `object_id` (`u32`, little-endian). Spec §3.1.
    pub(crate) const OBJECT_ID: usize = 4;
}

/// Byte offsets for the `type_table_section_header` record.
///
/// Spec §3.2. Record length 20 B.
pub(crate) mod type_table_section_header {
    /// Record length in bytes. Spec §3.2.
    pub(crate) const LEN: usize = 20;
    /// Stated value of `magic` (`bytes[4]`). Spec §3.2.
    pub(crate) const MAGIC_VALUE: [u8; 4] = *b"ePyT";
    /// Offset of `count` (`u16`, little-endian). Spec §3.2.
    pub(crate) const COUNT: usize = 10;
    /// Offset of `count_repeat` (`u16`, little-endian). Spec §3.2.
    pub(crate) const COUNT_REPEAT: usize = 14;
}

/// Byte offsets for the `type_declaration` record.
///
/// Spec §3.2. Record length 30 B.
pub(crate) mod type_declaration {
    /// Record length in bytes. Spec §3.2.
    pub(crate) const LEN: usize = 30;
    /// Offset of `index` (`u32`, little-endian). Spec §3.2.
    pub(crate) const INDEX: usize = 16;
    /// Offset of `marker_one` (`u16`, little-endian). Spec §3.2.
    pub(crate) const MARKER_ONE: usize = 20;
    /// Stated value of `marker_one` (`u16`). Spec §3.2.
    pub(crate) const MARKER_ONE_VALUE: u16 = 0x0001;
    /// Offset of `marker_absent` (`u16`, little-endian). Spec §3.2.
    pub(crate) const MARKER_ABSENT: usize = 22;
    /// Stated value of `marker_absent` (`u16`). Spec §3.2.
    pub(crate) const MARKER_ABSENT_VALUE: u16 = 0xffff;
    /// Offset of `name_len` (`u16`, little-endian). Spec §3.2.
    pub(crate) const NAME_LEN: usize = 28;
}

/// Byte offsets for the `type_compact_record` record.
///
/// Spec §3.2. Record length 26 B.
pub(crate) mod type_compact_record {
    /// Record length in bytes. Spec §3.2.
    pub(crate) const LEN: usize = 26;
    /// Offset of `link` (`u16`, little-endian). Spec §3.2.
    pub(crate) const LINK: usize = 18;
}

/// Byte offsets for the `part_feature_object` record.
///
/// Spec §4.3. Record length 12 B.
pub(crate) mod part_feature_object {
    /// Offset of `state` (`u32`, little-endian). Spec §4.3.
    pub(crate) const STATE: usize = 8;
}

/// Byte offsets for the `folder_object` record.
///
/// Spec §4.3. Record length 12 B.
pub(crate) mod folder_object {
    /// Offset of `member_count` (`u32`, little-endian). Spec §4.3.
    pub(crate) const MEMBER_COUNT: usize = 8;
}
