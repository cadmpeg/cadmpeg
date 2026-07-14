// SPDX-License-Identifier: Apache-2.0
//! Extract and classify compressed streams in an NX part payload.
//!
//! [`extract_streams`] scans the canonical `/Root/UG_PART/UG_PART` file span for
//! valid zlib headers. An inflated `PS 00 00` prologue identifies Parasolid
//! neutral-binary data and supplies its subtype and optional `SCH_` schema token.
//! Other inflated payloads are classified as [`StreamKind::Preview`].

use cadmpeg_ir::compression::inflate_zlib_prefix;

use crate::container;

/// Classification of an inflated payload in the part stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamKind {
    /// A Parasolid `(partition)` body snapshot.
    Partition,
    /// A Parasolid `(deltas)` edit overlay.
    Deltas,
    /// A cached Parasolid body without a partition or deltas subtype.
    Plain,
    /// An inflated non-Parasolid payload, such as preview or metadata data.
    Preview,
}

impl StreamKind {
    /// Return the stable label used in summaries and reports.
    pub fn label(self) -> &'static str {
        match self {
            StreamKind::Partition => "partition",
            StreamKind::Deltas => "deltas",
            StreamKind::Plain => "plain",
            StreamKind::Preview => "preview",
        }
    }

    /// Return whether this kind contains Parasolid neutral-binary records.
    pub fn is_parasolid(self) -> bool {
        !matches!(self, StreamKind::Preview)
    }
}

/// A located and inflated stream from the canonical part payload.
#[derive(Debug, Clone)]
pub struct Stream {
    /// Byte offset of the `78 01` zlib header in the source file.
    pub file_offset: usize,
    /// Inflated bytes.
    pub inflated: Vec<u8>,
    /// Payload classification.
    pub kind: StreamKind,
    /// The Parasolid `SCH_<version>` token, when present.
    pub schema: Option<String>,
}

/// One Parasolid attribute-class declaration preceding its field record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttributeDefinition<'a> {
    /// Inflated-stream offset of the `00 4f` tag.
    pub offset: usize,
    /// Stream-local definition record identity.
    pub xmt: u16,
    /// Exact printable class name.
    pub name: &'a str,
}

/// Decode length-bounded `00 4f` attribute-class declarations.
pub fn attribute_definitions(bytes: &[u8]) -> Vec<AttributeDefinition<'_>> {
    let mut definitions = Vec::new();
    let mut at = 0;
    while at + 9 <= bytes.len() {
        if bytes.get(at..at + 2) != Some(&[0x00, 0x4f]) {
            at += 1;
            continue;
        }
        let escaped = bytes.get(at + 2) == Some(&0xff);
        let header = at + 2 + usize::from(escaped);
        let Some(length_bytes) = bytes.get(header..header + 4) else {
            break;
        };
        let name_len = u32::from_be_bytes(length_bytes.try_into().expect("four bytes")) as usize;
        let Some(identity) = bytes.get(header + 4..header + 6) else {
            break;
        };
        let name_start = header + 6;
        let Some(name_end) = name_start.checked_add(name_len) else {
            at += 1;
            continue;
        };
        let Some(name_bytes) = bytes.get(name_start..name_end) else {
            at += 1;
            continue;
        };
        if name_len == 0
            || identity[0] != 0
            || !name_bytes
                .iter()
                .all(|byte| byte.is_ascii_graphic() && !byte.is_ascii_control())
            || bytes.get(name_end..name_end + 2) != Some(&[0x00, 0x50])
        {
            at += 1;
            continue;
        }
        let Ok(name) = std::str::from_utf8(name_bytes) else {
            at += 1;
            continue;
        };
        definitions.push(AttributeDefinition {
            offset: at,
            xmt: u16::from_be_bytes([identity[0], identity[1]]),
            name,
        });
        at = name_end;
    }
    definitions
}

/// The minimum inflated length for a candidate to count as a real stream; below
/// this a `78 01` match is almost certainly a coincidence in packed data.
const MIN_INFLATED: usize = 64;

/// Locate, inflate, and classify zlib streams in `/Root/UG_PART/UG_PART`.
///
/// Returns an empty vector if `data` is not a valid SPLMSSTR image or the
/// canonical part entry is absent or invalid. The scan remains inside that
/// entry, excluding compressed payloads stored elsewhere in the container.
pub fn extract_streams(data: &[u8]) -> Vec<Stream> {
    let Ok(container) = container::scan_bytes(data.to_vec()) else {
        return Vec::new();
    };
    let Some((part_offset, part_size)) = container
        .entries
        .iter()
        .find(|entry| entry.name == "/Root/UG_PART/UG_PART")
        .and_then(|entry| entry.file_span)
    else {
        return Vec::new();
    };
    let Ok(start) = usize::try_from(part_offset) else {
        return Vec::new();
    };
    let Ok(size) = usize::try_from(part_size) else {
        return Vec::new();
    };
    let Some(part) = data.get(start..start.saturating_add(size)) else {
        return Vec::new();
    };

    let mut streams = Vec::new();
    let mut i = 0usize;
    while i + 2 <= part.len() {
        if is_zlib_header(part[i], part[i + 1]) {
            if let Some(inflated) = inflate_zlib_prefix(&part[i..]) {
                if inflated.len() >= MIN_INFLATED {
                    let (kind, schema) = classify(&inflated);
                    streams.push(Stream {
                        file_offset: start + i,
                        inflated,
                        kind,
                        schema,
                    });
                    // Resume after this header; a valid stream will not start
                    // again inside its own compressed body at the very next byte.
                    i += 2;
                    continue;
                }
            }
        }
        i += 1;
    }
    streams
}

/// A zlib header has compression method 8 and a 16-bit header divisible by
/// 31. NX uses the standard `78 01`, `78 9c`, and `78 da` variants, but the
/// predicate accepts every standards-conforming FLG byte rather than treating a
/// compression level as a format discriminator.
#[allow(clippy::manual_is_multiple_of)] // `is_multiple_of` exceeds the workspace MSRV.
fn is_zlib_header(cmf: u8, flg: u8) -> bool {
    cmf & 0x0f == 8 && cmf >> 4 <= 7 && u16::from_be_bytes([cmf, flg]).is_multiple_of(31)
}

/// Classify an inflated payload from its prologue text and read the schema token.
fn classify(inflated: &[u8]) -> (StreamKind, Option<String>) {
    // A Parasolid neutral-binary stream begins `PS 00 00`.
    if !inflated.starts_with(b"PS\x00\x00") {
        return (StreamKind::Preview, None);
    }
    // The prologue is ASCII up to `END_OF_HEADER`/the first record; scan a bounded
    // window for the transmit subtype and the schema token.
    let window = &inflated[..inflated.len().min(512)];
    let kind = if contains(window, b"(partition)") {
        StreamKind::Partition
    } else if contains(window, b"(deltas)") {
        StreamKind::Deltas
    } else {
        StreamKind::Plain
    };
    (kind, read_schema(window))
}

/// Read a `SCH_<...>` schema token: the `SCH_` prefix followed by the run of
/// token characters (alphanumeric and `_`).
fn read_schema(window: &[u8]) -> Option<String> {
    let pos = find(window, b"SCH_")?;
    let mut end = pos;
    while end < window.len() && (window[end].is_ascii_alphanumeric() || window[end] == b'_') {
        end += 1;
    }
    Some(String::from_utf8_lossy(&window[pos..end]).into_owned())
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    find(haystack, needle).is_some()
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}
