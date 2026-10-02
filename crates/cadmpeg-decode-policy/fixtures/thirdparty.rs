// SPDX-License-Identifier: Apache-2.0
pub fn parse(text: &str) {
    let _xml = roxmltree::Document::parse(text); // finding: uncharged_decode_allocation, uncharged_decode_work
    let _json = serde_json::from_str::<serde_json::Value>(text); // finding: uncharged_decode_allocation, uncharged_decode_work
}
pub fn access(node: roxmltree::Node<'_, '_>, name: &str, value: &serde_json::Value) {
    let _element = node.is_element();
    let _tag = node.has_tag_name(name); // finding: uncharged_decode_work
    let _attribute = node.attribute(name); // finding: uncharged_decode_work
    let _text = value.as_str();
    let _field = value.get(name); // finding: uncharged_decode_work
}
