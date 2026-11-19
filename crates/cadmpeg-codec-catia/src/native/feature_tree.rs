// SPDX-License-Identifier: Apache-2.0
//! Compact record classes and the feature-tree membership of the part root,
//! bodies, and geometrical sets.

use super::entity_record::CatiaEntityRecord;
use super::paired_records::PairedRecords;
use super::{CatiaCatalog, CatiaCatalogEntry, CatiaDesignClass, CatiaObjectGraph};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const RETAIN_TREE: &str = "retain CATIA feature-tree membership";

/// A compact tag byte minus this base is a zero-based catalog ordinal.
const COMPACT_TAG_BASE: u8 = 0x34;
/// Prefix-relative offset of the class tag in a compact definition prefix.
const COMPACT_CLASS_OFFSET: usize = 2;

const ROOT_CLASS: &str = "MechanicalPart";
const CONTAINER_CLASSES: [&str; 4] = [ROOT_CLASS, "MechanicalTool", "HybridBody", "GSMTool"];
const MEMBER_LIST_FIELD: &str = "_FtrList";
const CURRENT_FEATURE_FIELD: &str = "CurrentFeature";
const MAIN_TOOL_FIELD: &str = "MainTool";

/// Class named by the one-byte tag of a compact definition prefix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaCompactClass {
    /// Byte offset of the class tag within the definition prefix.
    pub(crate) offset: u64,
    /// Stored class tag.
    pub(crate) tag: u8,
    /// Zero-based catalog ordinal selected by the tag.
    pub(crate) ordinal: u32,
    /// Selected catalog entry.
    pub(crate) entry: String,
    /// UTF-8 class name stored by the entry.
    pub(crate) name: String,
}

/// One tagged segment of a compact `7C0A` payload, in payload order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaCompactSegment {
    /// Byte offset of the tag within the payload.
    pub(crate) payload_offset: u64,
    /// Stored field tag.
    pub(crate) tag: u8,
    /// Catalog entry selected by the tag.
    pub(crate) entry: String,
    /// UTF-8 field name stored by the entry.
    pub(crate) name: String,
}

/// Same-graph record selected by a stored entity identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaFeatureTarget {
    /// Paired `7C05` entity record.
    pub(crate) entity_record: String,
    /// Paired `7C09` object record.
    pub(crate) object_record: String,
    /// Compact class of the selected record.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_target_class"
    )]
    pub(crate) class: Option<CatiaDesignClass>,
}

/// One single-reference segment of a container payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaFeatureReference {
    /// Byte offset of the reference token within the payload.
    pub(crate) payload_offset: u64,
    /// Stored entity identity.
    pub(crate) entity_id: u32,
    /// Selected same-graph record.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_reference_target"
    )]
    pub(crate) target: Option<CatiaFeatureTarget>,
}

/// One `_FtrList` member, in stored order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaFeatureMember {
    /// Byte offset of the member's entity token within the payload.
    pub(crate) payload_offset: u64,
    /// Stored entity identity.
    pub(crate) entity_id: u32,
    /// Stored persistent-identity token value.
    pub(crate) persistent_id: u32,
    /// Selected same-graph record.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_member_target"
    )]
    pub(crate) target: Option<CatiaFeatureTarget>,
}

/// Part root, body, or geometrical set and its stored feature-tree fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaFeatureContainer {
    /// Globally unique container identity.
    pub(crate) id: String,
    /// Containing object graph.
    pub(crate) object_graph: String,
    /// Container `7C09` object record.
    pub(crate) object_record: String,
    /// Container `7C05` entity record.
    pub(crate) entity_record: String,
    /// Container entity identity.
    pub(crate) entity_id: u32,
    /// Compact class of the container record.
    pub(crate) class: CatiaDesignClass,
    /// Payload segments in payload order.
    pub(crate) segments: Vec<CatiaCompactSegment>,
    /// Payload offset of the first byte no segment production covers.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_unsegmented_payload_offset"
    )]
    pub(crate) unsegmented_payload_offset: Option<u64>,
    /// `_FtrList` members in stored order; absent when the list is unread.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_members"
    )]
    pub(crate) members: Option<Vec<CatiaFeatureMember>>,
    /// Stored in-work object of the part root.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_current_feature"
    )]
    pub(crate) current_feature: Option<CatiaFeatureReference>,
    /// Stored main body of the part root.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_main_tool"
    )]
    pub(crate) main_tool: Option<CatiaFeatureReference>,
}

/// Class of a compact definition prefix `00 <flags> <tag> <second>`.
///
/// `flags` admits only bits `04` and `08`. The tag selects zero-based catalog
/// ordinal `tag − 34`; the second tag byte is not assigned.
pub(crate) fn compact_class(
    ctx: &DecodeContext<'_>,
    definition_prefix: &[u8],
    catalog: Option<&CatiaCatalog>,
) -> Result<Option<CatiaCompactClass>, CodecError> {
    let [0x00, flags, tag, _] = *definition_prefix else {
        return Ok(None);
    };
    if flags & !0x0c != 0 {
        return Ok(None);
    }
    let Some(entry) = catalog.and_then(|catalog| tag_entry(catalog, tag)) else {
        return Ok(None);
    };
    retain_text(ctx, &entry.id)?;
    retain_text(ctx, &entry.value)?;
    Ok(Some(CatiaCompactClass {
        offset: u64_from_index(COMPACT_CLASS_OFFSET),
        tag,
        ordinal: entry.ordinal,
        entry: entry.id.clone(),
        name: entry.value.clone(),
    }))
}

fn tag_entry(catalog: &CatiaCatalog, tag: u8) -> Option<&CatiaCatalogEntry> {
    catalog
        .entries
        .get(usize::from(tag.checked_sub(COMPACT_TAG_BASE)?))
}

/// Integer token of a compact payload: `80 <u32le>` is the value itself, and
/// the compact `81..D0` and paged `D1..E4 <low>` atoms store the value plus one.
/// The escaped form must leave the payload's final byte.
fn int_token(payload: &[u8], at: usize) -> Option<(u32, usize)> {
    match *payload.get(at)? {
        0x80 => {
            let end = at.checked_add(5)?;
            (end < payload.len()).then_some(())?;
            Some((View::u32_le_at(payload, at + 1)?, end))
        }
        byte @ 0x81..=0xd0 => Some((u32::from(byte - 0x81), at + 1)),
        byte @ 0xd1..=0xe4 => {
            let low = *payload.get(at + 1)?;
            Some((u32::from(byte - 0xd1) * 256 + u32::from(low), at + 2))
        }
        _ => None,
    }
}

struct ListItem {
    offset: usize,
    entity_id: u32,
    persistent_id: u32,
}

enum SegmentValue {
    Single { offset: usize, value: u32 },
    List(Vec<ListItem>),
}

struct Segment<'c> {
    offset: usize,
    tag: u8,
    entry: &'c CatiaCatalogEntry,
    value: SegmentValue,
}

struct Segmentation<'c> {
    segments: Vec<Segment<'c>>,
    unsegmented: Option<usize>,
}

/// Walk `<tag> <value>` segments up to the final `FE` run. A value is a
/// counted list `<count> 81 (<entity> <persistent-id>){count}` or one integer
/// token. The walk stops at the first byte no segment covers.
fn segment(payload: &[u8], catalog: &CatiaCatalog) -> Segmentation<'_> {
    let is_terminal = |at: usize| payload.get(at..).is_some_and(|rest| rest.iter().all(|&b| b == 0xfe));
    let starts_segment =
        |at: usize| is_terminal(at) || payload.get(at).is_some_and(|&tag| tag_entry(catalog, tag).is_some());
    let mut segments = Vec::new();
    let mut at = 0;
    loop {
        if payload.get(at) == Some(&0xfe) && is_terminal(at) {
            return Segmentation {
                segments,
                unsegmented: None,
            };
        }
        let parsed = payload.get(at).and_then(|&tag| {
            let entry = tag_entry(catalog, tag)?;
            let value_at = at + 1;
            let (value, after) = int_token(payload, value_at)?;
            let list = (payload.get(after) == Some(&0x81))
                .then(|| counted_list(payload, after + 1, value))
                .flatten()
                .filter(|(_, end)| starts_segment(*end));
            let (value, end) = match list {
                Some((items, end)) => (SegmentValue::List(items), end),
                None => (
                    SegmentValue::Single {
                        offset: value_at,
                        value,
                    },
                    after,
                ),
            };
            Some((
                Segment {
                    offset: at,
                    tag,
                    entry,
                    value,
                },
                end,
            ))
        });
        let Some((segment, end)) = parsed else {
            return Segmentation {
                segments,
                unsegmented: Some(at),
            };
        };
        segments.push(segment);
        at = end;
    }
}

fn counted_list(payload: &[u8], start: usize, count: u32) -> Option<(Vec<ListItem>, usize)> {
    let count = usize::try_from(count).ok()?;
    if count > payload.len().saturating_sub(start) / 2 {
        return None;
    }
    let mut items = Vec::with_capacity(count);
    let mut at = start;
    for _ in 0..count {
        let (entity_id, next) = int_token(payload, at)?;
        let (persistent_id, next) = int_token(payload, next)?;
        items.push(ListItem {
            offset: at,
            entity_id,
            persistent_id,
        });
        at = next;
    }
    Some((items, at))
}

/// Derive every part-root, body, and geometrical-set container of the graphs.
pub(crate) fn derive(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    graphs: &[CatiaObjectGraph],
    entities: &[CatiaEntityRecord],
    catalogs: &[CatiaCatalog],
) -> Result<Vec<CatiaFeatureContainer>, CodecError> {
    let catalogs = catalogs
        .iter()
        .map(|catalog| (catalog.id.as_str(), catalog))
        .collect::<HashMap<_, _>>();
    let objects = graphs.iter().flat_map(|graph| &graph.records).collect::<Vec<_>>();
    let pairs = PairedRecords::new(entities, &objects);
    let mut containers = Vec::new();
    for graph in graphs {
        let Some(catalog) = graph
            .catalog
            .as_deref()
            .and_then(|id| catalogs.get(id).copied())
        else {
            continue;
        };
        for record in &graph.records {
            let Some(entity_id) = record.entity_id() else {
                continue;
            };
            let Some((entity, object)) = pairs.pair(&graph.id, entity_id) else {
                continue;
            };
            let Some(class) = entity
                .compact_class
                .as_ref()
                .filter(|class| CONTAINER_CLASSES.contains(&class.name.as_str()))
            else {
                continue;
            };
            let Some(payload) = object_payload(bytes, object) else {
                continue;
            };
            ctx.charge_entities(1, "admit CATIA feature-tree container")?;
            let segmentation = segment(payload, catalog);
            let target = |entity_id: u32| -> Result<Option<CatiaFeatureTarget>, CodecError> {
                let Some((entity, object)) = pairs.pair(&graph.id, entity_id) else {
                    return Ok(None);
                };
                retain::<CatiaFeatureTarget>(ctx)?;
                Ok(Some(CatiaFeatureTarget {
                    entity_record: retain_text(ctx, &entity.id)?,
                    object_record: retain_text(ctx, &object.id)?,
                    class: entity.compact_class.as_ref().map(design_class),
                }))
            };
            let named = |name: &str| {
                let mut matches = segmentation
                    .segments
                    .iter()
                    .filter(move |segment| segment.entry.value == name);
                let first = matches.next();
                first.filter(|_| matches.next().is_none())
            };
            let members = match named(MEMBER_LIST_FIELD) {
                Some(Segment {
                    value: SegmentValue::List(items),
                    ..
                }) => {
                    ctx.charge_entities(u64_from_index(items.len()), "admit CATIA feature-tree member")?;
                    let mut members = Vec::with_capacity(items.len());
                    for item in items {
                        members.push(CatiaFeatureMember {
                            payload_offset: u64_from_index(item.offset),
                            entity_id: item.entity_id,
                            persistent_id: item.persistent_id,
                            target: target(item.entity_id)?,
                        });
                    }
                    Some(members)
                }
                Some(_) => None,
                None => segmentation
                    .unsegmented
                    .is_none()
                    .then(Vec::new)
                    .filter(|_| !has_entry(&segmentation, MEMBER_LIST_FIELD)),
            };
            let is_root = class.name == ROOT_CLASS;
            let reference = |name: &str| -> Result<Option<CatiaFeatureReference>, CodecError> {
                let Some(Segment {
                    value: SegmentValue::Single { offset, value },
                    ..
                }) = named(name).filter(|_| is_root)
                else {
                    return Ok(None);
                };
                Ok(Some(CatiaFeatureReference {
                    payload_offset: u64_from_index(*offset),
                    entity_id: *value,
                    target: target(*value)?,
                }))
            };
            let current_feature = reference(CURRENT_FEATURE_FIELD)?;
            let main_tool = reference(MAIN_TOOL_FIELD)?;
            let mut segments = Vec::with_capacity(segmentation.segments.len());
            for segment in &segmentation.segments {
                segments.push(CatiaCompactSegment {
                    payload_offset: u64_from_index(segment.offset),
                    tag: segment.tag,
                    entry: retain_text(ctx, &segment.entry.id)?,
                    name: retain_text(ctx, &segment.entry.value)?,
                });
            }
            containers.push(CatiaFeatureContainer {
                id: format!(
                    "catia:outer:feature-container#{:010}-{entity_id:010}",
                    graph.byte_offset
                ),
                object_graph: graph.id.clone(),
                object_record: object.id.clone(),
                entity_record: entity.id.clone(),
                entity_id,
                class: design_class(class),
                segments,
                unsegmented_payload_offset: segmentation.unsegmented.map(u64_from_index),
                members,
                current_feature,
                main_tool,
            });
        }
    }
    Ok(containers)
}

fn has_entry(segmentation: &Segmentation<'_>, name: &str) -> bool {
    segmentation
        .segments
        .iter()
        .any(|segment| segment.entry.value == name)
}

fn design_class(class: &CatiaCompactClass) -> CatiaDesignClass {
    CatiaDesignClass {
        entry: class.entry.clone(),
        name: class.name.clone(),
    }
}

/// The `7C0A` payload bytes of a nested object record: the record's last
/// `payload.size` bytes.
fn object_payload<'b>(bytes: &'b [u8], object: &super::CatiaObjectRecord) -> Option<&'b [u8]> {
    if object.inline_body.is_some() || object.payload.size == 0 {
        return None;
    }
    let start = usize::try_from(object.byte_offset).ok()?;
    let end = start.checked_add(usize::try_from(object.byte_len).ok()?)?;
    bytes.get(end.checked_sub(object.payload.size)?..end)
}

fn retain<T>(ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
    ctx.retain_bytes(u64_from_index(std::mem::size_of::<T>()), RETAIN_TREE)
}

fn retain_text(ctx: &DecodeContext<'_>, value: &str) -> Result<String, CodecError> {
    ctx.retain_bytes(u64_from_index(value.len()), RETAIN_TREE)?;
    Ok(value.to_owned())
}

#[cfg(test)]
mod tests;

cadmpeg_core::named_optional_field!(deserialize_target_class, CatiaDesignClass, "class");
cadmpeg_core::named_optional_field!(
    deserialize_reference_target,
    CatiaFeatureTarget,
    "target"
);
cadmpeg_core::named_optional_field!(deserialize_member_target, CatiaFeatureTarget, "target");
cadmpeg_core::named_optional_field!(
    deserialize_unsegmented_payload_offset,
    u64,
    "unsegmented_payload_offset"
);
cadmpeg_core::named_optional_field!(
    deserialize_members,
    Vec<CatiaFeatureMember>,
    "members"
);
cadmpeg_core::named_optional_field!(
    deserialize_current_feature,
    CatiaFeatureReference,
    "current_feature"
);
cadmpeg_core::named_optional_field!(deserialize_main_tool, CatiaFeatureReference, "main_tool");
