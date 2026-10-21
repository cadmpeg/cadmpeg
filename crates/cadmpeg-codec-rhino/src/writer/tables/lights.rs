// SPDX-License-Identifier: Apache-2.0
//! Light geometry and photometric controls have a native typed inverse.

use super::{
    CadIr, CodecError, Map, Value, class_wrapper, color, crc_chunk_with_direct, integer, records,
    short_chunk, utf16, uuid, wire_index,
};

const LIGHT_CLASS: [u8; 16] = [
    0x13, 0x85, 0xa0, 0x85, 0x83, 0xf3, 0xd3, 0x11, 0xbf, 0xe7, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];

fn scalar(fields: &Map<String, Value>, name: &str) -> Result<f64, CodecError> {
    fields
        .get(name)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .ok_or_else(|| CodecError::malformed(format!("Rhino light {name} must be finite")))
}

fn vector(fields: &Map<String, Value>, name: &str, output: &mut Vec<u8>) -> Result<(), CodecError> {
    let values = fields
        .get(name)
        .and_then(Value::as_array)
        .filter(|values| values.len() == 3)
        .ok_or_else(|| {
            CodecError::malformed(format!("Rhino light {name} needs three coordinates"))
        })?;
    for value in values {
        output.extend(
            value
                .as_f64()
                .filter(|value| value.is_finite())
                .ok_or_else(|| {
                    CodecError::malformed(format!("Rhino light {name} coordinate must be finite"))
                })?
                .to_le_bytes(),
        );
    }
    Ok(())
}

pub(in crate::writer) fn lights(ir: &CadIr) -> Result<Vec<Vec<u8>>, CodecError> {
    let mut output = Vec::new();
    for (index, record) in records(ir, "lights").iter().enumerate() {
        let fields = record.fields()?;
        let mut payload = vec![0x12];
        payload.extend(
            i32::from(
                fields
                    .get("enabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            )
            .to_le_bytes(),
        );
        payload.extend(integer(Some(&fields), "style", 0).to_le_bytes());
        for name in ["intensity", "watts"] {
            payload.extend(scalar(&fields, name)?.to_le_bytes());
        }
        for name in ["ambient", "diffuse", "specular"] {
            payload.extend(color(Some(&fields), name)?);
        }
        for name in ["direction", "location"] {
            vector(&fields, name, &mut payload)?;
        }
        for name in ["spot_angle_degrees", "spot_exponent"] {
            payload.extend(scalar(&fields, name)?.to_le_bytes());
        }
        vector(&fields, "attenuation", &mut payload)?;
        payload.extend(scalar(&fields, "shadow_intensity")?.to_le_bytes());
        payload.extend(wire_index(index)?.to_le_bytes());
        payload.extend(uuid(fields.get("source_uuid"), record.id()));
        payload.extend(utf16(
            fields.get("name").and_then(Value::as_str).unwrap_or(""),
        )?);
        for name in ["length", "width"] {
            vector(&fields, name, &mut payload)?;
        }
        payload.extend(scalar(&fields, "hotspot")?.to_le_bytes());
        let mut body = class_wrapper(LIGHT_CLASS, &payload)?;
        body.extend(short_chunk(0x8200_006f, 0));
        output.push(crc_chunk_with_direct(0x2000_8060, &body, &[])?);
    }
    Ok(output)
}
