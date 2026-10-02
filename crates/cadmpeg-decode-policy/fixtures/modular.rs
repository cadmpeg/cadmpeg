// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let bytes = bytes.get(at..at + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}
fn scanning_at(bytes: &[u8], at: usize) -> Option<u32> {
    for byte in bytes {
        // finding: uncharged_decode_work
        std::hint::black_box(byte);
    }
    u32_at(bytes, at)
}
fn context_read(_ctx: &DecodeContext, bytes: &[u8]) -> Option<u32> {
    u32_at(bytes, 0)
}
pub fn caller(ctx: &DecodeContext, bytes: &[u8]) {
    let _fixed = u32_at(bytes, 0);
    let _scanned = scanning_at(bytes, 0);
    let _context = context_read(ctx, bytes);
    let callback = || scanning_at(bytes, 0);
    let _closure = callback();
}
