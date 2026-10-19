// SPDX-License-Identifier: Apache-2.0
//! Camera frame reconstruction from the current neutral axis-angle pose.

use super::*;
use cadmpeg_ir::presentation::PresentationStateKind;

fn pose(
    ir: &CadIr,
    fields: &Map<String, Value>,
) -> Result<Option<([f64; 3], [[f64; 3]; 3])>, CodecError> {
    let Some(id) = fields.get("neutral_camera").and_then(Value::as_str) else {
        return Ok(None);
    };
    let document = ir
        .model
        .presentation_documents
        .iter()
        .find(|document| document.id.as_str() == id)
        .ok_or_else(|| {
            CodecError::UnsupportedContent(format!(
                "camera {id} is absent from the current document"
            ))
        })?;
    let cameras = document
        .states()
        .iter()
        .filter_map(|state| match &state.kind {
            PresentationStateKind::Camera(camera) => Some(camera),
            _ => None,
        })
        .collect::<Vec<_>>();
    let [camera] = cameras.as_slice() else {
        return Err(CodecError::UnsupportedContent(format!(
            "camera {id} does not have one current pose"
        )));
    };
    let position = camera
        .position
        .ok_or_else(|| {
            CodecError::UnsupportedContent(format!("camera {id} has no current position"))
        })?
        .get();
    let [x, y, z, angle] = camera
        .orientation
        .ok_or_else(|| {
            CodecError::UnsupportedContent(format!("camera {id} has no current orientation"))
        })?
        .get();
    let norm = x.hypot(y).hypot(z);
    if !norm.is_finite() || norm == 0.0 {
        return Err(CodecError::malformed(
            "Rhino camera orientation has no rotation axis",
        ));
    }
    let [x, y, z] = [x / norm, y / norm, z / norm];
    let (sin, cos) = angle.sin_cos();
    let one = 1.0 - cos;
    // Columns of the local-camera to document rotation.
    let axes = [
        [
            cos + x * x * one,
            y * x * one + z * sin,
            z * x * one - y * sin,
        ],
        [
            x * y * one - z * sin,
            cos + y * y * one,
            z * y * one + x * sin,
        ],
        [
            x * z * one + y * sin,
            y * z * one - x * sin,
            cos + z * z * one,
        ],
    ];
    Ok(Some((position, axes)))
}

pub(super) fn encode(ir: &CadIr, fields: &Map<String, Value>) -> Result<Vec<u8>, CodecError> {
    let pose = pose(ir, fields)?;
    let mut body = vec![0x15];
    for name in ["camera_valid", "frustum_valid", "port_valid"] {
        body.extend(i32::from(flag(fields, name, false)).to_le_bytes());
    }
    body.extend(integer(Some(fields), "projection", 1).to_le_bytes());
    if let Some((position, [x, y, z])) = pose {
        for vector in [position, z.map(|value| -value), y, x, y, z] {
            for coordinate in vector {
                body.extend(coordinate.to_le_bytes());
            }
        }
    } else {
        for name in [
            "camera_location_mm",
            "camera_direction",
            "camera_up",
            "camera_x_axis",
            "camera_y_axis",
            "camera_z_axis",
        ] {
            vector(fields, name, 3, &mut body)?;
        }
    }
    vector(fields, "frustum_mm", 6, &mut body)?;
    let port = fields
        .get("port")
        .and_then(Value::as_array)
        .filter(|values| values.len() == 6)
        .ok_or_else(|| CodecError::malformed("Rhino viewport needs six port coordinates"))?;
    for coordinate in port {
        body.extend(
            coordinate
                .as_i64()
                .and_then(|value| i32::try_from(value).ok())
                .ok_or_else(|| CodecError::malformed("Rhino port coordinate exceeds i32"))?
                .to_le_bytes(),
        );
    }
    body.extend(
        fields
            .get("source_uuid")
            .and_then(Value::as_str)
            .and_then(crate::presentation::parse_uuid_text)
            .map_or([0; 16], crate::wire::Uuid::to_wire),
    );
    for name in [
        "camera_up_locked",
        "camera_direction_locked",
        "camera_location_locked",
        "frustum_left_right_symmetric",
        "frustum_top_bottom_symmetric",
    ] {
        body.push(u8::from(flag(fields, name, false)));
    }
    if fields
        .get("target_millimeters")
        .is_some_and(|value| !value.is_null())
    {
        vector(fields, "target_millimeters", 3, &mut body)?;
    } else {
        // Minor 2 is sufficient when the source has no target/frame/scale.
        // Preserve absence rather than invent an aim point.
        body[0] = 0x12;
        return crc_chunk_with_direct(0x2000_823b, &body, &body);
    }
    if let Some(valid) = fields.get("camera_frame_valid").and_then(Value::as_bool) {
        body.push(u8::from(valid));
    } else {
        body[0] = 0x13;
        return crc_chunk_with_direct(0x2000_823b, &body, &body);
    }
    if fields
        .get("view_scale")
        .is_some_and(|value| !value.is_null())
    {
        vector(fields, "view_scale", 3, &mut body)?;
    } else {
        body[0] = 0x14;
    }
    crc_chunk_with_direct(0x2000_823b, &body, &body)
}
