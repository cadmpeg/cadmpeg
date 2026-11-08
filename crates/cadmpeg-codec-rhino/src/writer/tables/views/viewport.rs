// SPDX-License-Identifier: Apache-2.0
//! Camera frame reconstruction from the current neutral axis-angle pose.

use super::super::super::WriteError;
use super::{crc_chunk_with_direct, flag, integer, vector, Map, Value};
use cadmpeg_ir::presentation::{PresentationDocument, PresentationStateKind};
use cadmpeg_ir::report::loss::LossNote;

type CameraPose = ([f64; 3], [[f64; 3]; 3]);

fn pose(document: Option<&PresentationDocument>) -> Result<Option<CameraPose>, WriteError> {
    let Some(document) = document else {
        return Ok(None);
    };
    let id = &document.id;
    let cameras = document
        .states()
        .iter()
        .filter_map(|state| match &state.kind {
            PresentationStateKind::Camera(camera) => Some(camera),
            PresentationStateKind::Native(_) => None,
        })
        .collect::<Vec<_>>();
    let [camera] = cameras.as_slice() else {
        return Err(WriteError::Omit(format!(
            "camera {id} does not have one current pose"
        )));
    };
    let position = camera
        .position
        .ok_or_else(|| WriteError::Omit(format!("camera {id} has no current position")))?
        .get();
    let [x, y, z, angle] = camera
        .orientation
        .ok_or_else(|| WriteError::Omit(format!("camera {id} has no current orientation")))?
        .get();
    let norm = x.hypot(y).hypot(z);
    if !norm.is_finite() || norm == 0.0 {
        return Err(WriteError::Omit(
            "Rhino camera orientation has no rotation axis".into(),
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

fn canonical_orientation(raw: [f64; 4]) -> [f64; 4] {
    let [x, y, z, angle] = raw;
    let norm = x.hypot(y).hypot(z);
    if !norm.is_finite() || norm == 0.0 {
        return raw;
    }
    let axis = [x / norm, y / norm, z / norm];
    let (sin, cos) = angle.sin_cos();
    let one = 1.0 - cos;
    let axes = [
        [
            cos + axis[0] * axis[0] * one,
            axis[1] * axis[0] * one + axis[2] * sin,
            axis[2] * axis[0] * one - axis[1] * sin,
        ],
        [
            axis[0] * axis[1] * one - axis[2] * sin,
            cos + axis[1] * axis[1] * one,
            axis[2] * axis[1] * one + axis[0] * sin,
        ],
        [
            axis[0] * axis[2] * one + axis[1] * sin,
            axis[1] * axis[2] * one - axis[0] * sin,
            cos + axis[2] * axis[2] * one,
        ],
    ];
    let camera_x = axes[2];
    let camera_y = axes[1];
    let camera_z = axes[0];
    let matrix = [
        [camera_x[0], camera_y[0], camera_z[0]],
        [camera_x[1], camera_y[1], camera_z[1]],
        [camera_x[2], camera_y[2], camera_z[2]],
    ];
    let trace = matrix[0][0] + matrix[1][1] + matrix[2][2];
    let (w, recovered) = if trace > 0.0 {
        let scale = (trace + 1.0).sqrt() * 2.0;
        (
            scale / 4.0,
            [
                (matrix[2][1] - matrix[1][2]) / scale,
                (matrix[0][2] - matrix[2][0]) / scale,
                (matrix[1][0] - matrix[0][1]) / scale,
            ],
        )
    } else {
        let major_axis = if matrix[0][0] > matrix[1][1] && matrix[0][0] > matrix[2][2] {
            0
        } else if matrix[1][1] > matrix[2][2] {
            1
        } else {
            2
        };
        let next_axis = (major_axis + 1) % 3;
        let last_axis = (major_axis + 2) % 3;
        let scale = (1.0 + matrix[major_axis][major_axis]
            - matrix[next_axis][next_axis]
            - matrix[last_axis][last_axis])
            .sqrt()
            * 2.0;
        let mut recovered = [0.0; 3];
        recovered[major_axis] = scale / 4.0;
        recovered[next_axis] =
            (matrix[next_axis][major_axis] + matrix[major_axis][next_axis]) / scale;
        recovered[last_axis] =
            (matrix[last_axis][major_axis] + matrix[major_axis][last_axis]) / scale;
        (
            (matrix[last_axis][next_axis] - matrix[next_axis][last_axis]) / scale,
            recovered,
        )
    };
    let norm = recovered[0]
        .mul_add(
            recovered[0],
            recovered[1].mul_add(recovered[1], recovered[2] * recovered[2]),
        )
        .sqrt();
    if norm <= f64::EPSILON {
        return [0.0, 0.0, 1.0, 0.0];
    }
    let angle = 2.0 * norm.atan2(w);
    [
        recovered[0] / norm,
        recovered[1] / norm,
        recovered[2] / norm,
        angle,
    ]
}

pub(super) fn orientation_userdata(
    document: Option<&PresentationDocument>,
) -> Result<Option<Vec<u8>>, WriteError> {
    let Some(document) = document else {
        return Ok(None);
    };
    let camera = document
        .states()
        .iter()
        .find_map(|state| match &state.kind {
            PresentationStateKind::Camera(camera) => Some(camera),
            PresentationStateKind::Native(_) => None,
        });
    let Some(orientation) = camera.and_then(|camera| camera.orientation) else {
        return Ok(None);
    };
    let raw = orientation.get();
    if canonical_orientation(raw) == raw {
        return Ok(None);
    }
    let [x, y, z, angle] = raw;
    let text = format!("{x:e},{y:e},{z:e},{angle:e}");
    let strings = [("cadmpeg:camera-orientation".to_owned(), text)];
    let userdata = super::super::super::user_string_chunk(&strings)
        .map_err(|error| WriteError::Fail(error))?;
    let mut body = userdata;
    body.extend(super::super::super::short_chunk(0x8002_7fff, 0));
    Ok(Some(super::super::super::crc_chunk_with_direct(
        0x2000_8d3b,
        &body,
        &[],
    )?))
}

pub(super) fn reduced_state_order(document: Option<&PresentationDocument>) -> Option<LossNote> {
    let document = document?;
    let shifted = document
        .states()
        .iter()
        .enumerate()
        .any(|(index, state)| u64::from(state.order) != u64::from(index as u32));
    if shifted {
        Some(
            crate::loss::RhinoLossCode::WriterPresentationStateOrderReduced.note(format!(
                "camera {} presentation state order is stored at its sequence position",
                document.id
            )),
        )
    } else {
        None
    }
}

pub(super) fn encode(
    fields: &Map<String, Value>,
    camera: Option<&PresentationDocument>,
) -> Result<Vec<u8>, WriteError> {
    let pose = pose(camera)?;
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
        .ok_or_else(|| WriteError::Omit("Rhino viewport needs six port coordinates".into()))?;
    for coordinate in port {
        body.extend(
            coordinate
                .as_i64()
                .and_then(|value| i32::try_from(value).ok())
                .ok_or_else(|| WriteError::Omit("Rhino port coordinate exceeds i32".into()))?
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
        return Ok(crc_chunk_with_direct(0x2000_823b, &body, &body)?);
    }
    if let Some(valid) = fields.get("camera_frame_valid").and_then(Value::as_bool) {
        body.push(u8::from(valid));
    } else {
        body[0] = 0x13;
        return Ok(crc_chunk_with_direct(0x2000_823b, &body, &body)?);
    }
    if fields
        .get("view_scale")
        .is_some_and(|value| !value.is_null())
    {
        vector(fields, "view_scale", 3, &mut body)?;
    } else {
        body[0] = 0x14;
    }
    Ok(crc_chunk_with_direct(0x2000_823b, &body, &body)?)
}
