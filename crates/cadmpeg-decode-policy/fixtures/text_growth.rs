// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
impl DecodeContext {
    pub fn try_reserve_retained_text(&self, _text: &mut String, _count: usize, _operation: &str) -> Result<(), ()> { Ok(()) }
    pub fn charge_work(&self, _count: u64, _operation: &str) -> Result<(), ()> { Ok(()) }
}
pub fn admitted(ctx: &DecodeContext, value: &mut String, suffix: &str) -> Result<(), ()> {
    ctx.try_reserve_retained_text(value, suffix.len(), "text")?;
    ctx.charge_work(suffix.len() as u64, "text")?;
    value.push_str(suffix);
    Ok(())
}
pub fn wrong_target(ctx: &DecodeContext, value: &mut String, other: &mut String, suffix: &str) -> Result<(), ()> {
    ctx.try_reserve_retained_text(value, suffix.len(), "text")?;
    ctx.charge_work(suffix.len() as u64, "text")?;
    other.push_str(suffix); // finding: unproven_decode_charge
    Ok(())
}
pub fn wrong_count(ctx: &DecodeContext, value: &mut String, suffix: &str) -> Result<(), ()> {
    ctx.try_reserve_retained_text(value, 1, "text")?;
    ctx.charge_work(suffix.len() as u64, "text")?;
    value.push_str(suffix); // finding: unproven_decode_charge
    Ok(())
}
pub fn reused(ctx: &DecodeContext, value: &mut String, suffix: &str) -> Result<(), ()> {
    ctx.try_reserve_retained_text(value, suffix.len(), "text")?;
    ctx.charge_work(suffix.len() as u64, "text")?;
    value.push_str(suffix);
    ctx.charge_work(suffix.len() as u64, "text")?;
    value.push_str(suffix); // finding: unproven_decode_charge
    Ok(())
}
