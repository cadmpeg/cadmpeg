// SPDX-License-Identifier: Apache-2.0
pub mod decode {
    pub mod text {
        pub trait TextSource { fn as_text(&self) -> &str; }
    }
}
use decode::text::TextSource;
pub struct DecodeContext;
impl TextSource for str {
    fn as_text(&self) -> &str { self }
}
impl TextSource for String {
    fn as_text(&self) -> &str { self.as_str() }
}
pub struct Bad<'a>(&'a str);
impl TextSource for Bad<'_> {
    fn as_text(&self) -> &str {
        let _copy = self.0.to_owned(); // finding: uncharged_decode_allocation, uncharged_decode_work
        self.0
    }
}
pub fn closed<S: TextSource>(_ctx: &DecodeContext, value: &S) {
    std::hint::black_box(value.as_text());
}
pub fn custom<S: AsRef<str>>(_ctx: &DecodeContext, value: &S) {
    std::hint::black_box(value.as_ref()); // finding: unproven_decode_charge
}
