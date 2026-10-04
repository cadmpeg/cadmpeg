// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
impl DecodeContext {
    pub fn charge_work(&self, _count: u64, _operation: &str) -> Result<(), ()> {
        Ok(())
    }
}

pub fn decode(ctx: &DecodeContext, haystack: &[u8], needle: &[u8], other: &[u8]) -> Result<(), ()> {
    ctx.charge_work(u64::try_from(haystack.len()).map_err(|_| ())?, "haystack")?;
    ctx.charge_work(u64::try_from(needle.len()).map_err(|_| ())?, "needle")?;
    let _paid = memchr::memmem::find(haystack, needle);
    let _reused = memchr::memmem::find(haystack, needle); // finding: uncharged_decode_work
    ctx.charge_work(u64::try_from(haystack.len()).map_err(|_| ())?, "haystack")?;
    ctx.charge_work(u64::try_from(other.len()).map_err(|_| ())?, "wrong needle")?;
    let _wrong = memchr::memmem::rfind(haystack, needle); // finding: uncharged_decode_work
    ctx.charge_work(u64::try_from(haystack.len()).map_err(|_| ())?, "haystack")?;
    ctx.charge_work(u64::try_from(needle.len()).map_err(|_| ())?, "needle")?;
    let _reverse = memchr::memmem::rfind(haystack, needle);
    ctx.charge_work(u64::try_from(needle.len()).map_err(|_| ())?, "constructor")?;
    let _search = memchr::memmem::find_iter(haystack, needle);
    let _unpaid = memchr::memmem::find_iter(haystack, needle); // finding: uncharged_decode_work
    Ok(())
}
