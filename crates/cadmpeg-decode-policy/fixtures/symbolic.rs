// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
impl DecodeContext {
    pub fn charge_retained(&self, _bytes: u64, _operation: &str) -> Result<(), ()> { Ok(()) }
}
fn reserve<T>(ctx: &DecodeContext, values: &mut Vec<T>, count: usize) -> Result<(), ()> {
    let bytes = count.checked_mul(std::mem::size_of::<T>()).ok_or(())?;
    ctx.charge_retained(u64::try_from(bytes).map_err(|_| ())?, "slots")?;
    values.try_reserve_exact(count).map_err(|_| ())?;
    Ok(())
}
pub fn callers(ctx: &DecodeContext, small: &mut Vec<u8>, large: &mut Vec<[u64; 8]>, n: usize) -> Result<(), ()> {
    reserve(ctx, small, n)?;
    reserve(ctx, large, n)?;
    Ok(())
}
#[derive(Clone, PartialEq)]
pub struct Owned { pub values: Vec<u8> }
pub fn compare(ctx: &DecodeContext, left: &Owned, right: &Owned) -> bool {
    let _ctx = ctx;
    left == right // finding: uncharged_decode_work
}

fn wrong_count<T>(ctx: &DecodeContext, values: &mut Vec<T>, n: usize, other: usize) -> Result<(), ()> {
    let bytes = other.checked_mul(std::mem::size_of::<T>()).ok_or(())?;
    ctx.charge_retained(u64::try_from(bytes).map_err(|_| ())?, "wrong slots")?;
    values.try_reserve_exact(n).map_err(|_| ())?; // finding: unproven_decode_charge
    Ok(())
}
fn wrong_type<T>(ctx: &DecodeContext, values: &mut Vec<T>, n: usize) -> Result<(), ()> {
    let bytes = n.checked_mul(std::mem::size_of::<u8>()).ok_or(())?;
    ctx.charge_retained(u64::try_from(bytes).map_err(|_| ())?, "wrong type")?;
    values.try_reserve_exact(n).map_err(|_| ())?; // finding: unproven_decode_charge
    Ok(())
}
fn no_charge<T>(values: &mut Vec<T>, value: T) {
    values.push(value); // finding: uncharged_decode_allocation
}
fn duplicate<T>(ctx: &DecodeContext, values: &mut Vec<T>, n: usize) -> Result<(), ()> {
    let bytes = n.checked_mul(std::mem::size_of::<T>()).ok_or(())?;
    ctx.charge_retained(u64::try_from(bytes).map_err(|_| ())?, "one use")?;
    values.try_reserve_exact(n).map_err(|_| ())?;
    values.try_reserve_exact(n).map_err(|_| ())?; // finding: unproven_decode_charge
    Ok(())
}

impl DecodeContext {
    fn reserve_vec<T>(&self, _values: &mut Vec<T>, _count: usize) -> Result<(), ()> { Ok(()) }
    fn collection_vec<T>(&self, _count: usize) -> Result<Vec<T>, ()> { Ok(Vec::new()) }
    fn charge_work(&self, _count: u64, _operation: &str) -> Result<(), ()> { Ok(()) }
}
fn pushed<T>(ctx: &DecodeContext, values: &mut Vec<T>, value: T) -> Result<(), ()> {
    ctx.reserve_vec(values, 1)?;
    values.push(value);
    Ok(())
}
fn copied<T: Copy>(ctx: &DecodeContext, values: &[T]) -> Result<Vec<T>, ()> {
    ctx.charge_work(u64::try_from(values.len()).map_err(|_| ())?, "copy")?;
    let mut copy = ctx.collection_vec(values.len())?;
    copy.extend_from_slice(values);
    Ok(copy)
}
fn indexed<T>(ctx: &DecodeContext, count: usize, mut value: impl FnMut(usize) -> T) -> Result<Vec<T>, ()> {
    let mut values = ctx.collection_vec(count)?;
    for index in 0..count {
        ctx.charge_work(1, "each")?;
        values.push(value(index)); // finding: unproven_decode_charge
    }
    Ok(values)
}
fn wrong_target<T>(ctx: &DecodeContext, first: &mut Vec<T>, second: &mut Vec<T>, value: T) -> Result<(), ()> {
    ctx.reserve_vec(first, 1)?;
    second.push(value); // finding: unproven_decode_charge
    Ok(())
}

fn growth_delta<T>(ctx: &DecodeContext, values: &mut Vec<T>, capacity: usize) -> Result<(), ()> {
    let added = capacity - values.capacity();
    let bytes = added.checked_mul(std::mem::size_of::<T>()).ok_or(())?;
    ctx.charge_retained(u64::try_from(bytes).map_err(|_| ())?, "delta")?;
    values.try_reserve_exact(capacity - values.len()).map_err(|_| ())?;
    Ok(())
}

fn filled<T: Clone>(ctx: &DecodeContext, count: usize, value: T) -> Result<Vec<T>, ()> {
    ctx.charge_work(u64::try_from(count).map_err(|_| ())?, "fill")?;
    let mut values = ctx.collection_vec(count)?;
    values.resize(count, value);
    Ok(values)
}
pub fn fills(ctx: &DecodeContext, count: usize, text: String) -> Result<(), ()> {
    let _fixed = filled(ctx, count, 0u8)?;
    let _owned = filled(ctx, count, text)?; // finding: uncharged_decode_allocation, unproven_decode_charge
    Ok(())
}

fn double_push<T>(ctx: &DecodeContext, values: &mut Vec<T>, first: T, second: T) -> Result<(), ()> {
    ctx.reserve_vec(values, 1)?;
    values.push(first);
    values.push(second); // finding: unproven_decode_charge
    Ok(())
}
fn conditional<T>(ctx: &DecodeContext, values: &mut Vec<T>, value: T, yes: bool) -> Result<(), ()> {
    if yes { ctx.reserve_vec(values, 1)?; }
    values.push(value); // finding: unproven_decode_charge
    Ok(())
}
fn unrelated_result<T>(ctx: &DecodeContext, count: usize, value: T) -> Result<(), ()> {
    fn discard<T>(_: Vec<T>) -> Vec<T> { Vec::new() }
    let mut values = discard(ctx.collection_vec(count)?);
    values.push(value); // finding: unproven_decode_charge
    Ok(())
}
