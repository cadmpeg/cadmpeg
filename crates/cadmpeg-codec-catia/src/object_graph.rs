// SPDX-License-Identifier: Apache-2.0
//! Outer `7C08` feature and object-ownership graph decoder.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::checked::extents_overlap;
use crate::layout::outer_alias_row as alias_row;
use crate::{catalog, entity_table, value_block};

/// One decoded outer object graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ObjectGraph {
    /// Offset of the selected `7C08` root.
    pub(crate) pos: usize,
    /// Root total length, including its six-byte header.
    pub(crate) total_len: usize,
    /// Byte offset of the immediately associated `7C02` schema catalog.
    pub(crate) catalog_pos: Option<usize>,
    /// Consecutive `7C09` records.
    pub(crate) records: Vec<ObjectRecord>,
}

/// One `7C09` object record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ObjectRecordWire", into = "ObjectRecordWire")]
pub(crate) struct ObjectRecord {
    /// Record byte offset.
    pub(crate) pos: usize,
    /// Record total length, including its six-byte header.
    pub(crate) total_len: usize,
    /// First head byte.
    pub(crate) lead: u8,
    /// Inline body or nested head and payload.
    body: ObjectRecordBody,
}

// Serialized role fields are retained for wire compatibility and checked once on input.
#[derive(Serialize, Deserialize)]
struct ObjectRecordWire {
    pos: usize,
    total_len: usize,
    lead: u8,
    body: ObjectRecordBody,
    owner_ref: Option<u32>,
    owner_literal: Option<u8>,
    class_ref: Option<u32>,
    storage_ref: Option<u32>,
}

impl From<ObjectRecord> for ObjectRecordWire {
    fn from(record: ObjectRecord) -> Self {
        let roles = record.roles();
        let (owner_ref, owner_literal) = match roles.owner {
            Some(HeadOwner::Entity(value)) => (Some(value), None),
            Some(HeadOwner::UnassignedLiteral(value)) => (None, Some(value)),
            None => (None, None),
        };
        Self {
            pos: record.pos,
            total_len: record.total_len,
            lead: record.lead,
            body: record.body,
            owner_ref,
            owner_literal,
            class_ref: roles.class_ref,
            storage_ref: roles.storage_ref,
        }
    }
}

impl TryFrom<ObjectRecordWire> for ObjectRecord {
    type Error = &'static str;

    fn try_from(wire: ObjectRecordWire) -> Result<Self, Self::Error> {
        let owner = match (wire.owner_ref, wire.owner_literal) {
            (Some(value), None) => Some(HeadOwner::Entity(value)),
            (None, Some(value)) => Some(HeadOwner::UnassignedLiteral(value)),
            (None, None) => None,
            (Some(_), Some(_)) => return Err("owner_ref and owner_literal are mutually exclusive"),
        };
        let supplied = HeadRoles {
            owner,
            class_ref: wire.class_ref,
            storage_ref: wire.storage_ref,
        };
        let record = Self {
            pos: wire.pos,
            total_len: wire.total_len,
            lead: wire.lead,
            body: wire.body,
        };
        if supplied != record.roles() {
            return Err("object record roles disagree with head tokens");
        }
        Ok(record)
    }
}

/// Inline or nested body of one `7C09` object record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ObjectRecordBodyWire", into = "ObjectRecordBodyWire")]
enum ObjectRecordBody {
    /// Complete alternate inline body when the record has no nested `7C0A`.
    Inline(Vec<u8>),
    /// Nested head tokens and `7C0A` payload.
    Nested {
        /// Decoded head tokens.
        head: Vec<HeadToken>,
        /// Decoded nested payload.
        payload: ObjectPayload,
    },
}

#[derive(Serialize, Deserialize)]
enum ObjectRecordBodyWire {
    /// Complete alternate inline body when the record has no nested `7C0A`.
    Inline(Vec<u8>),
    /// Nested head tokens and `7C0A` payload.
    Nested {
        /// Decoded head tokens.
        head: Vec<HeadToken>,
        /// Decoded nested payload.
        payload: ObjectPayload,
        /// Counted reference suffix when the payload repeats its reference prefix exactly.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        repeated_reference_suffix: Option<RepeatedReferenceSuffix>,
        /// Structural payload classification.
        subtype: PayloadSubtype,
    },
}

impl From<ObjectRecordBody> for ObjectRecordBodyWire {
    fn from(body: ObjectRecordBody) -> Self {
        match body {
            ObjectRecordBody::Inline(bytes) => Self::Inline(bytes),
            ObjectRecordBody::Nested { head, payload } => Self::Nested {
                subtype: classify(&payload.fields),
                repeated_reference_suffix: repeated_reference_suffix(&payload),
                head,
                payload,
            },
        }
    }
}
impl TryFrom<ObjectRecordBodyWire> for ObjectRecordBody {
    type Error = &'static str;
    fn try_from(wire: ObjectRecordBodyWire) -> Result<Self, Self::Error> {
        match wire {
            ObjectRecordBodyWire::Inline(bytes) => Ok(Self::Inline(bytes)),
            ObjectRecordBodyWire::Nested {
                head,
                payload,
                subtype,
                repeated_reference_suffix: suffix,
            } => {
                if subtype != classify(&payload.fields) {
                    return Err("subtype disagrees with payload");
                }
                if suffix != repeated_reference_suffix(&payload) {
                    return Err("repeated_reference_suffix disagrees with payload");
                }
                Ok(Self::Nested { head, payload })
            }
        }
    }
}

impl ObjectRecord {
    pub(crate) fn roles(&self) -> HeadRoles {
        head_roles(self.lead, self.head())
    }

    pub(crate) fn inline_body(&self) -> Option<&[u8]> {
        match &self.body {
            ObjectRecordBody::Inline(bytes) => Some(bytes),
            ObjectRecordBody::Nested { .. } => None,
        }
    }

    pub(crate) fn head(&self) -> &[HeadToken] {
        match &self.body {
            ObjectRecordBody::Inline(_) => &[],
            ObjectRecordBody::Nested { head, .. } => head,
        }
    }

    pub(crate) fn payload(&self) -> &ObjectPayload {
        match &self.body {
            ObjectRecordBody::Inline(_) => {
                static EMPTY: ObjectPayload = ObjectPayload {
                    size: 0,
                    fields: Vec::new(),
                };
                &EMPTY
            }
            ObjectRecordBody::Nested { payload, .. } => payload,
        }
    }

    #[cfg(test)]
    fn subtype(&self) -> PayloadSubtype {
        classify(&self.payload().fields)
    }
}

/// Token in a `7C09` record head.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum HeadToken {
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
pub(crate) struct ObjectPayload {
    /// Payload size in bytes.
    pub(crate) size: usize,
    /// Decoded fields in serialization order.
    pub(crate) fields: Vec<PayloadField>,
}

impl ObjectPayload {
    pub(crate) fn copy_charged(&self, ctx: &DecodeContext<'_>) -> Result<Self, CodecError> {
        let mut fields = Vec::new();
        for field in &self.fields {
            let copy = match field {
                PayloadField::Blob { bytes, offset } => PayloadField::Blob {
                    bytes: crate::resource::copy_retained_slice(
                        ctx,
                        bytes,
                        "catia_native_payload_blob",
                    )?,
                    offset: *offset,
                },
                PayloadField::BulkTable {
                    count,
                    rows,
                    offset,
                } => PayloadField::BulkTable {
                    count: *count,
                    rows: crate::resource::copy_retained_slice(
                        ctx,
                        rows,
                        "catia_native_payload_bulk_rows",
                    )?,
                    offset: *offset,
                },
                PayloadField::List {
                    declared_count,
                    items,
                    offset,
                } => PayloadField::List {
                    declared_count: *declared_count,
                    items: crate::resource::copy_retained_slice(
                        ctx,
                        items,
                        "catia_native_payload_list_items",
                    )?,
                    offset: *offset,
                },
                other => other.clone(),
            };
            crate::resource::push(ctx, &mut fields, copy, "catia_native_payload_fields")?;
        }
        Ok(Self {
            size: self.size,
            fields,
        })
    }
}

/// One counted reference suffix whose reference prefix is serialized twice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RepeatedReferenceSuffix {
    /// Schema-selection production in the payload prefix before this suffix.
    pub(crate) schema_preamble: Option<ReferenceSchemaPreamble>,
    /// Ordered entity identities serialized in both vectors.
    pub(crate) repeated_references: Vec<u32>,
    /// Final reference in the first counted vector.
    pub(crate) terminal_reference: u32,
    /// Byte offset of the first count atom within the payload.
    first_count_offset: usize,
    /// Byte offset of the repeated count atom within the payload.
    repeated_count_offset: usize,
}

/// Schema reference carried by a repeated-reference payload preamble.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ReferenceSchemaPreamble {
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
pub(crate) enum ListItem {
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
pub(crate) struct BulkTableRow {
    /// Row identity encoded by the compact, paged, or escaped atom form.
    row_id: u32,
    /// Fixed-width little-endian allocation handle.
    handle: u32,
    /// Byte offset of the row's `0x81` tag within the payload.
    offset: usize,
}

/// One schema-free field in a `7C0A` payload.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "PayloadFieldWire")]
pub(crate) enum PayloadField {
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
        /// Complete blob bytes.
        #[serde(with = "cadmpeg_ir::bytes")]
        bytes: Vec<u8>,
        /// Byte offset within the payload.
        offset: usize,
    },
    /// Sane `0x3c` bulk-table header.
    BulkTable {
        /// Count atom preceding the table count.
        count: u32,
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

#[derive(Serialize, Deserialize)]
enum PayloadFieldWire {
    Atom {
        value: u32,
        offset: usize,
    },
    Reference {
        value: u32,
        offset: usize,
    },
    Scalar {
        tag: u8,
        value: u32,
        offset: usize,
    },
    Blob {
        declared_len: usize,
        #[serde(with = "cadmpeg_ir::bytes")]
        bytes: Vec<u8>,
        offset: usize,
    },
    BulkTable {
        count: u32,
        table_count: usize,
        rows: Vec<BulkTableRow>,
        offset: usize,
    },
    List {
        declared_count: u32,
        items: Vec<ListItem>,
        offset: usize,
    },
    Sentinel {
        offset: usize,
    },
    Terminator,
}

#[derive(Serialize)]
enum PayloadFieldWireRef<'a> {
    Atom { value: u32, offset: usize },
    Reference { value: u32, offset: usize },
    Scalar { tag: u8, value: u32, offset: usize },
    Blob {
        declared_len: usize,
        #[serde(with = "cadmpeg_ir::bytes")]
        bytes: &'a [u8],
        offset: usize,
    },
    BulkTable {
        count: u32,
        table_count: usize,
        rows: &'a [BulkTableRow],
        offset: usize,
    },
    List {
        declared_count: u32,
        items: &'a [ListItem],
        offset: usize,
    },
    Sentinel { offset: usize },
    Terminator,
}

impl Serialize for PayloadField {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let view = match self {
            Self::Atom { value, offset } => PayloadFieldWireRef::Atom { value: *value, offset: *offset },
            Self::Reference { value, offset } => PayloadFieldWireRef::Reference { value: *value, offset: *offset },
            Self::Scalar { tag, value, offset } => PayloadFieldWireRef::Scalar { tag: *tag, value: *value, offset: *offset },
            Self::Blob { bytes, offset } => PayloadFieldWireRef::Blob {
                declared_len: bytes.len(),
                bytes,
                offset: *offset,
            },
            Self::BulkTable { count, rows, offset } => PayloadFieldWireRef::BulkTable {
                count: *count,
                table_count: rows.len(),
                rows,
                offset: *offset,
            },
            Self::List { declared_count, items, offset } => PayloadFieldWireRef::List {
                declared_count: *declared_count,
                items,
                offset: *offset,
            },
            Self::Sentinel { offset } => PayloadFieldWireRef::Sentinel { offset: *offset },
            Self::Terminator => PayloadFieldWireRef::Terminator,
        };
        view.serialize(serializer)
    }
}

#[cfg(test)]
impl From<PayloadField> for PayloadFieldWire {
    fn from(value: PayloadField) -> Self {
        match value {
            PayloadField::Atom { value, offset } => Self::Atom { value, offset },
            PayloadField::Reference { value, offset } => Self::Reference { value, offset },
            PayloadField::Scalar { tag, value, offset } => Self::Scalar { tag, value, offset },
            PayloadField::Blob { bytes, offset } => Self::Blob {
                declared_len: bytes.len(),
                bytes,
                offset,
            },
            PayloadField::BulkTable {
                count,
                rows,
                offset,
            } => Self::BulkTable {
                count,
                table_count: rows.len(),
                rows,
                offset,
            },
            PayloadField::List {
                declared_count,
                items,
                offset,
            } => Self::List {
                declared_count,
                items,
                offset,
            },
            PayloadField::Sentinel { offset } => Self::Sentinel { offset },
            PayloadField::Terminator => Self::Terminator,
        }
    }
}
impl TryFrom<PayloadFieldWire> for PayloadField {
    type Error = &'static str;
    fn try_from(wire: PayloadFieldWire) -> Result<Self, Self::Error> {
        match wire {
            PayloadFieldWire::Atom { value, offset } => Ok(Self::Atom { value, offset }),
            PayloadFieldWire::Reference { value, offset } => Ok(Self::Reference { value, offset }),
            PayloadFieldWire::Scalar { tag, value, offset } => {
                Ok(Self::Scalar { tag, value, offset })
            }
            PayloadFieldWire::Blob {
                declared_len,
                bytes,
                offset,
            } => {
                if declared_len != bytes.len() {
                    return Err("declared_len disagrees with blob bytes");
                }
                Ok(Self::Blob { bytes, offset })
            }
            PayloadFieldWire::BulkTable {
                count,
                table_count,
                rows,
                offset,
            } => {
                if table_count != rows.len() {
                    return Err("table_count disagrees with rows");
                }
                Ok(Self::BulkTable {
                    count,
                    rows,
                    offset,
                })
            }
            PayloadFieldWire::List {
                declared_count,
                items,
                offset,
            } => Ok(Self::List {
                declared_count,
                items,
                offset,
            }),
            PayloadFieldWire::Sentinel { offset } => Ok(Self::Sentinel { offset }),
            PayloadFieldWire::Terminator => Ok(Self::Terminator),
        }
    }
}

/// Structural role of a decoded payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum PayloadSubtype {
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
pub(crate) enum AliasLead {
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

impl AliasLead {
    /// Classification of a stored alias lead word.
    pub(crate) fn from_raw(raw: u32) -> Self {
        if raw & 0xff == 1 {
            Self::SurfaceSupportStorage
        } else {
            match raw {
                0x8e => Self::E5LinkedSurfaceStorage,
                0x8f => Self::OrdinalLinkedStorage8f,
                0 => Self::NonSurfaceAlias,
                _ => Self::Unclassified(raw),
            }
        }
    }
}

/// Group-allocation header attached to an outer surface-alias row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AliasGroupMembership {
    /// `ObjectModeler` node prototype.
    pub(crate) prototype: u32,
    /// Identity shared by the nodes in one alias group.
    pub(crate) group_id: u32,
    /// Four-byte allocation slot beginning in F1's third byte.
    pub(crate) target_slot: u32,
    /// Complete bounded storage prefix between the group header and alias marker.
    #[serde(with = "cadmpeg_ir::bytes")]
    pub(crate) storage_prefix: Vec<u8>,
}

/// Fixed 20-byte core of an outer `01 00 04 00` surface-alias row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SurfaceAlias {
    /// Marker byte offset.
    pub(crate) pos: usize,
    /// Byte offset of the row frame. The marker sits at
    /// `outer_alias_row::MARKER` inside the row, and a row is admitted only
    /// when that many bytes precede the marker, so this offset is inside the
    /// image and never aliases the file head.
    row_pos: usize,
    /// Complete preceding word.
    pub(crate) lead_raw: u32,
    /// Complete stored tag word.
    pub(crate) tag_raw: u32,
    /// Single-byte row flag.
    pub(crate) flag: u8,
    /// Three-byte F1 field.
    pub(crate) f1: [u8; 3],
    /// First trailing fixed-width field.
    pub(crate) f2: u32,
    /// Second trailing fixed-width field.
    pub(crate) f3: u32,
    /// Group-allocation header immediately preceding this alias core.
    pub(crate) group: Option<AliasGroupMembership>,
}

impl SurfaceAlias {
    /// Classification of the stored alias lead word.
    fn lead(&self) -> AliasLead {
        AliasLead::from_raw(self.lead_raw)
    }
    /// Low 24 bits of the stored tag word.
    fn tag(&self) -> u32 {
        self.tag_raw & 0x00ff_ffff
    }
    /// Entity-table ordinal from the F1 field.
    #[cfg(test)]
    fn entity_record_ordinal(&self) -> u8 {
        self.f1[2]
    }
}

/// Decode fixed surface-alias row cores from an outer body.
pub(crate) fn surface_aliases(
    ctx: &DecodeContext<'_>,
    data: &[u8],
) -> Result<Vec<SurfaceAlias>, CodecError> {
    const MARKER: [u8; 4] = [0x01, 0x00, 0x04, 0x00];
    let mut aliases = Vec::new();
    for (pos, bytes) in data.windows(MARKER.len()).enumerate() {
        if bytes != MARKER {
            continue;
        }
        let Some(row) = pos.checked_sub(alias_row::MARKER) else {
            continue;
        };
        let Some(tag_raw) = View::u32_le_at(data, row + alias_row::TAG) else {
            continue;
        };
        if row + alias_row::LEN > data.len() {
            continue;
        }
        let Some(lead_raw) = View::u32_le_at(data, row + alias_row::LEAD) else {
            continue;
        };
        let group = alias_group_membership(ctx, data, pos)?;
        if lead_raw & 0xff != 1 && !matches!(lead_raw, 0x8e | 0x8f | 0x0000_0133) && group.is_none()
        {
            continue;
        }
        let f1 = [
            data[row + alias_row::F1],
            data[row + alias_row::F1 + 1],
            data[row + alias_row::F1 + 2],
        ];
        let (Some(f2), Some(f3)) = (
            View::u32_le_at(data, row + alias_row::F2),
            View::u32_le_at(data, row + alias_row::F3),
        ) else {
            continue;
        };
        crate::resource::push(
            ctx,
            &mut aliases,
            SurfaceAlias {
                pos,
                row_pos: row,
                lead_raw,
                tag_raw,
                flag: data[row + alias_row::FLAG],
                f1,
                f2,
                f3,
                group,
            },
            "catia_surface_aliases",
        )?;
    }
    Ok(aliases)
}

/// Resolve the outer persistent-surface alias closure for geometry routes.
///
/// The returned map contains only alias cores outside complete object graphs,
/// value blocks, and catalogs. A `None` value means that the raw tag exists but
/// its canonical target is not unique.
pub(crate) fn surface_alias_tag_map(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
) -> Result<HashMap<u32, Option<u32>>, cadmpeg_core::CodecError> {
    let paired_object_graph_roots = entity_table::paired_object_graph_roots(ctx, data)?;
    let mut object_graphs = parse_all_with_paired_roots(ctx, data, &paired_object_graph_roots)?;
    let mut value_blocks = value_block::parse(ctx, data)?;
    value_blocks.retain(|block| {
        !object_graphs
            .iter()
            .any(|graph| extent_contains(graph.pos, graph.total_len, block.pos, block.total_len()))
    });
    object_graphs.retain(|graph| {
        !value_blocks
            .iter()
            .any(|block| extent_contains(block.pos, block.total_len(), graph.pos, graph.total_len))
    });
    let catalogs = crate::resource::collect_vec(
        ctx,
        catalog::parse(ctx, data)?.into_iter().filter(|catalog| {
            !object_graphs.iter().any(|graph| {
                extent_contains(graph.pos, graph.total_len, catalog.pos, catalog.total_len)
            }) && !value_blocks.iter().any(|block| {
                extent_contains(block.pos, block.total_len(), catalog.pos, catalog.total_len)
            })
        }),
        "catia_alias_filtered_catalogs",
    )?;

    let mut rows = surface_aliases(ctx, data)?;
    rows.retain(|row| {
        let row_start = row.row_pos;
        !object_graphs
            .iter()
            .any(|graph| extents_overlap(row_start, 24, graph.pos, graph.total_len))
            && !value_blocks
                .iter()
                .any(|block| extents_overlap(row_start, 24, block.pos, block.total_len()))
            && !catalogs
                .iter()
                .any(|catalog| extents_overlap(row_start, 24, catalog.pos, catalog.total_len))
    });

    let mut stored_by_group = HashMap::<(u32, u32), Option<u32>>::new();
    for row in &rows {
        let Some(group) = row.group.as_ref() else {
            continue;
        };
        if row.lead() != AliasLead::SurfaceSupportStorage {
            continue;
        }
        crate::resource::admit_map_entry(
            ctx,
            &mut stored_by_group,
            &(group.prototype, group.group_id),
            "catia_alias_stored_groups",
        )?;
        stored_by_group
            .entry((group.prototype, group.group_id))
            .and_modify(|stored| *stored = None)
            .or_insert(Some(row.tag()));
    }

    let mut tags = HashMap::<u32, Option<u32>>::new();
    for row in rows {
        let canonical = match row.lead() {
            AliasLead::SurfaceSupportStorage => Some(row.tag()),
            AliasLead::NonSurfaceAlias => row.group.as_ref().and_then(|group| {
                stored_by_group
                    .get(&(group.prototype, group.group_id))
                    .copied()
                    .flatten()
            }),
            _ => None,
        };
        crate::resource::admit_map_entry(ctx, &mut tags, &row.tag(), "catia_alias_tags")?;
        tags.entry(row.tag())
            .and_modify(|stored| *stored = None)
            .or_insert(canonical);
    }
    Ok(tags)
}

pub(crate) fn extent_contains(
    owner_start: usize,
    owner_len: usize,
    candidate_start: usize,
    candidate_len: usize,
) -> bool {
    owner_start < candidate_start
        && owner_start
            .checked_add(owner_len)
            .zip(candidate_start.checked_add(candidate_len))
            .is_some_and(|(owner_end, candidate_end)| candidate_end <= owner_end)
}

fn alias_group_membership(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    marker: usize,
) -> Result<Option<AliasGroupMembership>, CodecError> {
    let mut candidate = None;
    for storage_len in [3usize, 4, 7, 8] {
        let Some(start) = marker.checked_sub(20 + storage_len) else {
            continue;
        };
        let Some(storage) = data.get(start + 20..marker) else {
            continue;
        };
        if data.get(start..start + 2) == Some(&[0x02, 0x00])
            && data.get(start + 10..start + 13) == Some(&[0x00, 0x05, 0x00])
            && data.get(start + 13..start + 17) == Some(&[0x01, 0x00, 0x00, 0x00])
            && data.get(start + 17..start + 20) == Some(&[0x30, 0x00, 0x00])
            && is_alias_group_storage_prefix(storage)
        {
            if candidate.is_some() {
                return Ok(None);
            }
            candidate = Some((start, storage));
        }
    }
    let Some((start, storage)) = candidate else {
        return Ok(None);
    };
    let (Some(prototype), Some(group_id), Some(target_slot)) = (
        View::u32_le_at(data, start + 2),
        View::u32_le_at(data, start + 6),
        View::u32_le_at(data, marker + 11),
    ) else {
        return Ok(None);
    };
    Ok(Some(AliasGroupMembership {
        prototype,
        group_id,
        target_slot,
        storage_prefix: crate::resource::copy_retained_slice(
            ctx,
            storage,
            "catia_alias_group_storage",
        )?,
    }))
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
pub(crate) fn parse(
    ctx: &DecodeContext<'_>,
    data: &[u8],
) -> Result<Option<ObjectGraph>, CodecError> {
    Ok(parse_all(ctx, data)?
        .into_iter()
        .max_by_key(|graph| graph.records.len()))
}

/// Parse every length-closed `7C08` object graph in source order.
pub(crate) fn parse_all(
    ctx: &DecodeContext<'_>,
    data: &[u8],
) -> Result<Vec<ObjectGraph>, CodecError> {
    parse_all_with_paired_roots(ctx, data, &std::collections::HashMap::new())
}

/// Parse every length-closed object graph, admitting opaque childless records
/// only when a preceding entity-table run selects the exact root and record
/// cardinality.
pub(crate) fn parse_all_with_paired_roots(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    paired_roots: &std::collections::HashMap<usize, usize>,
) -> Result<Vec<ObjectGraph>, CodecError> {
    let catalogs = catalog::parse(ctx, data)?;
    let value_blocks = value_block::parse(ctx, data)?;
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
        let graph = if let Some(graph) = parse_candidate(ctx, data, pos, false)? {
            Some(graph)
        } else if let Some(expected_count) = paired_roots.get(&pos) {
            parse_candidate(ctx, data, pos, true)?
                .filter(|graph| graph.records.len() == *expected_count)
        } else {
            None
        };
        let Some(graph) = graph else {
            continue;
        };
        if let Some(graph_end) = graph.pos.checked_add(graph.total_len) {
            enclosing_end = enclosing_end.max(graph_end);
        }
        crate::resource::push(ctx, &mut roots, graph, "catia_object_graph_roots")?;
    }
    for graph in &mut roots {
        bind_catalog(graph, &catalogs, &value_blocks);
    }
    Ok(roots)
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
                .and_then(|block| block.pos.checked_add(block.total_len()))
                .and_then(|value_end| catalogs.iter().find(|schema| schema.pos == value_end))
        });
    let Some(schema) = schema else {
        return;
    };
    graph.catalog_pos = Some(schema.pos);
}

fn parse_candidate(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    pos: usize,
    allow_opaque_childless_records: bool,
) -> Result<Option<ObjectGraph>, CodecError> {
    (|| -> Option<Result<ObjectGraph, CodecError>> {
        macro_rules! admitted {
            ($result:expr) => {
                match $result {
                    Ok(value) => value,
                    Err(error) => return Some(Err(error)),
                }
            };
        }
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
            let (lead, body_form) = match child {
                Some((child, _)) => {
                    let head_bytes = data.get(head_start..child)?;
                    let lead = *head_bytes.first()?;
                    let head = admitted!(decode_head(ctx, head_bytes));
                    let payload = admitted!(decode_payload(ctx, &data[child + 6..record_end]))?;
                    (lead, ObjectRecordBody::Nested { head, payload })
                }
                None if is_inline_body(body) => {
                    let lead = body[0];
                    (
                        lead,
                        ObjectRecordBody::Inline(admitted!(crate::resource::copy_retained_slice(
                            ctx,
                            body,
                            "catia_object_inline_body"
                        ))),
                    )
                }
                None if allow_opaque_childless_records && !body.is_empty() => {
                    let lead = body[0];
                    (
                        lead,
                        ObjectRecordBody::Inline(admitted!(crate::resource::copy_retained_slice(
                            ctx,
                            body,
                            "catia_object_inline_body"
                        ))),
                    )
                }
                None => return None,
            };
            admitted!(crate::resource::push(
                ctx,
                &mut records,
                ObjectRecord {
                    pos: at,
                    total_len: record_len,
                    lead,
                    body: body_form,
                },
                "catia_object_records"
            ));
            at = record_end;
        }
        (!records.is_empty() && at == end).then_some(Ok(ObjectGraph {
            pos,
            total_len,
            catalog_pos: None,
            records,
        }))
    })()
    .transpose()
}

/// Occupant of the object head owner slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HeadOwner {
    Entity(u32),
    UnassignedLiteral(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HeadRoles {
    pub(crate) owner: Option<HeadOwner>,
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
    let role_owner = |index: Option<usize>| match index.and_then(|index| head.get(index)) {
        Some(HeadToken::Reference(value)) => Some(HeadOwner::Entity(*value)),
        Some(HeadToken::Literal(value)) => Some(HeadOwner::UnassignedLiteral(*value)),
        _ => None,
    };
    if class_first {
        let class_ref = role_reference(class_index);
        let storage_ref = class_ref.and_then(|_| role_reference(storage_index));
        let owner = storage_ref.and_then(|_| role_owner(owner_index));
        HeadRoles {
            owner,
            class_ref,
            storage_ref,
        }
    } else {
        let owner_ref = role_reference(owner_index);
        let class_ref = owner_ref.and_then(|_| role_reference(class_index));
        let storage_ref = class_ref.and_then(|_| role_reference(storage_index));
        HeadRoles {
            owner: owner_ref.map(HeadOwner::Entity),
            class_ref,
            storage_ref,
        }
    }
}

fn extended_compact_role_count(head: &[HeadToken]) -> Option<usize> {
    let head = if matches!(head.first(), Some(HeadToken::Lead(0x56))) {
        if !matches!(head.last(), Some(HeadToken::Reference(3))) {
            return None;
        }
        &head[..head.len() - 1]
    } else {
        head
    };
    if matches!(
        head,
        [
            HeadToken::Lead(0x16 | 0x56),
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
            HeadToken::Lead(0x16 | 0x56),
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
            HeadToken::Lead(0x16 | 0x56),
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
            HeadToken::Lead(0x16 | 0x56),
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
            HeadToken::Lead(0x16 | 0x56),
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
            HeadToken::Lead(0x16 | 0x56),
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

struct RepeatedReferenceSuffixView<'a> {
    repeated: &'a [PayloadField],
    terminal_reference: u32,
    first_count_offset: usize,
    repeated_count_offset: usize,
    schema_preamble: Option<ReferenceSchemaPreamble>,
}

fn repeated_reference_suffix_view(
    payload: &ObjectPayload,
) -> Option<RepeatedReferenceSuffixView<'_>> {
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
            let first = fields.get(references_start..references_end)?;
            if !first
                .iter()
                .all(|field| matches!(field, PayloadField::Reference { .. }))
            {
                return None;
            }
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
            let repeated = fields.get(repeated_start..repeated_end)?;
            if !first[..count - 1]
                .iter()
                .zip(repeated)
                .all(|(left, right)| {
                    matches!((left, right),
                    (PayloadField::Reference { value: left, .. },
                     PayloadField::Reference { value: right, .. }) if left == right)
                })
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
            let terminal_reference = match first.last()? {
                PayloadField::Reference { value, .. } => *value,
                _ => return None,
            };
            Some(RepeatedReferenceSuffixView {
                schema_preamble: reference_schema_preamble(&fields[..count_index - 1]),
                repeated,
                terminal_reference,
                first_count_offset: *first_count_offset,
                repeated_count_offset: *repeated_count_offset,
            })
        });
    let suffix = matches.next()?;
    matches.next().is_none().then_some(suffix)
}

pub(crate) fn has_repeated_reference_suffix(payload: &ObjectPayload) -> bool {
    repeated_reference_suffix_view(payload).is_some()
}

pub(crate) fn repeated_reference_schema_preamble(
    payload: &ObjectPayload,
) -> Option<ReferenceSchemaPreamble> {
    repeated_reference_suffix_view(payload)?.schema_preamble
}

pub(crate) fn repeated_reference_suffix(
    payload: &ObjectPayload,
) -> Option<RepeatedReferenceSuffix> {
    let view = repeated_reference_suffix_view(payload)?;
    Some(RepeatedReferenceSuffix {
        schema_preamble: view.schema_preamble,
        repeated_references: view
            .repeated
            .iter()
            .filter_map(|field| match field {
                PayloadField::Reference { value, .. } => Some(*value),
                _ => None,
            })
            .collect(),
        terminal_reference: view.terminal_reference,
        first_count_offset: view.first_count_offset,
        repeated_count_offset: view.repeated_count_offset,
    })
}

pub(crate) fn repeated_reference_suffix_charged(
    ctx: &DecodeContext<'_>,
    payload: &ObjectPayload,
) -> Result<Option<RepeatedReferenceSuffix>, CodecError> {
    let Some(view) = repeated_reference_suffix_view(payload) else {
        return Ok(None);
    };
    let repeated_references = crate::resource::collect_vec(
        ctx,
        view.repeated.iter().filter_map(|field| match field {
            PayloadField::Reference { value, .. } => Some(*value),
            _ => None,
        }),
        "catia_native_repeated_reference_suffix",
    )?;
    Ok(Some(RepeatedReferenceSuffix {
        schema_preamble: view.schema_preamble,
        repeated_references,
        terminal_reference: view.terminal_reference,
        first_count_offset: view.first_count_offset,
        repeated_count_offset: view.repeated_count_offset,
    }))
}

fn reference_schema_preamble(fields: &[PayloadField]) -> Option<ReferenceSchemaPreamble> {
    let mut matches = fields.windows(4).filter_map(|fields| match fields {
        [
            PayloadField::Blob { bytes, .. },
            PayloadField::Atom { value: 5, .. },
            PayloadField::Atom { value: 46, .. },
            PayloadField::Atom {
                value: schema_ref,
                offset,
            },
        ] if bytes.len() == 59 => Some(ReferenceSchemaPreamble::BlobThenSchema {
            schema_ref: *schema_ref,
            offset: *offset,
        }),
        [
            PayloadField::Atom {
                value: schema_ref,
                offset,
            },
            PayloadField::Atom { value: 34, .. },
            PayloadField::Blob { bytes, .. },
            PayloadField::Atom { value: 5, .. },
        ] if bytes.len() == 59 => Some(ReferenceSchemaPreamble::SchemaThenBlob {
            schema_ref: *schema_ref,
            offset: *offset,
        }),
        _ => None,
    });
    let preamble = matches.next()?;
    matches.next().is_none().then_some(preamble)
}

fn decode_head(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<Vec<HeadToken>, CodecError> {
    let Some(&lead) = bytes.first() else {
        return Ok(Vec::new());
    };
    let mut tokens = Vec::new();
    crate::resource::push(
        ctx,
        &mut tokens,
        HeadToken::Lead(lead),
        "catia_object_head_tokens",
    )?;
    let mut at = 1;
    while at < bytes.len() {
        let byte = bytes[at];
        if byte == 0x01 {
            crate::resource::push(
                ctx,
                &mut tokens,
                HeadToken::Separator,
                "catia_object_head_tokens",
            )?;
            at += 1;
        } else if bytes.get(at..at + 4) == Some(&[0xff; 4]) {
            crate::resource::push(
                ctx,
                &mut tokens,
                HeadToken::NullHandle,
                "catia_object_head_tokens",
            )?;
            at += 4;
        } else if (0xd1..=0xe4).contains(&byte) && at + 1 < bytes.len() {
            crate::resource::push(
                ctx,
                &mut tokens,
                HeadToken::Reference(u32::from(byte - 0xd1) * 256 + u32::from(bytes[at + 1]) + 1),
                "catia_object_head_tokens",
            )?;
            at += 2;
        } else if (0x80..=0xd0).contains(&byte) {
            crate::resource::push(
                ctx,
                &mut tokens,
                HeadToken::Reference(u32::from(byte - 0x80)),
                "catia_object_head_tokens",
            )?;
            at += 1;
        } else {
            crate::resource::push(
                ctx,
                &mut tokens,
                HeadToken::Literal(byte),
                "catia_object_head_tokens",
            )?;
            at += 1;
        }
    }
    Ok(tokens)
}

fn atom(bytes: &[u8], at: usize) -> Option<(u32, usize)> {
    let byte = *bytes.get(at)?;
    match byte {
        0x80..=0xd0 => Some((u32::from(byte - 0x80), 1)),
        0x51..=0x7f => Some((u32::from(byte), 1)),
        // A payload atom must leave the framing terminator after its data.
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

fn decode_payload(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<ObjectPayload>, CodecError> {
    (|| -> Option<Result<ObjectPayload, CodecError>> {
        macro_rules! admitted {
            ($result:expr) => {
                match $result {
                    Ok(value) => value,
                    Err(error) => return Some(Err(error)),
                }
            };
        }
        let mut fields = Vec::new();
        let mut at = 0;
        while at < bytes.len() {
            let offset = at;
            if bytes[at] == 0xe5 {
                if let Some(end) = blob_end(bytes, at) {
                    admitted!(crate::resource::push(
                        ctx,
                        &mut fields,
                        PayloadField::Blob {
                            bytes: admitted!(crate::resource::copy_retained_slice(
                                ctx,
                                &bytes[at + 5..end],
                                "catia_object_payload_blob"
                            )),
                            offset,
                        },
                        "catia_object_payload_fields"
                    ));
                    at = end;
                    continue;
                }
                if blob_declared_end(bytes, at) == Some(bytes.len()) {
                    return None;
                }
            }
            if matches!(bytes[at], 0x80 | 0x32) {
                if let Some(value) = bytes
                    .get(at + 5)
                    .and_then(|_| View::u32_le_at(bytes, at + 1))
                {
                    admitted!(crate::resource::push(
                        ctx,
                        &mut fields,
                        if bytes[at] == 0x80 {
                            PayloadField::Atom { value, offset }
                        } else {
                            PayloadField::Reference { value, offset }
                        },
                        "catia_object_payload_fields"
                    ));
                    at += 5;
                    continue;
                }
            }
            match bytes[at] {
                0xfe if is_final_terminator_run(bytes, at) => {
                    while bytes.get(at) == Some(&0xfe) {
                        admitted!(crate::resource::push(
                            ctx,
                            &mut fields,
                            PayloadField::Terminator,
                            "catia_object_payload_fields"
                        ));
                        at += 1;
                    }
                    break;
                }
                0x3c => {
                    let Some((count, advance)) = atom(bytes, at + 1) else {
                        admitted!(crate::resource::push(
                            ctx,
                            &mut fields,
                            PayloadField::Atom {
                                value: 0x3c,
                                offset,
                            },
                            "catia_object_payload_fields"
                        ));
                        at += 1;
                        continue;
                    };
                    let table_at = at + 1 + advance;
                    let Some(table_count) = View::u32_le_at(bytes, table_at) else {
                        admitted!(crate::resource::push(
                            ctx,
                            &mut fields,
                            PayloadField::Atom {
                                value: 0x3c,
                                offset,
                            },
                            "catia_object_payload_fields"
                        ));
                        at += 1;
                        continue;
                    };
                    let table_end = table_at.checked_add(4)?;
                    if usize::try_from(table_count)
                        .ok()
                        .is_some_and(|count| count <= bytes.len() - table_end)
                    {
                        let (rows, end) =
                            admitted!(parse_bulk_table_rows(ctx, bytes, table_end, table_count))?;
                        admitted!(crate::resource::push(
                            ctx,
                            &mut fields,
                            PayloadField::BulkTable {
                                count,
                                rows,
                                offset,
                            },
                            "catia_object_payload_fields"
                        ));
                        at = end;
                        continue;
                    }
                    admitted!(crate::resource::push(
                        ctx,
                        &mut fields,
                        PayloadField::Atom {
                            value: 0x3c,
                            offset,
                        },
                        "catia_object_payload_fields"
                    ));
                    at += 1;
                }
                0x3b => {
                    if is_final_terminator_run(bytes, at + 1) {
                        admitted!(crate::resource::push(
                            ctx,
                            &mut fields,
                            PayloadField::Atom {
                                value: 0x3b,
                                offset,
                            },
                            "catia_object_payload_fields"
                        ));
                        at += 1;
                        continue;
                    }
                    let Some((declared_count, advance)) = atom(bytes, at + 1) else {
                        admitted!(crate::resource::push(
                            ctx,
                            &mut fields,
                            PayloadField::Atom {
                                value: 0x3b,
                                offset,
                            },
                            "catia_object_payload_fields"
                        ));
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
                        admitted!(crate::resource::push(
                            ctx,
                            &mut items,
                            if tagged_reference || fixed_reference {
                                ListItem::Reference {
                                    value,
                                    offset: item_offset,
                                }
                            } else {
                                ListItem::Atom {
                                    value,
                                    offset: item_offset,
                                }
                            },
                            "catia_object_list_items"
                        ));
                        at = value_at + consumed;
                    }
                    admitted!(crate::resource::push(
                        ctx,
                        &mut fields,
                        PayloadField::List {
                            declared_count,
                            items,
                            offset,
                        },
                        "catia_object_payload_fields"
                    ));
                }
                0x81 | 0x3a | 0x39 | 0x7a => {
                    let tag = bytes[at];
                    if is_final_terminator_run(bytes, at + 1) {
                        admitted!(crate::resource::push(
                            ctx,
                            &mut fields,
                            PayloadField::Atom {
                                value: u32::from(tag),
                                offset,
                            },
                            "catia_object_payload_fields"
                        ));
                        at += 1;
                        continue;
                    }
                    let Some((value, consumed)) = tagged_value(bytes, at + 1) else {
                        admitted!(crate::resource::push(
                            ctx,
                            &mut fields,
                            PayloadField::Atom {
                                value: u32::from(tag),
                                offset,
                            },
                            "catia_object_payload_fields"
                        ));
                        at += 1;
                        continue;
                    };
                    admitted!(crate::resource::push(
                        ctx,
                        &mut fields,
                        match tag {
                            0x81 => PayloadField::Reference { value, offset },
                            _ => PayloadField::Scalar { tag, value, offset },
                        },
                        "catia_object_payload_fields"
                    ));
                    at += 1 + consumed;
                }
                0x0d => {
                    admitted!(crate::resource::push(
                        ctx,
                        &mut fields,
                        PayloadField::Sentinel { offset },
                        "catia_object_payload_fields"
                    ));
                    at += 1;
                }
                _ => {
                    let (value, consumed) = atom(bytes, at).unwrap_or((u32::from(bytes[at]), 1));
                    admitted!(crate::resource::push(
                        ctx,
                        &mut fields,
                        PayloadField::Atom { value, offset },
                        "catia_object_payload_fields"
                    ));
                    at += consumed;
                }
            }
        }
        (at == bytes.len() && matches!(fields.last(), Some(PayloadField::Terminator))).then_some(
            Ok(ObjectPayload {
                size: bytes.len(),
                fields,
            }),
        )
    })()
    .transpose()
}

fn parse_bulk_table_rows(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    mut at: usize,
    table_count: u32,
) -> Result<Option<(Vec<BulkTableRow>, usize)>, CodecError> {
    (|| -> Option<Result<(Vec<BulkTableRow>, usize), CodecError>> {
        let count = usize::try_from(table_count).ok()?;
        let mut rows = Vec::new();
        if let Err(error) =
            crate::resource::reserve_vec(ctx, &mut rows, count, "catia_object_bulk_table_rows")
        {
            return Some(Err(error));
        }
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
        Some(Ok((rows, at)))
    })()
    .transpose()
}

fn bulk_row_id(bytes: &[u8], at: &mut usize) -> Option<u32> {
    let start = *at;
    let mut candidate = None;
    if let Some((value, consumed)) = atom(bytes, start) {
        let end = start.checked_add(consumed)?;
        if bytes.get(end) == Some(&0x80) && end.checked_add(5)? <= bytes.len() {
            candidate = Some((value, end));
        }
    }
    if bytes.get(start) == Some(&0x80) {
        let end = start.checked_add(5)?;
        if end.checked_add(5)? <= bytes.len() && bytes.get(end) == Some(&0x80) {
            if candidate.is_some() {
                return None;
            }
            candidate = Some((View::u32_le_at(bytes, start + 1)?, end));
        }
    }
    let (value, end) = candidate?;
    *at = end;
    Some(value)
}

fn blob_end(bytes: &[u8], at: usize) -> Option<usize> {
    let end = blob_declared_end(bytes, at)?;
    (end < bytes.len()).then_some(end)
}

fn blob_declared_end(bytes: &[u8], at: usize) -> Option<usize> {
    let declared_len = usize::try_from(View::u32_le_at(bytes, at + 1)?).ok()?;
    at.checked_add(5)?.checked_add(declared_len)
}

/// Structural classification of decoded payload fields.
pub(crate) fn classify(fields: &[PayloadField]) -> PayloadSubtype {
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
    use super::{
        repeated_reference_suffix, ObjectPayload, PayloadField, ReferenceSchemaPreamble,
        RepeatedReferenceSuffix,
    };

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
