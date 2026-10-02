// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
const MESSAGE: &'static str = "fixed message";
fn construct<T: Into<String>>(message: T) -> String { message.into() }
fn forward<T: Into<String>>(message: T) -> String { construct(message) }
pub fn literal(ctx: &DecodeContext, choose: bool) {
    let _ctx = ctx;
    let _error = construct("fixed message");
    let _error = forward(MESSAGE);
    let text = MESSAGE.to_owned();
    let _copy = text.clone();
    let text = "left".replace("left", "right");
    let _copy = text.clone();
    let text = if choose { "one" } else { "two" };
    let _error = construct(text);
    let formatted = format!("message: {}", MESSAGE);
    let _copy = formatted.clone();
}
pub fn input(ctx: &DecodeContext, text: &str) {
    let _ctx = ctx;
    let _error = construct(text); // finding: uncharged_decode_allocation, uncharged_decode_work
    let _error = forward(text); // finding: uncharged_decode_allocation, uncharged_decode_work
}
pub fn changed(ctx: &DecodeContext, text: &str) {
    let _ctx = ctx;
    let mut message = MESSAGE;
    message = text;
    let _error = construct(message); // finding: uncharged_decode_allocation, uncharged_decode_work
}

pub fn repetitions(ctx: &DecodeContext, count: usize) {
    let _ctx = ctx;
    let fixed = MESSAGE.repeat(3);
    let _copy = fixed.clone();
    let variable = MESSAGE.repeat(count); // finding: uncharged_decode_allocation, uncharged_decode_work
    let _copy = variable.clone(); // finding: uncharged_decode_allocation, uncharged_decode_work
}
