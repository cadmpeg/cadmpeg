// SPDX-License-Identifier: Apache-2.0
//! Byte-offset and value constants generated from `docs/layouts/creo.toml`.
//!
//! Do not edit by hand. Regenerate with:
//! `UPDATE_LAYOUT_CODE=1 cargo test -p cadmpeg --test layout_tables`.

#![allow(dead_code)] // Not every generated constant is referenced yet.

/// Tag constants from the table inventory.
pub(crate) mod token {
    /// `named-record header` (`e0`). Spec §2.2.
    pub(crate) const NAMED_RECORD_HEADER: [u8; 2] = *b"e0";
    /// `array opener` (`f8`). Spec §2.2.
    pub(crate) const ARRAY_OPENER: [u8; 2] = *b"f8";
    /// `count-bounded scalar body` (`f9`). Spec §2.2.
    pub(crate) const COUNT_BOUNDED_SCALAR_BODY: [u8; 2] = *b"f9";
    /// `entity reference` (`f7`). Spec §2.2.
    pub(crate) const ENTITY_REFERENCE: [u8; 2] = *b"f7";
    /// `array close` (`fb`). Spec §2.2.
    pub(crate) const ARRAY_CLOSE: [u8; 2] = *b"fb";
    /// `nested compound-body opener or continuation` (`e2`). Spec §2.2.
    pub(crate) const NESTED_COMPOUND_BODY_OPENER_OR_CONTINUATION: [u8; 2] = *b"e2";
    /// `compound close or row terminator` (`e3`). Spec §2.2.
    pub(crate) const COMPOUND_CLOSE_OR_ROW_TERMINATOR: [u8; 2] = *b"e3";
    /// `IEEE-fill, byte0 3F, repeated fill` (`29`). Spec §Three-byte IEEE-fill form.
    pub(crate) const IEEE_FILL_BYTE0_3F_REPEATED_FILL: u8 = 29;
    /// `IEEE-fill, byte0 3F, zero fill` (`2a`). Spec §Three-byte IEEE-fill form.
    pub(crate) const IEEE_FILL_BYTE0_3F_ZERO_FILL: [u8; 2] = *b"2a";
    /// `IEEE-fill, byte0 40, repeated fill` (`2e`). Spec §Three-byte IEEE-fill form.
    pub(crate) const IEEE_FILL_BYTE0_40_REPEATED_FILL: [u8; 2] = *b"2e";
    /// `IEEE-fill, byte0 40, zero fill` (`2f`). Spec §Three-byte IEEE-fill form.
    pub(crate) const IEEE_FILL_BYTE0_40_ZERO_FILL: [u8; 2] = *b"2f";
    /// `IEEE-fill, byte0 BF, repeated fill` (`42`). Spec §Three-byte IEEE-fill form.
    pub(crate) const IEEE_FILL_BYTE0_BF_REPEATED_FILL: u8 = 42;
    /// `IEEE-fill, byte0 BF, zero fill` (`43`). Spec §Three-byte IEEE-fill form.
    pub(crate) const IEEE_FILL_BYTE0_BF_ZERO_FILL: u8 = 43;
    /// `IEEE-fill, byte0 C0, repeated fill` (`47`). Spec §Three-byte IEEE-fill form.
    pub(crate) const IEEE_FILL_BYTE0_C0_REPEATED_FILL: u8 = 47;
    /// `IEEE-fill, byte0 C0, zero fill` (`48`). Spec §Three-byte IEEE-fill form.
    pub(crate) const IEEE_FILL_BYTE0_C0_ZERO_FILL: u8 = 48;
}

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
    /// Offset of `prefix` (`bytes[8]`). Spec §1.
    pub(crate) const PREFIX: usize = 0;
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
