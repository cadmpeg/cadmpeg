// SPDX-License-Identifier: Apache-2.0
//! Byte-offset and value constants generated from `docs/layouts/inventor.toml`.
//!
//! Do not edit by hand. Regenerate with:
//! `UPDATE_LAYOUT_CODE=1 cargo test -p cadmpeg --test layout_tables`.

/// Byte offsets for the `bulk_envelope` record.
///
/// Spec §4. Record length 18 B.
pub(crate) mod bulk_envelope {
    /// Record length in bytes. Spec §4.
    pub(crate) const LEN: usize = 18;
    /// Offset of `prefix` (`bytes[16]`). Spec §4.
    pub(crate) const PREFIX: usize = 0;
    /// Offset of `form` (`u16`, little-endian). Spec §4.
    pub(crate) const FORM: usize = 16;
}

/// Byte offsets for the `meta_body_prefix` record.
///
/// Spec §3. Record length 14 B.
pub(crate) mod meta_body_prefix {
    /// Record length in bytes. Spec §3.
    pub(crate) const LEN: usize = 14;
}

/// Byte offsets for the `meta_type_descriptor` record.
///
/// Spec §3. Record length 28 B.
pub(crate) mod meta_type_descriptor {
    /// Record length in bytes. Spec §3.
    pub(crate) const LEN: usize = 28;
}

/// Byte offsets for the `kernel_carrier_header` record.
///
/// Spec §5. Record length 14 B.
pub(crate) mod kernel_carrier_header {
    /// Record length in bytes. Spec §5.
    pub(crate) const LEN: usize = 14;
    /// Offset of `header_state` (`u32`, little-endian). Spec §5.
    pub(crate) const HEADER_STATE: usize = 0;
    /// Offset of `header_kind` (`u16`, little-endian). Spec §5.
    pub(crate) const HEADER_KIND: usize = 4;
    /// Offset of `header_value` (`u32`, little-endian). Spec §5.
    pub(crate) const HEADER_VALUE: usize = 6;
    /// Offset of `schema` (`u32`, little-endian). Spec §5.
    pub(crate) const SCHEMA: usize = 10;
}

/// Byte offsets for the `protein_header` record.
///
/// Spec §7. Record length 4 B.
///
/// ```text
/// Inventor compound-stream envelope around a Protein ZIP. The Protein page format is tabulated in `docs/layouts/protein.toml`.
/// ```
pub(crate) mod protein_header {
    /// Record length in bytes. Spec §7.
    pub(crate) const LEN: usize = 4;
}
