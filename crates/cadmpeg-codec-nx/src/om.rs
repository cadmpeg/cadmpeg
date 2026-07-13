// SPDX-License-Identifier: Apache-2.0
//! Frame NX object-model entities using external boundary and identity arrays.

use std::collections::BTreeSet;

use cadmpeg_ir::le::u32_at;

/// One NX object-model entity with persistent object identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityRecord<'a> {
    /// NX object identifier paired with this boundary slot, when the section
    /// carries a fixed-width object-id table.
    pub object_id: Option<u32>,
    /// Absolute byte offset of the entity payload.
    pub offset: usize,
    /// Exactly bounded serialized entity payload.
    pub bytes: &'a [u8],
}

/// One length-framed NX object-model class definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDefinition<'a> {
    /// Absolute byte offset of the definition's length byte.
    pub offset: usize,
    /// Registered `UGS::` class name.
    pub name: &'a str,
    /// Declaration code following the name.
    pub trailing_code: u8,
}

/// One member declaration in an NX OM field registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDefinition<'a> {
    /// Offset of the declaration length byte.
    pub offset: usize,
    /// Registered `m_` member name.
    pub name: &'a str,
    /// Declaration code immediately following the name.
    pub trailing_code: u8,
}

/// One self-framed printable string value in an NX OM entity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringValue<'a> {
    /// Absolute byte offset of the `66 32 03` marker.
    pub offset: usize,
    /// Printable value bytes.
    pub value: &'a str,
}

/// Tagged NX OM cross-record reference family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceKind {
    /// `e0` marker followed by a 32-bit big-endian persistent handle.
    PersistentHandle,
    /// Four-byte word whose high nibble is `c` and low 28 bits are the value.
    Tagged28,
}

/// One tagged reference occurrence in an externally bounded OM record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReferenceValue {
    /// Absolute byte offset of the reference marker.
    pub offset: usize,
    /// Reference family.
    pub kind: ReferenceKind,
    /// Unsigned reference value without its marker/tag bits.
    pub value: u32,
}

/// Unit declared by an NX numeric-expression serialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpressionUnit {
    /// Canonical model length in millimeters.
    Millimeter,
    /// Angular value in degrees as serialized by NX.
    Degree,
}

/// One numeric expression decoded from an exactly bounded OM entity.
#[derive(Debug, Clone, PartialEq)]
pub struct NumericExpression<'a> {
    /// Persistent identity of the containing OM entity, when indexed.
    pub object_id: Option<u32>,
    /// Absolute byte offset of the expression text.
    pub offset: usize,
    /// NX parameter name.
    pub name: &'a str,
    /// Declared native unit.
    pub unit: ExpressionUnit,
    /// Finite serialized numeric value in the declared unit.
    pub value: f64,
}

/// One validated external entity-index/object-id-table pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedSection<'a> {
    /// Self-anchored base used by every entity-index offset.
    pub base: usize,
    /// Absolute offset of the entity-index array.
    pub entity_index_offset: usize,
    /// Absolute offset of the object-id table or offset-only identity metadata.
    pub object_id_table_offset: usize,
    /// Length-framed class definitions preceding the entity index.
    pub types: Vec<TypeDefinition<'a>>,
    /// Entity records following the reserved zero-offset slot.
    pub records: Vec<EntityRecord<'a>>,
}

/// One size-framed NX object-model section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section<'a> {
    /// Offset of the `ff ff ff ff` section signature.
    pub offset: usize,
    /// Complete section length including its 16-byte header.
    pub byte_len: usize,
    /// Class declarations in the section's contiguous type registry.
    pub types: Vec<TypeDefinition<'a>>,
    /// Member declarations in the section's field registry.
    pub fields: Vec<FieldDefinition<'a>>,
}

impl<'a> IndexedSection<'a> {
    /// Decode explicit numeric-expression text within bounded entity records.
    pub fn numeric_expressions(&self) -> Vec<NumericExpression<'a>> {
        self.numeric_expression_records()
            .into_iter()
            .map(|(_, expression)| expression)
            .collect()
    }

    /// Decode expressions together with their owning record ordinal.
    pub fn numeric_expression_records(&self) -> Vec<(usize, NumericExpression<'a>)> {
        if !self.records.iter().any(|record| {
            record
                .bytes
                .windows(b"hostglobalvariables".len())
                .any(|window| window == b"hostglobalvariables")
        }) {
            return Vec::new();
        }
        self.records
            .iter()
            .enumerate()
            .filter_map(|(record_ordinal, record)| {
                numeric_expression_at(record.bytes, record.offset, record.object_id)
                    .map(|expression| (record_ordinal, expression))
            })
            .collect()
    }

    /// Decode every strictly framed printable string in each bounded record.
    pub fn string_values(&self) -> Vec<(usize, usize, Option<u32>, StringValue<'a>)> {
        self.records
            .iter()
            .enumerate()
            .flat_map(|(record_ordinal, record)| {
                string_values(record.bytes, record.offset)
                    .into_iter()
                    .enumerate()
                    .map(move |(value_ordinal, value)| {
                        (record_ordinal, value_ordinal, record.object_id, value)
                    })
            })
            .collect()
    }

    /// Decode tagged cross-record references from every bounded record.
    pub fn references(&self) -> Vec<(usize, usize, Option<u32>, ReferenceValue)> {
        self.records
            .iter()
            .enumerate()
            .flat_map(|(record_ordinal, record)| {
                record_references(record.bytes, record.offset)
                    .into_iter()
                    .enumerate()
                    .map(move |(reference_ordinal, reference)| {
                        (
                            record_ordinal,
                            reference_ordinal,
                            record.object_id,
                            reference,
                        )
                    })
            })
            .collect()
    }
}

/// Decode self-identifying persistent handles plus context-gated tagged refs.
pub fn record_references(bytes: &[u8], base_offset: usize) -> Vec<ReferenceValue> {
    let mut out = references(bytes, base_offset)
        .into_iter()
        .filter(|reference| reference.kind == ReferenceKind::PersistentHandle)
        .collect::<Vec<_>>();
    out.extend(
        dense_reference_suffix(bytes, base_offset)
            .into_iter()
            .filter(|reference| reference.kind == ReferenceKind::Tagged28),
    );
    out.sort_by_key(|reference| reference.offset);
    out
}

/// Decode tagged references wholly contained in `bytes`.
pub fn references(bytes: &[u8], base_offset: usize) -> Vec<ReferenceValue> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < bytes.len() {
        if bytes[at] == 0xe0 {
            if let Some(raw) = bytes
                .get(at + 1..at + 5)
                .and_then(|raw| raw.try_into().ok())
            {
                out.push(ReferenceValue {
                    offset: base_offset + at,
                    kind: ReferenceKind::PersistentHandle,
                    value: u32::from_be_bytes(raw),
                });
                at += 5;
                continue;
            }
        } else if bytes[at] & 0xf0 == 0xc0 {
            if let Some(raw) = bytes.get(at..at + 4).and_then(|raw| raw.try_into().ok()) {
                out.push(ReferenceValue {
                    offset: base_offset + at,
                    kind: ReferenceKind::Tagged28,
                    value: u32::from_be_bytes(raw) & 0x0fff_ffff,
                });
                at += 4;
                continue;
            }
        }
        at += 1;
    }
    out
}

/// Decode a dense tagged-reference suffix from one bounded OM record.
///
/// Sparse marker-shaped words can be ordinary per-class field data. A suffix
/// is a reference stream only when it contains at least eight persistent
/// handles and complete reference tokens cover at least 90% of its bytes.
pub fn dense_reference_suffix(bytes: &[u8], base_offset: usize) -> Vec<ReferenceValue> {
    let references = references(bytes, 0);
    for (index, first) in references.iter().enumerate() {
        let suffix = &references[index..];
        let persistent = suffix
            .iter()
            .filter(|reference| reference.kind == ReferenceKind::PersistentHandle)
            .count();
        if persistent < 8 {
            continue;
        }
        let covered = suffix
            .iter()
            .map(|reference| match reference.kind {
                ReferenceKind::PersistentHandle => 5,
                ReferenceKind::Tagged28 => 4,
            })
            .sum::<usize>();
        let span = bytes.len().saturating_sub(first.offset);
        if covered * 10 >= span * 9 {
            return suffix
                .iter()
                .map(|reference| ReferenceValue {
                    offset: base_offset + reference.offset,
                    ..*reference
                })
                .collect();
        }
    }
    Vec::new()
}

/// Decode `66 32 03` printable-string values wholly contained in `bytes`.
pub fn string_values(bytes: &[u8], base_offset: usize) -> Vec<StringValue<'_>> {
    const MARKER: &[u8] = &[0x66, 0x32, 0x03];
    bytes
        .windows(MARKER.len())
        .enumerate()
        .filter(|(_, window)| *window == MARKER)
        .filter_map(|(offset, _)| {
            let declared = usize::from(*bytes.get(offset + 3)?);
            let text_len = declared.checked_sub(2)?;
            let start = offset.checked_add(4)?;
            let end = start.checked_add(text_len)?;
            let raw = bytes.get(start..end)?;
            (!raw.is_empty()
                && raw
                    .iter()
                    .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
                && bytes.get(end) == Some(&0))
            .then(|| StringValue {
                offset: base_offset + offset,
                value: std::str::from_utf8(raw).expect("invariant: printable ASCII is valid UTF-8"),
            })
        })
        .collect()
}

/// Decode every strictly length-framed numeric expression in an OM payload.
///
/// The `hostglobalvariables` marker identifies the owning table. Individual
/// records are self-framed as `handle, 04, length, text, 00`, so expression
/// decoding does not depend on an object-id table having the same cardinality
/// as an external entity-index array.
pub fn numeric_expressions(bytes: &[u8]) -> Vec<NumericExpression<'_>> {
    if !bytes
        .windows(b"hostglobalvariables".len())
        .any(|window| window == b"hostglobalvariables")
    {
        return Vec::new();
    }
    bytes
        .windows(b"(Number [".len())
        .enumerate()
        .filter(|(_, window)| *window == b"(Number [")
        .filter_map(|(offset, _)| {
            numeric_expression_at(
                &bytes[offset.saturating_sub(3)..],
                offset.saturating_sub(3),
                None,
            )
        })
        .collect()
}

/// Locate independently size-framed OM sections and their type registries.
pub fn sections(bytes: &[u8]) -> Vec<Section<'_>> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at + 16 <= bytes.len() {
        let Some(relative) = bytes[at..]
            .windows(4)
            .position(|window| window == [0xff; 4])
        else {
            break;
        };
        let offset = at + relative;
        let Some(payload_len) = bytes
            .get(offset + 8..offset + 12)
            .and_then(|raw| raw.try_into().ok())
            .map(u32::from_be_bytes)
            .map(|value| value as usize)
        else {
            break;
        };
        let Some(end) = offset
            .checked_add(16)
            .and_then(|header_end| header_end.checked_add(payload_len))
        else {
            at = offset + 4;
            continue;
        };
        if bytes.get(offset + 12..offset + 14) != Some(b"OM") || end > bytes.len() {
            at = offset + 4;
            continue;
        }
        let types = type_definitions(bytes, offset + 16, end);
        let field_start = types.last().map_or(offset + 16, |definition| {
            definition.offset + definition.name.len() + 2
        });
        out.push(Section {
            offset,
            byte_len: end - offset,
            types,
            fields: field_definitions(bytes, field_start, end),
        });
        at = end;
    }
    out
}

/// Locate validated NX OM entity-index/object-id-table pairs.
///
/// A candidate is accepted only when the arrays are adjacent, the index is
/// monotone, its first offset is zero, its second offset self-anchors the first
/// entity exactly at the end of the object-id table, and that entity carries the
/// NX root marker.
pub fn indexed_sections(bytes: &[u8]) -> Vec<IndexedSection<'_>> {
    let mut out = Vec::new();
    let mut seen_record_starts = BTreeSet::new();
    for table in 0..bytes.len().saturating_sub(4) {
        let Some(count) = u32_at(bytes, table).map(|value| value as usize) else {
            continue;
        };
        if !(2..=100_000).contains(&count) {
            continue;
        }
        let Some(index_len) = count.checked_add(1).and_then(|n| n.checked_mul(4)) else {
            continue;
        };
        let Some(index_start) = table.checked_sub(index_len) else {
            continue;
        };
        let Some(table_end) = count
            .checked_mul(4)
            .and_then(|length| table.checked_add(4 + length))
        else {
            continue;
        };
        if !is_root_record(bytes.get(table_end..).unwrap_or_default())
            || u32_at(bytes, index_start) != Some(0)
            || !seen_record_starts.insert(table_end)
        {
            continue;
        }
        let Some(first) = u32_at(bytes, index_start + 4).map(|value| value as usize) else {
            continue;
        };
        let Some(base) = table_end.checked_sub(first) else {
            continue;
        };
        let mut offsets = Vec::with_capacity(count + 1);
        for index in 0..=count {
            let Some(value) = u32_at(bytes, index_start + index * 4).map(|v| v as usize) else {
                offsets.clear();
                break;
            };
            offsets.push(value);
        }
        if offsets.len() != count + 1
            || offsets[1] == 0
            || !offsets.windows(2).all(|pair| pair[0] <= pair[1])
            || base
                .checked_add(offsets[count])
                .is_none_or(|end| end > bytes.len())
        {
            continue;
        }
        let mut records = Vec::with_capacity(count - 1);
        for index in 1..count {
            let start = base + offsets[index];
            let end = base + offsets[index + 1];
            let Some(payload) = bytes.get(start..end) else {
                records.clear();
                break;
            };
            let Some(object_id) = u32_at(bytes, table + 4 + index * 4) else {
                records.clear();
                break;
            };
            records.push(EntityRecord {
                object_id: Some(object_id),
                offset: start,
                bytes: payload,
            });
        }
        if records.len() == count - 1 {
            let types = type_definitions(bytes, base, index_start);
            out.push(IndexedSection {
                base,
                entity_index_offset: index_start,
                object_id_table_offset: table,
                types,
                records,
            });
        }
    }
    for count_offset in 8..bytes.len().saturating_sub(4) {
        let Some(record_count) = u32_at(bytes, count_offset).map(|value| value as usize) else {
            continue;
        };
        if !(2..=100_000).contains(&record_count) {
            continue;
        }
        let offset_count = record_count + 2;
        let Some(index_len) = offset_count.checked_mul(4) else {
            continue;
        };
        let Some(index_start) = count_offset.checked_sub(index_len) else {
            continue;
        };
        let Some(first) = u32_at(bytes, index_start).map(|value| value as usize) else {
            continue;
        };
        let Some(second) = u32_at(bytes, index_start + 4).map(|value| value as usize) else {
            continue;
        };
        let Some(last) = u32_at(bytes, count_offset - 4).map(|value| value as usize) else {
            continue;
        };
        if first < count_offset + 4 || first >= second || second > last || last > bytes.len() {
            continue;
        }
        let mut offsets = Vec::with_capacity(offset_count);
        for index in 0..offset_count {
            let Some(offset) = u32_at(bytes, index_start + index * 4).map(|v| v as usize) else {
                offsets.clear();
                break;
            };
            offsets.push(offset);
        }
        if offsets.len() != offset_count
            || offsets[0] < count_offset + 4
            || !offsets.windows(2).all(|pair| pair[0] <= pair[1])
            || offsets.last().is_none_or(|end| *end > bytes.len())
            || !seen_record_starts.insert(offsets[1])
        {
            continue;
        }
        let records = offsets[1..]
            .windows(2)
            .map(|bounds| EntityRecord {
                object_id: None,
                offset: bounds[0],
                bytes: &bytes[bounds[0]..bounds[1]],
            })
            .collect::<Vec<_>>();
        if records.len() != record_count {
            continue;
        }
        out.push(IndexedSection {
            base: 0,
            entity_index_offset: index_start,
            object_id_table_offset: offsets[0],
            types: type_definitions(bytes, 0, index_start),
            records,
        });
    }
    out
}

fn is_root_record(bytes: &[u8]) -> bool {
    if bytes.get(..2) != Some(&[0x04, 0x01]) {
        return false;
    }
    let Some(length) = bytes
        .get(2)
        .copied()
        .map(usize::from)
        .and_then(|declared| declared.checked_sub(2))
    else {
        return false;
    };
    let Some(end) = 3usize.checked_add(length) else {
        return false;
    };
    bytes.get(3..end).is_some_and(|text| {
        text.starts_with(b"NX ")
            && text
                .iter()
                .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
    }) && bytes.get(end) == Some(&0)
}

fn type_definitions(bytes: &[u8], start: usize, end: usize) -> Vec<TypeDefinition<'_>> {
    let mut out = Vec::new();
    let mut at = start;
    while at < end {
        let declared = usize::from(bytes[at]);
        let Some(length) = declared.checked_sub(1) else {
            at += 1;
            continue;
        };
        let name_start = at + 1;
        let name_end = name_start.saturating_add(length);
        let Some(raw) = bytes.get(name_start..name_end) else {
            at += 1;
            continue;
        };
        let valid = raw.starts_with(b"UGS::")
            && raw.iter().all(|byte| (0x20..0x7f).contains(byte))
            && name_end < end;
        if valid {
            let name = std::str::from_utf8(raw)
                .expect("invariant: validated printable ASCII is valid UTF-8");
            out.push(TypeDefinition {
                offset: at,
                name,
                trailing_code: bytes[name_end],
            });
            at = name_end + 1;
        } else {
            at += 1;
        }
    }
    out
}

fn field_definitions(bytes: &[u8], start: usize, end: usize) -> Vec<FieldDefinition<'_>> {
    let mut out = Vec::new();
    let mut search = start;
    let mut limit = start.saturating_add(256).min(end);
    while let Some((definition, at)) = (search..limit)
        .find_map(|at| field_definition_at(bytes, at, end).map(|definition| (definition, at)))
    {
        let next = at + definition.name.len() + 2;
        search = next;
        limit = search.saturating_add(256).min(end);
        out.push(definition);
    }
    out
}

fn field_definition_at(bytes: &[u8], at: usize, end: usize) -> Option<FieldDefinition<'_>> {
    let declared = usize::from(*bytes.get(at)?);
    let length = declared.checked_sub(1)?;
    let name_start = at.checked_add(1)?;
    let name_end = name_start.checked_add(length)?;
    (name_end < end).then_some(())?;
    let raw = bytes.get(name_start..name_end)?;
    (raw.starts_with(b"m_") && raw.iter().all(|byte| (0x20..0x7f).contains(byte))).then_some(())?;
    Some(FieldDefinition {
        offset: at,
        name: std::str::from_utf8(raw).ok()?,
        trailing_code: bytes[name_end],
    })
}

fn numeric_expression_at(
    bytes: &[u8],
    base_offset: usize,
    object_id: Option<u32>,
) -> Option<NumericExpression<'_>> {
    const PREFIX: &[u8] = b"(Number [";
    let relative = bytes
        .windows(PREFIX.len())
        .position(|window| window == PREFIX)?;
    if relative < 3 || bytes.get(relative - 2) != Some(&0x04) {
        return None;
    }
    let declared = usize::from(*bytes.get(relative - 1)?);
    let text_len = declared.checked_sub(2)?;
    let text_end = relative.checked_add(text_len)?;
    (bytes.get(text_end) == Some(&0)).then_some(())?;
    let text = std::str::from_utf8(bytes.get(relative..text_end)?).ok()?;
    text.ends_with("; ").then_some(())?;
    let text = text.strip_prefix("(Number [")?;
    let (unit, rest) = text.split_once("]) ")?;
    let unit = match unit {
        "mm" => ExpressionUnit::Millimeter,
        "degrees" => ExpressionUnit::Degree,
        _ => return None,
    };
    let (name, value_tail) = rest.split_once(": ")?;
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return None;
    }
    let value_text = value_tail.strip_suffix("; ")?;
    let value = value_text.parse::<f64>().ok()?;
    value.is_finite().then_some(NumericExpression {
        object_id,
        offset: base_offset + relative,
        name,
        unit,
        value,
    })
}
