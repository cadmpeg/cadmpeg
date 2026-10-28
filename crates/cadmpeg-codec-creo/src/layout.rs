// SPDX-License-Identifier: Apache-2.0
//! Byte-offset and value constants generated from `docs/layouts/creo.toml`.
//!
//! Do not edit by hand. Regenerate with:
//! `UPDATE_LAYOUT_CODE=1 cargo test -p cadmpeg --test layout_tables`.

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
    /// Stated value of `magic` (`bytes[2]`). Spec §1.
    pub(crate) const MAGIC_VALUE: [u8; 2] = [0x1f, 0x9d];
    /// Offset of `flags` (`u8`). Spec §1.
    pub(crate) const FLAGS: usize = 2;
}

/// Byte offsets for the `cmnm_model_name_record` record.
///
/// Spec §1. Record length 11 B.
///
/// ```text
/// Fixed prefix only; `hhh` bytes of ASCII name follow at +11, then trailing ASCII space padding. A unique valid record supplies the header model filename; a repeated or malformed record does not establish identity. Binary model-data may provide the named `model_name` identity field described in the specification.
/// ```
pub(crate) mod cmnm_model_name_record {
    /// Record length in bytes. Spec §1.
    pub(crate) const LEN: usize = 11;
    /// Stated value of `prefix` (`bytes[8]`). Spec §1.
    pub(crate) const PREFIX_VALUE: [u8; 8] = *b"#- CMNM ";
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

/// Byte offsets for the `model_double_xar_metric_scale` record.
///
/// Spec §Constants and cache references. Record length 4 B.
pub(crate) mod model_double_xar_metric_scale {
    /// Record length in bytes. Spec §Constants and cache references.
    pub(crate) const LEN: usize = 4;
    /// Stated value of `image` (`bytes[4]`). Spec §Constants and cache references.
    pub(crate) const IMAGE_VALUE: [u8; 4] = [0x07, 0x23, 0x11, 0x2e];
}

/// Byte offsets for the `model_double_xar_imperial_scale` record.
///
/// Spec §Constants and cache references. Record length 11 B.
pub(crate) mod model_double_xar_imperial_scale {
    /// Record length in bytes. Spec §Constants and cache references.
    pub(crate) const LEN: usize = 11;
    /// Stated value of `opener` (`u8`). Spec §Constants and cache references.
    pub(crate) const OPENER_VALUE: u8 = 40;
    /// Offset of `trailer` (`bytes[3]`). Spec §Constants and cache references.
    pub(crate) const TRAILER: usize = 8;
    /// Stated value of `trailer` (`bytes[3]`). Spec §Constants and cache references.
    pub(crate) const TRAILER_VALUE: [u8; 3] = [0x23, 0x11, 0x2e];
}

/// Byte offsets for the `model_double_xar_control` record.
///
/// Spec §Constants and cache references. Record length 4 B.
pub(crate) mod model_double_xar_control {
    /// Record length in bytes. Spec §Constants and cache references.
    pub(crate) const LEN: usize = 4;
    /// Stated value of `image` (`bytes[4]`). Spec §Constants and cache references.
    pub(crate) const IMAGE_VALUE: [u8; 4] = [0xe8, 0x26, 0xd6, 0x95];
}

/// Byte offsets for the `section_full_width_scalar` record.
///
/// Spec §5. Record length 9 B.
///
/// ```text
/// The complete owning scalar field supplies the boundary. Only supported coordinate roles interpret the payload as big-endian binary64; other roles retain opaque bytes.
/// ```
pub(crate) mod section_full_width_scalar {
    /// Record length in bytes. Spec §5.
    pub(crate) const LEN: usize = 9;
    /// Stated value of `opener` (`bytes[1]`). Spec §5.
    pub(crate) const OPENER_VALUE: [u8; 1] = [0xed];
    /// Offset of `payload` (`bytes[8]`). Spec §5.
    pub(crate) const PAYLOAD: usize = 1;
}

/// Byte offsets for the `type24_cap_envelope_control_prefix` record.
///
/// Spec §3.2. Record length 22 B.
///
/// ```text
/// Physical scalar images remain opaque; no numeric role or carrier chart follows from this prefix.
/// ```
pub(crate) mod type24_cap_envelope_control_prefix {
    /// Record length in bytes. Spec §3.2.
    pub(crate) const LEN: usize = 22;
    /// Offset of `first_control` (`bytes[7]`). Spec §3.2.
    pub(crate) const FIRST_CONTROL: usize = 0;
    /// Offset of `separator` (`u8`). Spec §3.2.
    pub(crate) const SEPARATOR: usize = 7;
    /// Stated value of `separator` (`u8`). Spec §3.2.
    pub(crate) const SEPARATOR_VALUE: u8 = 24;
    /// Offset of `second_control` (`bytes[7]`). Spec §3.2.
    pub(crate) const SECOND_CONTROL: usize = 8;
    /// Offset of `third_control` (`bytes[7]`). Spec §3.2.
    pub(crate) const THIRD_CONTROL: usize = 15;
}
