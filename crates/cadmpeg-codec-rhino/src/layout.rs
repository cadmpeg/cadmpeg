// SPDX-License-Identifier: Apache-2.0
//! Byte-offset constants generated from `docs/layouts/rhino.toml`.
//!
//! Do not edit by hand. Regenerate with:
//! `UPDATE_LAYOUT_CODE=1 cargo test -p cadmpeg --test layout_tables`.

#![allow(dead_code)] // Not every generated constant is referenced yet.

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
    /// Offset of `magic` (`bytes[24]`). Spec §2.
    pub(crate) const MAGIC: usize = 0;
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
    /// Offset of `data1` (`u32`, little-endian). Spec §3.3.
    pub(crate) const DATA1: usize = 0;
    /// Offset of `data2` (`u16`, little-endian). Spec §3.3.
    pub(crate) const DATA2: usize = 4;
    /// Offset of `data3` (`u16`, little-endian). Spec §3.3.
    pub(crate) const DATA3: usize = 6;
    /// Offset of `data4` (`bytes[8]`). Spec §3.3.
    pub(crate) const DATA4: usize = 8;
}

/// Byte offsets for the `long_chunk_header_v2` record.
///
/// Spec §4. Record length 8 B.
///
/// ```text
/// Archive versions below 50. The length word is `i32` below archive version 50 and `i64` from 50; the 8-byte total here is the below-50 form. `declared_length` bytes of body follow and include the trailing checksum when present.
/// ```
pub(crate) mod long_chunk_header_v2 {
    /// Record length in bytes. Spec §4.
    pub(crate) const LEN: usize = 8;
    /// Offset of `typecode` (`u32`, little-endian). Spec §4.
    pub(crate) const TYPECODE: usize = 0;
    /// Offset of `declared_length` (`i32`, little-endian). Spec §4.
    pub(crate) const DECLARED_LENGTH: usize = 4;
}

/// Byte offsets for the `long_chunk_header_v50` record.
///
/// Spec §4. Record length 12 B.
///
/// ```text
/// Archive versions 50 and above widen the length word to `i64`.
/// ```
pub(crate) mod long_chunk_header_v50 {
    /// Record length in bytes. Spec §4.
    pub(crate) const LEN: usize = 12;
    /// Offset of `typecode` (`u32`, little-endian). Spec §4.
    pub(crate) const TYPECODE: usize = 0;
    /// Offset of `declared_length` (`i64`, little-endian). Spec §4.
    pub(crate) const DECLARED_LENGTH: usize = 4;
}

/// Byte offsets for the `endoffile_record_v50` record.
///
/// Spec §5. Record length 20 B.
///
/// ```text
/// `TCODE_ENDOFFILE = 0x00007fff` is a long, unchecksummed chunk whose declared length is exactly the file-size field width. The stored size includes the 32-byte header, all preceding chunks, the EOF typecode, the EOF value field, and the file-size field. Below archive version 50 the length and size words are four bytes each and the record is 12 bytes. The 20-byte total is derived from the three stated widths; the spec states no total.
/// ```
pub(crate) mod endoffile_record_v50 {
    /// Record length in bytes. Spec §5.
    pub(crate) const LEN: usize = 20;
    /// Offset of `typecode` (`u32`, little-endian). Spec §5.
    pub(crate) const TYPECODE: usize = 0;
    /// Offset of `declared_length` (`i64`, little-endian). Spec §5.
    pub(crate) const DECLARED_LENGTH: usize = 4;
    /// Offset of `file_size` (`u64`, little-endian). Spec §5.
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
    /// Offset of `class_uuid` (`on_uuid`). Spec §7.
    pub(crate) const CLASS_UUID: usize = 0;
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
    /// Record length in bytes. Spec §10.
    pub(crate) const LEN: usize = 9;
    /// Offset of `uncompressed_size` (`u32`, little-endian). Spec §10.
    pub(crate) const UNCOMPRESSED_SIZE: usize = 0;
    /// Offset of `crc32` (`u32`, little-endian). Spec §10.
    pub(crate) const CRC32: usize = 4;
    /// Offset of `method` (`u8`). Spec §10.
    pub(crate) const METHOD: usize = 8;
}

/// Byte offsets for the `subd_component_base` record.
///
/// Spec §19.5. Record length 10 B.
///
/// ```text
/// The field list is stated in order; the 10-byte total follows from the three stated widths.
/// ```
pub(crate) mod subd_component_base {
    /// Record length in bytes. Spec §19.5.
    pub(crate) const LEN: usize = 10;
    /// Offset of `archive_id` (`u32`, little-endian). Spec §19.5.
    pub(crate) const ARCHIVE_ID: usize = 0;
    /// Offset of `component_id` (`u32`, little-endian). Spec §19.5.
    pub(crate) const COMPONENT_ID: usize = 4;
    /// Offset of `subdivision_level` (`u16`, little-endian). Spec §19.5.
    pub(crate) const SUBDIVISION_LEVEL: usize = 8;
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
    pub(crate) const LEN: usize = 8;
    /// Offset of `major` (`i32`, little-endian). Spec §5.
    pub(crate) const MAJOR: usize = 0;
    /// Offset of `minor` (`i32`, little-endian). Spec §5.
    pub(crate) const MINOR: usize = 4;
}
