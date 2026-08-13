// SPDX-License-Identifier: Apache-2.0
//! Byte-offset constants generated from `docs/layouts/freecad.toml`.
//!
//! Do not edit by hand. Regenerate with:
//! `UPDATE_LAYOUT_CODE=1 cargo test -p cadmpeg --test layout_tables`.

#![allow(dead_code)] // Not every generated constant is referenced yet.

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
    /// Offset of `magic` (`u32`, endianness unstated). Spec §11.
    pub(crate) const MAGIC: usize = 0;
    /// Offset of `version` (`u32`, endianness unstated). Spec §11.
    pub(crate) const VERSION: usize = 4;
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
    /// Offset of `point_indices` (`u32[3]`, little-endian). Spec §11.
    pub(crate) const POINT_INDICES: usize = 0;
    /// Offset of `neighbour_indices` (`u32[3]`, little-endian). Spec §11.
    pub(crate) const NEIGHBOUR_INDICES: usize = 12;
}

/// Byte offsets for the `point_kernel_side_entry_header` record.
///
/// Spec §11. Record length 4 B.
///
/// ```text
/// Fixed prefix only; `count` float32 XYZ triples follow at +4. The property's `Points` element carries the sixteen finite row-major transform scalars separately, in XML.
/// ```
pub(crate) mod point_kernel_side_entry_header {
    /// Record length in bytes. Spec §11.
    pub(crate) const LEN: usize = 4;
    /// Offset of `point_count` (`u32`, little-endian). Spec §11.
    pub(crate) const POINT_COUNT: usize = 0;
}

/// Byte offsets for the `packed_color_list_header` record.
///
/// Spec §11. Record length 4 B.
///
/// ```text
/// Applies to `DiffuseColor`, `LineColorArray`, and `PointColorArray`. A count of one applies its colour to every member of the corresponding element-map group; otherwise the count must equal the number of names in that ordered group.
/// ```
pub(crate) mod packed_color_list_header {
    /// Record length in bytes. Spec §11.
    pub(crate) const LEN: usize = 4;
    /// Offset of `count` (`u32`, little-endian). Spec §11.
    pub(crate) const COUNT: usize = 0;
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
    /// Offset of `element_count` (`u32`, little-endian). Spec §9.
    pub(crate) const ELEMENT_COUNT: usize = 0;
}
