// SPDX-License-Identifier: Apache-2.0
//! Compression helpers shared by container codecs.

use std::io::Read;

use cadmpeg_core::decode::{DecodeContext, ExpandSpec, View};
use cadmpeg_core::CodecError;
use flate2::read::{DeflateDecoder, ZlibDecoder};
use flate2::{Decompress, FlushDecompress, Status};

const INFLATE_CHUNK: usize = 8192;
const PROBE_CHUNK: usize = 1024;

/// Inflates one zlib member that starts at `source`.
///
/// `source` may extend past the member. The walk stops at zlib `StreamEnd` and
/// returns the expanded view plus the compressed byte count consumed.
/// `spec` is charged through [`DecodeContext::begin_expand`].
pub fn inflate_zlib_member<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'_>,
    spec: ExpandSpec,
) -> Result<(View<'a>, usize), CodecError> {
    let input = source.window();
    let mut decoder = Decompress::new(true);
    let mut writer = ctx.begin_expand(source, spec)?;
    let mut chunk = [0_u8; INFLATE_CHUNK];
    let mut source_offset = 0usize;
    loop {
        let before_in = decoder.total_in();
        let before_out = decoder.total_out();
        let status = decoder
            .decompress(&input[source_offset..], &mut chunk, FlushDecompress::None)
            .map_err(|error| CodecError::malformed(format_args!("invalid zlib member: {error}")))?;
        let consumed = usize::try_from(decoder.total_in() - before_in)
            .map_err(|_| CodecError::Malformed("zlib input overflow".into()))?;
        source_offset = source_offset
            .checked_add(consumed)
            .ok_or_else(|| CodecError::Malformed("zlib input overflow".into()))?;
        let produced = usize::try_from(decoder.total_out() - before_out)
            .map_err(|_| CodecError::Malformed("zlib output overflow".into()))?;
        writer.write(&chunk[..produced])?;
        if status == Status::StreamEnd {
            break;
        }
        if consumed == 0 && produced == 0 {
            return Err(CodecError::Malformed("truncated zlib member".into()));
        }
    }
    Ok((writer.finalize()?, source_offset))
}

/// Inflates exactly one zlib member and rejects truncation or trailing input.
pub fn inflate_zlib_exact<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'_>,
) -> Result<View<'a>, CodecError> {
    let (view, consumed) = inflate_zlib_member(ctx, source, ExpandSpec::Unknown)?;
    if consumed != source.window().len() {
        return Err(CodecError::Malformed(
            "zlib member does not exhaust its declared input".into(),
        ));
    }
    Ok(view)
}

/// Inflates a raw-DEFLATE member that occupies all of `source`.
///
/// `spec` is charged through [`DecodeContext::begin_expand`].
pub fn inflate_deflate<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'_>,
    spec: ExpandSpec,
) -> Result<View<'a>, CodecError> {
    let mut decoder = DeflateDecoder::new(source.window());
    let mut writer = ctx.begin_expand(source, spec)?;
    let mut chunk = [0_u8; INFLATE_CHUNK];
    loop {
        let read = decoder.read(&mut chunk).map_err(|error| {
            CodecError::malformed(format_args!("invalid raw-DEFLATE member: {error}"))
        })?;
        if read == 0 {
            break;
        }
        writer.write(&chunk[..read])?;
    }
    writer.finalize()
}

/// Inflates at most `cap` raw-DEFLATE output bytes for format detection.
///
/// This helper is context-free because detection runs before a decode session
/// exists. Output beyond the cap is discarded and reported as failure.
pub fn inflate_bounded_probe(bytes: &[u8], cap: usize) -> Option<Vec<u8>> {
    let mut decoder = DeflateDecoder::new(bytes);
    probe_decoder(|chunk| decoder.read(chunk).ok(), cap)
}

/// Inflates at most `cap` zlib output bytes without a decode session.
///
/// The walk stops at stream end. Leftover input after the member is allowed.
/// Output beyond the cap, or any decode error, is reported as failure.
pub fn inflate_zlib_probe(bytes: &[u8], cap: usize) -> Option<Vec<u8>> {
    let mut decoder = ZlibDecoder::new(bytes);
    probe_decoder(|chunk| decoder.read(chunk).ok(), cap)
}

fn probe_decoder(
    mut read_chunk: impl FnMut(&mut [u8]) -> Option<usize>,
    cap: usize,
) -> Option<Vec<u8>> {
    let mut output = Vec::new();
    let mut chunk = [0_u8; PROBE_CHUNK];
    loop {
        let read = read_chunk(&mut chunk)?;
        if read == 0 {
            return Some(output);
        }
        if read > cap.saturating_sub(output.len()) {
            return None;
        }
        output.try_reserve(read).ok()?;
        output.extend_from_slice(&chunk[..read]);
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ExpandSpec};
    use flate2::{write::DeflateEncoder, write::ZlibEncoder, Compression};

    use super::*;

    #[test]
    fn exact_inflate_rejects_trailing_bytes() {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"one member").expect("write test member");
        let mut compressed = encoder.finish().expect("finish test member");
        compressed.extend_from_slice(b"suffix");
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&compressed, &arena, &DecodePolicy::default())
                .expect("test input fits the root allowance");
        assert!(inflate_zlib_exact(&ctx, root).is_err());
    }

    #[test]
    fn exact_inflate_accepts_one_complete_member() {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"one member").expect("write test member");
        let compressed = encoder.finish().expect("finish test member");
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&compressed, &arena, &DecodePolicy::default())
                .expect("test input fits the root allowance");
        assert_eq!(
            inflate_zlib_exact(&ctx, root)
                .expect("complete member inflates")
                .window(),
            b"one member"
        );
    }

    #[test]
    fn zlib_member_reports_consumed_and_ignores_trailing_input() {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"one member").expect("write test member");
        let member = encoder.finish().expect("finish test member");
        let member_len = member.len();
        let mut compressed = member;
        compressed.extend_from_slice(b"suffix");
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&compressed, &arena, &DecodePolicy::default())
                .expect("test input fits the root allowance");
        let (view, consumed) = inflate_zlib_member(&ctx, root, ExpandSpec::Unknown)
            .expect("member inflates from a longer view");
        assert_eq!(view.window(), b"one member");
        assert_eq!(consumed, member_len);
    }

    #[test]
    fn deflate_inflate_accepts_declared_output() {
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(b"Document.xml")
            .expect("writing to an in-memory deflate encoder succeeds");
        let compressed = encoder
            .finish()
            .expect("finishing an in-memory deflate encoder succeeds");
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&compressed, &arena, &DecodePolicy::default())
                .expect("test input fits the root allowance");
        assert_eq!(
            inflate_deflate(&ctx, root, ExpandSpec::Exact(12))
                .expect("declared raw-DEFLATE member inflates")
                .window(),
            b"Document.xml"
        );
    }

    #[test]
    fn bounded_probe_refuses_output_past_cap() {
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(b"Document.xml")
            .expect("writing to an in-memory deflate encoder succeeds");
        let compressed = encoder
            .finish()
            .expect("finishing an in-memory deflate encoder succeeds");
        assert_eq!(
            inflate_bounded_probe(&compressed, 12).as_deref(),
            Some(b"Document.xml".as_slice())
        );
        assert!(inflate_bounded_probe(&compressed, 11).is_none());
    }

    #[test]
    fn zlib_probe_refuses_output_past_cap() {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(b"Document.xml")
            .expect("writing to an in-memory zlib encoder succeeds");
        let compressed = encoder
            .finish()
            .expect("finishing an in-memory zlib encoder succeeds");
        assert_eq!(
            inflate_zlib_probe(&compressed, 12).as_deref(),
            Some(b"Document.xml".as_slice())
        );
        assert!(inflate_zlib_probe(&compressed, 11).is_none());
    }
}
