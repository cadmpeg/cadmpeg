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
pub(crate) mod endoffile_record_wide {
    /// Offset of `file_size` (`u64`, little-endian). Spec §5.
    #[cfg(test)]
    pub(crate) const FILE_SIZE: usize = 12;
}

/// Byte offsets for the `class_uuid_chunk_body` record.
///
/// Spec §7. Record length 20 B.
pub(crate) mod class_uuid_chunk_body {
    /// Record length in bytes. Spec §7.
    pub(crate) const LEN: usize = 20;
    /// Offset of `crc32` (`u32`, little-endian). Spec §7.
    pub(crate) const CRC32: usize = 16;
}

/// Byte offsets for the `compressed_buffer_prologue` record.
///
/// Spec §10. Record length 9 B.
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
pub(crate) mod anonymous_version_prefix {
    /// Record length in bytes. Spec §5.
    #[cfg(test)]
    pub(crate) const LEN: usize = 8;
    /// Offset of `minor` (`i32`, little-endian). Spec §5.
    #[cfg(test)]
    pub(crate) const MINOR: usize = 4;
}
