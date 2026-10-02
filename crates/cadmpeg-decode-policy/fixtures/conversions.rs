// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
impl DecodeContext {
    pub fn charge_retained(&self, _bytes: u64, _operation: &str) -> Result<(), ()> { Ok(()) }
}
fn construct<T: Into<String>>(text: T) -> String { text.into() }
fn forward<T: Into<String>>(text: T) -> String { construct(text) }
struct Tagged { tag: Option<String> }
impl Tagged {
    fn with_tag(mut self, tag: impl Into<String>) -> Self { self.tag = Some(tag.into()); self }
}
pub fn charged(ctx: &DecodeContext, text: &str) -> Result<(), ()> {
    ctx.charge_retained(u64::try_from(text.len()).map_err(|_| ())?, "owned")?;
    let _direct = text.to_owned();
    ctx.charge_retained(u64::try_from(text.len()).map_err(|_| ())?, "owned")?;
    let _from = String::from(text);
    ctx.charge_retained(u64::try_from(text.len()).map_err(|_| ())?, "owned")?;
    let _into: String = text.into();
    ctx.charge_retained(u64::try_from(text.len()).map_err(|_| ())?, "owned")?;
    let _nested = forward(text);
    ctx.charge_retained(u64::try_from(text.len()).map_err(|_| ())?, "tag")?;
    let _tagged = Tagged { tag: None }.with_tag(text);
    Ok(())
}
pub fn wrong(ctx: &DecodeContext, text: &str, other: &str, yes: bool) -> Result<(), ()> {
    ctx.charge_retained(u64::try_from(other.len()).map_err(|_| ())?, "wrong")?;
    let _wrong = forward(text); // finding: uncharged_decode_allocation, uncharged_decode_work
    if yes { ctx.charge_retained(u64::try_from(text.len()).map_err(|_| ())?, "branch")?; }
    let _conditional = text.to_owned(); // finding: uncharged_decode_allocation, uncharged_decode_work
    let _drop = ctx.charge_retained(u64::try_from(text.len()).map_err(|_| ())?, "drop");
    let _dropped = forward(text); // finding: uncharged_decode_allocation, uncharged_decode_work
    ctx.charge_retained(u64::try_from(text.len()).map_err(|_| ())?, "once")?;
    let _once = forward(text);
    let _reuse = forward(text); // finding: uncharged_decode_allocation, uncharged_decode_work
    Ok(())
}
pub fn changed<'a>(ctx: &DecodeContext, mut text: &'a str, other: &'a str) -> Result<(), ()> {
    ctx.charge_retained(u64::try_from(text.len()).map_err(|_| ())?, "before change")?;
    text = other;
    let _changed = forward(text); // finding: uncharged_decode_allocation, uncharged_decode_work
    Ok(())
}
fn duplicate(text: &str) -> (String, String) {
    (text.to_owned(), text.to_owned()) // finding: uncharged_decode_allocation, uncharged_decode_work, uncharged_decode_allocation, uncharged_decode_work
}
pub fn duplicated(ctx: &DecodeContext, text: &str) -> Result<(), ()> {
    ctx.charge_retained(u64::try_from(text.len()).map_err(|_| ())?, "one")?;
    let _copies = duplicate(text);
    Ok(())
}
