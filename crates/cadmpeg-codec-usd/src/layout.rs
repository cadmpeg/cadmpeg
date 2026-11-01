// SPDX-License-Identifier: Apache-2.0
//! Byte-offset and value constants generated from `docs/layouts/usd.toml`.
//!
//! Do not edit by hand. Regenerate with:
//! `UPDATE_LAYOUT_CODE=1 cargo test -p cadmpeg --test layout_tables`.

/// Byte offsets for the `usdc_bootstrap` record.
///
/// Spec §9.1. Record length 88 B.
pub(crate) mod usdc_bootstrap {
    /// Record length in bytes. Spec §9.1.
    pub(crate) const LEN: usize = 88;
    /// Offset of `identifier` (`bytes[8]`). Spec §9.1.
    pub(crate) const IDENTIFIER: usize = 0;
    /// Stated value of `identifier` (`bytes[8]`). Spec §9.1.
    pub(crate) const IDENTIFIER_VALUE: [u8; 8] = *b"PXR-USDC";
    /// Offset of `major` (`u8`). Spec §9.1.
    pub(crate) const MAJOR: usize = 8;
    /// Offset of `minor` (`u8`). Spec §9.1.
    pub(crate) const MINOR: usize = 9;
    /// Offset of `patch` (`u8`). Spec §9.1.
    pub(crate) const PATCH: usize = 10;
    /// Offset of `toc_offset` (`i64`, little-endian). Spec §9.1.
    pub(crate) const TOC_OFFSET: usize = 16;
}

/// Byte offsets for the `usdc_section` record.
///
/// Spec §9.2. Record length 32 B.
pub(crate) mod usdc_section {
    /// Record length in bytes. Spec §9.2.
    pub(crate) const LEN: usize = 32;
    /// Offset of `name` (`bytes[16]`). Spec §9.2.
    pub(crate) const NAME: usize = 0;
    /// Offset of `start` (`i64`, little-endian). Spec §9.2.
    pub(crate) const START: usize = 16;
    /// Offset of `size` (`i64`, little-endian). Spec §9.2.
    pub(crate) const SIZE: usize = 24;
}

/// Byte offsets for the `usdc_path_item` record.
///
/// Spec §9.5. Record length 12 B.
pub(crate) mod usdc_path_item {
    /// Record length in bytes. Spec §9.5.
    pub(crate) const LEN: usize = 12;
    /// Offset of `index` (`u32`, little-endian). Spec §9.5.
    pub(crate) const INDEX: usize = 0;
    /// Offset of `element_token` (`u32`, little-endian). Spec §9.5.
    pub(crate) const ELEMENT_TOKEN: usize = 4;
    /// Offset of `bits` (`u8`). Spec §9.5.
    pub(crate) const BITS: usize = 8;
}
