// SPDX-License-Identifier: Apache-2.0
use std::collections::HashMap;
pub struct DecodeContext;
impl DecodeContext {
    pub fn charge_work(&self, _n: u64, _operation: &str) -> Result<(), ()> { Ok(()) }
    pub fn charge_retained(&self, _n: u64, _operation: &str) -> Result<(), ()> { Ok(()) }
}
#[derive(Clone, Copy, Debug)]
pub struct Borrowed<'a> { pub text: &'a str }
pub struct Custom;
impl From<&str> for Custom { fn from(_: &str) -> Self { Self } }
pub fn decode(ctx: &DecodeContext, bytes: &[u8], values: &mut Vec<u8>, strings: &[String], map: &HashMap<u8, u8>, borrowed: Borrowed<'_>, callback: impl Iterator<Item = u8>) -> Result<(), ()> {
    let _index = values.get(0);
    values.clear();
    let _custom = Custom::from("input");
    let _text = format!("{borrowed:?}"); // finding: unproven_decode_charge
    ctx.charge_work(bytes.len() as u8 as u64, "narrow")?;
    let _narrow = bytes.iter().count(); // finding: unproven_decode_charge
    ctx.charge_work(strings.len() as u64, "children")?;
    let _children = strings == strings; // finding: unproven_decode_charge
    ctx.charge_work(map.len() as u64, "capacity")?;
    let _entries = map.iter().count(); // finding: unproven_decode_charge
    ctx.charge_work(1, "opaque")?;
    let _opaque = callback.count(); // finding: unproven_decode_charge
    let _parse = std::str::from_utf8(bytes).map_err(|_| ())?.parse::<u64>(); // finding: unproven_decode_charge, unproven_decode_charge
    Ok(())
}

pub fn wrong_dimension(ctx: &DecodeContext, bytes: &[u8]) -> Result<(), ()> {
    for _ in bytes { // finding: uncharged_decode_work
        ctx.charge_retained(1, "wrong dimension")?;
    }
    Ok(())
}

pub fn pointer_conversion(ctx: &DecodeContext, bytes: &mut [u8]) {
    let _ctx = ctx;
    let _pointer = std::ptr::NonNull::from(bytes);
}
pub fn unknown_range(ctx: &DecodeContext, count: usize) -> Result<(), ()> {
    ctx.charge_work((count / 2) as u64, "uncertain extent")?;
    for _ in 0..count {} // finding: unproven_decode_charge
    Ok(())
}
pub fn shared_copy(ctx: &DecodeContext, text: &str) {
    let _ctx = ctx;
    let _shared: std::rc::Rc<str> = text.into(); // finding: uncharged_decode_allocation, uncharged_decode_work
}
unsafe extern "C" { fn opaque_work(count: usize); }
pub fn opaque_scalar(ctx: &DecodeContext, count: usize) {
    let _ctx = ctx;
    unsafe { opaque_work(count); } // finding: unproven_decode_charge
}
