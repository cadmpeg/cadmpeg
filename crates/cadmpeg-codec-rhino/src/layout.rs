// SPDX-License-Identifier: Apache-2.0
//! Byte-offset and value constants generated from `docs/layouts/rhino.toml`.
//!
//! Do not edit by hand. Regenerate with:
//! `UPDATE_LAYOUT_CODE=1 cargo test -p cadmpeg --test layout_tables`.

/// Tag constants from the table inventory.
pub(crate) mod token {
    /// `TCODE_SHORT` (`0x80000000`). Spec §4.
    pub(crate) const TCODE_SHORT: u32 = 0x8000_0000;
    /// `TCODE_CRC` (`0x00008000`). Spec §4.
    pub(crate) const TCODE_CRC: u32 = 0x0000_8000;
    /// `end of file` (`0x00007fff`). Spec §6.1.
    pub(crate) const END_OF_FILE: u32 = 0x0000_7fff;
    /// `end of table` (`0xffffffff`). Spec §6.1.
    pub(crate) const END_OF_TABLE: u32 = 0xffff_ffff;
}

/// Byte offsets for the `file_header` record.
///
/// Spec §2. Record length 32 B.
///
/// ```text
/// The version field is right-justified decimal text, not a binary integer: leading ASCII spaces then at least one ASCII digit. Version `5` and version `50` are distinct.
/// ```
pub(crate) mod file_header {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 32;
    /// Stated value of `magic` (`bytes[24]`). Spec §2.
    pub(crate) const MAGIC_VALUE: [u8; 24] = *b"3D Geometry File Format ";
    /// Offset of `archive_version` (`bytes[8]`). Spec §2.
    pub(crate) const ARCHIVE_VERSION: usize = 24;
}

/// Byte offsets for the `uuid_wire_form` record.
///
/// Spec §3.3. Record length 16 B.
///
/// ```text
/// The only mixed-endian primitive in the format. The worked example is canonical `4ED7D4DD-E947-11D3-BFE5-0010830122F0`, wire `DD D4 D7 4E 47 E9 D3 11 BF E5 00 10 83 01 22 F0`.
/// ```
pub(crate) mod uuid_wire_form {
    /// Record length in bytes. Spec §3.3.
    pub(crate) const LEN: usize = 16;
    /// Offset of `data2` (`u16`, little-endian). Spec §3.3.
    pub(crate) const DATA2: usize = 4;
    /// Offset of `data3` (`u16`, little-endian). Spec §3.3.
    pub(crate) const DATA3: usize = 6;
    /// Offset of `data4` (`bytes[8]`). Spec §3.3.
    pub(crate) const DATA4: usize = 8;
}

/// Byte offsets for the `long_chunk_header_narrow` record.
///
/// Spec §4. Record length 8 B.
///
/// Dialects: `rhino:archive-1`, `rhino:archive-2`, `rhino:archive-3`, `rhino:archive-4`, `rhino:archive-5`, `rhino:unknown`.
///
/// ```text
/// The narrow form: a 4-byte `i32` length word, selected when the archive-version word is below 50 (`ArchiveVersion::uses_eight_byte_values`). `declared_length` bytes of body follow and include the trailing checksum when present. `rhino:unknown` is keyed on both this record and its wide sibling: the residual row is any version word outside the enumerated ten, and the width follows the word, not the row.
/// ```
pub(crate) mod long_chunk_header_narrow {
    /// Record length in bytes. Spec §4.
    #[cfg(test)]
    pub(crate) const LEN: usize = 8;
}

/// Byte offsets for the `long_chunk_header_wide` record.
///
/// Spec §4. Record length 12 B.
///
/// Dialects: `rhino:archive-50`, `rhino:archive-60`, `rhino:archive-70`, `rhino:archive-80`, `rhino:archive-90`, `rhino:unknown`.
///
/// ```text
/// The wide form: the length word widens to an 8-byte `i64` when the archive-version word is 50 or above. `rhino:unknown` is keyed here for the reason stated on the narrow form.
/// ```
pub(crate) mod long_chunk_header_wide {
    /// Record length in bytes. Spec §4.
    #[cfg(test)]
    pub(crate) const LEN: usize = 12;
    /// Offset of `declared_length` (`i64`, little-endian). Spec §4.
    #[cfg(test)]
    pub(crate) const DECLARED_LENGTH: usize = 4;
}

/// Byte offsets for the `endoffile_record_wide` record.
///
/// Spec §5. Record length 20 B.
///
/// Dialects: `rhino:archive-50`, `rhino:archive-60`, `rhino:archive-70`, `rhino:archive-80`, `rhino:archive-90`, `rhino:unknown`.
///
/// ```text
/// `TCODE_ENDOFFILE = 0x00007fff` is a long, unchecksummed chunk whose declared length is exactly the file-size field width. The stored size includes the 32-byte header, all preceding chunks, the EOF typecode, the EOF value field, and the file-size field. Below archive version 50 the length and size words are four bytes each and the record is 12 bytes; that narrow form has no record of its own here. `rhino:unknown` is keyed because an unenumerated version word at or above 50 reads this layout. The 20-byte total is derived from the three stated widths; the spec states no total.
/// ```
pub(crate) mod endoffile_record_wide {
    /// Offset of `file_size` (`u64`, little-endian). Spec §5.
    #[cfg(test)]
    pub(crate) const FILE_SIZE: usize = 12;
}

/// Byte offsets for the `class_uuid_chunk_body` record.
///
/// Spec §7. Record length 20 B.
///
/// ```text
/// One of the two places the specification states a record body size outright.
/// ```
pub(crate) mod class_uuid_chunk_body {
    /// Record length in bytes. Spec §7.
    pub(crate) const LEN: usize = 20;
    /// Offset of `crc32` (`u32`, little-endian). Spec §7.
    pub(crate) const CRC32: usize = 16;
}

/// Byte offsets for the `compressed_buffer_prologue` record.
///
/// Spec §10. Record length 9 B.
///
/// ```text
/// A zero size ends the buffer immediately: no CRC, method, or body follows, so the prologue collapses to its first four bytes. Method 0 stores the bytes verbatim; method 1 stores one anonymous long chunk whose body is a complete zlib stream.
/// ```
pub(crate) mod compressed_buffer_prologue {
    /// Offset of `crc32` (`u32`, little-endian). Spec §10.
    #[cfg(test)]
    pub(crate) const CRC32: usize = 4;
    /// Offset of `method` (`u8`). Spec §10.
    #[cfg(test)]
    pub(crate) const METHOD: usize = 8;
}

/// Byte offsets for the `anonymous_version_prefix` record.
///
/// Spec §5. Record length 8 B.
///
/// ```text
/// The anonymous form. The packed form is one byte with `major = version >> 4` and `minor = version & 0x0f`. The two forms are not interchangeable.
/// ```
pub(crate) mod anonymous_version_prefix {
    /// Record length in bytes. Spec §5.
    #[cfg(test)]
    pub(crate) const LEN: usize = 8;
    /// Offset of `minor` (`i32`, little-endian). Spec §5.
    #[cfg(test)]
    pub(crate) const MINOR: usize = 4;
}
