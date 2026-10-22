// SPDX-License-Identifier: Apache-2.0
//! Compression helpers shared by container codecs.

use std::io::Read;

use cadmpeg_core::decode::{DecodeContext, ExpandSpec, ExpandWriter, View};
use cadmpeg_core::CodecError;
use flate2::read::DeflateDecoder;
use flate2::{Decompress, FlushDecompress, Status};

const INFLATE_CHUNK: usize = 8192;

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
    let (writer, consumed) = inflate_zlib_writer(ctx, source, spec)?;
    Ok((writer.finalize()?, consumed))
}

/// Inflates one zlib member into an owned buffer without copying arena output.
pub fn inflate_zlib_member_owned(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    spec: ExpandSpec,
) -> Result<(Vec<u8>, usize), CodecError> {
    let (writer, consumed) = inflate_zlib_writer(ctx, source, spec)?;
    Ok((writer.finalize_owned()?, consumed))
}

/// Inflates one zlib member with its exact compressed source provenance.
/// The derived stream name identifies the canonical parent and consumed extent.
pub fn inflate_zlib_member_tracked<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'_>,
    spec: ExpandSpec,
) -> Result<(View<'a>, usize), CodecError> {
    let (writer, consumed) = inflate_zlib_writer(ctx, source, spec)?;
    let end = source
        .start()
        .checked_add(consumed)
        .ok_or_else(|| CodecError::malformed("zlib compressed source extent overflows"))?;
    let member = source
        .child(source.start(), end)
        .ok_or_else(|| CodecError::malformed("zlib compressed source extent escapes its input"))?;
    let capture = ctx.capture_part_extent(member)?;
    let name = format!(
        "zlib:{}:{}:{}:{}",
        capture.stream().len(),
        capture.stream(),
        capture.offset(),
        capture.byte_len()
    );
    Ok((writer.finalize_from(member, &name)?, consumed))
}

fn inflate_zlib_writer<'ctx, 'a>(
    ctx: &'ctx DecodeContext<'a>,
    source: View<'_>,
    spec: ExpandSpec,
) -> Result<(ExpandWriter<'ctx, 'a>, usize), CodecError> {
    let input = source.window();
    let mut admitted_input = input.len().min(INFLATE_CHUNK);
    let mut decoder = Decompress::new(true);
    let mut writer = ctx.begin_expand(spec)?;
    let mut chunk = [0_u8; INFLATE_CHUNK];
    let mut source_offset = 0usize;
    loop {
        let before_in = decoder.total_in();
        let before_out = decoder.total_out();
        let source_end = source_offset
            .checked_add(INFLATE_CHUNK)
            .ok_or_else(|| CodecError::Malformed("zlib input overflow".into()))?
            .min(input.len());
        if source_end > admitted_input {
            admitted_input = source_end;
        }
        let status = decoder
            .decompress(
                &input[source_offset..source_end],
                &mut chunk,
                FlushDecompress::None,
            )
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
    Ok((writer, source_offset))
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

/// Inflates exactly one Zstandard data frame through the shared bounded decoder.
/// Rejects skippable frames, concatenated frames, truncation and trailing input.
pub fn inflate_zstd_exact<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'a>,
) -> Result<View<'a>, CodecError> {
    inflate_zstd_writer(ctx, source)?.finalize()
}

/// Inflate exactly one Zstandard frame with its actual compressed-source lineage.
pub fn inflate_zstd_exact_tracked<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'a>,
) -> Result<View<'a>, CodecError> {
    let writer = inflate_zstd_writer(ctx, source)?;
    let capture = ctx.capture_part_extent(source)?;
    let name = format!(
        "zstd:{}:{}:{}:{}",
        capture.stream().len(),
        capture.stream(),
        capture.offset(),
        capture.byte_len()
    );
    writer.finalize_from(source, &name)
}

fn inflate_zstd_writer<'ctx, 'a>(
    ctx: &'ctx DecodeContext<'a>,
    source: View<'a>,
) -> Result<ExpandWriter<'ctx, 'a>, CodecError> {
    require_single_zstd_frame(source)?;
    let mut decoder = ctx.open_zstd(source)?;
    let mut writer = ctx.begin_expand(ExpandSpec::Unknown)?;
    let mut chunk = [0_u8; INFLATE_CHUNK];
    loop {
        let count = decoder.read_chunk(&mut chunk)?;
        if count == 0 {
            break;
        }
        writer.write(&chunk[..count])?;
    }
    Ok(writer)
}

// RFC 8878 sections 3.1.1.1 and 3.1.1.2 delimit one frame without decoding it.
// open_zstd owns content, checksum, dictionary and window validation.
fn require_single_zstd_frame(mut source: View<'_>) -> Result<(), CodecError> {
    if source.req_u32_le()? != 0xfd2f_b528 {
        return Err(CodecError::Malformed(
            "expected one Zstandard data frame".into(),
        ));
    }
    let flags = source.req_u8()?;
    let single_segment = flags & 0x20 != 0;
    if !single_segment {
        source.req_u8()?;
    }
    let dictionary_size = match flags & 3 {
        0 => 0,
        1 => 1,
        2 => 2,
        _ => 4,
    };
    source.req_take(dictionary_size)?;
    let content_size_width = match flags >> 6 {
        0 => usize::from(single_segment),
        1 => 2,
        2 => 4,
        _ => 8,
    };
    source.req_take(content_size_width)?;
    loop {
        let low = u32::from(source.req_u8()?);
        let header = low | (u32::from(source.req_u16_le()?) << 8);
        let block_size = match (header >> 1) & 3 {
            0 | 2 => usize::try_from(header >> 3)
                .map_err(|_| CodecError::Malformed("Zstandard block size overflow".into()))?,
            1 => 1,
            _ => {
                return Err(CodecError::Malformed(
                    "reserved Zstandard block type".into(),
                ))
            }
        };
        source.req_take(block_size)?;
        if header & 1 != 0 {
            break;
        }
    }
    if flags & 4 != 0 {
        source.req_u32_le()?;
    }
    if !source.is_empty() {
        return Err(CodecError::Malformed(
            "Zstandard frame does not exhaust its declared input".into(),
        ));
    }
    Ok(())
}

/// Inflates a raw-DEFLATE member that occupies all of `source`.
///
/// `spec` is charged through [`DecodeContext::begin_expand`].
pub fn inflate_deflate<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'_>,
    spec: ExpandSpec,
) -> Result<View<'a>, CodecError> {
    inflate_deflate_writer(ctx, source, spec)?.finalize()
}

/// Inflates a raw-DEFLATE member into an owned buffer without copying arena output.
pub fn inflate_deflate_owned(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    spec: ExpandSpec,
) -> Result<Vec<u8>, CodecError> {
    inflate_deflate_writer(ctx, source, spec)?.finalize_owned()
}

fn inflate_deflate_writer<'ctx, 'a>(
    ctx: &'ctx DecodeContext<'a>,
    source: View<'_>,
    spec: ExpandSpec,
) -> Result<ExpandWriter<'ctx, 'a>, CodecError> {
    let mut decoder = DeflateDecoder::new(source.window());
    let mut writer = ctx.begin_expand(spec)?;
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
    if decoder.total_in() != cadmpeg_core::decode::u64_from_index(source.window().len()) {
        return Err(CodecError::Malformed(
            "raw-DEFLATE member does not exhaust its declared input".into(),
        ));
    }
    Ok(writer)
}

/// Expands one ZIP member stored as Zstandard (method 93) into an owned buffer.
///
/// For a caller that reads members through `zip`: the workspace builds `zip`
/// without its `zstd` feature (a libzstd binding), so `zip` cannot expand
/// method 93 and the member is read as stored, through `by_index_raw`, and
/// expanded here by `cadmpeg-core`'s decoder, the one [`crate::ArchiveSnapshot`]
/// uses. The output is held to `spec` and to the member's `crc32`, the checks
/// `zip` makes on a member it expands itself.
///
/// `stored` is copied into `ctx` first, so the caller's buffer need not be
/// one of `ctx`'s views.
pub fn expand_zstd_member_owned(
    ctx: &DecodeContext<'_>,
    stored: &[u8],
    spec: ExpandSpec,
    crc32: u32,
) -> Result<Vec<u8>, CodecError> {
    let mut source = ctx.begin_expand(ExpandSpec::Exact(cadmpeg_core::decode::u64_from_index(
        stored.len(),
    )))?;
    source.write(stored)?;
    let mut decoder = ctx.open_zstd(source.finalize()?)?;
    let mut writer = ctx.begin_expand(spec)?;
    let mut chunk = [0_u8; INFLATE_CHUNK];
    loop {
        let read = decoder.read_chunk(&mut chunk)?;
        if read == 0 {
            break;
        }
        writer.write(&chunk[..read])?;
    }
    let bytes = writer.finalize_owned()?;
    if crc32fast::hash(&bytes) != crc32 {
        return Err(CodecError::Malformed(
            "Zstandard ZIP member fails its CRC-32".into(),
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use cadmpeg_core::decode::{
        DecodeArena, DecodeContext, DecodePolicy, ExpandSpec, ResourceDimension,
    };
    use cadmpeg_core::CodecError;
    use flate2::{write::DeflateEncoder, write::ZlibEncoder, Compression};

    use super::{
        expand_zstd_member_owned, inflate_deflate, inflate_deflate_owned, inflate_zlib_exact,
        inflate_zlib_member, inflate_zlib_member_owned,
    };

    mod provenance;
    mod zstd;

    #[test]
    fn owned_deflate_expansion_refuses_per_expand_limit() {
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(b"owned output")
            .expect("write test member");
        let compressed = encoder.finish().expect("finish test member");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_decompressed_bytes_per_expand = 11;
        let (ctx, root) =
            DecodeContext::from_root_bytes(&compressed, &arena, &policy).expect("root");
        let error = inflate_deflate_owned(&ctx, root, ExpandSpec::Exact(12))
            .expect_err("the per-expansion limit refuses the owned output");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::DecompressedBytes && limit.limit == 11));

        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&compressed, &arena, &DecodePolicy::service())
                .expect("root");
        assert_eq!(
            inflate_deflate_owned(&ctx, root, ExpandSpec::Exact(12)).expect("owned expansion"),
            b"owned output"
        );
    }

    #[test]
    fn owned_zlib_expansion_refuses_per_expand_limit() {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(b"owned member")
            .expect("write test member");
        let compressed = encoder.finish().expect("finish test member");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_decompressed_bytes_per_expand = 11;
        let (ctx, root) =
            DecodeContext::from_root_bytes(&compressed, &arena, &policy).expect("root");
        let error = inflate_zlib_member_owned(&ctx, root, ExpandSpec::Exact(12))
            .expect_err("the per-expansion limit refuses the owned output");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::DecompressedBytes && limit.limit == 11));

        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&compressed, &arena, &DecodePolicy::service())
                .expect("root");
        let (output, consumed) =
            inflate_zlib_member_owned(&ctx, root, ExpandSpec::Exact(12)).expect("owned expansion");
        assert_eq!(output, b"owned member");
        assert_eq!(consumed, compressed.len());
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
    fn zlib_member_reports_extent_before_a_large_unconsumed_suffix() {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"one member").expect("write member");
        let mut compressed = encoder.finish().expect("finish member");
        let member_len = compressed.len();
        compressed.resize(member_len + 1024 * 1024, 0x55);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&compressed, &arena, &policy).expect("root");
        let (expanded, consumed) = inflate_zlib_member(&ctx, root, ExpandSpec::Unknown)
            .expect("a complete member inflates despite the unrelated suffix");
        assert_eq!(expanded.window(), b"one member");
        assert_eq!(consumed, member_len);
    }

    #[test]
    fn invalid_zlib_candidate_reports_malformed_with_a_large_suffix() {
        let mut compressed = vec![0x78, 0x9c, 0x07];
        compressed.resize(1024 * 1024, 0x55);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&compressed, &arena, &policy).expect("root");
        assert!(matches!(
            inflate_zlib_member(&ctx, root, ExpandSpec::Unknown),
            Err(CodecError::Malformed(_))
        ));
    }

    #[test]
    fn large_zlib_member_preserves_consumed_extent_and_requires_its_checksum() {
        let payload = (0..65_537)
            .map(|index| u8::try_from(index % 251).expect("remainder fits in one byte"))
            .collect::<Vec<_>>();
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::none());
        encoder.write_all(&payload).expect("write member");
        let compressed = encoder.finish().expect("finish member");
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&compressed, &arena, &DecodePolicy::service())
                .expect("root");
        let (expanded, consumed) =
            inflate_zlib_member(&ctx, root, ExpandSpec::Unknown).expect("large member");
        assert_eq!(expanded.window(), payload);
        assert_eq!(consumed, compressed.len());
        for missing in [1, 4] {
            let truncated = root
                .child(0, compressed.len() - missing)
                .expect("truncated view");
            assert!(matches!(
                inflate_zlib_member(&ctx, truncated, ExpandSpec::Unknown),
                Err(CodecError::Malformed(_))
            ));
        }
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
    fn raw_deflate_helper_rejects_trailing_declared_bytes() {
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"one member").expect("write test member");
        let mut compressed = encoder.finish().expect("finish test member");
        compressed.extend_from_slice(b"suffix");
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&compressed, &arena, &DecodePolicy::service())
                .expect("build the decode context");
        assert!(matches!(
            inflate_deflate(&ctx, root, ExpandSpec::Exact(10)),
            Err(CodecError::Malformed(message))
                if message == "raw-DEFLATE member does not exhaust its declared input"
        ));
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
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&compressed, &arena, &DecodePolicy::default())
                .expect("root");
        assert_eq!(
            ctx.inflate_probe(root, 12, false)
                .expect("probe")
                .as_deref(),
            Some(b"Document.xml".as_slice())
        );
        assert!(ctx.inflate_probe(root, 11, false).expect("probe").is_none());
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
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&compressed, &arena, &DecodePolicy::default())
                .expect("root");
        assert_eq!(
            ctx.inflate_probe(root, 12, true).expect("probe").as_deref(),
            Some(b"Document.xml".as_slice())
        );
        assert!(ctx.inflate_probe(root, 11, true).expect("probe").is_none());
    }

    /// A Zstandard member is held to its record: the reference frame expands, and a declared
    /// size or CRC-32 that does not match the frame is refused rather than believed.
    #[test]
    fn a_zstandard_member_is_held_to_its_declared_size_and_crc() {
        let frame = cadmpeg_test_support::zip_fixture::ZSTD_GOLDEN_FRAME;
        let plain = cadmpeg_test_support::zip_fixture::zstd_golden_plaintext();
        let size = u64::try_from(plain.len()).expect("plaintext length fits u64");
        let crc = crc32fast::hash(&plain);
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
            .expect("empty root");

        assert_eq!(
            expand_zstd_member_owned(&ctx, frame, ExpandSpec::Exact(size), crc)
                .expect("the reference frame expands"),
            plain
        );
        assert!(matches!(
            expand_zstd_member_owned(&ctx, frame, ExpandSpec::Exact(size), crc ^ 1),
            Err(CodecError::Malformed(message)) if message.contains("CRC-32")
        ));
        for wrong in [size - 1, size + 1] {
            assert!(
                expand_zstd_member_owned(&ctx, frame, ExpandSpec::Exact(wrong), crc).is_err(),
                "a declared size of {wrong} for {size} bytes was accepted"
            );
        }
    }
}
