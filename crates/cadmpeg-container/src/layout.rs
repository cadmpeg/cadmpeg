// SPDX-License-Identifier: Apache-2.0
//! Byte-offset and value constants generated from `docs/layouts/zip.toml`.
//!
//! Do not edit by hand. Regenerate with:
//! `UPDATE_LAYOUT_CODE=1 cargo test -p cadmpeg --test layout_tables`.

/// Byte offsets for the `digital_signature` record.
///
/// Spec §3. Record length 6 B.
pub(crate) mod digital_signature {
    /// Record length in bytes. Spec §3.
    pub(crate) const LEN: usize = 6;
    /// Offset of `data_length` (`u16`, little-endian). Spec §3.
    pub(crate) const DATA_LENGTH: usize = 4;
}

/// Byte offsets for the `central_header` record.
///
/// Spec §1. Record length 46 B.
pub(crate) mod central_header {
    /// Record length in bytes. Spec §1.
    pub(crate) const LEN: usize = 46;
    /// Offset of `flags` (`u16`, little-endian). Spec §1.
    pub(crate) const FLAGS: usize = 8;
    /// Offset of `compression` (`u16`, little-endian). Spec §1.
    pub(crate) const COMPRESSION: usize = 10;
    /// Offset of `crc32` (`u32`, little-endian). Spec §1.
    pub(crate) const CRC32: usize = 16;
    /// Offset of `compressed_length` (`u32`, little-endian). Spec §1.
    pub(crate) const COMPRESSED_LENGTH: usize = 20;
    /// Offset of `expanded_length` (`u32`, little-endian). Spec §1.
    pub(crate) const EXPANDED_LENGTH: usize = 24;
    /// Offset of `name_length` (`u16`, little-endian). Spec §1.
    pub(crate) const NAME_LENGTH: usize = 28;
    /// Offset of `extra_length` (`u16`, little-endian). Spec §1.
    pub(crate) const EXTRA_LENGTH: usize = 30;
    /// Offset of `comment_length` (`u16`, little-endian). Spec §1.
    pub(crate) const COMMENT_LENGTH: usize = 32;
    /// Offset of `local_header_position` (`u32`, little-endian). Spec §1.
    pub(crate) const LOCAL_HEADER_POSITION: usize = 42;
}

/// Byte offsets for the `local_header` record.
///
/// Spec §2. Record length 30 B.
pub(crate) mod local_header {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 30;
    /// Offset of `name_length` (`u16`, little-endian). Spec §2.
    pub(crate) const NAME_LENGTH: usize = 26;
    /// Offset of `extra_length` (`u16`, little-endian). Spec §2.
    pub(crate) const EXTRA_LENGTH: usize = 28;
}

/// Byte offsets for the `end_record` record.
///
/// Spec §3. Record length 22 B.
pub(crate) mod end_record {
    /// Record length in bytes. Spec §3.
    pub(crate) const LEN: usize = 22;
    /// Offset of `disk_entries` (`u16`, little-endian). Spec §3.
    #[cfg(test)]
    pub(crate) const DISK_ENTRIES: usize = 8;
    /// Offset of `entries` (`u16`, little-endian). Spec §3.
    pub(crate) const ENTRIES: usize = 10;
    /// Offset of `directory_size` (`u32`, little-endian). Spec §3.
    pub(crate) const DIRECTORY_SIZE: usize = 12;
    /// Offset of `directory_position` (`u32`, little-endian). Spec §3.
    pub(crate) const DIRECTORY_POSITION: usize = 16;
    /// Offset of `comment_length` (`u16`, little-endian). Spec §3.
    pub(crate) const COMMENT_LENGTH: usize = 20;
}

/// Byte offsets for the `zip64_end` record.
///
/// Spec §3. Record length 56 B.
pub(crate) mod zip64_end {
    /// Record length in bytes. Spec §3.
    pub(crate) const LEN: usize = 56;
    /// Offset of `record_size` (`u64`, little-endian). Spec §3.
    pub(crate) const RECORD_SIZE: usize = 4;
    /// Offset of `producer_version` (`u16`, little-endian). Spec §3.
    #[cfg(test)]
    pub(crate) const PRODUCER_VERSION: usize = 12;
    /// Offset of `extraction_version` (`u16`, little-endian). Spec §3.
    #[cfg(test)]
    pub(crate) const EXTRACTION_VERSION: usize = 14;
    /// Offset of `disk_entries` (`u64`, little-endian). Spec §3.
    #[cfg(test)]
    pub(crate) const DISK_ENTRIES: usize = 24;
    /// Offset of `entries` (`u64`, little-endian). Spec §3.
    pub(crate) const ENTRIES: usize = 32;
    /// Offset of `directory_size` (`u64`, little-endian). Spec §3.
    pub(crate) const DIRECTORY_SIZE: usize = 40;
    /// Offset of `directory_position` (`u64`, little-endian). Spec §3.
    pub(crate) const DIRECTORY_POSITION: usize = 48;
}

/// Byte offsets for the `zip64_locator` record.
///
/// Spec §3. Record length 20 B.
pub(crate) mod zip64_locator {
    /// Record length in bytes. Spec §3.
    pub(crate) const LEN: usize = 20;
    /// Offset of `end_record_position` (`u64`, little-endian). Spec §3.
    pub(crate) const END_RECORD_POSITION: usize = 8;
    /// Offset of `disks` (`u32`, little-endian). Spec §3.
    #[cfg(test)]
    pub(crate) const DISKS: usize = 16;
}
