// SPDX-License-Identifier: Apache-2.0
//! XML and JSON parsing with nesting depth charged to the decode session.
//!
//! Each parser charges the document's nesting depth through
//! [`DecodeContext::enter_nested`], so a deep document refuses with the
//! session's recursion-depth dimension before code walks it recursively.
//! A scan of the text bounds the tree's storage before parsing. A tree that
//! could need more than the per-expansion allowance is
//! [`CodecError::NotImplemented`] and leaves the session open.

use std::collections::HashSet;

use super::{u64_from_index, DecodeContext, DepthGuard};
use crate::CodecError;

/// Attributes in one XML tag. roxmltree compares each attribute with the
/// earlier ones in its tag.
const MAX_XML_TAG_ATTRIBUTES: u64 = 256;
/// Distinct namespace prefixes declared in one XML text. roxmltree searches
/// the namespaces in scope for each element and prefixed attribute, and
/// copies them into each element that declares a namespace.
const MAX_XML_NAMESPACE_PREFIXES: u64 = 256;
/// Bounds one roxmltree 0.21.1 `NodeData` (four links, the node kind with an
/// inline expanded name, and the source range), the shared-string header of
/// owned text, the parser's per-node scratch and this module's depth slot.
const XML_NODE_BYTES: u64 = 160;
/// Bounds one roxmltree 0.21.1 `AttributeData` or `TempAttributeData` with
/// the shared-string header of an owned value.
const XML_ATTRIBUTE_BYTES: u64 = 128;
/// Bounds one roxmltree 0.21.1 `Namespace` with its sorted index.
const XML_NAMESPACE_BYTES: u64 = 64;
/// Parser state, initial capacities, and the temporary attributes of one
/// tag.
const XML_FIXED_BYTES: u64 = 1024 + 2 * MAX_XML_TAG_ATTRIBUTES * XML_ATTRIBUTE_BYTES;
/// One 32-byte `serde_json::Value` in an array that may hold twice its
/// length.
const JSON_VALUE_BYTES: u64 = 64;
/// Bounds one `BTreeMap<String, Value>` node of `serde_json` 1.0.151 without
/// `preserve_order`: eleven keys and values, twelve edges and parent links.
const JSON_MAP_NODE_BYTES: u64 = 768;
/// The allocation of one short JSON string, with allocator rounding.
const JSON_STRING_BYTES: u64 = 32;
/// Deserializer state and initial capacities.
const JSON_FIXED_BYTES: u64 = 1024;

impl DecodeContext<'_> {
    /// Holds `depth` nesting guards for the duration of `run`.
    fn at_depth<T>(
        &self,
        depth: u64,
        operation: &'static str,
        run: impl FnOnce() -> Result<T, CodecError>,
    ) -> Result<T, CodecError> {
        let mut guards: Vec<DepthGuard<'_>> = Vec::new();
        for _ in 0..depth {
            guards.push(self.enter_nested(operation)?);
        }
        let result = run();
        drop(guards);
        result
    }

    fn tree_malformed(error: impl std::fmt::Display) -> CodecError {
        CodecError::malformed(format_args!("{error}"))
    }

    /// The session holds the document's depth, so the parser's own
    /// recursion limit does not apply.
    fn deserialize_json<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, CodecError> {
        let mut deserializer = serde_json::Deserializer::from_str(text);
        deserializer.disable_recursion_limit();
        T::deserialize(&mut deserializer)
            .and_then(|value| deserializer.end().map(|()| value))
            .map_err(Self::tree_malformed)
    }

    /// Checks that a `serde_json::Value` tree of `text` fits the
    /// per-expansion allowance and returns the depth the parse can reach.
    fn admit_json(&self, text: &str, operation: &'static str) -> Result<u64, CodecError> {
        let shape = json_shape(text);
        let allowance = self.per_expand_allowance();
        if json_storage_bytes(u64_from_index(text.len()), &shape)
            .is_none_or(|bytes| bytes > allowance)
        {
            return Err(tree_too_large("JSON", allowance, operation));
        }
        Ok(shape.depth)
    }

    /// Parses XML with DTD processing disabled and charges its element depth.
    /// The scan bounds the tree's attributes, namespaces and text copies, and
    /// the rest of the per-expansion allowance limits its nodes.
    pub fn parse_xml<'input>(
        &self,
        text: &'input str,
        operation: &'static str,
    ) -> Result<roxmltree::Document<'input>, CodecError> {
        let shape = xml_shape(text);
        if shape.tag_attributes > MAX_XML_TAG_ATTRIBUTES {
            return Err(CodecError::NotImplemented(format!(
                "{operation}: an XML tag has more than {MAX_XML_TAG_ATTRIBUTES} attributes"
            )));
        }
        let prefixes = u64_from_index(shape.prefixes.len());
        if prefixes > MAX_XML_NAMESPACE_PREFIXES {
            return Err(CodecError::NotImplemented(format!(
                "{operation}: the XML text declares more than \
                 {MAX_XML_NAMESPACE_PREFIXES} namespace prefixes"
            )));
        }
        let allowance = self.per_expand_allowance();
        let nodes_limit = xml_storage_bytes(u64_from_index(text.len()), &shape, prefixes)
            .and_then(|bytes| allowance.checked_sub(bytes))
            .map(|rest| rest / (2 * XML_NODE_BYTES))
            .ok_or_else(|| tree_too_large("XML", allowance, operation))?;
        // roxmltree node ids are `u32`; a larger limit leaves the parser's own.
        let mut options = roxmltree::ParsingOptions::default();
        if let Ok(limit) = u32::try_from(nodes_limit) {
            options.nodes_limit = limit;
        }
        let document =
            roxmltree::Document::parse_with_options(text, options).map_err(
                |error| match error {
                    roxmltree::Error::NodesLimitReached => {
                        tree_too_large("XML", allowance, operation)
                    }
                    error => Self::tree_malformed(error),
                },
            )?;
        // Node ids follow document order, so a parent's depth is known
        // before its children's.
        let mut depths: Vec<u64> = Vec::new();
        let mut maximum = 0_u64;
        for node in document.root().descendants() {
            let depth = node
                .parent()
                .and_then(|parent| depths.get(parent.id().get_usize()))
                .map_or(0, |parent| parent + 1);
            maximum = maximum.max(depth);
            depths.push(depth);
        }
        self.at_depth(maximum, operation, || Ok(document))
    }

    /// Parses a JSON value tree after charging its nesting depth.
    pub fn parse_json_value(
        &self,
        text: &str,
        operation: &'static str,
    ) -> Result<serde_json::Value, CodecError> {
        let depth = self.admit_json(text, operation)?;
        self.at_depth(depth, operation, || Self::deserialize_json(text))
    }

    /// Parses a type with a derived `Deserialize` after charging the source's
    /// nesting depth. Its storage is bounded as a `serde_json::Value` tree of
    /// the same text.
    pub fn parse_json<T: serde::de::DeserializeOwned>(
        &self,
        text: &str,
        operation: &'static str,
    ) -> Result<T, CodecError> {
        let depth = self.admit_json(text, operation)?;
        self.at_depth(depth, operation, || Self::deserialize_json(text))
    }
}

fn tree_too_large(kind: &str, allowance: u64, operation: &str) -> CodecError {
    CodecError::NotImplemented(format!(
        "{operation}: the {kind} tree could need more than the \
         {allowance}-byte per-expansion allowance"
    ))
}

#[derive(Clone, Copy)]
enum XmlScan {
    Text,
    Tag { quote: u8 },
    Comment,
    Cdata,
    Instruction,
    Declaration,
}

/// Upper bounds on the parts of an XML text that roxmltree stores apart
/// from its nodes.
#[derive(Default)]
struct XmlShape<'text> {
    /// `=` outside quoted values in tags, one per attribute.
    attributes: u64,
    /// The most attributes in one tag.
    tag_attributes: u64,
    /// `xmlns` outside quoted values in tags, one per namespace declaration.
    declarations: u64,
    /// Names that follow `xmlns:` in tags.
    prefixes: HashSet<&'text [u8]>,
}

/// Scans `text` once. Comments, CDATA, processing instructions and
/// declarations end at their first terminator, never after the parser's, so
/// the counts do not fall below what the parser stores.
fn xml_shape(text: &str) -> XmlShape<'_> {
    let bytes = text.as_bytes();
    let mut shape = XmlShape::default();
    let mut state = XmlScan::Text;
    let mut tag_attributes = 0_u64;
    let mut prefix_start = None::<usize>;
    for (index, &byte) in bytes.iter().enumerate() {
        let tail = bytes.get(index..).unwrap_or_default();
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
                    tag_attributes = 0;
                    XmlScan::Tag { quote: 0 }
                }
            }
            XmlScan::Tag { quote: 0 } => {
                let delimiter =
                    byte.is_ascii_whitespace() || matches!(byte, b'=' | b'/' | b'>' | b'\'' | b'"');
                if let Some(start) = prefix_start.filter(|_| delimiter) {
                    shape
                        .prefixes
                        .insert(bytes.get(start..index).unwrap_or_default());
                    prefix_start = None;
                }
                if tail.starts_with(b"xmlns") {
                    shape.declarations += 1;
                    if prefix_start.is_none() && tail.get(5) == Some(&b':') {
                        prefix_start = Some(index + 6);
                    }
                }
                match byte {
                    b'\'' | b'"' => XmlScan::Tag { quote: byte },
                    b'>' => XmlScan::Text,
                    b'=' => {
                        tag_attributes += 1;
                        shape.attributes += 1;
                        shape.tag_attributes = shape.tag_attributes.max(tag_attributes);
                        XmlScan::Tag { quote: 0 }
                    }
                    _ => XmlScan::Tag { quote: 0 },
                }
            }
            XmlScan::Tag { quote } if byte == quote => XmlScan::Tag { quote: 0 },
            XmlScan::Comment if tail.starts_with(b"-->") => XmlScan::Text,
            XmlScan::Cdata if tail.starts_with(b"]]>") => XmlScan::Text,
            XmlScan::Instruction if tail.starts_with(b"?>") => XmlScan::Text,
            XmlScan::Declaration if byte == b'>' => XmlScan::Text,
            other => other,
        };
    }
    if let Some(start) = prefix_start {
        shape
            .prefixes
            .insert(bytes.get(start..).unwrap_or_default());
    }
    shape
}

/// Bytes that a roxmltree 0.21.1 tree of `shape` needs besides its nodes.
/// Vectors may hold twice their length. Each element that declares a
/// namespace stores its declarations and copies the namespaces in scope: at
/// most one per declared prefix, the default namespace and `xml`. Owned text
/// and attribute values, the pieces they are joined from, and normalization
/// scratch each stay within the text length.
fn xml_storage_bytes(length: u64, shape: &XmlShape<'_>, prefixes: u64) -> Option<u64> {
    let namespace_indices = shape
        .declarations
        .checked_mul(prefixes.checked_add(3)?)?
        .checked_add(1)?;
    XML_FIXED_BYTES
        .checked_add(length.checked_mul(3)?)?
        .checked_add(shape.attributes.checked_mul(2 * XML_ATTRIBUTE_BYTES)?)?
        .checked_add(
            shape
                .declarations
                .checked_add(1)?
                .checked_mul(2 * XML_NAMESPACE_BYTES)?,
        )?
        .checked_add(namespace_indices.checked_mul(2 * 2)?)
}

/// Upper bounds on a JSON parse: the nesting depth it can reach and the
/// parts of a `serde_json::Value` tree.
#[derive(Default)]
struct JsonShape {
    depth: u64,
    /// Array elements and the root value.
    values: u64,
    /// Object members.
    members: u64,
    maps: u64,
    strings: u64,
}

/// Scans `text` once, counting delimiters outside strings. With the
/// `raw_value` feature, a string that spells the private raw-value token is
/// parsed again, so its escaped delimiters can add depth and values that the
/// visible delimiters do not show; when such a string occurs, every delimiter
/// and escape counts.
fn json_shape(text: &str) -> JsonShape {
    let mut depth = 0_u64;
    let mut visible = JsonShape {
        values: 1,
        ..JsonShape::default()
    };
    let mut every = JsonShape {
        values: 1,
        ..JsonShape::default()
    };
    let mut raw = false;
    let mut string = JsonStringScan::default();
    for &byte in text.as_bytes() {
        let opening = u64::from(matches!(byte, b'[' | b'{'));
        let escape = u64::from(byte == b'\\');
        every.depth += opening + escape;
        every.values += u64::from(byte == b',') + opening + escape;
        every.members += u64::from(byte == b':') + escape;
        every.maps += u64::from(byte == b'{') + escape;
        every.strings += u64::from(byte == b'"') + escape;
        if string.active {
            raw |= string.byte(byte);
            continue;
        }
        match byte {
            b'"' => {
                visible.strings += 1;
                string = JsonStringScan {
                    active: true,
                    possible: true,
                    ..JsonStringScan::default()
                }
            }
            b'[' | b'{' => {
                depth += 1;
                visible.depth = visible.depth.max(depth);
                visible.values += 1;
                visible.maps += u64::from(byte == b'{');
            }
            b']' | b'}' if depth != 0 => depth -= 1,
            b',' => visible.values += 1,
            b':' => visible.members += 1,
            _ => {}
        }
    }
    if raw {
        every
    } else {
        visible
    }
}

/// Bytes that a `serde_json::Value` tree of `shape` may need. String
/// contents, the deserializer's scratch buffer, which may hold twice the
/// longest string, and a replayed raw value each stay within the text
/// length. Every map node but a root holds at least five members.
fn json_storage_bytes(length: u64, shape: &JsonShape) -> Option<u64> {
    let map_nodes = shape.maps.checked_add(shape.members / 5)?;
    JSON_FIXED_BYTES
        .checked_add(length.checked_mul(4)?)?
        .checked_add(shape.values.checked_mul(JSON_VALUE_BYTES)?)?
        .checked_add(map_nodes.checked_mul(JSON_MAP_NODE_BYTES)?)?
        .checked_add(shape.strings.checked_mul(JSON_STRING_BYTES)?)
}

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

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::{json_shape, xml_shape, MAX_XML_NAMESPACE_PREFIXES, MAX_XML_TAG_ATTRIBUTES};
    use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use crate::CodecError;

    #[derive(Debug, serde::Deserialize, PartialEq)]
    struct JsonRecord {
        name: String,
        values: Vec<u64>,
    }

    fn with_depth<T>(depth: u64, run: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_recursion_depth = depth;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("valid fixture");
        run(&ctx)
    }

    fn with_expand_ceiling<T>(bytes: u64, run: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_decompressed_bytes_per_expand = bytes;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("valid fixture");
        run(&ctx)
    }

    fn is_depth_refusal<T: std::fmt::Debug>(result: &Result<T, CodecError>) -> bool {
        matches!(result, Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RecursionDepth)
    }

    fn is_unsupported<T: std::fmt::Debug>(result: &Result<T, CodecError>, text: &str) -> bool {
        matches!(result, Err(CodecError::NotImplemented(message)) if message.contains(text))
    }

    #[test]
    fn xml_parses_and_rejects_malformed_text() {
        with_depth(256, |ctx| {
            let document = ctx
                .parse_xml("<r a=\"1\"><c/></r>", "XML")
                .expect("valid XML");
            assert_eq!(document.root_element().attribute("a"), Some("1"));
            assert!(matches!(
                ctx.parse_xml("<r>", "XML"),
                Err(CodecError::Malformed(_))
            ));
        });
    }

    #[test]
    fn xml_depth_is_charged_to_the_session() {
        let nested = "<a><b><c/></b></a>";
        with_depth(3, |ctx| assert!(ctx.parse_xml(nested, "XML").is_ok()));
        with_depth(2, |ctx| {
            assert!(is_depth_refusal(&ctx.parse_xml(nested, "XML")));
        });
    }

    #[test]
    fn xml_shape_counts_tag_attributes_and_declared_prefixes() {
        let shape = xml_shape(
            "<?xml version=\"1.0\"?><r xmlns=\"u\" xmlns:p=\"u\" a=\"x=y\"><!-- b=c xmlns:q='u' -->\
             <p:c xmlns:p ='v' d='1'/><![CDATA[e=f]]>g=h</r>",
        );
        assert_eq!(shape.attributes, 5);
        assert_eq!(shape.tag_attributes, 3);
        assert_eq!(shape.declarations, 3);
        assert_eq!(shape.prefixes.into_iter().collect::<Vec<_>>(), [&b"p"[..]]);
    }

    #[test]
    fn xml_tree_past_the_per_expand_allowance_is_unsupported() {
        let document = |elements: usize| format!("<r>{}</r>", "<a/>".repeat(elements));
        with_expand_ceiling(1 << 20, |ctx| {
            assert!(ctx.parse_xml(&document(1 << 10), "XML").is_ok());
            assert!(is_unsupported(
                &ctx.parse_xml(&document(1 << 14), "XML"),
                "the XML tree could need more than the 1048576-byte per-expansion allowance"
            ));
            assert_eq!(ctx.resource_refusal(), None);
        });
    }

    #[test]
    fn xml_tag_attributes_past_their_limit_are_unsupported() {
        let tag = |attributes: u64| {
            (0..attributes).fold(String::from("<r"), |mut tag, index| {
                write!(tag, " a{index}=''").expect("write attribute");
                tag + if index + 1 == attributes { "/>" } else { "" }
            })
        };
        with_depth(256, |ctx| {
            assert!(ctx.parse_xml(&tag(MAX_XML_TAG_ATTRIBUTES), "XML").is_ok());
            assert!(is_unsupported(
                &ctx.parse_xml(&tag(MAX_XML_TAG_ATTRIBUTES + 1), "XML"),
                "an XML tag has more than 256 attributes"
            ));
            assert_eq!(ctx.resource_refusal(), None);
        });
    }

    #[test]
    fn xml_namespace_prefixes_past_their_limit_are_unsupported() {
        let document = |prefixes: u64| {
            let mut text = String::from("<r>");
            for index in 0..prefixes {
                write!(text, "<c xmlns:p{index}='u'/>").expect("write declaration");
            }
            text + "</r>"
        };
        with_depth(256, |ctx| {
            assert!(ctx
                .parse_xml(&document(MAX_XML_NAMESPACE_PREFIXES), "XML")
                .is_ok());
            assert!(is_unsupported(
                &ctx.parse_xml(&document(MAX_XML_NAMESPACE_PREFIXES + 1), "XML"),
                "the XML text declares more than 256 namespace prefixes"
            ));
            assert_eq!(ctx.resource_refusal(), None);
        });
    }

    #[test]
    fn json_depth_follows_the_session_past_the_parser_limit() {
        let nested = format!("{}{}", "[".repeat(200), "]".repeat(200));
        with_depth(256, |ctx| {
            assert!(ctx.parse_json_value(&nested, "JSON").is_ok());
            assert!(ctx.parse_json::<serde_json::Value>(&nested, "JSON").is_ok());
        });
        with_depth(199, |ctx| {
            assert!(is_depth_refusal(&ctx.parse_json_value(&nested, "JSON")));
        });
    }

    #[test]
    fn json_tree_past_the_per_expand_allowance_is_unsupported() {
        let document = |maps: usize| format!("[{}]", vec![r#"{"a":0}"#; maps].join(","));
        with_expand_ceiling(1 << 20, |ctx| {
            assert!(ctx.parse_json_value(&document(1 << 8), "JSON").is_ok());
            let message =
                "the JSON tree could need more than the 1048576-byte per-expansion allowance";
            assert!(is_unsupported(
                &ctx.parse_json_value(&document(1 << 11), "JSON"),
                message
            ));
            assert!(is_unsupported(
                &ctx.parse_json::<serde_json::Value>(&document(1 << 11), "JSON"),
                message
            ));
            assert_eq!(ctx.resource_refusal(), None);
        });
    }

    #[test]
    fn json_value_and_typed_parse() {
        with_depth(256, |ctx| {
            let value = ctx
                .parse_json_value(r#"{"a":[1,2]}"#, "JSON")
                .expect("valid JSON");
            assert_eq!(value, serde_json::json!({"a": [1, 2]}));
            let record: JsonRecord = ctx
                .parse_json(r#"{"name":"r","values":[3]}"#, "JSON")
                .expect("valid JSON");
            assert_eq!(
                record,
                JsonRecord {
                    name: "r".into(),
                    values: vec![3],
                }
            );
            assert!(matches!(
                ctx.parse_json_value("{", "JSON"),
                Err(CodecError::Malformed(_))
            ));
        });
    }

    #[test]
    fn typed_json_keeps_duplicate_field_errors() {
        with_depth(256, |ctx| {
            let text = r#"{"name":"a","name":"b","values":[]}"#;
            assert!(matches!(
                ctx.parse_json::<JsonRecord>(text, "JSON"),
                Err(CodecError::Malformed(_))
            ));
            let value = ctx.parse_json_value(text, "JSON").expect("valid JSON");
            assert_eq!(value["name"], "b");
        });
    }

    #[test]
    fn typed_json_ignores_raw_carrier_extensions() {
        with_depth(256, |ctx| {
            let text = r#"{"name":"r","values":[],"extension":{"$serde_json::private::RawValue":"not-json"}}"#;
            let record: JsonRecord = ctx.parse_json(text, "JSON").expect("valid JSON");
            assert_eq!(record.name, "r");
        });
    }

    #[test]
    fn json_shape_counts_delimiters_outside_strings_and_raw_replay() {
        assert_eq!(json_shape(r#"["[[["]"#).depth, 1);
        let shape = json_shape(r#"{"a":[[1,2]],"b":"x,y:z"}"#);
        assert_eq!(shape.depth, 3);
        assert_eq!(shape.values, 6);
        assert_eq!(shape.members, 2);
        assert_eq!(shape.maps, 1);
        assert_eq!(shape.strings, 3);
        let raw = r#"{"$serde_json::private::RawValue":"\u005b0\u005d"}"#;
        assert!(json_shape(raw).depth >= 2);
        let escaped_key = r#"{"\u0024serde_json::private::RawValue":"[0]"}"#;
        assert!(json_shape(escaped_key).depth >= 2);
        with_depth(1, |ctx| {
            assert!(is_depth_refusal(&ctx.parse_json_value(raw, "JSON")));
        });
        with_depth(256, |ctx| {
            let value = ctx
                .parse_json_value(raw, "JSON")
                .expect("valid raw carrier");
            assert_eq!(value, serde_json::json!([0]));
        });
    }
}
