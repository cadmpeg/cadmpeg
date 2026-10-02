// SPDX-License-Identifier: Apache-2.0
pub fn copy<T: Clone>(value: &T) -> T {
    value.clone()
}
pub fn fixed(bytes: &[u8]) -> usize {
    bytes.len()
}
pub fn with<F: FnOnce() -> usize>(callback: F) -> usize {
    callback()
}

pub fn slice_copy<T: Copy>(values: &[T]) -> Vec<T> { values.to_vec() }
pub fn capacity<T>() -> Vec<T> { Vec::with_capacity(4) }

pub fn grow<T>(values: &mut Vec<T>, value: T) { values.push(value); }

pub mod writer;

pub trait Fixed { fn fixed(&self) -> usize { 1 } }
impl Fixed for u32 {}
