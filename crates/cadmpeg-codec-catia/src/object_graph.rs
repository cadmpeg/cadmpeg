// SPDX-License-Identifier: Apache-2.0
//! Outer `7C08` feature and object-ownership graph decoder.

use cadmpeg_ir::le::u32_at as u32_le;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{catalog, value_block};

/// One decoded outer object graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectGraph {
    /// Offset of the selected `7C08` root.
    pub pos: usize,
    /// Root total length, including its six-byte header.
    pub total_len: usize,
    /// Byte offset of the immediately associated `7C02` schema catalog.
    pub catalog_pos: Option<usize>,
    /// Consecutive nested `7C09` records.
    pub records: Vec<ObjectRecord>,
}

impl ObjectGraph {
    /// Resolve a one-based serialized object ordinal.
    #[must_use]
    #[cfg(test)]
    pub fn record(&self, ordinal: u32) -> Option<&ObjectRecord> {
        let index = usize::try_from(ordinal.checked_sub(1)?).ok()?;
        self.records.get(index)
    }

    /// Return records directly owned by `owner_ordinal`, in serialization order.
    #[cfg(test)]
    pub fn children(&self, owner_ordinal: u32) -> impl Iterator<Item = &ObjectRecord> {
        self.records
            .iter()
            .filter(move |record| record.owner_ref == Some(owner_ordinal))
    }
}

/// One `7C09` ownership record and its nested `7C0A` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectRecord {
    /// Zero-based serialized record index.
    pub index: usize,
    /// Record byte offset.
    pub pos: usize,
    /// Record total length, including its six-byte header.
    pub total_len: usize,
    /// First head byte.
    pub lead: u8,
    /// Decoded head tokens.
    pub head: Vec<HeadToken>,
    /// First head reference, identifying the owner by one-based record ordinal.
    pub owner_ref: Option<u32>,
    /// Second head reference, identifying the per-file class.
    pub class_ref: Option<u32>,
    /// UTF-8 class name at `class_ref` in the associated schema catalog.
    pub class_name: Option<String>,
    /// Third head reference, selecting the class-specific storage form.
    pub storage_ref: Option<u32>,
    /// Decoded nested payload.
    pub payload: ObjectPayload,
    /// Structural payload classification.
    pub subtype: PayloadSubtype,
}

/// Token in a `7C09` record head.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum HeadToken {
    /// Initial head lead.
    Lead(u8),
    /// `0x01` field separator.
    Separator,
    /// Compact or continued reference.
    Reference(u32),
    /// Literal byte outside an assigned reference or sentinel form.
    Literal(u8),
    /// Four-byte absent-handle sentinel.
    NullHandle,
}

/// Decoded `7C0A` tagged-atom payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectPayload {
    /// Payload size in bytes.
    pub size: usize,
    /// Decoded fields in serialization order.
    pub fields: Vec<PayloadField>,
}

/// Item within a count-prefixed `0x3b` list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ListItem {
    /// Referenced object ordinal.
    Reference {
        /// Referenced ordinal.
        value: u32,
        /// Byte offset of the item within the payload.
        offset: usize,
    },
    /// Untagged atom value.
    Atom {
        /// Decoded atom value.
        value: u32,
        /// Byte offset of the item within the payload.
        offset: usize,
    },
}

/// One schema-free field in a `7C0A` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum PayloadField {
    /// Untagged atom.
    Atom {
        /// Decoded atom value.
        value: u32,
        /// Byte offset within the payload.
        offset: usize,
    },
    /// Compact `0x81` or fixed-width `0x32` reference field.
    Reference {
        /// Referenced ordinal.
        value: u32,
        /// Byte offset within the payload.
        offset: usize,
    },
    /// Scalar field tagged `0x3a`, `0x32`, `0x39`, or `0x7a`.
    Scalar {
        /// Scalar field tag.
        tag: u8,
        /// Decoded scalar value.
        value: u32,
        /// Byte offset within the payload.
        offset: usize,
    },
    /// Length-framed `0xe5` binary descriptor.
    Blob {
        /// Length declared by the frame.
        declared_len: usize,
        /// Available blob bytes.
        #[serde(with = "cadmpeg_ir::bytes")]
        #[schemars(with = "String")]
        bytes: Vec<u8>,
        /// Byte offset within the payload.
        offset: usize,
    },
    /// Sane `0x3c` bulk-table header.
    BulkTable {
        /// Count atom preceding the table count.
        count: u32,
        /// Little-endian table row count.
        table_count: u32,
        /// Byte offset within the payload.
        offset: usize,
    },
    /// Count-prefixed `0x3b` list.
    List {
        /// Count declared by the list header.
        declared_count: u32,
        /// Available decoded list items.
        items: Vec<ListItem>,
        /// Byte offset within the payload.
        offset: usize,
    },
    /// `0x0d` sentinel.
    Sentinel {
        /// Byte offset within the payload.
        offset: usize,
    },
    /// `0xfe` payload terminator.
    Terminator,
}

/// Structural role of a decoded payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum PayloadSubtype {
    /// Contains a sane bulk-table header.
    BulkTable,
    /// Contains at least two scalar/atom/atom triplets.
    TripletChain,
    /// Contains a list with at least three declared items.
    ListAggregator,
    /// Contains a binary descriptor blob.
    Blob,
    /// Contains at least two atoms without triplets or lists.
    AtomVector,
    /// Empty or terminator-only payload.
    Empty,
    /// Payload combines other field shapes.
    Mixed,
}

/// Classification of the four-byte word preceding a surface-alias marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum AliasLead {
    /// Low byte `0x01`: ordinary surface-support storage.
    SurfaceSupportStorage,
    /// Exact value `0x8e`: E5-linked surface storage.
    E5LinkedSurfaceStorage,
    /// Exact value `0x8f`: ordinal-linked alias storage.
    OrdinalLinkedStorage8f,
    /// Zero lead: alias-like row outside surface storage.
    NonSurfaceAlias,
    /// Other lead value whose role is not assigned.
    Unclassified(u32),
}

/// Group-allocation header attached to an outer surface-alias row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AliasGroupMembership {
    /// `ObjectModeler` node prototype.
    pub prototype: u32,
    /// Identity shared by the nodes in one alias group.
    pub group_id: u32,
    /// Four-byte allocation slot beginning in F1's third byte.
    pub target_slot: u32,
    /// Complete bounded storage prefix between the group header and alias marker.
    #[serde(with = "cadmpeg_ir::bytes")]
    #[schemars(with = "String")]
    pub storage_prefix: Vec<u8>,
}

/// Fixed 20-byte core of an outer `01 00 04 00` surface-alias row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceAlias {
    /// Marker byte offset.
    pub pos: usize,
    /// Classified preceding word.
    pub lead: AliasLead,
    /// Complete preceding word.
    pub lead_raw: u32,
    /// Low 24 bits of the stored carrier tag.
    pub tag: u32,
    /// Complete stored tag word.
    pub tag_raw: u32,
    /// Single-byte row flag.
    pub flag: u8,
    /// Three-byte F1 field.
    pub f1: [u8; 3],
    /// `7C08` entity-table record ordinal in F1's third byte.
    pub entity_record_ordinal: u8,
    /// First trailing fixed-width field.
    pub f2: u32,
    /// Second trailing fixed-width field.
    pub f3: u32,
    /// Group-allocation header immediately preceding this alias core.
    pub group: Option<AliasGroupMembership>,
}

/// Literal unresolved `7C D9` marker occurrence and bounded source context.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg(test)]
pub struct Marker7cd9 {
    /// Marker byte offset.
    pub pos: usize,
    /// Bytes from the marker through the requested context bound or input end.
    pub context: Vec<u8>,
    /// Distance to the next literal marker occurrence.
    pub next_delta: Option<usize>,
}

/// Expose literal `7C D9` occurrences without assigning record framing or semantics.
#[must_use]
#[cfg(test)]
pub fn markers_7cd9(data: &[u8], context_len: usize) -> Vec<Marker7cd9> {
    let positions: Vec<usize> = data
        .windows(2)
        .enumerate()
        .filter_map(|(pos, bytes)| (bytes == [0x7c, 0xd9]).then_some(pos))
        .collect();
    positions
        .iter()
        .enumerate()
        .map(|(index, &pos)| Marker7cd9 {
            pos,
            context: data[pos..pos.saturating_add(context_len).min(data.len())].to_vec(),
            next_delta: positions.get(index + 1).map(|next| next - pos),
        })
        .collect()
}

/// Decode fixed surface-alias row cores from an outer body.
#[must_use]
pub fn surface_aliases(data: &[u8]) -> Vec<SurfaceAlias> {
    const MARKER: [u8; 4] = [0x01, 0x00, 0x04, 0x00];
    data.windows(MARKER.len())
        .enumerate()
        .filter(|(_, bytes)| *bytes == MARKER)
        .filter_map(|(pos, _)| {
            let tag_raw = u32_le(data, pos + 4)?;
            let tag = tag_raw & 0x00ff_ffff;
            if pos + 20 > data.len() {
                return None;
            }
            let lead_raw = u32_le(data, pos.checked_sub(4)?)?;
            let lead = if lead_raw & 0xff == 1 {
                AliasLead::SurfaceSupportStorage
            } else if lead_raw == 0x8e {
                AliasLead::E5LinkedSurfaceStorage
            } else if lead_raw == 0x8f {
                AliasLead::OrdinalLinkedStorage8f
            } else if lead_raw == 0 {
                AliasLead::NonSurfaceAlias
            } else {
                AliasLead::Unclassified(lead_raw)
            };
            let f1 = [data[pos + 9], data[pos + 10], data[pos + 11]];
            let group = alias_group_membership(data, pos);
            Some(SurfaceAlias {
                pos,
                lead,
                lead_raw,
                tag,
                tag_raw,
                flag: data[pos + 8],
                f1,
                entity_record_ordinal: f1[2],
                f2: u32_le(data, pos + 12)?,
                f3: u32_le(data, pos + 16)?,
                group,
            })
        })
        .collect()
}

fn alias_group_membership(data: &[u8], marker: usize) -> Option<AliasGroupMembership> {
    let candidates = [3usize, 4, 7, 8]
        .into_iter()
        .filter_map(|storage_len| {
            let start = marker.checked_sub(20 + storage_len)?;
            let storage = data.get(start + 20..marker)?;
            (data.get(start..start + 2) == Some(&[0x02, 0x00])
                && data.get(start + 10..start + 13) == Some(&[0x00, 0x05, 0x00])
                && data.get(start + 13..start + 17) == Some(&[0x01, 0x00, 0x00, 0x00])
                && data.get(start + 17..start + 20) == Some(&[0x30, 0x00, 0x00])
                && is_alias_group_storage_prefix(storage))
            .then_some((start, storage))
        })
        .collect::<Vec<_>>();
    let [(start, storage)] = candidates.as_slice() else {
        return None;
    };
    Some(AliasGroupMembership {
        prototype: u32_le(data, start + 2)?,
        group_id: u32_le(data, start + 6)?,
        target_slot: u32_le(data, marker + 11)?,
        storage_prefix: storage.to_vec(),
    })
}

pub(crate) fn is_alias_group_storage_prefix(storage: &[u8]) -> bool {
    matches!(
        storage,
        [0..=1, 0x00, 0x00]
            | [0..=1, 0..=1, 0x00, 0x00]
            | [0..=1, 0x01, 0x00, _, _, _, _]
            | [0..=1, 0..=1, 0x01, 0x00, _, _, _, _]
    )
}

/// Parse the valid `7C08` candidate containing the most `7C09` records.
#[must_use]
#[cfg(test)]
pub fn parse(data: &[u8]) -> Option<ObjectGraph> {
    parse_all(data)
        .into_iter()
        .max_by_key(|graph| graph.records.len())
}

/// Parse every length-closed `7C08` object graph in source order.
#[must_use]
pub fn parse_all(data: &[u8]) -> Vec<ObjectGraph> {
    let catalogs = catalog::parse(data);
    let value_blocks = value_block::parse(data);
    let candidates = data
        .windows(2)
        .enumerate()
        .filter(|(_, marker)| *marker == [0x7c, 0x08])
        .filter_map(|(pos, _)| parse_candidate(data, pos))
        .collect::<Vec<_>>();
    let mut roots = Vec::<ObjectGraph>::new();
    for graph in candidates {
        let graph_end = graph.pos + graph.total_len;
        if roots
            .iter()
            .any(|outer| outer.pos < graph.pos && outer.pos + outer.total_len >= graph_end)
        {
            continue;
        }
        roots.push(graph);
    }
    roots
        .into_iter()
        .map(|mut graph| {
            bind_catalog(&mut graph, &catalogs, &value_blocks);
            graph
        })
        .collect()
}

fn bind_catalog(
    graph: &mut ObjectGraph,
    catalogs: &[catalog::Catalog],
    value_blocks: &[value_block::ValueBlock],
) {
    let Some(graph_end) = graph.pos.checked_add(graph.total_len) else {
        return;
    };
    let schema = catalogs
        .iter()
        .find(|schema| schema.pos == graph_end)
        .or_else(|| {
            value_blocks
                .iter()
                .find(|block| block.pos == graph_end)
                .and_then(|block| block.pos.checked_add(block.total_len))
                .and_then(|value_end| catalogs.iter().find(|schema| schema.pos == value_end))
        });
    let Some(schema) = schema else {
        return;
    };
    graph.catalog_pos = Some(schema.pos);
    for record in &mut graph.records {
        record.class_name = record
            .class_ref
            .and_then(|ordinal| schema.entries.get(ordinal as usize))
            .map(|entry| entry.value.clone());
    }
}

fn parse_candidate(data: &[u8], pos: usize) -> Option<ObjectGraph> {
    let total_len = usize::try_from(u32_le(data, pos + 2)?).ok()?;
    let end = pos.checked_add(total_len)?;
    if total_len < 15 || end > data.len() {
        return None;
    }
    let mut at = pos + 6;
    let mut records = Vec::new();
    while at + 6 <= end && data.get(at..at + 2) == Some(&[0x7c, 0x09]) {
        let record_len = usize::try_from(u32_le(data, at + 2)?).ok()?;
        let record_end = at.checked_add(record_len)?;
        if record_len < 6 || record_end > end {
            return None;
        }
        let head_start = at + 6;
        let mut children = data[head_start..record_end]
            .windows(2)
            .enumerate()
            .filter_map(|(relative, marker)| {
                if marker != [0x7c, 0x0a] {
                    return None;
                }
                let child = head_start + relative;
                let child_len = usize::try_from(u32_le(data, child + 2)?).ok()?;
                (child_len >= 6 && child.checked_add(child_len) == Some(record_end))
                    .then_some((child, child_len))
            });
        let (child, _) = children.next()?;
        if children.next().is_some() {
            return None;
        }
        let lead = *data.get(head_start..child)?.first()?;
        let head = decode_head(&data[head_start..child]);
        let roles = if matches!(head.get(1), Some(HeadToken::Separator)) {
            &head[2..]
        } else {
            let native_role_count = match lead {
                0x02 => 1,
                0x12 => 2,
                0x52 => 3,
                _ => 0,
            };
            if head.len() == native_role_count + 1 {
                &head[1..]
            } else {
                &[]
            }
        };
        let owner_ref = match roles.first() {
            Some(HeadToken::Reference(value)) => Some(*value),
            _ => None,
        };
        let class_ref = owner_ref.and_then(|_| match roles.get(1) {
            Some(HeadToken::Reference(value)) => Some(*value),
            _ => None,
        });
        let storage_ref = class_ref.and_then(|_| match roles.get(2) {
            Some(HeadToken::Reference(value)) => Some(*value),
            _ => None,
        });
        let payload = decode_payload(&data[child + 6..record_end])?;
        let subtype = classify(&payload.fields);
        records.push(ObjectRecord {
            index: records.len(),
            pos: at,
            total_len: record_len,
            lead,
            head,
            owner_ref,
            class_ref,
            class_name: None,
            storage_ref,
            payload,
            subtype,
        });
        at = record_end;
    }
    (!records.is_empty() && at == end).then_some(ObjectGraph {
        pos,
        total_len,
        catalog_pos: None,
        records,
    })
}

fn decode_head(bytes: &[u8]) -> Vec<HeadToken> {
    let Some(&lead) = bytes.first() else {
        return Vec::new();
    };
    let mut tokens = vec![HeadToken::Lead(lead)];
    let mut at = 1;
    while at < bytes.len() {
        let byte = bytes[at];
        if byte == 0x01 {
            tokens.push(HeadToken::Separator);
            at += 1;
        } else if bytes.get(at..at + 4) == Some(&[0xff; 4]) {
            tokens.push(HeadToken::NullHandle);
            at += 4;
        } else if (0xd1..=0xe4).contains(&byte) && at + 1 < bytes.len() {
            tokens.push(HeadToken::Reference(
                u32::from(byte - 0xd1) * 256 + u32::from(bytes[at + 1]) + 1,
            ));
            at += 2;
        } else if (0x80..=0xd0).contains(&byte) {
            tokens.push(HeadToken::Reference(u32::from(byte - 0x80)));
            at += 1;
        } else {
            tokens.push(HeadToken::Literal(byte));
            at += 1;
        }
    }
    tokens
}

fn atom(bytes: &[u8], at: usize) -> Option<(u32, usize)> {
    let byte = *bytes.get(at)?;
    match byte {
        0x80..=0xd0 => Some((u32::from(byte - 0x80), 1)),
        0x51..=0x7f => Some((u32::from(byte), 1)),
        0xd1..=0xe4 => Some((
            u32::from(byte - 0xd1) * 256 + u32::from(*bytes.get(at + 1)?) + 1,
            2,
        )),
        _ => Some((u32::from(byte), 1)),
    }
}

fn decode_payload(bytes: &[u8]) -> Option<ObjectPayload> {
    let mut fields = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let offset = at;
        match bytes[at] {
            0xfe => {
                fields.push(PayloadField::Terminator);
                at += 1;
                break;
            }
            0xe5 if at + 5 <= bytes.len() => {
                let declared_len = usize::try_from(u32_le(bytes, at + 1).unwrap_or(0)).unwrap_or(0);
                let start = at + 5;
                let end = start.saturating_add(declared_len).min(bytes.len());
                fields.push(PayloadField::Blob {
                    declared_len,
                    bytes: bytes[start..end].to_vec(),
                    offset,
                });
                at = end;
            }
            0x3c => {
                let Some((count, advance)) = atom(bytes, at + 1) else {
                    fields.push(PayloadField::Atom {
                        value: 0x3c,
                        offset,
                    });
                    at += 1;
                    continue;
                };
                let table_at = at + 1 + advance;
                let table_count = u32_le(bytes, table_at).unwrap_or(u32::MAX);
                if usize::try_from(table_count)
                    .ok()
                    .is_some_and(|count| count <= bytes.len())
                {
                    fields.push(PayloadField::BulkTable {
                        count,
                        table_count,
                        offset,
                    });
                    at = table_at + 4;
                } else {
                    fields.push(PayloadField::Atom {
                        value: 0x3c,
                        offset,
                    });
                    at += 1;
                }
            }
            0x3b => {
                if bytes.get(at + 1) == Some(&0xfe) {
                    fields.push(PayloadField::Atom {
                        value: 0x3b,
                        offset,
                    });
                    at += 1;
                    continue;
                }
                let Some((declared_count, advance)) = atom(bytes, at + 1) else {
                    fields.push(PayloadField::Atom {
                        value: 0x3b,
                        offset,
                    });
                    at += 1;
                    continue;
                };
                at += 1 + advance;
                let mut items = Vec::new();
                for _ in 0..declared_count {
                    if at >= bytes.len() || bytes[at] == 0xfe {
                        break;
                    }
                    let item_offset = at;
                    let tagged_reference = bytes[at] == 0x81;
                    let tagged_atom = bytes[at] == 0x80;
                    let value_at = at + usize::from(tagged_reference || tagged_atom);
                    if (tagged_reference || tagged_atom)
                        && (value_at >= bytes.len() || bytes[value_at] == 0xfe)
                    {
                        at = value_at;
                        break;
                    }
                    let Some((value, consumed)) = atom(bytes, value_at) else {
                        break;
                    };
                    items.push(if tagged_reference {
                        ListItem::Reference {
                            value,
                            offset: item_offset,
                        }
                    } else {
                        ListItem::Atom {
                            value,
                            offset: item_offset,
                        }
                    });
                    at = value_at + consumed;
                }
                fields.push(PayloadField::List {
                    declared_count,
                    items,
                    offset,
                });
            }
            0x80 | 0x32 if at + 5 <= bytes.len() => {
                let tag = bytes[at];
                fields.push(if tag == 0x80 {
                    PayloadField::Atom {
                        value: u32_le(bytes, at + 1).expect("checked escaped atom extent"),
                        offset,
                    }
                } else {
                    PayloadField::Reference {
                        value: u32_le(bytes, at + 1).expect("checked scalar extent"),
                        offset,
                    }
                });
                at += 5;
            }
            0x81 | 0x3a | 0x39 | 0x7a => {
                let tag = bytes[at];
                if bytes.get(at + 1) == Some(&0xfe) {
                    fields.push(PayloadField::Atom {
                        value: u32::from(tag),
                        offset,
                    });
                    at += 1;
                    continue;
                }
                let Some((value, consumed)) = atom(bytes, at + 1) else {
                    fields.push(PayloadField::Atom {
                        value: u32::from(tag),
                        offset,
                    });
                    at += 1;
                    continue;
                };
                fields.push(match tag {
                    0x81 => PayloadField::Reference { value, offset },
                    _ => PayloadField::Scalar { tag, value, offset },
                });
                at += 1 + consumed;
            }
            0x0d => {
                fields.push(PayloadField::Sentinel { offset });
                at += 1;
            }
            _ => {
                let (value, consumed) = atom(bytes, at).unwrap_or((u32::from(bytes[at]), 1));
                fields.push(PayloadField::Atom { value, offset });
                at += consumed;
            }
        }
    }
    (at == bytes.len() && matches!(fields.last(), Some(PayloadField::Terminator))).then_some(
        ObjectPayload {
            size: bytes.len(),
            fields,
        },
    )
}

fn classify(fields: &[PayloadField]) -> PayloadSubtype {
    if fields
        .iter()
        .any(|field| matches!(field, PayloadField::BulkTable { .. }))
    {
        return PayloadSubtype::BulkTable;
    }
    let triplets = fields
        .windows(3)
        .filter(|window| {
            matches!(window[0], PayloadField::Scalar { .. })
                && matches!(window[1], PayloadField::Atom { .. })
                && matches!(window[2], PayloadField::Atom { .. })
        })
        .count();
    if triplets >= 2 {
        return PayloadSubtype::TripletChain;
    }
    if fields.iter().any(
        |field| matches!(field, PayloadField::List { declared_count, .. } if *declared_count >= 3),
    ) {
        return PayloadSubtype::ListAggregator;
    }
    if fields
        .iter()
        .any(|field| matches!(field, PayloadField::Blob { .. }))
    {
        return PayloadSubtype::Blob;
    }
    let atom_count = fields
        .iter()
        .filter(|field| matches!(field, PayloadField::Atom { .. }))
        .count();
    let list_count = fields
        .iter()
        .filter(|field| matches!(field, PayloadField::List { .. }))
        .count();
    if atom_count >= 2 && triplets == 0 && list_count == 0 {
        return PayloadSubtype::AtomVector;
    }
    if fields.is_empty() || matches!(fields, [PayloadField::Terminator]) {
        PayloadSubtype::Empty
    } else {
        PayloadSubtype::Mixed
    }
}
