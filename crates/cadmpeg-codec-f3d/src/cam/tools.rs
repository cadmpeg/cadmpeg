// SPDX-License-Identifier: Apache-2.0

use super::records::ManufacturingParameter;
use super::{owned, xml, Index, XmlNode};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

/// Elements of a tool record whose attributes the format documents: `nc`
/// (tool number and post settings), `body` (cutter dimensions), `motion`
/// (feed and spindle settings), and `holder` (holder dimensions).
const ATTRIBUTE_ELEMENTS: [&str; 4] = ["nc", "body", "motion", "holder"];

/// The source text of the element's start tag.
fn start_tag(node: XmlNode<'_>) -> &str {
    let text = xml(node);
    let mut quote = None;
    for (offset, byte) in text.bytes().enumerate() {
        match (quote, byte) {
            (None, b'"' | b'\'') => quote = Some(byte),
            (Some(open), close) if open == close => quote = None,
            (None, b'>') => return &text[..=offset],
            _ => {}
        }
    }
    text
}

/// The `Parameter` elements of a tool, then the attributes of the tool
/// element and of its `nc`, `body`, and `motion` elements, named
/// `element.attribute`.
pub(super) fn parameters<'a>(
    ctx: &DecodeContext<'_>,
    index: &Index<'a>,
    node: XmlNode<'a>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<Vec<ManufacturingParameter>, CodecError> {
    let mut parameters = super::parameters(ctx, index, node, losses)?;
    let local = index.local(node);
    let documented = local
        .iter()
        .copied()
        .filter(|child| ATTRIBUTE_ELEMENTS.contains(&child.tag_name().name()));
    for child in std::iter::once(node).chain(documented) {
        for attribute in child.attributes() {
            parameters.push(ManufacturingParameter {
                name: owned(
                    ctx,
                    &format!("{}.{}", child.tag_name().name(), attribute.name()),
                )?,
                value: owned(ctx, attribute.value())?,
                unit: None,
                kind: None,
                native_xml: owned(ctx, start_tag(child))?,
            });
        }
    }
    Ok(parameters)
}
