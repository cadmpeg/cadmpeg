// SPDX-License-Identifier: Apache-2.0
//! Parse Spatial ACIS `BinaryFile` headers and locate solved SAB records.
//!
//! The admitted ACIS 217 and 218 streams use the 32-bit SAB header layout:
//! a 15-byte `ACIS BinaryFile` magic, four little-endian `u32` words, three
//! `0x07`-tagged strings, and three `0x06`-tagged tolerance doubles. The SAB
//! record stream begins immediately after the doubles.

use crate::kernel_header::RefWidth;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

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
pub fn parse(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<Option<BinaryHeader>, CodecError> {
    if !has_acis_magic(bytes) {
        return Ok(None);
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
    } = read_string_region(ctx, bytes, acis_bf4::LEN)?;
    header.product_family = family;
    header.product_version = version;
    header.save_date = date;
    header.scale = scale;
    header.linear = linear;
    header.angular = angular;
    Ok(Some(BinaryHeader {
        width: RefWidth::Four,
        metadata: header,
    }))
}

/// Byte offset immediately after the three strings and three doubles.
pub fn record_stream_start(bytes: &[u8]) -> Option<usize> {
    if !has_acis_magic(bytes) {
        return None;
    }
    let (strings, doubles, position) = scan_string_region(bytes, acis_bf4::LEN);
    (strings == 3 && doubles == 3).then_some(position)
}

/// Byte offset immediately after the three strings and three doubles, using
/// an already-parsed ACIS header.
pub fn record_stream_start_with_header(bytes: &[u8], header: &BinaryHeader) -> Option<usize> {
    if header.width != RefWidth::Four {
        return None;
    }
    let (strings, doubles, position) = scan_string_region(bytes, acis_bf4::LEN);
    (strings == 3 && doubles == 3).then_some(position)
}

/// Exact boundary between solved records and a legacy `delta_state` history
/// partition.
pub fn solved_record_limit(bytes: &[u8]) -> Option<usize> {
    if !has_acis_magic(bytes) {
        return None;
    }
    let flags = View::u32_le_at(bytes, acis_bf4::FLAGS)?;
    if u64::from(flags) & crate::kernel_header::HISTORY_PARTITION_FLAG == 0 {
        return None;
    }
    let start = record_stream_start(bytes)?;
    crate::sab::scan_history_boundary(bytes, start, RefWidth::Four, None)
}

/// Exact solved-record boundary, using an already-parsed ACIS header.
pub fn solved_record_limit_with_header(bytes: &[u8], header: &BinaryHeader) -> Option<usize> {
    if !header.metadata.has_history_partition() {
        return None;
    }
    let start = record_stream_start_with_header(bytes, header)?;
    crate::sab::scan_history_boundary(bytes, start, RefWidth::Four, None)
}

#[cfg(test)]
mod tests {
    use super::{parse, record_stream_start, solved_record_limit, MAGIC};

    #[test]
    fn acis_product_string_refuses_retained_limit_before_copy() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let mut bytes = MAGIC.to_vec();
        bytes.resize(crate::layout::acisheader_binaryfile4::LEN, 0);
        bytes.extend_from_slice(&[7, 4]);
        bytes.extend_from_slice(b"ACIS");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 3;
        let (limited, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        assert!(matches!(
            parse(&limited, &bytes),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain kernel header product string"
        ));
        let (service, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        assert_eq!(
            parse(&service, &bytes)
                .unwrap()
                .unwrap()
                .metadata
                .product_family
                .as_deref(),
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

        let header = parse(&cadmpeg_test_support::service_decode_context(), &bytes)
            .expect("service policy admits header")
            .expect("ACIS header");
        assert_eq!(header.width.bytes(), 4);
        assert_eq!(header.metadata.save_format_version, Some(21_800));
        assert_eq!(header.metadata.entity_count, Some(2));
        assert_eq!(header.metadata.flags, Some(13));
        assert_eq!(record_stream_start(&bytes), Some(record_start));
        assert_eq!(solved_record_limit(&bytes), Some(history_start));
    }
}
