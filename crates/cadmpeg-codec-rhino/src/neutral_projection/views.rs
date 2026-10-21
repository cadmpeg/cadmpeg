// SPDX-License-Identifier: Apache-2.0
//! Saved views expose their names and admitted camera poses as neutral state.

use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::ids::{Identity, IdentityKey};
use cadmpeg_ir::native::NativeRecord;
use cadmpeg_ir::presentation::{
    CameraState, PresentationDocument, PresentationId, PresentationState, PresentationStateKind,
    ViewPresentation,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::units::{FiniteVector, NonzeroVector};
use cadmpeg_ir::CadIr;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

const EPS_CAMERA_FRAME: f64 = 1e-10;

fn vector(fields: &Map<String, Value>, key: &str) -> Option<[f64; 3]> {
    let values = fields.get(key)?.as_array()?;
    (values.len() == 3).then_some(())?;
    Some([
        values[0].as_f64()?,
        values[1].as_f64()?,
        values[2].as_f64()?,
    ])
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.into_iter().zip(b).map(|(a, b)| a * b).sum()
}

fn close(a: [f64; 3], b: [f64; 3]) -> bool {
    a.into_iter()
        .zip(b)
        .all(|(a, b)| (a - b).abs() <= EPS_CAMERA_FRAME)
}

fn orientation(viewport: &Map<String, Value>) -> Option<NonzeroVector<4>> {
    let camera_x = vector(viewport, "camera_x_axis")?;
    let camera_y = vector(viewport, "camera_y_axis")?;
    let camera_z = vector(viewport, "camera_z_axis")?;
    // This is admission of the stored orthonormal camera frame, not a repair
    // of an inconsistent source frame or a geometric tolerance substitution.
    if [camera_x, camera_y, camera_z]
        .into_iter()
        .any(|axis| (dot(axis, axis) - 1.0).abs() > EPS_CAMERA_FRAME)
        || [
            dot(camera_x, camera_y),
            dot(camera_x, camera_z),
            dot(camera_y, camera_z),
        ]
        .into_iter()
        .any(|value| value.abs() > EPS_CAMERA_FRAME)
        || !close(
            [
                camera_x[1] * camera_y[2] - camera_x[2] * camera_y[1],
                camera_x[2] * camera_y[0] - camera_x[0] * camera_y[2],
                camera_x[0] * camera_y[1] - camera_x[1] * camera_y[0],
            ],
            camera_z,
        )
        || !close(
            vector(viewport, "camera_direction")?,
            camera_z.map(|value| -value),
        )
        || !close(vector(viewport, "camera_up")?, camera_y)
    {
        return None;
    }
    let matrix = [
        [camera_x[0], camera_y[0], camera_z[0]],
        [camera_x[1], camera_y[1], camera_z[1]],
        [camera_x[2], camera_y[2], camera_z[2]],
    ];
    let trace = matrix[0][0] + matrix[1][1] + matrix[2][2];
    let (w, axis) = if trace > 0.0 {
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
        let mut axis = [0.0; 3];
        axis[major_axis] = scale / 4.0;
        axis[next_axis] = (matrix[next_axis][major_axis] + matrix[major_axis][next_axis]) / scale;
        axis[last_axis] = (matrix[last_axis][major_axis] + matrix[major_axis][last_axis]) / scale;
        (
            (matrix[last_axis][next_axis] - matrix[next_axis][last_axis]) / scale,
            axis,
        )
    };
    let norm = dot(axis, axis).sqrt();
    if norm <= f64::EPSILON {
        return NonzeroVector::new([0.0, 0.0, 1.0, 0.0]);
    }
    let angle = 2.0 * norm.atan2(w);
    NonzeroVector::new([axis[0] / norm, axis[1] / norm, axis[2] / norm, angle])
}

fn id(kind: &str, ordinal: usize) -> Result<PresentationId, CodecError> {
    let key =
        IdentityKey::try_new(format!("{kind}-{ordinal:06}")).map_err(CodecError::malformed)?;
    Ok(PresentationId::compose(
        &cadmpeg_ir::identity_namespace!("rhino", "model", "view"),
        key,
    ))
}

fn property(
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    name: &str,
    value: String,
) -> Result<(), CodecError> {
    let name = cadmpeg_core::text::NonBlankString::new(name)
        .ok_or_else(|| CodecError::malformed("Rhino view property name is blank"))?;
    properties.insert(name, value);
    Ok(())
}

pub(super) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
) -> Result<Vec<LossNote>, CodecError> {
    let mut sources = Vec::new();
    if let Some(records) = ir
        .native
        .namespace("rhino")
        .and_then(|namespace| namespace.arenas().get("views"))
    {
        for record in records {
            sources.push((record.id().to_owned(), record.copy_fields(ctx)?));
        }
    }
    // Copy source records before editing the neutral arenas; the copy is an
    // ephemeral decoder projection, never retained as another content home.
    let mut replacements = Vec::new();
    let mut losses = Vec::new();
    for (ordinal, (native_id, mut fields)) in sources.into_iter().enumerate() {
        let view_id = id("view", ordinal)?;
        let mut properties = BTreeMap::new();
        if let Some(name) = fields
            .get("name")
            .and_then(|value| value.as_str().map(str::to_owned))
            .filter(|name| !name.trim().is_empty())
        {
            property(&mut properties, "name", name)?;
        }
        for name in [
            "show_construction_grid",
            "show_construction_axes",
            "show_world_axes",
        ] {
            if let Some(value) = fields.get(name).and_then(Value::as_bool) {
                property(&mut properties, name, value.to_string())?;
            }
        }
        let legacy_mode = fields
            .get("legacy_display_mode")
            .and_then(serde_json::Value::as_i64);
        let modern_mode = fields
            .get("attributes")
            .and_then(Value::as_object)
            .and_then(|attributes| attributes.get("display"))
            .and_then(|value| value.as_str().map(str::to_owned));
        if modern_mode.is_some() {
            if let Some(legacy) = legacy_mode {
                property(
                    &mut properties,
                    "rhino.legacy_display_mode",
                    legacy.to_string(),
                )?;
            }
        }
        let display_mode = modern_mode.or_else(|| legacy_mode.map(|value| value.to_string()));
        let order = u32::try_from(ordinal).map_err(CodecError::malformed)?;
        ctx.charge_entities(1, "Rhino neutral views")?;
        ir.model.view_presentations.push(ViewPresentation {
            id: view_id.clone(),
            object: None,
            order,
            expanded: None,
            visible: None,
            display_mode,
            selection_style: None,
            line_width: None,
            point_size: None,
            properties,
            native_ref: Some(native_id.clone()),
        });
        fields.insert("neutral_view".into(), Value::String(view_id.to_string()));
        if let Some(viewport) = fields.get_mut("viewport").and_then(Value::as_object_mut) {
            let camera = vector(viewport, "camera_location_mm")
                .and_then(FiniteVector::new)
                .zip(orientation(viewport));
            if let Some((position, orientation)) = camera {
                let camera_id = id("camera", ordinal)?;
                let mut document = PresentationDocument::new(camera_id.clone());
                document.native_ref = Some(native_id.clone());
                document
                    .set_states(vec![PresentationState {
                        kind: PresentationStateKind::Camera(CameraState {
                            position: Some(position),
                            orientation: Some(orientation),
                            properties: BTreeMap::new(),
                        }),
                        order: 0,
                        attributes: BTreeMap::new(),
                        assets: Vec::new(),
                    }])
                    .map_err(CodecError::malformed)?;
                ctx.charge_entities(1, "Rhino neutral cameras")?;
                ir.model.presentation_documents.push(document);
                viewport.insert(
                    "neutral_camera".into(),
                    Value::String(camera_id.to_string()),
                );
            } else {
                losses.push(crate::loss::RhinoLossCode::PresentationRecordDropped.note(format!("{native_id}: camera frame has no admitted neutral rigid pose; typed viewport retained")));
            }
        }
        replacements.push(NativeRecord::new(
            Identity::new(native_id).map_err(CodecError::malformed)?,
            fields,
        )?);
    }
    if !replacements.is_empty() {
        ir.native
            .namespace_mut("rhino")
            .arenas_mut()
            .insert("views".into(), replacements);
    }
    Ok(losses)
}

#[cfg(test)]
mod tests;
