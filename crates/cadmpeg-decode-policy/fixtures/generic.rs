// SPDX-License-Identifier: Apache-2.0
fn copy<T: Clone>(value: &T) -> T {
    value.clone()
}
fn forward<T: Clone>(value: &T) -> T {
    copy(value)
}
pub fn fixed(value: u32) {
    let _copy = copy(&value);
}
pub fn text(value: &String) {
    let _copy = copy(value); // finding: uncharged_decode_allocation, uncharged_decode_work
}
pub fn transitive(value: &Vec<u8>) {
    let _copy = forward(value); // finding: uncharged_decode_allocation, uncharged_decode_work
}
pub struct Empty;
impl Clone for Empty {
    fn clone(&self) -> Self {
        Self
    }
}
pub fn checked(value: &Empty) {
    let _copy = copy(value);
}
pub fn unresolved<T: Clone>(value: &T) {
    let _copy = value.clone(); // finding: unproven_decode_charge
}
pub trait Read {
    fn read(&self);
}
pub fn object(value: &dyn Read) {
    value.read(); // finding: unproven_decode_charge
}
pub fn pointer(value: fn()) {
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

pub fn repeated(value: &String) {
    let _first = copy(value); // finding: uncharged_decode_allocation, uncharged_decode_work
    let _second = copy(value); // finding: uncharged_decode_allocation, uncharged_decode_work
}
fn closure_copy<T: Clone>(value: &T) {
    let callback = || value.clone();
    let _copy = callback();
}
pub fn closed(value: &String) {
    closure_copy(value); // finding: uncharged_decode_allocation, uncharged_decode_work
}
