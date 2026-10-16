// SPDX-License-Identifier: Apache-2.0
//! Window-admitted Zstandard streams with scoped decoder workspace.
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
//! Two things `ruzstd` does not reproduce, stated rather than hidden:
//!
//! 1. **A compressed block's LITERALS are not held to `Block_Maximum_Size`.** `ruzstd` 0.9.1
//!    refuses a block whose sequences regenerate past the maximum, as it executes them: a
//!    227-byte frame of a hundred maximal matches is refused with 0.27 MB of heap, where
//!    0.9.0 built its 13 MiB first (29.9 MB; both measured, 2026-10-13). That is why the
//!    requirement is `0.9.1` and not `0.9`. What it still does not check is the literals: a
//!    block with no sequences, and the literals after a block's last sequence. A literals
//!    section states at most 2^20 - 1 bytes, so the excess is bounded by 1 MiB a block, is
//!    ALLOCATED before anything can refuse it, and is refused afterwards by this module
//!    whenever the block's size is determinable (see `decode_block`). It is not determinable
//!    for a compressed block decoded while the buffer is still under its window and its
//!    length unknown; the test `zstd_known_gap_...` pins the frame that gets through. The
//!    workspace reservation therefore bounds conforming frames only.
//! 2. **The decoder's storage is not `window + 3 blocks`.** libzstd allocates the declared
//!    window when a frame opens. `ruzstd`'s ring buffer instead grows with what the frame
//!    regenerates, and holds its predecessor while it copies; see `workspace_bytes`. The
//!    reservation is that figure and not the libzstd decoder's: about two windows and three blocks for
//!    a frame that fills its window, and a few bytes for a short frame that declares a
//!    large one.

use std::io::Read as _;

use ruzstd::decoding::errors::FrameDecoderError;
use ruzstd::decoding::{BlockDecodingStrategy, FrameDecoder};

use super::{
    u64_from_index, DecodeContext, ResourceDimension, ResourceFailure, ScopedReservation, View,
};
use crate::CodecError;

const BLOCK_BYTES: u64 = 128 * 1024;
/// Bound on `ruzstd`'s per-decoder scratch outside the ring buffer, for conforming blocks.
///
/// A conforming block has at most 128 KiB of content, regenerates at most 128 KiB of
/// literals, and holds at most 43 690 twelve-byte sequences (each regenerates three bytes
/// or more). Each of the three lives in a `Vec` that may hold twice its length and coexist
/// with its half-sized predecessor while growing: 384 KiB + 384 KiB + 1 536 KiB. Entropy
/// tables are under 64 KiB. Read from `scratch.rs` and its users, ruzstd 0.9.0.
const FIXED_STATE_BYTES: u64 = 3 * 1024 * 1024;
const MAX_DECODER_WINDOW: u64 = 1_u64 << if usize::BITS == 32 { 30 } else { 31 };
const OUTPUT_STEP_BYTES: usize = 16 * 1024;
const FRAME_MAGIC: u32 = 0xfd2f_b528;

/// One decoder and its live workspace reservation. The decoder is dropped
/// before the reservation that admits its allocation.
pub struct ZstdDecoder<'ctx, 'input> {
    frame: Option<ActiveFrame<'input>>,
    _workspace: ScopedReservation<'ctx>,
    ctx: &'ctx DecodeContext<'input>,
    /// Frames not yet opened. Every byte was framed by `open_zstd`.
    input: View<'input>,
    finished: bool,
    /// What `_workspace` reserved. The libzstd decoder reported it in allocator refusals, which
    /// `ruzstd` has none of (it aborts on a failed allocation), so only the tests read it.
    #[cfg(test)]
    workspace_bytes: u64,
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
    /// Most bytes a conforming decode of this frame holds in `ruzstd`'s ring buffer.
    ring_bytes: u64,
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
    /// Inspects all frame windows before allocating a decoder. The largest
    /// frame window must fit the materialized-byte and per-expansion ceilings.
    pub fn open_zstd<'ctx>(
        &'ctx self,
        source: View<'input>,
    ) -> Result<ZstdDecoder<'ctx, 'input>, CodecError> {
        self.charge_work(
            u64_from_index(source.window().len()),
            "inspect Zstandard frames",
        )?;
        self.charge_work(
            u64_from_index(source.window().len()),
            "Zstandard compressed input",
        )?;
        let mut offset = 0;
        let mut largest_ring = 0_u64;
        let ceiling = window_ceiling(self);
        while offset < source.window().len() {
            self.charge_work(1, "inspect Zstandard frame header")?;
            let frame = source
                .child(source.start() + offset, source.end())
                .ok_or_else(|| {
                    CodecError::Malformed("Zstandard frame offset escapes input".into())
                })?;
            let window = frame_header(frame)?.1.map_or(0, |facts| facts.window);
            if window > ceiling {
                return Err(self.budget.refuse(
                    ResourceDimension::MaterializedBytes,
                    ResourceFailure::BudgetExceeded,
                    ceiling,
                    0,
                    window,
                    "Zstandard window",
                ));
            }
            let Inspected {
                size, ring_bytes, ..
            } = inspect_frame(frame)?;
            largest_ring = largest_ring.max(ring_bytes);
            if size == 0 || size > frame.window().len() {
                return Err(CodecError::Malformed("invalid Zstandard frame size".into()));
            }
            offset += size;
        }
        if offset == 0 {
            return Err(CodecError::Malformed("empty Zstandard stream".into()));
        }
        let workspace_bytes = workspace_bytes(largest_ring).ok_or_else(|| {
            CodecError::from(
                self.budget
                    .scoped_size_overflow_limit("Zstandard workspace"),
            )
        })?;
        let workspace = self.reserve_scoped(workspace_bytes, "Zstandard workspace")?;
        // No decoder exists yet: `ruzstd` allocates per frame, on `FrameDecoder::init`, and
        // each frame is opened under this one reservation with its own inspected window as
        // the decoder's limit.
        Ok(ZstdDecoder {
            frame: None,
            _workspace: workspace,
            ctx: self,
            input: source
                .child(source.start(), source.end())
                .ok_or_else(|| CodecError::Malformed("Zstandard input escapes its view".into()))?,
            finished: false,
            #[cfg(test)]
            workspace_bytes,
        })
    }
}

impl ZstdDecoder<'_, '_> {
    /// Reads admitted output into caller-owned storage and charges step/copy work.
    pub fn read_chunk(&mut self, bytes: &mut [u8]) -> Result<usize, CodecError> {
        if self.finished || bytes.is_empty() {
            return Ok(0);
        }
        let capacity = bytes.len().min(OUTPUT_STEP_BYTES);
        // One step is at most: open a frame, decode one block, collect one chunk, close the
        // frame. It is charged as the libzstd decoder charged one `ZSTD_decompressStream` call.
        loop {
            self.ctx
                .charge_work(u64_from_index(bytes.len()), "Zstandard decode step")?;
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
                frame.decode_block(self.ctx)?;
            }
            let produced = frame
                .decoder
                .read(&mut bytes[..capacity])
                .map_err(|error| malformed(&error))?;
            self.ctx
                .charge_work(u64_from_index(produced), "Zstandard output copy")?;
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
            .map_err(|error| zstd_error(self.ctx, &error, facts.window))?;
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
    fn decode_block(&mut self, ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
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
            .map_err(|error| zstd_error(ctx, &error, self.window))?;
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
            if length.saturating_sub(before) > self.block_maximum {
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

fn window_ceiling(ctx: &DecodeContext<'_>) -> u64 {
    ctx.policy()
        .limits
        .max_materialized_bytes
        .min(ctx.policy().limits.max_decompressed_bytes_per_expand)
        .min(MAX_DECODER_WINDOW)
}

/// Peak bytes one `ruzstd` frame decoder allocates while its ring buffer holds `ring_bytes`.
///
/// The ring buffer is asked for one byte more than it must hold. Up to two blocks it grows
/// to the next power of two and a sentinel; past that, to the next power of two of the excess
/// over two blocks, plus the two blocks and the sentinel. It copies from its predecessor
/// before freeing it, and the predecessor was too small for what is held now. Read from
/// `ringbuffer.rs` `reserve_amortized`, ruzstd 0.9.1 -- 0.9.0 doubled outright, and that
/// formula is 1 KiB short of 0.9.1's peak for a 256 KiB window. Checked against the
/// allocator, 2026-10-13: 63 frames, windows 1 KiB to 8 MiB, each read as `read_chunk`
/// reads; the heap peaked at least 56 KiB under this figure from 64 KiB windows up.
fn workspace_bytes(ring_bytes: u64) -> Option<u64> {
    const SLACK: u64 = 2 * BLOCK_BYTES;
    let required = ring_bytes.checked_add(1)?;
    let capacity = if required <= SLACK {
        required.checked_next_power_of_two()?.checked_add(1)?
    } else {
        (required - SLACK)
            .checked_next_power_of_two()?
            .checked_add(SLACK + 1)?
    };
    capacity
        .checked_add(ring_bytes)?
        .checked_add(FIXED_STATE_BYTES)
}

/// Most bytes `ruzstd`'s ring buffer holds while decoding a conforming frame.
///
/// It keeps one window and the block just decoded, since `read_chunk` collects everything
/// past the window before decoding another block; and it can never hold more than the frame
/// regenerates, which is bounded by its declared content size and by its blocks.
fn ring_bytes(facts: FrameFacts, regenerated: u64) -> u64 {
    let held = facts
        .window
        .saturating_add(facts.window.min(BLOCK_BYTES))
        .min(regenerated);
    facts
        .content_size
        .map_or(held, |declared| held.min(declared))
}

fn malformed(error: &dyn std::fmt::Display) -> CodecError {
    CodecError::malformed(format_args!("invalid Zstandard stream: {error}"))
}

fn zstd_error(ctx: &DecodeContext<'_>, error: &FrameDecoderError, window: u64) -> CodecError {
    match error {
        FrameDecoderError::WindowSizeTooBig { .. } => ctx.budget.refuse(
            ResourceDimension::MaterializedBytes,
            ResourceFailure::BudgetExceeded,
            window_ceiling(ctx),
            0,
            window,
            "Zstandard decoder window",
        ),
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
            ring_bytes: 0,
            data: None,
        });
    };
    let block_maximum = facts.window.min(BLOCK_BYTES);
    // What the blocks can regenerate: a raw or RLE block says, a compressed one is held to
    // the frame's block maximum.
    let mut regenerated = 0_u64;
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
        regenerated = regenerated.saturating_add(if kind == 2 {
            block_maximum
        } else {
            u64::from(declared)
        });
        if header & 1 != 0 {
            break;
        }
    }
    if checksum {
        body.skip(4).ok_or_else(truncated)?;
    }
    Ok(Inspected {
        size: body.read_len() - start,
        ring_bytes: ring_bytes(facts, regenerated),
        data: Some(facts),
    })
}

#[cfg(test)]
mod tests {
    use super::{workspace_bytes, BLOCK_BYTES, FIXED_STATE_BYTES};
    use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
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

    #[test]
    fn zstd_checks_each_window_before_decoder_allocation() {
        let mut bytes = single_segment(b"prefix");
        bytes.extend_from_slice(&[0x28, 0xb5, 0x2f, 0xfd, 0, 0xa0, 1, 0, 0]);
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
        assert!(
            matches!(ctx.open_zstd(root), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "Zstandard window" && limit.additional == 1 << 30)
        );
    }

    #[test]
    fn zstd_workspace_stays_reserved_until_decoder_drop() {
        let bytes = single_segment(b"payload");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Seven bytes held: a ring buffer of next_power_of_two(7 + 1) + 1 beside a
        // predecessor of at most seven.
        policy.limits.max_materialized_bytes = 9 + 7 + FIXED_STATE_BYTES;
        assert_eq!(
            workspace_bytes(7),
            Some(policy.limits.max_materialized_bytes)
        );
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let mut decoder = ctx.open_zstd(root).expect("decoder");
        assert_eq!(
            decoder.workspace_bytes,
            policy.limits.max_materialized_bytes
        );
        let mut output = [0; 7];
        assert_eq!(decoder.read_chunk(&mut output).expect("read"), 7);
        assert_eq!(&output, b"payload");
        drop(decoder);
        ctx.reserve_scoped(policy.limits.max_materialized_bytes, "reuse workspace")
            .expect("released workspace");
    }

    #[test]
    fn zstd_workspace_and_work_limits_remain_typed() {
        let bytes = single_segment(b"payload");
        for dimension in [
            ResourceDimension::MaterializedBytes,
            ResourceDimension::WorkUnits,
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            if dimension == ResourceDimension::MaterializedBytes {
                policy.limits.max_materialized_bytes = 0;
            } else {
                policy.limits.max_work_units = 0;
            }
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
            assert!(
                matches!(ctx.open_zstd(root), Err(CodecError::ResourceLimit(limit)) if limit.dimension == dimension)
            );
        }
    }

    /// The workspace is what `ruzstd` allocates for the frame, and its refusal is typed.
    ///
    /// Twenty 128 KiB blocks under a 1 MiB window: the ring buffer holds a window and a
    /// block, 1 179 648 bytes, in 1 MiB + 256 KiB + 1 beside a predecessor that held less.
    /// The libzstd decoder reserved 1 MiB + 384 KiB + 1 MiB + 64 for the same frame.
    #[test]
    fn zstd_workspace_is_the_ring_buffers_peak_and_its_refusal_is_typed() {
        let block_bytes = usize::try_from(BLOCK_BYTES).expect("block size");
        let mut blocks = vec![block(false, 1, block_bytes, b"x"); 19];
        blocks.push(block(true, 1, block_bytes, b"x"));
        let bytes = windowed(20, &blocks);
        let workspace = workspace_bytes((1 << 20) + BLOCK_BYTES).expect("workspace");
        assert_eq!(
            workspace,
            (1 << 20) + 2 * BLOCK_BYTES + 1 + (1 << 20) + BLOCK_BYTES + FIXED_STATE_BYTES
        );

        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = workspace - 1;
        let result = decode_with(&bytes, &policy);
        assert!(
            matches!(&result, Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "Zstandard workspace" && limit.additional == workspace),
            "{result:?}"
        );
        policy.limits.max_materialized_bytes = workspace;
        assert_eq!(
            decode_with(&bytes, &policy).expect("fits"),
            vec![b'x'; 20 * block_bytes]
        );
    }

    /// A short frame is not charged for a window it declares and `ruzstd` never allocates.
    /// (libzstd does allocate it, and the libzstd decoder refused this frame for its workspace.)
    #[test]
    fn zstd_short_frame_under_a_large_window_reserves_what_it_can_hold() {
        let bytes = windowed(27, &[raw(true, b"abcd")]);
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
        let decoder = ctx.open_zstd(root).expect("decoder");
        assert_eq!(decoder.workspace_bytes, 8 + 1 + 4 + FIXED_STATE_BYTES);
        drop(decoder);
        assert_eq!(decode(&bytes).expect("frame"), b"abcd");
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
