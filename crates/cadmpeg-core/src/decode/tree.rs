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
    Tag {
        closing: bool,
        quote: u8,
        last: u8,
        attributes: u64,
    },
    Comment,
    Cdata,
    Instruction,
    Declaration,
}

struct XmlBound {
    nodes: u64,
    attributes: u64,
    namespaces: u64,
    max_attributes: u64,
    depth: u64,
}

/// Counts delimiters and element depth in one pass. Quoted values, comments,
/// CDATA and processing instructions do not contribute element nesting.
/// An unmatched closing tag stops depth counting; the parser rejects it.
fn xml_bound(text: &str) -> XmlBound {
    let bytes = text.as_bytes();
    let mut markers = 0;
    let mut attributes = 0;
    let mut namespaces = 0;
    let mut depth = Some(0_u64);
    let mut maximum = 0;
    let mut max_attributes = 0;
    let mut state = XmlScan::Text;
    for (index, &byte) in bytes.iter().enumerate() {
        markers += u64::from(byte == b'<');
        attributes += u64::from(byte == b'=');
        let tail = &bytes[index..];
        namespaces += u64::from(tail.starts_with(b"xmlns"));
        state = match state {
            XmlScan::Text if byte == b'<' => {
                if tail.starts_with(b"<!--") {
                    XmlScan::Comment
                } else if tail.starts_with(b"<![CDATA[") {
                    XmlScan::Cdata
                } else if tail.starts_with(b"<?") {
                    XmlScan::Instruction
                } else if tail.starts_with(b"<!") {
                    XmlScan::Declaration
                } else {
                    XmlScan::Tag {
                        closing: tail.starts_with(b"</"),
                        quote: 0,
                        last: byte,
                        attributes: 0,
                    }
                }
            }
            XmlScan::Tag {
                closing,
                quote,
                last,
                attributes,
            } => {
                if quote != 0 {
                    XmlScan::Tag {
                        closing,
                        quote: if byte == quote { 0 } else { quote },
                        last,
                        attributes,
                    }
                } else if matches!(byte, b'\'' | b'"') {
                    XmlScan::Tag {
                        closing,
                        quote: byte,
                        last,
                        attributes,
                    }
                } else if byte == b'>' {
                    max_attributes = max_attributes.max(attributes);
                    if closing {
                        depth = depth.and_then(|depth| depth.checked_sub(1));
                    } else if let Some(parent_depth) = depth {
                        let element_depth = parent_depth + 1;
                        maximum = maximum.max(element_depth);
                        if last != b'/' {
                            depth = Some(element_depth);
                        }
                    }
                    XmlScan::Text
                } else {
                    let attributes = attributes + u64::from(byte == b'=');
                    max_attributes = max_attributes.max(attributes);
                    XmlScan::Tag {
                        closing,
                        quote,
                        last: byte,
                        attributes,
                    }
                }
            }
            XmlScan::Comment if tail.starts_with(b"-->") => XmlScan::Text,
            XmlScan::Cdata if tail.starts_with(b"]]>") => XmlScan::Text,
            XmlScan::Instruction if tail.starts_with(b"?>") => XmlScan::Text,
            XmlScan::Declaration if byte == b'>' => XmlScan::Text,
            other => other,
        };
    }
    XmlBound {
        nodes: markers,
        attributes,
        namespaces,
        max_attributes,
        depth: maximum,
    }
}

/// Holds nesting guards through the parser call without recursing in the
/// admission code. Guard slots and their temporary storage are admitted first.
fn at_depth<T>(
    ctx: &DecodeContext<'_>,
    depth: u64,
    operation: &'static str,
    parse: impl FnOnce() -> Result<T, CodecError>,
) -> Result<T, CodecError> {
    if depth == 0 {
        return parse();
    }
    ctx.charge_collection_items(depth, operation)?;
    let count =
        usize::try_from(depth).map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX, depth))?;
    let capacity = count
        .checked_add(4)
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, depth))?;
    let (mut guards, _reservation) = ctx.scoped_vector_storage(capacity, operation)?;
    ctx.charge_work(u64_from_index(count), operation)?;
    for _ in 0..count {
        guards.push(ctx.enter_nested(operation)?);
    }
    let result = parse();
    drop(guards);
    result
}

impl DecodeContext<'_> {
    fn tree_malformed(&self, error: impl std::fmt::Display, operation: &'static str) -> CodecError {
        match self.format_retained(format_args!("{error}"), operation) {
            Ok(message) => CodecError::Malformed(message),
            Err(error) => error,
        }
    }

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
    /// comparisons, including string comparisons and namespace insertion. Each
    /// element compares its attributes locally: work uses maximum attributes
    /// per tag, not the total across unrelated tags. Temporary node-ID, prefix,
    /// text, attribute and namespace-index slots are admitted separately.
    pub fn parse_xml<'input>(
        &self,
        text: &'input str,
        operation: &'static str,
    ) -> Result<AdmittedXml<'input, '_>, CodecError> {
        let length = u64_from_index(text.len());
        self.charge_work(length, operation)?;
        let bound = xml_bound(text);
        let nodes = bound
            .nodes
            .checked_mul(2)
            .and_then(|n| n.checked_add(2))
            .ok_or_else(|| self.tree_overflow(operation))?;
        let namespaces = bound
            .namespaces
            .checked_add(1)
            .ok_or_else(|| self.tree_overflow(operation))?;
        let capacity = |n: u64, minimum: u64| n.checked_mul(2).and_then(|n| n.checked_add(minimum));
        let bytes = (|| {
            let node_bytes = capacity(nodes, 4)?.checked_mul(NODE_RECORD_BOUND + 64)?;
            let attribute_bytes =
                capacity(bound.attributes, 16)?.checked_mul(ATTRIBUTE_RECORD_BOUND * 2)?;
            let namespace_bytes =
                capacity(namespaces, 4)?.checked_mul(NAMESPACE_RECORD_BOUND + 2)?;
            let inherited = capacity(nodes.checked_mul(namespaces)?, 4)?.checked_mul(2)?;
            let strings = nodes
                .checked_add(bound.attributes)?
                .checked_add(namespaces)?
                .checked_mul(32)?;
            node_bytes
                .checked_add(attribute_bytes)?
                .checked_add(namespace_bytes)?
                .checked_add(inherited)?
                .checked_add(strings)?
                .checked_add(length.checked_mul(8)?)?
                .checked_add(1024)
        })()
        .ok_or_else(|| self.tree_overflow(operation))?;
        let comparisons = bound
            .max_attributes
            .checked_add(namespaces)
            .and_then(|n| n.checked_add(1))
            .ok_or_else(|| self.tree_overflow(operation))?;
        let work = length
            .checked_mul(comparisons)
            .and_then(|n| n.checked_mul(comparisons))
            .and_then(|n| n.checked_add(nodes))
            .ok_or_else(|| self.tree_overflow(operation))?;
        self.charge_collection_items(nodes, operation)?;
        self.charge_collection_items(bound.attributes, operation)?;
        self.charge_collection_items(namespaces, operation)?;
        let scratch_items = nodes
            .checked_mul(3)
            .and_then(|n| n.checked_add(bound.attributes))
            .and_then(|n| n.checked_add(namespaces))
            .and_then(|n| n.checked_add(nodes.checked_mul(namespaces)?))
            .ok_or_else(|| self.tree_overflow(operation))?;
        self.charge_collection_items(scratch_items, operation)?;
        self.charge_work(work, operation)?;
        let reservation = self.reserve_scoped(bytes, operation)?;
        let nodes_limit = u32::try_from(nodes).map_err(|_| self.tree_overflow(operation))?;
        let document = at_depth(self, bound.depth, operation, || {
            roxmltree::Document::parse_with_options(
                text,
                roxmltree::ParsingOptions {
                    nodes_limit,
                    ..roxmltree::ParsingOptions::default()
                },
            )
            .map_err(|error| match error {
                roxmltree::Error::NodesLimitReached => {
                    self.refuse_codec_limit(operation, u64::from(nodes_limit), nodes)
                }
                other => self.tree_malformed(other, operation),
            })
        })?;
        Ok(AdmittedXml {
            document,
            _reservation: reservation,
        })
    }
}

/// `serde_json` 1.0.151 uses a `BTreeMap` without `preserve_order`. A leaf has
/// eleven (String, Value) slots; an internal node adds twelve pointers.
/// 4096 bytes per potential entry cover two nodes, split/root growth and
/// alignment, including a one-entry root where capacity rounding dominates.
const JSON_MAP_ENTRY_BOUND: u64 = 4096;
const RAW_VALUE_TOKEN: &[u8] = b"$serde_json::private::RawValue";

#[derive(Default)]
struct JsonStringScan {
    active: bool,
    escaped: bool,
    unicode_digits: u8,
    unicode_value: u32,
    matched: usize,
    possible: bool,
}

impl JsonStringScan {
    fn decoded(&mut self, byte: Option<u8>) {
        if byte.is_some() && byte == RAW_VALUE_TOKEN.get(self.matched).copied() {
            self.matched += 1;
        } else {
            self.possible = false;
        }
    }

    /// Returns true only when a completed string spells the raw-value token.
    fn byte(&mut self, byte: u8) -> bool {
        if self.unicode_digits != 0 {
            if let Some(digit) = char::from(byte).to_digit(16) {
                self.unicode_value = self.unicode_value * 16 + digit;
            } else {
                self.possible = false;
            }
            self.unicode_digits -= 1;
            if self.unicode_digits == 0 {
                self.decoded(u8::try_from(self.unicode_value).ok());
            }
        } else if self.escaped {
            self.escaped = false;
            if byte == b'u' {
                self.unicode_digits = 4;
                self.unicode_value = 0;
            } else {
                self.decoded(match byte {
                    b'"' | b'\\' | b'/' => Some(byte),
                    b'b' => Some(8),
                    b'f' => Some(12),
                    b'n' => Some(b'\n'),
                    b'r' => Some(b'\r'),
                    b't' => Some(b'\t'),
                    _ => None,
                });
            }
        } else if byte == b'\\' {
            self.escaped = true;
        } else if byte == b'"' {
            self.active = false;
            return self.possible && self.matched == RAW_VALUE_TOKEN.len();
        } else {
            self.decoded(Some(byte));
        }
        false
    }
}

/// Builds an ordinary JSON tree for derived typed conversion. The Value
/// deserializer's private raw-value carrier is not part of derived JSON data;
/// ignored extension members must remain ordinary objects during admission.
struct PlainJson(serde_json::Value);

impl<'de> serde::Deserialize<'de> for PlainJson {
    fn deserialize<D: serde::Deserializer<'de>>(parser: D) -> Result<Self, D::Error> {
        struct JsonVisitor;
        impl<'de> serde::de::Visitor<'de> for JsonVisitor {
            type Value = serde_json::Value;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a JSON value")
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(serde_json::Value::Null)
            }
            fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<Self::Value, E> {
                Ok(serde_json::Value::Bool(value))
            }
            fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Self::Value, E> {
                Ok(serde_json::Value::Number(value.into()))
            }
            fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Self::Value, E> {
                Ok(serde_json::Value::Number(value.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Self::Value, E> {
                serde_json::Number::from_f64(value)
                    .map(serde_json::Value::Number)
                    .ok_or_else(|| E::custom("JSON number is not finite"))
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(serde_json::Value::String(value.to_owned()))
            }
            fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Self::Value, E> {
                Ok(serde_json::Value::String(value))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = sequence.next_element::<PlainJson>()? {
                    values.push(value.0);
                }
                Ok(serde_json::Value::Array(values))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut values = serde_json::Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    let value = map.next_value::<PlainJson>()?;
                    drop(values.insert(key, value.0));
                }
                Ok(serde_json::Value::Object(values))
            }
        }
        parser.deserialize_any(JsonVisitor).map(Self)
    }
}

struct JsonBound {
    values: u64,
    entries: u64,
    depth: u64,
    bytes: u64,
}

impl DecodeContext<'_> {
    /// Bounds `serde_json` 1.0.151 trees in one scan. Delimiters '[' '{' ',' ':'
    /// plus one bound value slots. All array capacities together are at most
    /// six slots per value (doubling plus minimum four). Map entry capacity is
    /// charged per colon. Eight input lengths cover decoded
    /// strings, growing scratch and temporary copies; 1024 covers parser state.
    /// The `raw_value` feature can parse strings again when a first key spells
    /// its private token, including escaped spellings. For that carrier,
    /// opening delimiters and escapes bound replay depth and hidden delimiters;
    /// eight input lengths per level cover simultaneous replay strings/scratch.
    /// Work includes replay scans, map comparisons and copies before parsing.
    fn json_bound(&self, text: &str, operation: &'static str) -> Result<JsonBound, CodecError> {
        let length = u64_from_index(text.len());
        self.charge_work(length, operation)?;
        let mut values = 0_u64;
        let mut entries = 0_u64;
        let mut depth = 0_u64;
        let mut maximum = 0_u64;
        let mut openings = 0_u64;
        let mut escapes = 0_u64;
        let mut raw = false;
        let mut string = JsonStringScan::default();
        for &byte in text.as_bytes() {
            values += u64::from(matches!(byte, b'[' | b'{' | b',' | b':'));
            entries += u64::from(byte == b':');
            openings += u64::from(matches!(byte, b'[' | b'{'));
            escapes += u64::from(byte == b'\\');
            if string.active {
                raw |= string.byte(byte);
                continue;
            }
            match byte {
                b'"' => {
                    string = JsonStringScan {
                        active: true,
                        possible: true,
                        ..JsonStringScan::default()
                    }
                }
                b'[' | b'{' => {
                    depth += 1;
                    maximum = maximum.max(depth);
                }
                b']' | b'}' if depth != 0 => depth -= 1,
                _ => {}
            }
        }
        values = values
            .checked_add(1)
            .ok_or_else(|| self.tree_overflow(operation))?;
        let levels = if raw {
            maximum = openings
                .checked_add(escapes)
                .ok_or_else(|| self.tree_overflow(operation))?;
            values = values
                .checked_add(escapes)
                .ok_or_else(|| self.tree_overflow(operation))?;
            entries = entries
                .checked_add(escapes)
                .ok_or_else(|| self.tree_overflow(operation))?;
            maximum
                .checked_add(1)
                .ok_or_else(|| self.tree_overflow(operation))?
        } else {
            1
        };
        let bytes = (|| {
            values
                .checked_mul(6)?
                .checked_mul(u64_from_index(std::mem::size_of::<serde_json::Value>()))?
                .checked_add(entries.checked_mul(JSON_MAP_ENTRY_BOUND)?)?
                .checked_add(length.checked_mul(8)?.checked_mul(levels)?)?
                .checked_add(1024)
        })()
        .ok_or_else(|| self.tree_overflow(operation))?;
        Ok(JsonBound {
            values,
            entries,
            depth: maximum,
            bytes,
        })
    }

    fn parse_json_tree(
        &self,
        text: &str,
        operation: &'static str,
        interpret_raw: bool,
    ) -> Result<(serde_json::Value, ScopedReservation<'_>, JsonBound), CodecError> {
        let bound = self.json_bound(text, operation)?;
        self.charge_collection_items(bound.values, operation)?;
        let work = u64_from_index(text.len())
            .checked_mul(
                bound
                    .entries
                    .checked_add(1)
                    .ok_or_else(|| self.tree_overflow(operation))?,
            )
            .and_then(|n| n.checked_mul(bound.depth.checked_add(1)?))
            .ok_or_else(|| self.tree_overflow(operation))?;
        self.charge_work(work, operation)?;
        let reservation = self.reserve_scoped(bound.bytes, operation)?;
        let value = at_depth(self, bound.depth, operation, || {
            if interpret_raw {
                serde_json::from_str(text).map_err(|error| self.tree_malformed(error, operation))
            } else {
                use serde::Deserialize;
                let mut parser = serde_json::Deserializer::from_str(text);
                PlainJson::deserialize(&mut parser)
                    .and_then(|value| {
                        parser.end()?;
                        Ok(value.0)
                    })
                    .map_err(|error| self.tree_malformed(error, operation))
            }
        })?;
        Ok((value, reservation, bound))
    }

    /// Parses a value tree under collection, work, scoped storage and depth
    /// admission. Keep the returned reservation alive with the returned Value.
    /// `serde_json`'s own 128-level recursion ceiling also applies.
    pub fn parse_json_value(
        &self,
        text: &str,
        operation: &'static str,
    ) -> Result<(serde_json::Value, ScopedReservation<'_>), CodecError> {
        let (value, reservation, _) = self.parse_json_tree(text, operation, true)?;
        Ok((value, reservation))
    }

    /// Parses types with derived Deserialize: structs, enums, vectors, maps,
    /// strings and scalars. Custom allocating deserializers are outside this
    /// bound. The retained typed result is admitted as twice the value-tree
    /// storage plus `size_of::<T>()`; conversion charges one work unit per value.
    /// Source validation with the derived deserializer preserves duplicate-field
    /// errors that Value maps erase. Its temporary result has a separate scoped
    /// admission of twice the tree bytes plus `size_of::<T>()`; scan and conversion
    /// work are charged before validation. Typed trees use ordinary map members
    /// rather than Value's private raw-value carrier. The admission remains live.
    pub fn parse_json<T: serde::de::DeserializeOwned>(
        &self,
        text: &str,
        operation: &'static str,
    ) -> Result<T, CodecError> {
        let (_reservation, value, bound) = {
            let (value, reservation, bound) = self.parse_json_tree(text, operation, false)?;
            (reservation, value, bound)
        };
        let retained = bound
            .bytes
            .checked_mul(2)
            .and_then(|n| n.checked_add(u64_from_index(std::mem::size_of::<T>())))
            .ok_or_else(|| self.tree_overflow(operation))?;
        let validation_work = u64_from_index(text.len())
            .checked_mul(bound.values)
            .and_then(|n| n.checked_mul(bound.depth.checked_add(1)?))
            .ok_or_else(|| self.tree_overflow(operation))?;
        self.charge_work(validation_work, operation)?;
        self.charge_collection_items(bound.values, operation)?;
        {
            let _validation = self.reserve_scoped(retained, operation)?;
            at_depth(self, bound.depth, operation, || {
                let validated: T = serde_json::from_str(text)
                    .map_err(|error| self.tree_malformed(error, operation))?;
                drop(validated);
                Ok(())
            })?;
        }
        self.charge_retained(retained, operation)?;
        self.charge_collection_items(bound.values, operation)?;
        self.charge_work(bound.values, operation)?;
        at_depth(self, bound.depth, operation, || {
            serde_json::from_value(value).map_err(|error| self.tree_malformed(error, operation))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ATTRIBUTE_RECORD_BOUND;
    use crate::decode::{
        u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
    };
    use crate::CodecError;

    #[test]
    fn tree_xml_success_and_capacity_rounding() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::default())
            .expect("valid fixture");
        let tree = ctx.parse_xml("<r/>", "XML tree").expect("valid fixture");
        assert_eq!(tree.document().root_element().tag_name().name(), "r");
        let mut policy = DecodePolicy::default();
        policy.limits.max_materialized_bytes = 0;
        let (limited, _) =
            DecodeContext::from_root_bytes(b"", &arena, &policy).expect("valid fixture");
        let CodecError::ResourceLimit(limit) = limited
            .parse_xml("<r/>", "XML tree")
            .expect_err("fixture must refuse")
        else {
            panic!("byte refusal");
        };
        assert!(limit.additional >= 16 * ATTRIBUTE_RECORD_BOUND);
    }

    #[test]
    fn tree_xml_work_compares_attributes_within_each_tag() {
        let text = format!("<r>{}</r>", "<a x='0'/>".repeat(16));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_work_units = 32 * u64_from_index(text.len());
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("valid fixture");
        let tree = ctx.parse_xml(&text, "XML tree").expect("valid fixture");
        assert_eq!(tree.document().root_element().children().count(), 16);
    }

    #[test]
    fn tree_xml_unterminated_tag_keeps_attribute_work_bound() {
        let mut attributes = String::new();
        for index in 0..16 {
            std::fmt::Write::write_fmt(&mut attributes, format_args!(" x{index}='0'"))
                .expect("fixture string write");
        }
        let text = format!("<r{attributes}");
        assert_eq!(super::xml_bound(&text).max_attributes, 16);
    }

    #[test]
    fn tree_xml_resource_dimensions() {
        for dimension in [
            ResourceDimension::CollectionItems,
            ResourceDimension::MaterializedBytes,
            ResourceDimension::WorkUnits,
            ResourceDimension::RecursionDepth,
        ] {
            let mut policy = DecodePolicy::default();
            match dimension {
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = 0,
                _ => panic!("test dimension"),
            }
            let arena = DecodeArena::new();
            let (ctx, _) =
                DecodeContext::from_root_bytes(b"", &arena, &policy).expect("valid fixture");
            let CodecError::ResourceLimit(limit) = ctx
                .parse_xml("<r/>", "XML tree")
                .expect_err("fixture must refuse")
            else {
                panic!("resource refusal");
            };
            assert_eq!(limit.dimension, dimension);
            assert_eq!(ctx.resource_refusal(), Some(limit));
        }
    }

    #[test]
    fn tree_xml_malformed_and_lexical_depth() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_recursion_depth = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("valid fixture");
        assert!(matches!(
            ctx.parse_xml("<r>", "XML tree"),
            Err(CodecError::Malformed(_))
        ));
        assert!(ctx.resource_refusal().is_none());
        ctx.parse_xml(
            "<r a='>'><!-- <a> --><![CDATA[<b>]]><?pi <c> ?></r>",
            "XML tree",
        )
        .expect("valid fixture");
        assert!(
            matches!(ctx.parse_xml("<r><s/></r>", "XML tree"), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RecursionDepth)
        );
    }
    #[test]
    fn tree_malformed_diagnostic_preserves_retained_refusal() {
        for json in [false, true] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_retained_bytes = 0;
            let (ctx, _) =
                DecodeContext::from_root_bytes(b"", &arena, &policy).expect("valid fixture");
            let error = if json {
                ctx.parse_json_value("[", "JSON tree")
                    .expect_err("fixture must refuse")
            } else {
                ctx.parse_xml("<r>", "XML tree")
                    .expect_err("fixture must refuse")
            };
            let CodecError::ResourceLimit(limit) = error else {
                panic!("diagnostic admission must refuse");
            };
            assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
            assert_eq!(ctx.resource_refusal(), Some(limit));
        }
    }

    #[test]
    fn invalid_json_prefix_preserves_collection_refusal() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 2;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("valid fixture");
        let error = ctx
            .parse_json_value("[0,0,0,", "JSON nodes")
            .expect_err("fixture must refuse");
        let CodecError::ResourceLimit(limit) = error else {
            panic!("invalid prefix must preserve its admission refusal");
        };
        assert_eq!(limit.operation, "JSON nodes");
        assert_eq!(Some(limit), ctx.resource_refusal());
    }

    #[derive(Debug, serde::Deserialize, PartialEq)]
    struct JsonRecord {
        name: String,
        values: Vec<u64>,
    }

    #[test]
    fn tree_json_value_success_and_capacity_rounding() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::default())
            .expect("valid fixture");
        for text in ["[0]", "{}", "0", "\"a\""] {
            let (value, _scope) = ctx
                .parse_json_value(text, "JSON tree")
                .expect("valid fixture");
            assert_eq!(
                value,
                serde_json::from_str::<serde_json::Value>(text).expect("valid fixture")
            );
            let bound = ctx.json_bound(text, "JSON bound").expect("valid fixture");
            assert!(
                bound.bytes >= 4 * u64_from_index(std::mem::size_of::<serde_json::Value>()) + 1024
            );
        }
    }

    #[test]
    fn tree_json_typed_success() {
        let ctx = crate::decode::DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &ctx, &DecodePolicy::default())
            .expect("valid fixture");
        let record: JsonRecord = ctx
            .parse_json(r#"{"name":"r","values":[0]}"#, "typed JSON tree")
            .expect("valid fixture");
        assert_eq!(
            record,
            JsonRecord {
                name: "r".into(),
                values: vec![0]
            }
        );
    }

    #[test]
    fn tree_json_resource_dimensions() {
        for typed in [false, true] {
            for dimension in [
                ResourceDimension::CollectionItems,
                ResourceDimension::MaterializedBytes,
                ResourceDimension::WorkUnits,
                ResourceDimension::RecursionDepth,
                ResourceDimension::RetainedBytes,
            ] {
                if !typed && dimension == ResourceDimension::RetainedBytes {
                    continue;
                }
                let mut policy = DecodePolicy::default();
                match dimension {
                    ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                    ResourceDimension::MaterializedBytes => {
                        policy.limits.max_materialized_bytes = 0;
                    }
                    ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                    ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = 0,
                    ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
                    _ => panic!("test dimension"),
                }
                let arena = DecodeArena::new();
                let (ctx, _) =
                    DecodeContext::from_root_bytes(b"", &arena, &policy).expect("valid fixture");
                let error = if typed {
                    ctx.parse_json::<Vec<u64>>("[0]", "typed JSON tree")
                        .expect_err("fixture must refuse")
                } else {
                    ctx.parse_json_value("[0]", "JSON tree")
                        .expect_err("fixture must refuse")
                };
                let CodecError::ResourceLimit(limit) = error else {
                    panic!("resource refusal");
                };
                assert_eq!(limit.dimension, dimension);
                assert_eq!(ctx.resource_refusal(), Some(limit));
            }
        }
    }

    #[test]
    fn tree_json_malformed_preserves_parse_route() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::default())
            .expect("valid fixture");
        assert!(matches!(
            ctx.parse_json_value("[", "JSON tree"),
            Err(CodecError::Malformed(_))
        ));
        assert!(matches!(
            ctx.parse_json::<JsonRecord>("[", "typed JSON tree"),
            Err(CodecError::Malformed(_))
        ));
        assert!(matches!(
            ctx.parse_json::<JsonRecord>("{}", "typed JSON tree"),
            Err(CodecError::Malformed(_))
        ));
        assert!(ctx.resource_refusal().is_none());
    }

    #[test]
    fn tree_json_typed_preserves_ignored_raw_carrier_extensions() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::default())
            .expect("valid fixture");
        let text =
            r#"{"name":"r","values":[],"extension":{"$serde_json::private::RawValue":"not-json"}}"#;
        let record: JsonRecord = ctx
            .parse_json(text, "typed JSON tree")
            .expect("valid fixture");
        assert_eq!(
            record,
            JsonRecord {
                name: "r".into(),
                values: Vec::new()
            }
        );
    }

    #[test]
    fn tree_json_typed_preserves_duplicate_field_errors() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::default())
            .expect("valid fixture");
        let text = r#"{"name":"a","name":"b","values":[]}"#;
        assert!(matches!(
            ctx.parse_json::<JsonRecord>(text, "typed JSON tree"),
            Err(CodecError::Malformed(_))
        ));
        assert!(ctx.resource_refusal().is_none());
        let (value, _scope) = ctx
            .parse_json_value(text, "JSON tree")
            .expect("valid fixture");
        assert_eq!(value["name"], "b");
    }

    #[test]
    fn tree_json_lexical_and_raw_carrier_depth() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_recursion_depth = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("valid fixture");
        ctx.parse_json_value(r#"["[[["]"#, "JSON tree")
            .expect("valid fixture");
        let text = r#"{"$serde_json::private::RawValue":"\u005b0\u005d"}"#;
        assert!(
            matches!(ctx.parse_json_value(text, "JSON tree"), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RecursionDepth)
        );
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::default())
            .expect("valid fixture");
        let (value, _scope) = ctx
            .parse_json_value(text, "JSON tree")
            .expect("valid fixture");
        assert_eq!(value, serde_json::json!([0]));
        let escaped_key = r#"{"\u0024serde_json::private::RawValue":"[0]"}"#;
        let bound = ctx
            .json_bound(escaped_key, "JSON bound")
            .expect("valid fixture");
        assert!(bound.depth >= 2);
    }
}
