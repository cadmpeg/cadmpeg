// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
fn copy<T: Clone>(value: &T) -> T {
    value.clone()
}
fn forward<T: Clone>(value: &T) -> T {
    copy(value)
}
pub fn fixed(_ctx: &DecodeContext, value: u32) {
    let _copy = copy(&value);
}
pub fn text(_ctx: &DecodeContext, value: &String) {
    let _copy = copy(value); // finding: uncharged_decode_allocation, uncharged_decode_work
}
pub fn transitive(_ctx: &DecodeContext, value: &Vec<u8>) {
    let _copy = forward(value); // finding: uncharged_decode_allocation, uncharged_decode_work
}
pub struct Empty;
impl Clone for Empty {
    fn clone(&self) -> Self {
        Self
    }
}
pub fn checked(_ctx: &DecodeContext, value: &Empty) {
    let _copy = copy(value);
}
pub fn unresolved<T: Clone>(_ctx: &DecodeContext, value: &T) {
    let _copy = value.clone(); // finding: unproven_decode_charge
}
pub trait Read {
    fn read(&self);
}
pub fn object(_ctx: &DecodeContext, value: &dyn Read) {
    value.read(); // finding: unproven_decode_charge
}
pub fn pointer(_ctx: &DecodeContext, value: fn()) {
    value(); // finding: unproven_decode_charge
}

trait Local {
    fn visit(&self);
}
impl Local for Empty {
    fn visit(&self) {}
}
fn bounded<T: Local>(value: &T) {
    value.visit();
}

pub fn repeated(_ctx: &DecodeContext, value: &String) {
    let _first = copy(value); // finding: uncharged_decode_allocation, uncharged_decode_work
    let _second = copy(value); // finding: uncharged_decode_allocation, uncharged_decode_work
}
fn closure_copy<T: Clone>(value: &T) {
    let callback = || value.clone();
    let _copy = callback();
}
pub fn closed(_ctx: &DecodeContext, value: &String) {
    closure_copy(value); // finding: uncharged_decode_allocation, uncharged_decode_work
}

fn vector_copy<T: Clone>(values: &Vec<T>) -> Vec<T> {
    values.clone()
}
pub fn unit_vectors(_ctx: &DecodeContext, values: &Vec<()>) {
    let _copy = vector_copy(values);
}
pub fn byte_vectors(_ctx: &DecodeContext, values: &Vec<u8>) {
    let _copy = vector_copy(values); // finding: uncharged_decode_allocation, uncharged_decode_work
}

fn minimum<T: Ord>(first: T, second: T) -> T {
    std::cmp::min(first, second)
}
pub fn numbers(_ctx: &DecodeContext, value: u32) {
    let _minimum = minimum(value, value);
}
