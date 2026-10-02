// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
impl DecodeContext {
    pub fn charge_retained(&self, _bytes: u64, _operation: &str) -> Result<(), ()> { Ok(()) }
    pub fn charge_work(&self, _bytes: u64, _operation: &str) -> Result<(), ()> { Ok(()) }
}
pub fn exact(ctx: &DecodeContext, bytes: &[u8]) -> Result<Vec<u8>, ()> {
    let count = bytes.len();
    let total_bytes = u64::try_from(count).map_err(|_| ())?;
    ctx.charge_work(total_bytes, "copy")?;
    ctx.charge_retained(total_bytes, "copy")?;
    let mut output = Vec::new();
    output.try_reserve_exact(count).map_err(|_| ())?;
    output.extend_from_slice(bytes);
    Ok(output)
}
pub fn text(ctx: &DecodeContext, text: &str) -> Result<String, ()> {
    ctx.charge_work(u64::try_from(text.len()).map_err(|_| ())?, "copy")?;
    ctx.charge_retained(u64::try_from(text.len()).map_err(|_| ())?, "copy")?;
    let mut output = String::new();
    output.try_reserve_exact(text.len()).map_err(|_| ())?;
    output.push_str(text);
    Ok(output)
}
pub fn dominated(ctx: &DecodeContext, count: usize) -> Result<(), ()> {
    let bytes = count.checked_mul(8).ok_or(())?;
    ctx.charge_retained(u64::try_from(bytes).map_err(|_| ())?, "bytes")?;
    let mut output = Vec::<u32>::new();
    output.try_reserve_exact(count).map_err(|_| ())?;
    Ok(())
}
pub fn reuse(ctx: &DecodeContext, count: usize, other: usize) -> Result<(), ()> {
    ctx.charge_retained(u64::try_from(count).map_err(|_| ())?, "bytes")?;
    let mut first = Vec::<u8>::new();
    first.try_reserve_exact(count).map_err(|_| ())?;
    let mut second = Vec::<u8>::new();
    second.try_reserve_exact(count).map_err(|_| ())?; // finding: unproven_decode_charge
    second.try_reserve_exact(other).map_err(|_| ())?; // finding: unproven_decode_charge
    Ok(())
}
pub fn wrong_width(ctx: &DecodeContext, count: usize) -> Result<(), ()> {
    ctx.charge_retained(u64::try_from(count).map_err(|_| ())?, "bytes")?;
    let mut output = Vec::<u32>::new();
    output.try_reserve_exact(count).map_err(|_| ())?; // finding: unproven_decode_charge
    Ok(())
}
pub fn conditional(ctx: &DecodeContext, count: usize, yes: bool) -> Result<(), ()> {
    if yes { ctx.charge_retained(u64::try_from(count).map_err(|_| ())?, "bytes")?; }
    let mut output = Vec::<u8>::new();
    output.try_reserve_exact(count).map_err(|_| ())?; // finding: unproven_decode_charge
    Ok(())
}
pub fn dropped(ctx: &DecodeContext, count: usize) -> Result<(), ()> {
    let _charge = ctx.charge_retained(u64::try_from(count).map_err(|_| ())?, "bytes");
    let mut output = Vec::<u8>::new();
    output.try_reserve_exact(count).map_err(|_| ())?; // finding: unproven_decode_charge
    Ok(())
}

pub fn failed_reserve(ctx: &DecodeContext, bytes: &[u8]) -> Result<(), ()> {
    ctx.charge_work(u64::try_from(bytes.len()).map_err(|_| ())?, "work")?;
    ctx.charge_retained(u64::try_from(bytes.len()).map_err(|_| ())?, "storage")?;
    let mut output = Vec::new();
    let _discarded = output.try_reserve_exact(bytes.len());
    output.extend_from_slice(bytes); // finding: unproven_decode_charge
    Ok(())
}

pub fn repeated_reserve(ctx: &DecodeContext, count: usize, repeats: usize) -> Result<(), ()> {
    ctx.charge_retained(u64::try_from(count).map_err(|_| ())?, "once")?;
    for _ in 0..repeats { // finding: uncharged_decode_work
        let mut output = Vec::<u8>::new();
        output.try_reserve_exact(count).map_err(|_| ())?; // finding: unproven_decode_charge
    }
    for _ in 0..repeats { // finding: uncharged_decode_work
        ctx.charge_retained(u64::try_from(count).map_err(|_| ())?, "each")?;
        let mut output = Vec::<u8>::new();
        output.try_reserve_exact(count).map_err(|_| ())?;
    }
    Ok(())
}
