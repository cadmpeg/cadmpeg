// SPDX-License-Identifier: Apache-2.0
//! XML and JSON parsing with nesting depth charged to the decode session.
//!
//! Each parser charges the document's nesting depth through
//! [`DecodeContext::enter_nested`] before it parses, so a deep document
//! refuses with the session's recursion-depth dimension before the parser or
//! other code walks it recursively. A scan of the text gives the depth and
//! bounds the tree's storage. A tree that could need more than the
//! per-expansion allowance raises a part-scoped resource refusal. The owning
//! part transaction acknowledges that refusal before processing siblings.

use std::collections::HashSet;

use super::{u64_from_index, DecodeContext, DepthGuard, ResourceDimension};
use crate::CodecError;

/// Attributes in one XML tag. roxmltree compares each attribute with the
/// earlier ones in its tag.
const MAX_XML_TAG_ATTRIBUTES: u64 = 256;
/// Distinct namespace prefixes declared in one XML text. roxmltree searches
/// the namespaces in scope for each element and prefixed attribute, and
/// copies them into each element that declares a namespace.
const MAX_XML_NAMESPACE_PREFIXES: u64 = 256;
/// One roxmltree 0.21.1 `NodeData` with the `positions` feature: four links,
/// the node kind with an inline expanded name, and the source range.
const XML_NODE_BYTES: u64 = 72;
/// One roxmltree 0.21.1 `AttributeData` with the `positions` feature.
const XML_ATTRIBUTE_BYTES: u64 = 72;
/// One roxmltree 0.21.1 `TempAttributeData`.
const XML_TEMP_ATTRIBUTE_BYTES: u64 = 80;
/// Bounds one roxmltree 0.21.1 `Namespace` with its sorted index.
const XML_NAMESPACE_BYTES: u64 = 64;
/// One nesting level: a slot in roxmltree's prefix stack (16 bytes) and in
/// its list of nodes that wait for a subtree link (4 bytes), and one depth
/// guard (8 bytes). A growing vector can hold its old and its doubled
/// storage at once, three times its length.
const XML_LEVEL_BYTES: u64 = 3 * (16 + 4 + 8);
/// One text piece that roxmltree keeps to merge with adjacent CDATA, a
/// `Cow<str>` in a growing vector.
const XML_TEXT_PIECE_BYTES: u64 = 3 * 24;
/// The initial text buffer and the padded shared-string header of one
/// copied text or attribute value.
const XML_COPY_BYTES: u64 = 32 + 24;
/// While roxmltree copies one text or value, it can hold the growing
/// buffer, the merged pieces, their join and two shared strings. None is
/// longer than the source of that text, and earlier copies keep one shared
/// string each.
const XML_COPY_FACTOR: u64 = 5;
/// Parser state, the scan's prefix set, and the temporary attributes of one
/// tag in a growing vector.
const XML_FIXED_BYTES: u64 =
    1024 + 16 * 1024 + 3 * MAX_XML_TAG_ATTRIBUTES / 2 * XML_TEMP_ATTRIBUTE_BYTES;
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
        let storage = json_storage_bytes(u64_from_index(text.len()), &shape).ok_or_else(|| {
            self.refuse_part_limit(
                ResourceDimension::RetainedBytes,
                allowance,
                0,
                u64::MAX,
                operation,
            )
        })?;
        if storage > allowance {
            return Err(self.refuse_part_limit(
                ResourceDimension::RetainedBytes,
                allowance,
                0,
                storage,
                operation,
            ));
        }
        Ok(shape.depth)
    }

    /// Parses XML with DTD processing disabled and charges its node depth.
    /// The scan bounds the storage that roxmltree reserves and copies, and
    /// the rest of the per-expansion allowance limits its nodes.
    pub fn parse_xml<'input>(
        &self,
        text: &'input str,
        operation: &'static str,
    ) -> Result<roxmltree::Document<'input>, CodecError> {
        let shape = xml_shape(text);
        if shape.tag_attributes > MAX_XML_TAG_ATTRIBUTES {
            return Err(self.refuse_part_limit(
                ResourceDimension::Entities,
                MAX_XML_TAG_ATTRIBUTES,
                0,
                shape.tag_attributes,
                operation,
            ));
        }
        let prefixes = u64_from_index(shape.prefixes.len());
        if prefixes > MAX_XML_NAMESPACE_PREFIXES {
            return Err(self.refuse_part_limit(
                ResourceDimension::Entities,
                MAX_XML_NAMESPACE_PREFIXES,
                0,
                prefixes,
                operation,
            ));
        }
        let allowance = self.per_expand_allowance();
        let storage = xml_storage_bytes(&shape, prefixes).ok_or_else(|| {
            self.refuse_part_limit(
                ResourceDimension::RetainedBytes,
                allowance,
                0,
                u64::MAX,
                operation,
            )
        })?;
        let nodes_limit = allowance
            .checked_sub(storage)
            .and_then(|rest| xml_nodes_limit(rest, shape.less_than))
            .ok_or_else(|| {
                self.refuse_part_limit(
                    ResourceDimension::RetainedBytes,
                    allowance,
                    0,
                    storage,
                    operation,
                )
            })?;
        // roxmltree node ids are `u32`; a larger limit leaves the parser's own.
        let mut options = roxmltree::ParsingOptions::default();
        if let Ok(limit) = u32::try_from(nodes_limit) {
            options.nodes_limit = limit;
        }
        // roxmltree recurses once per element level.
        self.at_depth(shape.depth, operation, || {
            roxmltree::Document::parse_with_options(text, options).map_err(|error| match error {
                roxmltree::Error::NodesLimitReached => self.refuse_part_limit(
                    ResourceDimension::Entities,
                    nodes_limit,
                    nodes_limit,
                    1,
                    operation,
                ),
                error => Self::tree_malformed(error),
            })
        })
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

#[derive(Clone, Copy)]
enum XmlScan {
    Text,
    Tag { quote: u8 },
    Comment,
    Cdata,
    Instruction,
    Declaration,
}

/// Upper bounds on what roxmltree reserves, copies and keeps per nesting
/// level while it parses an XML text.
#[derive(Default)]
struct XmlShape<'text> {
    /// `<` anywhere in the text. roxmltree reserves one node for each.
    less_than: u64,
    /// `=` anywhere in the text. roxmltree reserves one attribute for each.
    equals: u64,
    /// `=` outside quoted values in tags, one per attribute.
    attributes: u64,
    /// The most attributes in one tag.
    tag_attributes: u64,
    /// `xmlns` outside quoted values in tags, one per namespace declaration.
    declarations: u64,
    /// Names that follow `xmlns:` in tags, kept up to one past
    /// [`MAX_XML_NAMESPACE_PREFIXES`].
    prefixes: HashSet<&'text [u8]>,
    /// The deepest node, counted as roxmltree counts ancestors: the root
    /// element is one, and an element, text, CDATA, comment or instruction
    /// is one more than the element that contains it.
    depth: u64,
    /// CDATA sections. roxmltree merges each with adjacent text.
    cdata: u64,
    /// Texts and attribute values that roxmltree copies: a text run with a
    /// reference, a carriage return or CDATA, and a value with a reference,
    /// a tab or a line break.
    copies: u64,
    /// The source bytes of those texts and values.
    copied: u64,
}

impl<'text> XmlShape<'text> {
    fn copy(&mut self, copied: bool, source: &[u8]) {
        if copied {
            self.copies += 1;
            self.copied += u64_from_index(source.len());
        }
    }

    /// Counts a node inside `open` elements.
    fn contains(&mut self, open: u64) {
        self.depth = self.depth.max(open + 1);
    }

    /// Keeps one prefix past the limit, enough for the limit to refuse.
    fn prefix(&mut self, name: &'text [u8]) {
        if u64_from_index(self.prefixes.len()) <= MAX_XML_NAMESPACE_PREFIXES {
            self.prefixes.insert(name);
        }
    }
}

/// Scans `text` once. Comments, CDATA, processing instructions and
/// declarations end at their first terminator, never after the parser's, so
/// the counts do not fall below what the parser stores. A text run ends at
/// markup other than CDATA, where roxmltree stops merging text.
fn xml_shape(text: &str) -> XmlShape<'_> {
    let bytes = text.as_bytes();
    let mut shape = XmlShape::default();
    let mut state = XmlScan::Text;
    let mut tag_attributes = 0_u64;
    let mut prefix_start = None::<usize>;
    // Open elements; whether the current tag closes one; its last byte
    // outside whitespace, which is `/` for an empty element.
    let mut open = 0_u64;
    let mut closing = false;
    let mut last = 0_u8;
    // Start of the current text run or attribute value, and whether
    // roxmltree copies it.
    let mut run_start = 0_usize;
    let mut copied = false;
    for (index, &byte) in bytes.iter().enumerate() {
        shape.less_than += u64::from(byte == b'<');
        shape.equals += u64::from(byte == b'=');
        let tail = bytes.get(index..).unwrap_or_default();
        state = match state {
            XmlScan::Text if byte == b'<' => {
                if tail.starts_with(b"<![CDATA[") {
                    shape.cdata += 1;
                    shape.contains(open);
                    copied = true;
                    XmlScan::Cdata
                } else {
                    shape.copy(copied, bytes.get(run_start..index).unwrap_or_default());
                    copied = false;
                    if tail.starts_with(b"<!--") {
                        shape.contains(open);
                        XmlScan::Comment
                    } else if tail.starts_with(b"<?") {
                        shape.contains(open);
                        XmlScan::Instruction
                    } else if tail.starts_with(b"<!") {
                        XmlScan::Declaration
                    } else {
                        tag_attributes = 0;
                        closing = tail.get(1) == Some(&b'/');
                        XmlScan::Tag { quote: 0 }
                    }
                }
            }
            XmlScan::Text => {
                copied |= matches!(byte, b'&' | b'\r');
                // Text outside the root element is not a node.
                if open != 0 {
                    shape.contains(open);
                }
                XmlScan::Text
            }
            XmlScan::Tag { quote: 0 } => {
                let delimiter =
                    byte.is_ascii_whitespace() || matches!(byte, b'=' | b'/' | b'>' | b'\'' | b'"');
                if let Some(start) = prefix_start.filter(|_| delimiter) {
                    shape.prefix(bytes.get(start..index).unwrap_or_default());
                    prefix_start = None;
                }
                if tail.starts_with(b"xmlns") {
                    shape.declarations += 1;
                    if prefix_start.is_none() && tail.get(5) == Some(&b':') {
                        prefix_start = Some(index + 6);
                    }
                }
                let next = match byte {
                    b'\'' | b'"' => {
                        run_start = index + 1;
                        copied = false;
                        XmlScan::Tag { quote: byte }
                    }
                    b'>' => {
                        if closing {
                            if let Some(outer) = open.checked_sub(1) {
                                open = outer;
                            }
                        } else {
                            shape.contains(open);
                            if last != b'/' {
                                open += 1;
                            }
                        }
                        run_start = index + 1;
                        XmlScan::Text
                    }
                    b'=' => {
                        tag_attributes += 1;
                        shape.attributes += 1;
                        shape.tag_attributes = shape.tag_attributes.max(tag_attributes);
                        XmlScan::Tag { quote: 0 }
                    }
                    _ => XmlScan::Tag { quote: 0 },
                };
                if !byte.is_ascii_whitespace() {
                    last = byte;
                }
                next
            }
            XmlScan::Tag { quote } if byte == quote => {
                shape.copy(copied, bytes.get(run_start..index).unwrap_or_default());
                copied = false;
                last = byte;
                XmlScan::Tag { quote: 0 }
            }
            XmlScan::Tag { quote } => {
                copied |= matches!(byte, b'&' | b'\t' | b'\n' | b'\r');
                XmlScan::Tag { quote }
            }
            XmlScan::Comment if tail.starts_with(b"-->") => {
                run_start = index + 3;
                XmlScan::Text
            }
            XmlScan::Cdata if tail.starts_with(b"]]>") => XmlScan::Text,
            XmlScan::Instruction if tail.starts_with(b"?>") => {
                run_start = index + 2;
                XmlScan::Text
            }
            XmlScan::Declaration if byte == b'>' => {
                run_start = index + 1;
                XmlScan::Text
            }
            other => other,
        };
    }
    // A text run or CDATA section that the text ends in.
    shape.copy(copied, bytes.get(run_start..).unwrap_or_default());
    if let Some(start) = prefix_start {
        shape.prefix(bytes.get(start..).unwrap_or_default());
    }
    shape
}

/// Bytes that roxmltree 0.21.1 and this module hold while they parse a text
/// of `shape`, besides nodes past the reserved slots. roxmltree reserves a
/// node for each `<` and an attribute for each `=` before it parses, and
/// shrinking the attribute vector copies the attributes. Each element that
/// declares a namespace stores its declarations and copies the namespaces
/// in scope: at most one per declared prefix, the default namespace and
/// `xml`.
fn xml_storage_bytes(shape: &XmlShape<'_>, prefixes: u64) -> Option<u64> {
    let namespace_indices = shape
        .declarations
        .checked_mul(prefixes.checked_add(3)?)?
        .checked_add(1)?;
    XML_FIXED_BYTES
        .checked_add(shape.less_than.checked_mul(XML_NODE_BYTES)?)?
        .checked_add(
            shape
                .equals
                .checked_add(shape.attributes)?
                .checked_mul(XML_ATTRIBUTE_BYTES)?,
        )?
        .checked_add(shape.depth.checked_add(2)?.checked_mul(XML_LEVEL_BYTES)?)?
        .checked_add(
            shape
                .cdata
                .checked_mul(2)?
                .checked_add(1)?
                .checked_mul(XML_TEXT_PIECE_BYTES)?,
        )?
        .checked_add(shape.copied.checked_mul(XML_COPY_FACTOR)?)?
        .checked_add(shape.copies.checked_mul(XML_COPY_BYTES)?)?
        .checked_add(
            shape
                .declarations
                .checked_add(1)?
                .checked_mul(2 * XML_NAMESPACE_BYTES)?,
        )?
        .checked_add(namespace_indices.checked_mul(3 * 2)?)
}

/// The most nodes whose storage fits in `rest` bytes besides the `reserved`
/// slots. Up to `reserved` nodes, shrinking the node vector copies each node
/// once. Past them the vector doubles, and growing or shrinking it can hold
/// three slots per node.
fn xml_nodes_limit(rest: u64, reserved: u64) -> Option<u64> {
    let grown = rest.checked_add(reserved.checked_mul(XML_NODE_BYTES)?)? / (3 * XML_NODE_BYTES);
    Some(if grown > reserved {
        grown
    } else {
        reserved.min(rest / XML_NODE_BYTES)
    })
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

    use super::{
        json_shape, xml_nodes_limit, xml_shape, xml_storage_bytes, MAX_XML_NAMESPACE_PREFIXES,
        MAX_XML_TAG_ATTRIBUTES, XML_NODE_BYTES,
    };
    use crate::decode::{
        u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
    };
    use crate::CodecError;

    /// Per-thread heap observation for the storage-bound tests.
    mod heap {
        use std::alloc::{GlobalAlloc, Layout, System};
        use std::cell::Cell;

        use crate::decode::u64_from_index;

        thread_local! {
            static TRACK: Cell<bool> = const { Cell::new(false) };
            static LIVE: Cell<i128> = const { Cell::new(0) };
            static PEAK: Cell<i128> = const { Cell::new(0) };
        }

        struct PeakAllocator;

        #[global_allocator]
        static ALLOCATOR: PeakAllocator = PeakAllocator;

        /// A reallocation holds the old and the new block at once.
        fn record(acquired: usize, released: usize) {
            if TRACK.try_with(Cell::get).unwrap_or(false) {
                let _recorded = LIVE.try_with(|live| {
                    let held = live.get() + i128::from(u64_from_index(acquired));
                    let _peaked = PEAK.try_with(|peak| peak.set(peak.get().max(held)));
                    live.set(held - i128::from(u64_from_index(released)));
                });
            }
        }

        unsafe impl GlobalAlloc for PeakAllocator {
            unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
                let pointer = unsafe { System.alloc(layout) };
                if !pointer.is_null() {
                    record(layout.size(), 0);
                }
                pointer
            }

            unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
                let pointer = unsafe { System.alloc_zeroed(layout) };
                if !pointer.is_null() {
                    record(layout.size(), 0);
                }
                pointer
            }

            unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
                let resized = unsafe { System.realloc(pointer, layout, new_size) };
                if !resized.is_null() {
                    record(new_size, layout.size());
                }
                resized
            }

            unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
                unsafe { System.dealloc(pointer, layout) };
                record(0, layout.size());
            }
        }

        /// The most heap bytes that `action` held at once on this thread,
        /// besides what it returns.
        pub(super) fn peak<R>(action: impl FnOnce() -> R) -> (R, u64) {
            LIVE.with(|live| live.set(0));
            PEAK.with(|peak| peak.set(0));
            TRACK.with(|track| track.set(true));
            let result = action();
            TRACK.with(|track| track.set(false));
            let peak = u64::try_from(PEAK.with(Cell::get)).expect("peak fits u64");
            (result, peak)
        }
    }

    /// Node storage as [`xml_nodes_limit`] inverts it.
    fn xml_node_storage(nodes: u64, reserved: u64) -> u64 {
        if nodes <= reserved {
            nodes * XML_NODE_BYTES
        } else {
            3 * nodes * XML_NODE_BYTES - reserved * XML_NODE_BYTES
        }
    }

    /// What `parse_xml` admits `text` against: storage and its nodes.
    fn xml_estimate(text: &str, nodes: u64) -> u64 {
        let shape = xml_shape(text);
        let storage = xml_storage_bytes(&shape, u64_from_index(shape.prefixes.len()))
            .expect("storage fits u64");
        storage + xml_node_storage(nodes, shape.less_than)
    }

    fn repeated(count: usize, part: impl Fn(usize) -> String) -> String {
        (0..count).map(part).collect()
    }

    /// A FreeCAD-shaped document: indented objects with typed properties.
    fn realistic_document(objects: usize) -> String {
        let declarations = repeated(objects, |index| {
            format!("\n        <Object type=\"Part::Feature\" name=\"Box{index}\" id=\"{index}\"/>")
        });
        let data = repeated(objects, |index| {
            format!(
                "\n        <Object name=\"Box{index}\">\n            <Properties Count=\"2\">\
                 \n                <Property name=\"Label\" type=\"App::PropertyString\">\
                 \n                    <String value=\"Box &amp; {index}\"/>\
                 \n                </Property>\
                 \n                <Property name=\"Length\" type=\"App::PropertyLength\">\
                 \n                    <Float value=\"10.0\"/>\
                 \n                </Property>\n            </Properties>\n        </Object>"
            )
        });
        format!(
            "<?xml version='1.0' encoding='utf-8'?>\n<Document SchemaVersion=\"4\">\
             \n    <Objects Count=\"{objects}\">{declarations}\n    </Objects>\
             \n    <ObjectData Count=\"{objects}\">{data}\n    </ObjectData>\n</Document>\n"
        )
    }

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

    fn with_expand_ceiling<T>(
        bytes: u64,
        run: impl FnOnce(&DecodeContext<'_>, crate::decode::View<'_>) -> T,
    ) -> T {
        let mut policy = DecodePolicy::default();
        policy.limits.max_decompressed_bytes_per_expand = bytes;
        with_part_root(policy, run)
    }

    fn with_part_root<T>(
        policy: DecodePolicy,
        run: impl FnOnce(&DecodeContext<'_>, crate::decode::View<'_>) -> T,
    ) -> T {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(b"", &arena, &policy).expect("valid fixture");
        run(&ctx, root)
    }

    fn is_depth_refusal<T: std::fmt::Debug>(result: &Result<T, CodecError>) -> bool {
        matches!(result, Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RecursionDepth)
    }

    fn is_part_limit(
        ctx: &DecodeContext<'_>,
        root: crate::decode::View<'_>,
        run: impl FnOnce() -> Result<(), CodecError>,
        dimension: ResourceDimension,
        allowance: u64,
    ) -> bool {
        let part = ctx
            .part_transaction(root, run)
            .expect("part limit is recoverable");
        matches!(part.result(), Err(CodecError::ResourceLimit(limit)) if limit.scope == crate::decode::LimitScope::Part && limit.dimension == dimension && limit.limit == allowance && limit.reason == crate::decode::ResourceFailure::BudgetExceeded)
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
    fn xml_shape_counts_reserved_slots_and_copied_text() {
        let shape = xml_shape(
            "<r a='x&amp;y' b='p\tq' c='z=w'><!-- < = -->t&lt;u<![CDATA[v]]>w<s/>\r\n</r>",
        );
        assert_eq!(shape.less_than, 6);
        assert_eq!(shape.equals, 5);
        assert_eq!(shape.cdata, 1);
        // Two values, the run that joins the CDATA, and the run with `\r`.
        assert_eq!(shape.copies, 4);
        assert_eq!(shape.copied, 7 + 3 + 20 + 2);
    }

    #[test]
    fn xml_shape_depth_matches_the_parsed_tree() {
        for text in [
            "<a/>",
            "<a>x</a>",
            "<a></a>",
            "<a><b> </b></a>",
            "<a><b/></a>",
            "<a><b></b><c><d>e</d></c></a>",
            "<!--c--><a/><?p?>",
            "<?xml version='1.0'?><a><b><!--c--></b></a>",
            "<a><b><?p x?></b></a>",
            "<a><b><![CDATA[x]]></b></a>",
            "<a b='/' c=\">\"><d e='1' /></a >",
            "<p:a xmlns:p='u'><p:b>t</p:b></p:a>",
            "<a>\r\n<b>\r\n</b>\r\n</a>\n",
        ] {
            let document = roxmltree::Document::parse(text).expect(text);
            let depth = document
                .descendants()
                .map(|node| u64_from_index(node.ancestors().count()) - 1)
                .max();
            assert_eq!(Some(xml_shape(text).depth), depth, "{text}");
        }
    }

    #[test]
    fn xml_nodes_limit_inverts_the_node_storage() {
        for reserved in [0, 1, 7, 1000] {
            for rest in [
                0,
                71,
                72,
                215,
                216,
                1000,
                144_000,
                144_215,
                144_216,
                1 << 20,
            ] {
                let limit = xml_nodes_limit(rest, reserved).expect("limit fits u64");
                assert!(
                    xml_node_storage(limit, reserved) <= rest,
                    "{reserved} {rest}"
                );
                assert!(
                    xml_node_storage(limit + 1, reserved) > rest,
                    "{reserved} {rest}"
                );
            }
        }
    }

    #[test]
    fn xml_storage_bound_covers_the_parse_peak() {
        let count = 1 << 12;
        let root_prefixes = repeated(100, |index| format!(" xmlns:q{index}='u'"));
        let documents = [
            ("elements", format!("<r>{}</r>", "<a/>".repeat(count))),
            (
                "attributes",
                format!("<r>{}</r>", "<a b='1' c='2' d='3' e='4'/>".repeat(count)),
            ),
            (
                "equals in text",
                format!("<r>{}</r>", "=".repeat(16 * count)),
            ),
            (
                "less-than in a comment",
                format!("<r><!--{}--></r>", "<".repeat(16 * count)),
            ),
            (
                "text around CDATA",
                format!("<r>{}</r>", "x<![CDATA[y]]>".repeat(count)),
            ),
            (
                "references in text",
                format!("<r>{}</r>", "<a>&amp;&lt;</a>".repeat(count)),
            ),
            (
                "references in values",
                format!("<r>{}</r>", "<a b='&amp;x&#10;'/>".repeat(count)),
            ),
            (
                "carriage returns",
                format!("<r>{}</r>", "a\r\n".repeat(4 * count)),
            ),
            (
                "nesting",
                format!("{}{}", "<a>".repeat(count), "</a>".repeat(count)),
            ),
            (
                "namespaces in scope",
                format!(
                    "<r xmlns='u'{root_prefixes}>{}</r>",
                    "<p:a xmlns:p='v' p:b='1'/>".repeat(count / 4)
                ),
            ),
            ("realistic", realistic_document(count / 4)),
        ];
        // roxmltree recurses once per level, past a test thread's stack.
        std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn(move || {
                with_depth(1 << 12, |ctx| {
                    for (label, text) in &documents {
                        let (document, peak) = heap::peak(|| ctx.parse_xml(text, "XML"));
                        let nodes = u64_from_index(document.expect(label).descendants().count());
                        let estimate = xml_estimate(text, nodes);
                        assert!(
                            peak <= estimate,
                            "{label}: peak {peak} > estimate {estimate}"
                        );
                    }
                });
            })
            .expect("spawn the parsing thread")
            .join()
            .expect("every peak within its estimate");
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn xml_storage_bound_stays_near_a_realistic_parse_peak() {
        let text = realistic_document(1 << 10);
        with_depth(256, |ctx| {
            let (document, peak) = heap::peak(|| ctx.parse_xml(&text, "XML"));
            let nodes = u64_from_index(document.expect("realistic XML").descendants().count());
            let estimate = xml_estimate(&text, nodes);
            assert!(
                estimate <= 2 * peak,
                "estimate {estimate} > twice peak {peak}"
            );
        });
    }

    #[test]
    fn xml_depth_refuses_before_the_parser_recurses() {
        let levels = 1 << 15;
        let nested = format!("{}{}", "<a>".repeat(levels), "</a>".repeat(levels));
        with_depth(256, |ctx| {
            assert!(is_depth_refusal(&ctx.parse_xml(&nested, "XML")));
        });
    }

    #[test]
    fn xml_tree_past_the_per_expand_allowance_is_unsupported() {
        let document = |elements: usize| format!("<r>{}</r>", "<a/>".repeat(elements));
        with_expand_ceiling(1 << 20, |ctx, root| {
            assert!(ctx.parse_xml(&document(1 << 10), "XML").is_ok());
            assert!(is_part_limit(
                ctx,
                root,
                || ctx.parse_xml(&document(1 << 14), "XML").map(drop),
                ResourceDimension::RetainedBytes,
                1 << 20
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
        with_part_root(DecodePolicy::default(), |ctx, root| {
            assert!(ctx.parse_xml(&tag(MAX_XML_TAG_ATTRIBUTES), "XML").is_ok());
            assert!(is_part_limit(
                ctx,
                root,
                || ctx
                    .parse_xml(&tag(MAX_XML_TAG_ATTRIBUTES + 1), "XML")
                    .map(drop),
                ResourceDimension::Entities,
                MAX_XML_TAG_ATTRIBUTES
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
        with_part_root(DecodePolicy::default(), |ctx, root| {
            assert!(ctx
                .parse_xml(&document(MAX_XML_NAMESPACE_PREFIXES), "XML")
                .is_ok());
            assert!(is_part_limit(
                ctx,
                root,
                || ctx
                    .parse_xml(&document(MAX_XML_NAMESPACE_PREFIXES + 1), "XML")
                    .map(drop),
                ResourceDimension::Entities,
                MAX_XML_NAMESPACE_PREFIXES
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
        with_expand_ceiling(1 << 20, |ctx, root| {
            assert!(ctx.parse_json_value(&document(1 << 8), "JSON").is_ok());
            assert!(is_part_limit(
                ctx,
                root,
                || ctx.parse_json_value(&document(1 << 11), "JSON").map(drop),
                ResourceDimension::RetainedBytes,
                1 << 20
            ));
            assert!(is_part_limit(
                ctx,
                root,
                || ctx
                    .parse_json::<serde_json::Value>(&document(1 << 11), "JSON")
                    .map(drop),
                ResourceDimension::RetainedBytes,
                1 << 20
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
