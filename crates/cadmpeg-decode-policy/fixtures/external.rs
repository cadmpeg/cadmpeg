// SPDX-License-Identifier: Apache-2.0
pub fn fixed(bytes: &[u8], value: usize) {
    let _length = bytes.len();
    let _windows = bytes.windows(4);
    let _sum = value.checked_add(1);
}
pub fn linear(bytes: &[u8]) {
    let _copy = bytes.to_vec(); // finding: uncharged_decode_allocation, uncharged_decode_work
    let _valid = std::str::from_utf8(bytes); // finding: uncharged_decode_work
}
pub fn missing() {
    let _thread = std::thread::current(); // finding: unproven_decode_charge
}

pub struct DecodeContext;
impl DecodeContext {
    pub fn charge_work(&self, _n: u64, _operation: &str) -> Result<(), ()> {
        Ok(())
    }
}
pub fn named_operand(ctx: &DecodeContext, destination: &mut String, text: &str) -> Result<(), ()> {
    ctx.charge_work(destination.len() as u64, "wrong operand")?;
    destination.push_str(text); // finding: uncharged_decode_allocation, uncharged_decode_work
    Ok(())
}

pub fn moving(mut value: Option<String>, replacement: String, repeated: String) {
    let _old = value.replace(replacement);
    let _taken = value.take();
    let _iterator = std::iter::repeat(repeated);
}
pub fn lossy(bytes: &[u8]) {
    let _text = String::from_utf8_lossy(bytes); // finding: unproven_decode_charge, uncharged_decode_work
}

pub fn repeating(text: &str, count: usize) {
    let _copy = text.repeat(count); // finding: uncharged_decode_allocation, uncharged_decode_work
}
