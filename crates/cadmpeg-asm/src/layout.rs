// SPDX-License-Identifier: Apache-2.0
//! Byte-offset and value constants generated from `docs/layouts/asm.toml`.
//!
//! Do not edit by hand. Regenerate with:
//! `UPDATE_LAYOUT_CODE=1 cargo test -p cadmpeg --test layout_tables`.

// Records omitted because the table declares a contradiction.
//
// - `coedge` (size_mismatch): The stated offsets end `chunk[10]` at +99 but the heading declares 100 B. Shifting every stated offset by +1 (head 9 bytes rather than 8, matching the `coedge` name-token length) closes the record at 100; so does leaving the offsets alone and declaring 99 B. The spec does not say which side is wrong.
// - `edge` (size_mismatch): The stated offsets end the continuity text at +99 but the heading declares 98 B. Removing the one-byte gap at +88 (placing the sense byte at +88 and the text at +89) closes the record at 98 exactly, which suggests both trailing offsets are one too high. The spec does not state which.

/// Byte offsets for the `asmheader_binaryfile8` record.
///
/// Spec §1. Record length 47 B.
///
/// Dialects: `acis:asm-binaryfile-8`.
///
/// ```text
/// Fixed prefix only. The string region and the six trailing tagged metadata fields begin at byte 47 and are a sequence, not a fixed-offset structure.
/// ```
pub(crate) mod asmheader_binaryfile8 {
    /// Record length in bytes. Spec §1.
    pub(crate) const LEN: usize = 47;
    /// Offset of `save_format_version` (`u32`, little-endian). Spec §1.
    pub(crate) const SAVE_FORMAT_VERSION: usize = 15;
    /// Offset of `entity_count` (`u64`, little-endian). Spec §1.
    pub(crate) const ENTITY_COUNT: usize = 31;
    /// Offset of `flags` (`u64`, little-endian). Spec §1.
    pub(crate) const FLAGS: usize = 39;
}

/// Byte offsets for the `asmheader_binaryfile4` record.
///
/// Spec §1. Record length 31 B.
///
/// Dialects: `acis:asm-binaryfile-4`.
///
/// ```text
/// Fixed prefix only; the string region begins at byte 31.
/// ```
pub(crate) mod asmheader_binaryfile4 {
    /// Record length in bytes. Spec §1.
    pub(crate) const LEN: usize = 31;
    /// Offset of `save_format_version` (`u32`, little-endian). Spec §1.
    pub(crate) const SAVE_FORMAT_VERSION: usize = 15;
    /// Offset of `record_count` (`u32`, little-endian). Spec §1.
    pub(crate) const RECORD_COUNT: usize = 19;
    /// Offset of `entity_count` (`u32`, little-endian). Spec §1.
    pub(crate) const ENTITY_COUNT: usize = 23;
    /// Offset of `flags` (`u32`, little-endian). Spec §1.
    pub(crate) const FLAGS: usize = 27;
}

/// Byte offsets for the `acisheader_binaryfile4` record.
///
/// Spec §1. Record length 31 B.
///
/// Dialects: `acis:save-format-217`, `acis:save-format-218`, `acis:save-format-binary-other`.
///
/// ```text
/// Fixed 32-bit ACIS prefix; the tagged string region begins at byte 31.
/// ```
pub(crate) mod acisheader_binaryfile4 {
    /// Record length in bytes. Spec §1.
    pub(crate) const LEN: usize = 31;
    /// Offset of `save_format_version` (`u32`, little-endian). Spec §1.
    pub(crate) const SAVE_FORMAT_VERSION: usize = 15;
    /// Offset of `entity_count` (`u32`, little-endian). Spec §1.
    pub(crate) const ENTITY_COUNT: usize = 23;
    /// Offset of `flags` (`u32`, little-endian). Spec §1.
    pub(crate) const FLAGS: usize = 27;
}
