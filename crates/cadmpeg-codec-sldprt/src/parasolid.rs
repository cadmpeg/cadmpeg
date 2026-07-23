// SPDX-License-Identifier: Apache-2.0
//! Extraction and header parsing for embedded Parasolid streams.
//!
//! A stream starts with `PS\0\0`, a big-endian description length and
//! description, padding, and a length-prefixed
//! `SCH_<modeller>_<schema>_<format>` token. Outer blocks may carry direct
//! streams or zlib-compressed streams inside a transmit wrapper. Stream
//! descriptions identify partition, deltas, and feature-profile payloads.

use crate::container::parasolid_offset;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::parasolid::{has_prologue, locate_streams, Inflate};

/// The constant 16-byte prefix of the wrapped Parasolid transmit-container
/// magic. When it is present, the actual `PS\0\0` stream is a nested zlib member
/// rather than bytes at the block payload's start ([spec §3](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md#3-parasolid-stream), "wrapped"/"nested"
/// families). The four bytes that follow this prefix are a per-container
/// length/type field and are not part of the signature.
const WRAPPED_MAGIC_PREFIX: [u8; 16] = [
    0x23, 0x1d, 0xd5, 0x71, 0xda, 0x81, 0x48, 0xa2, 0xa8, 0x58, 0x98, 0xb2, 0x1b, 0x89, 0xef, 0x99,
];

/// Extract every valid direct or nested Parasolid stream in one block payload.
pub fn extract_streams(payload: &[u8]) -> Vec<Vec<u8>> {
    extract_streams_with_offsets(payload)
        .into_iter()
        .map(|(_, stream)| stream)
        .collect()
}

/// Extract every stream with its direct or wrapper offset in the outer payload.
///
/// The direct and wrapped scans are the shared
/// [`cadmpeg_ir::parasolid::locate_streams`] sniff under [`Inflate::Bounded`],
/// whose bounded prefix inflate matches this codec's `inflate_zlib_prefix`
/// tolerance. Two admission rules stay codec-side around it: a `PS\0\0` in the
/// first 64 bytes that the direct header framing rejects is still admitted whole
/// (the [`parasolid_offset`] fallback), and the wrapped scan is honored only when
/// the `SolidWorks` transmit magic is present and keeps only members whose
/// description-framed [`stream_header`] parses — the sniff admits a wrapped
/// member on the prologue alone.
pub fn extract_streams_with_offsets(payload: &[u8]) -> Vec<(usize, Vec<u8>)> {
    let located = locate_streams(payload, Inflate::Bounded);
    // `locate_streams` returns direct streams whenever any frame, otherwise the
    // wrapped scan. A direct stream leads with the prologue at its payload
    // offset; a wrapped member's offset points at its zlib header. A leading
    // prologue therefore means the whole result is the direct scan.
    if located
        .first()
        .is_some_and(|stream| payload.get(stream.offset..).is_some_and(has_prologue))
    {
        return located
            .into_iter()
            .map(|stream| (stream.offset, stream.bytes))
            .collect();
    }
    // No direct stream framed: `located` holds the wrapped scan. The codec
    // fallbacks the sniff omits take precedence over it.
    if let Some(off) = parasolid_offset(payload) {
        return vec![(off, payload[off..].to_vec())];
    }
    if !contains(payload, &WRAPPED_MAGIC_PREFIX) {
        return Vec::new();
    }
    located
        .into_iter()
        .filter(|stream| stream_header(&stream.bytes).is_some())
        .map(|stream| (stream.offset, stream.bytes))
        .collect()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.len() >= needle.len() && haystack.windows(needle.len()).any(|w| w == needle)
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
    let desc_len = usize::from(cadmpeg_ir::wire::be::u16_at(payload, desc_len_at)?);
    let desc_start = desc_len_at + 2;
    let desc_end = desc_start + desc_len;
    let description = String::from_utf8_lossy(payload.get(desc_start..desc_end)?).into_owned();

    // The padding between description and the length-prefixed schema token is not
    // fixed, so the `SCH_` marker is located directly; the preceding byte is the
    // schema length ([spec §4.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md#41-stored-edge-direction)).
    let window_end = (desc_end + 64).min(payload.len());
    let rel = payload
        .get(desc_end..window_end)?
        .windows(4)
        .position(|w| w == b"SCH_")?;
    let schema_at = desc_end + rel;
    let schema_len = *payload.get(schema_at.checked_sub(1)?)? as usize;
    let schema_end = schema_at + schema_len;
    let schema = String::from_utf8_lossy(payload.get(schema_at..schema_end)?).into_owned();

    Some(StreamHeader {
        description,
        schema,
        body_offset: schema_end,
    })
}

/// Test whether the description identifies a partition or deltas body stream.
pub fn is_body_stream(header: &StreamHeader) -> bool {
    let d = header.description.to_ascii_lowercase();
    d.contains("partition") || d.contains("deltas")
}

/// Decode the unique counted XYZ polyline carried by a mesh stream.
///
/// Mesh coordinate arrays use a big-endian scalar count followed by the
/// `0x0022` array tag and consecutive f64 values. The scalar count is three
/// times the point count.
pub(crate) fn mesh_polyline(payload: &[u8]) -> Option<Vec<Point3>> {
    let header = stream_header(payload)?;
    let schema = header.schema.to_ascii_lowercase();
    if !schema.ends_with("_13006") {
        return None;
    }
    let mut candidates = Vec::new();
    for tag_at in header.body_offset..payload.len().saturating_sub(2) {
        if payload.get(tag_at..tag_at + 2) != Some(&[0x00, 0x22]) || tag_at < 4 {
            continue;
        }
        let Some(count_bytes) = payload
            .get(tag_at - 4..tag_at)
            .and_then(|bytes| bytes.try_into().ok())
        else {
            continue;
        };
        let Ok(scalar_count) = usize::try_from(u32::from_be_bytes(count_bytes)) else {
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
                f64::from_be_bytes(xyz[0..8].try_into().expect("eight-byte chunk")),
                f64::from_be_bytes(xyz[8..16].try_into().expect("eight-byte chunk")),
                f64::from_be_bytes(xyz[16..24].try_into().expect("eight-byte chunk")),
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
