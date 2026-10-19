// SPDX-License-Identifier: Apache-2.0
//! Window-admitted Zstandard streams.

use super::{DecodeContext, View};
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
    /// Inspects all frame windows before allocating a decoder. The output
    /// each frame's decoder buffers, its window or its declared content size
    /// when that is smaller, must fit the per-expansion decompression
    /// allowance, and the declared window must fit the decoder's. Either
    /// excess is [`CodecError::NotImplemented`] for this stream and leaves the
    /// session open, since nothing was allocated.
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
            let FrameWindow { declared, buffered } = frame_window(frame)?;
            if buffered > allowance {
                return Err(CodecError::NotImplemented(format!(
                    "Zstandard window of {buffered} bytes exceeds the \
                     {allowance}-byte per-expansion allowance"
                )));
            }
            if declared > MAX_DECODER_WINDOW {
                return Err(window_unsupported(declared));
            }
            largest_window = largest_window.max(declared);
            let size = zstd_safe::find_frame_compressed_size(frame.window())
                .map_err(|code| zstd_error(self, code, declared))?;
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
        // decoder's format minimum of 1 KiB. Each frame's buffered output was
        // checked against the exact allowance above before allocation.
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

/// One frame's declared window and the output its decoder buffers: the
/// window, or the declared content size when that is smaller.
struct FrameWindow {
    declared: u64,
    buffered: u64,
}

fn frame_window(mut frame: View<'_>) -> Result<FrameWindow, CodecError> {
    let magic = frame.req_u32_le()?;
    if (0x184d_2a50..=0x184d_2a5f).contains(&magic) {
        frame.req_u32_le()?;
        return Ok(FrameWindow {
            declared: 0,
            buffered: 0,
        });
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
        None
    } else {
        let descriptor = frame.req_u8()?;
        let base = 1_u64 << (10 + u32::from(descriptor >> 3));
        Some(base + (base / 8) * u64::from(descriptor & 7))
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
    let declared = window.or(content_size).unwrap_or(0);
    Ok(FrameWindow {
        declared,
        buffered: content_size.map_or(declared, |size| size.min(declared)),
    })
}

#[cfg(test)]
mod tests {
    use super::MAX_DECODER_WINDOW;
    use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ExpandSpec};
    use crate::CodecError;

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
        let Err(CodecError::NotImplemented(message)) = ctx.open_zstd(root) else {
            panic!("a window above the allowance is unsupported");
        };
        assert_eq!(
            message,
            format!(
                "Zstandard window of {window} bytes exceeds the \
                 {allowance}-byte per-expansion allowance"
            )
        );
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
            matches!(ctx.open_zstd(root), Err(CodecError::NotImplemented(message)) if message.starts_with(&format!("Zstandard window of {} bytes", 1_u64 << 30)) && message.ends_with("per-expansion allowance"))
        );
        assert_eq!(ctx.resource_refusal(), None);
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
