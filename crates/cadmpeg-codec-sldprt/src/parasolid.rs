// SPDX-License-Identifier: Apache-2.0
//! Extraction and header parsing for embedded Parasolid streams.
//!
//! A stream starts with `PS\0\0`, a big-endian description length and
//! description, padding, and a length-prefixed
//! `SCH_<modeller>_<schema>_<format>` token. Outer blocks may carry direct
//! streams or zlib-compressed streams inside a transmit wrapper. Stream
//! descriptions identify partition, deltas, and feature-profile payloads.

use std::io::Read;

use crate::container::parasolid_offset;

/// The constant 16-byte prefix of the wrapped Parasolid transmit-container
/// magic. When it is present, the actual `PS\0\0` stream is a nested zlib member
/// rather than bytes at the block payload's start ([spec §3](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md#3-parasolid-stream), "wrapped"/"nested"
/// families). The four bytes that follow this prefix are a per-container
/// length/type field and are not part of the signature.
const WRAPPED_MAGIC_PREFIX: [u8; 16] = [
    0x23, 0x1d, 0xd5, 0x71, 0xda, 0x81, 0x48, 0xa2, 0xa8, 0x58, 0x98, 0xb2, 0x1b, 0x89, 0xef, 0x99,
];

/// Extract the first valid Parasolid stream from a decompressed block payload.
///
/// Direct `PS\0\0` streams and zlib members in recognized transmit wrappers are
/// accepted. The returned buffer owns the extracted stream bytes.
pub fn extract_stream(payload: &[u8]) -> Option<Vec<u8>> {
    extract_streams(payload).into_iter().next()
}

/// Extract every valid direct or nested Parasolid stream in one block payload.
pub fn extract_streams(payload: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let signatures: Vec<_> = payload
        .windows(4)
        .enumerate()
        .filter_map(|(at, bytes)| (bytes == b"PS\0\0").then_some(at))
        .collect();
    for (index, start) in signatures.iter().copied().enumerate() {
        let end = signatures.get(index + 1).copied().unwrap_or(payload.len());
        let candidate = payload[start..end].to_vec();
        if stream_header(&candidate).is_some() {
            out.push(candidate);
        }
    }
    if !out.is_empty() {
        return out;
    }
    if let Some(off) = parasolid_offset(payload) {
        return vec![payload[off..].to_vec()];
    }
    if !contains(payload, &WRAPPED_MAGIC_PREFIX) {
        return out;
    }
    // Try each zlib member; the first that inflates to a `PS\0\0`-leading stream
    // is the embedded body. zlib headers are `78 01` / `78 9c` / `78 da`.
    let mut i = 0usize;
    while i + 2 <= payload.len() {
        if payload[i] == 0x78 && matches!(payload[i + 1], 0x01 | 0x9c | 0xda) {
            if let Some(inner) = zlib_inflate(&payload[i..]) {
                if inner.starts_with(&[b'P', b'S', 0x00, 0x00])
                    && stream_header(&inner).is_some()
                    && !out.contains(&inner)
                {
                    out.push(inner);
                }
            }
        }
        i += 1;
    }
    out
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.len() >= needle.len() && haystack.windows(needle.len()).any(|w| w == needle)
}

/// zlib-inflate (with the 2-byte header) from the start of `data`; `None` on any
/// error. A trailing-garbage error still yields the bytes decoded so far.
fn zlib_inflate(data: &[u8]) -> Option<Vec<u8>> {
    use flate2::read::ZlibDecoder;
    let mut out = Vec::new();
    let mut dec = ZlibDecoder::new(data);
    match dec.read_to_end(&mut out) {
        Ok(_) => Some(out),
        Err(_) if !out.is_empty() => Some(out),
        Err(_) => None,
    }
}

/// Parsed framing fields for one Parasolid stream.
#[derive(Debug, Clone)]
pub struct StreamHeader {
    /// Byte offset of the `PS\0\0` signature in the supplied buffer.
    pub signature_offset: usize,
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
    let desc_len =
        u16::from_be_bytes([*payload.get(desc_len_at)?, *payload.get(desc_len_at + 1)?]) as usize;
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
        signature_offset: sig,
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
