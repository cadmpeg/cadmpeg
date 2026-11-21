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
