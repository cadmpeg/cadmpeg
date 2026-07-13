// SPDX-License-Identifier: Apache-2.0
//! Semantic dimension records stored in `PMISemanticDataDB`.

use std::collections::{BTreeMap, HashSet};

use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::Exactness;

use crate::container::ContainerScan;
use crate::records::PmiDimension;

#[derive(Debug, Clone)]
enum Value {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Array(Vec<Value>),
    Map(BTreeMap<String, Value>),
    Nil,
}

pub(crate) fn dimensions(scan: &ContainerScan, annotations: &mut Annotations) -> Vec<PmiDimension> {
    let mut records = Vec::new();
    let mut seen = HashSet::<String>::new();
    for block in &scan.blocks {
        let Some(section) = block.section.as_deref() else {
            continue;
        };
        if !section.eq_ignore_ascii_case("Contents/PMISemanticDataDB") {
            continue;
        }
        for offset in map_offsets(&block.payload) {
            let mut cursor = offset;
            let Some(Value::Map(outer)) = parse_value(&block.payload, &mut cursor, 0) else {
                continue;
            };
            let Some(cad_text) = string_field(&outer, "cadText") else {
                continue;
            };
            let Some(Value::Array(items)) = outer.get("dimItems") else {
                continue;
            };
            let Some(Value::Map(item)) = items.first() else {
                continue;
            };
            if string_field(item, "class") != Some("DimSemData") {
                continue;
            }
            let Some(guid) = guid_before(&block.payload, offset) else {
                continue;
            };
            if !seen.insert(guid.clone()) {
                continue;
            }
            let Some(value) = float_field(item, "value") else {
                continue;
            };
            let id = format!("sldprt:pmi:dimension#{guid}");
            crate::annotations::note(
                annotations,
                id.clone(),
                section,
                offset as u64,
                "messagepack_dim_sem_data",
                Exactness::ByteExact,
            );
            records.push(PmiDimension {
                id,
                parent: format!("sldprt:file:block#{}", block.offset),
                offset: offset as u64,
                guid,
                cad_text: cad_text.to_string(),
                subtype: string_field(item, "dimSubType")
                    .unwrap_or_default()
                    .to_string(),
                value,
                precision: int_field(item, "valPrecision").unwrap_or_default(),
                display_text: string_field(&outer, "dimText").map(str::to_string),
                basic: bool_field(item, "isBasic").unwrap_or(false),
                inspection: bool_field(item, "isInspection").unwrap_or(false),
                reference_only: bool_field(item, "isReferenceOnly").unwrap_or(false),
            });
        }
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    records
}

fn map_offsets(payload: &[u8]) -> impl Iterator<Item = usize> + '_ {
    const PREFIX: &[u8] = b"\x87\xa8annoType";
    payload
        .windows(PREFIX.len())
        .enumerate()
        .filter_map(|(offset, bytes)| (bytes == PREFIX).then_some(offset))
}

fn guid_before(payload: &[u8], offset: usize) -> Option<String> {
    let start = offset.checked_sub(36)?;
    let guid = std::str::from_utf8(payload.get(start..offset)?).ok()?;
    let bytes = guid.as_bytes();
    (bytes.get(8) == Some(&b'-')
        && bytes.get(13) == Some(&b'-')
        && bytes.get(18) == Some(&b'-')
        && bytes.get(23) == Some(&b'-')
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| [8, 13, 18, 23].contains(&index) || byte.is_ascii_hexdigit()))
    .then(|| guid.to_ascii_lowercase())
}

fn parse_value(bytes: &[u8], cursor: &mut usize, depth: usize) -> Option<Value> {
    if depth > 16 {
        return None;
    }
    let marker = take_u8(bytes, cursor)?;
    match marker {
        0x00..=0x7f => Some(Value::Int(i64::from(marker))),
        0x80..=0x8f => parse_map(bytes, cursor, usize::from(marker & 0x0f), depth),
        0x90..=0x9f => parse_array(bytes, cursor, usize::from(marker & 0x0f), depth),
        0xa0..=0xbf => parse_string(bytes, cursor, usize::from(marker & 0x1f)),
        0xc0 => Some(Value::Nil),
        0xc2 => Some(Value::Bool(false)),
        0xc3 => Some(Value::Bool(true)),
        0xca => Some(Value::Float(f64::from(f32::from_bits(take_u32(
            bytes, cursor,
        )?)))),
        0xcb => Some(Value::Float(f64::from_bits(take_u64(bytes, cursor)?))),
        0xcc => Some(Value::Int(i64::from(take_u8(bytes, cursor)?))),
        0xcd => Some(Value::Int(i64::from(take_u16(bytes, cursor)?))),
        0xce => Some(Value::Int(i64::from(take_u32(bytes, cursor)?))),
        0xd0 => Some(Value::Int(i64::from(take_u8(bytes, cursor)? as i8))),
        0xd1 => Some(Value::Int(i64::from(take_u16(bytes, cursor)? as i16))),
        0xd2 => Some(Value::Int(i64::from(take_u32(bytes, cursor)? as i32))),
        0xd9 => {
            let len = usize::from(take_u8(bytes, cursor)?);
            parse_string(bytes, cursor, len)
        }
        0xda => {
            let len = usize::from(take_u16(bytes, cursor)?);
            parse_string(bytes, cursor, len)
        }
        0xde => {
            let len = usize::from(take_u16(bytes, cursor)?);
            parse_map(bytes, cursor, len, depth)
        }
        0xe0..=0xff => Some(Value::Int(i64::from(marker as i8))),
        _ => None,
    }
}

fn parse_map(bytes: &[u8], cursor: &mut usize, len: usize, depth: usize) -> Option<Value> {
    let mut values = BTreeMap::new();
    for _ in 0..len {
        let Value::String(key) = parse_value(bytes, cursor, depth + 1)? else {
            return None;
        };
        values.insert(key, parse_value(bytes, cursor, depth + 1)?);
    }
    Some(Value::Map(values))
}

fn parse_array(bytes: &[u8], cursor: &mut usize, len: usize, depth: usize) -> Option<Value> {
    let mut values = Vec::with_capacity(len.min(1024));
    for _ in 0..len {
        values.push(parse_value(bytes, cursor, depth + 1)?);
    }
    Some(Value::Array(values))
}

fn parse_string(bytes: &[u8], cursor: &mut usize, len: usize) -> Option<Value> {
    let end = cursor.checked_add(len)?;
    let value = std::str::from_utf8(bytes.get(*cursor..end)?)
        .ok()?
        .to_string();
    *cursor = end;
    Some(Value::String(value))
}

fn take_u8(bytes: &[u8], cursor: &mut usize) -> Option<u8> {
    let value = *bytes.get(*cursor)?;
    *cursor += 1;
    Some(value)
}

fn take_u16(bytes: &[u8], cursor: &mut usize) -> Option<u16> {
    let end = cursor.checked_add(2)?;
    let value = u16::from_be_bytes(bytes.get(*cursor..end)?.try_into().ok()?);
    *cursor = end;
    Some(value)
}

fn take_u32(bytes: &[u8], cursor: &mut usize) -> Option<u32> {
    let end = cursor.checked_add(4)?;
    let value = u32::from_be_bytes(bytes.get(*cursor..end)?.try_into().ok()?);
    *cursor = end;
    Some(value)
}

fn take_u64(bytes: &[u8], cursor: &mut usize) -> Option<u64> {
    let end = cursor.checked_add(8)?;
    let value = u64::from_be_bytes(bytes.get(*cursor..end)?.try_into().ok()?);
    *cursor = end;
    Some(value)
}

fn string_field<'a>(map: &'a BTreeMap<String, Value>, key: &str) -> Option<&'a str> {
    match map.get(key)? {
        Value::String(value) => Some(value),
        _ => None,
    }
}

fn bool_field(map: &BTreeMap<String, Value>, key: &str) -> Option<bool> {
    match map.get(key)? {
        Value::Bool(value) => Some(*value),
        _ => None,
    }
}

fn int_field(map: &BTreeMap<String, Value>, key: &str) -> Option<i64> {
    match map.get(key)? {
        Value::Int(value) => Some(*value),
        _ => None,
    }
}

fn float_field(map: &BTreeMap<String, Value>, key: &str) -> Option<f64> {
    match map.get(key)? {
        Value::Float(value) if value.is_finite() => Some(*value),
        Value::Int(value) => Some(*value as f64),
        _ => None,
    }
}
