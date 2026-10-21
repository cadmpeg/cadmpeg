// SPDX-License-Identifier: Apache-2.0
//! Typed view attributes, page settings and clipping planes.

use super::{
    anonymous, crc_chunk_with_direct, flag, integer, object, scalar, text, utf16, vector,
    wire_index, CodecError, Map, Value,
};

fn optional_real(fields: &Map<String, Value>, name: &str, output: &mut Vec<u8>) {
    output.extend(
        fields
            .get(name)
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite())
            .unwrap_or(0.0)
            .to_le_bytes(),
    );
}

fn page(fields: Option<&Map<String, Value>>) -> Result<Vec<u8>, CodecError> {
    let empty = Map::new();
    let fields = fields.unwrap_or(&empty);
    let mut body = integer(Some(fields), "page_number", 0)
        .to_le_bytes()
        .to_vec();
    for name in ["width_mm", "height_mm"] {
        optional_real(fields, name, &mut body);
    }
    if fields.get("margins_mm").is_some() {
        vector(fields, "margins_mm", 4, &mut body)?;
    } else {
        body.extend([0; 32]);
    }
    body.extend(utf16(text(fields, "printer_name"))?);
    anonymous(1, 0, &body)
}

fn clip(fields: &Map<String, Value>) -> Result<Vec<u8>, CodecError> {
    let mut body = Vec::new();
    vector(fields, "equation_mm", 4, &mut body)?;
    body.extend(
        fields
            .get("plane_uuid")
            .and_then(Value::as_str)
            .and_then(crate::presentation::parse_uuid_text)
            .map_or([0; 16], crate::wire::Uuid::to_wire),
    );
    body.push(u8::from(flag(fields, "enabled", false)));
    let depth = fields.get("depth_mm").and_then(Value::as_f64);
    if let Some(depth) = depth {
        body.extend(depth.to_le_bytes());
        body.push(u8::from(flag(fields, "depth_enabled", false)));
        anonymous(1, 3, &body)
    } else {
        anonymous(1, 0, &body)
    }
}

pub(super) fn encode(
    fields: &Map<String, Value>,
    current_display: Option<&str>,
) -> Result<Vec<u8>, CodecError> {
    let display = current_display
        .and_then(crate::presentation::parse_uuid_text)
        .or_else(|| {
            fields
                .get("display")
                .and_then(Value::as_str)
                .and_then(crate::presentation::parse_uuid_text)
        });
    let mut minor = if object(fields, "page_settings").is_some() || display.is_some() {
        2
    } else {
        1
    };
    if flag(fields, "projection_locked", false) {
        minor = minor.max(3);
    }
    let clips = fields
        .get("clipping_planes")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    if !clips.is_empty() {
        minor = minor.max(4);
    }
    if fields
        .get("named_view_uuid")
        .is_some_and(|value| !value.is_null())
    {
        minor = minor.max(5);
    }
    if flag(fields, "show_construction_z_axis", false) {
        minor = minor.max(6);
    }
    if fields
        .get("focal_blur_distance_mm")
        .is_some_and(|value| !value.is_null())
    {
        minor = minor.max(7);
    }
    if fields
        .get("rendering_size_pixels")
        .is_some_and(|value| !value.is_null())
    {
        minor = minor.max(8);
    }
    if fields
        .get("section_behavior")
        .is_some_and(|value| !value.is_null())
    {
        minor = minor.max(9);
    }
    let mut body = vec![0x10 + minor];
    body.extend(integer(Some(fields), "view_type", 0).to_le_bytes());
    for name in ["width", "height"] {
        body.extend(scalar(fields, name)?.to_le_bytes());
    }
    body.extend([0; 16]);
    body.extend([0; 48]);
    let mut children = Vec::new();
    if minor >= 2 {
        body.extend(display.map_or([0; 16], crate::wire::Uuid::to_wire));
        let start = body.len();
        body.extend(page(object(fields, "page_settings"))?);
        children.push(start..body.len());
    }
    if minor >= 3 {
        body.push(u8::from(flag(fields, "projection_locked", false)));
    }
    if minor >= 4 {
        body.extend(wire_index(clips.len())?.to_le_bytes());
        for plane in clips {
            let plane = plane
                .as_object()
                .ok_or_else(|| CodecError::malformed("Rhino clipping plane has no typed fields"))?;
            let start = body.len();
            body.extend(clip(plane)?);
            children.push(start..body.len());
        }
    }
    if minor >= 5 {
        body.extend(
            fields
                .get("named_view_uuid")
                .and_then(Value::as_str)
                .and_then(crate::presentation::parse_uuid_text)
                .map_or([0; 16], crate::wire::Uuid::to_wire),
        );
    }
    if minor >= 6 {
        body.push(u8::from(flag(fields, "show_construction_z_axis", false)));
    }
    if minor >= 7 {
        for name in [
            "focal_blur_distance_mm",
            "focal_blur_aperture",
            "focal_blur_jitter",
        ] {
            optional_real(fields, name, &mut body);
        }
        for name in ["focal_blur_sample_count", "focal_blur_mode"] {
            body.extend(integer(Some(fields), name, 0).to_le_bytes());
        }
    }
    if minor >= 8 {
        let pixels = fields
            .get("rendering_size_pixels")
            .and_then(Value::as_array)
            .filter(|values| values.len() == 2)
            .ok_or_else(|| CodecError::malformed("Rhino render size needs two pixels"))?;
        for value in pixels {
            body.extend(
                value
                    .as_i64()
                    .and_then(|value| i32::try_from(value).ok())
                    .ok_or_else(|| CodecError::malformed("Rhino render size exceeds i32"))?
                    .to_le_bytes(),
            );
        }
    }
    if minor >= 9 {
        body.push(
            fields
                .get("section_behavior")
                .and_then(Value::as_u64)
                .and_then(|value| u8::try_from(value).ok())
                .ok_or_else(|| CodecError::malformed("Rhino section behavior exceeds u8"))?,
        );
    }
    let ranges = crate::chunks::direct_checksum_ranges(&(0..body.len()), &children)
        .map_err(CodecError::malformed)?;
    let direct = (&ranges)
        .into_iter()
        .flat_map(|range| body[range].iter().copied())
        .collect::<Vec<_>>();
    crc_chunk_with_direct(0x2000_8c3b, &body, &direct)
}
