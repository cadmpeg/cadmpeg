// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
impl DecodeContext {
    pub fn charge_work(&self, _n: u64, _operation: &str) -> Result<(), ()> {
        Ok(())
    }
}
pub fn decode(
    ctx: &DecodeContext,
    values: &mut Vec<u8>,
    bytes: &[u8],
    flag: bool,
    count: usize,
) -> Result<(), ()> {
    let extent = bytes.len();
    ctx.charge_work(extent as u64, "alias")?;
    let _paid = bytes.iter().fold(0usize, |count, _| count + 1);
    ctx.charge_work(values.len() as u64, "mutation")?;
    values.push(0);
    let _changed = values.iter().fold(0usize, |count, _| count + 1); // finding: uncharged_decode_work
    ctx.charge_work(bytes.len() as u64, "child")?;
    if flag {
        let _child = bytes.iter().fold(0usize, |count, _| count + 1);
    }
    let _reused = bytes.iter().fold(0usize, |count, _| count + 1); // finding: uncharged_decode_work
    ctx.charge_work(bytes.len() as u64, "before closure")?;
    let _deferred = || bytes.iter().fold(0usize, |count, _| count + 1); // finding: uncharged_decode_work
    let _parent_scan = bytes.iter().fold(0usize, |count, _| count + 1);
    for _value in bytes.iter().filter(|b| **b > 0) {
        // finding: uncharged_decode_work
        ctx.charge_work(1, "yield")?;
    }
    let mut end = 0;
    end += count;
    for _index in 0..end {} // finding: uncharged_decode_work
    for _value in bytes {
        // finding: uncharged_decode_work
        std::hint::black_box(0);
        ctx.charge_work(1, "late")?;
    }
    Ok(())
}

pub fn ranges(ctx: &DecodeContext, bytes: &[u8], count: usize) -> Result<(), ()> {
    ctx.charge_work(count as u64, "range")?;
    for _index in 0..count {}
    ctx.charge_work(bytes.len() as u64, "indexed range")?;
    for _index in 0..bytes.len() {}
    for _index in 0..=4 {}
    let fixed = [1u8, 2, 3];
    for _index in 0..fixed.len() {}
    Ok(())
}

pub fn numeric_word(ctx: &DecodeContext, mut word: u64) {
    let _ctx = ctx;
    let mut digits = 1;
    while word >= 10 {
        digits += 1;
        word /= 10;
    }
    while word != 0 {
        word >>= 1;
    }
    std::hint::black_box(digits);
}
