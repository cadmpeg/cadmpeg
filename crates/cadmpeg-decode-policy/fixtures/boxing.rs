// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
pub struct Reservation;
impl DecodeContext {
    pub fn reserve_scoped(&self, _bytes: u64, _operation: &str) -> Result<Reservation, ()> {
        Ok(Reservation)
    }
    pub fn admit_moves<T>(&self, _values: &[T], _moves: u64, _operation: &str) -> Result<(), ()> {
        Ok(())
    }
}
pub fn admitted(ctx: &DecodeContext, values: Vec<u64>) -> Result<Box<[u64]>, ()> {
    let _storage = ctx.reserve_scoped((values.len() as u64).checked_mul(8).ok_or(())?, "box")?;
    ctx.admit_moves(&values, 1, "box")?;
    Ok(values.into_boxed_slice())
}
pub fn exact(_ctx: &DecodeContext, values: Vec<u64>) -> Option<Box<[u64]>> {
    if values.len() == values.capacity() {
        return Some(values.into_boxed_slice());
    }
    None
}
pub fn wrong_capacity(
    _ctx: &DecodeContext,
    values: Vec<u64>,
    other: Vec<u64>,
) -> Option<Box<[u64]>> {
    if values.len() == other.capacity() {
        return Some(values.into_boxed_slice()); // finding: unproven_decode_charge, uncharged_decode_work
    }
    None
}
pub fn changed_capacity(_ctx: &DecodeContext, mut values: Vec<u64>) -> Option<Box<[u64]>> {
    if values.len() == values.capacity() {
        values.pop();
        return Some(values.into_boxed_slice()); // finding: unproven_decode_charge, uncharged_decode_work
    }
    None
}
pub fn wrong_storage(
    ctx: &DecodeContext,
    values: Vec<u64>,
    other: &Vec<u64>,
) -> Result<Box<[u64]>, ()> {
    let _storage = ctx.reserve_scoped((other.len() as u64).checked_mul(8).ok_or(())?, "box")?;
    ctx.admit_moves(&values, 1, "box")?;
    Ok(values.into_boxed_slice()) // finding: unproven_decode_charge
}
pub fn dropped_storage(ctx: &DecodeContext, values: Vec<u64>) -> Result<Box<[u64]>, ()> {
    let storage = ctx.reserve_scoped((values.len() as u64).checked_mul(8).ok_or(())?, "box")?;
    drop(storage);
    ctx.admit_moves(&values, 1, "box")?;
    Ok(values.into_boxed_slice()) // finding: unproven_decode_charge
}
pub fn expired_storage(ctx: &DecodeContext, values: Vec<u64>) -> Result<Box<[u64]>, ()> {
    {
        let _storage =
            ctx.reserve_scoped((values.len() as u64).checked_mul(8).ok_or(())?, "box")?;
    }
    ctx.admit_moves(&values, 1, "box")?;
    Ok(values.into_boxed_slice()) // finding: unproven_decode_charge
}
pub fn missing_moves(ctx: &DecodeContext, values: Vec<u64>) -> Result<Box<[u64]>, ()> {
    let _storage = ctx.reserve_scoped((values.len() as u64).checked_mul(8).ok_or(())?, "box")?;
    Ok(values.into_boxed_slice()) // finding: uncharged_decode_work
}

fn consume(storage: Reservation) {
    drop(storage);
}

pub fn consumed_storage(ctx: &DecodeContext, values: Vec<u64>) -> Result<Box<[u64]>, ()> {
    let storage = ctx.reserve_scoped((values.len() as u64).checked_mul(8).ok_or(())?, "box")?;
    consume(storage);
    ctx.admit_moves(&values, 1, "box")?;
    Ok(values.into_boxed_slice()) // finding: unproven_decode_charge
}

pub fn zero_sized<T>(_ctx: &DecodeContext, values: Vec<T>) -> Option<Box<[T]>> {
    if std::mem::size_of::<T>() == 0 {
        return Some(values.into_boxed_slice());
    }
    None
}

pub fn wrong_zero_sized<T>(_ctx: &DecodeContext, values: Vec<T>) -> Option<Box<[T]>> {
    if std::mem::size_of::<()>() == 0 {
        return Some(values.into_boxed_slice()); // finding: unproven_decode_charge, uncharged_decode_work
    }
    None
}
