// SPDX-License-Identifier: Apache-2.0
//! Exact compressed provenance for a zlib member within a larger input.

use super::*;

#[test]
fn tracked_zlib_member_attributes_only_its_consumed_compressed_extent() {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(b"semantic bytes").expect("zlib input");
    let compressed = encoder.finish().expect("zlib member");
    let mut source = b"prefix".to_vec();
    let start = source.len();
    source.extend_from_slice(&compressed);
    let end = source.len();
    source.extend_from_slice(b"tail");
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::service()).expect("source");
    let (view, consumed) = crate::compression::inflate_zlib_member_tracked(
        &ctx,
        root.child(start, source.len()).expect("member and tail"),
        ExpandSpec::Exact(14),
    )
    .expect("tracked expansion");
    assert_eq!(consumed, compressed.len());
    assert_eq!(view.window(), b"semantic bytes");
    assert!(ctx.take_record_captures().expect("captures").is_empty());
    let (ctx, root) = DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::service())
        .expect("independent read source");
    let (view, consumed_again) = crate::compression::inflate_zlib_member_tracked(
        &ctx,
        root.child(start, source.len()).expect("member and tail"),
        ExpandSpec::Exact(14),
    )
    .expect("tracked expansion");
    assert_eq!(consumed_again, consumed);
    ctx.record(
        "zlib:test:value#0",
        view.child(0, 8).expect("value"),
        || {
            assert_eq!(view.read_at(0, 8), Some(b"semantic".as_slice()));
            Ok(())
        },
    )
    .expect("semantic read");
    let captures = ctx.take_record_captures().expect("captures");
    assert_eq!(
        captures[0].reads().get("file"),
        Some(&vec![
            cadmpeg_core::decode::u64_from_index(start)..cadmpeg_core::decode::u64_from_index(end)
        ])
    );
}

#[test]
fn tracked_zstd_frame_attributes_only_the_actual_compressed_child() {
    let frame = [
        0x28, 0xb5, 0x2f, 0xfd, 0x20, 3, 0x19, 0, 0, b'a', b'b', b'c',
    ];
    let mut bytes = b"prefix".to_vec();
    let start = bytes.len();
    bytes.extend_from_slice(&frame);
    let end = bytes.len();
    bytes.extend_from_slice(b"suffix");
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("source");
    let output = crate::compression::inflate_zstd_exact_tracked(
        &ctx,
        root.child(start, end).expect("actual frame"),
    )
    .expect("tracked zstd frame");
    assert_eq!(output.window(), b"abc");
    let extent = ctx.capture_part_extent(output).expect("genuine output");
    assert_eq!(extent.byte_len(), 3);
    ctx.record("zstd:test:value#0", output, || {
        assert_eq!(output.read_at(0, 1), Some(b"a".as_slice()));
        Ok(())
    })
    .expect("semantic output read");
    let captures = ctx.take_record_captures().expect("captures");
    assert_eq!(captures.len(), 1);
    assert_eq!(
        captures[0].reads().get("file"),
        Some(&vec![
            cadmpeg_core::decode::u64_from_index(start)..cadmpeg_core::decode::u64_from_index(end)
        ])
    );
}

#[test]
fn tracked_zstd_frame_keeps_exact_exhaustion_and_source_session_checks() {
    let frame = [
        0x28, 0xb5, 0x2f, 0xfd, 0x20, 3, 0x19, 0, 0, b'a', b'b', b'c',
    ];
    for end in 0..frame.len() {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&frame[..end], &arena, &DecodePolicy::service())
                .expect("truncated source");
        assert!(crate::compression::inflate_zstd_exact_tracked(&ctx, root).is_err());
    }
    let mut suffixed = frame.to_vec();
    suffixed.push(0xff);
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&suffixed, &arena, &DecodePolicy::service())
        .expect("source");
    assert!(crate::compression::inflate_zstd_exact_tracked(&ctx, root).is_err());
    let (first, root) =
        DecodeContext::from_root_bytes(&frame, &arena, &DecodePolicy::service()).expect("first");
    let (second, _) =
        DecodeContext::from_root_bytes(&frame, &arena, &DecodePolicy::service()).expect("second");
    assert!(crate::compression::inflate_zstd_exact_tracked(&second, root).is_err());
    assert!(first.take_record_captures().expect("no reads").is_empty());
    assert!(second.take_record_captures().expect("no reads").is_empty());
}
