// SPDX-License-Identifier: Apache-2.0
//! Source-free inverses for native presentation tables without neutral homes.

use super::{
    anonymous_with_children, class_wrapper, class_wrapper_with_direct, component,
    crc_chunk_with_direct, object_record, records, source_record_resolves, utf16, uuid, wire_index,
    CadIr, CodecError, Value,
};

const GROUP_CLASS: [u8; 16] = [
    0x97, 0x9f, 0x1d, 0x72, 0x45, 0x36, 0xc4, 0x44, 0x8b, 0xe6, 0xb2, 0xcf, 0x69, 0x7d, 0x25, 0xce,
];
const LINETYPE_CLASS: [u8; 16] = [
    0x24, 0x0a, 0xf1, 0x26, 0x13, 0x7d, 0x05, 0x4f, 0x8f, 0xda, 0x8e, 0x36, 0x4d, 0xaf, 0x8e, 0xa6,
];

pub(in crate::writer) fn groups(ir: &CadIr) -> Result<Vec<Vec<u8>>, CodecError> {
    let mut output = Vec::new();
    for (index, record) in records(ir, "groups")
        .iter()
        .filter(|record| group_resolves(ir, record))
        .enumerate()
    {
        let fields = record.fields()?;
        let mut payload = vec![0x11];
        payload.extend(wire_index(index)?.to_le_bytes());
        payload.extend(utf16(
            fields.get("name").and_then(Value::as_str).unwrap_or(""),
        )?);
        payload.extend(uuid(fields.get("source_uuid"), record.id()));
        output.push(crc_chunk_with_direct(
            0x2000_8073,
            &class_wrapper(GROUP_CLASS, &payload)?,
            &[],
        )?);
    }
    Ok(output)
}

pub(in crate::writer) fn group_resolves(ir: &CadIr, record: &cadmpeg_ir::NativeRecord) -> bool {
    record
        .field("links")
        .and_then(|value| {
            value.as_array().map(|links| {
                links.iter().all(|link| {
                    link.as_str()
                        .is_some_and(|source| source_record_resolves(ir, source))
                })
            })
        })
        .unwrap_or(false)
}

pub(in crate::writer) fn group_indices(ir: &CadIr, identity: &str) -> Result<Vec<i32>, CodecError> {
    let Some(source) = object_record(ir, identity) else {
        return Ok(Vec::new());
    };
    records(ir, "groups")
        .iter()
        .filter(|record| group_resolves(ir, record))
        .enumerate()
        .filter(|(_, record)| {
            record
                .field("links")
                .and_then(|value| {
                    value.as_array().map(|links| {
                        links
                            .iter()
                            .any(|link| link.as_str() == Some(source.as_str()))
                    })
                })
                .unwrap_or(false)
        })
        .map(|(index, _)| wire_index(index))
        .collect()
}

#[cfg(test)]
mod tests;

pub(in crate::writer) fn linetypes(ir: &CadIr) -> Result<Vec<Vec<u8>>, CodecError> {
    let mut output = Vec::new();
    for (index, record) in records(ir, "linetypes").iter().enumerate() {
        let fields = record.fields()?;
        let mut payload = component(
            record.id(),
            wire_index(index)?,
            fields.get("name").and_then(Value::as_str).unwrap_or(""),
            fields.get("source_uuid"),
        )?;
        let component_end = payload.len();
        let segments = fields
            .get("segments")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice);
        payload.extend(wire_index(segments.len())?.to_le_bytes());
        for segment in segments {
            payload.extend(
                segment
                    .get("length_millimeters")
                    .and_then(Value::as_f64)
                    .ok_or_else(|| {
                        CodecError::malformed("Rhino linetype segment has no finite length")
                    })?
                    .to_le_bytes(),
            );
            payload.extend(
                u32::try_from(
                    segment
                        .get("segment_type")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                )
                .map_err(CodecError::malformed)?
                .to_le_bytes(),
            );
        }
        for (tag, name) in [(1, "line_cap"), (2, "line_join")] {
            payload.extend([
                tag,
                u8::try_from(fields.get(name).and_then(Value::as_u64).unwrap_or(0))
                    .map_err(CodecError::malformed)?,
            ]);
        }
        payload.push(3);
        payload.extend(
            fields
                .get("width")
                .and_then(Value::as_f64)
                .unwrap_or(1.0)
                .to_le_bytes(),
        );
        payload.extend([
            4,
            u8::try_from(
                fields
                    .get("width_units")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            )
            .map_err(CodecError::malformed)?,
        ]);
        payload.push(5);
        let taper = fields
            .get("taper_points")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice);
        payload.extend(wire_index(taper.len())?.to_le_bytes());
        for point in taper {
            let pair = point
                .as_array()
                .filter(|pair| pair.len() == 2)
                .ok_or_else(|| {
                    CodecError::malformed("Rhino linetype taper needs two finite coordinates")
                })?;
            for value in pair {
                payload.extend(
                    value
                        .as_f64()
                        .ok_or_else(|| CodecError::malformed("Rhino linetype taper is nonnumeric"))?
                        .to_le_bytes(),
                );
            }
        }
        payload.extend([
            6,
            u8::from(
                fields
                    .get("always_model_distance")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            ),
            0,
        ]);
        output.push(crc_chunk_with_direct(
            0x2000_8078,
            &class_wrapper_with_direct(
                LINETYPE_CLASS,
                &anonymous_with_children(
                    2,
                    3,
                    &payload,
                    std::slice::from_ref(&(0..component_end)),
                )?,
                &[],
            )?,
            &[],
        )?);
    }
    Ok(output)
}
