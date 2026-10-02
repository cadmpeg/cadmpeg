// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
#[derive(Clone, Copy)]
pub enum Kind {
    A,
    B,
}
#[derive(Clone)]
pub struct Owned {
    pub text: String,
}
pub struct Custom {
    pub text: String,
}
impl Clone for Custom {
    fn clone(&self) -> Self {
        Self {
            text: String::new(),
        }
    }
}
pub fn decode(
    ctx: &DecodeContext,
    text: &str,
    owned: &Owned,
    custom: &Custom,
    number: u32,
    kind: Kind,
) {
    let _ctx = ctx;
    let _text = text.to_string(); // finding: uncharged_decode_allocation
    let _owned = owned.clone(); // finding: uncharged_decode_allocation
    let _formatted = format!("value: {text}"); // finding: uncharged_decode_allocation
    let _fixed = number.to_string();
    let _enum = kind.clone();
    let _custom = custom.clone();
    let _range = (0..number).clone();
    let _bounded = format!("value: {number:?}");
    let _constant = "literal".to_owned();
}

const NAME: &str = "fixed";
pub fn constant_formats(ctx: &DecodeContext) {
    let _ctx = ctx;
    let _value = format!("name: {NAME}");
    let _text = NAME.to_owned();
    let _literal = format!("value: {}", "fixed");
}

#[rustfmt::skip]
pub fn same_line(ctx: &DecodeContext, text: &str) {
    let _ctx = ctx;
    let _a = text.to_string(); let _b = text.to_string(); // finding: uncharged_decode_allocation, uncharged_decode_allocation
}
