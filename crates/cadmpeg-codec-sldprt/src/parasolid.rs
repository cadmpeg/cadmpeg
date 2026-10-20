// SPDX-License-Identifier: Apache-2.0
//! Extraction and header parsing for embedded Parasolid streams.
//!
//! A stream starts with `PS\0\0`, a big-endian description length and
//! description, padding, and a length-prefixed
//! `SCH_<modeller>_<schema>_<format>` token. Outer blocks may carry direct
//! streams or zlib-compressed streams inside a transmit wrapper. Stream
//! descriptions identify partition, deltas, and feature-profile payloads.

use cadmpeg_container::compression::inflate_zlib_member;
use cadmpeg_core::decode::{DecodeContext, ExpandSpec, View, WorkBudget};
use cadmpeg_core::CodecError;
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

/// Work allowed to the legacy zlib probe per byte of the outer payload. Each
/// zlib candidate charges the payload bytes after it.
pub(crate) const PROBE_WORK_PER_PAYLOAD_BYTE: usize = 16;

/// Streams extracted from one outer payload.
#[derive(Debug)]
pub(crate) struct ExtractedStreams {
    /// Streams in payload order.
    pub(crate) streams: Vec<ExtractedStream>,
    /// The legacy zlib probe reached its work limit before trying every
    /// candidate, so a later embedded body may be missing.
    pub(crate) probe_capped: bool,
}

impl ExtractedStreams {
    fn complete(streams: Vec<ExtractedStream>) -> Self {
        Self {
            streams,
            probe_capped: false,
        }
    }
}

/// Extract every stream with its direct or wrapper offset in the outer payload.
pub(crate) fn extract_streams_with_offsets(
    payload: &[u8],
    ctx: &DecodeContext<'_>,
) -> Result<ExtractedStreams, CodecError> {
    let mut out = Vec::new();
    let wrapped_prefix = has_wrapped_prefix(payload);
    let starts = if wrapped_prefix {
        Vec::new()
    } else {
        direct_stream_headers(payload)?
    };
    let mut starts = starts.into_iter().peekable();
    while let Some((start, header)) = starts.next() {
        let end = starts.peek().map_or(payload.len(), |(offset, _)| *offset);
        let payload = retained_copy(ctx, &payload[start..end], "retain direct Parasolid stream")?;
        out.push(ExtractedStream {
            offset: start,
            payload,
            header,
        });
    }
    if !out.is_empty() {
        return Ok(ExtractedStreams::complete(out));
    }
    let Some(scan_len) = payload.len().checked_sub(WRAPPED_MAGIC_PREFIX.len() - 1) else {
        return Ok(ExtractedStreams::complete(out));
    };
    let mut has_magic = false;
    for (magic_at, byte) in payload[..scan_len].iter().enumerate() {
        if *byte != WRAPPED_MAGIC_PREFIX[0] {
            continue;
        }
        if payload.get(magic_at..magic_at + WRAPPED_MAGIC_PREFIX.len())
            != Some(&WRAPPED_MAGIC_PREFIX)
        {
            continue;
        }
        has_magic = true;
        let stream = if magic_at == 0 {
            single_wrapped_stream(payload, magic_at, ctx)?
        } else {
            chained_wrapped_stream(payload, magic_at, ctx)?
        };
        if let Some(stream) = stream {
            if !stream_payload_present(&out, &stream.payload) {
                out.push(stream);
            }
        }
    }
    if !has_magic {
        return Ok(ExtractedStreams::complete(out));
    }
    if !out.is_empty() {
        out.sort_by_key(|stream| stream.offset);
        return Ok(ExtractedStreams::complete(out));
    }

    // A payload with the section prefix is a malformed chained wrapper, not a
    // reason to retain its first frame as a complete stream. This prevents a
    // bad continuation from silently recreating the historical one-megabyte
    // truncation.
    if wrapped_prefix {
        return Ok(ExtractedStreams::complete(out));
    }

    // Preserve older nested wrappers that do not carry the chained-section
    // prefix. Try each zlib member; the first that inflates to a `PS\0\0`-leading
    // stream is the embedded body. zlib headers are `78 01` / `78 9c` / `78 da`.
    // Each candidate charges the payload bytes after it; the probe stops at
    // `PROBE_WORK_PER_PAYLOAD_BYTE` times the payload length and reports the
    // stop. A payload too large to state that limit is reported unprobed.
    let Some(probe_limit) = payload.len().checked_mul(PROBE_WORK_PER_PAYLOAD_BYTE) else {
        return Ok(ExtractedStreams {
            streams: out,
            probe_capped: true,
        });
    };
    let work = WorkBudget::new(probe_limit);
    let mut probe_capped = false;
    let mut i = 0usize;
    while i + 2 <= payload.len() {
        if payload[i] == 0x78 && matches!(payload[i + 1], 0x01 | 0x9c | 0xda) {
            if !work.charge_by(payload.len() - i) {
                probe_capped = true;
                break;
            }
            let inner = match inflate_zlib_member(
                ctx,
                View::over_retained(&payload[i..]),
                ExpandSpec::Unknown,
            ) {
                Ok((view, _)) if view.window().starts_with(b"PS\0\0") => {
                    if let Some(header) = stream_header(view.window())? {
                        Some(ExtractedStream {
                            offset: i,
                            payload: retained_copy(
                                ctx,
                                view.window(),
                                "retain Parasolid zlib candidate",
                            )?,
                            header,
                        })
                    } else {
                        None
                    }
                }
                Ok(_) => None,
                Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                Err(_) => None,
            };
            if let Some(stream) = inner {
                if !stream_payload_present(&out, &stream.payload) {
                    out.push(stream);
                }
            }
        }
        i += 1;
    }
    Ok(ExtractedStreams {
        streams: out,
        probe_capped,
    })
}

fn stream_payload_present(streams: &[ExtractedStream], payload: &[u8]) -> bool {
    streams.iter().any(|known| known.payload == payload)
}

/// Copy stream bytes that the decoded output keeps.
fn retained_copy(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    operation: &'static str,
) -> Result<Vec<u8>, CodecError> {
    ctx.retain_bytes(cadmpeg_core::decode::u64_from_index(bytes.len()), operation)?;
    Ok(bytes.to_vec())
}

fn has_wrapped_prefix(payload: &[u8]) -> bool {
    payload.starts_with(&WRAPPED_MAGIC_PREFIX)
        || payload
            .get(chain_section_hdr::MAGIC..chain_section_hdr::MAGIC + WRAPPED_MAGIC_PREFIX.len())
            == Some(&WRAPPED_MAGIC_PREFIX)
}

fn single_wrapped_stream(
    payload: &[u8],
    magic_at: usize,
    ctx: &DecodeContext<'_>,
) -> Result<Option<ExtractedStream>, CodecError> {
    let frame = (|| {
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
        Some((payload.get(member_start..member_end)?, uncompressed_size))
    })();
    let Some((member, uncompressed_size)) = frame else {
        return Ok(None);
    };
    let inflated = inflate_zlib_frame(ctx, member, uncompressed_size)?;
    inflated.map_or(Ok(None), |bytes| extracted_stream(magic_at, bytes))
}

fn chained_wrapped_stream(
    payload: &[u8],
    magic_at: usize,
    ctx: &DecodeContext<'_>,
) -> Result<Option<ExtractedStream>, CodecError> {
    let Some(chain_len_at) = magic_at.checked_sub(chain_section_hdr::MAGIC) else {
        return Ok(None);
    };
    let Some(chain_len) = View::u32_le_at(payload, chain_len_at).and_then(as_usize) else {
        return Ok(None);
    };
    if chain_len < WRAPPED_MAGIC_PREFIX.len() + WRAPPED_FRAME_HEADER_LEN {
        return Ok(None);
    }
    let Some(section_end) = magic_at.checked_add(chain_len) else {
        return Ok(None);
    };
    if section_end > payload.len() {
        return Ok(None);
    }

    let Some(mut frame_at) = magic_at.checked_add(WRAPPED_MAGIC_PREFIX.len()) else {
        return Ok(None);
    };
    let mut frames = 0usize;
    let mut frame_outputs = Vec::new();
    while frame_at < section_end {
        let Some(remaining) = payload.get(frame_at..section_end) else {
            return Ok(None);
        };
        if remaining.iter().all(|byte| *byte == 0) {
            break;
        }
        if remaining.len() < WRAPPED_FRAME_HEADER_LEN {
            return Ok(None);
        }
        let Some(uncompressed_size) = frame_at
            .checked_add(chain_frame_hdr::UNCOMPRESSED_SIZE)
            .and_then(|at| View::u32_le_at(payload, at))
            .and_then(as_usize)
        else {
            return Ok(None);
        };
        let Some(member_size) = frame_at
            .checked_add(chain_frame_hdr::ZLIB_MEMBER_SIZE)
            .and_then(|at| View::u32_le_at(payload, at))
            .and_then(as_usize)
        else {
            return Ok(None);
        };
        if uncompressed_size == 0 || member_size == 0 {
            return Ok(None);
        }
        let Some(member_start) = frame_at.checked_add(WRAPPED_FRAME_HEADER_LEN) else {
            return Ok(None);
        };
        let Some(member_end) = member_start.checked_add(member_size) else {
            return Ok(None);
        };
        if member_end > section_end {
            return Ok(None);
        }
        let Some(member) = payload.get(member_start..member_end) else {
            return Ok(None);
        };
        let Some(frame) = inflate_zlib_frame(ctx, member, uncompressed_size)? else {
            return Ok(None);
        };
        frame_outputs.push(frame);
        let Some(next_frames) = frames.checked_add(1) else {
            return Ok(None);
        };
        frames = next_frames;
        frame_at = member_end;
    }
    if frames == 0 {
        return Ok(None);
    }
    let stream = if frame_outputs.len() == 1 {
        frame_outputs.remove(0)
    } else {
        ctx.concat_retained(&frame_outputs, "retain concatenated Parasolid stream")?
    };
    extracted_stream(chain_len_at, stream)
}

fn extracted_stream(
    offset: usize,
    payload: Vec<u8>,
) -> Result<Option<ExtractedStream>, CodecError> {
    if !payload.starts_with(b"PS\0\0") {
        return Ok(None);
    }
    let Some(header) = stream_header(&payload)? else {
        return Ok(None);
    };
    Ok(Some(ExtractedStream {
        offset,
        payload,
        header,
    }))
}

fn as_usize(value: u32) -> Option<usize> {
    usize::try_from(value).ok()
}

fn inflate_zlib_frame(
    ctx: &DecodeContext<'_>,
    member: &[u8],
    expected: usize,
) -> Result<Option<Vec<u8>>, CodecError> {
    if expected == 0 {
        return Ok(None);
    }
    let declared = u64::try_from(expected)
        .map_err(|_| CodecError::NotImplemented("Parasolid frame length exceeds u64".into()))?;
    // A frame that declares more than the frame ceiling is not extracted; the
    // block keeps its other stream candidates.
    if expected > MAX_WRAPPED_FRAME_UNCOMPRESSED {
        return Ok(None);
    }
    let mut output = ctx.begin_expand(ExpandSpec::Exact(declared))?;
    let mut decoder = Decompress::new(true);
    let mut input_at = 0usize;
    let mut chunk = [0_u8; 8192];
    loop {
        let before_input = decoder.total_in();
        let before_output = decoder.total_out();
        let Some(input) = member.get(input_at..) else {
            return Ok(None);
        };
        let Ok(status) = decoder.decompress(input, &mut chunk, FlushDecompress::None) else {
            return Ok(None);
        };
        let Some(consumed) = usize::try_from(decoder.total_in() - before_input).ok() else {
            return Ok(None);
        };
        let Some(produced) = usize::try_from(decoder.total_out() - before_output).ok() else {
            return Ok(None);
        };
        let Some(next_input_at) = input_at.checked_add(consumed) else {
            return Ok(None);
        };
        input_at = next_input_at;
        let Some(remaining) = u64::try_from(expected)
            .ok()
            .and_then(|expected| expected.checked_sub(output.written()))
        else {
            return Ok(None);
        };
        let produced_u64 = u64::try_from(produced)
            .map_err(|_| CodecError::NotImplemented("Parasolid frame output exceeds u64".into()))?;
        if input_at > member.len() || produced_u64 > remaining {
            return Ok(None);
        }
        output.write(&chunk[..produced])?;
        if status == Status::StreamEnd {
            if input_at != member.len() || output.written() != declared {
                return Ok(None);
            }
            return output.finalize_owned().map(Some);
        }
        if consumed == 0 && produced == 0 {
            return Ok(None);
        }
    }
}

fn direct_stream_headers(payload: &[u8]) -> Result<Vec<(usize, StreamHeader)>, CodecError> {
    let mut headers = Vec::new();
    for (start, bytes) in payload.windows(4).enumerate() {
        if bytes != b"PS\0\0" {
            continue;
        }
        if let Some(header) = stream_header(&payload[start..])? {
            headers.push((start, header));
        }
    }
    Ok(headers)
}

/// Parsed framing fields for one Parasolid stream.
#[derive(Debug, Clone)]
pub(crate) struct StreamHeader {
    /// Human-readable stream description.
    pub(crate) description: String,
    /// `SCH_<modeller>_<schema>_<format>` schema token.
    pub(crate) schema: cadmpeg_parasolid::OwnedSchemaToken,
    /// Byte offset where the class-definition record body begins.
    pub(crate) body_offset: usize,
}

/// Parse a Parasolid header from a buffer containing a leading-window signature.
///
/// Returns `None` when the signature, description, or schema token is missing or
/// truncated.
pub(crate) fn stream_header(payload: &[u8]) -> Result<Option<StreamHeader>, CodecError> {
    let Some((description_bytes, token, schema_end)) = (|| {
        let sig = parasolid_offset(payload)?;
        let desc_len_at = sig + 4;
        let mut view = View::over_retained(payload);
        view.seek(desc_len_at)?;
        let desc_len = usize::from(view.u16_be()?);
        let desc_start = desc_len_at + 2;
        let desc_end = desc_start + desc_len;
        let description_bytes = payload.get(desc_start..desc_end)?;

        // The padding between description and the length-prefixed schema token is not
        // fixed, so the `SCH_` marker is located directly; the preceding byte is the
        // schema length ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md#31-stream-header)).
        let window_end = (desc_end + 64).min(payload.len());
        let token = cadmpeg_parasolid::find_u8_length_prefixed_schema_token(
            payload.get(desc_end..window_end)?,
        )?;
        Some((description_bytes, token, desc_end + token.end()))
    })() else {
        return Ok(None);
    };
    let mut description = String::new();
    append_lossy_utf8(&mut description, description_bytes);

    let schema_text = token.value();
    let owned_schema = schema_text.to_owned();
    let schema = cadmpeg_parasolid::OwnedSchemaToken::try_from(owned_schema)
        .map_err(|_| CodecError::Malformed("Parasolid schema token is invalid".into()))?;

    Ok(Some(StreamHeader {
        description,
        schema,
        body_offset: schema_end,
    }))
}

fn append_lossy_utf8(output: &mut String, mut bytes: &[u8]) {
    loop {
        match std::str::from_utf8(bytes) {
            Ok(valid) => {
                output.push_str(valid);
                break;
            }
            Err(error) => {
                let valid = std::str::from_utf8(&bytes[..error.valid_up_to()]);
                if let Ok(valid) = valid {
                    output.push_str(valid);
                }
                output.push(char::REPLACEMENT_CHARACTER);
                let invalid = error
                    .error_len()
                    .unwrap_or(bytes.len() - error.valid_up_to());
                let Some(remaining) = bytes.get(error.valid_up_to() + invalid..) else {
                    break;
                };
                bytes = remaining;
            }
        }
    }
}

fn parasolid_offset(payload: &[u8]) -> Option<usize> {
    const SIGNATURE: &[u8] = b"PS\0\0";
    let window = payload.len().min(64);
    cadmpeg_core::bytes::find(&payload[..window], SIGNATURE)
}

/// Test whether the description identifies a partition or deltas body stream.
pub(crate) fn is_body_stream(header: &StreamHeader) -> bool {
    let bytes = header.description.as_bytes();
    bytes
        .windows(9)
        .any(|part| part.eq_ignore_ascii_case(b"partition"))
        || bytes
            .windows(6)
            .any(|part| part.eq_ignore_ascii_case(b"deltas"))
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
    let schema = header.schema.value();
    if !schema.as_bytes().ends_with(b"_13006") {
        return None;
    }
    let scan_end = payload.len().checked_sub(2)?;
    let mut candidates = Vec::new();
    for tag_at in header.body_offset..scan_end {
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
        let mut points = Vec::new();
        for xyz in values.chunks_exact(24) {
            let (Some(x), Some(y), Some(z)) = (
                View::f64_be_at(xyz, 0),
                View::f64_be_at(xyz, 8),
                View::f64_be_at(xyz, 16),
            ) else {
                return None;
            };
            let point = Point3::new(x, y, z);
            if !point.is_finite() {
                points.clear();
                break;
            }
            points.push(point);
        }
        if points.len() >= 2 {
            candidates.push((scalar_count, points));
        }
    }
    candidates.sort_by_key(|(count, _)| std::cmp::Reverse(*count));
    let (largest_count, _) = candidates.first()?;
    if candidates
        .get(1)
        .is_some_and(|(count, _)| count == largest_count)
    {
        return None;
    }
    candidates.into_iter().next().map(|(_, points)| points)
}

#[cfg(test)]
mod tests;
