// SPDX-License-Identifier: Apache-2.0
//! Persistent string-table and element-map recovery.

use std::collections::{BTreeMap, HashMap};

use cadmpeg_core::decode::{bounded_len, DecodeContext};
use cadmpeg_core::CodecError;

use crate::native::element_map::{
    ElementMapGroup, ElementMapNode, ElementMapNodes, ElementMapRecord, ElementMappedName,
};
use crate::native::{
    EntryRecord, PropertyRecord, StringTableEntry, StringTableRecord, StringTables,
};
use crate::resource::{append_retained, collection_vec, materialized_bytes, reserve_vec_items, retained_join, retained_string, retained_suffix};
use crate::topology_transfer::TopologyOccurrence;

const MAX_TABLE_ENTRIES: usize = 10_000_000;
const MAX_MAP_NODES: usize = 1_000_000;
const MAX_GROUPS: usize = 1_000_000;
const MAX_NAMES: usize = 10_000_000;

struct LegacyElementRecord {
    indexed_name: String,
    mapped_name: String,
    string_ids: Vec<i64>,
}

struct MapPayload {
    source_entry: Option<String>,
    declared_count: usize,
    parsed: ParsedMap,
}

enum ElementMapCarrier<'a, 'input> {
    New(roxmltree::Node<'a, 'input>),
    Legacy(roxmltree::Node<'a, 'input>),
}

/// Recover every string table and element map carried by `Document.xml`.
pub(crate) fn parse(
    ctx: &DecodeContext<'_>,
    document: &[u8],
    file_version: usize,
    properties: &[PropertyRecord],
    entries: &[EntryRecord],
) -> Result<(StringTables, Vec<ElementMapRecord>), CodecError> {
    let text = std::str::from_utf8(document)
        .map_err(|_| CodecError::Malformed("Document.xml is not UTF-8".into()))?;
    let xml = roxmltree::Document::parse(text)
        .map_err(|error| CodecError::malformed(format_args!("invalid Document.xml: {error}")))?;
    validate_string_hasher_framing(xml.root_element())?;
    let mut entry_data = HashMap::new();
    for entry in entries {
        if !entry_data.contains_key(entry.name.as_str()) {
            ctx.charge_collection_items(1, "FreeCAD element entry lookup")?;
            entry_data.try_reserve(1).map_err(|_| crate::resource::collection_allocation_failed(ctx, 1, "FreeCAD element entry lookup"))?;
        }
        entry_data.insert(entry.name.as_str(), entry.data.as_slice());
    }

    let mut tables = Vec::new();
    for node in xml.descendants().filter(|node| node.has_tag_name("StringHasher")) {
        let index = tables.len();
        let save_all = require_bool(node.attribute("saveall").unwrap_or("0"))?;
        let threshold = parse_decimal(node.attribute("threshold").unwrap_or("0"), "threshold")?;
        let owner_property = owning_property(ctx, node, properties)?;
        let new_layout = node.attribute("new").is_some_and(|value| value != "0");
        let data_node = if new_layout {
            string_hasher_successor(node)?
        } else {
            node
        };
        let source_entry = data_node.attribute("file").filter(|name| !name.is_empty());
        let inline_bytes = source_entry.is_none().then(|| node_text_bytes(ctx, data_node)).transpose()?;
        let bytes = if let Some(name) = source_entry {
            *entry_data.get(name).ok_or_else(|| {
                CodecError::malformed(format_args!("StringHasher references missing entry {name}"))
            })?
        } else {
            inline_bytes.as_ref().map_or(&[] as &[u8], |(bytes, _)| bytes.as_slice())
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
        let entries = parse_string_table(ctx, bytes, declared_count, source_entry.is_some())?;
        reserve_vec_items(ctx, &mut tables, 1, "FreeCAD string table records")?;
        tables.push(
            StringTableRecord::try_new(
                index,
                owner_property,
                save_all,
                threshold,
                source_entry.map(|name| retained_string(ctx, name, "FreeCAD string table side-entry name")).transpose()?,
                entries,
            )
            .map_err(CodecError::Malformed)?,
        );
    }

    let mut maps = Vec::new();
    for property in properties
        .iter()
        .filter(|property| property.type_name == "Part::PropertyPartShape")
    {
        let property_xml = roxmltree::Document::parse(property.xml.text()).map_err(|error| {
            CodecError::malformed(format_args!(
                "invalid shape property XML {}: {error}",
                property.id
            ))
        })?;
        let Some((part, carrier)) = direct_element_map(property_xml.root_element())? else {
            continue;
        };
        let version = retained_string(ctx, part.attribute("ElementMap").unwrap_or(""), "FreeCAD element map version")?;
        let hasher_index = part
            .attribute("HasherIndex")
            .map(|value| parse_usize(value, "HasherIndex"))
            .transpose()?;
        let payload = match carrier {
            ElementMapCarrier::New(map_node) => {
                let declared_count = map_node
                    .attribute("count")
                    .map(|count| parse_usize(count, "ElementMap2 count"))
                    .transpose()?;
                let source_entry = map_node
                    .attribute("file")
                    .filter(|name| !name.is_empty())
                    .map(|name| retained_string(ctx, name, "FreeCAD element map side-entry name"))
                    .transpose()?;
                let inline_bytes = source_entry.is_none().then(|| node_text_bytes(ctx, map_node)).transpose()?;
                let bytes = if let Some(name) = source_entry.as_deref() {
                    *entry_data.get(name).ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "ElementMap2 references missing entry {name}"
                        ))
                    })?
                } else {
                    inline_bytes.as_ref().map_or(&[] as &[u8], |(bytes, _)| bytes.as_slice())
                };
                let parsed = parse_element_map(ctx, bytes, source_entry.is_some())?;
                let declared_count = match declared_count {
                    Some(count) => count,
                    None => mapped_name_count(&parsed),
                };
                Some(MapPayload {
                    source_entry,
                    declared_count,
                    parsed,
                })
            }
            ElementMapCarrier::Legacy(marker) => {
                parse_legacy_element_map(ctx, marker, file_version, &entry_data)?
            }
        };
        let Some(payload) = payload else {
            continue;
        };
        let MapPayload {
            source_entry,
            declared_count,
            parsed,
        } = payload;
        reserve_vec_items(ctx, &mut maps, 1, "FreeCAD element map records")?;
        maps.push(ElementMapRecord {
            id: crate::native::native_child_id_charged(ctx, "element-map", &property.id, "map")?,
            property: retained_string(ctx, &property.id, "FreeCAD element map property")?,
            version,
            hasher_index,
            source_entry,
            map_id: parsed.map_id,
            declared_count,
            postfixes: parsed.postfixes,
            maps: parsed.maps,
        });
    }
    Ok((StringTables::try_from(tables)?, maps))
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
        return Err(CodecError::malformed(format_args!(
            "string-table entry count exceeds {MAX_TABLE_ENTRIES}"
        )));
    }
    Ok(count)
}

/// Connect kernel indexed-map positions to every neutral placed occurrence.
pub(crate) fn bind_topology(ctx: &DecodeContext<'_>, maps: &mut [ElementMapRecord], occurrences: &[TopologyOccurrence]) -> Result<(), CodecError> {
    for map in maps {
        for occurrence in occurrences
            .iter()
            .filter(|occurrence| occurrence.property == map.property)
        {
            map.maps.bind_root_topology(
                ctx,
                occurrence.indexed_name,
                occurrence.source_index,
                &occurrence.topology_id,
            )?;
        }
    }
    Ok(())
}

fn owning_property(
    ctx: &DecodeContext<'_>,
    node: roxmltree::Node<'_, '_>,
    properties: &[PropertyRecord],
) -> Result<Option<String>, CodecError> {
    let start = node.range().start as u64;
    let mut owners = properties
        .iter()
        .filter(|property| property.xml.start() <= start && start < property.xml.end());
    let Some(owner) = owners.next() else {
        return Ok(None);
    };
    if owners.next().is_some() {
        return Err(CodecError::Malformed(
            "StringHasher has multiple enclosing properties".into(),
        ));
    }
    Ok(Some(retained_string(ctx, &owner.id, "FreeCAD string table owner")?))
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

        let mut marker_index = None;
        let mut marker_count = 0;
        let mut part_index = None;
        let mut part_count = 0;
        let mut successor_index = None;
        let mut successor_count = 0;
        for (index, child) in parent.children().filter(roxmltree::Node::is_element).enumerate() {
            if child.has_tag_name("StringHasher") {
                marker_index = Some(index);
                marker_count += 1;
            }
            if child.has_tag_name("Part") {
                part_index = Some(index);
                part_count += 1;
            }
            if child.has_tag_name("StringHasher2") {
                successor_index = Some(index);
                successor_count += 1;
            }
        }
        if marker_count != 1 {
            return Err(CodecError::Malformed(
                "StringHasher has duplicate direct carriers".into(),
            ));
        }
        if parent_is_shape_property {
            if part_count != 1 || marker_index <= part_index {
                return Err(CodecError::Malformed(
                    "StringHasher is not owned by a direct Part carrier".into(),
                ));
            }
        }
        let new_layout = node.attribute("new").is_some_and(|value| value != "0");
        if new_layout {
            if successor_count != 1 || successor_index != marker_index.map(|index| index + 1) {
                return Err(CodecError::Malformed(
                    "StringHasher new=1 is not followed by one direct StringHasher2".into(),
                ));
            }
        } else if successor_count != 0 {
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
        let mut prior_is_marker = false;
        let direct_successor = parent.children().filter(roxmltree::Node::is_element).any(|child| {
            let direct = prior_is_marker && child == node;
            prior_is_marker = child.has_tag_name("StringHasher");
            direct
        });
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
) -> Result<Option<(roxmltree::Node<'a, 'input>, ElementMapCarrier<'a, 'input>)>, CodecError> {
    let mut part = None;
    let mut marker = None;
    let mut map = None;
    let mut part_count = 0;
    let mut marker_count = 0;
    let mut map_count = 0;
    for (index, node) in root.children().filter(roxmltree::Node::is_element).enumerate() {
        if node.has_tag_name("Part") {
            part = Some((index, node));
            part_count += 1;
        }
        if node.has_tag_name("ElementMap") {
            marker = Some((index, node));
            marker_count += 1;
        }
        if node.has_tag_name("ElementMap2") {
            map = Some((index, node));
            map_count += 1;
        }
    }
    if part_count > 1 {
        return Err(CodecError::Malformed(
            "shape property has multiple direct Part carriers".into(),
        ));
    }
    let Some((part_index, part)) = part else {
        return Ok(None);
    };
    if marker_count > 1 {
        return Err(CodecError::Malformed(
            "shape property has multiple direct ElementMap markers".into(),
        ));
    }
    if map_count > 1 {
        return Err(CodecError::Malformed(
            "shape property has multiple direct ElementMap2 carriers".into(),
        ));
    }
    let Some((marker_index, marker)) = marker else {
        if map.is_some() {
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
    let is_new = marker
        .attribute("new")
        .map(require_bool)
        .transpose()?
        .unwrap_or(false);
    let Some((map_index, map)) = map else {
        if is_new {
            return Err(CodecError::Malformed(
                "new ElementMap marker has no direct ElementMap2 successor".into(),
            ));
        }
        return Ok(Some((
            part,
            ElementMapCarrier::Legacy(marker),
        )));
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
    Ok(Some((
        part,
        ElementMapCarrier::New(map),
    )))
}

fn element_map_size(parsed: &ParsedMap) -> Result<usize, CodecError> {
    let root = parsed.maps.root();
    let mapped_name_count = root
        .groups
        .iter()
        .flat_map(|group| &group.names)
        .map(Vec::len)
        .sum::<usize>();
    let child_element_count = root
        .groups
        .iter()
        .flat_map(|group| &group.children)
        .map(|child| {
            let count = child.split_ascii_whitespace().nth(2).ok_or_else(|| {
                CodecError::Malformed("element-map child descriptor has no count".into())
            })?;
            parse_usize(count, "element-map child count")
        })
        .try_fold(0_usize, |total, count| {
            total
                .checked_add(count?)
                .ok_or_else(|| CodecError::Malformed("element-map size overflows".into()))
        })?;
    mapped_name_count
        .checked_add(child_element_count)
        .ok_or_else(|| CodecError::Malformed("element-map size overflows".into()))
}

fn mapped_name_count(parsed: &ParsedMap) -> usize {
    parsed
        .maps
        .iter()
        .flat_map(|node| &node.groups)
        .flat_map(|group| &group.names)
        .flat_map(|chain| chain.iter())
        .count()
}

fn parse_legacy_element_map(
    ctx: &DecodeContext<'_>,
    marker: roxmltree::Node<'_, '_>,
    file_version: usize,
    entry_data: &HashMap<&str, &[u8]>,
) -> Result<Option<MapPayload>, CodecError> {
    let source_entry = marker
        .attribute("file")
        .filter(|name| !name.is_empty())
        .map(str::to_owned);
    if let Some(name) = source_entry.as_deref() {
        let bytes = *entry_data.get(name).ok_or_else(|| {
            CodecError::Malformed("legacy ElementMap references missing entry".into())
        })?;
        let text = std::str::from_utf8(bytes)
            .map_err(|_| CodecError::Malformed("legacy element map is not UTF-8".into()))?;
        let mut header = text.split_ascii_whitespace();
        let first = header.next().unwrap_or_default();
        if first == "BeginElementMap" && header.next() == Some("v1") {
            let parsed = parse_element_map(ctx, bytes, true)?;
            let declared_count = match marker.attribute("count") {
                Some(count) => parse_usize(count, "ElementMap count")?,
                None => element_map_size(&parsed)?,
            };
            return Ok(Some(MapPayload {
                source_entry,
                declared_count,
                parsed,
            }));
        }
        let (declared_count, records) = parse_legacy_stream(ctx, bytes, None)?;
        return Ok(Some(legacy_map_payload(
            ctx,
            records,
            declared_count,
            source_entry,
        )?));
    }

    let declared_count = parse_count(marker, "ElementMap")?;
    if declared_count == 0 {
        return Ok(None);
    }
    let records = if file_version > 1 {
        let (bytes, _reservation) = node_text_bytes(ctx, marker)?;
        parse_legacy_stream(ctx, &bytes, Some(declared_count))?.1
    } else {
        parse_legacy_elements(ctx, marker, declared_count)?
    };
    Ok(Some(legacy_map_payload(
        ctx,
        records,
        declared_count,
        source_entry,
    )?))
}

fn parse_legacy_stream(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    expected_count: Option<usize>,
) -> Result<(usize, Vec<LegacyElementRecord>), CodecError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| CodecError::Malformed("legacy element map is not UTF-8".into()))?;
    let mut tokens = text.split_ascii_whitespace();
    let count = expected_count.map_or_else(
        || {
            parse_usize(
                next_token(&mut tokens, "element-map record count")?,
                "element-map record count",
            )
        },
        Ok,
    )?;
    let records = parse_legacy_records(ctx, &mut tokens, count)?;
    if tokens.next().is_some() {
        return Err(CodecError::Malformed(
            "legacy element map has trailing data".into(),
        ));
    }
    Ok((count, records))
}

fn parse_legacy_records<'a>(
    ctx: &DecodeContext<'_>,
    tokens: &mut impl Iterator<Item = &'a str>,
    count: usize,
) -> Result<Vec<LegacyElementRecord>, CodecError> {
    if count > MAX_NAMES {
        return Err(CodecError::Malformed(
            "legacy element-map record count exceeds limit".into(),
        ));
    }
    let mut records = crate::resource::collection_vec(ctx, count, "FreeCAD legacy element records")?;
    for _ in 0..count {
        let indexed_name = retained_string(ctx, next_token(tokens, "legacy element indexed name")?, "FreeCAD legacy indexed name")?;
        let mapped_name = retained_string(ctx, next_token(tokens, "legacy mapped name")?, "FreeCAD legacy mapped name")?;
        let sid_count = parse_usize(
            next_token(tokens, "legacy string-id count")?,
            "legacy string-id count",
        )?;
        if sid_count > MAX_NAMES {
            return Err(CodecError::Malformed(
                "legacy string-id count exceeds limit".into(),
            ));
        }
        let mut string_ids = crate::resource::collection_vec(ctx, sid_count, "FreeCAD legacy string IDs")?;
        for _ in 0..sid_count {
            string_ids.push(
                next_token(tokens, "legacy string id")?
                    .parse::<i64>()
                    .map_err(|_| CodecError::Malformed("invalid legacy string id".into()))?,
            );
        }
        records.push(LegacyElementRecord {
            indexed_name,
            mapped_name,
            string_ids,
        });
    }
    Ok(records)
}

fn parse_legacy_elements(
    ctx: &DecodeContext<'_>,
    marker: roxmltree::Node<'_, '_>,
    count: usize,
) -> Result<Vec<LegacyElementRecord>, CodecError> {
    let element_nodes = marker
        .children()
        .filter(roxmltree::Node::is_element);
    let mut elements = collection_vec(ctx, count, "FreeCAD legacy element nodes")?;
    elements.extend(element_nodes);
    if elements.len() != count
        || elements
            .iter()
            .any(|element| !element.has_tag_name("Element"))
    {
        return Err(CodecError::Malformed(
            "legacy ElementMap count does not match direct Element children".into(),
        ));
    }
    let mut records = collection_vec(ctx, count, "FreeCAD legacy element records")?;
    for element in elements {
            let indexed_name = retained_string(ctx, element
                .attribute("value")
                .ok_or_else(|| CodecError::Malformed("legacy Element has no value".into()))?, "FreeCAD legacy indexed name")?;
            let mapped_name = retained_string(ctx, element
                .attribute("key")
                .ok_or_else(|| CodecError::Malformed("legacy Element has no key".into()))?, "FreeCAD legacy mapped name")?;
            let string_ids = element
                .attribute("sid")
                .map(|value| parse_legacy_string_ids(ctx, value))
                .transpose()?
                .unwrap_or_default();
            records.push(LegacyElementRecord {
                indexed_name,
                mapped_name,
                string_ids,
            });
    }
    Ok(records)
}

fn parse_legacy_string_ids(ctx: &DecodeContext<'_>, value: &str) -> Result<Vec<i64>, CodecError> {
    let mut ids = Vec::new();
    for token in value
        .split(|character: char| !character.is_ascii_digit() && character != '-')
        .filter(|token| !token.is_empty())
    {
        let id = token
                .parse::<i64>()
                .map_err(|_| CodecError::Malformed("invalid legacy string id".into()))?;
        if id != 0 {
            reserve_vec_items(ctx, &mut ids, 1, "FreeCAD legacy string IDs")?;
            ids.push(id);
        }
    }
    Ok(ids)
}

fn legacy_map_payload(
    ctx: &DecodeContext<'_>,
    records: Vec<LegacyElementRecord>,
    declared_count: usize,
    source_entry: Option<String>,
) -> Result<MapPayload, CodecError> {
    let mut groups = BTreeMap::<String, Vec<Vec<ElementMappedName>>>::new();
    for record in records {
        let (indexed_name, index) = split_indexed_name(ctx, &record.indexed_name)?;
        if !groups.contains_key(&indexed_name) {
            ctx.charge_collection_items(1, "FreeCAD legacy element groups")?;
        }
        let names = groups.entry(indexed_name).or_default();
        if names.len() <= index {
            let additional = index + 1 - names.len();
            ctx.charge_collection_items(additional as u64, "FreeCAD legacy name slots")?;
            names.try_reserve(additional).map_err(|_| crate::resource::collection_allocation_failed(ctx, additional as u64, "FreeCAD legacy name slots"))?;
            names.resize_with(index + 1, Vec::new);
        }
        reserve_vec_items(ctx, &mut names[index], 1, "FreeCAD legacy mapped names")?;
        names[index].push(ElementMappedName {
            encoded: retained_string(ctx, &record.mapped_name, "FreeCAD legacy encoded name")?,
            resolved: Some(record.mapped_name),
            string_ids: record.string_ids,
            topology_ids: Vec::new(),
        });
    }
    let parsed = ParsedMap {
        map_id: 0,
        postfixes: Vec::new(),
        maps: ElementMapNodes::from_root_names(ctx, 0, groups)?,
    };
    Ok(MapPayload {
        source_entry,
        declared_count,
        parsed,
    })
}

fn split_indexed_name(ctx: &DecodeContext<'_>, value: &str) -> Result<(String, usize), CodecError> {
    if value.is_empty() {
        return Err(CodecError::Malformed("legacy indexed name is empty".into()));
    }
    let suffix_start = value
        .as_bytes()
        .iter()
        .rposition(|character| !character.is_ascii_digit())
        .map_or(0, |position| position + 1);
    let (type_name, suffix) = value.split_at(suffix_start);
    if type_name.is_empty()
        || !type_name
            .bytes()
            .all(|character| character.is_ascii_alphabetic() || character == b'_')
    {
        return Err(CodecError::Malformed("invalid legacy indexed name".into()));
    }
    let index = if suffix.is_empty() {
        0
    } else {
        suffix
            .parse::<usize>()
            .map_err(|_| CodecError::Malformed("invalid legacy indexed-name index".into()))?
    };
    if index > MAX_NAMES {
        return Err(CodecError::Malformed(
            "legacy indexed-name index exceeds limit".into(),
        ));
    }
    Ok((retained_string(ctx, type_name, "FreeCAD legacy element family")?, index))
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

fn node_text_bytes<'a>(
    ctx: &'a DecodeContext<'_>,
    node: roxmltree::Node<'_, '_>,
) -> Result<(Vec<u8>, cadmpeg_core::decode::ScopedReservation<'a>), CodecError> {
    let count = node.children()
        .filter_map(|child| child.is_text().then(|| child.text()).flatten())
        .try_fold(0_usize, |total, text| total.checked_add(text.len()))
        .ok_or_else(|| CodecError::Malformed("inline element-map text length overflows".into()))?;
    let (mut bytes, reservation) = materialized_bytes(ctx, count, "FreeCAD inline element text")?;
    for text in node.children().filter_map(|child| child.is_text().then(|| child.text()).flatten()) {
        bytes.extend_from_slice(text.as_bytes());
    }
    Ok((bytes, reservation))
}

fn parse_count(node: roxmltree::Node<'_, '_>, kind: &str) -> Result<usize, CodecError> {
    let count = node.attribute("count").unwrap_or("0");
    let count = parse_usize(count, &format!("{kind} count"))?;
    if count > MAX_TABLE_ENTRIES {
        return Err(CodecError::malformed(format_args!(
            "{kind} count exceeds limit"
        )));
    }
    Ok(count)
}

/// Reads an element-map boolean attribute, which is `0`, `1`, `false` or `true`.
///
/// Any other text is malformed.
fn require_bool(value: &str) -> Result<bool, CodecError> {
    match value {
        "0" | "false" => Ok(false),
        "1" | "true" => Ok(true),
        _ => Err(CodecError::malformed(format_args!(
            "invalid boolean {value:?}"
        ))),
    }
}

fn parse_decimal(value: &str, field: &str) -> Result<i64, CodecError> {
    value
        .parse()
        .map_err(|_| CodecError::malformed(format_args!("invalid {field} {value:?}")))
}

fn parse_usize(value: &str, field: &str) -> Result<usize, CodecError> {
    value
        .parse()
        .map_err(|_| CodecError::malformed(format_args!("invalid {field} {value:?}")))
}

fn parse_hex(value: &str, field: &str) -> Result<i64, CodecError> {
    let (negative, digits) = value
        .strip_prefix('-')
        .map_or((false, value), |digits| (true, digits));
    if digits.is_empty() {
        return Err(CodecError::malformed(format_args!("empty {field}")));
    }
    let value = i64::from_str_radix(digits, 16)
        .map_err(|_| CodecError::malformed(format_args!("invalid {field} {value:?}")))?;
    Ok(if negative { -value } else { value })
}

fn parse_string_table(
    ctx: &DecodeContext<'_>,
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
    let mut output = crate::resource::collection_vec(ctx, capacity, "FreeCAD string table entries")?;
    let mut previous_id = 0_i64;
    for _ in 0..declared_count {
        scanner.skip_whitespace();
        let record_start = scanner.position;
        let header = scanner.token()?;
        let mut fields = header.split('.');
        let encoded_id_field = fields.next().ok_or_else(|| CodecError::Malformed(
            "string-table record has incomplete numeric header".into(),
        ))?;
        let flags_field = fields.next().ok_or_else(|| CodecError::Malformed(
            "string-table record has incomplete numeric header".into(),
        ))?;
        let relative = encoded_id_field.starts_with('-');
        let encoded_id = parse_hex(encoded_id_field, "string id")?;
        let string_id = if relative {
            previous_id
                .checked_add(-encoded_id)
                .ok_or_else(|| CodecError::Malformed("relative string id overflows".into()))?
        } else {
            encoded_id
        };
        let flags = u64::from_str_radix(flags_field, 16)
            .map_err(|_| CodecError::Malformed("invalid string-table flags".into()))?;
        let mut components = Vec::new();
        for (position, field) in fields.enumerate() {
            let encoded = parse_hex(field, "string component")?;
            let component = if relative {
                if let Some(previous) = output.last().and_then(|entry: &StringTableEntry| entry.components.get(position)) {
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
            reserve_vec_items(ctx, &mut components, 1, "FreeCAD string table components")?;
            components.push(component);
        }
        let payload = if flags & 0x8 == 0 {
            scanner.encoded_text(ctx)?
        } else {
            let derived_prefix = flags & (0x10 | 0x20 | 0x40) != 0;
            let encoded_postfix = flags & 0x4 != 0;
            let mut values = Vec::new();
            if !derived_prefix {
                reserve_vec_items(ctx, &mut values, 1, "FreeCAD string table value words")?;
                values.push(retained_string(ctx, scanner.token()?, "FreeCAD string table value word")?);
            }
            if !encoded_postfix {
                reserve_vec_items(ctx, &mut values, 1, "FreeCAD string table value words")?;
                values.push(retained_string(ctx, scanner.token()?, "FreeCAD string table value word")?);
            }
            retained_join(ctx, &values, " ", "FreeCAD string table joined value")?
        };
        let raw = retained_string(ctx, text[record_start..scanner.position]
            .trim_end_matches(char::is_whitespace), "FreeCAD string table raw record")?;
        output.push(StringTableEntry {
            string_id,
            flags,
            components,
            payload,
            raw,
        });
        previous_id = string_id;
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

    fn encoded_text(&mut self, ctx: &DecodeContext<'_>) -> Result<String, CodecError> {
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
        retained_string(ctx, &self.text[content_start..self.position - 1], "FreeCAD string table encoded text")
    }

    fn is_done(&self) -> bool {
        self.position == self.text.len()
    }
}

struct ParsedMap {
    map_id: u64,
    postfixes: Vec<String>,
    maps: ElementMapNodes,
}

fn parse_element_map(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    side_entry: bool,
) -> Result<ParsedMap, CodecError> {
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
    let mut postfixes = collection_vec(ctx, postfix_count, "FreeCAD element map postfixes")?;
    for _ in 0..postfix_count {
        postfixes.push(retained_string(ctx, next_token(&mut tokens, "postfix")?, "FreeCAD element map postfix text")?);
    }
    expect(&mut tokens, "MapCount")?;
    let map_count = next_count(&mut tokens, "map count", MAX_MAP_NODES)?;
    // Each map node consumes at least one whitespace-separated token, so its count
    // cannot exceed the element map's byte length.
    let map_capacity = bounded_len(map_count as u64, 1, text.len())
        .ok_or_else(|| CodecError::Malformed("element-map node count exceeds input".into()))?;
    let mut maps = crate::resource::collection_vec(ctx, map_capacity, "FreeCAD element map nodes")?;
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
        let mut groups = crate::resource::collection_vec(ctx, group_capacity, "FreeCAD element map groups")?;
        for _ in 0..group_count {
            let indexed_name = retained_string(ctx, next_token(&mut tokens, "indexed name")?, "FreeCAD indexed element name")?;
            expect(&mut tokens, "ChildCount")?;
            let child_count = next_count(&mut tokens, "child count", MAX_NAMES)?;
            // Each child consumes at least one token, so its count cannot exceed the byte length.
            let child_capacity =
                bounded_len(child_count as u64, 1, text.len()).ok_or_else(|| {
                    CodecError::Malformed("element-map child count exceeds input".into())
                })?;
            let mut children = crate::resource::collection_vec(ctx, child_capacity, "FreeCAD element map children")?;
            for _ in 0..child_count {
                let fields = (0..7)
                    .map(|_| next_token(&mut tokens, "child descriptor"))
                    .collect::<Result<Vec<_>, _>>()?;
                children.push(retained_join(ctx, &fields, " ", "FreeCAD element child descriptor")?);
            }
            expect(&mut tokens, "NameCount")?;
            let name_count = next_count(&mut tokens, "name count", MAX_NAMES)?;
            // Each name consumes at least one token, so its count cannot exceed the byte length.
            let name_capacity = bounded_len(name_count as u64, 1, text.len()).ok_or_else(|| {
                CodecError::Malformed("element-map name count exceeds input".into())
            })?;
            let mut names = crate::resource::collection_vec(ctx, name_capacity, "FreeCAD element map names")?;
            for _ in 0..name_count {
                let mut chain = Vec::new();
                loop {
                    let encoded = next_token(&mut tokens, "mapped name")?;
                    if encoded == "0" {
                        break;
                    }
                    reserve_vec_items(ctx, &mut chain, 1, "FreeCAD mapped name chain")?;
                    chain.push(parse_mapped_name(ctx, encoded, &postfixes)?);
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
        maps: maps.try_into().map_err(CodecError::Malformed)?,
    })
}

fn parse_mapped_name(ctx: &DecodeContext<'_>, encoded: &str, postfixes: &[String]) -> Result<ElementMappedName, CodecError> {
    let field_count = encoded.split('.').count();
    let mut fields = collection_vec(ctx, field_count, "FreeCAD mapped name fields")?;
    fields.extend(encoded.split('.'));
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
            (retained_suffix(ctx, prefix, &element.to_string(), "FreeCAD mapped name base")?, 2, 3)
        } else if let Some(base) = fields[0]
            .strip_prefix(';')
            .or_else(|| fields[0].strip_prefix('$'))
        {
            (retained_string(ctx, base, "FreeCAD mapped name base")?, 1, 2)
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
        append_retained(ctx, &mut resolved, postfixes.get(postfix_index - 1).ok_or_else(|| {
            CodecError::Malformed("mapped-name postfix index is out of range".into())
        })?, "FreeCAD mapped name postfix")?;
    }
    let id_count = fields.iter().skip(id_position).filter(|value| !value.is_empty()).count();
    let mut string_ids = collection_vec(ctx, id_count, "FreeCAD mapped name string IDs")?;
    for value in fields.iter().skip(id_position).filter(|value| !value.is_empty()) {
        string_ids.push(parse_hex(value, "mapped-name string id")?);
    }
    Ok(ElementMappedName {
        encoded: retained_string(ctx, encoded, "FreeCAD encoded mapped name")?,
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
        .ok_or_else(|| CodecError::malformed(format_args!("element map ends before {field}")))
}

fn expect<'a>(
    tokens: &mut impl Iterator<Item = &'a str>,
    expected: &str,
) -> Result<(), CodecError> {
    let actual = next_token(tokens, expected)?;
    if actual != expected {
        return Err(CodecError::malformed(format_args!(
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
        return Err(CodecError::malformed(format_args!("{field} exceeds limit")));
    }
    Ok(value)
}

fn next_u64<'a>(
    tokens: &mut impl Iterator<Item = &'a str>,
    field: &str,
) -> Result<u64, CodecError> {
    next_token(tokens, field)?
        .parse()
        .map_err(|_| CodecError::malformed(format_args!("invalid {field}")))
}

#[cfg(test)]
mod tests;
