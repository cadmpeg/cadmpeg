// SPDX-License-Identifier: Apache-2.0
//! Persistent string-table and element-map recovery.

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::wire::cursor::bounded_len;

use crate::native::{
    ElementMapGroup, ElementMapNode, ElementMapRecord, ElementMappedName, EntryRecord,
    PropertyRecord, StringTableEntry, StringTableRecord,
};

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
    let entry_data = entries
        .iter()
        .map(|entry| (entry.name.as_str(), entry.data.as_slice()))
        .collect::<HashMap<_, _>>();

    let mut tables = Vec::new();
    let mut claimed_new_layout_records = HashSet::new();
    for node in xml
        .descendants()
        .filter(|node| node.has_tag_name("StringHasher"))
    {
        let index = tables.len();
        let save_all = parse_bool(node.attribute("saveall").unwrap_or("0"))?;
        let threshold = parse_decimal(node.attribute("threshold").unwrap_or("0"), "threshold")?;
        let owner_property = owning_property(node, properties);
        let new_layout = node.attribute("new").is_some_and(|value| value != "0");
        let data_node = if new_layout {
            node.parent()
                .into_iter()
                .flat_map(|parent| parent.children())
                .find(|sibling| {
                    sibling.has_tag_name("StringHasher2")
                        && sibling.range().start > node.range().end
                        && !claimed_new_layout_records.contains(&sibling.range().start)
                })
                .ok_or_else(|| {
                    CodecError::Malformed("StringHasher new=1 has no StringHasher2 record".into())
                })?
        } else {
            node
        };
        if new_layout {
            claimed_new_layout_records.insert(data_node.range().start);
        }
        let source_entry = data_node.attribute("file").filter(|name| !name.is_empty());
        let bytes = if let Some(name) = source_entry {
            *entry_data.get(name).ok_or_else(|| {
                CodecError::Malformed(format!("StringHasher references missing entry {name}"))
            })?
        } else {
            node_text_bytes(text, data_node)
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
        .filter(|property| property.type_name.contains("PropertyPartShape"))
    {
        let property_xml = roxmltree::Document::parse(&property.raw_xml).map_err(|error| {
            CodecError::Malformed(format!(
                "invalid shape property XML {}: {error}",
                property.id
            ))
        })?;
        let Some(part) = property_xml
            .descendants()
            .find(|node| node.has_tag_name("Part"))
        else {
            continue;
        };
        let version = part.attribute("ElementMap").unwrap_or("").to_owned();
        let hasher_index = part
            .attribute("HasherIndex")
            .map(|value| parse_usize(value, "HasherIndex"))
            .transpose()?;
        let Some(map_node) = property_xml
            .descendants()
            .find(|node| node.has_tag_name("ElementMap2"))
        else {
            continue;
        };
        let declared_count = map_node
            .attribute("count")
            .map(|count| parse_usize(count, "ElementMap count"))
            .transpose()?;
        let source_entry = map_node.attribute("file").filter(|name| !name.is_empty());
        let bytes = if let Some(name) = source_entry {
            *entry_data.get(name).ok_or_else(|| {
                CodecError::Malformed(format!("ElementMap references missing entry {name}"))
            })?
        } else {
            node_text_bytes(&property.raw_xml, map_node)
        };
        let parsed = parse_element_map(bytes, source_entry.is_some())?;
        let actual_count = parsed.maps.last().map_or(0, |node| {
            node.groups
                .iter()
                .flat_map(|group| &group.names)
                .filter(|chain| !chain.is_empty())
                .count()
        });
        if declared_count.is_some_and(|declared| declared != actual_count) {
            return Err(CodecError::Malformed(format!(
                "ElementMap for {} declares {} entries but contains {actual_count}",
                property.id,
                declared_count.unwrap_or_default(),
            )));
        }
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

/// Connect transient indexed-name positions to every neutral placed occurrence.
///
/// Neutral arenas preserve source traversal order. Repeated outer placements
/// therefore form consecutive copies of the same indexed-element sequence.
pub(crate) fn bind_topology(
    maps: &mut [ElementMapRecord],
    payload_ids: &HashMap<&str, &str>,
    ir: &CadIr,
) {
    for map in maps {
        let Some(payload_id) = payload_ids.get(map.property.as_str()).copied() else {
            continue;
        };
        let Some(root) = map.maps.last_mut() else {
            continue;
        };
        for group in &mut root.groups {
            let ids = topology_ids(ir, payload_id, &group.indexed_name);
            let populated_positions = group
                .names
                .iter()
                .enumerate()
                .filter_map(|(position, names)| (!names.is_empty()).then_some(position))
                .collect::<Vec<_>>();
            if populated_positions.is_empty()
                || !ids.len().is_multiple_of(populated_positions.len())
            {
                continue;
            }
            let name_count = populated_positions.len();
            for (position, id) in ids.into_iter().enumerate() {
                let slot = populated_positions[position % name_count];
                for name in &mut group.names[slot] {
                    name.topology_ids.push(id.clone());
                }
            }
        }
    }
}

fn topology_ids(ir: &CadIr, payload: &str, kind: &str) -> Vec<String> {
    let prefix = format!("{}:", crate::native::id_key(payload));
    let belongs_to_payload = |id: &str| crate::native::id_key(id).starts_with(&prefix);
    match kind {
        "Vertex" => ir
            .model
            .vertices
            .iter()
            .map(|entity| entity.id.0.as_str())
            .filter(|id| belongs_to_payload(id))
            .map(str::to_owned)
            .collect(),
        "Edge" => ir
            .model
            .edges
            .iter()
            .map(|entity| entity.id.0.as_str())
            .filter(|id| belongs_to_payload(id))
            .map(str::to_owned)
            .collect(),
        "Wire" => ir
            .model
            .loops
            .iter()
            .map(|entity| entity.id.0.as_str())
            .filter(|id| belongs_to_payload(id))
            .map(str::to_owned)
            .chain(
                ir.model
                    .shells
                    .iter()
                    .filter(|entity| !entity.wire_edges.is_empty())
                    .map(|entity| entity.id.0.as_str())
                    .filter(|id| belongs_to_payload(id))
                    .map(str::to_owned),
            )
            .collect(),
        "Face" => ir
            .model
            .faces
            .iter()
            .map(|entity| entity.id.0.as_str())
            .filter(|id| belongs_to_payload(id))
            .map(str::to_owned)
            .collect(),
        "Shell" => ir
            .model
            .shells
            .iter()
            .map(|entity| entity.id.0.as_str())
            .filter(|id| belongs_to_payload(id))
            .map(str::to_owned)
            .collect(),
        "Solid" | "CompSolid" | "Compound" => ir
            .model
            .bodies
            .iter()
            .map(|entity| entity.id.0.as_str())
            .filter(|id| belongs_to_payload(id))
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

fn owning_property(node: roxmltree::Node<'_, '_>, properties: &[PropertyRecord]) -> Option<String> {
    let start = node.range().start as u64;
    properties
        .iter()
        .find(|property| property.byte_start <= start && start < property.byte_end)
        .map(|property| property.id.clone())
}

fn node_text_bytes<'a>(text: &'a str, node: roxmltree::Node<'_, '_>) -> &'a [u8] {
    node.children()
        .find(roxmltree::Node::is_text)
        .map_or(&[], |child| text[child.range()].as_bytes())
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

struct ParsedMap {
    map_id: u64,
    postfixes: Vec<String>,
    maps: Vec<ElementMapNode>,
}

fn parse_element_map(bytes: &[u8], side_entry: bool) -> Result<ParsedMap, CodecError> {
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
}
