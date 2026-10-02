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

pub fn slice_copy<T: Copy>(values: &[T]) -> Vec<T> {
    values.to_vec()
}
pub fn capacity<T>() -> Vec<T> {
    Vec::with_capacity(4)
}

pub fn grow<T>(values: &mut Vec<T>, value: T) {
    values.push(value);
}

pub mod writer;

pub trait Fixed {
    fn fixed(&self) -> usize {
        1
    }
}
impl Fixed for u32 {}

pub struct DecodeContext;
pub fn context_copy<T: Clone>(_ctx: &DecodeContext, value: &T) -> T {
    value.clone()
}

pub fn constant_capacity<T, const N: usize>() -> Vec<T> {
    Vec::with_capacity(N)
}

pub fn array_copy<T: Copy>(values: &[T; 4]) -> Vec<T> {
    values.to_vec()
}
pub fn vector_copy<T: Clone>(values: &Vec<T>) -> Vec<T> {
    values.clone()
}

pub fn array_move<T>(values: [T; 4]) -> Vec<T> {
    Vec::from_iter(values)
}
pub fn array_collect<T>(values: [T; 4]) -> Vec<T> {
    values.into_iter().collect()
}

pub fn minimum<T: Ord>(first: T, second: T) -> T {
    std::cmp::min(first, second)
}

impl DecodeContext {
    pub fn charge_retained(&self, _bytes: u64, _operation: &str) -> Result<(), ()> { Ok(()) }
}
pub fn reserve<T>(ctx: &DecodeContext, values: &mut Vec<T>, n: usize) -> Result<(), ()> {
    let bytes = n.checked_mul(std::mem::size_of::<T>()).ok_or(())?;
    ctx.charge_retained(u64::try_from(bytes).map_err(|_| ())?, "slots")?;
    values.try_reserve_exact(n).map_err(|_| ())?;
    Ok(())
}

pub fn text<T: Into<String>>(message: T) -> String { message.into() }
pub fn forward_text<T: Into<String>>(message: T) -> String { text(message) }

fn overwrite<T>(value: &mut T, input: T) { *value = input; }
pub fn changed_text<T: Into<String>>(message: T, input: T) -> String {
    let mut message = message;
    overwrite(&mut message, input);
    text(message)
}

impl DecodeContext {
    fn charge_work(&self, _count: u64) -> Result<(), ()> { Ok(()) }
}
pub fn filled<T: Clone>(ctx: &DecodeContext, count: usize, value: T) -> Result<Vec<T>, ()> {
    let mut values = Vec::new();
    reserve(ctx, &mut values, count)?;
    ctx.charge_work(u64::try_from(count).map_err(|_| ())?)?;
    values.resize(count, value);
    Ok(values)
}
