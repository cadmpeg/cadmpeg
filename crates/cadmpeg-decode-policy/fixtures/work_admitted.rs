// SPDX-License-Identifier: Apache-2.0
pub mod decode {
    pub mod scan {
        pub struct AdmittedIter<I>(I);
        impl<I: Iterator> Iterator for AdmittedIter<I> {
            type Item = I::Item;
            fn next(&mut self) -> Option<Self::Item> {
                self.0.next()
            }
        }
        pub fn source<I: Iterator>(source: I) -> AdmittedIter<I> {
            AdmittedIter(source)
        }
    }
}
pub struct DecodeContext;
pub fn decode(
    ctx: &DecodeContext,
    bytes: &[u8],
    admitted: decode::scan::AdmittedIter<std::slice::Iter<'_, u8>>,
) {
    let _ctx = ctx;
    for byte in admitted {
        std::hint::black_box(byte);
    }
    let mut admitted = decode::scan::source(bytes.iter());
    std::hint::black_box(admitted.any(|byte| *byte == 1));
    let admitted = decode::scan::source(bytes.iter());
    for byte in admitted.filter(|byte| **byte != 0).take(2).enumerate() {
        std::hint::black_box(byte);
    }
    let admitted = decode::scan::source(bytes.iter());
    std::hint::black_box(admitted.filter(|byte| **byte != 0).count());
    let admitted = decode::scan::source(bytes.iter());
    std::hint::black_box(admitted.map(|byte| *byte).any(|byte| byte == 1));
    for byte in bytes {
        // finding: uncharged_decode_work
        std::hint::black_box(byte);
    }
}

pub fn utf16(ctx: &DecodeContext, units: &[u16]) {
    let _ctx = ctx;
    let source = decode::scan::source(units.iter());
    for character in char::decode_utf16(source.copied()) {
        std::hint::black_box(character.is_ok());
    }
    for character in char::decode_utf16(units.iter().copied()) {
        // finding: uncharged_decode_work
        std::hint::black_box(character.is_ok());
    }
}

fn opaque_admitted(bytes: &[u8]) -> impl Iterator<Item = u8> + '_ {
    decode::scan::source(bytes.iter()).copied()
}
fn opaque_raw(bytes: &[u8]) -> impl Iterator<Item = u8> + '_ {
    bytes.iter().copied()
}
pub fn opaque(ctx: &DecodeContext, bytes: &[u8]) {
    let _ctx = ctx;
    for byte in opaque_admitted(bytes) {
        std::hint::black_box(byte);
    }
    for byte in opaque_raw(bytes) {
        // finding: uncharged_decode_work
        std::hint::black_box(byte);
    }
}

pub fn fixed_and_admitted_chain(ctx: &DecodeContext, bytes: &[u8]) {
    let _ctx = ctx;
    let admitted = decode::scan::source(bytes.iter()).copied();
    for value in Some(0).into_iter().chain(admitted) {
        std::hint::black_box(value);
    }
    let admitted = decode::scan::source(bytes.iter()).copied();
    for value in admitted.chain(Some(0)) {
        std::hint::black_box(value);
    }
    for value in Some(0).into_iter().chain(bytes.iter().copied()) {
        // finding: uncharged_decode_work
        std::hint::black_box(value);
    }
}

fn fallible_opaque_chain(bytes: &[u8]) -> Result<impl Iterator<Item = &u8>, ()> {
    Ok(bytes
        .first()
        .into_iter()
        .chain(decode::scan::source(bytes.iter())))
}

fn fallible_opaque_raw(bytes: &[u8]) -> Result<impl Iterator<Item = &u8>, ()> {
    Ok(bytes.first().into_iter().chain(bytes.iter()))
}

fn fallible_opaque_utf16(
    units: &[u16],
) -> Result<impl Iterator<Item = Result<char, std::char::DecodeUtf16Error>> + '_, ()> {
    Ok(char::decode_utf16(decode::scan::source(units.iter()).copied()).map(|character| character))
}

pub fn fallible_opaque(ctx: &DecodeContext, bytes: &[u8], units: &[u16]) -> Result<(), ()> {
    let _ctx = ctx;
    for byte in fallible_opaque_chain(bytes)? {
        std::hint::black_box(byte);
    }
    for character in fallible_opaque_utf16(units)? {
        std::hint::black_box(character);
    }
    for byte in fallible_opaque_raw(bytes)? {
        // finding: uncharged_decode_work
        std::hint::black_box(byte);
    }
    Ok(())
}

pub fn fixed_options(ctx: &DecodeContext, bytes: &[u8]) {
    let _ctx = ctx;
    let first = Some(bytes);
    let second = None;
    for values in [&first, &second].into_iter().flatten() {
        std::hint::black_box(values);
        for value in *values {
            // finding: uncharged_decode_work
            std::hint::black_box(value);
        }
    }
    for value in [bytes, bytes].into_iter().flatten() {
        // finding: unproven_decode_charge
        std::hint::black_box(value);
    }
}
