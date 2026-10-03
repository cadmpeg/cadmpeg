// SPDX-License-Identifier: Apache-2.0
pub mod decode {
    pub mod scan {
        pub struct AdmittedIter<I>(I);
        impl<I: Iterator> Iterator for AdmittedIter<I> {
            type Item = I::Item;
            fn next(&mut self) -> Option<Self::Item> { self.0.next() }
        }
        pub fn source<I: Iterator>(source: I) -> AdmittedIter<I> { AdmittedIter(source) }
    }
}
pub struct DecodeContext;
pub fn decode(ctx: &DecodeContext, bytes: &[u8], admitted: decode::scan::AdmittedIter<std::slice::Iter<'_, u8>>) {
    let _ctx = ctx;
    for byte in admitted { std::hint::black_box(byte); }
    let mut admitted = decode::scan::source(bytes.iter());
    std::hint::black_box(admitted.any(|byte| *byte == 1));
    let admitted = decode::scan::source(bytes.iter());
    for byte in admitted.filter(|byte| **byte != 0).take(2).enumerate() { std::hint::black_box(byte); }
    let admitted = decode::scan::source(bytes.iter());
    std::hint::black_box(admitted.filter(|byte| **byte != 0).count());
    let admitted = decode::scan::source(bytes.iter());
    std::hint::black_box(admitted.map(|byte| *byte).any(|byte| byte == 1));
    for byte in bytes { std::hint::black_box(byte); } // finding: uncharged_decode_work
}

pub fn utf16(ctx: &DecodeContext, units: &[u16]) {
    let _ctx = ctx;
    let source = decode::scan::source(units.iter());
    for character in char::decode_utf16(source.copied()) { std::hint::black_box(character.is_ok()); }
    for character in char::decode_utf16(units.iter().copied()) { std::hint::black_box(character.is_ok()); } // finding: uncharged_decode_work
}
