// SPDX-License-Identifier: Apache-2.0
//! Typed hatch lines are emitted in the archive's millimetre units.

use super::{
    anonymous, anonymous_with_children, class_wrapper_with_direct, component,
    crc_chunk_with_direct, integer, records, utf16, wire_index, CadIr, CodecError, Value,
};

const PATTERN_CLASS: [u8; 16] = [
    0x91, 0x7c, 0x4e, 0x06, 0xf6, 0x35, 0x34, 0x47, 0xa4, 0x46, 0x79, 0xff, 0x7c, 0xd6, 0x59, 0xe1,
];

fn scalar(value: Option<&Value>, name: &str) -> Result<f64, CodecError> {
    value
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .ok_or_else(|| CodecError::malformed(format!("Rhino hatch {name} must be finite")))
}

fn line(value: &Value) -> Result<Vec<u8>, CodecError> {
    let mut body = scalar(value.get("angle_radians"), "angle")?
        .to_le_bytes()
        .to_vec();
    for name in ["base_millimeters", "offset_millimeters"] {
        let point = value
            .get(name)
            .and_then(Value::as_array)
            .filter(|values| values.len() == 2)
            .ok_or_else(|| {
                CodecError::malformed(format!("Rhino hatch {name} needs two coordinates"))
            })?;
        for coordinate in point {
            body.extend(scalar(Some(coordinate), name)?.to_le_bytes());
        }
    }
    let dashes = value
        .get("dashes_millimeters")
        .and_then(Value::as_array)
        .ok_or_else(|| CodecError::malformed("Rhino hatch has no typed dash list"))?;
    body.extend(wire_index(dashes.len())?.to_le_bytes());
    for dash in dashes {
        body.extend(scalar(Some(dash), "dash")?.to_le_bytes());
    }
    anonymous(1, 0, &body)
}

pub(in crate::writer) fn patterns(ir: &CadIr) -> Result<Vec<Vec<u8>>, CodecError> {
    let mut output = Vec::new();
    for (index, record) in records(ir, "hatch_patterns").iter().enumerate() {
        let fields = record.fields()?;
        let mut payload = component(
            record.id(),
            wire_index(index)?,
            fields.get("name").and_then(Value::as_str).unwrap_or(""),
            fields.get("source_uuid"),
        )?;
        let component_end = payload.len();
        payload.extend(integer(Some(&fields), "fill_type", 0).to_le_bytes());
        payload.extend(utf16(
            fields
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or(""),
        )?);
        let lines = fields
            .get("lines")
            .and_then(Value::as_array)
            .ok_or_else(|| CodecError::malformed("Rhino hatch has no typed line list"))?;
        let mut list = wire_index(lines.len())?.to_le_bytes().to_vec();
        let mut children = Vec::new();
        for value in lines {
            let start = list.len();
            list.extend(line(value)?);
            children.push(start..list.len());
        }
        let ranges = crate::chunks::direct_checksum_ranges(&(0..list.len()), &children)
            .map_err(CodecError::malformed)?;
        let mut direct = Vec::new();
        for range in &ranges {
            direct.extend(&list[range]);
        }
        let list_start = payload.len();
        payload.extend(crc_chunk_with_direct(0x4000_8000, &list, &direct)?);
        let list_end = payload.len();
        let payload =
            anonymous_with_children(1, 0, &payload, &[0..component_end, list_start..list_end])?;
        output.push(crc_chunk_with_direct(
            0x2000_8077,
            &class_wrapper_with_direct(PATTERN_CLASS, &payload, &[])?,
            &[],
        )?);
    }
    Ok(output)
}
