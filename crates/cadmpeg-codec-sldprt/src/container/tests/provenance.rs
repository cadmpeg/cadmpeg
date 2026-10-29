// SPDX-License-Identifier: Apache-2.0
//! Expanded block views and their payload bytes.

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
    assert_eq!(view.window(), b"native");
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
        let Err(error) = container::scan(&ctx, root) else {
            panic!("a declared local member that fails CRC validation is malformed");
        };
        assert!(error.to_string().contains("local member at 8"), "{error}");
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
        let Err(error) = container::scan(&ctx, root) else {
            panic!("a declared local member with a wrong expanded size is malformed");
        };
        assert!(error.to_string().contains("local member at 8"), "{error}");
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
}

#[test]
fn expanded_block_bytes_are_not_charged_again_as_retained_bytes() {
    let mut source = outer_header();
    let payload = vec![0x5a_u8; 64 * 1024];
    source.extend(make_block(0x42, "Contents/PMISemanticDataDB", &payload));
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64_from_index(payload.len() / 2);
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&source, &arena, &policy).unwrap();
    let scan = container::scan(&ctx, root).unwrap();
    let block = &scan.blocks[0];
    assert_eq!(block.payload, payload);
    assert_eq!(scan.block_view(block).unwrap().window(), payload.as_slice());
}
