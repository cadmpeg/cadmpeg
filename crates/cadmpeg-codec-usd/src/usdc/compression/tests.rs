// SPDX-License-Identifier: Apache-2.0

use super::{expand, Size};
use crate::test_support::with_root;

fn expanded(input: &[u8], size: Size) -> Result<Vec<u8>, String> {
    with_root(input, |ctx, root| {
        expand(ctx, root, size)
            .map(|view| view.window().to_vec())
            .map_err(|error| error.to_string())
    })
}

#[test]
fn one_block_of_literals() {
    let input = [&[0_u8, 0x50][..], b"hello"].concat();
    assert_eq!(expanded(&input, Size::Exact(5)), Ok(b"hello".to_vec()));
}

#[test]
fn overlapping_match_repeats_output() {
    // Three literals, then a match of length 6 at distance 3.
    let input = [0_u8, 0x32, b'a', b'b', b'c', 3, 0];
    assert_eq!(expanded(&input, Size::Exact(9)), Ok(b"abcabcabc".to_vec()));
}

#[test]
fn extended_literal_length() {
    let literals = vec![b'x'; 20];
    let input = [&[0_u8, 0xF0, 5][..], &literals].concat();
    assert_eq!(expanded(&input, Size::Exact(20)), Ok(literals));
}

#[test]
fn chunks_concatenate() {
    let mut input = vec![2_u8];
    for block in [&[0x20_u8, b'a', b'b'][..], &[0x10, b'c']] {
        input.extend_from_slice(&i32::try_from(block.len()).expect("size").to_le_bytes());
        input.extend_from_slice(block);
    }
    assert_eq!(expanded(&input, Size::Exact(3)), Ok(b"abc".to_vec()));
}

#[test]
fn at_most_accepts_a_shorter_output() {
    let input = [&[0_u8, 0x30][..], b"abc"].concat();
    assert_eq!(expanded(&input, Size::AtMost(8)), Ok(b"abc".to_vec()));
}

#[test]
fn refuses_an_output_of_another_size() {
    let input = [&[0_u8, 0x50][..], b"hello"].concat();
    assert!(expanded(&input, Size::Exact(4)).is_err());
    assert!(expanded(&input, Size::Exact(6)).is_err());
    assert!(expanded(&input, Size::AtMost(4)).is_err());
}

#[test]
fn refuses_a_match_before_the_block() {
    let input = [0_u8, 0x30, b'a', b'b', b'c', 4, 0];
    assert!(expanded(&input, Size::AtMost(64)).is_err());
}

#[test]
fn refuses_a_truncated_chunk() {
    let input = [1_u8, 9, 0, 0, 0, 0x10, b'a'];
    assert!(expanded(&input, Size::AtMost(64)).is_err());
}
