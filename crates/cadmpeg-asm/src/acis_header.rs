// SPDX-License-Identifier: Apache-2.0
//! Parse Spatial ACIS `BinaryFile` headers and locate solved SAB records.
//!
//! The admitted ACIS 217 and 218 streams use the 32-bit SAB header layout:
//! a 15-byte `ACIS BinaryFile` magic, four little-endian `u32` words, three
//! `0x07`-tagged strings, and three `0x06`-tagged tolerance doubles. The SAB
//! record stream begins immediately after the doubles.

use crate::kernel_header::RefWidth;
use cadmpeg_core::decode::View;

use crate::kernel_header::{
    read_string_region, scan_string_region, BinaryHeader, HeaderRegion, KernelHeader,
};
use crate::layout::acisheader_binaryfile4 as acis_bf4;

/// Exact binary ACIS magic, without a width suffix.
pub const MAGIC: &[u8; 15] = b"ACIS BinaryFile";

/// Whether `bytes` starts with the binary ACIS magic and at least one header
/// byte follows it.
pub fn has_acis_magic(bytes: &[u8]) -> bool {
    bytes.len() >= 16 && bytes.starts_with(MAGIC)
}

/// Parse the shared kernel metadata from a 32-bit ACIS binary header.
pub fn parse(bytes: &[u8]) -> Option<BinaryHeader> {
    if !has_acis_magic(bytes) {
        return None;
    }
    let mut header = KernelHeader {
        save_format_version: View::u32_le_at(bytes, acis_bf4::SAVE_FORMAT_VERSION),
        entity_count: View::u32_le_at(bytes, acis_bf4::ENTITY_COUNT).map(u64::from),
        flags: View::u32_le_at(bytes, acis_bf4::FLAGS).map(u64::from),
        product_family: None,
        product_version: None,
        save_date: None,
        scale: None,
        linear: None,
        angular: None,
    };
    let HeaderRegion {
        strings: [family, version, date],
        doubles: [scale, linear, angular],
    } = read_string_region(bytes, acis_bf4::LEN);
    header.product_family = family;
    header.product_version = version;
    header.save_date = date;
    header.scale = scale;
    header.linear = linear;
    header.angular = angular;
    Some(BinaryHeader {
        width: RefWidth::Four,
        metadata: header,
    })
}

/// Byte offset immediately after the three strings and three doubles.
pub fn record_stream_start(bytes: &[u8]) -> Option<usize> {
    if !has_acis_magic(bytes) {
        return None;
    }
    let (strings, doubles, position) = scan_string_region(bytes, acis_bf4::LEN);
    let version = View::u32_le_at(bytes, acis_bf4::SAVE_FORMAT_VERSION);
    (strings == 3 && doubles == 3).then(|| skip_key_string(bytes, position, version))
}

/// Tag of the true flag that announces the key string.
const KEY_FLAG_TRUE: u8 = 0x0a;
/// Tag of a one-byte-length string.
const SHORT_STRING_TAG: u8 = 0x07;

/// Offset past the key string that closes the header of a save format from 1100:
/// a true flag, then one short string.
fn skip_key_string(bytes: &[u8], position: usize, version: Option<u32>) -> usize {
    if !version
        .is_some_and(|version| crate::kernel_header::EXTENDED_ACIS_SAVE_FORMATS.contains(&version))
    {
        return position;
    }
    let (Some(&KEY_FLAG_TRUE), Some(&SHORT_STRING_TAG), Some(&length)) = (
        bytes.get(position),
        bytes.get(position + 1),
        bytes.get(position + 2),
    ) else {
        return position;
    };
    let end = position + 3 + usize::from(length);
    if end <= bytes.len() {
        end
    } else {
        position
    }
}

/// Byte offset immediately after the three strings and three doubles, using
/// an already-parsed ACIS header.
pub fn record_stream_start_with_header(bytes: &[u8], header: &BinaryHeader) -> Option<usize> {
    if header.width != RefWidth::Four {
        return None;
    }
    let (strings, doubles, position) = scan_string_region(bytes, acis_bf4::LEN);
    (strings == 3 && doubles == 3)
        .then(|| skip_key_string(bytes, position, header.metadata.save_format_version))
}

/// Exact boundary between solved records and a legacy `delta_state` history
/// partition.
pub fn solved_record_limit(bytes: &[u8]) -> Option<usize> {
    let (start, width) = (|| {
        if !has_acis_magic(bytes) {
            return None;
        }
        let flags = View::u32_le_at(bytes, acis_bf4::FLAGS)?;
        if u64::from(flags) & crate::kernel_header::HISTORY_PARTITION_FLAG == 0 {
            return None;
        }
        Some((record_stream_start(bytes)?, RefWidth::Four))
    })()?;
    crate::sab::scan_history_boundary(bytes, start, width, None)
}

/// Exact solved-record boundary, using an already-parsed ACIS header.
pub fn solved_record_limit_with_header(bytes: &[u8], header: &BinaryHeader) -> Option<usize> {
    if !header.metadata.has_history_partition() {
        return None;
    }
    let start = record_stream_start_with_header(bytes, header)?;
    crate::sab::scan_history_boundary(bytes, start, header.width, None)
}

#[cfg(test)]
mod tests {
    use super::{parse, record_stream_start, solved_record_limit, MAGIC};

    #[test]
    fn acis_header_reads_the_product_string() {
        let mut bytes = MAGIC.to_vec();
        bytes.resize(crate::layout::acisheader_binaryfile4::LEN, 0);
        bytes.extend_from_slice(&[7, 4]);
        bytes.extend_from_slice(b"ACIS");
        assert_eq!(
            parse(&bytes).unwrap().metadata.product_family.as_deref(),
            Some("ACIS")
        );
    }

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
        assert_eq!(header.width.bytes(), 4);
        assert_eq!(header.metadata.save_format_version, Some(21_800));
        assert_eq!(header.metadata.entity_count, Some(2));
        assert_eq!(header.metadata.flags, Some(13));
        assert_eq!(record_stream_start(&bytes), Some(record_start));
        assert_eq!(solved_record_limit(&bytes), Some(history_start));
    }
}
