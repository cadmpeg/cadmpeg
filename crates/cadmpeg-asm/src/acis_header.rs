// SPDX-License-Identifier: Apache-2.0
//! Parse Spatial ACIS `BinaryFile` headers and locate solved SAB records.
//!
//! The admitted ACIS 217 and 218 streams use the 32-bit SAB header layout:
//! a 15-byte `ACIS BinaryFile` magic, four little-endian `u32` words, three
//! `0x07`-tagged strings, and three `0x06`-tagged tolerance doubles. The SAB
//! record stream begins immediately after the doubles.

use cadmpeg_core::decode::View;

use crate::kernel_header::{read_string_region, KernelHeader};
use crate::layout::acisheader_binaryfile4 as acis_bf4;

/// Exact binary ACIS magic, without a width suffix.
pub const MAGIC: &[u8; 15] = b"ACIS BinaryFile";

/// Whether `bytes` starts with the binary ACIS magic and at least one header
/// byte follows it.
pub fn has_acis_magic(bytes: &[u8]) -> bool {
    bytes.len() >= 16 && bytes.starts_with(MAGIC)
}

/// Parse the shared kernel metadata from a 32-bit ACIS binary header.
pub fn parse(bytes: &[u8]) -> Option<KernelHeader> {
    if !has_acis_magic(bytes) {
        return None;
    }
    let mut header = KernelHeader {
        width: 4,
        save_format_version: View::u32_le_at(bytes, acis_bf4::SAVE_FORMAT_VERSION),
        record_count: View::u32_le_at(bytes, acis_bf4::RECORD_COUNT),
        entity_count: View::u32_le_at(bytes, acis_bf4::ENTITY_COUNT).map(u64::from),
        flags: View::u32_le_at(bytes, acis_bf4::FLAGS).map(u64::from),
        product_family: None,
        product_version: None,
        save_date: None,
        scale: None,
        linear: None,
        angular: None,
    };
    let (strings, doubles, _) = read_string_region(bytes, acis_bf4::LEN);
    let mut strings = strings.into_iter();
    header.product_family = strings.next();
    header.product_version = strings.next();
    header.save_date = strings.next();
    let mut doubles = doubles.into_iter();
    header.scale = doubles.next();
    header.linear = doubles.next();
    header.angular = doubles.next();
    Some(header)
}

/// Byte offset immediately after the three strings and three doubles.
pub fn record_stream_start(bytes: &[u8]) -> Option<usize> {
    parse(bytes)?;
    let (strings, doubles, position) = read_string_region(bytes, acis_bf4::LEN);
    (strings.len() == 3 && doubles.len() == 3).then_some(position)
}

/// Exact boundary between solved records and a legacy `delta_state` history
/// partition.
pub fn solved_record_limit(bytes: &[u8]) -> Option<usize> {
    let header = parse(bytes)?;
    if !header.has_history_partition() {
        return None;
    }
    let start = record_stream_start(bytes)?;
    let records = crate::sab::frame(bytes, start, bytes.len(), 4).ok()?;
    let mut next = match records.last() {
        Some(record) => record.offset.checked_add(record.len)?,
        None => start,
    };
    while bytes.get(next) == Some(&0x11) {
        next += 1;
    }
    exact_identifier_at(bytes, next, "delta_state").then_some(next)
}

fn exact_identifier_at(bytes: &[u8], at: usize, expected: &str) -> bool {
    let Some((&0x0d, rest)) = bytes.get(at..).and_then(|tail| tail.split_first()) else {
        return false;
    };
    let Some((&length, payload)) = rest.split_first() else {
        return false;
    };
    usize::from(length) == expected.len()
        && payload.get(..usize::from(length)) == Some(expected.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_32_bit_acis_header_and_record_boundary() {
        let mut bytes = Vec::from(MAGIC.as_slice());
        for value in [21_800_u32, 0, 2, 13] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in ["Inventor", "ASM 218.0 synthetic", "Synthetic"] {
            bytes.push(0x07);
            bytes.push(u8::try_from(value.len()).expect("short string"));
            bytes.extend_from_slice(value.as_bytes());
        }
        for value in [10.0_f64, 1.0e-6, 1.0e-10] {
            bytes.push(0x06);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let record_start = bytes.len();
        bytes.extend_from_slice(&[0x0d, 9]);
        bytes.extend_from_slice(b"asmheader");
        bytes.push(0x11);
        let history_start = bytes.len();
        bytes.extend_from_slice(&[0x0d, 11]);
        bytes.extend_from_slice(b"delta_state");

        let header = parse(&bytes).expect("ACIS header");
        assert_eq!(header.width, 4);
        assert_eq!(header.save_format_version, Some(21_800));
        assert_eq!(header.entity_count, Some(2));
        assert_eq!(header.format_revision(), Some(6));
        assert_eq!(record_stream_start(&bytes), Some(record_start));
        assert_eq!(solved_record_limit(&bytes), Some(history_start));
    }
}
