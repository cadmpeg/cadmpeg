// SPDX-License-Identifier: Apache-2.0
use std::hash::{Hash, Hasher};
pub struct DecodeContext;
pub fn fixed(_ctx: &DecodeContext, bytes: &[u8], other: &[u8], at: usize) -> Option<()> {
    let header = bytes.get(4..8)?;
    let _same = bytes.get(48..52) == Some(header);
    let _relative = &bytes[at..at + 4] == other;
    let _prefix = &bytes[..4] == other;
    let _copy = header.to_vec();
    let _optional = bytes.get(4..8) == bytes.get(48..52);
    let first = bytes.first_chunk::<4>()?;
    let (chunk, rest) = bytes.split_first_chunk::<4>()?;
    let _chunk_equal = first == chunk;
    let _rest_equal = rest == other; // finding: uncharged_decode_work
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    header.hash(&mut hasher);
    let _sum = hasher.finish();
    let mut output = [0; 4];
    output.copy_from_slice(header);
    Some(())
}
pub fn variable(_ctx: &DecodeContext, bytes: &[u8], at: usize, end: usize, width: usize) {
    let _range = &bytes[at..end] == bytes; // finding: uncharged_decode_work
    let _width = &bytes[at..at + width] == bytes; // finding: uncharged_decode_work
    let _get = bytes.get(at..end) == bytes.get(..end); // finding: uncharged_decode_work
    let _copy = bytes.get(at..end).unwrap().to_vec(); // finding: uncharged_decode_allocation, uncharged_decode_work
}
pub fn children(_ctx: &DecodeContext, values: &[String]) {
    let _same = &values[0..4] == values; // finding: uncharged_decode_work
    let _copy = values[0..4].to_owned(); // finding: uncharged_decode_allocation, uncharged_decode_work
}
pub fn mutation(_ctx: &DecodeContext, bytes: &[u8]) {
    let mut slice = &bytes[0..4];
    slice = bytes;
    let _same = slice == bytes; // finding: uncharged_decode_work
}
