// SPDX-License-Identifier: Apache-2.0

use super::records::ManufacturingParameter;
use super::{local_nodes, owned, xml, XmlNode};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

pub(super) fn parameters(
    ctx: &DecodeContext<'_>,
    node: XmlNode<'_>,
) -> Result<Vec<ManufacturingParameter>, CodecError> {
    let mut parameters = super::parameters(ctx, node)?;
    for child in std::iter::once(node).chain(local_nodes(node).filter(XmlNode::is_element)) {
        for attribute in child.attributes() {
            parameters.push(ManufacturingParameter {
                name: owned(
                    ctx,
                    &format!("{}.{}", child.tag_name().name(), attribute.name()),
                )?,
                value: owned(ctx, attribute.value())?,
                unit: None,
                kind: None,
                native_xml: owned(ctx, xml(child))?,
            });
        }
    }
    Ok(parameters)
}
