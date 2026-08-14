// SPDX-License-Identifier: Apache-2.0
//! Byte-offset and value constants generated from `docs/layouts/iges.toml`.
//!
//! Do not edit by hand. Regenerate with:
//! `UPDATE_LAYOUT_CODE=1 cargo test -p cadmpeg --test layout_tables`.

#![allow(dead_code)] // Not every generated constant is referenced yet.

/// Tag constants from the table inventory.
pub(crate) mod token {
    /// `Start section` (`S`). Spec §Physical representation.
    pub(crate) const START_SECTION: [u8; 1] = *b"S";
    /// `Global section` (`G`). Spec §Physical representation.
    pub(crate) const GLOBAL_SECTION: [u8; 1] = *b"G";
    /// `Directory Entry section` (`D`). Spec §Physical representation.
    pub(crate) const DIRECTORY_ENTRY_SECTION: [u8; 1] = *b"D";
    /// `Parameter Data section` (`P`). Spec §Physical representation.
    pub(crate) const PARAMETER_DATA_SECTION: [u8; 1] = *b"P";
    /// `Terminate section` (`T`). Spec §Physical representation.
    pub(crate) const TERMINATE_SECTION: [u8; 1] = *b"T";
}

/// Byte offsets for the `binary_flag` record.
///
/// Spec §Physical representation. Record length 80 B.
///
/// ```text
/// The six one-byte primitive length fields select the bit widths used by the remaining Binary representation. Each displacement counts its section and any following null padding.
/// ```
pub(crate) mod binary_flag {
    /// Record length in bytes. Spec §Physical representation.
    pub(crate) const LEN: usize = 80;
    /// Offset of `identifier` (`bytes[1]`). Spec §Physical representation.
    pub(crate) const IDENTIFIER: usize = 0;
    /// Offset of `remaining_byte_count` (`u32`, big-endian). Spec §Physical representation.
    pub(crate) const REMAINING_BYTE_COUNT: usize = 1;
    /// Stated value of `remaining_byte_count` (`u32`). Spec §Physical representation.
    pub(crate) const REMAINING_BYTE_COUNT_VALUE: u32 = 0x0000_004b;
    /// Offset of `primitive_bit_lengths` (`bytes[6]`). Spec §Physical representation.
    pub(crate) const PRIMITIVE_BIT_LENGTHS: usize = 5;
    /// Offset of `section_displacements` (`bytes[30]`). Spec §Physical representation.
    pub(crate) const SECTION_DISPLACEMENTS: usize = 11;
    /// Offset of `unassigned` (`bytes[31]`). Spec §Physical representation.
    pub(crate) const UNASSIGNED: usize = 41;
    /// Offset of `section_marker` (`bytes[1]`). Spec §Physical representation.
    pub(crate) const SECTION_MARKER: usize = 72;
    /// Offset of `sequence_padding` (`bytes[6]`). Spec §Physical representation.
    pub(crate) const SEQUENCE_PADDING: usize = 73;
    /// Offset of `sequence` (`bytes[1]`). Spec §Physical representation.
    pub(crate) const SEQUENCE: usize = 79;
}
