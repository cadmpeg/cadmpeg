// SPDX-License-Identifier: Apache-2.0
//! Byte-offset and value constants generated from `docs/layouts/creo_view_pvz.toml`.
//!
//! Do not edit by hand. Regenerate with:
//! `UPDATE_LAYOUT_CODE=1 cargo test -p cadmpeg --test layout_tables`.

/// Tag constants from the table inventory.
pub(crate) mod token {
    /// `DESC_U8_LENGTH` (`0x02`). Spec §3.4.
    pub(crate) const DESC_U8_LENGTH: u8 = 2;
    /// `DESC_U16_LENGTH` (`0x04`). Spec §3.4.
    pub(crate) const DESC_U16_LENGTH: u8 = 4;
    /// `DESC_U8_LENGTH_U8_TYPE` (`0x12`). Spec §3.4.
    pub(crate) const DESC_U8_LENGTH_U8_TYPE: u8 = 18;
    /// `DESC_U8_LENGTH_U16_TYPE` (`0x22`). Spec §3.4.
    pub(crate) const DESC_U8_LENGTH_U16_TYPE: u8 = 34;
    /// `DESC_CHILDREN` (`0x80`). Spec §3.4.
    pub(crate) const DESC_CHILDREN: u8 = 128;
    /// `DESC_U8_LENGTH_CHILDREN` (`0x82`). Spec §3.4.
    pub(crate) const DESC_U8_LENGTH_CHILDREN: u8 = 130;
    /// `DESC_U8_LENGTH_U8_TYPE_CHILDREN` (`0x92`). Spec §3.4.
    pub(crate) const DESC_U8_LENGTH_U8_TYPE_CHILDREN: u8 = 146;
    /// `STREAM_ATTRIBUTES` (`0x01000000`). Spec §4.5.
    pub(crate) const STREAM_ATTRIBUTES: u32 = 0x0100_0000;
    /// `STREAM_SURFACE_TESSELLATION` (`0x01000001`). Spec §4.5.
    pub(crate) const STREAM_SURFACE_TESSELLATION: u32 = 0x0100_0001;
    /// `STREAM_EDGE_TESSELLATION` (`0x01000002`). Spec §4.5.
    pub(crate) const STREAM_EDGE_TESSELLATION: u32 = 0x0100_0002;
    /// `STREAM_EXACT_F32` (`0x01000003`). Spec §4.5.
    pub(crate) const STREAM_EXACT_F32: u32 = 0x0100_0003;
    /// `STREAM_EXACT_F64` (`0x01000004`). Spec §4.5.
    pub(crate) const STREAM_EXACT_F64: u32 = 0x0100_0004;
    /// `STREAM_END_SENTINEL` (`0x00ffffff`). Spec §4.5.
    pub(crate) const STREAM_END_SENTINEL: u32 = 0x00ff_ffff;
    /// `ATTR_APPEARANCE_VALUES` (`0x05`). Spec §5.1.
    pub(crate) const ATTR_APPEARANCE_VALUES: u8 = 5;
    /// `ATTR_APPEARANCE_REFERENCE` (`0x07`). Spec §5.2.
    pub(crate) const ATTR_APPEARANCE_REFERENCE: u8 = 7;
    /// `TESS_PATCH_HEADER` (`0x0100000d`). Spec §6.2.
    pub(crate) const TESS_PATCH_HEADER: u32 = 0x0100_000d;
    /// `TESS_VERTEX_BLOCK` (`0x01000016`). Spec §6.3.
    pub(crate) const TESS_VERTEX_BLOCK: u32 = 0x0100_0016;
    /// `TESS_TRIANGLE_FAN` (`0x19`). Spec §6.4.
    pub(crate) const TESS_TRIANGLE_FAN: u8 = 25;
    /// `TESS_TRIANGLE_STRIP` (`0x1a`). Spec §6.4.
    pub(crate) const TESS_TRIANGLE_STRIP: u8 = 26;
    /// `TESS_TRIANGLE_LIST` (`0x1b`). Spec §6.4.
    pub(crate) const TESS_TRIANGLE_LIST: u8 = 27;
    /// `EDGE_TESS_INDEX_TABLE` (`0x0100000f`). Spec §7.1.
    pub(crate) const EDGE_TESS_INDEX_TABLE: u32 = 0x0100_000f;
    /// `TESS_LINE_STRIP` (`0x1c`). Spec §7.2.
    pub(crate) const TESS_LINE_STRIP: u8 = 28;
    /// `TESS_LINE_SEGMENTS` (`0x1d`). Spec §7.2.
    pub(crate) const TESS_LINE_SEGMENTS: u8 = 29;
    /// `EXACT_SURFACE` (`0x13`). Spec §8.2.
    pub(crate) const EXACT_SURFACE: u8 = 19;
    /// `EXACT_LOOP` (`0x01000010`). Spec §8.2.
    pub(crate) const EXACT_LOOP: u32 = 0x0100_0010;
    /// `EXACT_COEDGE` (`0x11`). Spec §8.2.
    pub(crate) const EXACT_COEDGE: u8 = 17;
    /// `EXACT_SINGULAR_USE` (`0x01000012`). Spec §8.2.
    pub(crate) const EXACT_SINGULAR_USE: u32 = 0x0100_0012;
    /// `EXACT_EDGE_CURVE` (`0x14`). Spec §8.2.
    pub(crate) const EXACT_EDGE_CURVE: u8 = 20;
    /// `EXACT_PCURVE` (`0x15`). Spec §8.2.
    pub(crate) const EXACT_PCURVE: u8 = 21;
}

/// Byte offsets for the `pvs_header` record.
///
/// Spec §3.1. Record length 18 B.
pub(crate) mod pvs_header {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 18;
    /// Offset of `first_stream_offset` (`u32`, little-endian). Spec §3.1.
    pub(crate) const FIRST_STREAM_OFFSET: usize = 10;
    /// Offset of `stream_count` (`u32`, little-endian). Spec §3.1.
    pub(crate) const STREAM_COUNT: usize = 14;
}

/// Byte offsets for the `pvs_stream_entry_head` record.
///
/// Spec §3.2. Record length 3 B.
///
/// ```text
/// Fixed head only. `name_length` name bytes follow at +3, then the `u32` stored length.
/// ```
pub(crate) mod pvs_stream_entry_head {
    /// Record length in bytes. Spec §3.2.
    pub(crate) const LEN: usize = 3;
    /// Offset of `stream_type` (`u8`). Spec §3.2.
    pub(crate) const STREAM_TYPE: usize = 0;
    /// Offset of `codec` (`u8`). Spec §3.2.
    pub(crate) const CODEC: usize = 1;
    /// Stated value of `codec` (`u8`). Spec §3.2.
    pub(crate) const CODEC_VALUE: u8 = 1;
    /// Offset of `name_length` (`u8`). Spec §3.2.
    pub(crate) const NAME_LENGTH: usize = 2;
}

/// Byte offsets for the `ol_header` record.
///
/// Spec §4.1. Record length 22 B.
pub(crate) mod ol_header {
    /// Record length in bytes. Spec §4.1.
    pub(crate) const LEN: usize = 22;
    /// Offset of `version` (`u32`, little-endian). Spec §4.1.
    pub(crate) const VERSION: usize = 10;
    /// Stated value of `version` (`u32`). Spec §4.1.
    pub(crate) const VERSION_VALUE: u32 = 0x0000_0001;
    /// Offset of `directory_length` (`u32`, little-endian). Spec §4.1.
    pub(crate) const DIRECTORY_LENGTH: usize = 14;
    /// Offset of `entry_count` (`u32`, little-endian). Spec §4.1.
    pub(crate) const ENTRY_COUNT: usize = 18;
}

/// Byte offsets for the `ol_directory_entry` record.
///
/// Spec §4.2. Record length 8 B.
pub(crate) mod ol_directory_entry {
    /// Record length in bytes. Spec §4.2.
    pub(crate) const LEN: usize = 8;
    /// Offset of `stream_id` (`u32`, little-endian). Spec §4.2.
    pub(crate) const STREAM_ID: usize = 0;
    /// Offset of `offset` (`u32`, little-endian). Spec §4.2.
    pub(crate) const OFFSET: usize = 4;
}

/// Byte offsets for the `ol_preamble_block` record.
///
/// Spec §4.3. Record length 19 B.
pub(crate) mod ol_preamble_block {
    /// Record length in bytes. Spec §4.3.
    pub(crate) const LEN: usize = 19;
    /// Offset of `tag` (`u32`, little-endian). Spec §4.3.
    pub(crate) const TAG: usize = 0;
    /// Stated value of `tag` (`u32`). Spec §4.3.
    pub(crate) const TAG_VALUE: u32 = 0x0100_00d4;
    /// Offset of `reserved` (`bytes[8]`). Spec §4.3.
    pub(crate) const RESERVED: usize = 11;
    /// Stated value of `reserved` (`bytes[8]`). Spec §4.3.
    pub(crate) const RESERVED_VALUE: [u8; 8] = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
}

/// Byte offsets for the `tlv_header` record.
///
/// Spec §4.4. Record length 8 B.
///
/// ```text
/// `length` payload bytes follow at +8.
/// ```
pub(crate) mod tlv_header {
    /// Record length in bytes. Spec §4.4.
    pub(crate) const LEN: usize = 8;
    /// Offset of `tag` (`u32`, little-endian). Spec §4.4.
    pub(crate) const TAG: usize = 0;
    /// Offset of `length` (`u32`, little-endian). Spec §4.4.
    pub(crate) const LENGTH: usize = 4;
}

/// Byte offsets for the `appearance_values` record.
///
/// Spec §5.1. Record length 56 B.
pub(crate) mod appearance_values {
    /// Record length in bytes. Spec §5.1.
    pub(crate) const LEN: usize = 56;
    /// Offset of `base_rgb` (`f32[3]`, little-endian). Spec §5.1.
    pub(crate) const BASE_RGB: usize = 0;
    /// Offset of `diffuse_rgb` (`f32[3]`, little-endian). Spec §5.1.
    pub(crate) const DIFFUSE_RGB: usize = 12;
    /// Offset of `specular_rgb` (`f32[3]`, little-endian). Spec §5.1.
    pub(crate) const SPECULAR_RGB: usize = 24;
    /// Offset of `shininess` (`f32`, little-endian). Spec §5.1.
    pub(crate) const SHININESS: usize = 36;
    /// Offset of `emissive_rgb` (`f32[3]`, little-endian). Spec §5.1.
    pub(crate) const EMISSIVE_RGB: usize = 40;
    /// Offset of `opacity` (`f32`, little-endian). Spec §5.1.
    pub(crate) const OPACITY: usize = 52;
}

/// Byte offsets for the `patch_header_head` record.
///
/// Spec §6.2. Record length 13 B.
///
/// ```text
/// `ref_count` u32 values follow at +13; the payload length is 13 + 4 × ref_count.
/// ```
pub(crate) mod patch_header_head {
    /// Record length in bytes. Spec §6.2.
    pub(crate) const LEN: usize = 13;
    /// Offset of `appearance` (`u32`, little-endian). Spec §6.2.
    pub(crate) const APPEARANCE: usize = 0;
    /// Offset of `ref_count` (`u32`, little-endian). Spec §6.2.
    pub(crate) const REF_COUNT: usize = 9;
}

/// Byte offsets for the `vertex_block_head` record.
///
/// Spec §6.3. Record length 5 B.
///
/// ```text
/// vertex_count vertices follow at +5 with the stride the layout byte selects (12, 24, or 32).
/// ```
pub(crate) mod vertex_block_head {
    /// Record length in bytes. Spec §6.3.
    pub(crate) const LEN: usize = 5;
    /// Offset of `layout` (`u8`). Spec §6.3.
    pub(crate) const LAYOUT: usize = 0;
    /// Offset of `vertex_count` (`u32`, little-endian). Spec §6.3.
    pub(crate) const VERTEX_COUNT: usize = 1;
}

/// Byte offsets for the `edge_tessellation_header` record.
///
/// Spec §7.2. Record length 12 B.
pub(crate) mod edge_tessellation_header {
    /// Record length in bytes. Spec §7.2.
    pub(crate) const LEN: usize = 12;
    /// Offset of `edge_index` (`u32`, little-endian). Spec §7.2.
    pub(crate) const EDGE_INDEX: usize = 0;
}

/// Byte offsets for the `loop_record` record.
///
/// Spec §8.11. Record length 4 B.
pub(crate) mod loop_record {
    /// Record length in bytes. Spec §8.11.
    pub(crate) const LEN: usize = 4;
}

/// Byte offsets for the `coedge_record` record.
///
/// Spec §8.12. Record length 13 B.
pub(crate) mod coedge_record {
    /// Record length in bytes. Spec §8.12.
    pub(crate) const LEN: usize = 13;
}
