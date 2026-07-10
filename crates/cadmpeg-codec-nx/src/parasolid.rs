// SPDX-License-Identifier: Apache-2.0
//! Extraction and classification of the embedded Parasolid neutral-binary streams.
//!
//! NX authors geometry directly with Parasolid and stores it as zlib-compressed
//! neutral-binary streams inside the SPLMSSTR container. Rather than address these
//! through the container directory (whose file/non-file entry payloads are
//! ambiguous), the streams are located by a **zlib scan**: every `78 01` position
//! whose inflate yields a substantial payload is a candidate, and the inflated
//! prologue text classifies it.
//!
//! An inflated Parasolid stream begins `PS 00 00`, followed by a text prologue
//! naming the transmit subtype (`(partition)`, `(deltas)`, or a plain cached body)
//! and a `SCH_<version>` schema token. Only partition and deltas streams carry the
//! topology/geometry records this codec reads; the JT/preview zlib blobs inflate to
//! non-`PS` payloads and are classified [`StreamKind::Preview`].

use flate2::read::ZlibDecoder;
use std::io::Read;

use crate::container;

/// Classification of an inflated zlib payload found in the container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamKind {
    /// A Parasolid `(partition)` stream: a full body snapshot.
    Partition,
    /// A Parasolid `(deltas)` stream: an incremental edit log paired with a
    /// partition.
    Deltas,
    /// A Parasolid plain stream: a cached body with no partition/deltas subtype.
    Plain,
    /// A non-Parasolid payload (JT/preview mesh, metadata) — never a source of
    /// analytic B-rep.
    Preview,
}

impl StreamKind {
    /// A short label for reporting.
    pub fn label(self) -> &'static str {
        match self {
            StreamKind::Partition => "partition",
            StreamKind::Deltas => "deltas",
            StreamKind::Plain => "plain",
            StreamKind::Preview => "preview",
        }
    }

    /// Whether this stream carries Parasolid neutral-binary geometry records.
    pub fn is_parasolid(self) -> bool {
        !matches!(self, StreamKind::Preview)
    }
}

/// One located, inflated stream.
#[derive(Debug, Clone)]
pub struct Stream {
    /// Byte offset of the `78 01` zlib header in the source file.
    pub file_offset: usize,
    /// Inflated bytes.
    pub inflated: Vec<u8>,
    /// Classification.
    pub kind: StreamKind,
    /// The `SCH_<version>` schema token, when the stream is Parasolid.
    pub schema: Option<String>,
}

/// The minimum inflated length for a candidate to count as a real stream; below
/// this a `78 01` match is almost certainly a coincidence in packed data.
const MIN_INFLATED: usize = 64;

/// Locate and inflate every embedded Parasolid stream in the canonical
/// `/Root/UG_PART/UG_PART` payload, classifying each.
///
/// The directory bounds are authoritative. Other SPLMSSTR streams, notably JT,
/// can contain independent zlib payloads and must not be interpreted as part
/// geometry even if their inflated bytes happen to begin with a Parasolid
/// prologue. A malformed or absent canonical part stream has no inline
/// Parasolid streams.
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
            if let Some(inflated) = inflate(&part[i..]) {
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
fn is_zlib_header(cmf: u8, flg: u8) -> bool {
    cmf & 0x0f == 8 && cmf >> 4 <= 7 && u16::from_be_bytes([cmf, flg]).is_multiple_of(31)
}

/// Inflate a zlib stream, tolerating trailing garbage after the compressed data
/// (the container packs streams back-to-back, so the slice runs past the end).
fn inflate(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut dec = ZlibDecoder::new(bytes);
    let mut out = Vec::new();
    match dec.read_to_end(&mut out) {
        Ok(_) => Some(out),
        // A truncated tail still yields the decoded prefix; accept it if useful.
        Err(_) if !out.is_empty() => Some(out),
        Err(_) => None,
    }
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
