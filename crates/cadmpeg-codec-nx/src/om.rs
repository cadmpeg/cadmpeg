// SPDX-License-Identifier: Apache-2.0
//! Frame NX object-model entities using external boundary and identity arrays.

use std::collections::BTreeSet;

use cadmpeg_ir::le::u32_at;

const ROOT_PREFIX: &[u8] = b"\x04\x01\x0eNX ";

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

impl<'a> IndexedSection<'a> {
    /// Decode explicit numeric-expression text within bounded entity records.
    pub fn numeric_expressions(&self) -> Vec<NumericExpression<'a>> {
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
            .filter_map(|record| {
                numeric_expression_at(record.bytes, record.offset, record.object_id)
            })
            .collect()
    }
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
        if bytes.get(table_end..table_end + ROOT_PREFIX.len()) != Some(ROOT_PREFIX)
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
            break;
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
