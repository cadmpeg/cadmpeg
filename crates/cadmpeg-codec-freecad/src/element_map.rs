// SPDX-License-Identifier: Apache-2.0
//! Persistent string-table and element-map recovery.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{bounded_len, DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;

use crate::native::element_map::{
    ElementMapGroup, ElementMapNode, ElementMapNodes, ElementMapRecord, ElementMappedName,
    ScopedData,
};
use crate::native::{
    EntryRecord, PropertyRecord, StringTableEntry, StringTableRecord, StringTables,
};
use crate::topology_transfer::TopologyOccurrence;

fn element_map_malformed(ctx: &DecodeContext<'_>, message: std::fmt::Arguments<'_>) -> CodecError {
    crate::resource::malformed_charged(ctx, message, "FreeCAD element map diagnostic")
}

const MAX_TABLE_ENTRIES: usize = 10_000_000;
const MAX_MAP_NODES: usize = 1_000_000;
const MAX_GROUPS: usize = 1_000_000;
const MAX_NAMES: usize = 10_000_000;

type LegacyNameGroups<'c> = ScopedData<'c, BTreeMap<String, Vec<Vec<ElementMappedName>>>>;

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
    xml: &roxmltree::Document<'_>,
    file_version: usize,
    properties: &[PropertyRecord],
    entries: &[EntryRecord],
) -> Result<(StringTables, Vec<ElementMapRecord>), CodecError> {
    let root = ctx.xml_root_element(xml, "FreeCAD element-map XML root")?;
    let (data, storage) = ctx.with_scoped_storage("FreeCAD string-hasher carriers", || {
        validate_string_hasher_framing(ctx, root)
    })?;
    let hashers = ScopedData { data, storage };
    let mut entry_data = EntryLookup {
        entries,
        index: None,
    };
    let mut owners = None;
    let mut tables = Vec::new();
    let mut hashers_iter = hashers.data.iter().copied();
    while hashers_iter.len() != 0 {
        let Some(node) = ctx.next_charged(&mut hashers_iter, "FreeCAD string-hasher scan")? else {
            break;
        };
        let index = tables.len();
        let save_all = require_bool(
            ctx,
            ctx.xml_attribute(node, "saveall", "FreeCAD element-map XML attribute")?
                .unwrap_or("0"),
        )?;
        let threshold = parse_decimal(
            ctx,
            ctx.xml_attribute(node, "threshold", "FreeCAD element-map XML attribute")?
                .unwrap_or("0"),
            "threshold",
        )?;
        let owner_property = if node.parent() == Some(root) {
            None
        } else {
            owning_property(
                ctx,
                node,
                match &mut owners {
                    Some(owners) => owners,
                    missing @ None => missing.insert(PropertyOwners::new(ctx, properties)?),
                },
            )?
        };
        let new_layout = ctx
            .xml_attribute(node, "new", "FreeCAD element-map XML attribute")?
            .is_some_and(|value| value != "0");
        let data_node = if new_layout {
            string_hasher_successor(ctx, node)?
        } else {
            node
        };
        let source_entry = ctx
            .xml_attribute(data_node, "file", "FreeCAD element-map XML attribute")?
            .filter(|name| !name.is_empty());
        let inline_bytes = source_entry
            .is_none()
            .then(|| node_text_bytes(ctx, data_node))
            .transpose()?;
        let bytes = if let Some(name) = source_entry {
            entry_data.get(ctx, name)?.ok_or_else(|| {
                element_map_malformed(
                    ctx,
                    format_args!("StringHasher references missing entry {name}"),
                )
            })?
        } else {
            inline_bytes
                .as_ref()
                .map_or(&[][..], |(bytes, _)| bytes.as_slice())
        };
        let declared_count = if source_entry.is_some() {
            let header_count = string_table_header_count(ctx, bytes)?;
            if !new_layout {
                let xml_count = parse_count(ctx, data_node, "StringHasher")?;
                if xml_count != header_count {
                    return Err(CodecError::Malformed(
                        "string-table XML and side-entry counts disagree".into(),
                    ));
                }
            }
            header_count
        } else {
            parse_count(ctx, data_node, "StringHasher")?
        };
        let entries = parse_string_table(ctx, bytes, declared_count, source_entry.is_some())?;
        ctx.reserve_vec(&mut tables, 1, "FreeCAD string table records")?;
        let mut table_index_storage = ctx.reserve_scoped(0, "FreeCAD string table index")?;
        tables.push(
            StringTableRecord::from_parts_with_admission(
                (
                    index,
                    owner_property,
                    save_all,
                    threshold,
                    source_entry
                        .map(|name| {
                            ctx.copy_retained_text(name, "FreeCAD string table side-entry name")
                        })
                        .transpose()?,
                ),
                entries,
                |length, operation| {
                    ctx.charge_work(cadmpeg_core::decode::u64_from_index(length), operation)
                },
                |seen, id| {
                    ctx.contains_btree_set(seen, &id, "FreeCAD string table component lookup")
                },
                |seen, id| {
                    table_index_storage.with_storage(|| {
                        ctx.insert_btree_set(seen, id, "FreeCAD string table index")
                    })
                },
            )?
            .map_err(CodecError::malformed)?,
        );
    }
    drop(hashers_iter);
    drop(hashers);
    drop(owners);

    let mut maps = Vec::new();
    let mut properties = properties.iter();
    while properties.len() != 0 {
        let Some(property) =
            ctx.next_charged(&mut properties, "FreeCAD element-map property scan")?
        else {
            break;
        };
        if property.type_name != "Part::PropertyPartShape" {
            continue;
        }
        let admitted_property_xml = ctx
            .parse_xml(property.xml.text(), "FreeCAD XML tree")
            .map_err(|error| {
                let CodecError::Malformed(error) = error else {
                    return error;
                };
                element_map_malformed(
                    ctx,
                    format_args!("invalid shape property XML {}: {error}", property.id),
                )
            })?;
        let property_xml = admitted_property_xml.document();
        let Some((part, carrier)) = direct_element_map(
            ctx,
            ctx.xml_root_element(property_xml, "FreeCAD shape-property XML root")?,
        )?
        else {
            continue;
        };
        let version = ctx
            .xml_attribute(part, "ElementMap", "FreeCAD element-map XML attribute")?
            .unwrap_or("");
        let hasher_index = ctx
            .xml_attribute(part, "HasherIndex", "FreeCAD element-map XML attribute")?
            .map(|value| parse_usize(ctx, value, "HasherIndex"))
            .transpose()?;
        let payload = match carrier {
            ElementMapCarrier::New(map_node) => {
                let declared_count = ctx
                    .xml_attribute(map_node, "count", "FreeCAD element-map XML attribute")?
                    .map(|count| parse_usize(ctx, count, "ElementMap2 count"))
                    .transpose()?;
                let source_entry = ctx
                    .xml_attribute(map_node, "file", "FreeCAD element-map XML attribute")?
                    .filter(|name| !name.is_empty())
                    .map(|name| ctx.copy_retained_text(name, "FreeCAD element map side-entry name"))
                    .transpose()?;
                let inline_bytes = source_entry
                    .is_none()
                    .then(|| node_text_bytes(ctx, map_node))
                    .transpose()?;
                let bytes = if let Some(name) = source_entry.as_deref() {
                    entry_data.get(ctx, name)?.ok_or_else(|| {
                        element_map_malformed(
                            ctx,
                            format_args!("ElementMap2 references missing entry {name}"),
                        )
                    })?
                } else {
                    inline_bytes
                        .as_ref()
                        .map_or(&[][..], |(bytes, _)| bytes.as_slice())
                };
                let parsed = parse_element_map(ctx, bytes, source_entry.is_some())?;
                let declared_count = match declared_count {
                    Some(count) => count,
                    None => mapped_name_count(ctx, &parsed)?,
                };
                Some(MapPayload {
                    source_entry,
                    declared_count,
                    parsed,
                })
            }
            ElementMapCarrier::Legacy(marker) => {
                parse_legacy_element_map(ctx, marker, file_version, &mut entry_data)?
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
        ctx.reserve_vec(&mut maps, 1, "FreeCAD element map records")?;
        maps.push(ElementMapRecord {
            id: crate::native::native_child_id_charged(ctx, "element-map", &property.id, "map")?,
            property: ctx.copy_retained_text(&property.id, "FreeCAD element map property")?,
            version: ctx.copy_retained_text(version, "FreeCAD element map version")?,
            hasher_index,
            source_entry,
            map_id: parsed.map_id,
            declared_count,
            postfixes: parsed.postfixes,
            maps: parsed.maps,
        });
    }
    let string_tables =
        StringTables::from_records_with_admission(tables, ctx)?.map_err(CodecError::from)?;
    Ok((string_tables, maps))
}

struct EntryLookup<'a, 'c> {
    entries: &'a [EntryRecord],
    index: Option<(BTreeMap<&'a str, &'a [u8]>, ScopedReservation<'c>)>,
}

impl<'a, 'c> EntryLookup<'a, 'c> {
    fn get(
        &mut self,
        ctx: &'c DecodeContext<'_>,
        name: &str,
    ) -> Result<Option<&'a [u8]>, CodecError> {
        let (index, _) = match &mut self.index {
            Some(index) => index,
            missing @ None => missing.insert(
                ctx.collect_scoped_btree_map(
                    self.entries
                        .iter()
                        .map(|entry| (entry.name(), entry.data())),
                    "FreeCAD element entry lookup",
                )?,
            ),
        };
        Ok(ctx
            .get_btree_map(index, name, "FreeCAD element entry lookup")?
            .copied())
    }
}

fn string_table_header_count(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<usize, CodecError> {
    let text = ctx
        .validate_utf8(bytes, "FreeCAD element-map UTF-8")?
        .map_err(|_| CodecError::Malformed("string table is not UTF-8".into()))?;
    let mut tokens = TextScanner::new_ascii(text);
    if tokens.next(ctx)? != Some("StringTableStart") || tokens.next(ctx)? != Some("v1") {
        return Err(CodecError::Malformed(
            "string-table side entry has invalid header".into(),
        ));
    }
    let count = tokens
        .next(ctx)?
        .ok_or_else(|| CodecError::Malformed("string-table side entry has no count".into()))?;
    let count = parse_usize(ctx, count, "string-table header count")?;
    if count > MAX_TABLE_ENTRIES {
        return Err(element_map_malformed(
            ctx,
            format_args!("string-table entry count exceeds {MAX_TABLE_ENTRIES}"),
        ));
    }
    Ok(count)
}

/// Whether any owning map can consume source topology bindings.
pub(crate) fn has_topology_consumers(
    ctx: &DecodeContext<'_>,
    maps: &[ElementMapRecord],
) -> Result<bool, CodecError> {
    let mut maps = maps.iter();
    while maps.len() != 0 {
        let Some(map) = ctx.next_charged(&mut maps, "FreeCAD element topology consumers")? else {
            break;
        };
        if map.maps.has_topology_names(ctx)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Connect kernel indexed-map positions to every neutral placed occurrence.
pub(crate) fn bind_topology(
    ctx: &DecodeContext<'_>,
    maps: &mut [ElementMapRecord],
    occurrences: &[TopologyOccurrence],
) -> Result<(), CodecError> {
    if maps.is_empty() || occurrences.is_empty() || !has_topology_consumers(ctx, maps)? {
        return Ok(());
    }
    let (data, storage) = ctx.collect_scoped_btree_groups(
        occurrences
            .iter()
            .map(|occurrence| (occurrence.property.as_str(), occurrence)),
        "FreeCAD element topology occurrence index",
    )?;
    let by_property = ScopedData { data, storage };
    let mut map_iter = maps.iter_mut();
    while map_iter.len() != 0 {
        let Some(map) = ctx.next_charged(&mut map_iter, "FreeCAD element topology map scan")?
        else {
            break;
        };
        if !map.maps.has_topology_names(ctx)? {
            continue;
        }
        let Some(occurrences) = ctx.get_btree_map(
            &by_property.data,
            map.property.as_str(),
            "FreeCAD element topology occurrence lookup",
        )?
        else {
            continue;
        };
        map.maps.bind_root_topology(
            ctx,
            occurrences.iter().map(|occurrence| {
                (
                    occurrence.indexed_name,
                    occurrence.source_index,
                    occurrence.topology_id.as_str(),
                )
            }),
        )?;
    }
    Ok(())
}

struct PropertyOwners<'a, 'c> {
    spans: Vec<(u64, Option<&'a PropertyRecord>, bool)>,
    _storage: cadmpeg_core::decode::ScopedReservation<'c>,
}

impl<'a, 'c> PropertyOwners<'a, 'c> {
    fn new(
        ctx: &'c DecodeContext<'_>,
        properties: &'a [PropertyRecord],
    ) -> Result<Self, CodecError> {
        let (data, storage) = ctx.collect_scoped_btree_groups(
            properties
                .iter()
                .enumerate()
                .flat_map(|(position, property)| {
                    [
                        (property.xml.start(), (position + 1, true)),
                        (property.xml.end(), (position + 1, false)),
                    ]
                }),
            "FreeCAD property ownership endpoints",
        )?;
        let events = ScopedData { data, storage };
        let (data, storage) =
            ctx.temporary_vec(events.data.len(), "FreeCAD property ownership spans")?;
        let mut spans = ScopedData { data, storage };
        let mut active_count = 0_usize;
        let mut active_xor = 0_usize;
        let mut offsets = events.data.iter();
        while offsets.len() != 0 {
            let Some((offset, events)) =
                ctx.next_charged(&mut offsets, "FreeCAD property ownership sweep")?
            else {
                break;
            };
            let mut event_iter = events.iter();
            while event_iter.len() != 0 {
                let Some(&(position, start)) =
                    ctx.next_charged(&mut event_iter, "FreeCAD property ownership events")?
                else {
                    break;
                };
                active_xor ^= position;
                if start {
                    active_count += 1;
                } else {
                    active_count -= 1;
                }
            }
            let owner = (active_count == 1).then(|| &properties[active_xor - 1]);
            spans.data.push((*offset, owner, active_count > 1));
        }
        let ScopedData {
            data: spans,
            storage,
        } = spans;
        Ok(Self {
            spans,
            _storage: storage,
        })
    }
}

fn owning_property(
    ctx: &DecodeContext<'_>,
    node: roxmltree::Node<'_, '_>,
    owners: &PropertyOwners<'_, '_>,
) -> Result<Option<String>, CodecError> {
    let start = cadmpeg_core::decode::u64_from_index(node.range().start);
    let position = ctx.partition_point(
        &owners.spans,
        |span| Ok(span.0 <= start),
        "FreeCAD property ownership lookup",
    )?;
    let Some((_, owner, ambiguous)) = position
        .checked_sub(1)
        .map(|position| &owners.spans[position])
    else {
        return Ok(None);
    };
    if *ambiguous {
        return Err(CodecError::Malformed(
            "StringHasher has multiple enclosing properties".into(),
        ));
    }
    owner
        .map(|owner| ctx.copy_retained_text(&owner.id, "FreeCAD string table owner"))
        .transpose()
}

fn validate_string_hasher_framing<'a, 'input>(
    ctx: &DecodeContext<'_>,
    document_root: roxmltree::Node<'a, 'input>,
) -> Result<Vec<roxmltree::Node<'a, 'input>>, CodecError> {
    let mut hashers = Vec::new();
    let mut descendants = document_root.descendants();
    while descendants.len() != 0 {
        let Some(node) = ctx.next_charged(&mut descendants, "FreeCAD string-hasher descendants")?
        else {
            break;
        };
        if ctx.xml_has_tag_name(node, "StringHasher2", "FreeCAD string-hasher tag")? {
            let Some(parent) = node.parent() else {
                return Err(CodecError::Malformed(
                    "StringHasher2 has no enclosing root".into(),
                ));
            };
            let mut prior_is_marker = false;
            let mut children = parent.children();
            let mut direct_successor = false;
            while let Some(child) =
                ctx.next_charged(&mut children, "FreeCAD string-hasher successor search")?
            {
                if !child.is_element() {
                    continue;
                }
                if prior_is_marker && child == node {
                    direct_successor = true;
                    break;
                }
                prior_is_marker =
                    ctx.xml_has_tag_name(child, "StringHasher", "FreeCAD string-hasher tag")?;
            }
            if !direct_successor {
                return Err(CodecError::Malformed(
                    "StringHasher2 is not the direct successor of StringHasher".into(),
                ));
            }
            continue;
        }
        if !ctx.xml_has_tag_name(node, "StringHasher", "FreeCAD string-hasher tag")? {
            continue;
        }

        let Some(parent) = node.parent() else {
            return Err(CodecError::Malformed(
                "StringHasher has no enclosing root".into(),
            ));
        };
        let parent_is_document = parent == document_root;
        let parent_is_shape_property = is_shape_property(ctx, parent)?;
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
        let mut children = parent.children();
        let mut index = 0;
        while let Some(child) =
            ctx.next_charged(&mut children, "FreeCAD string-hasher carrier children")?
        {
            if !child.is_element() {
                continue;
            }
            if ctx.xml_has_tag_name(child, "StringHasher", "FreeCAD string-hasher carrier tag")? {
                marker_index = Some(index);
                marker_count += 1;
            }
            if ctx.xml_has_tag_name(child, "Part", "FreeCAD string-hasher carrier tag")? {
                part_index = Some(index);
                part_count += 1;
            }
            if ctx.xml_has_tag_name(child, "StringHasher2", "FreeCAD string-hasher carrier tag")? {
                successor_index = Some(index);
                successor_count += 1;
            }
            index += 1;
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
        let new_layout = ctx
            .xml_attribute(node, "new", "FreeCAD element-map XML attribute")?
            .is_some_and(|value| value != "0");
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
        ctx.push_vec(&mut hashers, node, "FreeCAD string-hasher carriers")?;
    }

    Ok(hashers)
}

fn is_shape_property(
    ctx: &DecodeContext<'_>,
    node: roxmltree::Node<'_, '_>,
) -> Result<bool, CodecError> {
    Ok(node.is_element()
        && (ctx.xml_has_tag_name(node, "Property", "FreeCAD property tag")?
            || ctx.xml_has_tag_name(node, "_Property", "FreeCAD property tag")?)
        && ctx.xml_attribute(node, "type", "FreeCAD element-map XML attribute")?
            == Some("Part::PropertyPartShape"))
}

fn direct_element_map<'a, 'input>(
    ctx: &DecodeContext<'_>,
    root: roxmltree::Node<'a, 'input>,
) -> Result<Option<(roxmltree::Node<'a, 'input>, ElementMapCarrier<'a, 'input>)>, CodecError> {
    let mut part = None;
    let mut marker = None;
    let mut map = None;
    let mut part_count = 0;
    let mut marker_count = 0;
    let mut map_count = 0;
    let mut children = root.children();
    let mut index = 0;
    while let Some(node) =
        ctx.next_charged(&mut children, "FreeCAD element-map carrier children")?
    {
        if !node.is_element() {
            continue;
        }
        if ctx.xml_has_tag_name(node, "Part", "FreeCAD element-map carrier tag")? {
            part = Some((index, node));
            part_count += 1;
        }
        if ctx.xml_has_tag_name(node, "ElementMap", "FreeCAD element-map carrier tag")? {
            marker = Some((index, node));
            marker_count += 1;
        }
        if ctx.xml_has_tag_name(node, "ElementMap2", "FreeCAD element-map carrier tag")? {
            map = Some((index, node));
            map_count += 1;
        }
        index += 1;
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
    let is_new = ctx
        .xml_attribute(marker, "new", "FreeCAD element-map XML attribute")?
        .map(|value| require_bool(ctx, value))
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

fn element_map_size(ctx: &DecodeContext<'_>, parsed: &ParsedMap) -> Result<usize, CodecError> {
    let mut total = 0_usize;
    let mut groups = parsed.maps.root().groups.iter();
    while groups.len() != 0 {
        let Some(group) = ctx.next_charged(&mut groups, "FreeCAD element-map size groups")? else {
            break;
        };
        let mut names = group.names.iter();
        while names.len() != 0 {
            let Some(chain) = ctx.next_charged(&mut names, "FreeCAD element-map size names")?
            else {
                break;
            };
            total = total
                .checked_add(chain.len())
                .ok_or_else(|| CodecError::malformed("element-map size overflows"))?;
        }
        let mut children = group.children.iter();
        while children.len() != 0 {
            let Some(child) =
                ctx.next_charged(&mut children, "FreeCAD element-map size children")?
            else {
                break;
            };
            let mut fields = TextScanner::new_ascii(child);
            next_token(ctx, &mut fields, "child index")?;
            next_token(ctx, &mut fields, "child offset")?;
            let count = next_token(ctx, &mut fields, "child count")?;
            total = total
                .checked_add(parse_usize(ctx, count, "element-map child count")?)
                .ok_or_else(|| CodecError::malformed("element-map size overflows"))?;
        }
    }
    Ok(total)
}

fn mapped_name_count(ctx: &DecodeContext<'_>, parsed: &ParsedMap) -> Result<usize, CodecError> {
    let mut total = 0_usize;
    let mut nodes = parsed.maps.iter();
    while nodes.len() != 0 {
        let Some(node) = ctx.next_charged(&mut nodes, "FreeCAD mapped-name node count")? else {
            break;
        };
        let mut groups = node.groups.iter();
        while groups.len() != 0 {
            let Some(group) = ctx.next_charged(&mut groups, "FreeCAD mapped-name group count")?
            else {
                break;
            };
            let mut chains = group.names.iter();
            while chains.len() != 0 {
                let Some(chain) =
                    ctx.next_charged(&mut chains, "FreeCAD mapped-name chain count")?
                else {
                    break;
                };
                total = total
                    .checked_add(chain.len())
                    .ok_or_else(|| CodecError::malformed("element-map size overflows"))?;
            }
        }
    }
    Ok(total)
}

fn parse_legacy_element_map<'c>(
    ctx: &'c DecodeContext<'_>,
    marker: roxmltree::Node<'_, '_>,
    file_version: usize,
    entry_data: &mut EntryLookup<'_, 'c>,
) -> Result<Option<MapPayload>, CodecError> {
    let source_entry = ctx
        .xml_attribute(marker, "file", "FreeCAD element-map XML attribute")?
        .filter(|name| !name.is_empty())
        .map(|name| ctx.copy_retained_text(name, "FreeCAD legacy element map side-entry name"))
        .transpose()?;
    if let Some(name) = source_entry.as_deref() {
        let bytes = entry_data.get(ctx, name)?.ok_or_else(|| {
            CodecError::Malformed("legacy ElementMap references missing entry".into())
        })?;
        let text = ctx
            .validate_utf8(bytes, "FreeCAD element-map UTF-8")?
            .map_err(|_| CodecError::Malformed("legacy element map is not UTF-8".into()))?;
        let mut header = TextScanner::new_ascii(text);
        let first = header.next(ctx)?.unwrap_or_default();
        if first == "BeginElementMap" && header.next(ctx)? == Some("v1") {
            let parsed = parse_element_map(ctx, bytes, true)?;
            let declared_count =
                match ctx.xml_attribute(marker, "count", "FreeCAD element-map XML attribute")? {
                    Some(count) => parse_usize(ctx, count, "ElementMap count")?,
                    None => element_map_size(ctx, &parsed)?,
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

    let declared_count = parse_count(ctx, marker, "ElementMap")?;
    if declared_count == 0 {
        return Ok(None);
    }
    let records = if file_version > 1 {
        let (data, storage) = node_text_bytes(ctx, marker)?;
        let bytes = ScopedData { data, storage };
        parse_legacy_stream(ctx, &bytes.data, Some(declared_count))?.1
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

fn parse_legacy_stream<'c>(
    ctx: &'c DecodeContext<'_>,
    bytes: &[u8],
    expected_count: Option<usize>,
) -> Result<(usize, LegacyNameGroups<'c>), CodecError> {
    let text = ctx
        .validate_utf8(bytes, "FreeCAD element-map UTF-8")?
        .map_err(|_| CodecError::Malformed("legacy element map is not UTF-8".into()))?;
    let mut tokens = TextScanner::new_ascii(text);
    let count = expected_count.map_or_else(
        || {
            parse_usize(
                ctx,
                next_token(ctx, &mut tokens, "element-map record count")?,
                "element-map record count",
            )
        },
        Ok,
    )?;
    bounded_len(cadmpeg_core::decode::u64_from_index(count), 1, bytes.len())
        .ok_or_else(|| CodecError::malformed("legacy element-map record count exceeds input"))?;
    let groups = parse_legacy_records(ctx, &mut tokens, count)?;
    if tokens.next(ctx)?.is_some() {
        return Err(CodecError::Malformed(
            "legacy element map has trailing data".into(),
        ));
    }
    Ok((count, groups))
}

fn parse_legacy_records<'c>(
    ctx: &'c DecodeContext<'_>,
    tokens: &mut TextScanner<'_>,
    count: usize,
) -> Result<LegacyNameGroups<'c>, CodecError> {
    if count > MAX_NAMES {
        return Err(CodecError::Malformed(
            "legacy element-map record count exceeds limit".into(),
        ));
    }
    let mut groups = ScopedData {
        data: BTreeMap::new(),
        storage: ctx.reserve_scoped(0, "FreeCAD legacy element groups")?,
    };
    let mut records_left = 0..count;
    while !records_left.is_empty()
        && ctx
            .next_charged(&mut records_left, "FreeCAD element-map record step")?
            .is_some()
    {
        let indexed_name = next_token(ctx, tokens, "legacy element indexed name")?;
        let mapped_name = ctx.copy_retained_text(
            next_token(ctx, tokens, "legacy mapped name")?,
            "FreeCAD legacy mapped name",
        )?;
        let sid_count = parse_usize(
            ctx,
            next_token(ctx, tokens, "legacy string-id count")?,
            "legacy string-id count",
        )?;
        if sid_count > MAX_NAMES {
            return Err(CodecError::Malformed(
                "legacy string-id count exceeds limit".into(),
            ));
        }
        let sid_capacity = bounded_len(
            cadmpeg_core::decode::u64_from_index(sid_count),
            1,
            tokens.text.len() - tokens.position,
        )
        .ok_or_else(|| CodecError::malformed("legacy string-id count exceeds input"))?;
        let mut string_ids = ctx.collection_vec(sid_capacity, "FreeCAD legacy string IDs")?;
        let mut records_left = 0..sid_count;
        while !records_left.is_empty()
            && ctx
                .next_charged(&mut records_left, "FreeCAD element-map record step")?
                .is_some()
        {
            string_ids.push(
                ctx.parse_text::<i64>(
                    next_token(ctx, tokens, "legacy string id")?,
                    "FreeCAD legacy string-id number",
                )?
                .map_err(|_| CodecError::Malformed("invalid legacy string id".into()))?,
            );
        }
        append_legacy_name(ctx, &mut groups, indexed_name, mapped_name, string_ids)?;
    }
    Ok(groups)
}

fn parse_legacy_elements<'c>(
    ctx: &'c DecodeContext<'_>,
    marker: roxmltree::Node<'_, '_>,
    count: usize,
) -> Result<LegacyNameGroups<'c>, CodecError> {
    let mut groups = ScopedData {
        data: BTreeMap::new(),
        storage: ctx.reserve_scoped(0, "FreeCAD legacy element groups")?,
    };
    let mut record_count = 0;
    let mut children = marker.children();
    while let Some(element) = ctx.next_charged(&mut children, "FreeCAD legacy element children")? {
        if !element.is_element() {
            continue;
        }
        if !ctx.xml_has_tag_name(element, "Element", "FreeCAD legacy element tag")?
            || record_count == count
        {
            return Err(CodecError::Malformed(
                "legacy ElementMap count does not match direct Element children".into(),
            ));
        }
        let indexed_name = ctx
            .xml_attribute(element, "value", "FreeCAD element-map XML attribute")?
            .ok_or_else(|| CodecError::Malformed("legacy Element has no value".into()))?;
        let mapped_name = ctx.copy_retained_text(
            ctx.xml_attribute(element, "key", "FreeCAD element-map XML attribute")?
                .ok_or_else(|| CodecError::Malformed("legacy Element has no key".into()))?,
            "FreeCAD legacy mapped name",
        )?;
        let string_ids = ctx
            .xml_attribute(element, "sid", "FreeCAD element-map XML attribute")?
            .map(|value| parse_legacy_string_ids(ctx, value))
            .transpose()?
            .unwrap_or_default();
        append_legacy_name(ctx, &mut groups, indexed_name, mapped_name, string_ids)?;
        record_count += 1;
    }
    if record_count != count {
        return Err(CodecError::Malformed(
            "legacy ElementMap count does not match direct Element children".into(),
        ));
    }
    Ok(groups)
}

fn parse_legacy_string_ids(ctx: &DecodeContext<'_>, value: &str) -> Result<Vec<i64>, CodecError> {
    let mut ids = Vec::new();
    let mut fields = TextScanner::new(value);
    while let Some(token) = fields.next_legacy_id(ctx)? {
        let id = ctx
            .parse_text::<i64>(token, "FreeCAD legacy string-id number")?
            .map_err(|_| CodecError::malformed("invalid legacy string id"))?;
        if id != 0 {
            ctx.reserve_vec(&mut ids, 1, "FreeCAD legacy string IDs")?;
            ids.push(id);
        }
    }
    Ok(ids)
}

fn append_legacy_name(
    ctx: &DecodeContext<'_>,
    groups: &mut LegacyNameGroups<'_>,
    indexed: &str,
    mapped_name: String,
    string_ids: Vec<i64>,
) -> Result<(), CodecError> {
    let (family, index) = split_indexed_name(ctx, indexed)?;
    if !ctx.contains_key_btree_map(&groups.data, family, "FreeCAD legacy element group lookup")? {
        let family = ctx.copy_retained_text(family, "FreeCAD legacy element family")?;
        groups.storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut groups.data,
                family,
                Vec::new(),
                "FreeCAD legacy element groups",
            )
        })?;
    }
    let names = ctx
        .get_mut_btree_map(
            &mut groups.data,
            family,
            "FreeCAD legacy element group lookup",
        )?
        .ok_or_else(|| CodecError::malformed("legacy element group was not inserted"))?;
    if names.len() <= index {
        let additional = index + 1 - names.len();
        ctx.reserve_vec(names, additional, "FreeCAD legacy name slots")?;
        for _ in ctx.admit_iter(0..additional, "FreeCAD legacy name slot scan")? {
            names.push(Vec::new());
        }
    }
    ctx.reserve_vec(&mut names[index], 1, "FreeCAD legacy mapped names")?;
    names[index].push(ElementMappedName {
        encoded: ctx.copy_retained_text(&mapped_name, "FreeCAD legacy encoded name")?,
        resolved: Some(mapped_name),
        string_ids,
        topology_ids: Vec::new(),
    });
    Ok(())
}

fn legacy_map_payload(
    ctx: &DecodeContext<'_>,
    groups: LegacyNameGroups<'_>,
    declared_count: usize,
    source_entry: Option<String>,
) -> Result<MapPayload, CodecError> {
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

fn split_indexed_name<'a>(
    ctx: &DecodeContext<'_>,
    value: &'a str,
) -> Result<(&'a str, usize), CodecError> {
    if value.is_empty() {
        return Err(CodecError::Malformed("legacy indexed name is empty".into()));
    }
    let suffix_start = ctx
        .rposition_by(
            value.as_bytes(),
            |character| Ok(!character.is_ascii_digit()),
            "FreeCAD legacy indexed-name suffix",
        )?
        .map_or(0, |position| position + 1);
    let (type_name, suffix) = value.split_at(suffix_start);
    if type_name.is_empty()
        || !ctx.all_by(
            type_name.as_bytes(),
            |character| Ok(character.is_ascii_alphabetic() || *character == b'_'),
            "FreeCAD legacy indexed-name family",
        )?
    {
        return Err(CodecError::Malformed("invalid legacy indexed name".into()));
    }
    let index = if suffix.is_empty() {
        0
    } else {
        ctx.parse_text::<usize>(suffix, "FreeCAD legacy indexed-name number")?
            .map_err(|_| CodecError::Malformed("invalid legacy indexed-name index".into()))?
    };
    if index > MAX_NAMES {
        return Err(CodecError::Malformed(
            "legacy indexed-name index exceeds limit".into(),
        ));
    }
    Ok((type_name, index))
}

fn string_hasher_successor<'a, 'input>(
    ctx: &DecodeContext<'_>,
    node: roxmltree::Node<'a, 'input>,
) -> Result<roxmltree::Node<'a, 'input>, CodecError> {
    let mut siblings = node.next_siblings();
    ctx.next_charged(&mut siblings, "FreeCAD string-hasher sibling scan")?;
    let Some(mut successor) =
        ctx.next_charged(&mut siblings, "FreeCAD string-hasher sibling scan")?
    else {
        return Err(CodecError::Malformed(
            "StringHasher new=1 is not followed by StringHasher2".into(),
        ));
    };
    while successor.is_text()
        && successor
            .text()
            .map(|text| {
                ctx.trim_text(text, "FreeCAD string-hasher sibling whitespace")
                    .map(str::is_empty)
            })
            .transpose()?
            .unwrap_or(false)
    {
        successor = ctx
            .next_charged(&mut siblings, "FreeCAD string-hasher sibling scan")?
            .ok_or_else(|| {
                CodecError::Malformed("StringHasher new=1 is not followed by StringHasher2".into())
            })?;
    }
    if successor.is_element()
        && ctx.xml_has_tag_name(
            successor,
            "StringHasher2",
            "FreeCAD string-hasher successor tag",
        )?
    {
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
    let mut reservation = ctx.reserve_scoped(0, "FreeCAD inline element text")?;
    let mut bytes = Vec::new();
    let mut children = node.children();
    while let Some(child) =
        ctx.next_charged(&mut children, "FreeCAD inline element text children")?
    {
        if !child.is_text() {
            continue;
        }
        if let Some(text) = child.text() {
            reservation.with_storage(|| {
                ctx.extend_from_slice(&mut bytes, text.as_bytes(), "FreeCAD inline element text")
            })?;
        }
    }
    Ok((bytes, reservation))
}

fn parse_count(
    ctx: &DecodeContext<'_>,
    node: roxmltree::Node<'_, '_>,
    kind: &str,
) -> Result<usize, CodecError> {
    let count = ctx
        .xml_attribute(node, "count", "FreeCAD element-map XML attribute")?
        .unwrap_or("0");
    let count = parse_usize(ctx, count, &format!("{kind} count"))?;
    if count > MAX_TABLE_ENTRIES {
        return Err(element_map_malformed(
            ctx,
            format_args!("{kind} count exceeds limit"),
        ));
    }
    Ok(count)
}

/// Reads an element-map boolean attribute, which is `0`, `1`, `false` or `true`.
///
/// Any other text is malformed.
fn require_bool(ctx: &DecodeContext<'_>, value: &str) -> Result<bool, CodecError> {
    match value {
        "0" | "false" => Ok(false),
        "1" | "true" => Ok(true),
        _ => Err(element_map_malformed(
            ctx,
            format_args!("invalid boolean {value:?}"),
        )),
    }
}

fn parse_decimal(ctx: &DecodeContext<'_>, value: &str, field: &str) -> Result<i64, CodecError> {
    ctx.parse_text(value, "FreeCAD element-map number")?
        .map_err(|_| element_map_malformed(ctx, format_args!("invalid {field} {value:?}")))
}

fn parse_usize(ctx: &DecodeContext<'_>, value: &str, field: &str) -> Result<usize, CodecError> {
    ctx.parse_text(value, "FreeCAD element-map number")?
        .map_err(|_| element_map_malformed(ctx, format_args!("invalid {field} {value:?}")))
}

fn parse_hex(ctx: &DecodeContext<'_>, value: &str, field: &str) -> Result<i64, CodecError> {
    let (negative, digits) = value
        .strip_prefix('-')
        .map_or((false, value), |digits| (true, digits));
    if digits.is_empty() {
        return Err(element_map_malformed(ctx, format_args!("empty {field}")));
    }
    let value = ctx
        .parse_radix::<i64>(digits, 16, "FreeCAD element-map hexadecimal number")?
        .map_err(|_| element_map_malformed(ctx, format_args!("invalid {field} {value:?}")))?;
    Ok(if negative { -value } else { value })
}

fn parse_string_table(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    declared_count: usize,
    side_entry: bool,
) -> Result<Vec<StringTableEntry>, CodecError> {
    let text = ctx
        .validate_utf8(bytes, "FreeCAD element-map UTF-8")?
        .map_err(|_| CodecError::Malformed("string table is not UTF-8".into()))?;
    let mut scanner = TextScanner::new(text);
    if side_entry {
        if scanner.token(ctx)? != "StringTableStart" || scanner.token(ctx)? != "v1" {
            return Err(CodecError::Malformed(
                "string-table side entry has invalid header".into(),
            ));
        }
        if parse_usize(ctx, scanner.token(ctx)?, "string-table header count")? != declared_count {
            return Err(CodecError::Malformed(
                "string-table XML and side-entry counts disagree".into(),
            ));
        }
    }
    // Each record consumes at least one non-whitespace byte, so the declared count
    // cannot exceed the table's byte length.
    let capacity = bounded_len(
        cadmpeg_core::decode::u64_from_index(declared_count),
        1,
        text.len(),
    )
    .ok_or_else(|| CodecError::Malformed("string-table record count exceeds input".into()))?;
    let mut output = ctx.collection_vec(capacity, "FreeCAD string table entries")?;
    let mut previous_id = 0_i64;
    let mut records_left = 0..declared_count;
    while !records_left.is_empty()
        && ctx
            .next_charged(&mut records_left, "FreeCAD element-map record step")?
            .is_some()
    {
        scanner.skip_whitespace(ctx)?;
        let record_start = scanner.position;
        let header = scanner.token(ctx)?;
        let mut fields = TextScanner::new(header);
        let encoded_id_field = fields.next_field(ctx)?.ok_or_else(|| {
            CodecError::Malformed("string-table record has incomplete numeric header".into())
        })?;
        let flags_field = fields.next_field(ctx)?.ok_or_else(|| {
            CodecError::Malformed("string-table record has incomplete numeric header".into())
        })?;
        let relative = encoded_id_field.starts_with('-');
        let encoded_id = parse_hex(ctx, encoded_id_field, "string id")?;
        let string_id = if relative {
            previous_id
                .checked_add(-encoded_id)
                .ok_or_else(|| CodecError::Malformed("relative string id overflows".into()))?
        } else {
            encoded_id
        };
        let flags = ctx
            .parse_radix::<u64>(flags_field, 16, "FreeCAD string-table flags")?
            .map_err(|_| CodecError::Malformed("invalid string-table flags".into()))?;
        let mut components = Vec::new();
        let mut position = 0;
        while let Some(field) = fields.next_field(ctx)? {
            let encoded = parse_hex(ctx, field, "string component")?;
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
            ctx.reserve_vec(&mut components, 1, "FreeCAD string table components")?;
            components.push(component);
            position += 1;
        }
        let payload = if flags & 0x8 == 0 {
            scanner.encoded_text(ctx)?
        } else {
            let derived_prefix = flags & (0x10 | 0x20 | 0x40) != 0;
            let encoded_postfix = flags & 0x4 != 0;
            let mut value_storage = ctx.reserve_scoped(0, "FreeCAD string table value words")?;
            let mut values = Vec::new();
            if !derived_prefix {
                value_storage.with_storage(|| {
                    ctx.reserve_vec(&mut values, 1, "FreeCAD string table value words")
                })?;
                values.push(scanner.token(ctx)?);
            }
            if !encoded_postfix {
                value_storage.with_storage(|| {
                    ctx.reserve_vec(&mut values, 1, "FreeCAD string table value words")
                })?;
                values.push(scanner.token(ctx)?);
            }
            ctx.join_retained(&values, " ", "FreeCAD string table joined value")?
        };
        let raw = ctx.copy_retained_text(
            ctx.trim_end_text(
                &text[record_start..scanner.position],
                "FreeCAD string-table raw trim",
            )?,
            "FreeCAD string table raw record",
        )?;
        output.push(StringTableEntry {
            string_id,
            flags,
            components,
            payload,
            raw,
        });
        previous_id = string_id;
    }
    scanner.skip_whitespace(ctx)?;
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
    ascii_whitespace: bool,
    fields_done: bool,
}

impl<'a> TextScanner<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            position: 0,
            ascii_whitespace: false,
            fields_done: false,
        }
    }
    fn new_ascii(text: &'a str) -> Self {
        Self {
            ascii_whitespace: true,
            ..Self::new(text)
        }
    }

    fn skip_whitespace(&mut self, ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
        while self.position < self.text.len() {
            let Some(character) = ctx.next_charged(
                &mut self.text[self.position..].chars(),
                "FreeCAD element-map whitespace scan",
            )?
            else {
                break;
            };
            if !(if self.ascii_whitespace {
                character.is_ascii_whitespace()
            } else {
                character.is_whitespace()
            }) {
                break;
            }
            self.position += character.len_utf8();
        }
        Ok(())
    }

    fn next(&mut self, ctx: &DecodeContext<'_>) -> Result<Option<&'a str>, CodecError> {
        self.skip_whitespace(ctx)?;
        let start = self.position;
        while self.position < self.text.len() {
            let Some(character) = ctx.next_charged(
                &mut self.text[self.position..].chars(),
                "FreeCAD element-map token scan",
            )?
            else {
                break;
            };
            if if self.ascii_whitespace {
                character.is_ascii_whitespace()
            } else {
                character.is_whitespace()
            } {
                break;
            }
            self.position += character.len_utf8();
        }
        Ok((start != self.position).then_some(&self.text[start..self.position]))
    }

    fn token(&mut self, ctx: &DecodeContext<'_>) -> Result<&'a str, CodecError> {
        self.next(ctx)?
            .ok_or_else(|| CodecError::malformed("string table ends before declared count"))
    }

    fn next_field(&mut self, ctx: &DecodeContext<'_>) -> Result<Option<&'a str>, CodecError> {
        if self.fields_done {
            return Ok(None);
        }
        let start = self.position;
        while self.position < self.text.len() {
            let Some(character) = ctx.next_charged(
                &mut self.text[self.position..].chars(),
                "FreeCAD element-map field scan",
            )?
            else {
                break;
            };
            self.position += character.len_utf8();
            if character == '.' {
                return Ok(Some(&self.text[start..self.position - 1]));
            }
        }
        self.fields_done = true;
        Ok(Some(&self.text[start..self.position]))
    }

    fn next_legacy_id(&mut self, ctx: &DecodeContext<'_>) -> Result<Option<&'a str>, CodecError> {
        while self.position < self.text.len() {
            let Some(character) = ctx.next_charged(
                &mut self.text[self.position..].chars(),
                "FreeCAD legacy string-id separator scan",
            )?
            else {
                break;
            };
            if character.is_ascii_digit() || character == '-' {
                break;
            }
            self.position += character.len_utf8();
        }
        let start = self.position;
        while self.position < self.text.len() {
            let Some(character) = ctx.next_charged(
                &mut self.text[self.position..].chars(),
                "FreeCAD legacy string-id token scan",
            )?
            else {
                break;
            };
            if !character.is_ascii_digit() && character != '-' {
                break;
            }
            self.position += character.len_utf8();
        }
        Ok((start != self.position).then_some(&self.text[start..self.position]))
    }

    fn encoded_text(&mut self, ctx: &DecodeContext<'_>) -> Result<String, CodecError> {
        self.skip_whitespace(ctx)?;
        let count_start = self.position;
        let digits = ctx
            .position_by(
                &self.text.as_bytes()[self.position..],
                |byte| Ok(!byte.is_ascii_digit()),
                "FreeCAD string-table line-count scan",
            )?
            .unwrap_or(self.text.len() - self.position);
        self.position += digits;
        if count_start == self.position || self.text.as_bytes().get(self.position) != Some(&b':') {
            return Err(CodecError::malformed(
                "string-table text has invalid line-count prefix",
            ));
        }
        let line_count = parse_usize(
            ctx,
            &self.text[count_start..self.position],
            "text line count",
        )?;
        self.position += 1;
        let content_start = self.position;
        let mut lines = 0..=line_count;
        while lines.size_hint().1 != Some(0)
            && ctx
                .next_charged(&mut lines, "FreeCAD string-table encoded line step")?
                .is_some()
        {
            let newline = ctx
                .find_map(
                    self.text[self.position..].char_indices(),
                    |(offset, character)| Ok((character == '\n').then_some(offset)),
                    "FreeCAD string-table line delimiter scan",
                )?
                .ok_or_else(|| {
                    CodecError::malformed("string-table text ends before its line delimiter")
                })?;
            self.position += newline + 1;
        }
        ctx.copy_retained_text(
            &self.text[content_start..self.position - 1],
            "FreeCAD string table encoded text",
        )
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
    let text = ctx
        .validate_utf8(bytes, "FreeCAD element-map UTF-8")?
        .map_err(|_| CodecError::Malformed("element map is not UTF-8".into()))?;
    let mut tokens = TextScanner::new(text);
    if side_entry {
        expect(ctx, &mut tokens, "BeginElementMap")?;
        expect(ctx, &mut tokens, "v1")?;
    }
    let map_id = next_u64(ctx, &mut tokens, "element-map id")?;
    expect(ctx, &mut tokens, "PostfixCount")?;
    let postfix_count = next_count(ctx, &mut tokens, "postfix count", MAX_NAMES)?;
    let postfix_capacity = bounded_len(
        cadmpeg_core::decode::u64_from_index(postfix_count),
        1,
        text.len() - tokens.position,
    )
    .ok_or_else(|| CodecError::malformed("element-map postfix count exceeds input"))?;
    let mut postfixes = ctx.collection_vec(postfix_capacity, "FreeCAD element map postfixes")?;
    let mut records_left = 0..postfix_count;
    while !records_left.is_empty()
        && ctx
            .next_charged(&mut records_left, "FreeCAD element-map record step")?
            .is_some()
    {
        postfixes.push(ctx.copy_retained_text(
            next_token(ctx, &mut tokens, "postfix")?,
            "FreeCAD element map postfix text",
        )?);
    }
    expect(ctx, &mut tokens, "MapCount")?;
    let map_count = next_count(ctx, &mut tokens, "map count", MAX_MAP_NODES)?;
    // Each map node consumes at least one whitespace-separated token, so its count
    // cannot exceed the element map's byte length.
    let map_capacity = bounded_len(
        cadmpeg_core::decode::u64_from_index(map_count),
        1,
        text.len(),
    )
    .ok_or_else(|| CodecError::Malformed("element-map node count exceeds input".into()))?;
    let mut maps = ctx.collection_vec(map_capacity, "FreeCAD element map nodes")?;
    let mut records_left = 1..=map_count;
    while !records_left.is_empty() {
        let Some(expected_index) =
            ctx.next_charged(&mut records_left, "FreeCAD element-map record step")?
        else {
            break;
        };
        expect(ctx, &mut tokens, "ElementMap")?;
        let index = next_count(ctx, &mut tokens, "map index", MAX_MAP_NODES)?;
        if index != expected_index {
            return Err(CodecError::Malformed(
                "element-map node indices are not contiguous".into(),
            ));
        }
        let node_id = next_u64(ctx, &mut tokens, "map node id")?;
        let group_count = next_count(ctx, &mut tokens, "group count", MAX_GROUPS)?;
        // Each group consumes at least one token, so its count cannot exceed the byte length.
        let group_capacity = bounded_len(
            cadmpeg_core::decode::u64_from_index(group_count),
            1,
            text.len(),
        )
        .ok_or_else(|| CodecError::Malformed("element-map group count exceeds input".into()))?;
        let mut groups = ctx.collection_vec(group_capacity, "FreeCAD element map groups")?;
        let mut records_left = 0..group_count;
        while !records_left.is_empty()
            && ctx
                .next_charged(&mut records_left, "FreeCAD element-map record step")?
                .is_some()
        {
            let indexed_name = ctx.copy_retained_text(
                next_token(ctx, &mut tokens, "indexed name")?,
                "FreeCAD indexed element name",
            )?;
            expect(ctx, &mut tokens, "ChildCount")?;
            let child_count = next_count(ctx, &mut tokens, "child count", MAX_NAMES)?;
            // Each child consumes at least one token, so its count cannot exceed the byte length.
            let child_capacity = bounded_len(
                cadmpeg_core::decode::u64_from_index(child_count),
                1,
                text.len(),
            )
            .ok_or_else(|| CodecError::Malformed("element-map child count exceeds input".into()))?;
            let mut children =
                ctx.collection_vec(child_capacity, "FreeCAD element map children")?;
            let mut records_left = 0..child_count;
            while !records_left.is_empty()
                && ctx
                    .next_charged(&mut records_left, "FreeCAD element-map record step")?
                    .is_some()
            {
                let mut fields = [""; 7];
                for field in &mut fields {
                    *field = next_token(ctx, &mut tokens, "child descriptor")?;
                }
                children.push(ctx.join_retained(
                    &fields,
                    " ",
                    "FreeCAD element child descriptor",
                )?);
            }
            expect(ctx, &mut tokens, "NameCount")?;
            let name_count = next_count(ctx, &mut tokens, "name count", MAX_NAMES)?;
            // Each name consumes at least one token, so its count cannot exceed the byte length.
            let name_capacity = bounded_len(
                cadmpeg_core::decode::u64_from_index(name_count),
                1,
                text.len(),
            )
            .ok_or_else(|| CodecError::Malformed("element-map name count exceeds input".into()))?;
            let mut names = ctx.collection_vec(name_capacity, "FreeCAD element map names")?;
            let mut records_left = 0..name_count;
            while !records_left.is_empty()
                && ctx
                    .next_charged(&mut records_left, "FreeCAD element-map record step")?
                    .is_some()
            {
                let mut chain = Vec::new();
                loop {
                    let encoded = next_token(ctx, &mut tokens, "mapped name")?;
                    if encoded == "0" {
                        break;
                    }
                    ctx.reserve_vec(&mut chain, 1, "FreeCAD mapped name chain")?;
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
        expect(ctx, &mut tokens, "EndMap")?;
        maps.push(ElementMapNode {
            map_id: node_id,
            groups,
        });
    }
    if tokens.next(ctx)?.is_some() {
        return Err(CodecError::Malformed(
            "element map has trailing non-whitespace data".into(),
        ));
    }
    Ok(ParsedMap {
        map_id,
        postfixes,
        maps: ElementMapNodes::from_nodes(maps, ctx)?.map_err(CodecError::Malformed)?,
    })
}

fn parse_mapped_name(
    ctx: &DecodeContext<'_>,
    encoded: &str,
    postfixes: &[String],
) -> Result<ElementMappedName, CodecError> {
    let mut scanner = TextScanner::new(encoded);
    let mut field_storage = ctx.reserve_scoped(0, "FreeCAD mapped name fields")?;
    let mut fields = Vec::new();
    while let Some(field) = scanner.next_field(ctx)? {
        field_storage
            .with_storage(|| ctx.push_vec(&mut fields, field, "FreeCAD mapped name fields"))?;
    }
    let (base, postfix_position, id_position) =
        if let Some(dictionary) = fields[0].strip_prefix(':') {
            if fields.len() < 3 {
                return Err(CodecError::Malformed(
                    "indexed mapped name has incomplete dictionary fields".into(),
                ));
            }
            let dictionary = parse_usize(ctx, dictionary, "mapped-name prefix index")?;
            let prefix = postfixes
                .get(dictionary.checked_sub(1).ok_or_else(|| {
                    CodecError::Malformed("mapped-name prefix index is zero".into())
                })?)
                .ok_or_else(|| {
                    CodecError::Malformed("mapped-name prefix index is out of range".into())
                })?;
            let element = usize::try_from(parse_hex(ctx, fields[1], "mapped-name element index")?)
                .map_err(|_| CodecError::Malformed("negative mapped-name element index".into()))?;
            (
                ctx.format_retained(
                    format_args!("{prefix}{element}"),
                    "FreeCAD mapped name base",
                )?,
                2,
                3,
            )
        } else if let Some(base) = fields[0]
            .strip_prefix(';')
            .or_else(|| fields[0].strip_prefix('$'))
        {
            (
                ctx.copy_retained_text(base, "FreeCAD mapped name base")?,
                1,
                2,
            )
        } else {
            return Err(CodecError::Malformed(
                "mapped name has unknown base encoding".into(),
            ));
        };
    let postfix_index = fields
        .get(postfix_position)
        .ok_or_else(|| CodecError::Malformed("mapped name has no postfix index".into()))
        .and_then(|value| {
            usize::try_from(parse_hex(ctx, value, "mapped-name postfix index")?)
                .map_err(|_| CodecError::Malformed("negative mapped-name postfix index".into()))
        })?;
    let mut resolved = base;
    if postfix_index != 0 {
        ctx.append_retained(
            &mut resolved,
            postfixes.get(postfix_index - 1).ok_or_else(|| {
                CodecError::Malformed("mapped-name postfix index is out of range".into())
            })?,
            "FreeCAD mapped name postfix",
        )?;
    }
    let mut string_ids = Vec::new();
    let mut values = fields.get(id_position..).unwrap_or_default().iter();
    while values.len() != 0 {
        let Some(value) = ctx.next_charged(&mut values, "FreeCAD mapped-name string-id scan")?
        else {
            break;
        };
        if !value.is_empty() {
            ctx.push_vec(
                &mut string_ids,
                parse_hex(ctx, value, "mapped-name string id")?,
                "FreeCAD mapped name string IDs",
            )?;
        }
    }
    Ok(ElementMappedName {
        encoded: ctx.copy_retained_text(encoded, "FreeCAD encoded mapped name")?,
        resolved: Some(resolved),
        string_ids,
        topology_ids: Vec::new(),
    })
}

fn next_token<'a>(
    ctx: &DecodeContext<'_>,
    tokens: &mut TextScanner<'a>,
    field: &str,
) -> Result<&'a str, CodecError> {
    tokens
        .next(ctx)?
        .ok_or_else(|| element_map_malformed(ctx, format_args!("element map ends before {field}")))
}

fn expect(
    ctx: &DecodeContext<'_>,
    tokens: &mut TextScanner<'_>,
    expected: &str,
) -> Result<(), CodecError> {
    let actual = next_token(ctx, tokens, expected)?;
    if actual != expected {
        return Err(element_map_malformed(
            ctx,
            format_args!("expected element-map token {expected:?}, found {actual:?}"),
        ));
    }
    Ok(())
}

fn next_count(
    ctx: &DecodeContext<'_>,
    tokens: &mut TextScanner<'_>,
    field: &str,
    limit: usize,
) -> Result<usize, CodecError> {
    let value = parse_usize(ctx, next_token(ctx, tokens, field)?, field)?;
    if value > limit {
        return Err(element_map_malformed(
            ctx,
            format_args!("{field} exceeds limit"),
        ));
    }
    Ok(value)
}

fn next_u64(
    ctx: &DecodeContext<'_>,
    tokens: &mut TextScanner<'_>,
    field: &str,
) -> Result<u64, CodecError> {
    ctx.parse_text(
        next_token(ctx, tokens, field)?,
        "FreeCAD element-map number",
    )?
    .map_err(|_| element_map_malformed(ctx, format_args!("invalid {field}")))
}

#[cfg(test)]
mod tests;
