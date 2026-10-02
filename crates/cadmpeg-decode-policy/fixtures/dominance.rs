// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
impl DecodeContext {
    pub fn charge_work(&self, _amount: u64, _operation: &str) -> Result<(), ()> { Ok(()) }
}
pub fn adapters(ctx: &DecodeContext, bytes: &[u8], other: &[u8]) -> Result<(), ()> {
    ctx.charge_work(u64::try_from(bytes.len()).map_err(|_| ())?, "windows")?;
    for _window in bytes.windows(4).enumerate().filter_map(|(_, x)| Some(x)) {}
    ctx.charge_work(bytes.len() as u64, "chunks")?;
    for _chunk in bytes.chunks(4) {}
    ctx.charge_work(bytes.len() as u64, "chunks exact")?;
    for _chunk in bytes.chunks_exact(4).skip(1).take(3).step_by(2).filter(|_| true).map(|x| x).enumerate() {}
    ctx.charge_work(other.len() as u64, "zip right")?;
    for _pair in bytes.iter().zip(other) {}
    ctx.charge_work(bytes.len() as u64, "subslice")?;
    for _byte in &bytes[1..] {}
    ctx.charge_work(bytes.len() as u64, "split left")?;
    let (left, _right) = bytes.split_at(1);
    for _byte in left {}
    ctx.charge_work(bytes.len() as u64, "split right")?;
    let (_left, right) = bytes.split_at(1);
    for _byte in right {}
    ctx.charge_work(bytes.len() as u64, "get")?;
    if let Some(sub) = bytes.get(1..) {
        for _byte in sub {}
    }
    Ok(())
}
pub fn sums(ctx: &DecodeContext, bytes: &[u8], other: &[u8]) -> Result<(), ()> {
    let work = (bytes.len() as u64).checked_add(other.len() as u64).ok_or(())?;
    ctx.charge_work(work, "sum")?;
    for _byte in bytes {}
    for _byte in other {}
    for _byte in bytes {} // finding: uncharged_decode_work
    Ok(())
}
pub fn products(ctx: &DecodeContext, bytes: &[u8]) -> Result<(), ()> {
    let work = (bytes.len() as u64).checked_mul(2).ok_or(())?;
    ctx.charge_work(work, "two branches")?;
    for _branch in [0, 1] {
        for _window in bytes.windows(4) {}
    }
    for _window in bytes.windows(4) {} // finding: uncharged_decode_work
    Ok(())
}
pub fn insufficient(ctx: &DecodeContext, bytes: &[u8]) -> Result<(), ()> {
    ctx.charge_work(bytes.len() as u64, "one pass")?;
    for _branch in [0, 1] {
        for _window in bytes.windows(4) {} // finding: uncharged_decode_work
    }
    Ok(())
}
pub fn unrelated(ctx: &DecodeContext, bytes: &[u8], other: &[u8]) -> Result<(), ()> {
    ctx.charge_work(bytes.len() as u64, "unrelated")?;
    for _chunk in other.chunks(4) {} // finding: uncharged_decode_work
    Ok(())
}
pub fn child_extent(ctx: &DecodeContext, values: &std::collections::HashMap<usize, String>) -> Result<(), ()> {
    ctx.charge_work(values.len() as u64, "parent extent")?;
    if let Some(text) = values.get(&0) {
        for _byte in text.as_bytes() {} // finding: uncharged_decode_work
    }
    Ok(())
}
