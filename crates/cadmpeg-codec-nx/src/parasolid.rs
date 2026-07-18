// SPDX-License-Identifier: Apache-2.0
//! Extract and classify compressed streams in an NX part payload.
//!
//! [`extract_streams`] scans the canonical `/Root/UG_PART/UG_PART` file span for
//! valid zlib headers. An inflated `PS 00 00` prologue identifies Parasolid
//! neutral-binary data and supplies its subtype and optional `SCH_` schema token.
//! Other inflated payloads are classified as [`StreamKind::Preview`].
#![deny(clippy::disallowed_methods)]

use std::io::Read;

use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::decode::{ByteRange, DecodeContext, ExpandSpec, View};
use flate2::read::ZlibDecoder;

use crate::container::Container;

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
    /// Compressed input bytes the decoder consumed at `file_offset`.
    ///
    /// The physical extent `[file_offset, file_offset + consumed)` in the source.
    pub consumed: u64,
    /// Inflated bytes.
    pub inflated: Vec<u8>,
    /// Payload classification.
    pub kind: StreamKind,
    /// The Parasolid `SCH_<version>` token, when present.
    pub schema: Option<String>,
}

/// The minimum inflated length for a candidate to count as a real stream; below
/// this a `78 01` match is almost certainly a coincidence in packed data.
///
/// This threshold rejects coincidental zlib-header matches in packed data; it
/// is not a resource bound.
/// The per-expand and cumulative decompressed-bytes ceilings enforced by
/// [`DecodeContext::begin_expand`] are what bound inflation against a
/// decompression bomb.
const MIN_INFLATED: usize = 64;

/// Chunk size for streaming inflated output through the expander.
const INFLATE_CHUNK: usize = 8192;

/// Locate, inflate, and classify zlib streams in `/Root/UG_PART/UG_PART`.
///
/// Registers the canonical part payload as a [`SpaceOrigin::Slice`] span in the
/// runtime space graph, then inflates each embedded zlib stream through
/// [`DecodeContext::begin_expand`] so every decompressed byte is charged and
/// bounded by the per-expand and cumulative ceilings, and each stream registers
/// as a decompression `Transform` space only on successful finalize. Returns an
/// empty vector when the canonical part entry is absent, so
/// an assembly `.prt` with no inline geometry decodes without error.
///
/// [`SpaceOrigin::Slice`]: cadmpeg_ir::decode::SpaceOrigin::Slice
pub fn extract_streams<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
    container: &Container,
) -> Result<Vec<Stream>, CodecError> {
    let Some((part_offset, part_size)) = container
        .entries
        .iter()
        .find(|entry| entry.name == "/Root/UG_PART/UG_PART")
        .and_then(|entry| entry.file_span)
    else {
        return Ok(Vec::new());
    };
    let (Ok(start), Ok(size)) = (usize::try_from(part_offset), usize::try_from(part_size)) else {
        return Ok(Vec::new());
    };
    let Some(end) = start.checked_add(size) else {
        return Ok(Vec::new());
    };
    let (_part_space, part_view) = ctx.register_slice(
        root,
        ByteRange {
            start: start as u64,
            end: end as u64,
        },
    )?;
    let part = part_view.window();
    ctx.charge_work(
        part.len() as u64,
        "nx_parasolid_scan",
        Some(part_view.location()),
    )?;

    let mut streams = Vec::new();
    let mut i = 0usize;
    while i + 2 <= part.len() {
        if is_zlib_header(part[i], part[i + 1]) {
            if let Some((inflated, consumed)) = inflate_stream(ctx, part_view, i)? {
                let (kind, schema) = classify(&inflated);
                streams.push(Stream {
                    file_offset: start + i,
                    consumed,
                    inflated,
                    kind,
                    schema,
                });
                // Resume past the bytes this member consumed, not at the next
                // byte: a spurious `78 xx` zlib header inside the compressed
                // body would otherwise inflate into a second stream whose source
                // extent [file_offset, file_offset+consumed) overlaps this
                // member's, double-attributing the same compressed bytes to two
                // decompression origins. Skipping the consumed run keeps packed
                // members' input extents disjoint.
                i = i.saturating_add((consumed as usize).max(2));
                continue;
            }
        }
        i += 1;
    }
    Ok(streams)
}

/// Inflate the zlib member at `offset` within the part payload through
/// [`DecodeContext::begin_expand`], returning the inflated bytes when they clear
/// [`MIN_INFLATED`].
///
/// The compressed source is framed as a nested [`View`] of the part span —
/// never a raw length — so the expander bounds and attributes the stream to its
/// own extent. Output streams through the writer in chunks, so no unbounded
/// intermediate buffer forms; the decoder is truncation-tolerant, accepting a
/// decoded prefix when trailing input belongs to the next packed stream. A
/// candidate that inflates below the threshold registers no space: the writer
/// is dropped without finalizing.
fn inflate_stream<'a>(
    ctx: &DecodeContext<'a>,
    part_view: View<'a>,
    offset: usize,
) -> Result<Option<(Vec<u8>, u64)>, CodecError> {
    let Some(source) = part_view.child(offset, part_view.end()) else {
        return Ok(None);
    };
    let mut decoder = ZlibDecoder::new(source.window());
    let mut writer = ctx.begin_expand(source, ExpandSpec::Unknown)?;
    let mut inflated = Vec::new();
    let mut chunk = [0u8; INFLATE_CHUNK];
    loop {
        match decoder.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => {
                writer.write(&chunk[..read])?;
                inflated.extend_from_slice(&chunk[..read]);
            }
            Err(_) => break,
        }
    }
    if inflated.len() < MIN_INFLATED {
        return Ok(None);
    }
    let consumed = decoder.total_in();
    writer.set_consumed(consumed);
    writer.finalize()?;
    Ok(Some((inflated, consumed)))
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
    if !inflated.starts_with(b"PS\x00\x00") {
        return (StreamKind::Preview, None);
    }
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
