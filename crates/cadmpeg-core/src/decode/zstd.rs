// SPDX-License-Identifier: Apache-2.0
//! Window-admitted Zstandard streams.

use super::{DecodeContext, ResourceDimension, ResourceFailure, View};
use crate::CodecError;
use zstd::zstd_safe::{self, DCtx, DParameter, InBuffer, OutBuffer};

/// The largest window the linked decoder can open on this target. A larger
/// frame is outside the reader, not over a session ceiling.
const MAX_DECODER_WINDOW: u64 = 1_u64 << if usize::BITS == 32 { 30 } else { 31 };

/// One Zstandard decoder over an admitted input.
pub struct ZstdDecoder<'ctx, 'input> {
    decoder: DCtx<'static>,
    ctx: &'ctx DecodeContext<'input>,
    input: InBuffer<'input>,
    finished: bool,
    window_bytes: u64,
}

impl<'input> DecodeContext<'input> {
    /// Inspects all frame windows before allocating a decoder. Each frame
    /// window must fit the per-expansion decompression allowance; a window
    /// that fits it but not the decoder is
    /// [`CodecError::NotImplemented`] and leaves the session open.
    pub fn open_zstd<'ctx>(
        &'ctx self,
        source: View<'input>,
    ) -> Result<ZstdDecoder<'ctx, 'input>, CodecError> {
        let mut offset = 0;
        let mut largest_window = 0_u64;
        let allowance = self.per_expand_allowance();
        while offset < source.window().len() {
            let frame = source
                .child(source.start() + offset, source.end())
                .ok_or_else(|| {
                    CodecError::Malformed("Zstandard frame offset escapes input".into())
                })?;
            let window = frame_window(frame)?;
            if window > allowance {
                return Err(self.budget.refuse(
                    ResourceDimension::DecompressedBytes,
                    ResourceFailure::BudgetExceeded,
                    allowance,
                    0,
                    window,
                    "Zstandard window",
                ));
            }
            if window > MAX_DECODER_WINDOW {
                return Err(window_unsupported(window));
            }
            largest_window = largest_window.max(window);
            let size = zstd_safe::find_frame_compressed_size(frame.window())
                .map_err(|code| zstd_error(self, code, window))?;
            if size == 0 || size > frame.window().len() {
                return Err(CodecError::Malformed("invalid Zstandard frame size".into()));
            }
            offset += size;
        }
        if offset == 0 {
            return Err(CodecError::Malformed("empty Zstandard stream".into()));
        }
        let mut decoder = DCtx::try_create().ok_or_else(|| {
            self.budget
                .retained_allocation_failed(largest_window, "Zstandard context")
        })?;
        // Round the inspected maximum up to the next permitted log, with the
        // decoder's format minimum of 1 KiB. Each actual frame was checked
        // against the exact allowance above before allocation.
        let minimum_window = largest_window.max(1024);
        let window_log = u64::BITS - (minimum_window - 1).leading_zeros();
        decoder
            .set_parameter(DParameter::WindowLogMax(window_log))
            .map_err(|code| zstd_error(self, code, largest_window))?;
        Ok(ZstdDecoder {
            decoder,
            ctx: self,
            input: InBuffer {
                src: source.window(),
                pos: 0,
            },
            finished: false,
            window_bytes: largest_window,
        })
    }
}

impl ZstdDecoder<'_, '_> {
    /// Reads decoded output into caller-owned storage.
    pub fn read_chunk(&mut self, bytes: &mut [u8]) -> Result<usize, CodecError> {
        if self.finished || bytes.is_empty() {
            return Ok(0);
        }
        loop {
            let before = self.input.pos;
            let mut chunk = [0_u8; 16 * 1024];
            let capacity = bytes.len().min(chunk.len());
            let mut output = OutBuffer::around(&mut chunk[..capacity]);
            let remaining = self
                .decoder
                .decompress_stream(&mut output, &mut self.input)
                .map_err(|code| zstd_error(self.ctx, code, self.window_bytes))?;
            let produced = output.pos();

            bytes[..produced].copy_from_slice(&chunk[..produced]);
            if remaining == 0 && self.input.pos == self.input.src.len() {
                self.finished = true;
            }
            if produced != 0 || self.finished {
                return Ok(produced);
            }
            if before == self.input.pos {
                return Err(CodecError::Malformed("truncated Zstandard stream".into()));
            }
        }
    }
}

fn window_unsupported(window: u64) -> CodecError {
    CodecError::NotImplemented(format!(
        "Zstandard window of {window} bytes exceeds the {MAX_DECODER_WINDOW}-byte decoder window"
    ))
}

fn zstd_error(ctx: &DecodeContext<'_>, code: usize, window: u64) -> CodecError {
    // SAFETY: the code is the error result returned by the same zstd library.
    let error = unsafe { zstd_safe::zstd_sys::ZSTD_getErrorCode(code) };
    match error {
        zstd_safe::zstd_sys::ZSTD_ErrorCode::ZSTD_error_memory_allocation => ctx
            .budget
            .retained_allocation_failed(window, "Zstandard decoder allocation"),
        zstd_safe::zstd_sys::ZSTD_ErrorCode::ZSTD_error_frameParameter_windowTooLarge => {
            window_unsupported(window)
        }
        _ => CodecError::malformed(format_args!(
            "invalid Zstandard stream: {}",
            zstd_safe::get_error_name(code)
        )),
    }
}

fn frame_window(mut frame: View<'_>) -> Result<u64, CodecError> {
    let magic = frame.req_u32_le()?;
    if (0x184d_2a50..=0x184d_2a5f).contains(&magic) {
        frame.req_u32_le()?;
        return Ok(0);
    }
    if magic != 0xfd2f_b528 {
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
        0 if single_segment => u64::from(frame.req_u8()?),
        0 => 0,
        1 => u64::from(frame.req_u16_le()?) + 256,
        2 => u64::from(frame.req_u32_le()?),
        _ => frame.req_u64_le()?,
    };
    Ok(if single_segment { content_size } else { window })
}

#[cfg(test)]
mod tests {
    use super::MAX_DECODER_WINDOW;
    use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use crate::CodecError;

    /// A frame header with a non-single-segment window of `window` bytes,
    /// followed by one empty raw block.
    fn frame_with_window(window: u64) -> Vec<u8> {
        let exponent = u8::try_from(window.trailing_zeros() - 10).expect("window exponent");
        vec![0x28, 0xb5, 0x2f, 0xfd, 0, exponent << 3, 1, 0, 0]
    }

    #[test]
    fn zstd_window_refuses_against_the_per_expand_allowance() {
        let window = 32 << 20;
        let bytes = frame_with_window(window);
        let policy = DecodePolicy::service();
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let allowance = ctx.per_expand_allowance();
        assert!(allowance < window);
        assert!(window < policy.limits.max_decompressed_bytes_per_expand);
        let Err(CodecError::ResourceLimit(limit)) = ctx.open_zstd(root) else {
            panic!("a window above the allowance refuses");
        };
        assert_eq!(limit.dimension, ResourceDimension::DecompressedBytes);
        assert_eq!(limit.limit, allowance);
        assert_eq!(limit.additional, window);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(fused)) if fused == limit)
        );
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
        assert!(matches!(
            ctx.open_zstd(root),
            Err(CodecError::NotImplemented(message)) if message.contains("decoder window")
        ));
        assert_eq!(ctx.resource_refusal(), None);
        assert!(ctx.finish_session().is_ok());
    }

    #[test]
    fn zstd_checks_each_window_before_decoder_allocation() {
        let mut bytes = zstd::bulk::compress(b"prefix", 0).expect("frame");
        bytes.extend_from_slice(&[0x28, 0xb5, 0x2f, 0xfd, 0, 0xa0, 1, 0, 0]);
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
        assert!(
            matches!(ctx.open_zstd(root), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::DecompressedBytes && limit.operation == "Zstandard window" && limit.additional == 1 << 30)
        );
    }

    #[test]
    fn zstd_decodes_a_frame() {
        let bytes = zstd::bulk::compress(b"payload", 0).expect("frame");
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
        let mut decoder = ctx.open_zstd(root).expect("decoder");
        let mut output = [0; 7];
        assert_eq!(decoder.read_chunk(&mut output).expect("read"), 7);
        assert_eq!(&output, b"payload");
    }
}
