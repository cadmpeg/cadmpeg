// SPDX-License-Identifier: Apache-2.0
//! Byte-offset and value constants generated from `docs/layouts/cfb.toml`.
//!
//! Do not edit by hand. Regenerate with:
//! `UPDATE_LAYOUT_CODE=1 cargo test -p cadmpeg --test layout_tables`.

/// Byte offsets for the `directory_entry` record.
///
/// Spec §1. Record length 128 B.
pub(crate) mod directory_entry {
    /// Record length in bytes. Spec §1.
    pub(crate) const LEN: usize = 128;
    /// Offset of `name` (`bytes[64]`). Spec §1.
    pub(crate) const NAME: usize = 0;
    /// Offset of `name_length` (`u16`, little-endian). Spec §1.
    pub(crate) const NAME_LENGTH: usize = 64;
    /// Offset of `object_type` (`u8`). Spec §1.
    pub(crate) const OBJECT_TYPE: usize = 66;
    /// Offset of `color` (`u8`). Spec §1.
    pub(crate) const COLOR: usize = 67;
    /// Offset of `left` (`u32`, little-endian). Spec §1.
    pub(crate) const LEFT: usize = 68;
    /// Offset of `right` (`u32`, little-endian). Spec §1.
    pub(crate) const RIGHT: usize = 72;
    /// Offset of `child` (`u32`, little-endian). Spec §1.
    pub(crate) const CHILD: usize = 76;
    /// Offset of `clsid` (`bytes[16]`). Spec §1.
    #[cfg(test)]
    pub(crate) const CLSID: usize = 80;
    /// Offset of `state_bits` (`u32`, little-endian). Spec §1.
    #[cfg(test)]
    pub(crate) const STATE_BITS: usize = 96;
    /// Offset of `creation_time` (`u64`, little-endian). Spec §1.
    #[cfg(test)]
    pub(crate) const CREATION_TIME: usize = 100;
    /// Offset of `modified_time` (`u64`, little-endian). Spec §1.
    #[cfg(test)]
    pub(crate) const MODIFIED_TIME: usize = 108;
    /// Offset of `start_sector` (`u32`, little-endian). Spec §1.
    pub(crate) const START_SECTOR: usize = 116;
    /// Offset of `stream_size` (`u64`, little-endian). Spec §1.
    pub(crate) const STREAM_SIZE: usize = 120;
}
