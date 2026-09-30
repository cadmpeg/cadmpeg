// SPDX-License-Identifier: Apache-2.0
//! Byte-offset and value constants generated from `docs/layouts/protein.toml`.
//!
//! Do not edit by hand. Regenerate with:
//! `UPDATE_LAYOUT_CODE=1 cargo test -p cadmpeg --test layout_tables`.

/// Byte offsets for the `instance_stream_header` record.
///
/// Spec §2. Record length 16 B.
pub(crate) mod instance_stream_header {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 16;
    /// Offset of `declared_size` (`u32`, little-endian). Spec §2.
    pub(crate) const DECLARED_SIZE: usize = 0;
}

/// Byte offsets for the `record_start_page` record.
///
/// Spec §3. Record length 136 B.
pub(crate) mod record_start_page {
    /// Record length in bytes. Spec §3.
    pub(crate) const LEN: usize = 136;
    /// Offset of `marker` (`bytes[4]`). Spec §3.
    pub(crate) const MARKER: usize = 4;
    /// Stated value of `marker` (`bytes[4]`). Spec §3.
    pub(crate) const MARKER_VALUE: [u8; 4] = [0x80, 0x00, 0x01, 0x00];
    /// Offset of `body` (`bytes[128]`). Spec §3.
    pub(crate) const BODY: usize = 8;
}

/// Byte offsets for the `continuation_page` record.
///
/// Spec §3. Record length 136 B.
pub(crate) mod continuation_page {
    /// Offset of `marker` (`bytes[4]`). Spec §3.
    pub(crate) const MARKER: usize = 4;
    /// Stated value of `marker` (`bytes[4]`). Spec §3.
    pub(crate) const MARKER_VALUE: [u8; 4] = [0x80, 0x00, 0x00, 0x00];
    /// Offset of `body` (`bytes[128]`). Spec §3.
    pub(crate) const BODY: usize = 8;
}

/// Byte offsets for the `terminal_page` record.
///
/// Spec §3. Record length 136 B.
pub(crate) mod terminal_page {
    /// Offset of `marker` (`bytes[4]`). Spec §3.
    pub(crate) const MARKER: usize = 0;
    /// Stated value of `marker` (`bytes[4]`). Spec §3.
    pub(crate) const MARKER_VALUE: [u8; 4] = [0xff, 0xff, 0xff, 0xff];
    /// Offset of `used` (`u16`, little-endian). Spec §3.
    pub(crate) const USED: usize = 4;
    /// Offset of `body` (`bytes[128]`). Spec §3.
    pub(crate) const BODY: usize = 8;
}
