// SPDX-License-Identifier: Apache-2.0
//! Byte-offset and value constants generated from `docs/layouts/zip.toml`.
//!
//! Do not edit by hand. Regenerate with:
//! `UPDATE_LAYOUT_CODE=1 cargo test -p cadmpeg --test layout_tables`.

/// Byte offsets for the `central_header` record.
///
/// Spec §1. Record length 46 B.
pub(crate) mod central_header {
    /// Offset of `compression` (`u16`, little-endian). Spec §1.
    pub(crate) const COMPRESSION: usize = 10;
}
