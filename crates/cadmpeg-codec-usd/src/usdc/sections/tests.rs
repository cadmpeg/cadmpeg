// SPDX-License-Identifier: Apache-2.0

use super::{paths, PathNode};
use crate::test_support::{usdc_compressed_ints, with_root};

/// The tree `/`, `/a`, `/a/b`, `/a.p`, `/c` with the tokens `a`, `b`, `p`,
/// `c`.
fn expected() -> Vec<Option<PathNode>> {
    let node = |parent, element, property| {
        Some(PathNode {
            parent,
            element,
            property,
        })
    };
    vec![
        node(None, 0, false),
        node(Some(0), 0, false),
        node(Some(1), 1, false),
        node(Some(1), 2, true),
        node(Some(0), 3, false),
    ]
}

fn read_paths(bytes: &[u8], version: [u8; 3]) -> Result<Vec<Option<PathNode>>, String> {
    with_root(bytes, |ctx, root| {
        paths(ctx, root, version, root, 4).map_err(|error| error.to_string())
    })
}

#[test]
fn compressed_jumps_build_the_tree() {
    let mut bytes = 5_u64.to_le_bytes().to_vec();
    bytes.extend_from_slice(&5_u64.to_le_bytes());
    bytes.extend(usdc_compressed_ints(&[0, 1, 2, 3, 4]));
    bytes.extend(usdc_compressed_ints(&[0, 0, 1, -2, 3]));
    bytes.extend(usdc_compressed_ints(&[-1, 3, 0, -2, -2]));
    assert_eq!(read_paths(&bytes, [0, 8, 0]), Ok(expected()));
}

/// One path item header of 0.1.0 (spec §9.5).
fn header(index: u32, token: u32, bits: u8) -> Vec<u8> {
    let mut out = index.to_le_bytes().to_vec();
    out.extend_from_slice(&token.to_le_bytes());
    out.extend_from_slice(&[bits, 0, 0, 0]);
    out
}

#[test]
fn item_headers_build_the_tree() {
    let mut bytes = 5_u64.to_le_bytes().to_vec();
    bytes.extend(header(0, 0, 1));
    bytes.extend(header(1, 0, 3));
    // The sibling `/c` starts after `/a`, its offset, `/a/b`, and `/a.p`.
    bytes.extend_from_slice(&64_i64.to_le_bytes());
    bytes.extend(header(2, 1, 2));
    bytes.extend(header(3, 2, 4));
    bytes.extend(header(4, 3, 0));
    assert_eq!(bytes.len(), 76);
    assert_eq!(read_paths(&bytes, [0, 1, 0]), Ok(expected()));
}

#[test]
fn padded_item_headers_of_the_first_version() {
    let mut bytes = 2_u64.to_le_bytes().to_vec();
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend(header(0, 0, 1));
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend(header(1, 3, 0));
    let read = read_paths(&bytes, [0, 0, 1]).expect("paths");
    assert_eq!(read[1].as_ref().map(|node| node.element), Some(3));
}

#[test]
fn refuses_a_repeated_path_index() {
    let mut bytes = 2_u64.to_le_bytes().to_vec();
    bytes.extend_from_slice(&2_u64.to_le_bytes());
    bytes.extend(usdc_compressed_ints(&[0, 0]));
    bytes.extend(usdc_compressed_ints(&[0, 0]));
    bytes.extend(usdc_compressed_ints(&[-1, -2]));
    assert!(read_paths(&bytes, [0, 8, 0]).is_err());
}

#[test]
fn refuses_a_jump_out_of_the_table() {
    let mut bytes = 2_u64.to_le_bytes().to_vec();
    bytes.extend_from_slice(&2_u64.to_le_bytes());
    bytes.extend(usdc_compressed_ints(&[0, 1]));
    bytes.extend(usdc_compressed_ints(&[0, 0]));
    bytes.extend(usdc_compressed_ints(&[-1, 0]));
    assert!(read_paths(&bytes, [0, 8, 0]).is_err());
}
