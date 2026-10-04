// SPDX-License-Identifier: Apache-2.0
pub fn fixed(_ctx: &DecodeContext, bytes: &[u8], value: usize) {
    let _length = bytes.len();
    let _windows = bytes.windows(4);
    let _sum = value.checked_add(1);
}
pub fn linear(_ctx: &DecodeContext, bytes: &[u8]) {
    let _copy = bytes.to_vec(); // finding: uncharged_decode_allocation, uncharged_decode_work
    let _valid = std::str::from_utf8(bytes); // finding: uncharged_decode_work
}
pub fn missing(_ctx: &DecodeContext) {
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

pub fn moving(
    _ctx: &DecodeContext,
    mut value: Option<String>,
    replacement: String,
    repeated: String,
) {
    let _old = value.replace(replacement);
    let _taken = value.take();
    let _iterator = std::iter::repeat(repeated);
}
pub fn lossy(_ctx: &DecodeContext, bytes: &[u8]) {
    let _text = String::from_utf8_lossy(bytes); // finding: unproven_decode_charge, uncharged_decode_work
}

pub fn repeating(_ctx: &DecodeContext, text: &str, count: usize) {
    let _copy = text.repeat(count); // finding: uncharged_decode_allocation, uncharged_decode_work
}

pub fn missing_constructor(_ctx: &DecodeContext, name: &str) {
    let _command = std::process::Command::new(name); // finding: unproven_decode_charge
}

pub fn scalar_and_metadata(
    _ctx: &DecodeContext,
    value: f64,
    number: u64,
    bytes: &[u8],
    text: &str,
) {
    let _abs = value.abs();
    let _sin = value.sin();
    let _bits = value.to_bits();
    let _leading = number.leading_zeros();
    let _first = bytes.first();
    let _last = bytes.last();
    let _array: [usize; 4] = std::array::from_fn(|index| index);
    let _number = text.parse::<u64>(); // finding: uncharged_decode_work
    let _same = text.eq_ignore_ascii_case(text); // finding: uncharged_decode_work
    let _fixed = "a".eq_ignore_ascii_case("A");
    let _next = bytes.iter().next();
    let _mapped = Some(number).map_or_else(|| 0, |n| n + 1);
}

pub fn reference_operators(_ctx: &DecodeContext, value: &f64, number: &u64, flag: &bool) {
    let _sum = value + value;
    let _product = value * value;
    let _bits = number & number;
    let _inverse = !flag;
    let _comparison = number >= number;
}
