// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
impl DecodeContext {
    pub fn charge_work(&self, _n: u64, _operation: &str) -> Result<(), ()> { Ok(()) }
}
pub fn decode(ctx: &DecodeContext, bytes: &[u8], other: &[u8], n: usize, flag: bool) -> Result<(), ()> {
    for _byte in bytes {} // finding: uncharged_decode_work
    for _index in 0..n {} // finding: uncharged_decode_work
    for _byte in [1u8, 2, 3] {}
    for _index in 0..4 {}
    let _equal = bytes == other; // finding: uncharged_decode_work
    let _fixed_equal = bytes == b"fixed";
    let _scalar_equal = n == 5;
    let _scalar_min = n.min(4);
    let _scalar_max = n.max(4);
    let _count = bytes.iter().count(); // finding: uncharged_decode_work
    let _search = bytes.iter().any(|b| *b == 0); // finding: uncharged_decode_work
    let _prefix = bytes.starts_with(other); // finding: uncharged_decode_work
    let _fixed_prefix = bytes.starts_with(b"fixed");
    ctx.charge_work(bytes.len() as u64, "count")?;
    let _paid = bytes.iter().count();
    let _reused = bytes.iter().count(); // finding: uncharged_decode_work
    if flag { ctx.charge_work(bytes.len() as u64, "conditional")?; }
    let _conditional = bytes.iter().count(); // finding: uncharged_decode_work
    for _byte in bytes {
        ctx.charge_work(1, "iteration")?;
    }
    for _byte in bytes { // finding: uncharged_decode_work
        if flag { ctx.charge_work(1, "conditional")?; }
    }
    for _byte in bytes {
        if flag { ctx.charge_work(1, "yes")?; } else { ctx.charge_work(1, "no")?; }
    }
    ctx.charge_work(other.len() as u64, "unrelated")?;
    let _unrelated = bytes.iter().count(); // finding: uncharged_decode_work
    ctx.charge_work(bytes.len() as u64, "outer")?;
    for _byte in bytes {
        let _nested = bytes.iter().count(); // finding: uncharged_decode_work
    }
    for _byte in bytes { // finding: uncharged_decode_work
        ctx.charge_work(0, "zero")?;
    }
    loop {
        ctx.charge_work(1, "loop")?;
        break;
    }
    while flag {} // finding: uncharged_decode_work
    Ok(())
}
