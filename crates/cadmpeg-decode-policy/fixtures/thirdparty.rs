// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
pub fn parse(_ctx: &DecodeContext, text: &str) {
    let _xml = roxmltree::Document::parse(text); // finding: uncharged_decode_allocation, uncharged_decode_work
    let _json = serde_json::from_str::<serde_json::Value>(text); // finding: unproven_decode_charge
}
pub fn access(
    _ctx: &DecodeContext,
    node: roxmltree::Node<'_, '_>,
    name: &str,
    value: &serde_json::Value,
) {
    let _element = node.is_element();
    let _tag = node.has_tag_name(name); // finding: uncharged_decode_work
    let _attribute = node.attribute(name); // finding: uncharged_decode_work
    let _text = value.as_str();
    let _field = value.get(name); // finding: uncharged_decode_work
}

pub fn constructors(_ctx: &DecodeContext, text: &str, bytes: &[u8]) {
    let _text_reader = serde_json::Deserializer::from_str(text);
    let _byte_reader = serde_json::Deserializer::from_slice(bytes);
}

pub fn parse_bytes(_ctx: &DecodeContext, bytes: &[u8]) {
    let _json = serde_json::from_slice::<serde_json::Value>(bytes); // finding: unproven_decode_charge
}

pub fn node_identity(_ctx: &DecodeContext, node: roxmltree::Node<'_, '_>) {
    let _same = node == node;
}
