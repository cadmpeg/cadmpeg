// SPDX-License-Identifier: Apache-2.0
//! Byte-offset and value constants generated from `docs/layouts/asm.toml`.
//!
//! Do not edit by hand. Regenerate with:
//! `UPDATE_LAYOUT_CODE=1 cargo test -p cadmpeg --test layout_tables`.

#![allow(dead_code)] // Not every generated constant is referenced yet.

// Records omitted because the table declares a contradiction.
//
// - `coedge` (size_mismatch): The stated offsets end `chunk[10]` at +99 but the heading declares 100 B. Shifting every stated offset by +1 (head 9 bytes rather than 8, matching the `coedge` name-token length) closes the record at 100; so does leaving the offsets alone and declaring 99 B. The spec does not say which side is wrong.
// - `edge` (size_mismatch): The stated offsets end the continuity text at +99 but the heading declares 98 B. Removing the one-byte gap at +88 (placing the sense byte at +88 and the text at +89) closes the record at 98 exactly, which suggests both trailing offsets are one too high. The spec does not state which.

/// Tag constants from the table inventory.
pub(crate) mod token {
    /// `CHAR` (`0x02`). Spec §2.1.
    pub(crate) const CHAR: u8 = 2;
    /// `SHORT` (`0x03`). Spec §2.1.
    pub(crate) const SHORT: u8 = 3;
    /// `LONG` (`0x04`). Spec §2.1.
    pub(crate) const LONG: u8 = 4;
    /// `FLOAT` (`0x05`). Spec §2.1.
    pub(crate) const FLOAT: u8 = 5;
    /// `DOUBLE` (`0x06`). Spec §2.1.
    pub(crate) const DOUBLE: u8 = 6;
    /// `TRUE` (`0x0A`). Spec §2.1.
    pub(crate) const TRUE: u8 = 10;
    /// `FALSE` (`0x0B`). Spec §2.1.
    pub(crate) const FALSE: u8 = 11;
    /// `ENTITY_REF` (`0x0C`). Spec §2.1.
    pub(crate) const ENTITY_REF: u8 = 12;
    /// `IDENT` (`0x0D`). Spec §2.1.
    pub(crate) const IDENT: u8 = 13;
    /// `SUBIDENT` (`0x0E`). Spec §2.1.
    pub(crate) const SUBIDENT: u8 = 14;
    /// `TERMINATOR` (`0x11`). Spec §2.1.
    pub(crate) const TERMINATOR: u8 = 17;
    /// `POSITION` (`0x13`). Spec §2.1.
    pub(crate) const POSITION: u8 = 19;
    /// `VECTOR_3D` (`0x14`). Spec §2.1.
    pub(crate) const VECTOR_3D: u8 = 20;
    /// `ENUM_VALUE` (`0x15`). Spec §2.1.
    pub(crate) const ENUM_VALUE: u8 = 21;
    /// `VECTOR_2D` (`0x16`). Spec §2.1.
    pub(crate) const VECTOR_2D: u8 = 22;
    /// `INT64` (`0x17`). Spec §2.1.
    pub(crate) const INT64: u8 = 23;
}

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
    /// Offset of `magic` (`bytes[15]`). Spec §1.
    pub(crate) const MAGIC: usize = 0;
    /// Offset of `save_format_version` (`u32`, little-endian). Spec §1.
    pub(crate) const SAVE_FORMAT_VERSION: usize = 15;
    /// Offset of `zero_pad` (`bytes[12]`). Spec §1.
    pub(crate) const ZERO_PAD: usize = 19;
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
    /// Offset of `magic` (`bytes[15]`). Spec §1.
    pub(crate) const MAGIC: usize = 0;
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
    /// Offset of `magic` (`bytes[15]`). Spec §1.
    pub(crate) const MAGIC: usize = 0;
    /// Offset of `save_format_version` (`u32`, little-endian). Spec §1.
    pub(crate) const SAVE_FORMAT_VERSION: usize = 15;
    /// Offset of `record_count` (`u32`, little-endian). Spec §1.
    pub(crate) const RECORD_COUNT: usize = 19;
    /// Offset of `entity_count` (`u32`, little-endian). Spec §1.
    pub(crate) const ENTITY_COUNT: usize = 23;
    /// Offset of `flags` (`u32`, little-endian). Spec §1.
    pub(crate) const FLAGS: usize = 27;
}

/// Byte offsets for the `body` record.
///
/// Spec §5.2. Record length 61 B.
///
/// Dialects: `acis:asm-binaryfile-8`.
///
/// ```text
/// Offsets are record-relative from the leading `0x11`. On `BinaryFile4` streams ref/int chunks are 5 bytes and the offsets scale accordingly.
/// ```
pub(crate) mod body {
    /// Record length in bytes. Spec §5.2.
    pub(crate) const LEN: usize = 61;
    /// Offset of `chunk1_history_body_flags` (`sab_ref8`). Spec §5.2.
    pub(crate) const CHUNK1_HISTORY_BODY_FLAGS: usize = 16;
    /// Offset of `chunk3_first_lump` (`sab_ref8`). Spec §5.2.
    pub(crate) const CHUNK3_FIRST_LUMP: usize = 34;
    /// Offset of `chunk4_first_wire` (`sab_ref8`). Spec §5.2.
    pub(crate) const CHUNK4_FIRST_WIRE: usize = 43;
    /// Offset of `chunk5_transform` (`sab_ref8`). Spec §5.2.
    pub(crate) const CHUNK5_TRANSFORM: usize = 52;
}

/// Byte offsets for the `lump` record.
///
/// Spec §5.2. Record length 61 B.
///
/// ```text
/// `chunk[0]` is the attribute-chain head and `chunk[3]` is the next sibling lump; the spec states no offset for either.
/// ```
pub(crate) mod lump {
    /// Record length in bytes. Spec §5.2.
    pub(crate) const LEN: usize = 61;
    /// Offset of `reserved_slot` (`sab_ref8`). Spec §5.2.
    pub(crate) const RESERVED_SLOT: usize = 27;
    /// Offset of `chunk4_first_shell` (`sab_ref8`). Spec §5.2.
    pub(crate) const CHUNK4_FIRST_SHELL: usize = 43;
    /// Offset of `chunk5_owner_body` (`sab_ref8`). Spec §5.2.
    pub(crate) const CHUNK5_OWNER_BODY: usize = 52;
}

/// Byte offsets for the `shell` record.
///
/// Spec §5.2. Record length 80 B.
pub(crate) mod shell {
    /// Record length in bytes. Spec §5.2.
    pub(crate) const LEN: usize = 80;
    /// Offset of `chunk5_first_face` (`sab_ref8`). Spec §5.2.
    pub(crate) const CHUNK5_FIRST_FACE: usize = 53;
    /// Offset of `chunk6_wire` (`sab_ref8`). Spec §5.2.
    pub(crate) const CHUNK6_WIRE: usize = 62;
    /// Offset of `chunk7_owner` (`sab_ref8`). Spec §5.2.
    pub(crate) const CHUNK7_OWNER: usize = 71;
}

/// Byte offsets for the `face` record.
///
/// Spec §5.2. Record length 81 B.
///
/// ```text
/// Single-sided faces end after `sides`. A double-sided face carries one further chunk, `+81 chunk[10] containment`, which is outside this fixed 81-byte extent.
/// ```
pub(crate) mod face {
    /// Record length in bytes. Spec §5.2.
    pub(crate) const LEN: usize = 81;
    /// Offset of `chunk1_history_face_flags` (`sab_ref8`). Spec §5.2.
    pub(crate) const CHUNK1_HISTORY_FACE_FLAGS: usize = 16;
    /// Offset of `chunk3_next_face` (`sab_ref8`). Spec §5.2.
    pub(crate) const CHUNK3_NEXT_FACE: usize = 34;
    /// Offset of `chunk4_first_loop` (`sab_ref8`). Spec §5.2.
    pub(crate) const CHUNK4_FIRST_LOOP: usize = 43;
    /// Offset of `chunk5_owner_shell` (`sab_ref8`). Spec §5.2.
    pub(crate) const CHUNK5_OWNER_SHELL: usize = 52;
    /// Offset of `chunk7_surface` (`sab_ref8`). Spec §5.2.
    pub(crate) const CHUNK7_SURFACE: usize = 70;
    /// Offset of `chunk8_sense` (`enum8`). Spec §5.2.
    pub(crate) const CHUNK8_SENSE: usize = 79;
    /// Offset of `chunk9_sides` (`enum8`). Spec §5.2.
    pub(crate) const CHUNK9_SIDES: usize = 80;
}

/// Byte offsets for the `vertex` record.
///
/// Spec §5.2. Record length 63 B.
pub(crate) mod vertex {
    /// Record length in bytes. Spec §5.2.
    pub(crate) const LEN: usize = 63;
    /// Offset of `chunk3_owning_edge` (`sab_ref8`). Spec §5.2.
    pub(crate) const CHUNK3_OWNING_EDGE: usize = 36;
    /// Offset of `chunk4_index_flag` (`sab_ref8`). Spec §5.2.
    pub(crate) const CHUNK4_INDEX_FLAG: usize = 45;
    /// Offset of `chunk5_point` (`sab_ref8`). Spec §5.2.
    pub(crate) const CHUNK5_POINT: usize = 54;
}

/// Byte offsets for the `point` record.
///
/// Spec §5.3. Record length 60 B.
///
/// Dialects: `acis:asm-binaryfile-8`.
///
/// ```text
/// The record terminates immediately after the position and carries no trailing reference-count integer.
/// ```
pub(crate) mod point {
    /// Record length in bytes. Spec §5.3.
    pub(crate) const LEN: usize = 60;
    /// Offset of `record_head` (`bytes[8]`). Spec §5.3.
    pub(crate) const RECORD_HEAD: usize = 0;
    /// Offset of `entity_base_0` (`sab_ref8`). Spec §5.3.
    pub(crate) const ENTITY_BASE_0: usize = 8;
    /// Offset of `entity_base_1` (`sab_ref8`). Spec §5.3.
    pub(crate) const ENTITY_BASE_1: usize = 17;
    /// Offset of `entity_base_2` (`sab_ref8`). Spec §5.3.
    pub(crate) const ENTITY_BASE_2: usize = 26;
    /// Offset of `position` (`sab_position`). Spec §5.3.
    pub(crate) const POSITION: usize = 35;
}
