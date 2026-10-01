// SPDX-License-Identifier: Apache-2.0
//! Admission of parser-owned XML and JSON trees.

use super::{u64_from_index, DecodeContext, ScopedReservation};
use crate::CodecError;

/// Bounds roxmltree 0.21.1 `NodeData`, including `NodeKind`, four node IDs,
/// positions (`Range<usize>`), enum alignment and inline expanded names.
const NODE_RECORD_BOUND: u64 = 128;
/// Bounds roxmltree 0.21.1 `AttributeData` and `TempAttributeData`, including
/// expanded names, `StringStorage`, position ranges and alignment.
const ATTRIBUTE_RECORD_BOUND: u64 = 128;
/// Bounds roxmltree 0.21.1 `Namespace`, including optional prefix,
/// `StringStorage` and alignment. Namespace indices are admitted separately.
const NAMESPACE_RECORD_BOUND: u64 = 64;

/// An XML document and the admission held until its storage is released.
#[derive(Debug)]
pub struct AdmittedXml<'input, 'ctx> {
    document: roxmltree::Document<'input>,
    _reservation: ScopedReservation<'ctx>,
}

impl<'input> AdmittedXml<'input, '_> {
    /// Borrows the admitted tree without releasing its reservation.
    pub fn document(&self) -> &roxmltree::Document<'input> {
        &self.document
    }
}

#[derive(Clone, Copy)]
enum XmlScan {
    Text,
    Tag { closing: bool, quote: u8, last: u8 },
    Comment,
    Cdata,
    Instruction,
    Declaration,
}

struct XmlBound {
    nodes: u64,
    attributes: u64,
    namespaces: u64,
    depth: u64,
}

/// Counts delimiters and element depth in one pass. Quoted values, comments,
/// CDATA and processing instructions do not contribute element nesting.
fn xml_bound(text: &str) -> XmlBound {
    let bytes = text.as_bytes();
    let mut markers = 0;
    let mut attributes = 0;
    let mut namespaces = 0;
    let mut depth = 0_u64;
    let mut maximum = 0;
    let mut state = XmlScan::Text;
    for (index, &byte) in bytes.iter().enumerate() {
        markers += u64::from(byte == b'<');
        attributes += u64::from(byte == b'=');
        let tail = &bytes[index..];
        namespaces += u64::from(tail.starts_with(b"xmlns"));
        state = match state {
            XmlScan::Text if byte == b'<' => {
                if tail.starts_with(b"<!--") { XmlScan::Comment }
                else if tail.starts_with(b"<![CDATA[") { XmlScan::Cdata }
                else if tail.starts_with(b"<?") { XmlScan::Instruction }
                else if tail.starts_with(b"<!") { XmlScan::Declaration }
                else { XmlScan::Tag { closing: tail.starts_with(b"</"), quote: 0, last: byte } }
            }
            XmlScan::Tag { closing, quote, last } => {
                if quote != 0 {
                    XmlScan::Tag { closing, quote: if byte == quote { 0 } else { quote }, last }
                } else if matches!(byte, b'\'' | b'"') {
                    XmlScan::Tag { closing, quote: byte, last }
                } else if byte == b'>' {
                    if closing {
                        if depth != 0 { depth -= 1; }
                    } else {
                        maximum = maximum.max(depth + 1);
                        if last != b'/' { depth += 1; }
                    }
                    XmlScan::Text
                } else { XmlScan::Tag { closing, quote, last: byte } }
            }
            XmlScan::Comment if tail.starts_with(b"-->") => XmlScan::Text,
            XmlScan::Cdata if tail.starts_with(b"]]>") => XmlScan::Text,
            XmlScan::Instruction if tail.starts_with(b"?>") => XmlScan::Text,
            XmlScan::Declaration if byte == b'>' => XmlScan::Text,
            other => other,
        };
    }
    XmlBound { nodes: markers, attributes, namespaces, depth: maximum }
}

/// Holds all nesting guards through the parser call. Each step is admitted
/// before recursion, including nesting already active in the caller.
fn at_depth<T>(
    ctx: &DecodeContext<'_>,
    depth: u64,
    operation: &'static str,
    parse: impl FnOnce() -> Result<T, CodecError>,
) -> Result<T, CodecError> {
    if depth == 0 { return parse(); }
    let _guard = ctx.enter_nested(operation)?;
    at_depth(ctx, depth - 1, operation, parse)
}

impl DecodeContext<'_> {
    fn tree_overflow(&self, operation: &'static str) -> CodecError {
        self.refuse_codec_limit(operation, u64::MAX, u64::MAX)
    }

    /// Admits roxmltree 0.21.1 storage before parsing with DTD disabled.
    /// For N = 2 * '<' + 2, A = '=' and S = 'xmlns' + 1, growing vectors
    /// reserve 2 * count + their minimum capacity. Storage covers nodes,
    /// permanent and temporary attributes (minimum 16), namespace records
    /// and sorted indices, and N * S inherited namespace indices. Node-sized
    /// scratch includes awaiting-subtree IDs, parent prefixes and text Cows.
    /// Eight input lengths cover normalization buffers, joined text, shared
    /// strings and transient copies; 32 bytes per string cover Arc headers.
    /// The fixed 1024 bytes cover initial scratch capacities and parser state.
    /// Work covers the scan, parser scans and quadratic attribute/namespace
    /// comparisons, including string comparisons and namespace insertion.
    pub fn parse_xml<'input>(
        &self,
        text: &'input str,
        operation: &'static str,
    ) -> Result<AdmittedXml<'input, '_>, CodecError> {
        let length = u64_from_index(text.len());
        self.charge_work(length, operation)?;
        let bound = xml_bound(text);
        let nodes = bound.nodes.checked_mul(2).and_then(|n| n.checked_add(2))
            .ok_or_else(|| self.tree_overflow(operation))?;
        let namespaces = bound.namespaces.checked_add(1)
            .ok_or_else(|| self.tree_overflow(operation))?;
        let capacity = |n: u64, minimum: u64| n.checked_mul(2).and_then(|n| n.checked_add(minimum));
        let bytes = (|| {
            let node_bytes = capacity(nodes, 4)?.checked_mul(NODE_RECORD_BOUND + 64)?;
            let attribute_bytes = capacity(bound.attributes, 16)?.checked_mul(ATTRIBUTE_RECORD_BOUND * 2)?;
            let namespace_bytes = capacity(namespaces, 4)?.checked_mul(NAMESPACE_RECORD_BOUND + 2)?;
            let inherited = capacity(nodes.checked_mul(namespaces)?, 4)?.checked_mul(2)?;
            let strings = nodes.checked_add(bound.attributes)?.checked_add(namespaces)?.checked_mul(32)?;
            node_bytes.checked_add(attribute_bytes)?.checked_add(namespace_bytes)?
                .checked_add(inherited)?.checked_add(strings)?.checked_add(length.checked_mul(8)?)?.checked_add(1024)
        })().ok_or_else(|| self.tree_overflow(operation))?;
        let comparisons = bound.attributes.checked_add(namespaces).and_then(|n| n.checked_add(1))
            .ok_or_else(|| self.tree_overflow(operation))?;
        let work = length.checked_mul(comparisons).and_then(|n| n.checked_mul(comparisons))
            .and_then(|n| n.checked_add(nodes)).ok_or_else(|| self.tree_overflow(operation))?;
        self.charge_collection_items(nodes, operation)?;
        self.charge_collection_items(bound.attributes, operation)?;
        self.charge_collection_items(namespaces, operation)?;
        self.charge_work(work, operation)?;
        let reservation = self.reserve_scoped(bytes, operation)?;
        let nodes_limit = u32::try_from(nodes).map_err(|_| self.tree_overflow(operation))?;
        let document = at_depth(self, bound.depth, operation, || {
            roxmltree::Document::parse_with_options(text, roxmltree::ParsingOptions {
                nodes_limit, ..roxmltree::ParsingOptions::default()
            }).map_err(|error| match error {
                roxmltree::Error::NodesLimitReached => self.refuse_codec_limit(operation, u64::from(nodes_limit), nodes),
                other => CodecError::malformed(format_args!("{other}")),
            })
        })?;
        Ok(AdmittedXml { document, _reservation: reservation })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::{DecodeArena, DecodePolicy, ResourceDimension};

    #[test]
    fn tree_xml_success_and_capacity_rounding() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::default()).unwrap();
        let tree = ctx.parse_xml("<r/>", "XML tree").unwrap();
        assert_eq!(tree.document().root_element().tag_name().name(), "r");
        let mut policy = DecodePolicy::default();
        policy.limits.max_materialized_bytes = 0;
        let (limited, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
        let CodecError::ResourceLimit(limit) = limited.parse_xml("<r/>", "XML tree").unwrap_err() else { panic!("byte refusal"); };
        assert!(limit.additional >= 16 * ATTRIBUTE_RECORD_BOUND);
    }

    #[test]
    fn tree_xml_resource_dimensions() {
        for dimension in [ResourceDimension::CollectionItems, ResourceDimension::MaterializedBytes,
            ResourceDimension::WorkUnits, ResourceDimension::RecursionDepth] {
            let mut policy = DecodePolicy::default();
            match dimension {
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = 0,
                _ => panic!("test dimension"),
            }
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
            let CodecError::ResourceLimit(limit) = ctx.parse_xml("<r/>", "XML tree").unwrap_err() else { panic!("resource refusal"); };
            assert_eq!(limit.dimension, dimension);
            assert_eq!(ctx.resource_refusal(), Some(limit));
        }
    }

    #[test]
    fn tree_xml_malformed_and_lexical_depth() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_recursion_depth = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
        assert!(matches!(ctx.parse_xml("<r>", "XML tree"), Err(CodecError::Malformed(_))));
        assert!(ctx.resource_refusal().is_none());
        ctx.parse_xml("<r a='>'><!-- <a> --><![CDATA[<b>]]><?pi <c> ?></r>", "XML tree").unwrap();
        assert!(matches!(ctx.parse_xml("<r><s/></r>", "XML tree"), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RecursionDepth));
    }
}
