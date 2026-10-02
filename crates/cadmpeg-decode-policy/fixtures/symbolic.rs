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
