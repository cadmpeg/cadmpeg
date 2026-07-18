// SPDX-License-Identifier: Apache-2.0
//! Outer `7C08` feature and object-ownership graph decoder.
#![deny(clippy::disallowed_methods)]

use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::decode::{DecodeContext, View};
use cadmpeg_ir::le::u32_at as u32_le;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One decoded outer object graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectGraph {
    /// Offset of the selected `7C08` root.
    pub pos: usize,
    /// Root total length, including its six-byte header.
    pub total_len: usize,
    /// Consecutive nested `7C09` records.
    pub records: Vec<ObjectRecord>,
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
    /// First head reference, identifying the owner.
    pub owner_ref: Option<u32>,
    /// Second head reference, identifying the per-file class.
    pub class_ref: Option<u32>,
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
    /// Literal byte below `0x80`.
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
    Reference(u32),
    /// Untagged atom value.
    Atom(u32),
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
    /// `0x81` reference field.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AliasLead {
    /// Low byte `0x01`: ordinary surface-support storage.
    SurfaceSupportStorage,
    /// Exact value `0x8e`: E5-linked surface storage.
    E5LinkedSurfaceStorage,
    /// Zero lead: alias-like row outside surface storage.
    NonSurfaceAlias,
    /// Other lead value whose role is not assigned.
    Unclassified(u32),
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
///
/// The whole-image marker scan is charged once as `work`; each retained context
/// window is charged against `retained_bytes` before it is copied.
pub fn markers_7cd9<'a>(
    ctx: &DecodeContext<'a>,
    view: View<'a>,
    context_len: usize,
) -> Result<Vec<Marker7cd9>, CodecError> {
    let data = view.window();
    ctx.charge_work(
        data.len() as u64,
        "catia_marker_7cd9_scan",
        Some(view.location()),
    )?;
    let mut positions = ctx.grow_vec::<usize>();
    for (pos, bytes) in data.windows(2).enumerate() {
        if bytes == [0x7c, 0xd9] {
            positions.try_push(pos)?;
        }
    }
    let positions = positions.finish();
    let mut markers = ctx.grow_vec::<Marker7cd9>();
    for (index, &pos) in positions.iter().enumerate() {
        let end = pos.saturating_add(context_len).min(data.len());
        let context_bytes = end - pos;
        ctx.charge_retained(
            context_bytes as u64,
            "catia_marker_7cd9_context",
            Some(view.location()),
        )?;
        markers.try_push(Marker7cd9 {
            pos,
            context: data[pos..end].to_vec(),
            next_delta: positions.get(index + 1).map(|next| next - pos),
        })?;
    }
    Ok(markers.finish())
}

/// Decode fixed surface-alias row cores from an outer body.
///
/// The whole-image marker scan is charged once as `work`; each admitted row is
/// a fixed-width read into a `grow_vec` accumulator, so no untrusted count sizes
/// an allocation.
pub fn surface_aliases<'a>(
    ctx: &DecodeContext<'a>,
    view: View<'a>,
) -> Result<Vec<SurfaceAlias>, CodecError> {
    const MARKER: [u8; 4] = [0x01, 0x00, 0x04, 0x00];
    let data = view.window();
    ctx.charge_work(
        data.len() as u64,
        "catia_surface_alias_scan",
        Some(view.location()),
    )?;
    let mut rows = ctx.grow_vec::<SurfaceAlias>();
    for pos in 0..data.len().saturating_sub(MARKER.len() - 1) {
        if data[pos..pos + MARKER.len()] != MARKER {
            continue;
        }
        if let Some(row) = alias_at(data, pos) {
            rows.try_push(row)?;
        }
    }
    Ok(rows.finish())
}

fn alias_at(data: &[u8], pos: usize) -> Option<SurfaceAlias> {
    let tag_raw = u32_le(data, pos + 4)?;
    let tag = tag_raw & 0x00ff_ffff;
    if tag == 0 || pos + 20 > data.len() {
        return None;
    }
    let lead_raw = pos
        .checked_sub(4)
        .and_then(|at| u32_le(data, at))
        .unwrap_or(0);
    let lead = if lead_raw & 0xff == 1 {
        AliasLead::SurfaceSupportStorage
    } else if lead_raw == 0x8e {
        AliasLead::E5LinkedSurfaceStorage
    } else if lead_raw == 0 {
        AliasLead::NonSurfaceAlias
    } else {
        AliasLead::Unclassified(lead_raw)
    };
    let f1 = [data[pos + 9], data[pos + 10], data[pos + 11]];
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
    })
}

/// Parse the valid `7C08` candidate containing the most `7C09` records.
///
/// The whole-image `7C08` marker scan is charged once as `work`; each candidate
/// charges its framed extent as the bytes its nested record walk examines, and
/// records/heads/fields grow through charged accumulators.
pub fn parse<'a>(
    ctx: &DecodeContext<'a>,
    view: View<'a>,
) -> Result<Option<ObjectGraph>, CodecError> {
    let data = view.window();
    ctx.charge_work(
        data.len() as u64,
        "catia_object_graph_scan",
        Some(view.location()),
    )?;
    let mut best: Option<ObjectGraph> = None;
    for pos in 0..data.len().saturating_sub(1) {
        if data[pos..pos + 2] != [0x7c, 0x08] {
            continue;
        }
        if let Some(candidate) = parse_candidate(ctx, view, data, pos)? {
            // Equal record counts select the later candidate.
            if best
                .as_ref()
                .is_none_or(|graph| candidate.records.len() >= graph.records.len())
            {
                best = Some(candidate);
            }
        }
    }
    Ok(best)
}

fn parse_candidate<'a>(
    ctx: &DecodeContext<'a>,
    view: View<'a>,
    data: &[u8],
    pos: usize,
) -> Result<Option<ObjectGraph>, CodecError> {
    let Some(total_len) = u32_le(data, pos + 2).and_then(|len| usize::try_from(len).ok()) else {
        return Ok(None);
    };
    let Some(end) = pos.checked_add(total_len) else {
        return Ok(None);
    };
    if total_len < 15 || end > data.len() {
        return Ok(None);
    }
    // The nested `7C09` record walk examines at most this candidate's framed
    // extent once; charge those bytes as work before entering it.
    ctx.charge_work(
        total_len as u64,
        "catia_object_graph_records",
        Some(view.location()),
    )?;
    let mut at = pos + 6;
    let mut records = ctx.grow_vec::<ObjectRecord>();
    while at + 6 <= end && data.get(at..at + 2) == Some(&[0x7c, 0x09]) {
        let Some(record_len) = u32_le(data, at + 2).and_then(|len| usize::try_from(len).ok())
        else {
            return Ok(None);
        };
        let Some(record_end) = at.checked_add(record_len) else {
            return Ok(None);
        };
        if record_len < 6 || record_end > end {
            return Ok(None);
        }
        let head_start = at + 6;
        let Some(child) = data[head_start..record_end]
            .windows(2)
            .position(|bytes| bytes == [0x7c, 0x0a])
            .map(|relative| head_start + relative)
        else {
            return Ok(None);
        };
        let Some(child_len) = u32_le(data, child + 2).and_then(|len| usize::try_from(len).ok())
        else {
            return Ok(None);
        };
        if child.checked_add(child_len) != Some(record_end) || child_len < 6 {
            return Ok(None);
        }
        let head = decode_head(ctx, &data[head_start..child])?;
        let references: Vec<u32> = head
            .iter()
            .filter_map(|token| match token {
                HeadToken::Reference(value) => Some(*value),
                _ => None,
            })
            .collect();
        let payload = decode_payload(ctx, &data[child + 6..record_end])?;
        let subtype = classify(&payload.fields);
        records.try_push(ObjectRecord {
            index: records.len(),
            pos: at,
            total_len: record_len,
            lead: data.get(head_start).copied().unwrap_or(0),
            head,
            owner_ref: references.first().copied(),
            class_ref: references.get(1).copied(),
            storage_ref: references.get(2).copied(),
            payload,
            subtype,
        })?;
        at = record_end;
    }
    let records = records.finish();
    Ok((records.len() >= 2).then_some(ObjectGraph {
        pos,
        total_len,
        records,
    }))
}

fn decode_head(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<Vec<HeadToken>, CodecError> {
    let mut tokens = ctx.grow_vec::<HeadToken>();
    let Some(&lead) = bytes.first() else {
        return Ok(tokens.finish());
    };
    tokens.try_push(HeadToken::Lead(lead))?;
    let mut at = 1;
    while at < bytes.len() {
        let byte = bytes[at];
        if byte == 0x01 {
            tokens.try_push(HeadToken::Separator)?;
            at += 1;
        } else if bytes.get(at..at + 4) == Some(&[0xff; 4]) {
            tokens.try_push(HeadToken::NullHandle)?;
            at += 4;
        } else if byte == 0x81 && at + 2 < bytes.len() {
            tokens.try_push(HeadToken::Reference(
                u32::from(bytes[at + 1].wrapping_sub(0x80)) * 128 + u32::from(bytes[at + 2]),
            ))?;
            at += 3;
        } else if byte >= 0x80 {
            tokens.try_push(HeadToken::Reference(u32::from(byte - 0x80)))?;
            at += 1;
        } else {
            tokens.try_push(HeadToken::Literal(byte))?;
            at += 1;
        }
    }
    Ok(tokens.finish())
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

fn decode_payload(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<ObjectPayload, CodecError> {
    let mut fields = ctx.grow_vec::<PayloadField>();
    let mut at = 0;
    while at < bytes.len() {
        let offset = at;
        match bytes[at] {
            0xfe => {
                fields.try_push(PayloadField::Terminator)?;
                break;
            }
            0xe5 if at + 5 <= bytes.len() => {
                let declared_len = usize::try_from(u32_le(bytes, at + 1).unwrap_or(0)).unwrap_or(0);
                let start = at + 5;
                let end = start.saturating_add(declared_len).min(bytes.len());
                // Raw-byte egress: the blob payload copy is charged as retained
                // bytes before it is taken.
                ctx.charge_retained((end - start) as u64, "catia_object_graph_blob", None)?;
                fields.try_push(PayloadField::Blob {
                    declared_len,
                    bytes: bytes[start..end].to_vec(),
                    offset,
                })?;
                at = end;
            }
            0x3c => {
                let Some((count, advance)) = atom(bytes, at + 1) else {
                    fields.try_push(PayloadField::Atom {
                        value: 0x3c,
                        offset,
                    })?;
                    at += 1;
                    continue;
                };
                let table_at = at + 1 + advance;
                let table_count = u32_le(bytes, table_at).unwrap_or(u32::MAX);
                if usize::try_from(table_count)
                    .ok()
                    .is_some_and(|count| count <= bytes.len())
                {
                    fields.try_push(PayloadField::BulkTable {
                        count,
                        table_count,
                        offset,
                    })?;
                    at = table_at + 4;
                } else {
                    fields.try_push(PayloadField::Atom {
                        value: 0x3c,
                        offset,
                    })?;
                    at += 1;
                }
            }
            0x3b => {
                let Some((declared_count, advance)) = atom(bytes, at + 1) else {
                    fields.try_push(PayloadField::Atom {
                        value: 0x3b,
                        offset,
                    })?;
                    at += 1;
                    continue;
                };
                at += 1 + advance;
                let mut items = ctx.grow_vec::<ListItem>();
                for _ in 0..declared_count {
                    if at >= bytes.len() {
                        break;
                    }
                    let tagged_reference = bytes[at] == 0x81;
                    let tagged_atom = bytes[at] == 0x80;
                    let value_at = at + usize::from(tagged_reference || tagged_atom);
                    let Some((value, consumed)) = atom(bytes, value_at) else {
                        break;
                    };
                    items.try_push(if tagged_reference {
                        ListItem::Reference(value)
                    } else {
                        ListItem::Atom(value)
                    })?;
                    at = value_at + consumed;
                }
                fields.try_push(PayloadField::List {
                    declared_count,
                    items: items.finish(),
                    offset,
                })?;
            }
            0x81 | 0x80 | 0x3a | 0x32 | 0x39 | 0x7a => {
                let tag = bytes[at];
                let Some((value, consumed)) = atom(bytes, at + 1) else {
                    fields.try_push(PayloadField::Atom {
                        value: u32::from(tag),
                        offset,
                    })?;
                    at += 1;
                    continue;
                };
                fields.try_push(match tag {
                    0x81 => PayloadField::Reference { value, offset },
                    0x80 => PayloadField::Atom { value, offset },
                    _ => PayloadField::Scalar { tag, value, offset },
                })?;
                at += 1 + consumed;
            }
            0x0d => {
                fields.try_push(PayloadField::Sentinel { offset })?;
                at += 1;
            }
            _ => {
                let (value, consumed) = atom(bytes, at).unwrap_or((u32::from(bytes[at]), 1));
                fields.try_push(PayloadField::Atom { value, offset })?;
                at += consumed;
            }
        }
    }
    Ok(ObjectPayload {
        size: bytes.len(),
        fields: fields.finish(),
    })
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
        || (fields.len() <= 3
            && fields
                .iter()
                .any(|field| matches!(field, PayloadField::Terminator)))
    {
        PayloadSubtype::Empty
    } else {
        PayloadSubtype::Mixed
    }
}
