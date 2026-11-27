// SPDX-License-Identifier: Apache-2.0
//! Byte-offset and value constants generated from `docs/layouts/ironcad.toml`.
//!
//! Do not edit by hand. Regenerate with:
//! `UPDATE_LAYOUT_CODE=1 cargo test -p cadmpeg-layout-tables`.

/// Tag constants from the table inventory.
pub(crate) mod token {
    /// `KERNEL_ACIS` (`0x80000001`). Spec §3.
    pub(crate) const KERNEL_ACIS: u32 = 0x8000_0001;
    /// `KERNEL_PARASOLID` (`0x80000002`). Spec §3.
    pub(crate) const KERNEL_PARASOLID: u32 = 0x8000_0002;
}

/// Byte offsets for the `frame_header` record.
///
/// Spec §2. Record length 20 B.
pub(crate) mod frame_header {
    /// Stated value of `magic` (`u32`). Spec §2.
    pub(crate) const MAGIC_VALUE: u32 = 0x3de3_de00;
}

/// Byte offsets for the `tree_node_header` record.
///
/// Spec §3. Record length 8 B.
pub(crate) mod tree_node_header {
    /// Record length in bytes. Spec §3.
    pub(crate) const LEN: usize = 8;
}

/// Byte offsets for the `kernel_record_header` record.
///
/// Spec §3. Record length 28 B.
pub(crate) mod kernel_record_header {
    /// Record length in bytes. Spec §3.
    pub(crate) const LEN: usize = 28;
    /// Offset of `marker` (`u32`, little-endian). Spec §3.
    pub(crate) const MARKER: usize = 4;
    /// Offset of `kernel` (`u32`, little-endian). Spec §3.
    pub(crate) const KERNEL: usize = 8;
    /// Offset of `words` (`u32[3]`, little-endian). Spec §3.
    pub(crate) const WORDS: usize = 12;
    /// Offset of `reserved` (`u32`, little-endian). Spec §3.
    pub(crate) const RESERVED: usize = 24;
}

/// Byte offsets for the `scene_object_header` record.
///
/// Spec §5. Record length 20 B.
pub(crate) mod scene_object_header {
    /// Offset of `version` (`u32`, little-endian). Spec §5.
    pub(crate) const VERSION: usize = 0;
    /// Offset of `class` (`u32`, little-endian). Spec §5.
    pub(crate) const CLASS: usize = 4;
    /// Offset of `class_tail` (`u8[12]`). Spec §5.
    pub(crate) const CLASS_TAIL: usize = 8;
}

/// Byte offsets for the `scene_record_matrix_short` record.
///
/// Spec §5. Record length 161 B.
pub(crate) mod scene_record_matrix_short {
    /// Offset of `flag` (`u8`). Spec §5.
    pub(crate) const FLAG: usize = 32;
    /// Offset of `matrix` (`f64[16]`, little-endian). Spec §5.
    pub(crate) const MATRIX: usize = 33;
}

/// Byte offsets for the `scene_record_matrix` record.
///
/// Spec §5. Record length 169 B.
pub(crate) mod scene_record_matrix {
    /// Offset of `flag` (`u8`). Spec §5.
    pub(crate) const FLAG: usize = 40;
    /// Stated value of `flag` (`u8`). Spec §5.
    pub(crate) const FLAG_VALUE: u8 = 1;
    /// Offset of `matrix` (`f64[16]`, little-endian). Spec §5.
    pub(crate) const MATRIX: usize = 41;
}
