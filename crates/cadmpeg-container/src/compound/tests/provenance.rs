// SPDX-License-Identifier: Apache-2.0
//! Source lineage across physical CFB sector seams.

use super::*;

#[test]
fn fragmented_stream_read_projects_into_its_exact_physical_sector_pieces() {
    let mut file = fixture();
    let fat = sector_mut(&mut file, 11);
    put_u32(fat, 2 * 4, 4);
    put_u32(fat, 4 * 4, 3);
    put_u32(fat, 3 * 4, 5);
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &DecodePolicy::service())
        .expect("CFB source");
    let snapshot = CompoundSnapshot::new(&ctx, root).expect("CFB structure");
    let view = snapshot
        .open(&ctx, snapshot.stream("Store/Large").expect("stream"))
        .expect("fragmented source stream");
    assert_eq!(view.window(), &[0x5a; 4096]);
    assert!(ctx.take_record_captures().expect("captures").is_empty());
    let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &DecodePolicy::service())
        .expect("independent read source");
    let snapshot = CompoundSnapshot::new(&ctx, root).expect("CFB structure");
    let view = snapshot
        .open(&ctx, snapshot.stream("Store/Large").expect("stream"))
        .expect("fragmented source stream");
    ctx.record(
        "cfb:test:value#0",
        view.child(511, 513).expect("seam"),
        || {
            assert_eq!(view.read_at(511, 2), Some([0x5a; 2].as_slice()));
            Ok(())
        },
    )
    .expect("tracked source read");
    let stream = ctx.capture_part_extent(view).expect("stream capture");
    assert_eq!((stream.offset(), stream.byte_len()), (0, 4096));
    assert!(ctx.source_streams().expect("streams").iter().any(|source| {
        source.name() == stream.stream()
            && matches!(source.origin(), cadmpeg_core::decode::SourceOrigin::Composite { segments }
                if segments.len() == 8 && segments[0].extent() == cadmpeg_core::decode::ByteRange { start: 1536, end: 2048 }
                    && segments[1].extent() == cadmpeg_core::decode::ByteRange { start: 2560, end: 3072 })
    }));
    let captures = ctx.take_record_captures().expect("captures");
    assert_eq!(captures.len(), 1);
    assert_eq!(
        captures[0].reads().get("file"),
        Some(&vec![2047..2048, 2560..2561])
    );
}
