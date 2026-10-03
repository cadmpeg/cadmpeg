// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
impl DecodeContext {
    fn charge_work(&self, _: u64, _: &str) -> Result<(), ()> { Ok(()) }
    fn reserve_precharged_bytes<T>(&self, _: &mut Vec<T>, _: usize, _: &str, _: impl Fn(u64)) -> Result<(), ()> { Ok(()) }
}
pub fn admitted(ctx: &DecodeContext, values: &mut Vec<u8>, bytes: &[u8]) -> Result<(), ()> {
    ctx.reserve_precharged_bytes(values, bytes.len(), "bytes", |_| ())?;
    ctx.charge_work(bytes.len() as u64, "copy")?;
    values.extend_from_slice(bytes);
    ctx.charge_work(bytes.len() as u64, "copy")?;
    values.extend_from_slice(bytes); // finding: unproven_decode_charge
    Ok(())
}
pub fn wrong_target(ctx: &DecodeContext, values: &mut Vec<u8>, other: &mut Vec<u8>, bytes: &[u8]) -> Result<(), ()> {
    ctx.reserve_precharged_bytes(values, bytes.len(), "bytes", |_| ())?;
    ctx.charge_work(bytes.len() as u64, "copy")?;
    other.extend_from_slice(bytes); // finding: uncharged_decode_allocation
    Ok(())
}
pub fn wrong_count(ctx: &DecodeContext, values: &mut Vec<u8>, bytes: &[u8], end: usize) -> Result<(), ()> {
    ctx.reserve_precharged_bytes(values, bytes[..end].len(), "bytes", |_| ())?;
    ctx.charge_work(bytes.len() as u64, "copy")?;
    values.extend_from_slice(bytes); // finding: unproven_decode_charge
    Ok(())
}
pub fn wrong_element(ctx: &DecodeContext, values: &mut Vec<u16>, bytes: &[u16]) -> Result<(), ()> {
    ctx.reserve_precharged_bytes(values, bytes.len(), "bytes", |_| ())?;
    ctx.charge_work(bytes.len() as u64, "copy")?;
    values.extend_from_slice(bytes); // finding: unproven_decode_charge
    Ok(())
}
pub fn discarded_refusal(ctx: &DecodeContext, values: &mut Vec<u8>, bytes: &[u8]) -> Result<(), ()> {
    drop(ctx.reserve_precharged_bytes(values, bytes.len(), "bytes", |_| ()));
    ctx.charge_work(bytes.len() as u64, "copy")?;
    values.extend_from_slice(bytes); // finding: unproven_decode_charge
    Ok(())
}
pub fn mutation(ctx: &DecodeContext, values: &mut Vec<u8>, bytes: &[u8]) -> Result<(), ()> {
    ctx.reserve_precharged_bytes(values, bytes.len(), "bytes", |_| ())?;
    values.clear();
    ctx.charge_work(bytes.len() as u64, "copy")?;
    values.extend_from_slice(bytes); // finding: unproven_decode_charge
    Ok(())
}
