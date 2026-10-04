// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
pub struct Reader<'a> {
    pub ctx: &'a DecodeContext,
}
impl Reader<'_> {
    pub fn decode(&self, text: &str) {
        let _copy = text.to_owned(); // finding: uncharged_decode_allocation
    }
}
pub fn owned(ctx: DecodeContext, text: &str) {
    let _ctx = ctx;
    let _copy = text.to_owned(); // finding: uncharged_decode_allocation
}
pub fn locals(text: &str) {
    let _ctx = DecodeContext;
    let _copy = text.to_owned();
}
pub fn closures(ctx: &DecodeContext, text: &str) {
    let _ctx = ctx;
    let _later = || text.to_owned(); // finding: uncharged_decode_allocation
    fn independent(text: &str) {
        let _copy = text.to_owned(); // finding: uncharged_decode_allocation
    }
    independent(text);
}
pub fn independent(text: &str) {
    let _copy = text.to_owned();
}
#[cfg(test)]
fn test_only(ctx: &DecodeContext, text: &str) {
    let _ctx = ctx;
    let _copy = text.to_owned();
}

pub fn boxed(ctx: Box<DecodeContext>, text: &str) {
    let _ctx = ctx;
    let _copy = text.to_owned(); // finding: uncharged_decode_allocation
}
pub fn shared(ctx: std::sync::Arc<DecodeContext>, text: &str) {
    let _ctx = ctx;
    let _copy = text.to_owned(); // finding: uncharged_decode_allocation
}

pub fn context_map(ctx: std::collections::HashMap<u32, DecodeContext>, text: &str) {
    let _ctx = ctx;
    let _copy = text.to_owned(); // finding: uncharged_decode_allocation
}
