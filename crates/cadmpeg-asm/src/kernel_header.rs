// SPDX-License-Identifier: Apache-2.0
//! Kernel header metadata shared by binary ASM, binary ACIS, and text streams.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

/// Integer and reference payload width of a kernel stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefWidth {
    /// Four-byte signed integers and references.
    Four,
    /// Eight-byte signed integers and references.
    Eight,
}

impl RefWidth {
    /// Encoded payload size in bytes.
    #[must_use]
    pub const fn bytes(self) -> usize {
        match self {
            Self::Four => 4,
            Self::Eight => 8,
        }
    }
}

impl std::fmt::Display for RefWidth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.bytes().fmt(f)
    }
}

/// Binary kernel framing with its mandatory integer and reference width.
#[derive(Debug, Clone, PartialEq)]
pub struct BinaryHeader {
    /// Integer and reference width used by the binary record stream.
    pub width: RefWidth,
    /// Encoding-independent kernel metadata.
    pub metadata: KernelHeader,
}

/// The recognized metadata fields of an ASM or ACIS model stream.
#[derive(Debug, Clone, PartialEq)]
pub struct KernelHeader {
    /// ACIS save-format version, encoded as `100 * major + minor`.
    pub save_format_version: Option<u32>,
    /// Entity-count word.
    pub entity_count: Option<u64>,
    /// Kernel flags word.
    pub flags: Option<u64>,
    /// Product family string.
    pub product_family: Option<String>,
    /// Product version string.
    pub product_version: Option<String>,
    /// Save date string.
    pub save_date: Option<String>,
    /// Kernel scale metadata slot. Coordinate decoding does not apply it.
    pub scale: Option<f64>,
    /// Absolute distance tolerance `resabs`, normalized to centimetres.
    pub linear: Option<f64>,
    /// Normal tolerance `resnor`.
    pub angular: Option<f64>,
}

/// Flag bit selecting the optional construction-history partition.
pub const HISTORY_PARTITION_FLAG: u64 = 1;

/// Flag bits 1 to 7, which hold the save format's revision number.
pub const FORMAT_REVISION_FLAGS: u64 = 0xfe;

impl KernelHeader {
    /// Major component of the encoded ACIS save-format version.
    pub fn save_format_major(&self) -> Option<u32> {
        self.save_format_version.map(|version| version / 100)
    }

    /// Minor component of the encoded ACIS save-format version.
    pub fn save_format_minor(&self) -> Option<u32> {
        self.save_format_version.map(|version| version % 100)
    }

    /// Whether the stream header declares a construction-history partition.
    pub fn has_history_partition(&self) -> bool {
        self.flags
            .is_some_and(|flags| flags & HISTORY_PARTITION_FLAG != 0)
    }
}

pub(crate) struct HeaderRegion {
    pub(crate) strings: [Option<String>; 3],
    pub(crate) doubles: [Option<f64>; 3],
}

pub(crate) fn read_string_region(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
) -> Result<HeaderRegion, CodecError> {
    let mut cur = start;
    let mut strings = [None, None, None];
    for slot in &mut strings {
        match read_u8_string_span(ctx, bytes, cur)? {
            Some((value, next)) => {
                *slot = Some(ctx.copy_retained_text(value, "retain kernel header product string")?);
                cur = next;
            }
            None => break,
        }
    }
    let mut doubles = [None, None, None];
    for slot in &mut doubles {
        match read_tagged_f64(bytes, cur) {
            Some((value, next)) => {
                *slot = Some(value);
                cur = next;
            }
            None => break,
        }
    }
    Ok(HeaderRegion { strings, doubles })
}

/// Locate tagged header values without materializing a second copy.
pub(crate) fn scan_string_region(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
) -> Result<(usize, usize, usize), CodecError> {
    let mut cur = start;
    let mut strings = 0;
    while strings < 3 {
        let Some((_, next)) = read_u8_string_span(ctx, bytes, cur)? else {
            break;
        };
        strings += 1;
        cur = next;
    }
    let mut doubles = 0;
    while doubles < 3 {
        let Some((_, next)) = read_tagged_f64(bytes, cur) else {
            break;
        };
        doubles += 1;
        cur = next;
    }
    Ok((strings, doubles, cur))
}

fn read_u8_string_span<'bytes>(
    ctx: &DecodeContext<'_>,
    bytes: &'bytes [u8],
    at: usize,
) -> Result<Option<(&'bytes str, usize)>, CodecError> {
    if bytes.get(at) != Some(&0x07) {
        return Ok(None);
    }
    let Some(length) = bytes.get(at + 1) else {
        return Ok(None);
    };
    let len = usize::from(*length);
    let start = at + 2;
    let Some(end) = start.checked_add(len) else {
        return Ok(None);
    };
    let Some(value_bytes) = bytes.get(start..end) else {
        return Ok(None);
    };
    let Ok(value) = ctx.validate_utf8(value_bytes, "validate kernel header product string")? else {
        return Ok(None);
    };
    Ok(Some((value, end)))
}

fn read_tagged_f64(bytes: &[u8], at: usize) -> Option<(f64, usize)> {
    if *bytes.get(at)? != 0x06 {
        return None;
    }
    let value = View::f64_le_at(bytes, at + 1)?;
    Some((value, at + 9))
}

#[cfg(test)]
mod tests {
    #[test]
    fn binary_header_product_string_refuses_retained_limit() {
        use cadmpeg_core::decode::ResourceDimension;
        let bytes = [0x07, 3, b'a', b'b', b'c'];
        let refusal = crate::test_support::resource_limit_at(
            &bytes,
            ResourceDimension::RetainedBytes,
            "retain kernel header product string",
            |ctx| super::read_string_region(ctx, &bytes, 0),
        );
        assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(refusal.operation, "retain kernel header product string");
    }

    #[test]
    fn binary_header_scan_refuses_utf8_validation_work() {
        use cadmpeg_core::decode::ResourceDimension;
        let bytes = [0x07, 3, b'a', b'b', b'c'];
        let refusal = crate::test_support::resource_limit_at(
            &bytes,
            ResourceDimension::WorkUnits,
            "validate kernel header product string",
            |ctx| super::scan_string_region(ctx, &bytes, 0),
        );
        assert_eq!(refusal.operation, "validate kernel header product string");
    }

    #[test]
    fn partial_binary_headers_retain_linear_tolerance_without_angular() {
        for (magic, header_len) in [
            (
                b"ASM BinaryFile4".as_slice(),
                crate::layout::asmheader_binaryfile4::LEN,
            ),
            (
                b"ASM BinaryFile8".as_slice(),
                crate::layout::asmheader_binaryfile8::LEN,
            ),
            (
                b"ACIS BinaryFile".as_slice(),
                crate::layout::acisheader_binaryfile4::LEN,
            ),
        ] {
            let mut bytes = magic.to_vec();
            bytes.resize(header_len, 0);
            bytes.extend_from_slice(&[7, 0, 7, 0, 7, 0]);
            for value in [1.0_f64, 0.125] {
                bytes.push(6);
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            let parse = if magic.starts_with(b"ASM") {
                crate::asm_header::parse
            } else {
                crate::acis_header::parse
            };
            let header = parse(&cadmpeg_test_support::service_decode_context(), &bytes)
                .expect("service policy admits header")
                .expect("recognized partial header");
            assert_eq!(header.metadata.linear, Some(0.125));
            assert_eq!(header.metadata.angular, None);
            bytes.push(6);
            bytes.extend_from_slice(&0.25_f64.to_le_bytes());
            let header = parse(&cadmpeg_test_support::service_decode_context(), &bytes)
                .expect("service policy admits header")
                .expect("recognized complete header");
            assert_eq!(header.metadata.linear, Some(0.125));
            assert_eq!(header.metadata.angular, Some(0.25));
        }
    }
}
