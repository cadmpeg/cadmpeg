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
use crate::topology_transfer::TopologyOccurrence;

fn recover_metadata<T>(
    result: Result<T, CodecError>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    field: &str,
) -> Result<Option<T>, CodecError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error @ (CodecError::Malformed(_) | CodecError::Truncated { .. })) => {
            losses.push(
                crate::loss::FreecadLossCode::ElementMapMetadataUnresolved.note(format!(
                    "{field} remains source-only: {error}; independent shape geometry is retained"
                )),
            );
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

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
    declared_count: Option<usize>,
    parsed: ParsedMap,
}

/// The two property ranges with the greatest ends at or before `start`.
/// A point query can prove both ownership and ambiguity without scanning ranges.
struct OwnershipPrefix<'a> {
    start: u64,
    furthest: &'a PropertyRecord,
    second_furthest: Option<&'a PropertyRecord>,
}

enum ElementMapCarrier<'a, 'input> {
    New(roxmltree::Node<'a, 'input>),
    Legacy(roxmltree::Node<'a, 'input>),
}

/// Recover every string table and element map carried by `Document.xml`.
pub(crate) fn parse(
    ctx: &DecodeContext<'_>,
    xml: &roxmltree::Document<'_>,
    file_version: Option<usize>,
    properties: &[PropertyRecord],
    entries: &[EntryRecord],
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(StringTables, Vec<ElementMapRecord>), CodecError> {
    // Orphan carriers are diagnosed once; each marker still proves its own
    // framing so a damaged table cannot erase independently framed peers.
    // discarded-value: framing errors become diagnostics; node admission below is authoritative
    let _ = recover_metadata(
        validate_string_hasher_framing(xml.root_element()),
        losses,
        "StringHasher carrier framing",
    )?;
    let mut entry_data = HashMap::new();
    for entry in entries {
        entry_data.insert(entry.name(), entry.data());
    }

    let hasher_nodes = xml
        .descendants()
        .filter(|node| node.has_tag_name("StringHasher"));
    let owner_count = if hasher_nodes.clone().next().is_some() {
        properties.len()
    } else {
        0
    };
    let mut ownership = Vec::new();
    ownership.extend(
        properties
            .iter()
            .take(owner_count)
            .map(|property| OwnershipPrefix {
                start: property.xml.start(),
                furthest: property,
                second_furthest: None,
            }),
    );
    ownership.sort_unstable_by_key(|left| left.start);

    let mut furthest: Option<&PropertyRecord> = None;
    let mut second_furthest: Option<&PropertyRecord> = None;
    for prefix in &mut ownership {
        match furthest {
            Some(previous) if previous.xml.end() >= prefix.furthest.xml.end() => {
                if second_furthest.is_none_or(|second| second.xml.end() < prefix.furthest.xml.end())
                {
                    second_furthest = Some(prefix.furthest);
                }
                prefix.furthest = previous;
            }
            _ => {
                second_furthest = furthest;
                furthest = Some(prefix.furthest);
            }
        }
        prefix.second_furthest = second_furthest;
    }

    let mut tables = Vec::new();
    let mut framing_by_parent = HashMap::new();
    for node in hasher_nodes {
        let index = tables.len();
        let save_all = recover_metadata(
            require_bool(node.attribute("saveall").unwrap_or("0")),
            losses,
            "StringHasher saveall",
        )?;
        let threshold = recover_metadata(
            parse_decimal(node.attribute("threshold").unwrap_or("0"), "threshold"),
            losses,
            "StringHasher threshold",
        )?;
        let owner_property = owning_property(node, &ownership)?;
        let parent = node.parent();
        let parent_id = parent.map(|parent| parent.id());
        let framed = if let Some(framed) = framing_by_parent.get(&parent_id) {
            *framed
        } else {
            let framed = recover_metadata(
                validate_string_hasher_node(xml.root_element(), node),
                losses,
                "StringHasher carrier framing",
            )?
            .is_some();
            framing_by_parent.insert(parent_id, framed);
            framed
        };
        let parsed = (|| {
            if !framed {
                return Ok(None);
            }
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
                    CodecError::malformed(format_args!(
                        "StringHasher references missing entry {name}"
                    ))
                })?
            } else {
                inline_bytes.as_ref().map_or(&[][..], Vec::as_slice)
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
            StringTableRecord::try_new(
                index,
                None,
                save_all,
                threshold,
                source_entry.map(ToOwned::to_owned),
                Some(entries),
            )
            .map(Some)
            .map_err(CodecError::Malformed)
        })();
        let payload = recover_metadata(parsed, losses, "StringHasher payload")?.flatten();
        let mut record = match payload {
            Some(record) => record,
            None => StringTableRecord::try_new(index, None, save_all, threshold, None, None)
                .map_err(CodecError::Malformed)?,
        };
        record.owner_property = owner_property;
        tables.push(record);
    }

    let mut maps = Vec::new();
    let mut ids_by_table = HashMap::<usize, std::collections::HashSet<i64>>::new();
    for property in properties
        .iter()
        .filter(|property| property.type_name == "Part::PropertyPartShape")
    {
        let parsed = (|| {
            let property_xml = ctx
                .parse_xml(property.xml.text(), "FreeCAD XML tree")
                .map_err(|error| {
                    let CodecError::Malformed(error) = error else {
                        return error;
                    };
                    CodecError::malformed(format_args!(
                        "invalid shape property XML {}: {error}",
                        property.id
                    ))
                })?;
            let Some((part, carrier)) = direct_element_map(property_xml.root_element())? else {
                return Ok(None);
            };
            let version = part.attribute("ElementMap").unwrap_or("").to_owned();
            let mut hasher_index = part
                .attribute("HasherIndex")
                .map(|value| {
                    recover_metadata(
                        parse_usize(value, "HasherIndex"),
                        losses,
                        "Part HasherIndex",
                    )
                })
                .transpose()?
                .flatten();
            if hasher_index.is_some_and(|index| tables.get(index).is_none()) {
                recover_metadata::<()>(
                    Err(CodecError::Malformed(
                        "HasherIndex references an unavailable string table".into(),
                    )),
                    losses,
                    "Part HasherIndex",
                )?;
                hasher_index = None;
            }
            let payload = match carrier {
                ElementMapCarrier::New(map_node) => {
                    let declared_count = map_node
                        .attribute("count")
                        .map(|count| {
                            recover_metadata(
                                parse_usize(count, "ElementMap2 count"),
                                losses,
                                "ElementMap2 count",
                            )
                        })
                        .transpose()?;
                    let source_entry = map_node
                        .attribute("file")
                        .filter(|name| !name.is_empty())
                        .map(str::to_owned);
                    let inline_bytes = source_entry.is_none().then(|| node_text_bytes(map_node));
                    let bytes = if let Some(name) = source_entry.as_deref() {
                        *entry_data.get(name).ok_or_else(|| {
                            CodecError::malformed(format_args!(
                                "ElementMap2 references missing entry {name}"
                            ))
                        })?
                    } else {
                        inline_bytes.as_ref().map_or(&[][..], Vec::as_slice)
                    };
                    let parsed = parse_element_map(bytes, source_entry.is_some())?;
                    let declared_count =
                        declared_count.unwrap_or_else(|| Some(mapped_name_count(&parsed)));
                    Some(MapPayload {
                        source_entry,
                        declared_count,
                        parsed,
                    })
                }
                ElementMapCarrier::Legacy(marker) => parse_legacy_element_map(
                    marker,
                    file_version,
                    &entry_data,
                    &property.id,
                    losses,
                )?,
            };
            let Some(payload) = payload else {
                return Ok(None);
            };
            let MapPayload {
                source_entry,
                declared_count,
                parsed,
            } = payload;
            if let Some((index, entries)) = hasher_index.and_then(|index| {
                tables
                    .get(index)
                    .and_then(StringTableRecord::entries)
                    .map(|entries| (index, entries))
            }) {
                ids_by_table.entry(index).or_insert_with(|| {
                    let mut known_ids = std::collections::HashSet::new();
                    for entry in entries {
                        known_ids.insert(entry.string_id);
                    }
                    known_ids
                });
                if let Some(known_ids) = ids_by_table.get(&index) {
                    let mut missing = false;
                    for id in parsed
                        .maps
                        .root()
                        .groups
                        .iter()
                        .flat_map(|group| &group.names)
                        .flatten()
                        .flat_map(|name| &name.string_ids)
                    {
                        missing |= !known_ids.contains(id);
                    }
                    if missing {
                        recover_metadata::<()>(
                            Err(CodecError::Malformed(
                                "mapped names reference unavailable persistent string IDs".into(),
                            )),
                            losses,
                            "Part HasherIndex",
                        )?;
                        hasher_index = None;
                    }
                }
            }
            Ok(Some(ElementMapRecord {
                id: crate::native::native_child_id("element-map", &property.id, "map"),
                property: property.id.clone(),
                version,
                hasher_index,
                source_entry,
                map_id: parsed.map_id,
                declared_count,
                postfixes: parsed.postfixes,
                maps: parsed.maps,
            }))
        })();
        if let Some(map) = recover_metadata(parsed, losses, &property.id)?.flatten() {
            maps.push(map);
        }
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
            "FreeCAD string table entry count {count} exceeds {MAX_TABLE_ENTRIES}"
        )));
    }
    Ok(count)
}

/// Connect kernel indexed-map positions to every neutral placed occurrence.
pub(crate) fn bind_topology(maps: &mut [ElementMapRecord], occurrences: &[TopologyOccurrence]) {
    for map in maps {
        for occurrence in occurrences
            .iter()
            .filter(|occurrence| occurrence.property == map.property)
        {
            map.maps.bind_root_topology(
                occurrence.indexed_name,
                occurrence.source_index,
                &occurrence.topology_id,
            );
        }
    }
}

fn owning_property(
    node: roxmltree::Node<'_, '_>,
    ownership: &[OwnershipPrefix<'_>],
) -> Result<Option<String>, CodecError> {
    let start = cadmpeg_core::decode::u64_from_index(node.range().start);
    let upper = ownership.partition_point(|prefix| prefix.start <= start);
    let Some(prefix) = upper.checked_sub(1).and_then(|index| ownership.get(index)) else {
        return Ok(None);
    };
    if prefix.furthest.xml.end() <= start {
        return Ok(None);
    }
    if prefix
        .second_furthest
        .is_some_and(|property| property.xml.end() > start)
    {
        return Err(CodecError::Malformed(
            "StringHasher has multiple enclosing properties".into(),
        ));
    }
    Ok(Some(prefix.furthest.id.clone()))
}

fn validate_string_hasher_framing(
    document_root: roxmltree::Node<'_, '_>,
) -> Result<(), CodecError> {
    for node in document_root
        .descendants()
        .filter(|node| node.has_tag_name("StringHasher"))
    {
        validate_string_hasher_node(document_root, node)?;
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
        let direct_successor = parent
            .children()
            .filter(roxmltree::Node::is_element)
            .any(|child| {
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

fn validate_string_hasher_node(
    document_root: roxmltree::Node<'_, '_>,
    node: roxmltree::Node<'_, '_>,
) -> Result<(), CodecError> {
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
    for (index, child) in parent
        .children()
        .filter(roxmltree::Node::is_element)
        .enumerate()
    {
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
    if parent_is_shape_property && (part_count != 1 || marker_index <= part_index) {
        return Err(CodecError::Malformed(
            "StringHasher is not owned by a direct Part carrier".into(),
        ));
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
    for (index, node) in root
        .children()
        .filter(roxmltree::Node::is_element)
        .enumerate()
    {
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
        return Ok(Some((part, ElementMapCarrier::Legacy(marker))));
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
    Ok(Some((part, ElementMapCarrier::New(map))))
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
    marker: roxmltree::Node<'_, '_>,
    file_version: Option<usize>,
    entry_data: &HashMap<&str, &[u8]>,
    owner: &str,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<Option<MapPayload>, CodecError> {
    let source_entry = marker
        .attribute("file")
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned);
    if let Some(name) = source_entry.as_deref() {
        let bytes = *entry_data.get(name).ok_or_else(|| {
            CodecError::Malformed("legacy ElementMap references missing entry".into())
        })?;
        let text = std::str::from_utf8(bytes)
            .map_err(|_| CodecError::Malformed("legacy element map is not UTF-8".into()))?;
        let mut header = text.split_ascii_whitespace();
        let first = header.next().unwrap_or_default();
        if first == "BeginElementMap" && header.next() == Some("v1") {
            let parsed = parse_element_map(bytes, true)?;
            let declared_count = match marker.attribute("count") {
                Some(count) => parse_usize(count, "ElementMap count")?,
                None => element_map_size(&parsed)?,
            };
            return Ok(Some(MapPayload {
                source_entry,
                declared_count: Some(declared_count),
                parsed,
            }));
        }
        let (declared_count, records) = parse_legacy_stream(bytes, None)?;
        return Ok(Some(legacy_map_payload(
            records,
            declared_count,
            source_entry,
        )?));
    }

    let declared_count = parse_count(marker, "ElementMap")?;
    if declared_count == 0 {
        return Ok(None);
    }
    let Some(file_version) = file_version else {
        let message = format!("property {owner} inline legacy ElementMap requires a verified FileVersion; source XML retained without assigning an encoding");
        losses.push(crate::loss::FreecadLossCode::ElementMapVersionUnresolved.note(message));
        return Ok(None);
    };
    let records = if file_version > 1 {
        let bytes = node_text_bytes(marker);
        parse_legacy_stream(&bytes, Some(declared_count))?.1
    } else {
        parse_legacy_elements(marker, declared_count)?
    };
    Ok(Some(legacy_map_payload(
        records,
        declared_count,
        source_entry,
    )?))
}

fn parse_legacy_stream(
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
    let records = parse_legacy_records(&mut tokens, count)?;
    if tokens.next().is_some() {
        return Err(CodecError::Malformed(
            "legacy element map has trailing data".into(),
        ));
    }
    Ok((count, records))
}

fn parse_legacy_records<'a>(
    tokens: &mut impl Iterator<Item = &'a str>,
    count: usize,
) -> Result<Vec<LegacyElementRecord>, CodecError> {
    if count > MAX_NAMES {
        return Err(CodecError::malformed(format_args!(
            "FreeCAD legacy element record count {count} exceeds {MAX_NAMES}"
        )));
    }
    let mut records = Vec::new();
    for _ in 0..count {
        let indexed_name = next_token(tokens, "legacy element indexed name")?.to_owned();
        let mapped_name = next_token(tokens, "legacy mapped name")?.to_owned();
        let sid_count = parse_usize(
            next_token(tokens, "legacy string-id count")?,
            "legacy string-id count",
        )?;
        if sid_count > MAX_NAMES {
            return Err(CodecError::malformed(format_args!(
                "FreeCAD legacy string ID count {sid_count} exceeds {MAX_NAMES}"
            )));
        }
        let mut string_ids = Vec::new();
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
    marker: roxmltree::Node<'_, '_>,
    count: usize,
) -> Result<Vec<LegacyElementRecord>, CodecError> {
    let element_nodes = marker.children().filter(roxmltree::Node::is_element);
    let mut elements = Vec::new();
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
    let mut records = Vec::new();
    for element in elements {
        let indexed_name = element
            .attribute("value")
            .ok_or_else(|| CodecError::Malformed("legacy Element has no value".into()))?
            .to_owned();
        let mapped_name = element
            .attribute("key")
            .ok_or_else(|| CodecError::Malformed("legacy Element has no key".into()))?
            .to_owned();
        let string_ids = element
            .attribute("sid")
            .map(parse_legacy_string_ids)
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

fn parse_legacy_string_ids(value: &str) -> Result<Vec<i64>, CodecError> {
    let mut ids = Vec::new();
    for token in value
        .split(|character: char| !character.is_ascii_digit() && character != '-')
        .filter(|token| !token.is_empty())
    {
        let id = token
            .parse::<i64>()
            .map_err(|_| CodecError::Malformed("invalid legacy string id".into()))?;
        if id != 0 {
            ids.push(id);
        }
    }
    Ok(ids)
}

fn legacy_map_payload(
    records: Vec<LegacyElementRecord>,
    declared_count: usize,
    source_entry: Option<String>,
) -> Result<MapPayload, CodecError> {
    let mut groups = BTreeMap::<String, Vec<Vec<ElementMappedName>>>::new();
    for record in records {
        let (indexed_name, index) = split_indexed_name(&record.indexed_name)?;
        let names = groups.entry(indexed_name).or_default();
        if names.len() <= index {
            let additional = index + 1 - names.len();
            for _ in 0..additional {
                names.push(Vec::new());
            }
        }
        names[index].push(ElementMappedName {
            encoded: record.mapped_name.clone(),
            resolved: Some(record.mapped_name),
            string_ids: record.string_ids,
            topology_ids: Vec::new(),
        });
    }
    let parsed = ParsedMap {
        map_id: 0,
        postfixes: Vec::new(),
        maps: ElementMapNodes::from_root_names(0, groups),
    };
    Ok(MapPayload {
        source_entry,
        declared_count: Some(declared_count),
        parsed,
    })
}

fn split_indexed_name(value: &str) -> Result<(String, usize), CodecError> {
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
        return Err(CodecError::malformed(format_args!(
            "FreeCAD legacy indexed name {index} exceeds {MAX_NAMES}"
        )));
    }
    Ok((type_name.to_owned(), index))
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
    let mut bytes = Vec::new();
    for text in node
        .children()
        .filter_map(|child| child.is_text().then(|| child.text()).flatten())
    {
        bytes.extend_from_slice(text.as_bytes());
    }
    bytes
}

fn parse_count(node: roxmltree::Node<'_, '_>, kind: &str) -> Result<usize, CodecError> {
    let count = node.attribute("count").unwrap_or("0");
    let count = parse_usize(count, &format!("{kind} count"))?;
    if count > MAX_TABLE_ENTRIES {
        return Err(CodecError::malformed(format_args!(
            "FreeCAD naming record count {count} exceeds {MAX_TABLE_ENTRIES}"
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
    bounded_len(
        cadmpeg_core::decode::u64_from_index(declared_count),
        1,
        text.len(),
    )
    .ok_or_else(|| CodecError::Malformed("string-table record count exceeds input".into()))?;
    let mut output = Vec::new();
    let mut previous_id = 0_i64;
    for _ in 0..declared_count {
        scanner.skip_whitespace();
        let record_start = scanner.position;
        let header = scanner.token()?;
        let mut fields = header.split('.');
        let encoded_id_field = fields.next().ok_or_else(|| {
            CodecError::Malformed("string-table record has incomplete numeric header".into())
        })?;
        let flags_field = fields.next().ok_or_else(|| {
            CodecError::Malformed("string-table record has incomplete numeric header".into())
        })?;
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
                if let Some(previous) = output
                    .last()
                    .and_then(|entry: &StringTableEntry| entry.components.get(position))
                {
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
    maps: ElementMapNodes,
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
    let mut postfixes = Vec::new();
    for _ in 0..postfix_count {
        postfixes.push(next_token(&mut tokens, "postfix")?.to_owned());
    }
    expect(&mut tokens, "MapCount")?;
    let map_count = next_count(&mut tokens, "map count", MAX_MAP_NODES)?;
    // Each map node consumes at least one whitespace-separated token, so its count
    // cannot exceed the element map's byte length.
    bounded_len(
        cadmpeg_core::decode::u64_from_index(map_count),
        1,
        text.len(),
    )
    .ok_or_else(|| CodecError::Malformed("element-map node count exceeds input".into()))?;
    let mut maps = Vec::new();
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
        bounded_len(
            cadmpeg_core::decode::u64_from_index(group_count),
            1,
            text.len(),
        )
        .ok_or_else(|| CodecError::Malformed("element-map group count exceeds input".into()))?;
        let mut groups = Vec::new();
        for _ in 0..group_count {
            let indexed_name = next_token(&mut tokens, "indexed name")?.to_owned();
            expect(&mut tokens, "ChildCount")?;
            let child_count = next_count(&mut tokens, "child count", MAX_NAMES)?;
            // Each child consumes at least one token, so its count cannot exceed the byte length.
            bounded_len(
                cadmpeg_core::decode::u64_from_index(child_count),
                1,
                text.len(),
            )
            .ok_or_else(|| CodecError::Malformed("element-map child count exceeds input".into()))?;
            let mut children = Vec::new();
            for _ in 0..child_count {
                let fields = (0..7)
                    .map(|_| next_token(&mut tokens, "child descriptor"))
                    .collect::<Result<Vec<_>, _>>()?;
                children.push(fields.join(" "));
            }
            expect(&mut tokens, "NameCount")?;
            let name_count = next_count(&mut tokens, "name count", MAX_NAMES)?;
            // Each name consumes at least one token, so its count cannot exceed the byte length.
            bounded_len(
                cadmpeg_core::decode::u64_from_index(name_count),
                1,
                text.len(),
            )
            .ok_or_else(|| CodecError::Malformed("element-map name count exceeds input".into()))?;
            let mut names = Vec::new();
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

fn parse_mapped_name(encoded: &str, postfixes: &[String]) -> Result<ElementMappedName, CodecError> {
    let mut fields = Vec::new();
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
    let mut string_ids = Vec::new();
    for value in fields
        .iter()
        .skip(id_position)
        .filter(|value| !value.is_empty())
    {
        string_ids.push(parse_hex(value, "mapped-name string id")?);
    }
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
        return Err(CodecError::malformed(format_args!(
            "FreeCAD element map count {value} exceeds {limit}"
        )));
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
