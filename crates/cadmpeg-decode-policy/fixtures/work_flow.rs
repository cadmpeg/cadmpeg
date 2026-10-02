// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
impl DecodeContext { pub fn charge_work(&self, _n: u64, _operation: &str) -> Result<(), ()> { Ok(()) } }
pub fn decode(ctx: &DecodeContext, values: &mut Vec<u8>, bytes: &[u8], flag: bool, count: usize) -> Result<(), ()> {
    let extent = bytes.len();
    ctx.charge_work(extent as u64, "alias")?;
    let _paid = bytes.iter().count();
    ctx.charge_work(values.len() as u64, "mutation")?;
    values.push(0);
    let _changed = values.iter().count(); // finding: uncharged_decode_work
    ctx.charge_work(bytes.len() as u64, "child")?;
    if flag { let _child = bytes.iter().count(); }
    let _reused = bytes.iter().count(); // finding: uncharged_decode_work
    ctx.charge_work(bytes.len() as u64, "before closure")?;
    let _deferred = || bytes.iter().count(); // finding: uncharged_decode_work
    let _parent_scan = bytes.iter().count();
    for _value in bytes.iter().filter(|b| **b > 0) { // finding: uncharged_decode_work
        ctx.charge_work(1, "yield")?;
    }
    let mut end = 0;
    end += count;
    for _index in 0..end {} // finding: uncharged_decode_work
    for _value in bytes { // finding: uncharged_decode_work
        std::hint::black_box(0);
        ctx.charge_work(1, "late")?;
    }
    Ok(())
}
