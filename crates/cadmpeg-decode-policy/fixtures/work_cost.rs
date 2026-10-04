// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
impl DecodeContext {
    pub fn cost_sum(&self, left: u64, right: u64, _operation: &str) -> Result<u64, ()> {
        left.checked_add(right).ok_or(())
    }
    pub fn cost_product(&self, left: u64, right: u64, _operation: &str) -> Result<u64, ()> {
        left.checked_mul(right).ok_or(())
    }
    pub fn charge_work(&self, _amount: u64, _operation: &str) -> Result<(), ()> {
        Ok(())
    }
}
pub fn admitted(ctx: &DecodeContext, text: &str, pattern: &str) -> Result<(), ()> {
    let positions = ctx.cost_sum(text.len() as u64, 1, "search")?;
    let comparisons = ctx.cost_sum(pattern.len() as u64, 1, "search")?;
    ctx.charge_work(
        ctx.cost_product(positions, comparisons, "search")?,
        "search",
    )?;
    let _result = text.find(pattern);
    Ok(())
}
pub fn wrong_operand(ctx: &DecodeContext, bytes: &[u8], other: &[u8]) -> Result<(), ()> {
    ctx.charge_work(ctx.cost_product(bytes.len() as u64, 2, "copy")?, "copy")?;
    let _result = other.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte)); // finding: uncharged_decode_work
    Ok(())
}
pub fn affine_sum(ctx: &DecodeContext, bytes: &[u8]) -> Result<(), ()> {
    ctx.charge_work(ctx.cost_sum(bytes.len() as u64, 1, "scan")?, "scan")?;
    let _first = bytes.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte));
    let _second = bytes.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte)); // finding: uncharged_decode_work
    Ok(())
}
