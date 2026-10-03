// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
pub struct ScopedReservation;
impl DecodeContext {
    pub fn linear_growth<T>(&self, _length: usize, _capacity: usize, count: usize, _operation: &str) -> Result<(usize, usize, ScopedReservation), ()> { Ok((count, std::mem::size_of::<T>(), ScopedReservation)) }
    pub fn charge_hash_growth<T>(&self, _length: usize, _capacity: usize, count: usize, _operation: &str) -> Result<(usize, ScopedReservation), ()> { Ok((count, ScopedReservation)) }
}
pub fn admitted(ctx: &DecodeContext, values: &mut Vec<u64>) -> Result<(), ()> {
    let (additional, _bytes, _growth) = ctx.linear_growth::<u64>(values.len(), values.capacity(), 1, "growth")?;
    values.try_reserve_exact(additional).map_err(|_| ())?;
    values.push(1);
    Ok(())
}
fn runtime_count() -> usize { 1 }
pub fn returned_count_admits_runtime_growth(ctx: &DecodeContext, values: &mut Vec<u64>) -> Result<(), ()> {
    let count = runtime_count();
    let (additional, _bytes, _growth) = ctx.linear_growth::<u64>(values.len(), values.capacity(), count, "growth")?;
    values.try_reserve_exact(additional).map_err(|_| ())?;
    Ok(())
}
pub fn wrong_target(ctx: &DecodeContext, values: &mut Vec<u64>, other: &mut Vec<u64>) -> Result<(), ()> {
    let (additional, _bytes, _growth) = ctx.linear_growth::<u64>(values.len(), values.capacity(), 1, "growth")?;
    other.try_reserve_exact(additional).map_err(|_| ())?; // finding: unproven_decode_charge
    Ok(())
}
pub fn wrong_element(ctx: &DecodeContext, values: &mut Vec<u64>) -> Result<(), ()> {
    let (additional, _bytes, _growth) = ctx.linear_growth::<u8>(values.len(), values.capacity(), 1, "growth")?;
    values.try_reserve_exact(additional).map_err(|_| ())?; // finding: unproven_decode_charge
    Ok(())
}
pub fn wrong_capacity(ctx: &DecodeContext, values: &mut Vec<u64>, other: &Vec<u64>) -> Result<(), ()> {
    let (additional, _bytes, _growth) = ctx.linear_growth::<u64>(values.len(), other.capacity(), 1, "growth")?;
    values.try_reserve_exact(additional).map_err(|_| ())?; // finding: unproven_decode_charge
    Ok(())
}
pub fn wrong_count(ctx: &DecodeContext, values: &mut Vec<u64>, count: usize) -> Result<(), ()> {
    let (_additional, bytes, _growth) = ctx.linear_growth::<u64>(values.len(), values.capacity(), 1, "growth")?;
    values.try_reserve_exact(count).map_err(|_| ())?; // finding: unproven_decode_charge
    values.try_reserve_exact(bytes).map_err(|_| ())?; // finding: unproven_decode_charge
    Ok(())
}
pub fn reused_count(ctx: &DecodeContext, values: &mut Vec<u64>) -> Result<(), ()> {
    let (additional, _bytes, _growth) = ctx.linear_growth::<u64>(values.len(), values.capacity(), 1, "growth")?;
    values.try_reserve_exact(additional).map_err(|_| ())?;
    values.try_reserve_exact(additional).map_err(|_| ())?; // finding: unproven_decode_charge
    Ok(())
}
pub fn reversed_metadata(ctx: &DecodeContext, values: &mut Vec<u64>) -> Result<(), ()> {
    let (additional, _bytes, _growth) = ctx.linear_growth::<u64>(values.capacity(), values.len(), 1, "growth")?;
    values.try_reserve_exact(additional).map_err(|_| ())?; // finding: unproven_decode_charge
    Ok(())
}
pub fn mutation_invalidates_returned_reserve_count(ctx: &DecodeContext, values: &mut Vec<u64>) -> Result<(), ()> {
    let (additional, _bytes, _growth) = ctx.linear_growth::<u64>(values.len(), values.capacity(), 1, "growth")?;
    values.push(1);
    values.try_reserve_exact(additional).map_err(|_| ())?; // finding: unproven_decode_charge
    Ok(())
}

pub fn string_capacity(ctx: &DecodeContext, value: &mut String, count: usize) -> Result<(), ()> {
    let (reserve, _bytes, _growth) = ctx.linear_growth::<u8>(value.len(), value.capacity(), count, "growth")?;
    value.try_reserve_exact(reserve).map_err(|_| ())?;
    Ok(())
}
pub fn wrong_string_element(ctx: &DecodeContext, value: &mut String, count: usize) -> Result<(), ()> {
    let (reserve, _bytes, _growth) = ctx.linear_growth::<u64>(value.len(), value.capacity(), count, "growth")?;
    value.try_reserve_exact(reserve).map_err(|_| ())?; // finding: unproven_decode_charge
    Ok(())
}

pub fn dropped_overlap(ctx: &DecodeContext, values: &mut Vec<u64>) -> Result<(), ()> {
    let (additional, _bytes, growth) = ctx.linear_growth::<u64>(values.len(), values.capacity(), 1, "growth")?;
    drop(growth);
    values.try_reserve_exact(additional).map_err(|_| ())?; // finding: unproven_decode_charge
    Ok(())
}
fn consume(_guard: ScopedReservation) {}
pub fn transferred_overlap(ctx: &DecodeContext, values: &mut Vec<u64>) -> Result<(), ()> {
    let (additional, _bytes, growth) = ctx.linear_growth::<u64>(values.len(), values.capacity(), 1, "growth")?;
    consume(growth);
    values.try_reserve_exact(additional).map_err(|_| ())?; // finding: unproven_decode_charge
    Ok(())
}
pub fn discarded_overlap(ctx: &DecodeContext, values: &mut Vec<u64>) -> Result<(), ()> {
    let (additional, _bytes, _) = ctx.linear_growth::<u64>(values.len(), values.capacity(), 1, "growth")?;
    values.try_reserve_exact(additional).map_err(|_| ())?; // finding: unproven_decode_charge
    Ok(())
}

pub fn hash_growth(ctx: &DecodeContext, values: &mut std::collections::HashSet<u64>) -> Result<(), ()> {
    let (_bytes, _growth) = ctx.charge_hash_growth::<u64>(values.len(), values.capacity(), 1, "growth")?;
    values.try_reserve(1).map_err(|_| ())?;
    Ok(())
}
pub fn hash_wrong_layout(ctx: &DecodeContext, values: &mut std::collections::HashSet<u64>) -> Result<(), ()> {
    let (_bytes, _growth) = ctx.charge_hash_growth::<u8>(values.len(), values.capacity(), 1, "growth")?;
    values.try_reserve(1).map_err(|_| ())?; // finding: unproven_decode_charge
    Ok(())
}
pub fn hash_wrong_map_layout(ctx: &DecodeContext, values: &mut std::collections::HashMap<u64, String>) -> Result<(), ()> {
    let (_bytes, _growth) = ctx.charge_hash_growth::<(u64, u8)>(values.len(), values.capacity(), 1, "growth")?;
    values.try_reserve(1).map_err(|_| ())?; // finding: unproven_decode_charge
    Ok(())
}
pub fn hash_map_growth(ctx: &DecodeContext, values: &mut std::collections::HashMap<u64, String>) -> Result<(), ()> {
    let (_bytes, _growth) = ctx.charge_hash_growth::<(u64, String)>(values.len(), values.capacity(), 1, "growth")?;
    values.try_reserve(1).map_err(|_| ())?;
    Ok(())
}
pub fn hash_dropped_overlap(ctx: &DecodeContext, values: &mut std::collections::HashSet<u64>) -> Result<(), ()> {
    let (_bytes, growth) = ctx.charge_hash_growth::<u64>(values.len(), values.capacity(), 1, "growth")?;
    drop(growth);
    values.try_reserve(1).map_err(|_| ())?; // finding: unproven_decode_charge
    Ok(())
}
