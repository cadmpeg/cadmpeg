// SPDX-License-Identifier: Apache-2.0

use crate::compression::inflate_zstd_exact;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

// RFC 8878: single segment, three-byte content, one raw last block.
const ABC: &[u8] = &[
    0x28, 0xb5, 0x2f, 0xfd, 0x20, 3, 0x19, 0, 0, b'a', b'b', b'c',
];

fn decode(bytes: &[u8]) -> Result<Vec<u8>, CodecError> {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())?;
    Ok(inflate_zstd_exact(&ctx, root)?.window().to_vec())
}

#[test]
fn exact_frame_accepts_raw_rle_and_multiple_blocks() {
    assert_eq!(decode(ABC).expect("raw frame"), b"abc");
    let mut rle = ABC[..6].to_vec();
    rle.extend_from_slice(&[0x1b, 0, 0, b'a']);
    assert_eq!(decode(&rle).expect("RLE frame"), b"aaa");
    let mut blocks = ABC[..6].to_vec();
    blocks.extend_from_slice(&[8, 0, 0, b'a', 0x11, 0, 0, b'b', b'c']);
    assert_eq!(decode(&blocks).expect("two blocks"), b"abc");
}

#[test]
fn exact_frame_rejects_all_truncations_suffixes_and_skippable_frames() {
    for end in 0..ABC.len() {
        assert!(decode(&ABC[..end]).is_err(), "truncated at {end}");
    }
    let skippable = [0x50, 0x2a, 0x4d, 0x18, 0, 0, 0, 0];
    assert!(decode(&skippable).is_err());
    for suffix in [ABC, &skippable, &[0xff]] {
        let mut bytes = ABC.to_vec();
        bytes.extend_from_slice(suffix);
        assert!(decode(&bytes).is_err());
    }
}

#[test]
fn exact_frame_keeps_shared_content_and_checksum_validation() {
    let mut wrong_size = ABC.to_vec();
    wrong_size[5] = 4;
    assert!(decode(&wrong_size).is_err());
    let mut wrong_checksum = ABC.to_vec();
    wrong_checksum[4] |= 4;
    wrong_checksum.extend_from_slice(&[0; 4]);
    assert!(decode(&wrong_checksum).is_err());
    let mut reserved = ABC.to_vec();
    reserved[6] |= 6;
    assert!(decode(&reserved).is_err());
}

#[test]
fn exact_frame_accepts_optional_header_widths() {
    // Non-single segment, 1 KiB window, no declared content size.
    let mut window = ABC.to_vec();
    window[4] = 0;
    window[5] = 0;
    assert_eq!(decode(&window).expect("window frame"), b"abc");
    for (flag, size) in [
        (0xa0, vec![3, 0, 0, 0]),
        (0xe0, vec![3, 0, 0, 0, 0, 0, 0, 0]),
    ] {
        for (dictionary_flag, dictionary_width) in [(0, 0), (1, 1), (2, 2), (3, 4)] {
            let mut bytes = ABC[..4].to_vec();
            bytes.push(flag | dictionary_flag);
            bytes.resize(bytes.len() + dictionary_width, 0);
            bytes.extend_from_slice(&size);
            bytes.extend_from_slice(&ABC[6..]);
            assert_eq!(decode(&bytes).expect("optional header"), b"abc");
        }
    }
    // Two-byte content size stores the value minus 256; RLE expands to 259 bytes.
    let bytes = [0x28, 0xb5, 0x2f, 0xfd, 0x60, 3, 0, 0x1b, 8, 0, b'a'];
    let output = decode(&bytes).expect("two-byte size");
    assert_eq!(output.len(), 259);
    assert!(output.iter().all(|byte| *byte == b'a'));
}

#[test]
fn exact_frame_bounds_output_without_a_declared_content_size() {
    // A 1 KiB window and two RLE blocks, each with 1 KiB of decoded content.
    let bytes = [0x28, 0xb5, 0x2f, 0xfd, 0, 0, 2, 32, 0, b'a', 3, 32, 0, b'b'];
    for total in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        if total {
            policy.limits.max_decompressed_bytes_total = 1024;
        } else {
            policy.limits.max_decompressed_bytes_per_expand = 1024;
        }
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
        assert!(matches!(
            inflate_zstd_exact(&ctx, root),
            Err(CodecError::ResourceLimit(_))
        ));
    }
}
