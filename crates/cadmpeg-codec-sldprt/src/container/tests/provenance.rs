// SPDX-License-Identifier: Apache-2.0
//! Genuine expanded block extents for subsequent Rule B source-layout capture.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{ByteRange, SourceOrigin};
use cadmpeg_ir::SourceFidelity;

use super::*;

#[test]
fn direct_block_view_retains_real_compressed_extent_and_exact_payload_bytes() {
    let mut source = outer_header();
    let marker = source.len();
    let name = "Contents/PMISemanticDataDB";
    let payload = b"semantic payload";
    source.extend(make_block(0x42, name, payload));
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::service()).unwrap();
    let scan = container::scan(&ctx, root).unwrap();
    let block = &scan.blocks[0];
    let view = scan.block_view(block).unwrap();
    assert_eq!(view.window(), payload);
    assert_eq!(block.payload, payload);
    assert_eq!(block.section.name(), Some(name));
    assert_eq!(block.offset, marker);
    assert_ne!(view.window().as_ptr(), block.payload.as_ptr());
    let compressed_start = marker + super::super::block_hdr::LEN + name.len();
    let extent = ByteRange {
        start: u64_from_index(compressed_start),
        end: u64_from_index(compressed_start) + u64::from(block.comp_sz),
    };
    let stream = ctx.capture_part_extent(view).unwrap().stream().to_owned();
    assert!(ctx.source_streams().unwrap().iter().any(|source| {
        source.name() == stream
            && matches!(source.origin(), SourceOrigin::Expanded { parent, extent: actual }
            if parent == "file" && *actual == extent)
    }));
    ctx.record(
        "sldprt:test:block-value#0",
        view.child(1, 4).unwrap(),
        || {
            assert_eq!(view.read_at(1, 3), Some(b"ema".as_slice()));
            Ok(())
        },
    )
    .unwrap();
    let mut fidelity = SourceFidelity::default();
    fidelity
        .install_record_captures(&ctx, |_, _| Ok(()))
        .unwrap();
    assert_eq!(fidelity.record_layout().len(), 1);
    let (id, layout) = fidelity.record_layout().iter().next().unwrap();
    assert_eq!(id, "sldprt:test:block-value#0");
    assert_eq!((layout.offset(), layout.byte_len()), (1, 3));
    assert_eq!(layout.stream(), stream);
    assert_eq!(
        layout.reads(),
        &BTreeMap::from([
            ("file".into(), vec![extent.start..extent.end]),
            (stream, vec![1..4]),
        ])
    );
}

#[test]
fn duplicate_section_names_keep_distinct_actual_block_sources() {
    let mut source = outer_header();
    source.extend(make_block(0x42, "Contents/Same", b"first"));
    source.extend(make_block(0x43, "Contents/Same", b"second"));
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::service()).unwrap();
    let scan = container::scan(&ctx, root).unwrap();
    assert_eq!(scan.blocks.len(), 2);
    let first = scan.block_view(&scan.blocks[0]).unwrap();
    let second = scan.block_view(&scan.blocks[1]).unwrap();
    assert_eq!(first.window(), b"first");
    assert_eq!(second.window(), b"second");
    assert_eq!(scan.blocks[0].section.name(), scan.blocks[1].section.name());
    assert_ne!(
        ctx.capture_part_extent(first).unwrap().stream(),
        ctx.capture_part_extent(second).unwrap().stream()
    );
    assert_eq!(ctx.source_streams().unwrap().len(), 3);
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn nonzero_archive_view_uses_absolute_physical_payload_coordinates() {
    let mut image = outer_header();
    image.extend(make_block(0x42, "Contents/Values", b"native"));
    let mut source = b"prefix".to_vec();
    let start = source.len();
    source.extend_from_slice(&image);
    let end = source.len();
    source.extend_from_slice(b"suffix");
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::service()).unwrap();
    let scan = container::scan(&ctx, root.child(start, end).unwrap()).unwrap();
    let block = &scan.blocks[0];
    let view = scan.block_view(block).unwrap();
    let payload_start =
        start + outer_header().len() + super::super::block_hdr::LEN + "Contents/Values".len();
    let extent = ByteRange {
        start: u64_from_index(payload_start),
        end: u64_from_index(payload_start) + u64::from(block.comp_sz),
    };
    let capture = ctx.capture_part_extent(view).unwrap();
    assert_eq!(view.window(), b"native");
    assert_eq!(capture.offset(), 0);
    assert!(ctx.source_streams().unwrap().iter().any(|stream| {
        stream.name() == capture.stream()
            && matches!(stream.origin(), SourceOrigin::Expanded { parent, extent: actual }
            if parent == "file" && *actual == extent)
    }));
}

#[test]
fn invalid_crc_and_empty_invalid_members_publish_no_expanded_block_view() {
    for payload in [b"native".as_slice(), b"".as_slice()] {
        let mut block = make_block(0x42, "Contents/Invalid", payload);
        let crc = u32::from_le_bytes(
            block[super::super::block_hdr::CRC32..super::super::block_hdr::CRC32 + 4]
                .try_into()
                .unwrap(),
        );
        block[super::super::block_hdr::CRC32..super::super::block_hdr::CRC32 + 4]
            .copy_from_slice(&(crc ^ 1).to_le_bytes());
        let mut source = outer_header();
        source.extend(block);
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::service()).unwrap();
        let scan = container::scan(&ctx, root).unwrap();
        assert!(scan.blocks.is_empty());
        assert!(scan.block_views.is_empty());
        assert_eq!(ctx.source_streams().unwrap().len(), 1);
    }
}

#[test]
fn invalid_declared_size_skips_candidate_without_publishing_an_expanded_source() {
    for declared in [1_u32, 100_u32] {
        let mut block = make_block(0x42, "Contents/Invalid", b"native");
        block[super::super::block_hdr::UNCOMP_SZ..super::super::block_hdr::UNCOMP_SZ + 4]
            .copy_from_slice(&declared.to_le_bytes());
        let mut source = outer_header();
        source.extend(block);
        source.extend(make_block(0x43, "Contents/Kept", b"kept"));
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::service()).unwrap();
        let scan = container::scan(&ctx, root).unwrap();
        assert_eq!(scan.blocks.len(), 1);
        assert_eq!(scan.blocks[0].type_id, 0x43);
        assert_eq!(scan.block_view(&scan.blocks[0]).unwrap().window(), b"kept");
        assert_eq!(ctx.source_streams().unwrap().len(), 2);
        assert!(ctx.finish_session().is_ok());
    }
}

#[test]
fn empty_admitted_block_has_a_genuine_zero_length_expanded_view() {
    let mut source = outer_header();
    source.extend(make_block(0x42, "Contents/Empty", &[]));
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::service()).unwrap();
    let scan = container::scan(&ctx, root).unwrap();
    let view = scan.block_view(&scan.blocks[0]).unwrap();
    assert!(view.window().is_empty());
    assert_eq!(ctx.capture_part_extent(view).unwrap().byte_len(), 0);
    assert_eq!(ctx.source_streams().unwrap().len(), 2);
}
