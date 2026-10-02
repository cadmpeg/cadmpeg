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
