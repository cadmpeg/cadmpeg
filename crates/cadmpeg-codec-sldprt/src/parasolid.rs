// SPDX-License-Identifier: Apache-2.0
//! Extraction and header parsing for embedded Parasolid streams.
//!
//! A stream starts with `PS\0\0`, a big-endian description length and
//! description, padding, and a length-prefixed
//! `SCH_<modeller>_<schema>_<format>` token. Outer blocks may carry direct
//! streams or zlib-compressed streams inside a transmit wrapper. Stream
//! descriptions identify partition, deltas, and feature-profile payloads.

use cadmpeg_container::compression::inflate_zlib_probe;
use cadmpeg_core::bytes::contains;
use cadmpeg_core::decode::View;
use cadmpeg_ir::math::Point3;
use flate2::{Decompress, FlushDecompress, Status};

use crate::layout::{
    parasolid_chain_frame_header as chain_frame_hdr,
    parasolid_chain_section_header as chain_section_hdr, zlb_wrapper_header as zlb_hdr,
};

/// The constant 16-byte prefix of the wrapped Parasolid transmit-container
/// magic. When it is present, the actual `PS\0\0` stream is a nested zlib member
/// rather than bytes at the block payload's start ([spec §3](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md#3-parasolid-stream), "wrapped"/"nested"
/// families). Native block sections place a chain length immediately before
/// this magic; compound one-frame wrappers place the frame header immediately
/// after it.
const WRAPPED_MAGIC_PREFIX: [u8; 16] = zlb_hdr::MAGIC_VALUE;
const WRAPPED_FRAME_HEADER_LEN: usize = chain_frame_hdr::LEN;
const MAX_WRAPPED_FRAME_UNCOMPRESSED: usize = 512 * 1024 * 1024;

/// One extracted stream with the location and header proven during extraction.
#[derive(Debug, Clone)]
pub(crate) struct ExtractedStream {
    /// Direct-stream or wrapper offset in the outer payload.
    pub(crate) offset: usize,
    /// Complete extracted Parasolid stream bytes.
    pub(crate) payload: Vec<u8>,
    /// Header parsed from `payload` at extraction time.
    pub(crate) header: StreamHeader,
}

/// Extract every stream with its direct or wrapper offset in the outer payload.
pub(crate) fn extract_streams_with_offsets(payload: &[u8]) -> Vec<ExtractedStream> {
    let mut out = Vec::new();
    let wrapped_prefix = has_wrapped_prefix(payload);
    let starts = if wrapped_prefix {
        Vec::new()
    } else {
        direct_stream_headers(payload)
    };
    for (index, (start, header)) in starts.iter().enumerate() {
        let end = starts
            .get(index + 1)
            .map_or(payload.len(), |(offset, _)| *offset);
        out.push(ExtractedStream {
            offset: *start,
            payload: payload[*start..end].to_vec(),
            header: header.clone(),
        });
    }
    if !out.is_empty() {
        return out;
    }
    if !contains(payload, &WRAPPED_MAGIC_PREFIX) {
        return out;
    }

    let magic_starts = payload
        .windows(WRAPPED_MAGIC_PREFIX.len())
        .enumerate()
        .filter_map(|(offset, bytes)| (bytes == WRAPPED_MAGIC_PREFIX).then_some(offset))
        .collect::<Vec<_>>();
    for magic_at in magic_starts.iter().copied() {
        let stream = if magic_at == 0 {
            single_wrapped_stream(payload, magic_at)
        } else {
            chained_wrapped_stream(payload, magic_at)
        };
        if let Some(stream) = stream {
            if !out
                .iter()
                .any(|existing| existing.payload == stream.payload)
            {
                out.push(stream);
            }
        }
    }
    if !out.is_empty() {
        out.sort_by_key(|stream| stream.offset);
        return out;
    }

    // A payload with the section prefix is a malformed chained wrapper, not a
    // reason to retain its first frame as a complete stream. This prevents a
    // bad continuation from silently recreating the historical one-megabyte
    // truncation.
    if wrapped_prefix {
        return out;
    }

    // Preserve older nested wrappers that do not carry the chained-section
    // prefix. Try each zlib member; the first that inflates to a `PS\0\0`-leading
    // stream is the embedded body. zlib headers are `78 01` / `78 9c` / `78 da`.
    let mut i = 0usize;
    while i + 2 <= payload.len() {
        if payload[i] == 0x78 && matches!(payload[i + 1], 0x01 | 0x9c | 0xda) {
            if let Some(inner) = inflate_zlib_candidate(&payload[i..]) {
                if let Some(stream) = extracted_stream(i, inner) {
                    if !out
                        .iter()
                        .any(|existing| existing.payload == stream.payload)
                    {
                        out.push(stream);
                    }
                }
            }
        }
        i += 1;
    }
    out
}

fn has_wrapped_prefix(payload: &[u8]) -> bool {
    payload.starts_with(&WRAPPED_MAGIC_PREFIX)
        || payload
            .get(chain_section_hdr::MAGIC..chain_section_hdr::MAGIC + WRAPPED_MAGIC_PREFIX.len())
            == Some(&WRAPPED_MAGIC_PREFIX)
}

fn single_wrapped_stream(payload: &[u8], magic_at: usize) -> Option<ExtractedStream> {
    let frame_at = magic_at.checked_add(WRAPPED_MAGIC_PREFIX.len())?;
    let uncompressed_size = View::u32_le_at(
        payload,
        frame_at.checked_add(chain_frame_hdr::UNCOMPRESSED_SIZE)?,
    )
    .and_then(as_usize)?;
    let member_size = View::u32_le_at(
        payload,
        frame_at.checked_add(chain_frame_hdr::ZLIB_MEMBER_SIZE)?,
    )
    .and_then(as_usize)?;
    let member_start = frame_at.checked_add(WRAPPED_FRAME_HEADER_LEN)?;
    let member_end = member_start.checked_add(member_size)?;
    let member = payload.get(member_start..member_end)?;
    extracted_stream(magic_at, inflate_zlib_frame(member, uncompressed_size)?)
}

fn chained_wrapped_stream(payload: &[u8], magic_at: usize) -> Option<ExtractedStream> {
    let chain_len_at = magic_at.checked_sub(chain_section_hdr::MAGIC)?;
    let chain_len = View::u32_le_at(payload, chain_len_at).and_then(as_usize)?;
    if chain_len < WRAPPED_MAGIC_PREFIX.len() + WRAPPED_FRAME_HEADER_LEN {
        return None;
    }
    let section_end = magic_at.checked_add(chain_len)?;
    if section_end > payload.len() {
        return None;
    }

    let mut frame_at = magic_at.checked_add(WRAPPED_MAGIC_PREFIX.len())?;
    let mut frames = 0usize;
    let mut stream = Vec::new();
    while frame_at < section_end {
        let remaining = payload.get(frame_at..section_end)?;
        if remaining.iter().all(|byte| *byte == 0) {
            break;
        }
        if remaining.len() < WRAPPED_FRAME_HEADER_LEN {
            return None;
        }
        let uncompressed_size = View::u32_le_at(
            payload,
            frame_at.checked_add(chain_frame_hdr::UNCOMPRESSED_SIZE)?,
        )
        .and_then(as_usize)?;
        let member_size = View::u32_le_at(
            payload,
            frame_at.checked_add(chain_frame_hdr::ZLIB_MEMBER_SIZE)?,
        )
        .and_then(as_usize)?;
        if uncompressed_size == 0 || member_size == 0 {
            return None;
        }
        let member_start = frame_at.checked_add(WRAPPED_FRAME_HEADER_LEN)?;
        let member_end = member_start.checked_add(member_size)?;
        if member_end > section_end {
            return None;
        }
        let member = payload.get(member_start..member_end)?;
        let frame = inflate_zlib_frame(member, uncompressed_size)?;
        stream.try_reserve(frame.len()).ok()?;
        stream.extend_from_slice(&frame);
        frames = frames.checked_add(1)?;
        frame_at = member_end;
    }
    if frames == 0 {
        return None;
    }
    extracted_stream(chain_len_at, stream)
}

fn extracted_stream(offset: usize, payload: Vec<u8>) -> Option<ExtractedStream> {
    if !payload.starts_with(b"PS\0\0") {
        return None;
    }
    let header = stream_header(&payload)?;
    Some(ExtractedStream {
        offset,
        payload,
        header,
    })
}

fn as_usize(value: u32) -> Option<usize> {
    usize::try_from(value).ok()
}

/// Inflate one declared zlib member and require both declared extents to be
/// exact. The caller has already bounded the member slice; this additionally
/// rejects a member-size field that includes trailing bytes.
fn inflate_zlib_frame(member: &[u8], expected: usize) -> Option<Vec<u8>> {
    if expected == 0 || expected > MAX_WRAPPED_FRAME_UNCOMPRESSED {
        return None;
    }
    let mut decoder = Decompress::new(true);
    let mut output = Vec::new();
    let mut input_at = 0usize;
    let mut chunk = [0_u8; 8192];
    loop {
        let before_input = decoder.total_in();
        let before_output = decoder.total_out();
        let status = decoder
            .decompress(member.get(input_at..)?, &mut chunk, FlushDecompress::None)
            .ok()?;
        let consumed = usize::try_from(decoder.total_in() - before_input).ok()?;
        let produced = usize::try_from(decoder.total_out() - before_output).ok()?;
        input_at = input_at.checked_add(consumed)?;
        if input_at > member.len() || produced > expected.saturating_sub(output.len()) {
            return None;
        }
        output.try_reserve(produced).ok()?;
        output.extend_from_slice(&chunk[..produced]);
        if status == Status::StreamEnd {
            return (input_at == member.len() && output.len() == expected).then_some(output);
        }
        if consumed == 0 && produced == 0 {
            return None;
        }
    }
}

fn inflate_zlib_candidate(bytes: &[u8]) -> Option<Vec<u8>> {
    let cap = (16 * 1024 * 1024_usize)
        .saturating_add(bytes.len().saturating_mul(256))
        .min(2 * 1024 * 1024 * 1024);
    inflate_zlib_probe(bytes, cap)
}

fn direct_stream_headers(payload: &[u8]) -> Vec<(usize, StreamHeader)> {
    payload
        .windows(4)
        .enumerate()
        .filter_map(|(at, bytes)| (bytes == b"PS\0\0").then_some(at))
        .filter_map(|start| stream_header(&payload[start..]).map(|header| (start, header)))
        .collect()
}

/// Parsed framing fields for one Parasolid stream.
#[derive(Debug, Clone)]
pub struct StreamHeader {
    /// Human-readable stream description.
    pub description: String,
    /// `SCH_<modeller>_<schema>_<format>` schema token.
    pub schema: String,
    /// Byte offset where the class-definition record body begins.
    pub body_offset: usize,
}

/// Parse a Parasolid header from a buffer containing a leading-window signature.
///
/// Returns `None` when the signature, description, or schema token is missing or
/// truncated.
pub fn stream_header(payload: &[u8]) -> Option<StreamHeader> {
    let sig = parasolid_offset(payload)?;
    let desc_len_at = sig + 4;
    let mut view = View::over_retained(payload);
    view.seek(desc_len_at)?;
    let desc_len = usize::from(view.u16_be()?);
    let desc_start = desc_len_at + 2;
    let desc_end = desc_start + desc_len;
    let description = String::from_utf8_lossy(payload.get(desc_start..desc_end)?).into_owned();

    // The padding between description and the length-prefixed schema token is not
    // fixed, so the `SCH_` marker is located directly; the preceding byte is the
    // schema length ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md#31-stream-header)).
    let window_end = (desc_end + 64).min(payload.len());
    let token = cadmpeg_parasolid::find_u8_length_prefixed_schema_token(
        payload.get(desc_end..window_end)?,
    )?;
    let schema_end = desc_end + token.end();
    let schema = token.value().to_owned();

    Some(StreamHeader {
        description,
        schema,
        body_offset: schema_end,
    })
}

fn parasolid_offset(payload: &[u8]) -> Option<usize> {
    const SIGNATURE: &[u8] = b"PS\0\0";
    let window = payload.len().min(64);
    cadmpeg_core::bytes::find(&payload[..window], SIGNATURE)
}

/// Test whether the description identifies a partition or deltas body stream.
pub fn is_body_stream(header: &StreamHeader) -> bool {
    let d = header.description.to_ascii_lowercase();
    d.contains("partition") || d.contains("deltas")
}

/// Decode the unique counted XYZ polyline carried by a classified mesh stream.
///
/// Mesh coordinate arrays use a big-endian scalar count followed by the
/// `0x0022` array tag and consecutive f64 values. The scalar count is three
/// times the point count.
pub(crate) fn mesh_polyline_from_header(
    payload: &[u8],
    header: &StreamHeader,
) -> Option<Vec<Point3>> {
    let schema = header.schema.to_ascii_lowercase();
    if !schema.ends_with("_13006") {
        return None;
    }
    let mut candidates = Vec::new();
    for tag_at in header.body_offset..payload.len().saturating_sub(2) {
        if payload.get(tag_at..tag_at + 2) != Some(&[0x00, 0x22]) || tag_at < 4 {
            continue;
        }
        let Some(scalar_count) =
            View::u32_be_at(payload, tag_at - 4).and_then(|count| usize::try_from(count).ok())
        else {
            continue;
        };
        if scalar_count < 6 || scalar_count % 3 != 0 {
            continue;
        }
        let Some(byte_count) = scalar_count.checked_mul(8) else {
            continue;
        };
        let Some(values) = payload.get(tag_at + 2..tag_at + 2 + byte_count) else {
            continue;
        };
        let mut points = Vec::with_capacity(scalar_count / 3);
        for xyz in values.chunks_exact(24) {
            let point = Point3::new(
                View::f64_be_at(xyz, 0)?,
                View::f64_be_at(xyz, 8)?,
                View::f64_be_at(xyz, 16)?,
            );
            if ![point.x, point.y, point.z].into_iter().all(f64::is_finite) {
                points.clear();
                break;
            }
            points.push(point);
        }
        if points.len() >= 2 {
            candidates.push((scalar_count, points));
        }
    }
    candidates.sort_by_key(|(scalar_count, _)| std::cmp::Reverse(*scalar_count));
    let (largest_count, points) = candidates.first()?;
    if candidates
        .get(1)
        .is_some_and(|(count, _)| count == largest_count)
    {
        return None;
    }
    Some(points.clone())
}

#[cfg(test)]
mod tests;
