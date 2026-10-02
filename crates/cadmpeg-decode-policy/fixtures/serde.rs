// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
use serde::Deserialize;
#[derive(Clone, PartialEq, serde::Serialize, Deserialize)]
pub struct Owned {
    pub values: Vec<String>,
}
pub fn parse(_ctx: &DecodeContext, text: &str) {
    Local(text).serialize();
    let _record = serde_json::from_str::<Owned>(text); // finding: uncharged_decode_allocation, uncharged_decode_work
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let _record = Owned::deserialize(&mut deserializer); // finding: uncharged_decode_allocation, uncharged_decode_work
}

trait Serialize {
    fn serialize(&self);
}
struct Local<'a>(&'a str);
impl Serialize for Local<'_> {
    fn serialize(&self) {
        let _copy = self.0.to_owned(); // finding: uncharged_decode_allocation, uncharged_decode_work
    }
}

struct DecodeText<'a>(&'a str);
impl serde::Serialize for DecodeText<'_> {
    fn serialize<S: serde::Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
        for byte in self.0.as_bytes() {
            std::hint::black_box(byte);
        }
        let _deferred: fn(&[u8]) = |bytes| {
            for byte in bytes { std::hint::black_box(byte); }
        };
        Err(serde::ser::Error::custom("fixed"))
    }
}
pub fn decode_serialization<S: serde::Serializer>(ctx: &DecodeContext, text: &str, serializer: S) {
    let _context = ctx;
    let _record = serde::Serialize::serialize(&DecodeText(text), serializer); // finding: unproven_decode_charge
}

struct WriteText<'a>(&'a str);
impl serde::Serialize for WriteText<'_> {
    fn serialize<S: serde::Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
        for byte in self.0.as_bytes() {
            std::hint::black_box(byte);
        }
        let _deferred: fn(&[u8]) = |bytes| {
            for byte in bytes { std::hint::black_box(byte); }
        };
        Err(serde::ser::Error::custom("fixed"))
    }
}
pub fn serialization_only<S: serde::Serializer>(text: &str, serializer: S) {
    let _record = serde::Serialize::serialize(&WriteText(text), serializer);
}
