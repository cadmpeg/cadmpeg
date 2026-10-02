// SPDX-License-Identifier: Apache-2.0
use std::collections::HashMap;
pub struct DecodeContext;
impl DecodeContext {
    pub fn charge_work(&self, _n: u64, _operation: &str) -> Result<(), ()> {
        Ok(())
    }
    pub fn charge_retained(&self, _n: u64, _operation: &str) -> Result<(), ()> {
        Ok(())
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Borrowed<'a> {
    pub text: &'a str,
}
pub struct Custom;
impl From<&str> for Custom {
    fn from(_: &str) -> Self {
        Self
    }
}
pub fn decode(
    ctx: &DecodeContext,
    bytes: &[u8],
    values: &mut Vec<u8>,
    strings: &[String],
    map: &HashMap<u8, u8>,
    borrowed: Borrowed<'_>,
    callback: impl Iterator<Item = u8>,
) -> Result<(), ()> {
    let _index = values.get(0);
    values.clear();
    let _custom = Custom::from("input");
    let _text = format!("{borrowed:?}"); // finding: unproven_decode_charge
    ctx.charge_work(bytes.len() as u8 as u64, "narrow")?;
    let _narrow = bytes.iter().fold(0usize, |count, _| count + 1); // finding: unproven_decode_charge
    ctx.charge_work(strings.len() as u64, "children")?;
    let _children = strings == strings; // finding: unproven_decode_charge
    ctx.charge_work(map.len() as u64, "capacity")?;
    let _entries = map.iter().fold(0usize, |count, _| count + 1); // finding: unproven_decode_charge
    ctx.charge_work(1, "opaque")?;
    let _opaque = callback.count(); // finding: unproven_decode_charge
    let _parse = std::str::from_utf8(bytes).map_err(|_| ())?.parse::<u64>(); // finding: unproven_decode_charge, unproven_decode_charge
    Ok(())
}

pub fn wrong_dimension(ctx: &DecodeContext, bytes: &[u8]) -> Result<(), ()> {
    for _ in bytes {
        // finding: uncharged_decode_work
        ctx.charge_retained(1, "wrong dimension")?;
    }
    Ok(())
}

pub fn pointer_conversion(ctx: &DecodeContext, bytes: &mut [u8]) {
    let _ctx = ctx;
    let _pointer = std::ptr::NonNull::from(bytes);
}
pub fn unknown_range(ctx: &DecodeContext, count: usize) -> Result<(), ()> {
    ctx.charge_work((count / 2) as u64, "uncertain extent")?;
    for _ in 0..count {} // finding: unproven_decode_charge
    Ok(())
}
pub fn shared_copy(ctx: &DecodeContext, text: &str) {
    let _ctx = ctx;
    let _shared: std::rc::Rc<str> = text.into(); // finding: uncharged_decode_allocation, uncharged_decode_work
}
unsafe extern "C" {
    fn opaque_work(count: usize);
}
pub fn opaque_scalar(ctx: &DecodeContext, count: usize) {
    let _ctx = ctx;
    unsafe {
        opaque_work(count);
    } // finding: unproven_decode_charge
}

pub fn variable_constructor(ctx: &DecodeContext, bytes: &[u8]) {
    let _ctx = ctx;
    let _value = std::ffi::CString::new(bytes); // finding: unproven_decode_charge
}

pub fn empty_and_borrowed(ctx: &DecodeContext, text: &str, values: &mut Vec<u8>) {
    let _ctx = ctx;
    let _borrowed: std::borrow::Cow<'_, str> = text.into();
    let _empty = text.repeat(0);
    values.reserve(0);
    values.resize(0, 1);
    let _empty_children = vec![text.to_owned(); 0]; // finding: uncharged_decode_allocation, uncharged_decode_work
    let _outer = Box::new(text);
    let _copy = text.repeat(1); // finding: uncharged_decode_allocation, uncharged_decode_work
}

pub fn storage_and_count(ctx: &DecodeContext, bytes: &[u8], n: usize) {
    let _ctx = ctx;
    let _length = bytes.iter().count();
    let _range_length = (0..n).count();
    let _slots = Vec::<()>::with_capacity(n);
    let _filled = vec![(); n];
    let mut slots = Vec::<()>::new();
    slots.push(());
    let _items: Vec<()> = bytes.iter().map(|_| ()).collect(); // finding: uncharged_decode_work
}

pub struct NonAlloc {
    pub text: String,
}
impl NonAlloc {
    pub fn to_string(&self) -> String {
        String::new()
    }
}
pub fn named_nonallocator(ctx: &DecodeContext, value: &NonAlloc, text: &str) {
    let _ctx = ctx;
    let _empty = value.to_string();
    let _window = text.get(0..1);
    let mut slots = Vec::<u8>::new();
    slots.push(1); // finding: unproven_decode_charge
}

#[derive(Clone)]
pub struct DerivedCustom {
    pub child: NonAllocClone,
}
pub struct NonAllocClone {
    pub text: String,
}
impl Clone for NonAllocClone {
    fn clone(&self) -> Self {
        Self {
            text: String::new(),
        }
    }
}
pub fn derived_custom(ctx: &DecodeContext, value: &DerivedCustom) {
    let _ctx = ctx;
    let _copy = value.clone();
}
pub struct BorrowedClone<'a> {
    pub text: &'a str,
}
impl Clone for BorrowedClone<'_> {
    fn clone(&self) -> Self {
        let _temporary = self.text.to_owned(); // finding: uncharged_decode_allocation, uncharged_decode_work
        Self { text: self.text }
    }
}
pub fn custom_borrowed(ctx: &DecodeContext, value: &BorrowedClone<'_>) {
    let _ctx = ctx;
    let _copy = value.clone();
}

#[derive(PartialEq)]
pub struct Recursive {
    pub next: Option<Box<Recursive>>,
}
#[derive(PartialEq, Copy, Clone)]
pub struct ScalarRecord {
    pub value: u32,
}
#[derive(PartialEq, Copy, Clone)]
pub struct Siblings {
    pub left: ScalarRecord,
    pub right: ScalarRecord,
}
pub fn recursive_comparison(
    ctx: &DecodeContext,
    left: &Recursive,
    right: &Recursive,
    small: Siblings,
) {
    let _ctx = ctx;
    let _equal = left == right; // finding: unproven_decode_charge
    let _small = small == small;
}

fn from(n: usize) -> usize {
    n / 2
}
pub fn named_extent(ctx: &DecodeContext, bytes: &[u8]) -> Result<(), ()> {
    ctx.charge_work(from(bytes.len()) as u64, "not a conversion")?;
    let _count = bytes.iter().fold(0usize, |n, _| n + 1); // finding: unproven_decode_charge
    Ok(())
}

pub struct CustomRef<'a> {
    pub bytes: &'a [u8],
}
impl AsRef<[u8]> for CustomRef<'_> {
    fn as_ref(&self) -> &[u8] {
        let _scanned = self.bytes.iter().fold(0usize, |n, _| n + 1); // finding: uncharged_decode_work
        self.bytes
    }
}
pub fn custom_reference(ctx: &DecodeContext, value: &CustomRef<'_>) {
    let _ctx = ctx;
    let _bytes = value.as_ref();
}

fn opaque_iterator(bytes: &[u8]) -> impl Iterator<Item = u8> + '_ {
    bytes.iter().copied()
}
pub fn opaque_storage(ctx: &DecodeContext, bytes: &[u8]) {
    let _ctx = ctx;
    let mut stack = Vec::new();
    stack.push(opaque_iterator(bytes)); // finding: unproven_decode_charge
}
pub fn zero_sized_copies(ctx: &DecodeContext, units: &[()], output: &mut [()]) {
    let _ctx = ctx;
    let _copy = units.to_vec();
    let _owned = units.to_owned();
    output.copy_from_slice(units);
    output.copy_within(0..1, 1);
}

pub struct WrappedContext<'a> {
    pub ctx: &'a DecodeContext,
}
impl WrappedContext<'_> {
    pub fn charge_work(&self, _n: u64, _operation: &str) -> Result<(), ()> {
        Ok(())
    }
}
pub fn named_charge(reader: &WrappedContext<'_>, bytes: &[u8]) -> Result<(), ()> {
    reader.charge_work(bytes.len() as u64, "not a core charge")?;
    let _count = bytes.iter().fold(0usize, |n, _| n + 1); // finding: unproven_decode_charge
    for _byte in bytes {
        // finding: unproven_decode_charge
        reader.charge_work(1, "not a core charge")?;
    }
    Ok(())
}
unsafe extern "Rust" {
    fn opaque_context(ctx: &DecodeContext, bytes: &[u8]) -> usize;
}
pub fn context_argument(ctx: &DecodeContext, bytes: &[u8]) {
    let _value = unsafe { opaque_context(ctx, bytes) }; // finding: unproven_decode_charge
}

pub struct ConstantDisplay {
    pub text: String,
}
impl std::fmt::Display for ConstantDisplay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("fixed")
    }
}
pub fn custom_format(ctx: &DecodeContext, value: &ConstantDisplay) {
    let _ctx = ctx;
    let _text = format!("value: {value}"); // finding: unproven_decode_charge
}

pub fn moved_vector(ctx: &DecodeContext, values: Vec<u8>) {
    let _ctx = ctx;
    let _moved: Vec<_> = values.into_iter().collect();
}
pub fn consumed_vector(ctx: &DecodeContext, values: std::vec::IntoIter<u8>) {
    let _ctx = ctx;
    let _moved: Vec<_> = values.collect(); // finding: unproven_decode_charge
}
pub fn mapped_vector(ctx: &DecodeContext, values: Vec<u8>) {
    let _ctx = ctx;
    let _mapped: Vec<_> = values.into_iter().map(|x| x + 1).collect(); // finding: unproven_decode_charge, uncharged_decode_work
}
pub fn empty_iterator(ctx: &DecodeContext) {
    let _ctx = ctx;
    let values = Vec::<u8>::new();
    let _empty: Vec<_> = values.iter().copied().collect();
}

pub fn guarded_growth(ctx: &DecodeContext, values: &std::cell::RefCell<Vec<u8>>) {
    let _ctx = ctx;
    values.borrow_mut().push(1); // finding: uncharged_decode_allocation
}
