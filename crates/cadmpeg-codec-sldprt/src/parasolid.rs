// SPDX-License-Identifier: Apache-2.0
//! Extraction and header parsing for embedded Parasolid streams.
//!
//! A stream starts with `PS\0\0`, a big-endian description length and
//! description, padding, and a length-prefixed
//! `SCH_<modeller>_<schema>_<format>` token. Outer blocks may carry direct
//! streams or zlib-compressed streams inside a transmit wrapper. Stream
//! descriptions identify partition, deltas, and feature-profile payloads.

use cadmpeg_container::compression::inflate_zlib_member;
use cadmpeg_core::decode::{DecodeContext, ExpandSpec, ScopedReservation, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Point3;
use flate2::{Decompress, FlushDecompress, Status};
use std::collections::HashMap;

use crate::container::NameWords;

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
///
/// Streams come out in payload order: direct streams by their signature
/// offset, wrapped streams by their wrapper offset, nested zlib streams by
/// their member offset.
pub(crate) fn extract_streams_with_offsets(
    payload: &[u8],
    ctx: &DecodeContext<'_>,
) -> Result<Vec<ExtractedStream>, CodecError> {
    let wrapped_prefix = has_wrapped_prefix(payload);
    if !wrapped_prefix {
        let direct = direct_streams(payload, ctx)?;
        if !direct.is_empty() {
            return Ok(direct);
        }
    }
    let mut out = DistinctStreams::new(ctx)?;
    let mut wrapped = false;
    for magic_at in ctx.find_bytes_iter(
        payload,
        &WRAPPED_MAGIC_PREFIX,
        "find Parasolid wrapper prefix",
    )? {
        wrapped = true;
        let stream = if magic_at == 0 {
            single_wrapped_stream(payload, magic_at, ctx)?
        } else {
            chained_wrapped_stream(payload, magic_at, ctx)?
        };
        if let Some(stream) = stream {
            out.push(ctx, stream, "collect wrapped Parasolid streams")?;
        }
    }
    // A payload with the section prefix is a malformed chained wrapper, not a
    // reason to retain its first frame as a complete stream. This prevents a
    // bad continuation from silently recreating the historical one-megabyte
    // truncation.
    if !wrapped || !out.streams.is_empty() || wrapped_prefix {
        return Ok(out.streams);
    }

    // Preserve older nested wrappers that do not carry the chained-section
    // prefix. Try each zlib member; the first that inflates to a `PS\0\0`-leading
    // stream is the embedded body. zlib headers are `78 01` / `78 9c` / `78 da`.
    let local_limit = cadmpeg_core::decode::u64_from_index(payload.len())
        .checked_mul(16)
        .ok_or_else(|| {
            cadmpeg_core::decode::refuse_local_limit(
                "sldprt Parasolid probe work",
                u64::MAX,
                u64::MAX,
            )
        })?;
    let work = ctx.work_budget(local_limit);
    let mut work_used = 0_u64;
    for (i, pair) in ctx
        .admit_iter(payload, "scan Parasolid zlib candidates")?
        .windows(const { crate::nonzero(2) })
        .enumerate()
    {
        if pair[0] != 0x78 || !matches!(pair[1], 0x01 | 0x9c | 0xda) {
            continue;
        }
        let effort = u64::try_from(payload.len() - i)
            .map_err(|_| CodecError::NotImplemented("Parasolid probe work exceeds u64".into()))?;
        if !work.charge_by(payload.len() - i) {
            ctx.charge_work(0, "probe Parasolid zlib candidates")?;
            return Err(ctx.refuse_codec_limit(
                "probe Parasolid zlib candidates",
                local_limit,
                work_used.checked_add(effort).ok_or_else(|| {
                    CodecError::NotImplemented("Parasolid probe work exceeds u64".into())
                })?,
            ));
        }
        work_used = work_used
            .checked_add(effort)
            .ok_or_else(|| CodecError::NotImplemented("Parasolid probe work exceeds u64".into()))?;
        let inner =
            match inflate_zlib_member(ctx, View::over_retained(&payload[i..]), ExpandSpec::Unknown)
            {
                Ok((view, _)) if view.window().starts_with(b"PS\0\0") => {
                    if let Some(header) = stream_header(ctx, view.window())? {
                        Some(ExtractedStream {
                            offset: i,
                            payload: ctx
                                .copy_retained(view.window(), "retain Parasolid zlib candidate")?,
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
            out.push(ctx, stream, "collect nested Parasolid streams")?;
        }
    }
    Ok(out.streams)
}

/// Streams already extracted from one payload, in extraction order, with an
/// index by content hash. A repeated stream is found by comparing it with the
/// streams that share its hash, not with every earlier stream.
struct DistinctStreams<'ctx> {
    streams: Vec<ExtractedStream>,
    by_hash: HashMap<u64, Vec<usize>>,
    index_storage: ScopedReservation<'ctx>,
}

impl<'ctx> DistinctStreams<'ctx> {
    fn new(ctx: &'ctx DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            streams: Vec::new(),
            by_hash: HashMap::new(),
            index_storage: ctx.reserve_scoped(0, "index Parasolid stream candidates")?,
        })
    }

    /// Keep `stream` unless an earlier stream has the same bytes.
    fn push(
        &mut self,
        ctx: &DecodeContext<'_>,
        stream: ExtractedStream,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        const OPERATION: &str = "compare Parasolid stream candidates";
        let hash = ctx.hash_value(stream.payload.as_slice(), OPERATION)?;
        if let Some(group) = ctx.get_hash_map(&self.by_hash, &hash, OPERATION)? {
            if ctx.any_by(
                group,
                |known| ctx.equal_bytes(&self.streams[*known].payload, &stream.payload, OPERATION),
                OPERATION,
            )? {
                return Ok(());
            }
        }
        let index = self.streams.len();
        ctx.push_vec(&mut self.streams, stream, operation)?;
        let by_hash = &mut self.by_hash;
        self.index_storage.with_storage(|| {
            ctx.push_hash_group(
                by_hash,
                hash,
                index,
                "index Parasolid stream candidates",
                "index Parasolid stream candidates",
            )
        })
    }
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
    let inflated = inflate_zlib_frame_budgeted(ctx, member, uncompressed_size)?;
    inflated.map_or(Ok(None), |bytes| extracted_stream(ctx, magic_at, bytes))
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
    let mut frame_outputs = Vec::new();
    while frame_at < section_end {
        let Some(remaining) = payload.get(frame_at..section_end) else {
            return Ok(None);
        };
        // Zero padding closes the section; the test stops at the first
        // nonzero byte, which is normally the next frame's first byte.
        if ctx.all_by(
            remaining,
            |byte| Ok(*byte == 0),
            "scan Parasolid chained section padding",
        )? {
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
        let Some(frame) = inflate_zlib_frame_budgeted(ctx, member, uncompressed_size)? else {
            return Ok(None);
        };
        ctx.push_vec(&mut frame_outputs, frame, "collect Parasolid frames")?;
        frame_at = member_end;
    }
    let stream = match frame_outputs.len() {
        0 => return Ok(None),
        1 => match frame_outputs.pop() {
            Some(frame) => frame,
            None => return Ok(None),
        },
        _ => ctx.concat_retained(&frame_outputs, "retain concatenated Parasolid stream")?,
    };
    extracted_stream(ctx, chain_len_at, stream)
}

fn extracted_stream(
    ctx: &DecodeContext<'_>,
    offset: usize,
    payload: Vec<u8>,
) -> Result<Option<ExtractedStream>, CodecError> {
    if !payload.starts_with(b"PS\0\0") {
        return Ok(None);
    }
    let Some(header) = stream_header(ctx, &payload)? else {
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

fn inflate_zlib_frame_budgeted(
    ctx: &DecodeContext<'_>,
    member: &[u8],
    expected: usize,
) -> Result<Option<Vec<u8>>, CodecError> {
    if expected == 0 {
        return Ok(None);
    }
    let declared = u64::try_from(expected)
        .map_err(|_| CodecError::NotImplemented("Parasolid frame length exceeds u64".into()))?;
    if expected > MAX_WRAPPED_FRAME_UNCOMPRESSED {
        return Err(ctx.refuse_codec_limit(
            "inflate Parasolid frame",
            cadmpeg_core::decode::u64_from_index(MAX_WRAPPED_FRAME_UNCOMPRESSED),
            declared,
        ));
    }
    let reservation = ctx.reserve_scoped(declared, "inflate Parasolid frame")?;
    let mut output = ctx.begin_expand(ExpandSpec::Exact(declared))?;
    let work = declared
        .checked_add(cadmpeg_core::decode::u64_from_index(member.len()))
        .ok_or_else(|| ctx.refuse_codec_limit("inflate Parasolid frame", u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(work, "inflate Parasolid frame")?;
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
            reservation.commit()?;
            return output.finalize_owned().map(Some);
        }
        if consumed == 0 && produced == 0 {
            return Ok(None);
        }
    }
}

/// Every directly framed stream, each running from its signature to the
/// next stream's signature or the payload end.
fn direct_streams(
    payload: &[u8],
    ctx: &DecodeContext<'_>,
) -> Result<Vec<ExtractedStream>, CodecError> {
    let mut streams = Vec::new();
    let mut pending: Option<(usize, StreamHeader)> = None;
    for start in ctx.find_bytes_iter(payload, b"PS\0\0", "scan Parasolid stream header windows")? {
        let Some(header) = stream_header(ctx, &payload[start..])? else {
            continue;
        };
        if let Some((previous, previous_header)) = pending.replace((start, header)) {
            push_direct_stream(ctx, &mut streams, payload, previous..start, previous_header)?;
        }
    }
    if let Some((start, header)) = pending {
        push_direct_stream(ctx, &mut streams, payload, start..payload.len(), header)?;
    }
    Ok(streams)
}

fn push_direct_stream(
    ctx: &DecodeContext<'_>,
    streams: &mut Vec<ExtractedStream>,
    payload: &[u8],
    range: std::ops::Range<usize>,
    header: StreamHeader,
) -> Result<(), CodecError> {
    ctx.reserve_vec(streams, 1, "collect direct Parasolid streams")?;
    let offset = range.start;
    let payload = ctx.copy_retained(&payload[range], "retain direct Parasolid stream")?;
    streams.push(ExtractedStream {
        offset,
        payload,
        header,
    });
    Ok(())
}

/// Parsed framing fields for one Parasolid stream.
#[derive(Debug, Clone)]
pub(crate) struct StreamHeader {
    /// Human-readable stream description.
    pub(crate) description: String,
    /// Classification words of [`Self::description`].
    pub(crate) words: NameWords,
    /// `SCH_<modeller>_<schema>_<format>` schema token.
    pub(crate) schema: cadmpeg_parasolid::OwnedSchemaToken,
    /// Byte offset where the class-definition record body begins.
    pub(crate) body_offset: usize,
}

/// Parse a Parasolid header from a buffer containing a leading-window signature.
///
/// Returns `None` when the signature, description, or schema token is missing or
/// truncated.
pub(crate) fn stream_header(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Option<StreamHeader>, CodecError> {
    let window = payload.len().min(64);
    let Some(sig) = ctx.find_bytes(
        &payload[..window],
        b"PS\0\0",
        "decode Parasolid stream header",
    )?
    else {
        return Ok(None);
    };
    let Some((description_bytes, desc_end)) = (|| {
        let desc_len_at = sig + 4;
        let mut view = View::over_retained(payload);
        view.seek(desc_len_at)?;
        let desc_len = usize::from(view.u16_be()?);
        let desc_start = desc_len_at + 2;
        let desc_end = desc_start + desc_len;
        Some((payload.get(desc_start..desc_end)?, desc_end))
    })() else {
        return Ok(None);
    };
    // The padding before the length-prefixed schema token is variable. The
    // preceding byte bounds the token separately from the first record.
    let window_end = (desc_end + 64).min(payload.len());
    let Some(prologue) = payload.get(desc_end..window_end) else {
        return Ok(None);
    };
    let Some(token) = cadmpeg_parasolid::find_u8_length_prefixed_schema_token(ctx, prologue)?
    else {
        return Ok(None);
    };
    let schema_end = desc_end + token.end();
    let header_work = cadmpeg_core::decode::u64_from_index(description_bytes.len())
        .checked_mul(4)
        .ok_or_else(|| {
            ctx.refuse_codec_limit("decode Parasolid stream header", u64::MAX - 1, u64::MAX)
        })?;
    ctx.charge_work(header_work, "decode Parasolid stream header")?;
    let description_len = lossy_utf8_len(description_bytes).ok_or_else(|| {
        ctx.refuse_codec_limit(
            "retain Parasolid stream description",
            u64::MAX - 1,
            u64::MAX,
        )
    })?;
    let mut description = String::new();
    ctx.try_reserve_retained_text(
        &mut description,
        description_len,
        "retain Parasolid stream description",
    )?;
    append_lossy_utf8(ctx, &mut description, description_bytes)?;
    let words = NameWords::of(ctx, &description, "classify Parasolid stream description")?;

    let schema_text = token.value();
    let owned_schema = ctx.copy_retained_text(schema_text, "retain Parasolid schema token")?;
    let schema = cadmpeg_parasolid::OwnedSchemaToken::parse(ctx, owned_schema)?
        .map_err(|_| CodecError::Malformed("Parasolid schema token is invalid".into()))?;

    Ok(Some(StreamHeader {
        description,
        words,
        schema,
        body_offset: schema_end,
    }))
}

fn lossy_utf8_len(mut bytes: &[u8]) -> Option<usize> {
    let mut size = 0usize;
    loop {
        match std::str::from_utf8(bytes) {
            Ok(valid) => return size.checked_add(valid.len()),
            Err(error) => {
                size = size.checked_add(error.valid_up_to())?.checked_add(3)?;
                let invalid = error
                    .error_len()
                    .unwrap_or(bytes.len() - error.valid_up_to());
                bytes = bytes.get(error.valid_up_to().checked_add(invalid)?..)?;
            }
        }
    }
}

fn append_lossy_utf8(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    output: &mut String,
    mut bytes: &[u8],
) -> Result<(), cadmpeg_core::CodecError> {
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
                ctx.push_retained_char(
                    &mut *output,
                    char::REPLACEMENT_CHARACTER,
                    "append SLDPRT decoded character",
                )?;
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
    Ok::<_, cadmpeg_core::CodecError>(())
}

impl StreamHeader {
    /// Whether the description identifies a partition or deltas body stream.
    pub(crate) fn is_body_stream(&self) -> bool {
        self.words.partition || self.words.deltas
    }
}

/// Decode the unique counted XYZ polyline carried by a classified mesh stream.
///
/// Mesh coordinate arrays use a big-endian scalar count followed by the
/// `0x0022` array tag and consecutive f64 values. The scalar count is three
/// times the point count.
pub(crate) fn mesh_polyline_from_header(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    header: &StreamHeader,
) -> Result<Option<Vec<Point3>>, CodecError> {
    const SCAN: &str = "scan Parasolid mesh coordinates";
    if !header.schema.value().as_bytes().ends_with(b"_13006") {
        return Ok(None);
    }
    let Some(scan_end) = payload.len().checked_sub(2) else {
        return Ok(None);
    };
    // Every counted array that fits the payload, as (scalar count, tag offset).
    let mut storage = ctx.reserve_scoped(0, SCAN)?;
    let mut arrays = Vec::new();
    for tag_at in ctx.admit_iter(header.body_offset..scan_end, SCAN)? {
        if tag_at < 4 || payload.get(tag_at..tag_at + 2) != Some(&[0x00, 0x22]) {
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
        if mesh_coordinates(payload, tag_at, scalar_count).is_none() {
            continue;
        }
        ctx.push_scoped_vec(&mut storage, &mut arrays, (scalar_count, tag_at), SCAN)?;
    }
    ctx.stable_sort_by_key(
        &mut arrays,
        |(scalar_count, _)| *scalar_count,
        |left, right| right.cmp(left),
        "sort Parasolid mesh candidates",
    )?;
    // The polyline is the largest array whose coordinates are all finite,
    // when no other finite array has its size. Arrays are visited largest
    // first, and only down to the selected size.
    let mut selected: Option<(usize, usize)> = None;
    let mut candidates = arrays.iter();
    while let Some(&(scalar_count, tag_at)) =
        ctx.next_charged(&mut candidates, "select Parasolid mesh candidate")?
    {
        if selected.is_some_and(|(selected_count, _)| scalar_count < selected_count) {
            break;
        }
        let Some(values) = mesh_coordinates(payload, tag_at, scalar_count) else {
            continue;
        };
        if !ctx.all_by(
            values.chunks_exact(8),
            |value| Ok(View::f64_be_at(value, 0).is_some_and(f64::is_finite)),
            "decode Parasolid mesh coordinates",
        )? {
            continue;
        }
        if selected.is_some() {
            return Ok(None);
        }
        selected = Some((scalar_count, tag_at));
    }
    let Some((scalar_count, tag_at)) = selected else {
        return Ok(None);
    };
    let Some(values) = mesh_coordinates(payload, tag_at, scalar_count) else {
        return Ok(None);
    };
    let mut points = ctx.vector_storage(scalar_count / 3, "decode Parasolid mesh points")?;
    for xyz in ctx
        .admit_iter(values, "decode Parasolid mesh points")?
        .chunks(const { crate::nonzero(24) })
    {
        let (Some(x), Some(y), Some(z)) = (
            View::f64_be_at(xyz, 0),
            View::f64_be_at(xyz, 8),
            View::f64_be_at(xyz, 16),
        ) else {
            return Ok(None);
        };
        ctx.push_vec(
            &mut points,
            Point3::new(x, y, z),
            "decode Parasolid mesh points",
        )?;
    }
    Ok(Some(points))
}

/// The big-endian f64 values a counted array tag at `tag_at` states, when
/// they lie inside the payload.
fn mesh_coordinates(payload: &[u8], tag_at: usize, scalar_count: usize) -> Option<&[u8]> {
    let start = tag_at.checked_add(2)?;
    payload.get(start..start.checked_add(scalar_count.checked_mul(8)?)?)
}

#[cfg(test)]
mod tests;
