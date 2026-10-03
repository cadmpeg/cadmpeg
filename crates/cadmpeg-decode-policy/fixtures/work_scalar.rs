// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
impl DecodeContext {
    fn charge_work(&self, _count: u64, _operation: &str) -> Result<(), ()> {
        Ok(())
    }
}
pub mod decode {
    pub mod text {
        pub trait TextScalar: std::str::FromStr {}
        impl TextScalar for u64 {}
    }
}
pub fn scalar<T: decode::text::TextScalar>(ctx: &DecodeContext, text: &str) -> Result<(), ()> {
    ctx.charge_work(u64::try_from(text.len()).map_err(|_| ())?, "parse")?;
    let _parsed = text.parse::<T>();
    Ok(())
}
pub fn raw<T: decode::text::TextScalar>(_ctx: &DecodeContext, text: &str) {
    let _parsed = text.parse::<T>(); // finding: uncharged_decode_work
}
pub fn unresolved<T: std::str::FromStr>(ctx: &DecodeContext, text: &str) -> Result<(), ()> {
    ctx.charge_work(u64::try_from(text.len()).map_err(|_| ())?, "parse")?;
    let _parsed = text.parse::<T>(); // finding: unproven_decode_charge
    Ok(())
}

pub fn scalar_default<T: decode::text::TextScalar + Default>(_ctx: &DecodeContext) -> T {
    T::default()
}
pub fn unresolved_default<T: Default>(_ctx: &DecodeContext) -> T {
    T::default() // finding: unproven_decode_charge
}

pub fn character_edges(ctx: &DecodeContext, text: &str, character: char, prefix: &str) {
    let _ctx = ctx;
    std::hint::black_box(text.starts_with(character));
    std::hint::black_box(text.ends_with(character));
    std::hint::black_box(text.starts_with(prefix)); // finding: uncharged_decode_work
    std::hint::black_box(text.ends_with(prefix)); // finding: uncharged_decode_work
    std::hint::black_box(text.contains(character)); // finding: uncharged_decode_work
    std::hint::black_box(text.find(character)); // finding: uncharged_decode_work
}
