// SPDX-License-Identifier: Apache-2.0
//! Compression helpers shared by container codecs.

use std::io::Read;

use cadmpeg_core::decode::{DecodeContext, ExpandSpec, View};
use cadmpeg_core::CodecError;
use flate2::bufread::ZlibDecoder as BufferedZlibDecoder;
use flate2::read::{DeflateDecoder, ZlibDecoder};

/// Inflates exactly one zlib member and rejects truncation or trailing input.
pub fn inflate_zlib_exact<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'_>,
) -> Result<View<'a>, CodecError> {
    let mut decoder = BufferedZlibDecoder::new(std::io::Cursor::new(source.window()));
    let mut writer = ctx.begin_expand(source, ExpandSpec::Unknown)?;
    let mut chunk = [0_u8; 8192];
    loop {
        let read = decoder
            .read(&mut chunk)
            .map_err(|error| CodecError::Malformed(format!("invalid zlib member: {error}")))?;
        if read == 0 {
            break;
        }
        writer.write(&chunk[..read])?;
    }
    if decoder.total_in() != source.window().len() as u64 {
        return Err(CodecError::Malformed(
            "zlib member does not exhaust its declared input".into(),
        ));
    }
    writer.finalize()
}

/// Inflates a zlib member under the decode budget, retaining a nonempty prefix
/// when the compressed stream is truncated or followed by another stream.
pub fn inflate_zlib_prefix<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'_>,
) -> Result<Option<View<'a>>, CodecError> {
    inflate_prefix(ctx, source, |bytes| Box::new(ZlibDecoder::new(bytes)))
}

/// Inflates a raw-DEFLATE member under the decode budget, retaining a nonempty
/// prefix when the compressed stream is truncated or followed by another stream.
pub fn inflate_raw_prefix<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'_>,
) -> Result<Option<View<'a>>, CodecError> {
    inflate_prefix(ctx, source, |bytes| Box::new(DeflateDecoder::new(bytes)))
}

fn inflate_prefix<'a, 'input>(
    ctx: &DecodeContext<'a>,
    source: View<'input>,
    decoder: impl FnOnce(&'input [u8]) -> Box<dyn Read + 'input>,
) -> Result<Option<View<'a>>, CodecError> {
    let mut decoder = decoder(source.window());
    let mut writer = ctx.begin_expand(source, ExpandSpec::Unknown)?;
    let mut chunk = [0_u8; 8192];
    loop {
        match decoder.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => writer.write(&chunk[..read])?,
            Err(_) if writer.written() != 0 => break,
            Err(_) => return Ok(None),
        }
    }
    if writer.written() == 0 {
        Ok(None)
    } else {
        writer.finalize().map(Some)
    }
}

/// Inflates at most `cap` raw-DEFLATE output bytes for format detection.
///
/// This helper is context-free because detection runs before a decode session
/// exists. Output beyond the cap is discarded and reported as failure.
pub fn inflate_bounded_probe(bytes: &[u8], cap: usize) -> Option<Vec<u8>> {
    let mut decoder = DeflateDecoder::new(bytes);
    let mut output = Vec::new();
    let mut chunk = [0_u8; 1024];
    loop {
        let read = decoder.read(&mut chunk).ok()?;
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

    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
    use flate2::{write::DeflateEncoder, write::ZlibEncoder, Compression};

    use super::*;

    #[test]
    fn inflates_complete_member_with_trailing_bytes() {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(b"parasolid")
            .expect("writing to an in-memory zlib encoder succeeds");
        let mut compressed = encoder
            .finish()
            .expect("finishing an in-memory zlib encoder succeeds");
        compressed.extend_from_slice(b"next stream");
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&compressed, &arena, &DecodePolicy::default())
                .expect("test input fits the root allowance");
        assert_eq!(
            inflate_zlib_prefix(&ctx, root)
                .expect("test output fits the expansion allowance")
                .map(View::window),
            Some(b"parasolid".as_slice())
        );
    }

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
}
