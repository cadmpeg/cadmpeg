// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Write as _;

use flate2::write::ZlibEncoder;
use flate2::Compression;

/// Inflated block size of a frame.
pub(crate) const BLOCK_SIZE: u32 = 0x40000;

/// A framed stream whose zlib stream inflates to `block` bytes.
pub(crate) fn framed_with(payload: &[u8], block: u32, declared: u64) -> Vec<u8> {
    let mut inflated = payload.to_vec();
    inflated.resize(usize::try_from(block).unwrap(), 0);
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&inflated).unwrap();
    let compressed = encoder.finish().unwrap();
    let mut stream = 0x3de3_de00_u32.to_le_bytes().to_vec();
    stream.extend_from_slice(&block.to_le_bytes());
    stream.extend_from_slice(&declared.to_le_bytes());
    stream.extend_from_slice(&u32::try_from(compressed.len()).unwrap().to_le_bytes());
    stream.extend_from_slice(&compressed);
    stream
}

/// A canonical framed stream holding `payload`.
pub(crate) fn framed(payload: &[u8]) -> Vec<u8> {
    framed_with(payload, BLOCK_SIZE, u64::try_from(payload.len()).unwrap())
}

/// A tree node holding `items` in order.
pub(crate) fn node(items: &[Vec<u8>]) -> Vec<u8> {
    let content: Vec<u8> = items.concat();
    let mut bytes = u64::try_from(content.len() + 8)
        .unwrap()
        .to_le_bytes()
        .to_vec();
    bytes.extend_from_slice(&content);
    bytes
}

/// A kernel record with the given kernel tag, header words, and kernel bytes.
pub(crate) fn record_with(kernel: u32, words: [u32; 4], body: &[u8]) -> Vec<u8> {
    let mut bytes = u32::try_from(24 + body.len())
        .unwrap()
        .to_le_bytes()
        .to_vec();
    bytes.extend_from_slice(&0x0009_b401_u32.to_le_bytes());
    bytes.extend_from_slice(&kernel.to_le_bytes());
    for word in words {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes.extend_from_slice(body);
    bytes
}

/// A kernel record with canonical header words.
pub(crate) fn record(kernel: u32, body: &[u8]) -> Vec<u8> {
    record_with(kernel, [1, 1, 1, 0], body)
}

/// The prologue of a little-endian Parasolid transmit.
pub(crate) fn parasolid_prologue(description: &str, schema: &str) -> Vec<u8> {
    let mut bytes = vec![0x41];
    bytes.extend_from_slice(&u16::try_from(description.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(description.as_bytes());
    bytes.extend_from_slice(&u32::try_from(schema.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(schema.as_bytes());
    bytes
}

/// Bytes of a committed `IronCAD` specimen.
pub(crate) fn corpus(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/ironcad")
        .join(name);
    std::fs::read(path).unwrap()
}
