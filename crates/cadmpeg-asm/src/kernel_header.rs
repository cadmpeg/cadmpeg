// SPDX-License-Identifier: Apache-2.0
//! Kernel header metadata shared by binary ASM, binary ACIS, and text streams.

use cadmpeg_core::decode::View;

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
    /// Absolute distance tolerance `resabs`.
    pub linear: Option<f64>,
    /// Normal tolerance `resnor`.
    pub angular: Option<f64>,
}

/// Flag bit selecting the optional construction-history partition.
pub const HISTORY_PARTITION_FLAG: u64 = 1;

/// Flag bits 1 to 7, which hold the save format's revision number.
pub const FORMAT_REVISION_FLAGS: u64 = 0xfe;

const FORMAT_REVISION_SHIFT: u32 = 1;

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

    /// Save format revision from flag bits 1 to 7.
    pub fn format_revision(&self) -> Option<u32> {
        self.flags
            .map(|flags| ((flags & FORMAT_REVISION_FLAGS) >> FORMAT_REVISION_SHIFT) as u32)
    }

    /// Flags outside the history and revision fields.
    pub fn unassigned_flags(&self) -> Option<u64> {
        self.flags
            .map(|flags| flags & !(HISTORY_PARTITION_FLAG | FORMAT_REVISION_FLAGS))
    }
}

pub(crate) fn read_string_region(bytes: &[u8], start: usize) -> (Vec<String>, Vec<f64>, usize) {
    let mut cur = start;
    let mut strings = Vec::new();
    while strings.len() < 3 {
        match read_u8_string(bytes, cur) {
            Some((value, next)) => {
                strings.push(value);
                cur = next;
            }
            None => break,
        }
    }
    let mut doubles = Vec::new();
    while doubles.len() < 3 {
        match read_tagged_f64(bytes, cur) {
            Some((value, next)) => {
                doubles.push(value);
                cur = next;
            }
            None => break,
        }
    }
    (strings, doubles, cur)
}

fn read_u8_string(bytes: &[u8], at: usize) -> Option<(String, usize)> {
    if *bytes.get(at)? != 0x07 {
        return None;
    }
    let len = *bytes.get(at + 1)? as usize;
    let start = at + 2;
    let value = std::str::from_utf8(bytes.get(start..start + len)?)
        .ok()?
        .to_string();
    Some((value, start + len))
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
            let header = parse(&bytes).expect("recognized partial header");
            assert_eq!(header.metadata.linear, Some(0.125));
            assert_eq!(header.metadata.angular, None);
            bytes.push(6);
            bytes.extend_from_slice(&0.25_f64.to_le_bytes());
            let header = parse(&bytes).expect("recognized complete header");
            assert_eq!(header.metadata.linear, Some(0.125));
            assert_eq!(header.metadata.angular, Some(0.25));
        }
    }
}
