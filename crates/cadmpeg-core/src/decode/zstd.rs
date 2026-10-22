// SPDX-License-Identifier: Apache-2.0
//! Window-admitted Zstandard streams.
//!
//! The `open_zstd` / `ZstdDecoder` interface, built on the pure-Rust `ruzstd` crate rather
//! than on `zstd::zstd_safe` (libzstd through `zstd-sys` and `cc`), so the workspace builds
//! with no C code.
//!
//! `ruzstd` is a block decoder with a ring buffer, not a port of libzstd, so everything
//! libzstd checks and `ruzstd` does not is checked HERE, from this module's own reading of
//! the frame, and each was measured against the Zstandard CLI v1.5.5 on 2026-10-12:
//!
//! | rule | libzstd | bare `ruzstd` 0.9 | this module |
//! |---|---|---|---|
//! | reserved frame-descriptor bit | refused | accepted | refused at open |
//! | content checksum compared | refused on mismatch | read, never compared | compared |
//! | `Frame_Content_Size` equals output | refused on mismatch | ignored | compared |
//! | block header size above `Block_Maximum_Size` | refused | refused since 0.9.1 | refused |
//! | sequences regenerating past `Block_Maximum_Size` | refused | refused since 0.9.1 | (`ruzstd`'s) |
//! | several frames, skippable frames | decoded / skipped | first frame only | decoded / skipped |
//! | bytes after the last frame | refused | ignored | refused at open |
//!
//! One thing `ruzstd` does not reproduce, stated rather than hidden: **a compressed block's
//! LITERALS are not held to `Block_Maximum_Size`.** `ruzstd` 0.9.1 refuses a block whose
//! sequences regenerate past the maximum, as it executes them: a 227-byte frame of a hundred
//! maximal matches is refused with 0.27 MB of heap, where 0.9.0 built its 13 MiB first (29.9
//! MB; both measured, 2026-10-13). That is why the requirement is `0.9.1` and not `0.9`. What
//! it still does not check is the literals: a block with no sequences, and the literals after
//! a block's last sequence. A literals section states at most 2^20 - 1 bytes, so the excess is
//! bounded by 1 MiB a block, is ALLOCATED before anything can refuse it, and is refused
//! afterwards by this module whenever the block's size is determinable (see `decode_block`).
//! It is not determinable for a compressed block decoded while the buffer is still under its
//! window and its length unknown; the test `zstd_known_gap_...` pins the frame that gets
//! through.
//!
//! Two differences in memory behaviour, also stated rather than hidden:
//!
//! - **The admission bounds the window, not peak heap.** libzstd allocates one window per
//!   frame. `ruzstd`'s ring buffer grows by reallocation and holds the old and new buffers
//!   while copying, so a frame that fills its window peaks near two windows plus three
//!   blocks: about twice the window the admission charged.
//! - **A failed allocation is not returned.** libzstd reported an allocation failure as an
//!   error, which this module turned into `retained_allocation_failed`; `ruzstd` panics on a
//!   null from the allocator for its ring buffer and aborts through the global allocation
//!   error handler elsewhere, so an allocation that fails inside the decoder now panics or
//!   aborts instead of returning an error.

use std::io::Read as _;

use ruzstd::decoding::errors::FrameDecoderError;
use ruzstd::decoding::{BlockDecodingStrategy, FrameDecoder};

use super::{
    u64_from_index, DecodeContext, LimitScope, ResourceDimension, ResourceFailure, ResourceLimit,
    View,
};
use crate::CodecError;

const BLOCK_BYTES: u64 = 128 * 1024;
/// The largest window the decoder opens on this target. A larger frame is
/// outside the reader, not over a session ceiling.
const MAX_DECODER_WINDOW: u64 = 1_u64 << if usize::BITS == 32 { 30 } else { 31 };
const OUTPUT_STEP_BYTES: usize = 16 * 1024;
const FRAME_MAGIC: u32 = 0xfd2f_b528;

/// One Zstandard decoder over an admitted input.
pub struct ZstdDecoder<'ctx, 'input> {
    frame: Option<ActiveFrame<'input>>,
    /// The decoder is opened under its context's admission and does not outlive it.
    ctx: &'ctx DecodeContext<'input>,
    /// Frames not yet opened. Every byte was framed by `open_zstd`.
    input: View<'input>,
    finished: bool,
}

/// The frame being decoded, with this module's own account of it beside `ruzstd`'s.
struct ActiveFrame<'input> {
    decoder: FrameDecoder,
    /// Exactly this frame's bytes, as measured by `inspect_frame`; `ruzstd` reads from here
    /// and must leave it empty.
    source: &'input [u8],
    window: u64,
    block_maximum: u64,
    content_size: Option<u64>,
    produced: u64,
    /// Bytes held in `ruzstd`'s ring buffer, when that is determinable.
    buffered: Option<u64>,
}

/// What `open_zstd` and `read_chunk` need from one frame's framing.
struct Inspected {
    /// Whole frame, header to checksum, in bytes.
    size: usize,
    /// `None` for a skippable frame, which has no decoder state.
    data: Option<FrameFacts>,
}

#[derive(Clone, Copy)]
struct FrameFacts {
    /// `Window_Size`, or `Frame_Content_Size` for a single-segment frame.
    window: u64,
    content_size: Option<u64>,
}

impl<'input> DecodeContext<'input> {
    /// Inspects all frame windows before allocating a decoder. The output
    /// each frame's decoder buffers, its window or its declared content size
    /// when that is smaller, must fit the per-expansion decompression
    /// allowance, and the declared window must fit the decoder's. Either
    /// excess raises a part-scoped resource refusal before allocation.
    pub fn open_zstd<'ctx>(
        &'ctx self,
        source: View<'input>,
    ) -> Result<ZstdDecoder<'ctx, 'input>, CodecError> {
        let mut offset = 0;
        let allowance = self.per_expand_allowance();
        while offset < source.window().len() {
            let frame = source
                .child(source.start() + offset, source.end())
                .ok_or_else(|| {
                    CodecError::Malformed("Zstandard frame offset escapes input".into())
                })?;
            if let Some(facts) = frame_header(frame)?.1 {
                let buffered = facts
                    .content_size
                    .map_or(facts.window, |size| size.min(facts.window));
                if buffered > allowance {
                    return Err(self.refuse_part_limit(
                        ResourceDimension::DecompressedBytes,
                        allowance,
                        0,
                        buffered,
                        "Zstandard per-expansion window",
                    ));
                }
                if facts.window > MAX_DECODER_WINDOW {
                    return Err(self
                        .budget
                        .refuse_resource(window_limit(facts.window))
                        .into());
                }
            }
            let Inspected { size, .. } = inspect_frame(frame)?;
            if size == 0 || size > frame.window().len() {
                return Err(CodecError::Malformed("invalid Zstandard frame size".into()));
            }
            offset += size;
        }
        if offset == 0 {
            return Err(CodecError::Malformed("empty Zstandard stream".into()));
        }
        // No decoder exists yet: `ruzstd` allocates per frame, on `FrameDecoder::init`, and
        // each frame is opened with its own inspected window as the decoder's limit.
        Ok(ZstdDecoder {
            frame: None,
            ctx: self,
            input: source
                .child(source.start(), source.end())
                .ok_or_else(|| CodecError::Malformed("Zstandard input escapes its view".into()))?,
            finished: false,
        })
    }
}

impl ZstdDecoder<'_, '_> {
    /// Reads decoded output into caller-owned storage.
    pub fn read_chunk(&mut self, bytes: &mut [u8]) -> Result<usize, CodecError> {
        if let Some(limit) = self.ctx.resource_refusal() {
            return Err(limit.into());
        }
        if self.finished || bytes.is_empty() {
            return Ok(0);
        }
        let capacity = bytes.len().min(OUTPUT_STEP_BYTES);
        // One step is at most: open a frame, decode one block, collect one chunk, close the
        // frame.
        loop {
            if self.frame.is_none() {
                if self.input.is_empty() {
                    self.finished = true;
                    return Ok(0);
                }
                self.open_next_frame()?;
            }
            let Some(frame) = self.frame.as_mut() else {
                // A skippable frame was passed over.
                continue;
            };
            if frame.decoder.can_collect() == 0 && !frame.decoder.is_finished() {
                frame
                    .decode_block()
                    .map_err(|error| fuse_zstd_error(self.ctx, error))?;
            }
            let produced = frame
                .decoder
                .read(&mut bytes[..capacity])
                .map_err(|error| malformed(&error))?;
            frame.produced += u64_from_index(produced);
            if frame
                .content_size
                .is_some_and(|declared| frame.produced > declared)
            {
                return Err(CodecError::Malformed(
                    "Zstandard frame regenerates more than its declared content size".into(),
                ));
            }
            // The frame's own verdict travels with its last bytes: a caller that stops
            // reading once its buffer is full must not be able to skip the checksum.
            if frame.decoder.is_finished() && frame.decoder.can_collect() == 0 {
                self.close_frame()?;
                if self.input.is_empty() {
                    self.finished = true;
                }
            }
            if produced != 0 || self.finished {
                return Ok(produced);
            }
        }
    }

    /// Skips a skippable frame, or creates the decoder for the next data frame.
    fn open_next_frame(&mut self) -> Result<(), CodecError> {
        let inspected = inspect_frame(self.input)?;
        let source = self
            .input
            .take(inspected.size)
            .ok_or_else(|| CodecError::Malformed("truncated Zstandard stream".into()))?;
        let Some(facts) = inspected.data else {
            return Ok(());
        };
        let mut decoder = FrameDecoder::new();
        // The limit is this frame's own admitted window, so `ruzstd` cannot size a buffer
        // from a header this module read differently.
        decoder.set_max_window_size(facts.window);
        let mut frame = ActiveFrame {
            decoder,
            source,
            window: facts.window,
            block_maximum: facts.window.min(BLOCK_BYTES),
            content_size: facts.content_size,
            produced: 0,
            buffered: Some(0),
        };
        frame
            .decoder
            .init(&mut frame.source)
            .map_err(|error| fuse_zstd_error(self.ctx, zstd_error(&error, facts.window)))?;
        self.frame = Some(frame);
        Ok(())
    }

    /// The checks libzstd makes at the end of a frame and `ruzstd` leaves to its caller.
    fn close_frame(&mut self) -> Result<(), CodecError> {
        let Some(frame) = self.frame.take() else {
            return Ok(());
        };
        // `ruzstd` reads the frame's content checksum and never compares it. `None` on the
        // declared side is a frame without one, which is legal and is what most encoders write.
        if let (Some(declared), Some(computed)) = (
            frame.decoder.get_checksum_from_data(),
            frame.decoder.get_calculated_checksum(),
        ) {
            if declared != computed {
                return Err(CodecError::Malformed(
                    "Zstandard content checksum mismatch".into(),
                ));
            }
        }
        if frame
            .content_size
            .is_some_and(|declared| declared != frame.produced)
        {
            return Err(CodecError::Malformed(
                "Zstandard frame does not regenerate its declared content size".into(),
            ));
        }
        // `inspect_frame` and `ruzstd` each walked this frame's blocks; they must agree on
        // where it ends, or the bytes between are neither decoded nor refused. An invariant
        // check: no input is known that separates the two walks, so no test can fail on it.
        if !frame.source.is_empty() {
            return Err(CodecError::Malformed(
                "Zstandard decoder stopped before the end of its frame".into(),
            ));
        }
        Ok(())
    }
}

impl ActiveFrame<'_> {
    /// Decodes exactly one block, holding it to `Block_Maximum_Size` on both sides.
    fn decode_block(&mut self) -> Result<(), CodecError> {
        let header = View::u24_le_at(self.source, 0)
            .ok_or_else(|| CodecError::Malformed("truncated Zstandard stream".into()))?;
        let kind = (header >> 1) & 3;
        let declared = u64::from(header >> 3);
        // libzstd holds the header's size field to min(window, 128 KiB) for every block
        // type; `ruzstd` holds it to 128 KiB alone.
        if declared > self.block_maximum {
            return Err(CodecError::Malformed(
                "Zstandard block size exceeds its frame's maximum".into(),
            ));
        }
        self.decoder
            .decode_blocks(&mut self.source, BlockDecodingStrategy::UptoBlocks(1))
            .map_err(|error| zstd_error(&error, self.window))?;
        // What the block regenerated. For a raw or RLE block the header says; for a
        // compressed block `ruzstd` exposes only what is collectable, which gives the buffer
        // length exactly once the frame is finished or the buffer is past its window, and
        // not otherwise.
        let collectable = u64_from_index(self.decoder.can_collect());
        let length = if self.decoder.is_finished() {
            Some(collectable)
        } else if collectable != 0 {
            self.window.checked_add(collectable)
        } else if kind == 2 {
            None
        } else {
            self.buffered
                .and_then(|buffered| buffered.checked_add(declared))
        };
        if let Some(length) = length {
            // Without the earlier length, the buffer held at most one window.
            let before = self.buffered.unwrap_or(self.window);
            if length
                .checked_sub(before)
                .is_some_and(|grown| grown > self.block_maximum)
            {
                return Err(CodecError::Malformed(
                    "Zstandard block regenerates more than its frame's maximum".into(),
                ));
            }
        }
        // Everything past the window is collected before the next block is decoded.
        self.buffered = length.map(|length| length.min(self.window));
        Ok(())
    }
}

fn window_limit(window: u64) -> ResourceLimit {
    ResourceLimit {
        scope: LimitScope::Part,
        dimension: ResourceDimension::DecompressedBytes,
        reason: ResourceFailure::BudgetExceeded,
        limit: MAX_DECODER_WINDOW,
        used: 0,
        additional: window,
        operation: "Zstandard decoder window",
    }
}

fn fuse_zstd_error(ctx: &DecodeContext<'_>, error: CodecError) -> CodecError {
    match error {
        CodecError::ResourceLimit(limit) => ctx.budget.refuse_resource(limit).into(),
        error => error,
    }
}

fn malformed(error: &dyn std::fmt::Display) -> CodecError {
    CodecError::malformed(format_args!("invalid Zstandard stream: {error}"))
}

fn zstd_error(error: &FrameDecoderError, window: u64) -> CodecError {
    match error {
        FrameDecoderError::WindowSizeTooBig { .. } => window_limit(window).into(),
        _ => malformed(error),
    }
}

/// Reads one frame header, returning the view past it and the frame's facts. A skippable
/// frame has no facts, and its view is left before its four-byte length.
fn frame_header(mut frame: View<'_>) -> Result<(View<'_>, Option<FrameFacts>), CodecError> {
    let magic = frame.req_u32_le()?;
    if (0x184d_2a50..=0x184d_2a5f).contains(&magic) {
        return Ok((frame, None));
    }
    if magic != FRAME_MAGIC {
        return Err(CodecError::Malformed("invalid Zstandard magic".into()));
    }
    let flags = frame.req_u8()?;
    if flags & 8 != 0 {
        return Err(CodecError::Malformed(
            "reserved Zstandard frame flag".into(),
        ));
    }
    let single_segment = flags & 32 != 0;
    let window = if single_segment {
        0
    } else {
        let descriptor = frame.req_u8()?;
        let base = 1_u64 << (10 + u32::from(descriptor >> 3));
        base + (base / 8) * u64::from(descriptor & 7)
    };
    let dictionary_bytes = match flags & 3 {
        0 => 0,
        1 => 1,
        2 => 2,
        _ => 4,
    };
    frame.req_take(dictionary_bytes)?;
    let content_size = match flags >> 6 {
        0 if single_segment => Some(u64::from(frame.req_u8()?)),
        0 => None,
        1 => Some(u64::from(frame.req_u16_le()?) + 256),
        2 => Some(u64::from(frame.req_u32_le()?)),
        _ => Some(frame.req_u64_le()?),
    };
    Ok((
        frame,
        Some(FrameFacts {
            window: if single_segment {
                content_size.unwrap_or(0)
            } else {
                window
            },
            content_size,
        }),
    ))
}

/// Measures one frame without decoding it: header, every block header, optional checksum.
///
/// This is `ZSTD_findFrameCompressedSize`, which the libzstd decoder called and `ruzstd` has no
/// equivalent of. It is what lets every frame's window be admitted before any decoder
/// exists, and what refuses bytes that follow the last frame.
fn inspect_frame(frame: View<'_>) -> Result<Inspected, CodecError> {
    let truncated = || CodecError::Malformed("truncated Zstandard frame".into());
    let start = frame.read_len();
    let checksum = frame.unread().get(4).is_some_and(|flags| flags & 4 != 0);
    let (mut body, facts) = frame_header(frame)?;
    let Some(facts) = facts else {
        let length = usize::try_from(body.req_u32_le()?).map_err(|_| truncated())?;
        body.skip(length).ok_or_else(truncated)?;
        return Ok(Inspected {
            size: body.read_len() - start,
            data: None,
        });
    };
    loop {
        let header = body.take(3).ok_or_else(truncated)?;
        let header = u32::from(header[0]) | u32::from(header[1]) << 8 | u32::from(header[2]) << 16;
        let declared = header >> 3;
        let kind = (header >> 1) & 3;
        let stored = match kind {
            1 => 1,
            3 => {
                return Err(CodecError::Malformed(
                    "reserved Zstandard block type".into(),
                ))
            }
            _ => usize::try_from(declared).map_err(|_| truncated())?,
        };
        body.skip(stored).ok_or_else(truncated)?;
        if header & 1 != 0 {
            break;
        }
    }
    if checksum {
        body.skip(4).ok_or_else(truncated)?;
    }
    Ok(Inspected {
        size: body.read_len() - start,
        data: Some(facts),
    })
}

#[cfg(test)]
mod tests {
    use super::{window_limit, BLOCK_BYTES, MAX_DECODER_WINDOW};
    use crate::decode::{
        DecodeArena, DecodeContext, DecodePolicy, ExpandSpec, LimitScope, ResourceDimension,
        ResourceFailure,
    };
    use crate::CodecError;

    const MAGIC: [u8; 4] = [0x28, 0xb5, 0x2f, 0xfd];

    // Frames are laid out by hand rather than with `zstd::bulk::compress`, which is the
    // libzstd binding this crate no longer links; the frames written by the reference
    // encoder live in `cadmpeg-test-support::zip_fixture` and are decoded through this module
    // by `cadmpeg-container`'s tests, which this crate cannot depend on.

    fn block(last: bool, kind: u32, size: usize, stored: &[u8]) -> Vec<u8> {
        let header = u32::try_from(size).expect("block size") << 3 | kind << 1 | u32::from(last);
        let mut out = header.to_le_bytes()[..3].to_vec();
        out.extend_from_slice(stored);
        out
    }

    fn raw(last: bool, payload: &[u8]) -> Vec<u8> {
        block(last, 0, payload.len(), payload)
    }

    /// A single-segment frame declaring its content size, one raw block.
    fn single_segment(payload: &[u8]) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        out.extend_from_slice(&[0x20, u8::try_from(payload.len()).expect("one-byte size")]);
        out.extend(raw(true, payload));
        out
    }

    /// A frame declaring `window_log` and no content size, from ready-made blocks.
    fn windowed(window_log: u8, blocks: &[Vec<u8>]) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        out.extend_from_slice(&[0x00, (window_log - 10) << 3]);
        for block in blocks {
            out.extend_from_slice(block);
        }
        out
    }

    fn decode_with(bytes: &[u8], policy: &DecodePolicy) -> Result<Vec<u8>, CodecError> {
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, policy).expect("root");
        let mut decoder = ctx.open_zstd(root)?;
        let mut output = Vec::new();
        let mut chunk = [0_u8; 16 * 1024];
        loop {
            let read = decoder.read_chunk(&mut chunk)?;
            if read == 0 {
                return Ok(output);
            }
            output.extend_from_slice(&chunk[..read]);
        }
    }

    fn decode(bytes: &[u8]) -> Result<Vec<u8>, CodecError> {
        decode_with(bytes, &DecodePolicy::service())
    }

    fn assert_malformed(bytes: &[u8], what: &str) {
        let result = decode(bytes);
        assert!(
            matches!(result, Err(CodecError::Malformed(_))),
            "{what}: {result:?}"
        );
    }

    /// A frame cut short is refused as `Truncated` when the cut falls inside a frame header
    /// (the bounded reader's own refusal, as with libzstd) and as `Malformed` anywhere after it.
    fn assert_cut_short(bytes: &[u8], what: &str) {
        let result = decode(bytes);
        assert!(
            matches!(
                result,
                Err(CodecError::Malformed(_) | CodecError::Truncated { .. })
            ),
            "{what}: {result:?}"
        );
    }

    /// A frame header with a non-single-segment window of `window` bytes,
    /// followed by one empty raw block.
    fn frame_with_window(window: u64) -> Vec<u8> {
        let exponent = u8::try_from(window.trailing_zeros() - 10).expect("window exponent");
        vec![0x28, 0xb5, 0x2f, 0xfd, 0, exponent << 3, 1, 0, 0]
    }

    #[test]
    fn zstd_window_past_the_per_expand_allowance_is_unsupported_and_leaves_the_session_open() {
        let window = 32 << 20;
        let bytes = frame_with_window(window);
        let policy = DecodePolicy::service();
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let allowance = ctx.per_expand_allowance();
        assert!(allowance < window);
        assert!(window < policy.limits.max_decompressed_bytes_per_expand);
        let part = ctx
            .part_transaction(root, || ctx.open_zstd(root).map(drop))
            .expect("only this part is given up");
        let Err(CodecError::ResourceLimit(limit)) = part.result() else {
            panic!("a window above the allowance is a part refusal");
        };
        assert_eq!(limit.scope, LimitScope::Part);
        assert_eq!(limit.dimension, ResourceDimension::DecompressedBytes);
        assert_eq!(limit.reason, ResourceFailure::BudgetExceeded);
        assert_eq!(
            (limit.limit, limit.used, limit.additional),
            (allowance, 0, window)
        );
        assert_eq!(limit.operation, "Zstandard per-expansion window");
        assert_eq!(ctx.resource_refusal(), None);
        let mut writer = ctx
            .begin_expand(ExpandSpec::Exact(1))
            .expect("later expansions are admitted");
        writer.write(b"x").expect("write");
        writer.finalize().expect("finalize");
        assert!(ctx.finish_session().is_ok());
    }

    #[test]
    fn zstd_window_is_admitted_by_a_smaller_declared_content_size() {
        let window = 32 << 20;
        let content = [7_u8; 256];
        // A 32 MiB window descriptor, a two-byte content size of 256 bytes,
        // and one raw last block holding the content.
        let mut bytes = vec![
            0x28,
            0xb5,
            0x2f,
            0xfd,
            0x40,
            15 << 3,
            0,
            0,
            0x01,
            0x08,
            0x00,
        ];
        bytes.extend_from_slice(&content);
        let policy = DecodePolicy::service();
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        assert!(ctx.per_expand_allowance() < window);
        let mut decoder = ctx.open_zstd(root).expect("the content size fits");
        let mut output = Vec::new();
        let mut chunk = [0; 64];
        loop {
            let read = decoder.read_chunk(&mut chunk).expect("read");
            if read == 0 {
                break;
            }
            output.extend_from_slice(&chunk[..read]);
        }
        assert_eq!(output, content);
        assert_eq!(ctx.resource_refusal(), None);
    }

    #[test]
    fn zstd_window_above_the_decoder_is_unsupported_and_leaves_the_session_open() {
        let window = MAX_DECODER_WINDOW * 2;
        let mut bytes = frame_with_window(window);
        // Each input byte raises the per-expand allowance by 256 bytes.
        bytes.resize(usize::try_from(window / 256).expect("padding length"), 0);
        let mut policy = DecodePolicy::service();
        policy.limits.max_decompressed_bytes_per_expand = window;
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        assert!(ctx.per_expand_allowance() >= window);
        let part = ctx
            .part_transaction(root, || ctx.open_zstd(root).map(drop))
            .expect("only this part is given up");
        assert!(
            matches!(part.result(), Err(CodecError::ResourceLimit(limit)) if *limit == window_limit(window))
        );
        assert_eq!(ctx.resource_refusal(), None);
        assert!(ctx.finish_session().is_ok());
    }

    #[test]
    fn zstd_checks_each_window_before_decoder_allocation() {
        let mut bytes = single_segment(b"prefix");
        bytes.extend_from_slice(&[0x28, 0xb5, 0x2f, 0xfd, 0, 0xa0, 1, 0, 0]);
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
        let part = ctx
            .part_transaction(root, || ctx.open_zstd(root).map(drop))
            .expect("later oversized frame gives up this part");
        assert!(
            matches!(part.result(), Err(CodecError::ResourceLimit(limit)) if limit.scope == LimitScope::Part && limit.dimension == ResourceDimension::DecompressedBytes && limit.additional == 1_u64 << 30 && limit.limit == ctx.per_expand_allowance())
        );
        assert_eq!(ctx.resource_refusal(), None);
    }

    #[test]
    fn zstd_decodes_a_frame() {
        let bytes = single_segment(b"payload");
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
        let mut decoder = ctx.open_zstd(root).expect("decoder");
        let mut output = [0; 7];
        assert_eq!(decoder.read_chunk(&mut output).expect("read"), 7);
        assert_eq!(&output, b"payload");
    }

    /// A frame that fills its window decodes: twenty 128 KiB blocks under a 1 MiB window,
    /// whose ring buffer holds a window and a block.
    #[test]
    fn zstd_decodes_a_frame_that_fills_its_window() {
        let block_bytes = usize::try_from(BLOCK_BYTES).expect("block size");
        let mut blocks = vec![block(false, 1, block_bytes, b"x"); 19];
        blocks.push(block(true, 1, block_bytes, b"x"));
        let bytes = windowed(20, &blocks);
        assert_eq!(decode(&bytes).expect("fits"), vec![b'x'; 20 * block_bytes]);
    }

    /// A short frame declaring a window above the per-expansion allowance and no content size
    /// is unsupported, though `ruzstd` would never allocate that window for it; a declared
    /// content size is what admits it. The admitted frame's 256 MiB window is above `ruzstd`'s
    /// own default limit (128 MiB in 0.9.1), so it decodes only because the admitted window is
    /// passed to `ruzstd` as that frame's limit.
    #[test]
    fn zstd_short_frame_under_a_large_window_needs_its_content_size() {
        let bytes = windowed(28, &[raw(true, b"abcd")]);
        assert!(matches!(
            decode(&bytes),
            Err(CodecError::ResourceLimit(limit)) if limit.scope == LimitScope::Part && limit.dimension == ResourceDimension::DecompressedBytes && limit.operation == "Zstandard per-expansion window"
        ));
        let mut sized = MAGIC.to_vec();
        sized.extend_from_slice(&[0x80, 18 << 3, 4, 0, 0, 0]); // 256 MiB window, content size 4
        sized.extend(raw(true, b"abcd"));
        assert_eq!(decode(&sized).expect("frame"), b"abcd");
    }

    #[test]
    fn zstd_decodes_every_frame_and_skips_skippable_ones() {
        let first = single_segment(b"first ");
        let second = windowed(10, &[raw(false, b"sec"), raw(true, b"ond")]);
        let skippable = [&[0x53, 0x2a, 0x4d, 0x18, 3, 0, 0, 0][..], b"zzz"].concat();

        assert_eq!(
            decode(&[first.clone(), second.clone()].concat()).expect("two frames"),
            b"first second"
        );
        assert_eq!(
            decode(&[skippable.clone(), first.clone(), skippable.clone(), second].concat())
                .expect("skippable frames between data frames"),
            b"first second"
        );
        assert_eq!(
            decode(&skippable).expect("only a skippable frame"),
            b"",
            "a stream of skippable frames is an empty payload"
        );
        assert_malformed(b"", "an empty stream");
        // Bytes after the last frame belong to no frame, and are refused before any decoding.
        for tail in [&b"junk"[..], &b"\0"[..], &skippable[..9]] {
            let bytes = [&first[..], tail].concat();
            let arena = DecodeArena::new();
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                    .expect("root");
            assert!(
                matches!(
                    ctx.open_zstd(root),
                    Err(CodecError::Malformed(_) | CodecError::Truncated { .. })
                ),
                "a {}-byte tail was admitted",
                tail.len()
            );
        }
    }

    #[test]
    fn zstd_rle_blocks_and_block_boundaries_decode() {
        let frame = windowed(
            10,
            &[
                raw(false, b"ab"),
                block(false, 1, 1000, b"x"),
                block(false, 0, 0, b""),
                block(true, 1, 600, b"y"),
            ],
        );
        let mut expected = b"ab".to_vec();
        expected.extend(std::iter::repeat_n(b'x', 1000));
        expected.extend(std::iter::repeat_n(b'y', 600));
        assert_eq!(decode(&frame).expect("frame"), expected);
        for keep in 0..frame.len() {
            assert_cut_short(&frame[..keep], "a truncated frame");
        }
    }

    /// The content checksum is compared here; `ruzstd` only reads it.
    #[test]
    fn zstd_content_checksum_is_compared() {
        // XXH64("abcd", seed 0) = 0xde0327b0d25d92cc; the frame carries its low 32 bits.
        let mut frame = MAGIC.to_vec();
        frame.extend_from_slice(&[0x04, 0x00]);
        frame.extend(raw(true, b"abcd"));
        frame.extend_from_slice(&0xd25d_92cc_u32.to_le_bytes());
        assert_eq!(decode(&frame).expect("matching checksum"), b"abcd");
        for index in frame.len() - 4..frame.len() {
            let mut corrupt = frame.clone();
            corrupt[index] ^= 1;
            assert_malformed(&corrupt, "a wrong content checksum");
        }
        let payload = frame.len() - 5;
        frame[payload] ^= 1;
        assert_malformed(&frame, "a payload its checksum does not cover");
    }

    /// `Frame_Content_Size` is compared with what the frame regenerated, in both directions.
    #[test]
    fn zstd_declared_content_size_is_compared() {
        let mut frame = single_segment(b"abcd");
        assert_eq!(decode(&frame).expect("exact"), b"abcd");
        frame[5] = 8;
        assert_malformed(&frame, "a frame shorter than its declared size");
        // Longer than declared, with a window so that the block itself is legal.
        let mut frame = MAGIC.to_vec();
        frame.extend_from_slice(&[0x40, 0x00, 0x00, 0x00]); // window 1 KiB, content size 256
        frame.extend(raw(true, &[7; 300]));
        assert_malformed(&frame, "a frame longer than its declared size");
        frame[6] = 44; // 256 + 44
        assert_eq!(decode(&frame).expect("exact"), [7; 300]);
    }

    /// A frame that outruns its declared content size is stopped there, not at its end.
    #[test]
    fn zstd_stops_at_the_declared_content_size() {
        let block_bytes = usize::try_from(BLOCK_BYTES).expect("block size");
        let mut frame = MAGIC.to_vec();
        frame.extend_from_slice(&[0x40, 0x50, 0x00, 0x00]); // window 1 MiB, content size 256
        for index in 0..10 {
            frame.extend(block(index == 9, 1, block_bytes, b"x"));
        }
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&frame, &arena, &DecodePolicy::service()).expect("root");
        let mut decoder = ctx.open_zstd(root).expect("decoder");
        let mut chunk = [0_u8; 16 * 1024];
        let mut delivered = 0;
        let error = loop {
            match decoder.read_chunk(&mut chunk) {
                Ok(0) => panic!("the frame was accepted"),
                Ok(read) => delivered += read,
                Err(error) => break error,
            }
        };
        assert!(matches!(error, CodecError::Malformed(_)), "{error:?}");
        assert!(
            delivered <= 256,
            "{delivered} bytes were delivered past a declared size of 256"
        );
    }

    /// `Block_Maximum_Size` is min(window, 128 KiB), held on the header and on the output.
    #[test]
    fn zstd_blocks_are_held_to_their_frames_maximum() {
        assert_eq!(
            decode(&windowed(10, &[raw(true, &[1; 1024])])).expect("at the maximum"),
            [1; 1024]
        );
        assert_malformed(
            &windowed(10, &[raw(true, &[1; 1025])]),
            "a raw block over its window",
        );
        assert_malformed(
            &windowed(10, &[block(true, 1, 1025, b"x")]),
            "an RLE block over its window",
        );
        let compressed = rle_literals;
        assert_eq!(
            decode(&windowed(10, &[compressed(true, 1000)])).expect("under the maximum"),
            [b'A'; 1000]
        );
        assert_malformed(
            &windowed(10, &[compressed(true, 2000)]),
            "a compressed block regenerating more than its window, last",
        );
        assert_malformed(
            &windowed(10, &[compressed(false, 2000), raw(true, b"z")]),
            "a compressed block regenerating more than its window, not last",
        );
        assert_malformed(
            &windowed(10, &[raw(false, &[1; 600]), compressed(true, 1025)]),
            "a compressed block regenerating more than its window after a raw one",
        );
    }

    /// One compressed block regenerating 13 MiB -- a hundred times any conforming block --
    /// is refused, and by `ruzstd` 0.9.1 itself, at the first sequence past the maximum.
    /// 0.9.0 built all of it first; see the module documentation.
    #[test]
    fn zstd_block_regenerating_past_128_kib_is_refused() {
        const SEQUENCES: u8 = 100;
        // No literals; every sequence is literal length 0, repeat offset, match length code
        // 52 with sixteen extra bits set: 131 074 bytes each.
        let mut content = vec![0x00, SEQUENCES, 0x54, 0, 0, 52];
        content.extend(std::iter::repeat_n(0xff, 2 * usize::from(SEQUENCES)));
        content.push(0x01);
        let frame = windowed(
            21,
            &[
                raw(false, b"abcdefgh"),
                block(true, 2, content.len(), &content),
            ],
        );
        assert_malformed(&frame, "a block regenerating 13 MiB");
    }

    /// One compressed block WITH sequences, written by the reference encoder: the block the
    /// Zstandard CLI v1.5.5 (`zstd -19 --no-check`) emits for `reference_block_plaintext`,
    /// lifted out of its frame. Everything else in this module's tests is laid out by hand;
    /// this is the one block whose regenerated size `ruzstd` does not disclose until the
    /// buffer passes its window.
    const REFERENCE_BLOCK: [u8; 77] = [
        0x42, 0x43, 0x0c, 0x11, 0xa0, 0x6f, 0x20, 0xe9, 0x9f, 0x94, 0x37, 0x01, 0x80, 0xf3, 0xc9,
        0x9f, 0x90, 0xaa, 0xdf, 0x42, 0x1e, 0xc7, 0x14, 0x32, 0x84, 0xb7, 0x27, 0x74, 0x4c, 0x21,
        0x43, 0x38, 0xf8, 0x69, 0xeb, 0x75, 0x8c, 0x9f, 0xaa, 0x15, 0x28, 0x75, 0x90, 0xbf, 0x55,
        0x66, 0x92, 0xbb, 0x9b, 0x3f, 0xba, 0x17, 0x12, 0xa8, 0x10, 0xe8, 0xeb, 0xff, 0x19, 0xf0,
        0x35, 0x03, 0x10, 0x66, 0x84, 0x1d, 0xf1, 0x7f, 0xe3, 0x03, 0xac, 0x7c, 0x2b, 0xad, 0x02,
        0x92, 0x0c,
    ];

    /// The 600 bytes `REFERENCE_BLOCK` regenerates, generated rather than stored.
    fn reference_block_plaintext() -> Vec<u8> {
        let mut out = Vec::new();
        for line in 0..20 {
            out.extend_from_slice(format!("line {line:03} of the miss-case fixture\n").as_bytes());
        }
        out.truncate(600);
        out
    }

    /// RLE literals regenerating `size` bytes of `A`, no sequences: a compressed block whose
    /// regenerated size is readable from its four bytes.
    fn rle_literals(last: bool, size: u16) -> Vec<u8> {
        let header = 1 | 1 << 2 | size << 4;
        block(
            last,
            2,
            4,
            &[&header.to_le_bytes()[..], b"A", &[0]].concat(),
        )
    }

    #[test]
    fn zstd_decodes_a_reference_encoded_block_with_sequences() {
        let plain = reference_block_plaintext();
        let reference = |last| block(last, 2, REFERENCE_BLOCK.len(), &REFERENCE_BLOCK);
        assert_eq!(
            decode(&windowed(10, &[reference(true)])).expect("block"),
            plain
        );
        // Followed by blocks that take the buffer past its 1 KiB window.
        let expected = [&plain[..], &[b'A'; 1000], b"end"].concat();
        assert_eq!(
            decode(&windowed(
                10,
                &[
                    reference(false),
                    rle_literals(false, 1000),
                    raw(true, b"end")
                ]
            ))
            .expect("three blocks"),
            expected
        );
    }

    /// The header's size field is held to the block maximum even when the buffer's length is
    /// not known -- the case the regenerated-size check below cannot see.
    ///
    /// `ruzstd` 0.9.1 refuses the raw and the RLE block itself. The compressed one only this
    /// module refuses: its 1 028 stored bytes are over the maximum, and the 1 025 raw literals
    /// they hold are the literals `ruzstd` does not check.
    #[test]
    fn zstd_block_header_maximum_holds_when_the_buffer_length_is_unknown() {
        let reference = block(false, 2, REFERENCE_BLOCK.len(), &REFERENCE_BLOCK);
        let raw_literals = |size: u16| {
            let header = 1_u16 << 2 | size << 4;
            let stored = [
                &header.to_le_bytes()[..],
                &vec![b'A'; usize::from(size)],
                &[0],
            ]
            .concat();
            block(true, 2, stored.len(), &stored)
        };
        assert_eq!(
            decode(&windowed(10, &[reference.clone(), raw_literals(400)]))
                .expect("control: a compressed block of raw literals under the maximum")
                .len(),
            reference_block_plaintext().len() + 400
        );
        assert_malformed(
            &windowed(10, &[reference.clone(), raw_literals(1025)]),
            "a compressed block storing more than its window after a compressed one",
        );
        assert_malformed(
            &windowed(10, &[reference.clone(), raw(true, &[1; 1025])]),
            "a raw block over its window after a compressed one",
        );
        assert_malformed(
            &windowed(10, &[reference, block(true, 1, 1025, b"x")]),
            "an RLE block over its window after a compressed one",
        );
    }

    /// THE KNOWN GAP, pinned so that it is a decision and not a surprise.
    ///
    /// libzstd refuses this frame ("Data corruption detected", Zstandard CLI v1.5.5): its
    /// second block is a compressed block regenerating 1 100 bytes under a 1 024-byte window.
    /// This module accepts it. `ruzstd` reports how much is collectable, not how much a block
    /// produced: after the first block (600 bytes, under the window) nothing is collectable, so
    /// the buffer's length is unknown, and after the second only `1 700 - 1 024` is, which is
    /// what a legal 676-byte block after a full window would also show.
    ///
    /// The output is still exactly what the blocks encode; what is lost is the refusal of a
    /// non-conforming frame. If this test goes red, `ruzstd` or this module got stricter:
    /// delete it and the paragraph in the module documentation.
    #[test]
    fn zstd_known_gap_a_compressed_block_over_its_maximum_under_an_unfilled_window() {
        let reference = block(false, 2, REFERENCE_BLOCK.len(), &REFERENCE_BLOCK);
        let expected = [&reference_block_plaintext()[..], &[b'A'; 1100]].concat();
        assert_eq!(
            decode(&windowed(10, &[reference, rle_literals(true, 1100)]))
                .expect("accepted, where libzstd refuses"),
            expected
        );
        // The same second block is refused wherever its size is determinable.
        assert_malformed(
            &windowed(
                10,
                &[
                    raw(false, &reference_block_plaintext()),
                    rle_literals(true, 1100),
                ],
            ),
            "the same block after a raw one",
        );
    }

    #[test]
    fn zstd_refuses_what_the_frame_format_reserves() {
        let mut frame = single_segment(b"abcd");
        frame[4] |= 8;
        assert_malformed(&frame, "the reserved frame-descriptor bit");
        assert_malformed(
            &windowed(10, &[block(true, 3, 0, b"")]),
            "the reserved block type",
        );
        // A dictionary this decoder was never given. Dictionary ID 0 means "none".
        let mut frame = MAGIC.to_vec();
        frame.extend_from_slice(&[0x01, 0x00, 0x05]);
        frame.extend(raw(true, b"abcd"));
        assert_malformed(&frame, "a frame naming a dictionary");
        frame[6] = 0;
        assert_eq!(decode(&frame).expect("dictionary ID 0"), b"abcd");
    }

    /// A caller that stops reading when its buffer is full still gets the frame's verdict.
    #[test]
    fn zstd_last_bytes_carry_the_frames_verdict() {
        let mut frame = MAGIC.to_vec();
        frame.extend_from_slice(&[0x04, 0x00]);
        frame.extend(raw(true, b"abcd"));
        frame.extend_from_slice(&0xd25d_92cd_u32.to_le_bytes());
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&frame, &arena, &DecodePolicy::service()).expect("root");
        let mut decoder = ctx.open_zstd(root).expect("decoder");
        let mut output = [0; 4];
        assert!(matches!(
            decoder.read_chunk(&mut output),
            Err(CodecError::Malformed(_))
        ));
    }
}
