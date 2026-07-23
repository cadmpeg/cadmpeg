// SPDX-License-Identifier: Apache-2.0
//! Semantic dimension records stored in `PMISemanticDataDB`.

use std::collections::{BTreeMap, HashSet};

use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::Exactness;

use crate::container::ContainerScan;
use crate::records::PmiDimension;

/// Add uniquely owner-qualified PMI dimensions to a projection copy of history.
pub(crate) fn enrich_history_parameters(
    histories: &mut [crate::records::FeatureHistory],
    records: &[PmiDimension],
) {
    let mut owners = BTreeMap::<&str, Vec<(usize, usize)>>::new();
    for (history_index, history) in histories.iter().enumerate() {
        for (feature_index, feature) in history.features.iter().enumerate() {
            owners
                .entry(feature.name.as_str())
                .or_default()
                .push((history_index, feature_index));
        }
    }
    let mut candidates = BTreeMap::<(usize, usize, String), Vec<String>>::new();
    for record in records {
        let Some((name, owner_name)) = record.cad_text.split_once('@') else {
            continue;
        };
        let Some([(history_index, feature_index)]) = owners.get(owner_name).map(Vec::as_slice)
        else {
            continue;
        };
        let millimetres = record.value * 1000.0;
        let expression = match record.subtype.as_str() {
            "Linear" => format!("{millimetres}mm"),
            "Angle" => record.value.to_string(),
            "Diameter" => format!("<MOD-DIAM>{millimetres}mm"),
            "Radial" => format!("R{millimetres}mm"),
            _ => continue,
        };
        candidates
            .entry((*history_index, *feature_index, name.to_string()))
            .or_default()
            .push(expression);
    }
    for ((history_index, feature_index, name), mut expressions) in candidates {
        expressions.sort();
        expressions.dedup();
        let [expression] = expressions.as_slice() else {
            continue;
        };
        histories[history_index].features[feature_index]
            .parameters
            .entry(name)
            .or_insert_with(|| expression.clone());
    }
}

pub(crate) fn patch_payload(
    ir: &cadmpeg_ir::CadIr,
    block_id: &str,
    payload: &mut [u8],
) -> Result<(), cadmpeg_ir::CodecError> {
    use cadmpeg_ir::features::{ParameterValue, PmiDimensionSubtype};

    let Some(namespace) = ir.native.namespace("sldprt") else {
        return Ok(());
    };
    let native = crate::native::SldprtNative::load(namespace).map_err(|error| {
        cadmpeg_ir::CodecError::Malformed(format!("invalid SLDPRT native PMI: {error}"))
    })?;
    for record in native
        .pmi_dimensions
        .iter()
        .filter(|record| record.parent == block_id)
    {
        let mut parameters = ir.model.parameters.iter().filter(|parameter| {
            parameter.pmi.as_ref().map(|pmi| pmi.native_ref.as_str()) == Some(record.id.as_str())
        });
        let Some(parameter) = parameters.next() else {
            continue;
        };
        if parameters.next().is_some() {
            return Err(cadmpeg_ir::CodecError::Malformed(format!(
                "multiple parameters reference PMI record {}",
                record.id
            )));
        }
        let semantic = parameter.pmi.as_ref().expect("filtered above");
        let subtype = match record.subtype.as_str() {
            "Linear" => PmiDimensionSubtype::Linear,
            "Angle" => PmiDimensionSubtype::Angle,
            "Diameter" => PmiDimensionSubtype::Diameter,
            "Radial" => PmiDimensionSubtype::Radial,
            other => PmiDimensionSubtype::Native(other.to_string()),
        };
        if semantic.subtype != subtype {
            return Err(cadmpeg_ir::CodecError::NotImplemented(format!(
                "SLDPRT PMI record {} changes dimension subtype",
                record.id
            )));
        }
        let native_value = match (&subtype, &parameter.value) {
            (PmiDimensionSubtype::Angle, Some(ParameterValue::Angle(angle))) => angle.0,
            (
                PmiDimensionSubtype::Linear
                | PmiDimensionSubtype::Diameter
                | PmiDimensionSubtype::Radial,
                Some(ParameterValue::Length(length)),
            ) => length.0 / 1000.0,
            _ => {
                return Err(cadmpeg_ir::CodecError::NotImplemented(format!(
                    "SLDPRT PMI record {} has a value incompatible with its dimension subtype",
                    record.id
                )));
            }
        };
        patch_bytes(
            payload,
            record.value_offset,
            &native_value.to_be_bytes(),
            &record.id,
        )?;
        let precision = u8::try_from(semantic.precision)
            .ok()
            .filter(|value| *value < 128)
            .ok_or_else(|| {
                cadmpeg_ir::CodecError::NotImplemented(format!(
                    "SLDPRT PMI record {} requires fixint precision",
                    record.id
                ))
            })?;
        patch_bytes(payload, record.precision_offset, &[precision], &record.id)?;
        for (offset, value) in [
            (record.basic_offset, semantic.basic),
            (record.inspection_offset, semantic.inspection),
            (record.reference_only_offset, semantic.reference_only),
        ] {
            patch_bytes(
                payload,
                offset,
                &[if value { 0xc3 } else { 0xc2 }],
                &record.id,
            )?;
        }
        if semantic.display_text != record.display_text {
            let (Some(offset), Some(text), Some(previous)) = (
                record.display_text_offset,
                semantic.display_text.as_deref(),
                record.display_text.as_deref(),
            ) else {
                return Err(cadmpeg_ir::CodecError::NotImplemented(format!(
                    "SLDPRT PMI record {} changes optional display text",
                    record.id
                )));
            };
            if text.len() != previous.len() {
                return Err(cadmpeg_ir::CodecError::NotImplemented(format!(
                    "SLDPRT PMI record {} changes display-text width",
                    record.id
                )));
            }
            patch_bytes(payload, offset, text.as_bytes(), &record.id)?;
        }
    }
    Ok(())
}

fn patch_bytes(
    payload: &mut [u8],
    offset: u64,
    bytes: &[u8],
    record: &str,
) -> Result<(), cadmpeg_ir::CodecError> {
    let start = usize::try_from(offset).map_err(|_| {
        cadmpeg_ir::CodecError::Malformed(format!(
            "SLDPRT PMI record {record} exceeds address space"
        ))
    })?;
    let end = start.checked_add(bytes.len()).ok_or_else(|| {
        cadmpeg_ir::CodecError::Malformed(format!("SLDPRT PMI record {record} offset overflows"))
    })?;
    payload
        .get_mut(start..end)
        .ok_or_else(|| {
            cadmpeg_ir::CodecError::Malformed(format!(
                "SLDPRT PMI record {record} lies outside its block"
            ))
        })?
        .copy_from_slice(bytes);
    Ok(())
}

pub(crate) fn apply_to_parameters(
    parameters: &mut Vec<cadmpeg_ir::features::DesignParameter>,
    features: &[cadmpeg_ir::features::Feature],
    records: &[PmiDimension],
) {
    use cadmpeg_ir::features::{
        DesignParameter, DimensionDisplay, Length, ParameterId, ParameterPmi, ParameterValue,
        PmiDimensionSubtype,
    };

    let mut feature_names = BTreeMap::<&str, Vec<&cadmpeg_ir::features::Feature>>::new();
    for feature in features {
        if let Some(name) = feature.name.as_deref() {
            feature_names.entry(name).or_default().push(feature);
        }
    }
    for record in records {
        let Some((name, owner_name)) = record.cad_text.split_once('@') else {
            continue;
        };
        let Some([owner]) = feature_names.get(owner_name).map(Vec::as_slice) else {
            continue;
        };
        let subtype = match record.subtype.as_str() {
            "Linear" => PmiDimensionSubtype::Linear,
            "Angle" => PmiDimensionSubtype::Angle,
            "Diameter" => PmiDimensionSubtype::Diameter,
            "Radial" => PmiDimensionSubtype::Radial,
            other => PmiDimensionSubtype::Native(other.to_string()),
        };
        let millimetres = record.value * 1000.0;
        let (expression, display, value) = match subtype {
            PmiDimensionSubtype::Linear => (
                format!("{millimetres}mm"),
                None,
                Some(ParameterValue::Length(Length(millimetres))),
            ),
            PmiDimensionSubtype::Angle => (
                record.value.to_string(),
                None,
                Some(ParameterValue::Angle(cadmpeg_ir::features::Angle(
                    record.value,
                ))),
            ),
            PmiDimensionSubtype::Diameter => (
                format!("<MOD-DIAM>{millimetres}mm"),
                Some(DimensionDisplay::Diameter),
                Some(ParameterValue::Length(Length(millimetres))),
            ),
            PmiDimensionSubtype::Radial => (
                format!("R{millimetres}mm"),
                Some(DimensionDisplay::Radius),
                Some(ParameterValue::Length(Length(millimetres))),
            ),
            PmiDimensionSubtype::Native(_) => (record.value.to_string(), None, None),
        };
        let semantic = ParameterPmi {
            subtype,
            precision: record.precision,
            display_text: record.display_text.clone(),
            basic: record.basic,
            inspection: record.inspection,
            reference_only: record.reference_only,
            native_ref: record.id.clone(),
        };
        if let Some(parameter) = parameters
            .iter_mut()
            .find(|parameter| parameter.owner.as_ref() == Some(&owner.id) && parameter.name == name)
        {
            parameter.expression = expression;
            parameter.display = display;
            parameter.value = value;
            parameter.pmi = Some(semantic);
            continue;
        }
        let ordinal = parameters
            .iter()
            .filter(|parameter| parameter.owner.as_ref() == Some(&owner.id))
            .map(|parameter| parameter.ordinal)
            .max()
            .map_or(0, |ordinal| ordinal.saturating_add(1));
        parameters.push(DesignParameter {
            id: ParameterId(format!("sldprt:model:parameter#pmi:{}", record.guid)),
            owner: Some(owner.id.clone()),
            ordinal,
            name: name.to_string(),
            expression,
            display,
            value,
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: Some(semantic),
            native_ref: None,
        });
    }
}

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
    for source in scan.sections() {
        let Some(section) = source.name() else {
            continue;
        };
        if !section.eq_ignore_ascii_case("Contents/PMISemanticDataDB") {
            continue;
        }
        let payload = source.payload();
        for offset in map_offsets(payload) {
            let mut cursor = offset;
            let Some(Value::Map(outer)) = parse_value(payload, &mut cursor, 0) else {
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
            let Some(guid) = guid_before(payload, offset) else {
                continue;
            };
            if !seen.insert(guid.clone()) {
                continue;
            }
            let Some(value) = float_field(item, "value") else {
                continue;
            };
            let Some(value_marker) = field_marker(payload, offset, cursor, "value") else {
                continue;
            };
            if payload.get(value_marker) != Some(&0xcb) {
                continue;
            }
            let Some(precision_offset) = field_marker(payload, offset, cursor, "valPrecision")
            else {
                continue;
            };
            let Some(basic_offset) = field_marker(payload, offset, cursor, "isBasic") else {
                continue;
            };
            let Some(inspection_offset) = field_marker(payload, offset, cursor, "isInspection")
            else {
                continue;
            };
            let Some(reference_only_offset) =
                field_marker(payload, offset, cursor, "isReferenceOnly")
            else {
                continue;
            };
            let display_text_offset = field_marker(payload, offset, cursor, "dimText")
                .and_then(|marker| string_data_offset(payload, marker));
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
                parent: source.native_id(),
                offset: offset as u64,
                guid,
                cad_text: cad_text.to_string(),
                subtype: string_field(item, "dimSubType")
                    .unwrap_or_default()
                    .to_string(),
                value,
                value_offset: (value_marker + 1) as u64,
                precision: int_field(item, "valPrecision").unwrap_or_default(),
                precision_offset: precision_offset as u64,
                display_text: string_field(&outer, "dimText").map(str::to_string),
                display_text_offset: display_text_offset.map(|offset| offset as u64),
                basic: bool_field(item, "isBasic").unwrap_or(false),
                basic_offset: basic_offset as u64,
                inspection: bool_field(item, "isInspection").unwrap_or(false),
                inspection_offset: inspection_offset as u64,
                reference_only: bool_field(item, "isReferenceOnly").unwrap_or(false),
                reference_only_offset: reference_only_offset as u64,
            });
        }
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    records
}

fn field_marker(payload: &[u8], start: usize, end: usize, key: &str) -> Option<usize> {
    if key.len() >= 32 {
        return None;
    }
    let mut encoded = Vec::with_capacity(key.len() + 1);
    encoded.push(0xa0 | key.len() as u8);
    encoded.extend_from_slice(key.as_bytes());
    payload
        .get(start..end)?
        .windows(encoded.len())
        .position(|bytes| bytes == encoded)
        .map(|relative| start + relative + encoded.len())
}

fn string_data_offset(payload: &[u8], marker: usize) -> Option<usize> {
    match *payload.get(marker)? {
        0xa0..=0xbf => marker.checked_add(1),
        0xd9 => marker.checked_add(2),
        0xda => marker.checked_add(3),
        _ => None,
    }
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
    // Every element encodes as at least one marker byte, so a length exceeding
    // the unread input cannot be satisfied and is rejected before allocating.
    let remaining = bytes.len().saturating_sub(*cursor);
    let len = cadmpeg_ir::wire::cursor::bounded_len(len as u64, 1, remaining)?;
    let mut values = Vec::with_capacity(len);
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
