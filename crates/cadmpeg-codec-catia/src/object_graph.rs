// SPDX-License-Identifier: Apache-2.0
//! Outer `7C08` feature and object-ownership graph decoder.

use cadmpeg_core::decode::View;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::layout::outer_alias_row as alias_row;
use crate::{catalog, value_block};

/// One decoded outer object graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ObjectGraph {
    /// Offset of the selected `7C08` root.
    pub pos: usize,
    /// Root total length, including its six-byte header.
    pub total_len: usize,
    /// Byte offset of the immediately associated `7C02` schema catalog.
    pub catalog_pos: Option<usize>,
    /// Consecutive `7C09` records.
    pub records: Vec<ObjectRecord>,
}

/// One `7C09` object record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
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
    /// Complete alternate inline body when the record has no nested `7C0A`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(with = "Option<String>"))]
    pub inline_body: Option<Vec<u8>>,
    /// First head reference, identifying the owner by stored entity identity.
    pub owner_ref: Option<u32>,
    /// Literal value occupying a structurally assigned owner slot.
    pub owner_literal: Option<u8>,
    /// Second head reference, identifying the per-file class.
    pub class_ref: Option<u32>,
    /// UTF-8 class name at `class_ref` in the associated schema catalog.
    pub class_name: Option<String>,
    /// Third head reference, selecting the class-specific storage form.
    pub storage_ref: Option<u32>,
    /// Decoded nested payload, empty for an inline record.
    pub payload: ObjectPayload,
    /// Counted reference suffix when the payload repeats its reference prefix exactly.
    pub repeated_reference_suffix: Option<RepeatedReferenceSuffix>,
    /// Structural payload classification.
    pub subtype: PayloadSubtype,
}

/// Token in a `7C09` record head.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ObjectPayload {
    /// Payload size in bytes.
    pub size: usize,
    /// Decoded fields in serialization order.
    pub fields: Vec<PayloadField>,
}

/// One counted reference suffix whose reference prefix is serialized twice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct RepeatedReferenceSuffix {
    /// Schema-selection production in the payload prefix before this suffix.
    pub schema_preamble: Option<ReferenceSchemaPreamble>,
    /// Ordered entity identities serialized in both vectors.
    pub repeated_references: Vec<u32>,
    /// Final reference in the first counted vector.
    pub terminal_reference: u32,
    /// Byte offset of the first count atom within the payload.
    pub first_count_offset: usize,
    /// Byte offset of the repeated count atom within the payload.
    pub repeated_count_offset: usize,
}

/// Schema reference carried by a repeated-reference payload preamble.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum ReferenceSchemaPreamble {
    /// `<59-byte blob> <5:atom> <46:atom> <schema-ref:atom>`.
    BlobThenSchema {
        /// Per-file schema-catalog ordinal.
        schema_ref: u32,
        /// Byte offset of `schema_ref` within the payload.
        offset: usize,
    },
    /// `<schema-ref:atom> <34:atom> <59-byte blob> <5:atom>`.
    SchemaThenBlob {
        /// Per-file schema-catalog ordinal.
        schema_ref: u32,
        /// Byte offset of `schema_ref` within the payload.
        offset: usize,
    },
}

/// Item within a count-prefixed `0x3b` list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
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

/// One allocation row in a `0x3c` bulk table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct BulkTableRow {
    /// Row identity encoded by the compact, paged, or escaped atom form.
    pub row_id: u32,
    /// Fixed-width little-endian allocation handle.
    pub handle: u32,
    /// Byte offset of the row's `0x81` tag within the payload.
    pub offset: usize,
}

/// One schema-free field in a `7C0A` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
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
        #[cfg_attr(feature = "schema", schemars(with = "String"))]
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
        /// Complete allocation rows in serialized order.
        rows: Vec<BulkTableRow>,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum AliasLead {
    /// Low byte `0x01`: ordinary surface-support storage.
    SurfaceSupportStorage,
    /// Exact value `0x8e`: E5-linked surface storage.
    E5LinkedSurfaceStorage,
    /// Exact value `0x8f`: ordinal-linked alias storage.
    OrdinalLinkedStorage8f,
    /// Zero word preceding a complete grouped-alias core.
    NonSurfaceAlias,
    /// Other admitted word preceding a complete alias core.
    Unclassified(u32),
}

/// Group-allocation header attached to an outer surface-alias row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct AliasGroupMembership {
    /// `ObjectModeler` node prototype.
    pub prototype: u32,
    /// Identity shared by the nodes in one alias group.
    pub group_id: u32,
    /// Four-byte allocation slot beginning in F1's third byte.
    pub target_slot: u32,
    /// Complete bounded storage prefix between the group header and alias marker.
    #[serde(with = "cadmpeg_ir::bytes")]
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
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
            let row = pos.checked_sub(alias_row::MARKER)?;
            let tag_raw = View::u32_le_at(data, row + alias_row::TAG)?;
            let tag = tag_raw & 0x00ff_ffff;
            if row + alias_row::LEN > data.len() {
                return None;
            }
            let lead_raw = View::u32_le_at(data, row + alias_row::LEAD)?;
            let group = alias_group_membership(data, pos);
            let lead = if lead_raw & 0xff == 1 {
                AliasLead::SurfaceSupportStorage
            } else if lead_raw == 0x8e {
                AliasLead::E5LinkedSurfaceStorage
            } else if lead_raw == 0x8f {
                AliasLead::OrdinalLinkedStorage8f
            } else if lead_raw == 0x0000_0133 {
                AliasLead::Unclassified(lead_raw)
            } else if group.is_none() {
                return None;
            } else if lead_raw == 0 {
                AliasLead::NonSurfaceAlias
            } else {
                AliasLead::Unclassified(lead_raw)
            };
            let f1 = [
                data[row + alias_row::F1],
                data[row + alias_row::F1 + 1],
                data[row + alias_row::F1 + 2],
            ];
            Some(SurfaceAlias {
                pos,
                lead,
                lead_raw,
                tag,
                tag_raw,
                flag: data[row + alias_row::FLAG],
                f1,
                entity_record_ordinal: f1[2],
                f2: View::u32_le_at(data, row + alias_row::F2)?,
                f3: View::u32_le_at(data, row + alias_row::F3)?,
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
        prototype: View::u32_le_at(data, start + 2)?,
        group_id: View::u32_le_at(data, start + 6)?,
        target_slot: View::u32_le_at(data, marker + 11)?,
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
pub fn parse(data: &[u8]) -> Option<ObjectGraph> {
    parse_all(data)
        .into_iter()
        .max_by_key(|graph| graph.records.len())
}

/// Parse every length-closed `7C08` object graph in source order.
#[must_use]
pub fn parse_all(data: &[u8]) -> Vec<ObjectGraph> {
    parse_all_with_paired_roots(data, &std::collections::HashMap::new())
}

/// Parse every length-closed object graph, admitting opaque childless records
/// only when a preceding entity-table run selects the exact root and record
/// cardinality.
#[must_use]
pub(crate) fn parse_all_with_paired_roots(
    data: &[u8],
    paired_roots: &std::collections::HashMap<usize, usize>,
) -> Vec<ObjectGraph> {
    let catalogs = catalog::parse(data);
    let value_blocks = value_block::parse(data);
    let mut roots = Vec::<ObjectGraph>::new();
    let mut enclosing_end = 0usize;
    for pos in memchr::memchr_iter(0x7c, data) {
        let Some(marker_tail) = pos.checked_add(1) else {
            continue;
        };
        if data.get(marker_tail) != Some(&0x08) {
            continue;
        }
        let declared_end = pos
            .checked_add(2)
            .and_then(|length_offset| View::u32_le_at(data, length_offset))
            .and_then(|length| usize::try_from(length).ok())
            .and_then(|length| pos.checked_add(length));
        if pos < enclosing_end && declared_end.is_some_and(|end| end <= enclosing_end) {
            continue;
        }
        let graph = parse_candidate(data, pos, false).or_else(|| {
            let expected_count = *paired_roots.get(&pos)?;
            let graph = parse_candidate(data, pos, true)?;
            (graph.records.len() == expected_count).then_some(graph)
        });
        let Some(graph) = graph else {
            continue;
        };
        if let Some(graph_end) = graph.pos.checked_add(graph.total_len) {
            enclosing_end = enclosing_end.max(graph_end);
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

fn parse_candidate(
    data: &[u8],
    pos: usize,
    allow_opaque_childless_records: bool,
) -> Option<ObjectGraph> {
    let total_len = usize::try_from(View::u32_le_at(data, pos + 2)?).ok()?;
    let end = pos.checked_add(total_len)?;
    if total_len < 15 || end > data.len() {
        return None;
    }
    let mut at = pos + 6;
    let mut records = Vec::new();
    while at + 6 <= end && data.get(at..at + 2) == Some(&[0x7c, 0x09]) {
        let record_len = usize::try_from(View::u32_le_at(data, at + 2)?).ok()?;
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
                let child_len = usize::try_from(View::u32_le_at(data, child + 2)?).ok()?;
                (child_len >= 6 && child.checked_add(child_len) == Some(record_end))
                    .then_some((child, child_len))
            });
        let child = children.next();
        if child.is_some() && children.next().is_some() {
            return None;
        }
        let body = data.get(head_start..record_end)?;
        let (head_bytes, inline_body, payload) = match child {
            Some((child, _)) => (
                data.get(head_start..child)?,
                None,
                decode_payload(&data[child + 6..record_end])?,
            ),
            None if is_inline_body(body) => (
                &[][..],
                Some(body.to_vec()),
                ObjectPayload {
                    size: 0,
                    fields: Vec::new(),
                },
            ),
            None if allow_opaque_childless_records && !body.is_empty() => (
                &[][..],
                Some(body.to_vec()),
                ObjectPayload {
                    size: 0,
                    fields: Vec::new(),
                },
            ),
            None => return None,
        };
        let lead = if inline_body.is_some() {
            body[0]
        } else {
            *head_bytes.first()?
        };
        let head = decode_head(head_bytes);
        let roles = head_roles(lead, &head);
        let repeated_reference_suffix = repeated_reference_suffix(&payload);
        let subtype = classify(&payload.fields);
        records.push(ObjectRecord {
            index: records.len(),
            pos: at,
            total_len: record_len,
            lead,
            head,
            inline_body,
            owner_ref: roles.owner_ref,
            owner_literal: roles.owner_literal,
            class_ref: roles.class_ref,
            class_name: None,
            storage_ref: roles.storage_ref,
            payload,
            repeated_reference_suffix,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HeadRoles {
    pub(crate) owner_ref: Option<u32>,
    pub(crate) owner_literal: Option<u8>,
    pub(crate) class_ref: Option<u32>,
    pub(crate) storage_ref: Option<u32>,
}

pub(crate) fn head_roles(lead: u8, head: &[HeadToken]) -> HeadRoles {
    let separator_roles = matches!(head.get(1), Some(HeadToken::Separator));
    let extended_role_count = extended_compact_role_count(head);
    let null_lane_roles = matches!(
        head,
        [
            HeadToken::Lead(0x1a),
            HeadToken::Reference(_),
            HeadToken::Reference(0),
            HeadToken::NullHandle,
            HeadToken::Reference(owner),
        ] if *owner != 0
    ) || matches!(
        head,
        [
            HeadToken::Lead(0x1a),
            HeadToken::Reference(_),
            HeadToken::Reference(0),
            HeadToken::NullHandle,
            HeadToken::Reference(0),
            HeadToken::Reference(_),
            HeadToken::Literal(0),
            HeadToken::Literal(0),
        ]
    ) || matches!(
        head,
        [
            HeadToken::Lead(0x1a),
            HeadToken::Reference(_),
            HeadToken::Reference(0),
            HeadToken::NullHandle,
            HeadToken::Reference(0),
            HeadToken::Reference(_) | HeadToken::Literal(_),
            HeadToken::Literal(20 | 21 | 22 | 23 | 26 | 27 | 28),
            HeadToken::Literal(0),
            HeadToken::Literal(0),
        ]
    );
    let terminal_null_lane_roles = matches!(
        head,
        [
            HeadToken::Lead(0x5a),
            HeadToken::Reference(_),
            HeadToken::Reference(0),
            HeadToken::NullHandle,
            HeadToken::Reference(owner),
            HeadToken::Reference(3),
        ] if *owner != 0
    );
    let terminal_lane_roles = matches!(
        head,
        [
            HeadToken::Lead(0x56),
            HeadToken::Reference(_),
            HeadToken::Reference(_),
            HeadToken::Reference(owner),
            HeadToken::Reference(3),
        ] if *owner != 0
    );
    let fixed_role_count = match lead {
        0x02 => 1,
        0x12 => 2,
        0x16 | 0x52 => 3,
        _ => 0,
    };
    let fixed_roles = fixed_role_count != 0 && head.len() == fixed_role_count + 1;
    let (owner_index, class_index, storage_index, class_first) = if separator_roles {
        (Some(2), Some(3), Some(4), false)
    } else if null_lane_roles || terminal_null_lane_roles {
        (Some(4), Some(1), Some(2), true)
    } else if terminal_lane_roles {
        (Some(3), Some(1), Some(2), true)
    } else if extended_role_count.is_some() || fixed_roles {
        match lead {
            0x02 => (Some(1), None, None, false),
            0x12 => (
                Some(1),
                (extended_role_count != Some(1)).then_some(2),
                None,
                false,
            ),
            0x16 | 0x56 => (Some(3), Some(1), Some(2), true),
            0x52 => (Some(1), Some(2), Some(3), false),
            _ => (None, None, None, false),
        }
    } else {
        (None, None, None, false)
    };
    let role_reference = |index: Option<usize>| match index.and_then(|index| head.get(index)) {
        Some(HeadToken::Reference(value)) => Some(*value),
        _ => None,
    };
    let role_literal = |index: Option<usize>| match index.and_then(|index| head.get(index)) {
        Some(HeadToken::Literal(value)) => Some(*value),
        _ => None,
    };
    if class_first {
        let class_ref = role_reference(class_index);
        let storage_ref = class_ref.and_then(|_| role_reference(storage_index));
        let owner_ref = storage_ref.and_then(|_| role_reference(owner_index));
        let owner_literal = storage_ref.and_then(|_| role_literal(owner_index));
        HeadRoles {
            owner_ref,
            owner_literal,
            class_ref,
            storage_ref,
        }
    } else {
        let owner_ref = role_reference(owner_index);
        let class_ref = owner_ref.and_then(|_| role_reference(class_index));
        let storage_ref = class_ref.and_then(|_| role_reference(storage_index));
        HeadRoles {
            owner_ref,
            owner_literal: None,
            class_ref,
            storage_ref,
        }
    }
}

fn extended_compact_role_count(head: &[HeadToken]) -> Option<usize> {
    if matches!(
        (head.first(), head.last()),
        (Some(HeadToken::Lead(0x56)), Some(HeadToken::Reference(3)))
    ) {
        let mut base_head = head[..head.len() - 1].to_vec();
        base_head[0] = HeadToken::Lead(0x16);
        return extended_compact_role_count(&base_head);
    }
    if matches!(
        head,
        [
            HeadToken::Lead(0x16),
            HeadToken::Reference(_),
            HeadToken::Reference(storage),
            HeadToken::Reference(0),
            HeadToken::Reference(_),
            HeadToken::Literal(0),
            HeadToken::Literal(0),
        ] if *storage != 0
    ) {
        return Some(3);
    }
    if matches!(
        head,
        [
            HeadToken::Lead(0x16),
            HeadToken::Reference(_),
            HeadToken::Reference(0),
            owner_token @ (HeadToken::Reference(_) | HeadToken::Literal(_)),
            HeadToken::Literal(21 | 23),
            HeadToken::Literal(0),
            HeadToken::Literal(0),
            HeadToken::Reference(_),
        ] if !matches!(owner_token, HeadToken::Reference(0))
    ) {
        return Some(3);
    }
    if matches!(
        head,
        [
            HeadToken::Lead(0x12),
            HeadToken::Reference(owner),
            HeadToken::Reference(0),
            ..,
            HeadToken::Literal(0),
            HeadToken::Literal(0),
        ] if *owner != 0 && matches!(head.len(), 6 | 7)
    ) {
        return Some(1);
    }
    if matches!(
        head,
        [
            HeadToken::Lead(0x16),
            HeadToken::Reference(_),
            HeadToken::Reference(storage),
            HeadToken::Reference(0),
            _,
            HeadToken::Literal(20 | 21),
            HeadToken::Literal(0),
            HeadToken::Literal(0),
        ] if *storage != 0
    ) {
        return Some(3);
    }
    if matches!(
        head,
        [
            HeadToken::Lead(0x16),
            HeadToken::Reference(_),
            HeadToken::Reference(0),
            HeadToken::Reference(_) | HeadToken::Literal(_) | HeadToken::Separator,
            HeadToken::Literal(21 | 22 | 23 | 26 | 27 | 28),
            HeadToken::Literal(0),
            HeadToken::Literal(0),
            HeadToken::Reference(0),
            _,
            HeadToken::Literal(20 | 21 | 24 | 25 | 28),
            HeadToken::Literal(0),
            HeadToken::Literal(0),
        ]
    ) {
        return Some(3);
    }
    let extended_owner_class_storage = matches!(
        head,
        [
            HeadToken::Lead(0x52),
            HeadToken::Reference(owner),
            HeadToken::Reference(0),
            HeadToken::Reference(_) | HeadToken::Literal(_),
            HeadToken::Literal(_),
            HeadToken::Literal(0),
            HeadToken::Literal(0),
            HeadToken::Reference(3),
        ] if *owner != 0
    ) || matches!(
        head,
        [
            HeadToken::Lead(0x52),
            HeadToken::Reference(owner),
            HeadToken::Reference(0),
            HeadToken::Reference(_),
            HeadToken::Literal(0),
            HeadToken::Literal(0),
            HeadToken::Reference(3),
        ] if *owner != 0
    ) || matches!(
        head,
        [
            HeadToken::Lead(0x52),
            HeadToken::Reference(owner),
            HeadToken::Reference(0),
            HeadToken::Reference(_),
            HeadToken::Literal(0),
            HeadToken::Literal(0),
            HeadToken::Reference(0),
            HeadToken::Reference(_),
            HeadToken::Literal(0),
            HeadToken::Literal(0),
        ] if *owner != 0 && head[3] == head[7]
    ) || matches!(
        head,
        [
            HeadToken::Lead(0x52),
            HeadToken::Reference(owner),
            HeadToken::Reference(0),
            HeadToken::Reference(_) | HeadToken::Literal(_),
            HeadToken::Literal(_),
            HeadToken::Literal(0),
            HeadToken::Literal(0),
            HeadToken::Reference(0),
            HeadToken::Reference(_) | HeadToken::Literal(_),
            HeadToken::Literal(_),
            HeadToken::Literal(0),
            HeadToken::Literal(0),
        ] if *owner != 0 && head[3] == head[8] && head[4] == head[9]
    );
    if extended_owner_class_storage {
        return Some(3);
    }
    let extended_class_storage_owner = matches!(
        head,
        [
            HeadToken::Lead(0x16),
            HeadToken::Reference(_),
            HeadToken::Reference(0),
            owner_token @ (HeadToken::Reference(_) | HeadToken::Literal(_)),
            HeadToken::Literal(22 | 23),
            HeadToken::Literal(0),
            HeadToken::Literal(0),
            HeadToken::Reference(0),
            HeadToken::Reference(_),
            HeadToken::Literal(0),
            HeadToken::Literal(0),
        ] if !matches!(owner_token, HeadToken::Reference(0))
    ) || matches!(
        head,
        [
            HeadToken::Lead(0x16),
            HeadToken::Reference(_),
            HeadToken::Reference(0),
            HeadToken::Reference(owner),
            HeadToken::Literal(0),
            HeadToken::Literal(0),
            HeadToken::Reference(0),
            _,
            HeadToken::Literal(28),
            HeadToken::Literal(0),
            HeadToken::Literal(0),
        ] if *owner != 0
    );
    extended_class_storage_owner.then_some(3)
}

pub(crate) fn is_inline_body(body: &[u8]) -> bool {
    let Some(rest) = body.strip_prefix(&[0x10, 0xfe]) else {
        return false;
    };
    let Some(rest) = strip_reference(rest) else {
        return false;
    };
    let rest = if let Some(rest) = rest.strip_prefix(&[0x82, 0xf2, 0xf0, 0x82]) {
        rest
    } else if rest.len() >= 12
        && rest[0] == 0x82
        && rest[1] == 0x32
        && rest[6] == 0x32
        && rest[11] == 0x82
    {
        &rest[12..]
    } else {
        return false;
    };
    let Some(rest) = strip_reference(rest) else {
        return false;
    };
    if rest == [0x81, 0x06] {
        return true;
    }
    let Some(rest) = rest.strip_prefix(&[0x82]) else {
        return false;
    };
    strip_reference(rest) == Some(&[0x06][..])
}

fn strip_reference(bytes: &[u8]) -> Option<&[u8]> {
    match bytes {
        [0x80..=0xd0, rest @ ..] => Some(rest),
        [0xd1..=0xe4, _, rest @ ..] => Some(rest),
        _ => None,
    }
}

pub(crate) fn repeated_reference_suffix(
    payload: &ObjectPayload,
) -> Option<RepeatedReferenceSuffix> {
    let fields = &payload.fields;
    let mut matches = fields
        .iter()
        .enumerate()
        .filter_map(|(count_index, field)| {
            let PayloadField::Atom {
                value: declared_count,
                offset: first_count_offset,
            } = field
            else {
                return None;
            };
            if *declared_count < 2
                || !matches!(
                    fields.get(count_index.checked_sub(1)?),
                    Some(PayloadField::Atom { value: 48, .. })
                )
            {
                return None;
            }
            let count = usize::try_from(*declared_count).ok()?;
            let references_start = count_index.checked_add(1)?;
            let references_end = references_start.checked_add(count)?;
            let first = fields
                .get(references_start..references_end)?
                .iter()
                .map(|field| match field {
                    PayloadField::Reference { value, .. } => Some(*value),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()?;
            let PayloadField::Atom {
                value: repeated_count,
                offset: repeated_count_offset,
            } = fields.get(references_end)?
            else {
                return None;
            };
            if repeated_count != declared_count {
                return None;
            }
            let repeated_start = references_end.checked_add(1)?;
            let repeated_end = repeated_start.checked_add(count.checked_sub(1)?)?;
            let terminator_start = repeated_end.checked_add(1)?;
            let repeated = fields
                .get(repeated_start..repeated_end)?
                .iter()
                .map(|field| match field {
                    PayloadField::Reference { value, .. } => Some(*value),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()?;
            if repeated != first[..count - 1]
                || !matches!(
                    fields.get(repeated_end),
                    Some(PayloadField::Atom { value: 129, .. })
                )
                || !matches!(
                    fields.get(terminator_start..),
                    Some([PayloadField::Terminator])
                )
            {
                return None;
            }
            Some(RepeatedReferenceSuffix {
                schema_preamble: reference_schema_preamble(&fields[..count_index - 1]),
                repeated_references: repeated,
                terminal_reference: first[count - 1],
                first_count_offset: *first_count_offset,
                repeated_count_offset: *repeated_count_offset,
            })
        });
    let suffix = matches.next()?;
    matches.next().is_none().then_some(suffix)
}

fn reference_schema_preamble(fields: &[PayloadField]) -> Option<ReferenceSchemaPreamble> {
    let mut matches = fields.windows(4).filter_map(|fields| match fields {
        [
            PayloadField::Blob {
                declared_len: 59, ..
            },
            PayloadField::Atom { value: 5, .. },
            PayloadField::Atom { value: 46, .. },
            PayloadField::Atom {
                value: schema_ref,
                offset,
            },
        ] => Some(ReferenceSchemaPreamble::BlobThenSchema {
            schema_ref: *schema_ref,
            offset: *offset,
        }),
        [
            PayloadField::Atom {
                value: schema_ref,
                offset,
            },
            PayloadField::Atom { value: 34, .. },
            PayloadField::Blob {
                declared_len: 59, ..
            },
            PayloadField::Atom { value: 5, .. },
        ] => Some(ReferenceSchemaPreamble::SchemaThenBlob {
            schema_ref: *schema_ref,
            offset: *offset,
        }),
        _ => None,
    });
    let preamble = matches.next()?;
    matches.next().is_none().then_some(preamble)
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
        0xd1..=0xe4 if at + 2 < bytes.len() => Some((
            u32::from(byte - 0xd1) * 256 + u32::from(bytes[at + 1]) + 1,
            2,
        )),
        0xd1..=0xe4 => None,
        _ => Some((u32::from(byte), 1)),
    }
}

fn tagged_value(bytes: &[u8], at: usize) -> Option<(u32, usize)> {
    if matches!(bytes.get(at), Some(0x80 | 0x32)) && at.checked_add(5)? < bytes.len() {
        return Some((View::u32_le_at(bytes, at + 1)?, 5));
    }
    atom(bytes, at)
}

fn is_final_terminator_run(bytes: &[u8], at: usize) -> bool {
    bytes.get(at) == Some(&0xfe) && bytes[at..].iter().all(|byte| *byte == 0xfe)
}

fn decode_payload(bytes: &[u8]) -> Option<ObjectPayload> {
    let mut fields = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let offset = at;
        match bytes[at] {
            0xfe if is_final_terminator_run(bytes, at) => {
                while bytes.get(at) == Some(&0xfe) {
                    fields.push(PayloadField::Terminator);
                    at += 1;
                }
                break;
            }
            0xe5 if blob_end(bytes, at).is_some() => {
                let declared_len =
                    usize::try_from(View::u32_le_at(bytes, at + 1).expect("checked blob header"))
                        .expect("u32 fits supported usize");
                let start = at + 5;
                let end = blob_end(bytes, at).expect("checked blob extent");
                fields.push(PayloadField::Blob {
                    declared_len,
                    bytes: bytes[start..end].to_vec(),
                    offset,
                });
                at = end;
            }
            0xe5 if blob_declared_end(bytes, at) == Some(bytes.len()) => return None,
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
                let Some(table_count) = View::u32_le_at(bytes, table_at) else {
                    fields.push(PayloadField::Atom {
                        value: 0x3c,
                        offset,
                    });
                    at += 1;
                    continue;
                };
                let table_end = table_at.checked_add(4)?;
                if table_count <= u32::try_from(bytes.len().saturating_sub(table_end)).unwrap_or(0)
                {
                    let (rows, end) = parse_bulk_table_rows(bytes, table_end, table_count)?;
                    fields.push(PayloadField::BulkTable {
                        count,
                        table_count,
                        rows,
                        offset,
                    });
                    at = end;
                    continue;
                }
                fields.push(PayloadField::Atom {
                    value: 0x3c,
                    offset,
                });
                at += 1;
            }
            0x3b => {
                if is_final_terminator_run(bytes, at + 1) {
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
                    if at >= bytes.len() || is_final_terminator_run(bytes, at) {
                        break;
                    }
                    let item_offset = at;
                    let tagged_reference = bytes[at] == 0x81;
                    let tagged_atom = bytes[at] == 0x80;
                    let fixed_reference = bytes[at] == 0x32;
                    let fixed_atom = tagged_atom
                        && at
                            .checked_add(5)
                            .is_some_and(|fixed_end| fixed_end < bytes.len());
                    let value_at =
                        at + usize::from(tagged_reference || (tagged_atom && !fixed_atom));
                    if (tagged_reference || tagged_atom)
                        && (value_at >= bytes.len() || is_final_terminator_run(bytes, value_at))
                    {
                        at = value_at;
                        break;
                    }
                    let Some((value, consumed)) = tagged_value(bytes, value_at) else {
                        break;
                    };
                    items.push(if tagged_reference || fixed_reference {
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
            0x80 | 0x32 if at + 5 < bytes.len() => {
                let tag = bytes[at];
                fields.push(if tag == 0x80 {
                    PayloadField::Atom {
                        value: View::u32_le_at(bytes, at + 1).expect("checked escaped atom extent"),
                        offset,
                    }
                } else {
                    PayloadField::Reference {
                        value: View::u32_le_at(bytes, at + 1).expect("checked scalar extent"),
                        offset,
                    }
                });
                at += 5;
            }
            0x81 | 0x3a | 0x39 | 0x7a => {
                let tag = bytes[at];
                if is_final_terminator_run(bytes, at + 1) {
                    fields.push(PayloadField::Atom {
                        value: u32::from(tag),
                        offset,
                    });
                    at += 1;
                    continue;
                }
                let Some((value, consumed)) = tagged_value(bytes, at + 1) else {
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

fn parse_bulk_table_rows(
    bytes: &[u8],
    mut at: usize,
    table_count: u32,
) -> Option<(Vec<BulkTableRow>, usize)> {
    let count = usize::try_from(table_count).ok()?;
    let mut rows = Vec::with_capacity(count.min(bytes.len()));
    for _ in 0..count {
        let offset = at;
        if bytes.get(at) != Some(&0x81) {
            return None;
        }
        at += 1;
        let row_id = bulk_row_id(bytes, &mut at)?;
        if bytes.get(at) != Some(&0x80) {
            return None;
        }
        let handle = View::u32_le_at(bytes, at + 1)?;
        at += 5;
        rows.push(BulkTableRow {
            row_id,
            handle,
            offset,
        });
    }
    Some((rows, at))
}

fn bulk_row_id(bytes: &[u8], at: &mut usize) -> Option<u32> {
    let start = *at;
    let mut candidates = Vec::new();
    if let Some((value, consumed)) = atom(bytes, start) {
        let end = start.checked_add(consumed)?;
        if bytes.get(end) == Some(&0x80) && end.checked_add(5)? <= bytes.len() {
            candidates.push((value, end));
        }
    }
    if bytes.get(start) == Some(&0x80) {
        let end = start.checked_add(5)?;
        if end.checked_add(5)? <= bytes.len() && bytes.get(end) == Some(&0x80) {
            candidates.push((View::u32_le_at(bytes, start + 1)?, end));
        }
    }
    let [(value, end)] = candidates.as_slice() else {
        return None;
    };
    *at = *end;
    Some(*value)
}

fn blob_end(bytes: &[u8], at: usize) -> Option<usize> {
    let end = blob_declared_end(bytes, at)?;
    (end < bytes.len()).then_some(end)
}

fn blob_declared_end(bytes: &[u8], at: usize) -> Option<usize> {
    let declared_len = usize::try_from(View::u32_le_at(bytes, at + 1)?).ok()?;
    at.checked_add(5)?.checked_add(declared_len)
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
    if fields.is_empty()
        || fields
            .iter()
            .all(|field| matches!(field, PayloadField::Terminator))
    {
        PayloadSubtype::Empty
    } else {
        PayloadSubtype::Mixed
    }
}

#[cfg(test)]
mod repeated_reference_suffix_tests {
    use super::*;

    fn atom(value: u32, offset: usize) -> PayloadField {
        PayloadField::Atom { value, offset }
    }

    fn reference(value: u32, offset: usize) -> PayloadField {
        PayloadField::Reference { value, offset }
    }

    #[test]
    fn repeated_reference_suffix_requires_an_exact_counted_reference_copy() {
        let payload = ObjectPayload {
            size: 83,
            fields: vec![
                atom(44, 0),
                PayloadField::Blob {
                    declared_len: 59,
                    bytes: vec![0; 59],
                    offset: 1,
                },
                atom(5, 65),
                atom(46, 66),
                atom(19, 67),
                atom(48, 68),
                atom(3, 69),
                reference(60, 70),
                reference(62, 72),
                reference(49, 74),
                atom(3, 76),
                reference(60, 77),
                reference(62, 79),
                atom(129, 81),
                PayloadField::Terminator,
            ],
        };

        assert_eq!(
            repeated_reference_suffix(&payload),
            Some(RepeatedReferenceSuffix {
                schema_preamble: Some(ReferenceSchemaPreamble::BlobThenSchema {
                    schema_ref: 19,
                    offset: 67,
                }),
                repeated_references: vec![60, 62],
                terminal_reference: 49,
                first_count_offset: 69,
                repeated_count_offset: 76,
            })
        );
    }

    #[test]
    fn repeated_reference_suffix_decodes_schema_then_blob_preamble() {
        let payload = ObjectPayload {
            size: 79,
            fields: vec![
                atom(33, 0),
                atom(19, 1),
                atom(34, 2),
                PayloadField::Blob {
                    declared_len: 59,
                    bytes: vec![0; 59],
                    offset: 3,
                },
                atom(5, 67),
                atom(48, 68),
                atom(2, 69),
                reference(60, 70),
                reference(49, 72),
                atom(2, 74),
                reference(60, 75),
                atom(129, 77),
                PayloadField::Terminator,
            ],
        };

        assert_eq!(
            repeated_reference_suffix(&payload)
                .expect("repeated reference suffix")
                .schema_preamble,
            Some(ReferenceSchemaPreamble::SchemaThenBlob {
                schema_ref: 19,
                offset: 1,
            })
        );
    }

    #[test]
    fn repeated_reference_suffix_rejects_a_changed_reference() {
        let payload = ObjectPayload {
            size: 16,
            fields: vec![
                atom(48, 0),
                atom(3, 1),
                reference(60, 2),
                reference(62, 3),
                reference(49, 4),
                atom(3, 5),
                reference(60, 6),
                reference(63, 7),
                atom(129, 8),
                PayloadField::Terminator,
            ],
        };

        assert_eq!(repeated_reference_suffix(&payload), None);
    }
}

#[cfg(test)]
mod tests;
