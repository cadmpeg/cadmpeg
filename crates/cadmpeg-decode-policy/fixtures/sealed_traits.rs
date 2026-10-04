// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
mod seal {
    pub trait Sealed {}
}
pub trait Closed: seal::Sealed {
    fn format(&self) -> &str;
}
pub struct Name(String);
impl seal::Sealed for Name {}
impl Closed for Name {
    fn format(&self) -> &str {
        &self.0
    }
}
pub fn closed<T: Closed>(_ctx: &DecodeContext, value: &T) -> usize {
    value.format().len()
}
pub trait Open {
    fn format(&self) -> &str;
}
pub fn open<T: Open>(_ctx: &DecodeContext, value: &T) -> usize {
    value.format().len() // finding: unproven_decode_charge
}
mod blanket_seal {
    pub trait Sealed {}
    impl<T> Sealed for T {}
}
pub trait Blanket: blanket_seal::Sealed {
    fn format(&self) -> &str;
}
pub fn blanket<T: Blanket>(_ctx: &DecodeContext, value: &T) -> usize {
    value.format().len() // finding: unproven_decode_charge
}
mod exported_seal {
    pub trait Sealed {}
}
pub use exported_seal::Sealed;
pub trait Exported: Sealed {
    fn format(&self) -> &str;
}
pub fn exported<T: Exported>(_ctx: &DecodeContext, value: &T) -> usize {
    value.format().len() // finding: unproven_decode_charge
}
mod scan_seal {
    pub trait Sealed {}
}
pub trait Scans: scan_seal::Sealed {
    fn scan(&self, bytes: &[u8]);
}
pub struct Scanner;
impl scan_seal::Sealed for Scanner {}
impl Scans for Scanner {
    fn scan(&self, bytes: &[u8]) {
        let copy = bytes.to_vec(); // finding: uncharged_decode_allocation, uncharged_decode_work
        drop(copy);
    }
}
pub fn scan<T: Scans>(_ctx: &DecodeContext, value: &T, bytes: &[u8]) {
    value.scan(bytes);
}
