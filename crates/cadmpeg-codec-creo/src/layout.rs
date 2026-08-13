// SPDX-License-Identifier: Apache-2.0
//! Byte-offset constants generated from `docs/layouts/creo.toml`.
//!
//! Do not edit by hand. Regenerate with:
//! `UPDATE_LAYOUT_CODE=1 cargo test -p cadmpeg --test layout_tables`.

#![allow(dead_code)] // Not every generated constant is referenced yet.

/// Byte offsets for the `unix_compress_header` record.
///
/// Spec §1. Record length 3 B.
///
/// ```text
/// The low five flag bits give the maximum code width from 9 through 16; bit 7 enables block mode and code 256 clears the dictionary. Codes are packed least significant bit first in code-width-sized byte blocks.
/// ```
pub(crate) mod unix_compress_header {
    /// Record length in bytes. Spec §1.
    pub(crate) const LEN: usize = 3;
    /// Offset of `magic` (`bytes[2]`). Spec §1.
    pub(crate) const MAGIC: usize = 0;
    /// Offset of `flags` (`u8`). Spec §1.
    pub(crate) const FLAGS: usize = 2;
}

/// Byte offsets for the `cmnm_model_name_record` record.
///
/// Spec §1. Record length 11 B.
///
/// ```text
/// Fixed prefix only; `hhh` bytes of ASCII name follow at +11, then trailing ASCII space padding. Exactly one record establishes model identity; an absent or repeated record leaves model identity undefined.
/// ```
pub(crate) mod cmnm_model_name_record {
    /// Record length in bytes. Spec §1.
    pub(crate) const LEN: usize = 11;
    /// Offset of `prefix` (`bytes[8]`). Spec §1.
    pub(crate) const PREFIX: usize = 0;
    /// Offset of `name_length_hex` (`bytes[3]`). Spec §1.
    pub(crate) const NAME_LENGTH_HEX: usize = 8;
}

/// Byte offsets for the `type24_first_coordinate_bounded_round` record.
///
/// Spec §3.3. Record length 50 B.
///
/// ```text
/// The two diameter endpoints and five extent scalars use the tabulated-cylinder first-coordinate lane, including its positive eight-byte `2d` form. Terminal `18` at offset 49 is the zero-valued sixth extent coordinate.
/// ```
pub(crate) mod type24_first_coordinate_bounded_round {
    /// Record length in bytes. Spec §3.3.
    pub(crate) const LEN: usize = 50;
    /// Offset of `opener` (`bytes[2]`). Spec §3.3.
    pub(crate) const OPENER: usize = 0;
    /// Offset of `first_diameter_endpoint` (`bytes[8]`). Spec §3.3.
    pub(crate) const FIRST_DIAMETER_ENDPOINT: usize = 7;
    /// Offset of `separator` (`u8`). Spec §3.3.
    pub(crate) const SEPARATOR: usize = 15;
    /// Offset of `second_diameter_endpoint` (`bytes[8]`). Spec §3.3.
    pub(crate) const SECOND_DIAMETER_ENDPOINT: usize = 16;
    /// Offset of `extent_scalars` (`bytes[25]`). Spec §3.3.
    pub(crate) const EXTENT_SCALARS: usize = 24;
    /// Offset of `terminal` (`u8`). Spec §3.3.
    pub(crate) const TERMINAL: usize = 49;
}

/// Byte offsets for the `type24_segmented_first_coordinate_bounded_round` record.
///
/// Spec §3.3. Record length 56 B.
///
/// ```text
/// Both diameter endpoints and all six extent coordinates use the tabulated-cylinder first-coordinate lane. Every byte range in this record is stated outright, so the table tiles it with no gap.
/// ```
pub(crate) mod type24_segmented_first_coordinate_bounded_round {
    /// Record length in bytes. Spec §3.3.
    pub(crate) const LEN: usize = 56;
    /// Offset of `opener` (`u8`). Spec §3.3.
    pub(crate) const OPENER: usize = 0;
    /// Offset of `first_diameter_endpoint` (`bytes[8]`). Spec §3.3.
    pub(crate) const FIRST_DIAMETER_ENDPOINT: usize = 1;
    /// Offset of `literal_run` (`bytes[7]`). Spec §3.3.
    pub(crate) const LITERAL_RUN: usize = 9;
    /// Offset of `second_diameter_endpoint` (`bytes[8]`). Spec §3.3.
    pub(crate) const SECOND_DIAMETER_ENDPOINT: usize = 16;
    /// Offset of `extent_coordinates` (`bytes[30]`). Spec §3.3.
    pub(crate) const EXTENT_COORDINATES: usize = 24;
    /// Offset of `trailer` (`bytes[2]`). Spec §3.3.
    pub(crate) const TRAILER: usize = 54;
}
