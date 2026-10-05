// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use roxmltree::{Document, Node};

pub fn admitted_queries(
    ctx: &DecodeContext<'_>,
    document: &Document<'_>,
    tag_name: &str,
    attribute_name: &str,
) -> Result<(), CodecError> {
    let root = ctx.xml_root_element(document, "XML root lookup")?;
    let _has_expected_tag = ctx.xml_has_tag_name(root, tag_name, "XML tag lookup")?;
    let _attribute = ctx.xml_attribute(root, attribute_name, "XML attribute lookup")?;
    Ok(())
}

pub fn raw_queries_without_admission(
    _ctx: &DecodeContext<'_>,
    document: &Document<'_>,
    node: Node<'_, '_>,
    name: &str,
) {
    let _root = document.root_element(); // finding: uncharged_decode_work
    let _has_tag = node.has_tag_name(name); // finding: uncharged_decode_work
    let _attribute = node.attribute(name); // finding: uncharged_decode_work
}

pub fn raw_tag_query_charged_for_another_name(
    ctx: &DecodeContext<'_>,
    node: Node<'_, '_>,
    name: &str,
    unrelated_name: &str,
) -> Result<(), CodecError> {
    ctx.charge_work(
        u64_from_index(unrelated_name.len()),
        "unrelated XML tag name",
    )?;
    let _has_tag = node.has_tag_name(name); // finding: uncharged_decode_work
    Ok(())
}

pub fn raw_attribute_query_charged_for_another_node(
    ctx: &DecodeContext<'_>,
    node: Node<'_, '_>,
    unrelated_node: Node<'_, '_>,
    name: &str,
) -> Result<(), CodecError> {
    ctx.charge_work(
        u64_from_index(unrelated_node.range().len()),
        "unrelated XML node extent",
    )?;
    let _attribute = node.attribute(name); // finding: unproven_decode_charge
    Ok(())
}
