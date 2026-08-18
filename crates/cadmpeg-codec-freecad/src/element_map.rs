// SPDX-License-Identifier: Apache-2.0
//! Persistent string-table and element-map recovery.

use std::collections::HashMap;

use cadmpeg_core::decode::bounded_len;
use cadmpeg_core::CodecError;

use crate::native::{
    ElementMapGroup, ElementMapNode, ElementMapRecord, ElementMappedName, EntryRecord,
    PropertyRecord, StringTableEntry, StringTableRecord,
};
use crate::topology_transfer::TopologyOccurrence;

const MAX_TABLE_ENTRIES: usize = 10_000_000;
const MAX_MAP_NODES: usize = 1_000_000;
const MAX_GROUPS: usize = 1_000_000;
const MAX_NAMES: usize = 10_000_000;

/// Recover every string table and element map carried by `Document.xml`.
pub(crate) fn parse(
    document: &[u8],
    properties: &[PropertyRecord],
    entries: &[EntryRecord],
) -> Result<(Vec<StringTableRecord>, Vec<ElementMapRecord>), CodecError> {
    let text = std::str::from_utf8(document)
        .map_err(|_| CodecError::Malformed("Document.xml is not UTF-8".into()))?;
    let xml = roxmltree::Document::parse(text)
        .map_err(|error| CodecError::Malformed(format!("invalid Document.xml: {error}")))?;
    let string_hasher_nodes = xml
        .descendants()
        .filter(|node| node.has_tag_name("StringHasher"))
        .collect::<Vec<_>>();
    validate_string_hasher_framing(xml.root_element())?;
    let entry_data = entries
        .iter()
        .map(|entry| (entry.name.as_str(), entry.data.as_slice()))
        .collect::<HashMap<_, _>>();

    let mut tables = Vec::new();
    for node in string_hasher_nodes {
        let index = tables.len();
        let save_all = parse_bool(node.attribute("saveall").unwrap_or("0"))?;
        let threshold = parse_decimal(node.attribute("threshold").unwrap_or("0"), "threshold")?;
        let owner_property = owning_property(node, properties)?;
        let new_layout = node.attribute("new").is_some_and(|value| value != "0");
        let data_node = if new_layout {
            string_hasher_successor(node)?
        } else {
            node
        };
        let source_entry = data_node.attribute("file").filter(|name| !name.is_empty());
        let inline_bytes = source_entry.is_none().then(|| node_text_bytes(data_node));
        let bytes = if let Some(name) = source_entry {
            *entry_data.get(name).ok_or_else(|| {
                CodecError::Malformed(format!("StringHasher references missing entry {name}"))
            })?
        } else {
            inline_bytes.as_deref().unwrap_or_default()
        };
        let declared_count = if source_entry.is_some() {
            let header_count = string_table_header_count(bytes)?;
            if !new_layout {
                let xml_count = parse_count(data_node, "StringHasher")?;
                if xml_count != header_count {
                    return Err(CodecError::Malformed(
                        "string-table XML and side-entry counts disagree".into(),
                    ));
                }
            }
            header_count
        } else {
            parse_count(data_node, "StringHasher")?
        };
        let entries = parse_string_table(bytes, declared_count, source_entry.is_some())?;
        tables.push(StringTableRecord {
            id: crate::native::native_id("string-table", index.to_string()),
            index,
            owner_property,
            save_all,
            threshold,
            declared_count,
            source_entry: source_entry.map(str::to_owned),
            entries,
        });
    }

    let mut maps = Vec::new();
    for property in properties
        .iter()
        .filter(|property| property.type_name == "Part::PropertyPartShape")
    {
        let property_xml = roxmltree::Document::parse(&property.raw_xml).map_err(|error| {
            CodecError::Malformed(format!(
                "invalid shape property XML {}: {error}",
                property.id
            ))
        })?;
        let Some((part, map_node)) = direct_element_map(property_xml.root_element())? else {
            continue;
        };
        let version = part.attribute("ElementMap").unwrap_or("").to_owned();
        let hasher_index = part
            .attribute("HasherIndex")
            .map(|value| parse_usize(value, "HasherIndex"))
            .transpose()?;
        let declared_count = map_node
            .attribute("count")
            .map(|count| parse_usize(count, "ElementMap count"))
            .transpose()?;
        let source_entry = map_node.attribute("file").filter(|name| !name.is_empty());
        let inline_bytes = source_entry.is_none().then(|| node_text_bytes(map_node));
        let bytes = if let Some(name) = source_entry {
            *entry_data.get(name).ok_or_else(|| {
                CodecError::Malformed(format!("ElementMap references missing entry {name}"))
            })?
        } else {
            inline_bytes.as_deref().unwrap_or_default()
        };
        let parsed = parse_element_map(bytes, source_entry.is_some())?;
        let actual_count = parsed
            .maps
            .iter()
            .flat_map(|node| &node.groups)
            .flat_map(|group| &group.names)
            .flat_map(|chain| chain.iter())
            .count();
        let declared_count = declared_count.unwrap_or(actual_count);
        maps.push(ElementMapRecord {
            id: crate::native::native_child_id("element-map", &property.id, "map"),
            property: property.id.clone(),
            version,
            hasher_index,
            source_entry: source_entry.map(str::to_owned),
            map_id: parsed.map_id,
            declared_count,
            postfixes: parsed.postfixes,
            maps: parsed.maps,
        });
    }
    Ok((tables, maps))
}

fn string_table_header_count(bytes: &[u8]) -> Result<usize, CodecError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| CodecError::Malformed("string table is not UTF-8".into()))?;
    let mut tokens = text.split_ascii_whitespace();
    if tokens.next() != Some("StringTableStart") || tokens.next() != Some("v1") {
        return Err(CodecError::Malformed(
            "string-table side entry has invalid header".into(),
        ));
    }
    let count = tokens
        .next()
        .ok_or_else(|| CodecError::Malformed("string-table side entry has no count".into()))?;
    let count = parse_usize(count, "string-table header count")?;
    if count > MAX_TABLE_ENTRIES {
        return Err(CodecError::Malformed(format!(
            "string-table entry count exceeds {MAX_TABLE_ENTRIES}"
        )));
    }
    Ok(count)
}

/// Connect kernel indexed-map positions to every neutral placed occurrence.
pub(crate) fn bind_topology(maps: &mut [ElementMapRecord], occurrences: &[TopologyOccurrence]) {
    for map in maps {
        let Some(root) = map.maps.last_mut() else {
            continue;
        };
        for group in &mut root.groups {
            let indexed_name = group.indexed_name.clone();
            for occurrence in occurrences.iter().filter(|occurrence| {
                occurrence.property == map.property && occurrence.indexed_name == indexed_name
            }) {
                bind_group_occurrence(group, occurrence.source_index, &occurrence.topology_id);
            }
        }
    }
}

fn bind_group_occurrence(group: &mut ElementMapGroup, source_index: usize, id: &str) {
    let Some(names) = group.names.get_mut(source_index) else {
        return;
    };
    for name in names {
        if !name.topology_ids.iter().any(|existing| existing == id) {
            name.topology_ids.push(id.to_owned());
        }
    }
}

fn owning_property(
    node: roxmltree::Node<'_, '_>,
    properties: &[PropertyRecord],
) -> Result<Option<String>, CodecError> {
    let start = node.range().start as u64;
    let mut owners = properties
        .iter()
        .filter(|property| property.byte_start <= start && start < property.byte_end);
    let Some(owner) = owners.next() else {
        return Ok(None);
    };
    if owners.next().is_some() {
        return Err(CodecError::Malformed(
            "StringHasher has multiple enclosing properties".into(),
        ));
    }
    Ok(Some(owner.id.clone()))
}

fn validate_string_hasher_framing(
    document_root: roxmltree::Node<'_, '_>,
) -> Result<(), CodecError> {
    for node in document_root
        .descendants()
        .filter(|node| node.has_tag_name("StringHasher"))
    {
        let Some(parent) = node.parent() else {
            return Err(CodecError::Malformed(
                "StringHasher has no enclosing root".into(),
            ));
        };
        let parent_is_document = parent == document_root;
        let parent_is_shape_property = is_shape_property(parent);
        if !parent_is_document && !parent_is_shape_property {
            return Err(CodecError::Malformed(
                "StringHasher is not a direct document or shape-property carrier".into(),
            ));
        }

        let children = parent
            .children()
            .filter(roxmltree::Node::is_element)
            .collect::<Vec<_>>();
        let marker_indices = children
            .iter()
            .enumerate()
            .filter_map(|(index, child)| child.has_tag_name("StringHasher").then_some(index))
            .collect::<Vec<_>>();
        if marker_indices.len() != 1 {
            return Err(CodecError::Malformed(
                "StringHasher has duplicate direct carriers".into(),
            ));
        }
        if parent_is_shape_property {
            let part_indices = children
                .iter()
                .enumerate()
                .filter_map(|(index, child)| child.has_tag_name("Part").then_some(index))
                .collect::<Vec<_>>();
            if part_indices.len() != 1 || marker_indices[0] <= part_indices[0] {
                return Err(CodecError::Malformed(
                    "StringHasher is not owned by a direct Part carrier".into(),
                ));
            }
        }

        let successor_indices = children
            .iter()
            .enumerate()
            .filter_map(|(index, child)| child.has_tag_name("StringHasher2").then_some(index))
            .collect::<Vec<_>>();
        let marker = children[marker_indices[0]];
        let new_layout = marker.attribute("new").is_some_and(|value| value != "0");
        if new_layout {
            if successor_indices.len() != 1 || successor_indices[0] != marker_indices[0] + 1 {
                return Err(CodecError::Malformed(
                    "StringHasher new=1 is not followed by one direct StringHasher2".into(),
                ));
            }
        } else if !successor_indices.is_empty() {
            return Err(CodecError::Malformed(
                "legacy StringHasher has a direct StringHasher2 successor".into(),
            ));
        }
    }

    for node in document_root
        .descendants()
        .filter(|node| node.has_tag_name("StringHasher2"))
    {
        let Some(parent) = node.parent() else {
            return Err(CodecError::Malformed(
                "StringHasher2 has no enclosing root".into(),
            ));
        };
        let direct_successor = parent
            .children()
            .filter(roxmltree::Node::is_element)
            .collect::<Vec<_>>()
            .windows(2)
            .any(|pair| pair[0].has_tag_name("StringHasher") && pair[1] == node);
        if !direct_successor {
            return Err(CodecError::Malformed(
                "StringHasher2 is not the direct successor of StringHasher".into(),
            ));
        }
    }

    Ok(())
}

fn is_shape_property(node: roxmltree::Node<'_, '_>) -> bool {
    node.is_element()
        && (node.has_tag_name("Property") || node.has_tag_name("_Property"))
        && node.attribute("type") == Some("Part::PropertyPartShape")
}

fn direct_element_map<'a, 'input>(
    root: roxmltree::Node<'a, 'input>,
) -> Result<Option<(roxmltree::Node<'a, 'input>, roxmltree::Node<'a, 'input>)>, CodecError> {
    let children = root
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    let part_indices = children
        .iter()
        .enumerate()
        .filter_map(|(index, node)| node.has_tag_name("Part").then_some(index))
        .collect::<Vec<_>>();
    if part_indices.len() > 1 {
        return Err(CodecError::Malformed(
            "shape property has multiple direct Part carriers".into(),
        ));
    }
    let Some(part_index) = part_indices.first().copied() else {
        return Ok(None);
    };
    let marker_indices = children
        .iter()
        .enumerate()
        .filter_map(|(index, node)| node.has_tag_name("ElementMap").then_some(index))
        .collect::<Vec<_>>();
    if marker_indices.len() > 1 {
        return Err(CodecError::Malformed(
            "shape property has multiple direct ElementMap markers".into(),
        ));
    }
    let map_indices = children
        .iter()
        .enumerate()
        .filter_map(|(index, node)| node.has_tag_name("ElementMap2").then_some(index))
        .collect::<Vec<_>>();
    if map_indices.len() > 1 {
        return Err(CodecError::Malformed(
            "shape property has multiple direct ElementMap2 carriers".into(),
        ));
    }
    let Some(marker_index) = marker_indices.first().copied() else {
        if !map_indices.is_empty() {
            return Err(CodecError::Malformed(
                "ElementMap2 has no direct ElementMap marker".into(),
            ));
        }
        return Ok(None);
    };
    if marker_index <= part_index {
        return Err(CodecError::Malformed(
            "ElementMap marker precedes the direct Part carrier".into(),
        ));
    }
    let marker = children[marker_index];
    let is_new = marker
        .attribute("new")
        .map(parse_bool)
        .transpose()?
        .unwrap_or(false);
    let Some(map_index) = map_indices.first().copied() else {
        if is_new {
            return Err(CodecError::Malformed(
                "new ElementMap marker has no direct ElementMap2 successor".into(),
            ));
        }
        return Ok(None);
    };
    if !is_new {
        return Err(CodecError::Malformed(
            "ElementMap2 requires a new ElementMap marker".into(),
        ));
    }
    if map_index != marker_index + 1 {
        return Err(CodecError::Malformed(
            "ElementMap2 is not the direct successor of ElementMap".into(),
        ));
    }
    Ok(Some((children[part_index], children[map_index])))
}

fn string_hasher_successor<'a, 'input>(
    node: roxmltree::Node<'a, 'input>,
) -> Result<roxmltree::Node<'a, 'input>, CodecError> {
    let mut siblings = node.next_siblings().skip(1);
    let Some(mut successor) = siblings.next() else {
        return Err(CodecError::Malformed(
            "StringHasher new=1 is not followed by StringHasher2".into(),
        ));
    };
    while successor.is_text() && successor.text().is_some_and(|text| text.trim().is_empty()) {
        successor = siblings.next().ok_or_else(|| {
            CodecError::Malformed("StringHasher new=1 is not followed by StringHasher2".into())
        })?;
    }
    if successor.is_element() && successor.has_tag_name("StringHasher2") {
        Ok(successor)
    } else {
        Err(CodecError::Malformed(
            "StringHasher new=1 is not followed by StringHasher2".into(),
        ))
    }
}

fn node_text_bytes(node: roxmltree::Node<'_, '_>) -> Vec<u8> {
    node.children()
        .filter_map(|child| child.is_text().then(|| child.text()).flatten())
        .flat_map(str::bytes)
        .collect()
}

fn parse_count(node: roxmltree::Node<'_, '_>, kind: &str) -> Result<usize, CodecError> {
    let count = node.attribute("count").unwrap_or("0");
    let count = parse_usize(count, &format!("{kind} count"))?;
    if count > MAX_TABLE_ENTRIES {
        return Err(CodecError::Malformed(format!("{kind} count exceeds limit")));
    }
    Ok(count)
}

fn parse_bool(value: &str) -> Result<bool, CodecError> {
    match value {
        "0" | "false" => Ok(false),
        "1" | "true" => Ok(true),
        _ => Err(CodecError::Malformed(format!("invalid boolean {value:?}"))),
    }
}

fn parse_decimal(value: &str, field: &str) -> Result<i64, CodecError> {
    value
        .parse()
        .map_err(|_| CodecError::Malformed(format!("invalid {field} {value:?}")))
}

fn parse_usize(value: &str, field: &str) -> Result<usize, CodecError> {
    value
        .parse()
        .map_err(|_| CodecError::Malformed(format!("invalid {field} {value:?}")))
}

fn parse_hex(value: &str, field: &str) -> Result<i64, CodecError> {
    let (negative, digits) = value
        .strip_prefix('-')
        .map_or((false, value), |digits| (true, digits));
    if digits.is_empty() {
        return Err(CodecError::Malformed(format!("empty {field}")));
    }
    let value = i64::from_str_radix(digits, 16)
        .map_err(|_| CodecError::Malformed(format!("invalid {field} {value:?}")))?;
    Ok(if negative { -value } else { value })
}

fn parse_string_table(
    bytes: &[u8],
    declared_count: usize,
    side_entry: bool,
) -> Result<Vec<StringTableEntry>, CodecError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| CodecError::Malformed("string table is not UTF-8".into()))?;
    let mut scanner = TextScanner::new(text);
    if side_entry {
        if scanner.token()? != "StringTableStart" || scanner.token()? != "v1" {
            return Err(CodecError::Malformed(
                "string-table side entry has invalid header".into(),
            ));
        }
        if parse_usize(scanner.token()?, "string-table header count")? != declared_count {
            return Err(CodecError::Malformed(
                "string-table XML and side-entry counts disagree".into(),
            ));
        }
    }
    // Each record consumes at least one non-whitespace byte, so the declared count
    // cannot exceed the table's byte length.
    let capacity = bounded_len(declared_count as u64, 1, text.len())
        .ok_or_else(|| CodecError::Malformed("string-table record count exceeds input".into()))?;
    let mut output = Vec::with_capacity(capacity);
    let mut previous_id = 0_i64;
    let mut previous_components = Vec::<i64>::new();
    for _ in 0..declared_count {
        scanner.skip_whitespace();
        let record_start = scanner.position;
        let header = scanner.token()?;
        let fields = header.split('.').collect::<Vec<_>>();
        if fields.len() < 2 {
            return Err(CodecError::Malformed(
                "string-table record has incomplete numeric header".into(),
            ));
        }
        let relative = fields[0].starts_with('-');
        let encoded_id = parse_hex(fields[0], "string id")?;
        let string_id = if relative {
            previous_id
                .checked_add(-encoded_id)
                .ok_or_else(|| CodecError::Malformed("relative string id overflows".into()))?
        } else {
            encoded_id
        };
        let flags = u64::from_str_radix(fields[1], 16)
            .map_err(|_| CodecError::Malformed("invalid string-table flags".into()))?;
        let mut components = Vec::new();
        for (position, field) in fields.iter().skip(2).enumerate() {
            let encoded = parse_hex(field, "string component")?;
            let component = if relative {
                if let Some(previous) = previous_components.get(position) {
                    previous.checked_add(encoded).ok_or_else(|| {
                        CodecError::Malformed("relative string component overflows".into())
                    })?
                } else {
                    string_id.checked_sub(encoded).ok_or_else(|| {
                        CodecError::Malformed("relative string component overflows".into())
                    })?
                }
            } else {
                encoded
            };
            components.push(component);
        }
        let payload = if flags & 0x8 == 0 {
            scanner.encoded_text()?
        } else {
            let derived_prefix = flags & (0x10 | 0x20 | 0x40) != 0;
            let encoded_postfix = flags & 0x4 != 0;
            let mut values = Vec::new();
            if !derived_prefix {
                values.push(scanner.token()?.to_owned());
            }
            if !encoded_postfix {
                values.push(scanner.token()?.to_owned());
            }
            values.join(" ")
        };
        let raw = text[record_start..scanner.position]
            .trim_end_matches(char::is_whitespace)
            .to_owned();
        output.push(StringTableEntry {
            string_id,
            flags,
            components: components.clone(),
            payload,
            raw,
        });
        previous_id = string_id;
        previous_components = components;
    }
    scanner.skip_whitespace();
    if !scanner.is_done() {
        return Err(CodecError::Malformed(
            "string table contains records beyond declared count".into(),
        ));
    }
    Ok(output)
}

struct TextScanner<'a> {
    text: &'a str,
    position: usize,
}

impl<'a> TextScanner<'a> {
    fn new(text: &'a str) -> Self {
        Self { text, position: 0 }
    }

    fn skip_whitespace(&mut self) {
        while let Some(character) = self.text[self.position..].chars().next() {
            if !character.is_whitespace() {
                break;
            }
            self.position += character.len_utf8();
        }
    }

    fn token(&mut self) -> Result<&'a str, CodecError> {
        self.skip_whitespace();
        let start = self.position;
        while let Some(character) = self.text[self.position..].chars().next() {
            if character.is_whitespace() {
                break;
            }
            self.position += character.len_utf8();
        }
        if start == self.position {
            return Err(CodecError::Malformed(
                "string table ends before declared count".into(),
            ));
        }
        Ok(&self.text[start..self.position])
    }

    fn encoded_text(&mut self) -> Result<String, CodecError> {
        self.skip_whitespace();
        let count_start = self.position;
        while self
            .text
            .as_bytes()
            .get(self.position)
            .is_some_and(u8::is_ascii_digit)
        {
            self.position += 1;
        }
        if count_start == self.position || self.text.as_bytes().get(self.position) != Some(&b':') {
            return Err(CodecError::Malformed(
                "string-table text has invalid line-count prefix".into(),
            ));
        }
        let line_count = parse_usize(&self.text[count_start..self.position], "text line count")?;
        self.position += 1;
        let content_start = self.position;
        for _ in 0..=line_count {
            let remaining = &self.text[self.position..];
            let Some(newline) = remaining.find('\n') else {
                return Err(CodecError::Malformed(
                    "string-table text ends before its line delimiter".into(),
                ));
            };
            self.position += newline + 1;
        }
        Ok(self.text[content_start..self.position - 1].to_owned())
    }

    fn is_done(&self) -> bool {
        self.position == self.text.len()
    }
}

pub(crate) struct ParsedMap {
    map_id: u64,
    postfixes: Vec<String>,
    maps: Vec<ElementMapNode>,
}

pub(crate) fn parse_element_map(bytes: &[u8], side_entry: bool) -> Result<ParsedMap, CodecError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| CodecError::Malformed("element map is not UTF-8".into()))?;
    let mut tokens = text.split_whitespace();
    if side_entry {
        expect(&mut tokens, "BeginElementMap")?;
        expect(&mut tokens, "v1")?;
    }
    let map_id = next_u64(&mut tokens, "element-map id")?;
    expect(&mut tokens, "PostfixCount")?;
    let postfix_count = next_count(&mut tokens, "postfix count", MAX_NAMES)?;
    let postfixes = (0..postfix_count)
        .map(|_| next_token(&mut tokens, "postfix").map(str::to_owned))
        .collect::<Result<Vec<_>, _>>()?;
    expect(&mut tokens, "MapCount")?;
    let map_count = next_count(&mut tokens, "map count", MAX_MAP_NODES)?;
    if map_count == 0 {
        return Err(CodecError::Malformed(
            "element map has zero map nodes".into(),
        ));
    }
    // Each map node consumes at least one whitespace-separated token, so its count
    // cannot exceed the element map's byte length.
    let map_capacity = bounded_len(map_count as u64, 1, text.len())
        .ok_or_else(|| CodecError::Malformed("element-map node count exceeds input".into()))?;
    let mut maps = Vec::with_capacity(map_capacity);
    for expected_index in 1..=map_count {
        expect(&mut tokens, "ElementMap")?;
        let index = next_count(&mut tokens, "map index", MAX_MAP_NODES)?;
        if index != expected_index {
            return Err(CodecError::Malformed(
                "element-map node indices are not contiguous".into(),
            ));
        }
        let node_id = next_u64(&mut tokens, "map node id")?;
        let group_count = next_count(&mut tokens, "group count", MAX_GROUPS)?;
        // Each group consumes at least one token, so its count cannot exceed the byte length.
        let group_capacity = bounded_len(group_count as u64, 1, text.len())
            .ok_or_else(|| CodecError::Malformed("element-map group count exceeds input".into()))?;
        let mut groups = Vec::with_capacity(group_capacity);
        for _ in 0..group_count {
            let indexed_name = next_token(&mut tokens, "indexed name")?.to_owned();
            expect(&mut tokens, "ChildCount")?;
            let child_count = next_count(&mut tokens, "child count", MAX_NAMES)?;
            // Each child consumes at least one token, so its count cannot exceed the byte length.
            let child_capacity =
                bounded_len(child_count as u64, 1, text.len()).ok_or_else(|| {
                    CodecError::Malformed("element-map child count exceeds input".into())
                })?;
            let mut children = Vec::with_capacity(child_capacity);
            for _ in 0..child_count {
                let fields = (0..7)
                    .map(|_| next_token(&mut tokens, "child descriptor"))
                    .collect::<Result<Vec<_>, _>>()?;
                children.push(fields.join(" "));
            }
            expect(&mut tokens, "NameCount")?;
            let name_count = next_count(&mut tokens, "name count", MAX_NAMES)?;
            // Each name consumes at least one token, so its count cannot exceed the byte length.
            let name_capacity = bounded_len(name_count as u64, 1, text.len()).ok_or_else(|| {
                CodecError::Malformed("element-map name count exceeds input".into())
            })?;
            let mut names = Vec::with_capacity(name_capacity);
            for _ in 0..name_count {
                let mut chain = Vec::new();
                loop {
                    let encoded = next_token(&mut tokens, "mapped name")?;
                    if encoded == "0" {
                        break;
                    }
                    chain.push(parse_mapped_name(encoded, &postfixes)?);
                }
                names.push(chain);
            }
            groups.push(ElementMapGroup {
                indexed_name,
                children,
                names,
            });
        }
        expect(&mut tokens, "EndMap")?;
        maps.push(ElementMapNode {
            index,
            map_id: node_id,
            groups,
        });
    }
    if tokens.next().is_some() {
        return Err(CodecError::Malformed(
            "element map has trailing non-whitespace data".into(),
        ));
    }
    Ok(ParsedMap {
        map_id,
        postfixes,
        maps,
    })
}

fn parse_mapped_name(encoded: &str, postfixes: &[String]) -> Result<ElementMappedName, CodecError> {
    let fields = encoded.split('.').collect::<Vec<_>>();
    let (base, postfix_position, id_position) =
        if let Some(dictionary) = fields[0].strip_prefix(':') {
            if fields.len() < 3 {
                return Err(CodecError::Malformed(
                    "indexed mapped name has incomplete dictionary fields".into(),
                ));
            }
            let dictionary = parse_usize(dictionary, "mapped-name prefix index")?;
            let prefix = postfixes
                .get(dictionary.checked_sub(1).ok_or_else(|| {
                    CodecError::Malformed("mapped-name prefix index is zero".into())
                })?)
                .ok_or_else(|| {
                    CodecError::Malformed("mapped-name prefix index is out of range".into())
                })?;
            let element = usize::try_from(parse_hex(fields[1], "mapped-name element index")?)
                .map_err(|_| CodecError::Malformed("negative mapped-name element index".into()))?;
            (format!("{prefix}{element}"), 2, 3)
        } else if let Some(base) = fields[0]
            .strip_prefix(';')
            .or_else(|| fields[0].strip_prefix('$'))
        {
            (base.to_owned(), 1, 2)
        } else {
            return Err(CodecError::Malformed(
                "mapped name has unknown base encoding".into(),
            ));
        };
    let postfix_index = fields
        .get(postfix_position)
        .ok_or_else(|| CodecError::Malformed("mapped name has no postfix index".into()))
        .and_then(|value| {
            usize::try_from(parse_hex(value, "mapped-name postfix index")?)
                .map_err(|_| CodecError::Malformed("negative mapped-name postfix index".into()))
        })?;
    let mut resolved = base;
    if postfix_index != 0 {
        resolved.push_str(postfixes.get(postfix_index - 1).ok_or_else(|| {
            CodecError::Malformed("mapped-name postfix index is out of range".into())
        })?);
    }
    let string_ids = fields
        .iter()
        .skip(id_position)
        .filter(|value| !value.is_empty())
        .map(|value| parse_hex(value, "mapped-name string id"))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ElementMappedName {
        encoded: encoded.to_owned(),
        resolved: Some(resolved),
        string_ids,
        topology_ids: Vec::new(),
    })
}

fn next_token<'a>(
    tokens: &mut impl Iterator<Item = &'a str>,
    field: &str,
) -> Result<&'a str, CodecError> {
    tokens
        .next()
        .ok_or_else(|| CodecError::Malformed(format!("element map ends before {field}")))
}

fn expect<'a>(
    tokens: &mut impl Iterator<Item = &'a str>,
    expected: &str,
) -> Result<(), CodecError> {
    let actual = next_token(tokens, expected)?;
    if actual != expected {
        return Err(CodecError::Malformed(format!(
            "expected element-map token {expected:?}, found {actual:?}"
        )));
    }
    Ok(())
}

fn next_count<'a>(
    tokens: &mut impl Iterator<Item = &'a str>,
    field: &str,
    limit: usize,
) -> Result<usize, CodecError> {
    let value = parse_usize(next_token(tokens, field)?, field)?;
    if value > limit {
        return Err(CodecError::Malformed(format!("{field} exceeds limit")));
    }
    Ok(value)
}

fn next_u64<'a>(
    tokens: &mut impl Iterator<Item = &'a str>,
    field: &str,
) -> Result<u64, CodecError> {
    next_token(tokens, field)?
        .parse()
        .map_err(|_| CodecError::Malformed(format!("invalid {field}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;
    use crate::FcstdCodec;
    use cadmpeg_ir::{Codec, DecodeOptions};
    use std::io::Cursor;

    fn test_property(type_name: &str, raw_xml: &str) -> PropertyRecord {
        PropertyRecord {
            id: "fcstd:test:property#Shape".into(),
            owner: "fcstd:test:object#Shape".into(),
            name: "Shape".into(),
            type_name: type_name.into(),
            family: crate::native::PropertyFamily::Unknown,
            status: None,
            transient: false,
            dynamic: None,
            order: 0,
            values: Vec::new(),
            links: Vec::new(),
            side_entries: Vec::new(),
            raw_xml: raw_xml.into(),
            byte_start: 0,
            byte_end: raw_xml.len() as u64,
        }
    }

    #[test]
    fn restores_absolute_and_relative_string_table_headers() {
        let records = parse_string_table(b"a.c.2 alpha\n-3.c.-1 beta\n", 2, false)
            .expect("parse relative string table");
        assert_eq!(records[0].string_id, 10);
        assert_eq!(records[0].components, [2]);
        assert_eq!(records[1].string_id, 13);
        assert_eq!(records[1].components, [1]);
        assert_eq!(records[1].payload, "beta");
    }

    #[test]
    fn parses_map_nodes_and_mapped_name_chains() {
        let input = b"7 PostfixCount 1 :tag MapCount 1\n\
            ElementMap 1 7 1 Face ChildCount 0 NameCount 2\n\
            ;Generated.0.a 0 :1.a.0.b 0 EndMap";
        let parsed = parse_element_map(input, false).expect("parse element map");
        assert_eq!(parsed.map_id, 7);
        assert_eq!(parsed.postfixes, [":tag"]);
        assert_eq!(parsed.maps[0].groups[0].names.len(), 2);
        assert_eq!(parsed.maps[0].groups[0].names[1][0].string_ids, [11]);
        assert_eq!(
            parsed.maps[0].groups[0].names[1][0].resolved.as_deref(),
            Some(":tag10")
        );
    }

    #[test]
    fn rejects_declared_string_table_count_mismatch() {
        assert!(parse_string_table(b"1.c name\n", 2, false).is_err());
    }

    #[test]
    fn accepts_document_and_direct_shape_string_hasher_roots() {
        let xml = roxmltree::Document::parse(
            r#"<Document>
<StringHasher saveall="0" threshold="0" count="0" new="1"/>
<StringHasher2 count="0"></StringHasher2>
<Property name="Shape" type="Part::PropertyPartShape">
<Part/>
<StringHasher saveall="0" threshold="0" count="0" new="1"/>
<StringHasher2 count="0"></StringHasher2>
</Property>
</Document>"#,
        )
        .expect("framed string hashers");
        validate_string_hasher_framing(xml.root_element()).expect("valid roots");
    }

    #[test]
    fn accepts_legacy_document_string_hasher_carrier() {
        let (tables, maps) = parse(
            br#"<Document><StringHasher count="1">a.c legacy</StringHasher></Document>"#,
            &[],
            &[],
        )
        .expect("legacy string table carrier");
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].entries[0].payload, "legacy");
        assert!(maps.is_empty());
    }

    #[test]
    fn rejects_nested_string_hasher_carrier() {
        let xml = roxmltree::Document::parse(
            r#"<Document><Property name="Shape" type="Part::PropertyPartShape">
<Part/><Wrapper><StringHasher count="0"></StringHasher></Wrapper>
</Property></Document>"#,
        )
        .expect("nested string hasher");
        assert!(validate_string_hasher_framing(xml.root_element()).is_err());
    }

    #[test]
    fn rejects_duplicate_direct_string_hasher_carriers() {
        let xml = roxmltree::Document::parse(
            r#"<Document><Property name="Shape" type="Part::PropertyPartShape">
<Part/><StringHasher count="0"></StringHasher><StringHasher count="0"></StringHasher>
</Property></Document>"#,
        )
        .expect("duplicate string hashers");
        assert!(validate_string_hasher_framing(xml.root_element()).is_err());
    }

    #[test]
    fn rejects_orphan_string_hasher2_carrier() {
        let xml = roxmltree::Document::parse("<Document><StringHasher2/></Document>")
            .expect("orphan string hasher successor");
        assert!(validate_string_hasher_framing(xml.root_element()).is_err());
    }

    #[test]
    fn rejects_non_successor_string_hasher2_carrier() {
        let xml = roxmltree::Document::parse(
            r#"<Document><StringHasher new="1"/><Wrapper/><StringHasher2/></Document>"#,
        )
        .expect("non-successor string hasher");
        assert!(validate_string_hasher_framing(xml.root_element()).is_err());
    }

    #[test]
    fn restores_multiline_length_prefixed_string() {
        let records = parse_string_table(b"1.0 1:first\nsecond\n", 1, false)
            .expect("parse multiline string table");
        assert_eq!(records[0].payload, "first\nsecond");
    }

    #[test]
    fn parses_side_entry_headers() {
        let table = parse_string_table(b"StringTableStart v1 1\n1.c value\n", 1, true)
            .expect("parse absolute string table");
        assert_eq!(table[0].payload, "value");
        let map = parse_element_map(
            b"BeginElementMap v1 1 PostfixCount 0 MapCount 1 ElementMap 1 1 0 EndMap",
            true,
        )
        .expect("parse absolute element map");
        assert_eq!(map.map_id, 1);
    }

    #[test]
    fn joins_inline_text_and_cdata_sections() {
        let xml = roxmltree::Document::parse("<Table>\n<![CDATA[1.c value\n]]>\n</Table>")
            .expect("inline table XML");
        let bytes = node_text_bytes(xml.root_element());
        let table = parse_string_table(&bytes, 1, false).expect("inline string table");
        assert_eq!(table[0].payload, "value");
    }

    #[test]
    fn topology_binding_preserves_empty_indexed_name_slots() {
        let mapped_name = || ElementMappedName {
            encoded: ";stable.0".into(),
            resolved: Some("stable".into()),
            string_ids: Vec::new(),
            topology_ids: Vec::new(),
        };
        let mut group = ElementMapGroup {
            indexed_name: "Edge".into(),
            children: Vec::new(),
            names: vec![Vec::new(), vec![mapped_name()], Vec::new()],
        };
        bind_group_occurrence(&mut group, 1, "edge-first-placement");
        bind_group_occurrence(&mut group, 2, "unmapped-edge");
        bind_group_occurrence(&mut group, 1, "edge-second-placement");

        assert_eq!(
            group.names[1][0].topology_ids,
            ["edge-first-placement", "edge-second-placement"]
        );
    }

    #[test]
    fn connects_persistent_element_names_to_neutral_topology() {
        let document = r#"<Document SchemaVersion="4" FileVersion="1" StringHasher="1">
<Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects>
<ObjectData Count="1"><Object name="Shape"><Properties Count="2">
<Property name="AuxShape" type="Part::PropertyPartShape">
<Part HasherIndex="0" SaveHasher="1" ElementMap="1.0" file="AuxShape.brp"/>
<ElementMap new="1" count="1"><Element key="compat" value="compat"/></ElementMap>
<ElementMap2 count="5">
41 PostfixCount 0 MapCount 1
ElementMap 1 41 3
Face ChildCount 0 NameCount 2
0
;FaceStable.0.a 0
Edge ChildCount 0 NameCount 3
0
;EdgeStable1.0.a 0
;EdgeStable2.0.a 0
Vertex ChildCount 0 NameCount 3
0
;VertexStable1.0.a 0
;VertexStable2.0.a 0
EndMap
</ElementMap2>
</Property>
<Property name="Shape" type="Part::PropertyPartShape">
<Part HasherIndex="0" SaveHasher="1" ElementMap="1.0" file="Shape.brp"/>
<StringHasher saveall="0" threshold="16" count="0" new="1"/>
<StringHasher2 count="1">
a.c PersistentSource
</StringHasher2>
<ElementMap new="1" count="1"><Element key="compat" value="compat"/></ElementMap>
<ElementMap2 count="5">
41 PostfixCount 0 MapCount 1
ElementMap 1 41 3
Face ChildCount 0 NameCount 2
0
;FaceStable.0.a 0
Edge ChildCount 0 NameCount 3
0
;EdgeStable1.0.a 0
;EdgeStable2.0.a 0
Vertex ChildCount 0 NameCount 3
0
;VertexStable1.0.a 0
;VertexStable2.0.a 0
EndMap
</ElementMap2>
</Property></Properties></Object></ObjectData>
</Document>"#;
        let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="Shape"><Properties Count="4">
<Property name="ShapeColor" type="App::PropertyColor"><PropertyColor value="3435973632"/></Property>
<Property name="DiffuseColor" type="App::PropertyColorList"><ColorList file="DiffuseColor"/></Property>
<Property name="LineColorArray" type="App::PropertyColorList"><ColorList file="LineColorArray"/></Property>
<Property name="PointColorArray" type="App::PropertyColorList"><ColorList file="PointColorArray"/></Property>
</Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#;
        let brep = b"CASCADE Topology V1, (c) Matra-Datavision
Locations 0
Curve2ds 2
1 0 0 1 0
1 1 0 -1 0
Curves 2
1 0 0 0 1 0 0
1 1 0 0 -1 0 0
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 1
1 0 0 0 0 0 1 1 0 0 0 1 0
Triangulations 0
TShapes 9
Ve 0.001 0 0 0 0 0 1001000 *
Ve 0.001 1 0 0 0 0 1001000 *
Ed 0.001 1 1 0 1 1 0 0 1 2 1 1 0 0 1 0 1001000 +9 0 -8 0 *
Ed 0.001 1 1 0 1 2 0 0 1 2 2 1 0 0 1 0 1001000 +8 0 -9 0 *
Wi 1001000 +7 0 +6 0 *
Fa 0 0.001 1 0 1001000 +5 0 *
Sh 1001000 +4 0 *
So 1001000 +3 0 *
Co 1001000 +2 0 *
+1 0 *";
        let face_colors = [1_u8, 0, 0, 0, 0, 0, 0, 255];
        let edge_colors = [2_u8, 0, 0, 0, 255, 0, 0, 255, 0, 255, 0, 255];
        let point_colors = [2_u8, 0, 0, 0, 0, 0, 255, 255, 255, 255, 0, 255];
        let bytes = archive_entries(&[
            ("Document.xml", document.as_bytes()),
            ("GuiDocument.xml", gui),
            ("DiffuseColor", &face_colors),
            ("LineColorArray", &edge_colors),
            ("PointColorArray", &point_colors),
            ("AuxShape.brp", brep),
            ("Shape.brp", brep),
        ]);
        let result = FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("persistent element map");
        let namespace = result
            .ir()
            .native
            .namespace("fcstd")
            .expect("required invariant");
        let tables = namespace
            .arena_as::<crate::native::StringTableRecord>("string_tables")
            .expect("required invariant");
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].entries[0].string_id, 10);
        let maps = namespace
            .arena_as::<crate::native::ElementMapRecord>("element_maps")
            .expect("required invariant");
        assert_eq!(maps.len(), 2);
        let shape_map = maps
            .iter()
            .find(|map| map.property.ends_with("#Shape:Shape"))
            .expect("displayed Shape element map");
        assert_eq!(shape_map.hasher_index, Some(0));
        let groups = &shape_map.maps[0].groups;
        assert_eq!(groups[0].names[1][0].topology_ids.len(), 1);
        assert_eq!(groups[1].names[1][0].topology_ids.len(), 1);
        assert_eq!(groups[1].names[2][0].topology_ids.len(), 1);
        assert_eq!(groups[2].names[1][0].topology_ids.len(), 1);
        assert_eq!(groups[2].names[2][0].topology_ids.len(), 1);
        let shape_face_ids = groups[0]
            .names
            .iter()
            .flatten()
            .flat_map(|name| &name.topology_ids)
            .collect::<std::collections::HashSet<_>>();
        assert!(result.ir().model.appearance_bindings.iter().any(|binding| {
        matches!(
            &binding.target,
            cadmpeg_ir::appearance::AppearanceTarget::Face(face) if shape_face_ids.contains(&face.0)
        ) && binding.channels.get("precedence").map(String::as_str) == Some("face_over_object")
    }));
        assert_eq!(
            result
                .ir()
                .model
                .appearance_bindings
                .iter()
                .filter(|binding| {
                    matches!(
                        binding.target,
                        cadmpeg_ir::appearance::AppearanceTarget::Edge(_)
                    ) && binding.channels.get("precedence").map(String::as_str)
                        == Some("edge_array_over_line")
                })
                .count(),
            2
        );
        assert_eq!(
            result
                .ir()
                .model
                .appearance_bindings
                .iter()
                .filter(|binding| {
                    matches!(
                        binding.target,
                        cadmpeg_ir::appearance::AppearanceTarget::Vertex(_)
                    ) && binding.channels.get("precedence").map(String::as_str)
                        == Some("vertex_array_over_point")
                })
                .count(),
            2
        );
        assert!(crate::validate_native(result.ir()).is_empty());
        assert_valid_document(result.ir());
    }

    #[test]
    fn rejects_interleaved_new_string_hasher_payload() {
        let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects>
<ObjectData Count="1"><Object name="Shape"><Properties Count="1">
<Property name="Shape" type="Part::PropertyPartShape"><Part file=""/>
<StringHasher new="1" count="0"/><Interleaved/><StringHasher2 count="0"/>
</Property></Properties></Object></ObjectData></Document>"#;
        let error = FcstdCodec
            .decode(
                &mut Cursor::new(archive(document)),
                &DecodeOptions::default(),
            )
            .expect_err("interleaved string table must fail");

        assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    }

    #[test]
    fn rejects_interleaved_new_string_hasher_payload_when_parsed_directly() {
        let document = br#"<Document><StringHasher new="1" count="0"/><Interleaved/><StringHasher2 count="0"/></Document>"#;
        let error = parse(document, &[], &[]).expect_err("interleaved string table must fail");

        assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    }

    #[test]
    fn rejects_ambiguous_shape_carriers() {
        let duplicate_part = test_property(
            "Part::PropertyPartShape",
            "<Property><Part/><Part/></Property>",
        );
        assert!(matches!(
            parse(b"<Document/>", &[duplicate_part], &[]),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));

        let duplicate_map = test_property(
            "Part::PropertyPartShape",
            "<Property><Part/><ElementMap2/><ElementMap2/></Property>",
        );
        assert!(matches!(
            parse(b"<Document/>", &[duplicate_map], &[]),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
    }

    #[test]
    fn rejects_nested_element_map_successor() {
        let nested_map = test_property(
            "Part::PropertyPartShape",
            r#"<Property><Part ElementMap="1.0"/><ElementMap new="1" count="1"><Element key="compat" value="compat"/></ElementMap><Wrapper><ElementMap2/></Wrapper></Property>"#,
        );
        assert!(matches!(
            parse(b"<Document/>", &[nested_map], &[]),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
    }

    #[test]
    fn rejects_non_adjacent_element_map_successor() {
        let non_adjacent_map = test_property(
            "Part::PropertyPartShape",
            r#"<Property><Part ElementMap="1.0"/><ElementMap new="1" count="1"><Element key="compat" value="compat"/></ElementMap><Wrapper/><ElementMap2/></Property>"#,
        );
        assert!(matches!(
            parse(b"<Document/>", &[non_adjacent_map], &[]),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
    }

    #[test]
    fn rejects_element_map_without_compatibility_marker() {
        let unmarked_map = test_property(
            "Part::PropertyPartShape",
            r#"<Property><Part ElementMap="1.0"/><ElementMap2/></Property>"#,
        );
        assert!(matches!(
            parse(b"<Document/>", &[unmarked_map], &[]),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
    }

    #[test]
    fn ignores_non_shape_runtime_names() {
        let custom = test_property(
            "Custom::PropertyPartShape",
            "<Property><Part/><ElementMap2/></Property>",
        );
        let (tables, maps) = parse(b"<Document/>", &[custom], &[]).expect("unknown type");

        assert!(tables.is_empty());
        assert!(maps.is_empty());
    }

    #[test]
    fn rejects_ambiguous_string_table_property_ownership() {
        let xml = roxmltree::Document::parse("<Document><StringHasher count=\"0\"/></Document>")
            .expect("test XML");
        let node = xml.root_element().first_element_child().expect("hasher");
        let mut first = test_property("App::PropertyString", "<Property/>");
        first.byte_end = 1000;
        let mut second = test_property("App::PropertyString", "<Property/>");
        second.id = "fcstd:test:property#Other".into();
        second.byte_end = 1000;

        assert!(matches!(
            owning_property(node, &[first, second]),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
    }
}
