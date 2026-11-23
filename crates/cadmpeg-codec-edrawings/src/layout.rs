// SPDX-License-Identifier: Apache-2.0
//! Byte-offset and value constants generated from `docs/layouts/edrawings.toml`.
//!
//! Do not edit by hand. Regenerate with:
//! `UPDATE_LAYOUT_CODE=1 cargo test -p cadmpeg-layout-tables`.

/// Byte offsets for the `wrapper` record.
///
/// Spec §2.1. Record length 47 B.
///
/// ```text
/// Comment line, `I` record, `B` record, and the `Z` tag. The zlib stream starts at offset 47.
/// ```
pub(crate) mod wrapper {
    /// Record length in bytes. Spec §2.1.
    #[cfg(test)]
    pub(crate) const LEN: usize = 47;
    /// Offset of `comment` (`bytes[15]`). Spec §2.1.
    #[cfg(test)]
    pub(crate) const COMMENT: usize = 0;
    /// Offset of `info` (`bytes[5]`). Spec §2.1.
    #[cfg(test)]
    pub(crate) const INFO: usize = 15;
    /// Offset of `bounds` (`bytes[26]`). Spec §2.1.
    #[cfg(test)]
    pub(crate) const BOUNDS: usize = 20;
    /// Offset of `body_tag` (`u8`). Spec §2.1.
    #[cfg(test)]
    pub(crate) const BODY_TAG: usize = 46;
    /// Stated value of `body_tag` (`u8`). Spec §2.1.
    pub(crate) const BODY_TAG_VALUE: u8 = 90;
}

/// Byte offsets for the `wrapper_comment` record.
///
/// Spec §2.1. Record length 15 B.
pub(crate) mod wrapper_comment {
    /// Record length in bytes. Spec §2.1.
    #[cfg(test)]
    pub(crate) const LEN: usize = 15;
    /// Stated value of `prefix` (`bytes[8]`). Spec §2.1.
    pub(crate) const PREFIX_VALUE: [u8; 8] = *b";; HSF V";
}

/// Byte offsets for the `wrapper_info` record.
///
/// Spec §2.1. Record length 5 B.
pub(crate) mod wrapper_info {
    /// Record length in bytes. Spec §2.1.
    pub(crate) const LEN: usize = 5;
    /// Stated value of `tag` (`u8`). Spec §2.1.
    pub(crate) const TAG_VALUE: u8 = 73;
    /// Offset of `value` (`u32`, little-endian). Spec §2.1.
    pub(crate) const VALUE: usize = 1;
}

/// Byte offsets for the `wrapper_bounds` record.
///
/// Spec §2.1. Record length 26 B.
pub(crate) mod wrapper_bounds {
    /// Record length in bytes. Spec §2.1.
    pub(crate) const LEN: usize = 26;
    /// Stated value of `tag` (`u8`). Spec §2.1.
    pub(crate) const TAG_VALUE: u8 = 66;
    /// Offset of `flag` (`u8`). Spec §2.1.
    pub(crate) const FLAG: usize = 1;
    /// Offset of `min` (`f32[3]`, little-endian). Spec §2.1.
    pub(crate) const MIN: usize = 2;
    /// Offset of `max` (`f32[3]`, little-endian). Spec §2.1.
    pub(crate) const MAX: usize = 14;
}

/// Byte offsets for the `shell_head` record.
///
/// Spec §3.6. Record length 11 B.
///
/// ```text
/// The fixed head of a shell record. The vertices follow at offset 11.
/// ```
pub(crate) mod shell_head {
    /// Record length in bytes. Spec §3.6.
    pub(crate) const LEN: usize = 11;
    /// Stated value of `opcode` (`u8`). Spec §3.6.
    pub(crate) const OPCODE_VALUE: u8 = 83;
    /// Stated value of `constant` (`u8`). Spec §3.6.
    pub(crate) const CONSTANT_VALUE: u8 = 14;
    /// Offset of `id` (`u32`, little-endian). Spec §3.6.
    pub(crate) const ID: usize = 2;
    /// Offset of `zero` (`u8`). Spec §3.6.
    pub(crate) const ZERO: usize = 6;
    /// Stated value of `zero` (`u8`). Spec §3.6.
    pub(crate) const ZERO_VALUE: u8 = 0;
    /// Offset of `vertex_count` (`u32`, little-endian). Spec §3.6.
    pub(crate) const VERTEX_COUNT: usize = 7;
}

/// Byte offsets for the `shell_face_list_head` record.
///
/// Spec §3.6. Record length 5 B.
pub(crate) mod shell_face_list_head {
    /// Record length in bytes. Spec §3.6.
    pub(crate) const LEN: usize = 5;
    /// Offset of `marker` (`u8`). Spec §3.6.
    pub(crate) const MARKER: usize = 0;
    /// Stated value of `marker` (`u8`). Spec §3.6.
    pub(crate) const MARKER_VALUE: u8 = 1;
    /// Offset of `length` (`u32`, little-endian). Spec §3.6.
    pub(crate) const LENGTH: usize = 1;
}

/// Byte offsets for the `shell_normals_head` record.
///
/// Spec §3.6. Record length 7 B.
pub(crate) mod shell_normals_head {
    /// Record length in bytes. Spec §3.6.
    pub(crate) const LEN: usize = 7;
    /// Offset of `marker` (`bytes[3]`). Spec §3.6.
    pub(crate) const MARKER: usize = 0;
    /// Stated value of `marker` (`bytes[3]`). Spec §3.6.
    pub(crate) const MARKER_VALUE: [u8; 3] = [0x01, 0x07, 0x0c];
    /// Offset of `length` (`u32`, little-endian). Spec §3.6.
    pub(crate) const LENGTH: usize = 3;
}

/// Byte offsets for the `shell_range_head` record.
///
/// Spec §3.6.1. Record length 24 B.
///
/// ```text
/// The range block without its K bytes. The K bytes follow at offset 24.
/// ```
pub(crate) mod shell_range_head {
    /// Record length in bytes. Spec §3.6.1.
    pub(crate) const LEN: usize = 24;
    /// Stated value of `opcode` (`u8`). Spec §3.6.1.
    pub(crate) const OPCODE_VALUE: u8 = 3;
    /// Offset of `head` (`bytes[2]`). Spec §3.6.1.
    pub(crate) const HEAD: usize = 1;
    /// Stated value of `head` (`bytes[2]`). Spec §3.6.1.
    pub(crate) const HEAD_VALUE: [u8; 2] = [0x01, 0x02];
    /// Offset of `range` (`f32[4]`, little-endian). Spec §3.6.1.
    pub(crate) const RANGE: usize = 3;
    /// Offset of `marker` (`u8`). Spec §3.6.1.
    pub(crate) const MARKER: usize = 19;
    /// Stated value of `marker` (`u8`). Spec §3.6.1.
    pub(crate) const MARKER_VALUE: u8 = 8;
    /// Offset of `length` (`u32`, little-endian). Spec §3.6.1.
    pub(crate) const LENGTH: usize = 20;
}
