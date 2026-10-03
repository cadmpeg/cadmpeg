// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
impl DecodeContext {
    pub fn linear_growth<T>(&self, _length: usize, _capacity: usize, count: usize, _operation: &str) -> Result<(usize, usize), ()> { Ok((count, std::mem::size_of::<T>())) }
}
pub fn admitted(ctx: &DecodeContext, values: &mut Vec<u64>) -> Result<(), ()> {
    let (additional, _bytes) = ctx.linear_growth::<u64>(values.len(), values.capacity(), 1, "growth")?;
    values.try_reserve_exact(additional).map_err(|_| ())?;
    values.push(1);
    Ok(())
}
pub fn wrong_target(ctx: &DecodeContext, values: &mut Vec<u64>, other: &mut Vec<u64>) -> Result<(), ()> {
    let (additional, _bytes) = ctx.linear_growth::<u64>(values.len(), values.capacity(), 1, "growth")?;
    other.try_reserve_exact(additional).map_err(|_| ())?; // finding: unproven_decode_charge
    Ok(())
}
pub fn wrong_element(ctx: &DecodeContext, values: &mut Vec<u64>) -> Result<(), ()> {
    let (additional, _bytes) = ctx.linear_growth::<u8>(values.len(), values.capacity(), 1, "growth")?;
    values.try_reserve_exact(additional).map_err(|_| ())?; // finding: unproven_decode_charge
    Ok(())
}
pub fn wrong_capacity(ctx: &DecodeContext, values: &mut Vec<u64>, other: &Vec<u64>) -> Result<(), ()> {
    let (additional, _bytes) = ctx.linear_growth::<u64>(values.len(), other.capacity(), 1, "growth")?;
    values.try_reserve_exact(additional).map_err(|_| ())?; // finding: unproven_decode_charge
    Ok(())
}
pub fn wrong_count(ctx: &DecodeContext, values: &mut Vec<u64>, count: usize) -> Result<(), ()> {
    let (_additional, bytes) = ctx.linear_growth::<u64>(values.len(), values.capacity(), 1, "growth")?;
    values.try_reserve_exact(count).map_err(|_| ())?; // finding: unproven_decode_charge
    values.try_reserve_exact(bytes).map_err(|_| ())?; // finding: unproven_decode_charge
    Ok(())
}
pub fn reused_count(ctx: &DecodeContext, values: &mut Vec<u64>) -> Result<(), ()> {
    let (additional, _bytes) = ctx.linear_growth::<u64>(values.len(), values.capacity(), 1, "growth")?;
    values.try_reserve_exact(additional).map_err(|_| ())?;
    values.try_reserve_exact(additional).map_err(|_| ())?; // finding: unproven_decode_charge
    Ok(())
}
pub fn reversed_metadata(ctx: &DecodeContext, values: &mut Vec<u64>) -> Result<(), ()> {
    let (additional, _bytes) = ctx.linear_growth::<u64>(values.capacity(), values.len(), 1, "growth")?;
    values.try_reserve_exact(additional).map_err(|_| ())?; // finding: unproven_decode_charge
    Ok(())
}
pub fn mutation_invalidates_returned_reserve_count(ctx: &DecodeContext, values: &mut Vec<u64>) -> Result<(), ()> {
    let (additional, _bytes) = ctx.linear_growth::<u64>(values.len(), values.capacity(), 1, "growth")?;
    values.push(1);
    values.try_reserve_exact(additional).map_err(|_| ())?; // finding: unproven_decode_charge
    Ok(())
}
