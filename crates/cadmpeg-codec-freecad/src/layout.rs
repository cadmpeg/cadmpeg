// SPDX-License-Identifier: Apache-2.0
//! Byte-offset and value constants generated from `docs/layouts/freecad.toml`.
//!
//! Do not edit by hand. Regenerate with:
//! `UPDATE_LAYOUT_CODE=1 cargo test -p cadmpeg --test layout_tables`.

/// Byte offsets for the `mesh_kernel_side_entry_header` record.
///
/// Spec §11. Record length 264 B.
///
/// ```text
/// Both integer byte orders are accepted when the magic and version agree, so the header states no single endianness. Two 32-bit counts follow at +264, then ordered float32 XYZ points and facets, then six float32 bounding-box limits.
/// ```
pub(crate) mod mesh_kernel_side_entry_header {
    /// Record length in bytes. Spec §11.
    pub(crate) const LEN: usize = 264;
    /// Stated value of `magic` (`u32`). Spec §11.
    pub(crate) const MAGIC_VALUE: u32 = 0xa0b0_c0d0;
    /// Stated value of `version` (`u32`). Spec §11.
    pub(crate) const VERSION_VALUE: u32 = 0x0001_0000;
    /// Offset of `information` (`bytes[256]`). Spec §11.
    pub(crate) const INFORMATION: usize = 8;
}

/// Byte offsets for the `mesh_facet` record.
///
/// Spec §11. Record length 24 B.
///
/// ```text
/// Six 32-bit indices. The 24-byte stride is derived from the stated field count and the record's 32-bit index width; the spec states no total.
/// ```
pub(crate) mod mesh_facet {
    /// Record length in bytes. Spec §11.
    pub(crate) const LEN: usize = 24;
}

/// Byte offsets for the `link_array_side_entry_header` record.
///
/// Spec §9. Record length 4 B.
///
/// ```text
/// Fixed prefix only. Placement records carry position plus quaternion (seven components); scale records carry three. The exact entry length selects f32 or f64 components, so no fixed element stride exists.
/// ```
pub(crate) mod link_array_side_entry_header {
    /// Record length in bytes. Spec §9.
    pub(crate) const LEN: usize = 4;
}
