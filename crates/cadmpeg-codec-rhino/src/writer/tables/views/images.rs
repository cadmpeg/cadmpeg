// SPDX-License-Identifier: Apache-2.0
//! Typed wallpaper and trace-image settings, without retained source images.

use super::{
    CodecError, Map, Value, anonymous, anonymous_with_children, crc_chunk_with_direct, flag,
    object, scalar, text, utf16, vector,
};

fn reference(fields: &Map<String, Value>) -> Result<Vec<u8>, CodecError> {
    let digest = text(fields, "content_sha1");
    if digest.len() != 40 || !digest.is_ascii() {
        return Err(CodecError::malformed(
            "Rhino image content SHA-1 needs 40 hexadecimal digits",
        ));
    }
    let mut bytes = Vec::new();
    for index in (0..40).step_by(2) {
        bytes.push(
            u8::from_str_radix(&digest[index..index + 2], 16).map_err(CodecError::malformed)?,
        );
    }
    let name_hash = anonymous(1, 0, &[0; 20])?;
    let content_hash = anonymous(1, 0, &bytes)?;
    let mut hash_body = vec![0; 24];
    let first = hash_body.len();
    hash_body.extend(name_hash);
    let first = first..hash_body.len();
    let second = hash_body.len();
    hash_body.extend(content_hash);
    let second = second..hash_body.len();
    let hash = anonymous_with_children(1, 0, &hash_body, &[first, second])?;
    let mut body = utf16(text(fields, "full_path"))?;
    body.extend(utf16(text(fields, "relative_path"))?);
    let start = body.len();
    body.extend(hash);
    let child = start..body.len();
    body.extend(0u32.to_le_bytes());
    let embedded = fields
        .get("embedded_file_uuid")
        .and_then(Value::as_str)
        .and_then(crate::presentation::parse_uuid_text);
    if let Some(embedded) = embedded {
        body.extend(embedded.to_wire());
    }
    anonymous_with_children(1, i32::from(embedded.is_some()), &body, &[child])
}

fn with_reference(
    typecode: u32,
    mut body: Vec<u8>,
    fields: &Map<String, Value>,
) -> Result<Vec<u8>, CodecError> {
    let mut children = Vec::new();
    if let Some(reference_fields) = object(fields, "file_reference") {
        let start = body.len();
        body.extend(reference(reference_fields)?);
        children.push(start..body.len());
    }
    let ranges = crate::chunks::direct_checksum_ranges(&(0..body.len()), &children)
        .map_err(CodecError::malformed)?;
    let direct = ranges
        .into_iter()
        .flat_map(|range| body[range].to_vec())
        .collect::<Vec<_>>();
    crc_chunk_with_direct(typecode, &body, &direct)
}

pub(super) fn append(
    fields: &Map<String, Value>,
    children: &mut Vec<Vec<u8>>,
) -> Result<(), CodecError> {
    if let Some(trace) = object(fields, "trace_image") {
        let has_reference = object(trace, "file_reference").is_some();
        let mut body = vec![if has_reference { 0x14 } else { 0x13 }];
        body.extend(utf16(text(trace, "legacy_file_path"))?);
        for name in ["width_mm", "height_mm"] {
            body.extend(scalar(trace, name)?.to_le_bytes());
        }
        for name in ["plane_origin_mm", "plane_x_axis", "plane_y_axis"] {
            vector(trace, name, 3, &mut body)?;
        }
        let x = trace
            .get("plane_x_axis")
            .and_then(Value::as_array)
            .ok_or_else(|| CodecError::malformed("Rhino trace X axis absent"))?;
        let y = trace
            .get("plane_y_axis")
            .and_then(Value::as_array)
            .ok_or_else(|| CodecError::malformed("Rhino trace Y axis absent"))?;
        let origin = trace
            .get("plane_origin_mm")
            .and_then(Value::as_array)
            .ok_or_else(|| CodecError::malformed("Rhino trace origin absent"))?;
        let read = |values: &[Value], index| {
            values
                .get(index)
                .and_then(Value::as_f64)
                .ok_or_else(|| CodecError::malformed("Rhino trace frame coordinate absent"))
        };
        let x = [read(x, 0)?, read(x, 1)?, read(x, 2)?];
        let y = [read(y, 0)?, read(y, 1)?, read(y, 2)?];
        let z = [
            x[1] * y[2] - x[2] * y[1],
            x[2] * y[0] - x[0] * y[2],
            x[0] * y[1] - x[1] * y[0],
        ];
        for value in z {
            body.extend(value.to_le_bytes());
        }
        for value in z {
            body.extend(value.to_le_bytes());
        }
        body.extend(
            (-(z[0] * read(origin, 0)? + z[1] * read(origin, 1)? + z[2] * read(origin, 2)?))
                .to_le_bytes(),
        );
        for name in ["grayscale", "hidden", "filtered"] {
            body.push(u8::from(flag(trace, name, false)));
        }
        children.push(with_reference(0x2000_863b, body, trace)?);
    }
    if let Some(wallpaper) = object(fields, "wallpaper") {
        let has_reference = object(wallpaper, "file_reference").is_some();
        let mut body = vec![if has_reference { 0x12 } else { 0x11 }];
        body.extend(utf16(text(wallpaper, "legacy_file_path"))?);
        for name in ["grayscale", "hidden"] {
            body.push(u8::from(flag(wallpaper, name, false)));
        }
        children.push(with_reference(0x2000_874b, body, wallpaper)?);
    }
    Ok(())
}
