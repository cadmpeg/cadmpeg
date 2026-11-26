// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

use super::payload;
use crate::test_support::{framed, framed_with, BLOCK_SIZE};

fn read(stream: &[u8]) -> Result<Vec<u8>, cadmpeg_core::CodecError> {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(stream, &arena, &policy).unwrap();
    payload(&ctx, root).map(|view| view.window().to_vec())
}

#[test]
fn payload_is_the_declared_prefix_of_the_inflated_block() {
    assert_eq!(read(&framed(b"payload")).unwrap(), b"payload");
    assert_eq!(read(&framed(b"")).unwrap(), b"");
}

#[test]
fn payload_longer_than_the_block_is_rejected() {
    let stream = framed_with(b"abc", 16, 17);
    assert!(read(&stream).is_err());
}

#[test]
fn block_that_inflates_to_another_size_is_rejected() {
    let mut stream = framed(b"abc");
    stream[4..8].copy_from_slice(&(BLOCK_SIZE + 1).to_le_bytes());
    assert!(read(&stream).is_err());
}

#[test]
fn compressed_length_past_the_stream_is_rejected() {
    let mut stream = framed(b"abc");
    let past = u32::try_from(stream.len()).unwrap();
    stream[16..20].copy_from_slice(&past.to_le_bytes());
    assert!(read(&stream).is_err());
}

#[test]
fn compressed_length_past_the_zlib_end_is_rejected() {
    let mut stream = framed(b"abc");
    let declared = u32::from_le_bytes(stream[16..20].try_into().unwrap());
    stream[16..20].copy_from_slice(&(declared + 1).to_le_bytes());
    stream.push(0);
    assert!(read(&stream).is_err());
}

#[test]
fn another_magic_is_rejected() {
    let mut stream = framed(b"abc");
    stream[0] ^= 1;
    assert!(read(&stream).is_err());
}
