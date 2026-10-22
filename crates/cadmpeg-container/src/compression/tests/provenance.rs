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
