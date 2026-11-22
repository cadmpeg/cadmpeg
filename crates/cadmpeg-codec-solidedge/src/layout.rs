// SPDX-License-Identifier: Apache-2.0
//! Byte-offset and value constants generated from `docs/layouts/solidedge.toml`.
//!
//! Do not edit by hand. Regenerate with:
//! `UPDATE_LAYOUT_CODE=1 cargo test -p cadmpeg --test layout_tables`.

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

/// Byte offsets for the `profile` record.
///
/// Spec §5.1. Record length 304 B.
pub(crate) mod profile {
    /// Offset of `origin` (`f64[3]`, little-endian). Spec §5.1.
    pub(crate) const ORIGIN: usize = 26;
    /// Offset of `normal` (`f64[3]`, little-endian). Spec §5.1.
    pub(crate) const NORMAL: usize = 50;
    /// Offset of `x_axis` (`f64[3]`, little-endian). Spec §5.1.
    pub(crate) const X_AXIS: usize = 74;
    /// Offset of `profile_2d` (`u32`, little-endian). Spec §5.1.
    pub(crate) const PROFILE_2D: usize = 127;
}

/// Byte offsets for the `profile_2d` record.
///
/// Spec §5.1. Record length 16 B.
pub(crate) mod profile_2d {
    /// Offset of `member_count` (`u32`, little-endian). Spec §5.1.
    pub(crate) const MEMBER_COUNT: usize = 8;
    /// Offset of `first_member` (`u32`, little-endian). Spec §5.1.
    pub(crate) const FIRST_MEMBER: usize = 12;
}

/// Byte offsets for the `line_2d` record.
///
/// Spec §5.2. Record length 42 B.
pub(crate) mod line_2d {
    /// Offset of `x0` (`f64`, little-endian). Spec §5.2.
    pub(crate) const X0: usize = 10;
    /// Offset of `x1` (`f64`, little-endian). Spec §5.2.
    pub(crate) const X1: usize = 26;
}

/// Byte offsets for the `circle_2d` record.
///
/// Spec §5.2. Record length 34 B.
pub(crate) mod circle_2d {
    /// Offset of `center_x` (`f64`, little-endian). Spec §5.2.
    pub(crate) const CENTER_X: usize = 10;
    /// Offset of `radius` (`f64`, little-endian). Spec §5.2.
    pub(crate) const RADIUS: usize = 26;
}

/// Byte offsets for the `arc_2d` record.
///
/// Spec §5.2. Record length 51 B.
pub(crate) mod arc_2d {
    /// Offset of `center_x` (`f64`, little-endian). Spec §5.2.
    pub(crate) const CENTER_X: usize = 10;
    /// Offset of `radius` (`f64`, little-endian). Spec §5.2.
    pub(crate) const RADIUS: usize = 26;
    /// Offset of `start_angle` (`f64`, little-endian). Spec §5.2.
    pub(crate) const START_ANGLE: usize = 35;
    /// Offset of `end_angle` (`f64`, little-endian). Spec §5.2.
    pub(crate) const END_ANGLE: usize = 43;
}

/// Byte offsets for the `double_value` record.
///
/// Spec §6.1. Record length 16 B.
pub(crate) mod double_value {
    /// Offset of `value` (`f64`, little-endian). Spec §6.1.
    pub(crate) const VALUE: usize = 8;
}

/// Byte offsets for the `dimension` record.
///
/// Spec §7.1. Record length 210 B.
pub(crate) mod dimension {
    /// Offset of `marker` (`u32`, little-endian). Spec §7.1.
    pub(crate) const MARKER: usize = 144;
    /// Stated value of `marker` (`u32`). Spec §7.1.
    pub(crate) const MARKER_VALUE: u32 = 0x0000_0001;
    /// Offset of `target` (`u32`, little-endian). Spec §7.1.
    pub(crate) const TARGET: usize = 148;
    /// Offset of `target_repeat` (`u32`, little-endian). Spec §7.1.
    pub(crate) const TARGET_REPEAT: usize = 198;
    /// Offset of `value` (`f64`, little-endian). Spec §7.1.
    pub(crate) const VALUE: usize = 202;
}

/// Byte offsets for the `dimension_compact` record.
///
/// Spec §7.1. Record length 206 B.
pub(crate) mod dimension_compact {
    /// Offset of `marker` (`u32`, little-endian). Spec §7.1.
    pub(crate) const MARKER: usize = 140;
    /// Stated value of `marker` (`u32`). Spec §7.1.
    pub(crate) const MARKER_VALUE: u32 = 0x0000_0001;
    /// Offset of `target` (`u32`, little-endian). Spec §7.1.
    pub(crate) const TARGET: usize = 144;
    /// Offset of `target_repeat` (`u32`, little-endian). Spec §7.1.
    pub(crate) const TARGET_REPEAT: usize = 194;
    /// Offset of `value` (`f64`, little-endian). Spec §7.1.
    pub(crate) const VALUE: usize = 198;
}

/// Byte offsets for the `hole_prop_data` record.
///
/// Spec §7.2. Record length 80 B.
pub(crate) mod hole_prop_data {
    /// Offset of `hole_type` (`u32`, little-endian). Spec §7.2.
    pub(crate) const HOLE_TYPE: usize = 8;
    /// Offset of `diameter` (`f64`, little-endian). Spec §7.2.
    pub(crate) const DIAMETER: usize = 12;
    /// Offset of `head_diameter` (`f64`, little-endian). Spec §7.2.
    pub(crate) const HEAD_DIAMETER: usize = 20;
    /// Offset of `head_depth` (`f64`, little-endian). Spec §7.2.
    pub(crate) const HEAD_DEPTH: usize = 28;
    /// Offset of `sink_angle` (`f64`, little-endian). Spec §7.2.
    pub(crate) const SINK_ANGLE: usize = 36;
    /// Offset of `depth` (`f64`, little-endian). Spec §7.2.
    pub(crate) const DEPTH: usize = 72;
}

/// Byte offsets for the `hole_2d` record.
///
/// Spec §7.2. Record length 24 B.
pub(crate) mod hole_2d {
    /// Offset of `x` (`f64`, little-endian). Spec §7.2.
    pub(crate) const X: usize = 8;
    /// Offset of `y` (`f64`, little-endian). Spec §7.2.
    pub(crate) const Y: usize = 16;
}

/// Byte offsets for the `ref_axis` record.
///
/// Spec §6.4. Record length 61 B.
pub(crate) mod ref_axis {
    /// Offset of `marker` (`u8`). Spec §6.4.
    pub(crate) const MARKER: usize = 12;
    /// Stated value of `marker` (`u8`). Spec §6.4.
    pub(crate) const MARKER_VALUE: u8 = 3;
    /// Offset of `point` (`f64[3]`, little-endian). Spec §6.4.
    pub(crate) const POINT: usize = 13;
    /// Offset of `direction` (`f64[3]`, little-endian). Spec §6.4.
    pub(crate) const DIRECTION: usize = 37;
}

/// Byte offsets for the `pattern_group` record.
///
/// Spec §8.1. Record length 341 B.
pub(crate) mod pattern_group {
    /// Offset of `fast_pattern_part` (`u32`, little-endian). Spec §8.1.
    pub(crate) const FAST_PATTERN_PART: usize = 106;
    /// Offset of `seed` (`u32`, little-endian). Spec §8.1.
    pub(crate) const SEED: usize = 337;
}

/// Byte offsets for the `mirror_group` record.
///
/// Spec §8.2. Record length 333 B.
pub(crate) mod mirror_group {
    /// Offset of `seed_count` (`u32`, little-endian). Spec §8.2.
    pub(crate) const SEED_COUNT: usize = 92;
    /// Offset of `first_seed` (`u32`, little-endian). Spec §8.2.
    pub(crate) const FIRST_SEED: usize = 96;
    /// Offset of `plane` (`u32`, little-endian). Spec §8.2.
    pub(crate) const PLANE: usize = 329;
}

/// Byte offsets for the `reference_plane` record.
///
/// Spec §8.2. Record length 61 B.
pub(crate) mod reference_plane {
    /// Offset of `point` (`f64[3]`, little-endian). Spec §8.2.
    pub(crate) const POINT: usize = 13;
    /// Offset of `normal` (`f64[3]`, little-endian). Spec §8.2.
    pub(crate) const NORMAL: usize = 37;
}

/// Byte offsets for the `references_collection` record.
///
/// Spec §9.1. Record length 20 B.
pub(crate) mod references_collection {
    /// Offset of `count` (`u32`, little-endian). Spec §9.1.
    pub(crate) const COUNT: usize = 16;
}

/// Byte offsets for the `assembly_reference` record.
///
/// Spec §9.2. Record length 229 B.
pub(crate) mod assembly_reference {
    /// Record length in bytes. Spec §9.2.
    pub(crate) const LEN: usize = 229;
    /// Offset of `state` (`u32`, little-endian). Spec §9.2.
    pub(crate) const STATE: usize = 16;
    /// Offset of `site` (`u32`, little-endian). Spec §9.2.
    pub(crate) const SITE: usize = 113;
    /// Offset of `rotation` (`f64[9]`, little-endian). Spec §9.2.
    pub(crate) const ROTATION: usize = 125;
    /// Offset of `translation` (`f64[3]`, little-endian). Spec §9.2.
    pub(crate) const TRANSLATION: usize = 197;
    /// Offset of `unit` (`f64`, little-endian). Spec §9.2.
    pub(crate) const UNIT: usize = 221;
}

/// Byte offsets for the `file_moniker` record.
///
/// Spec §9.3. Record length 22 B.
pub(crate) mod file_moniker {
    /// Record length in bytes. Spec §9.3.
    pub(crate) const LEN: usize = 22;
    /// Stated value of `clsid` (`bytes[16]`). Spec §9.3.
    pub(crate) const CLSID_VALUE: [u8; 16] = [
        0x03, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x46,
    ];
    /// Offset of `up_level` (`u16`, little-endian). Spec §9.3.
    pub(crate) const UP_LEVEL: usize = 16;
    /// Offset of `path_len` (`u32`, little-endian). Spec §9.3.
    pub(crate) const PATH_LEN: usize = 18;
}
