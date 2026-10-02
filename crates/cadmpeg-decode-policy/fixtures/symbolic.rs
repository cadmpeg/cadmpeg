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
