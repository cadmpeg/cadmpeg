// SPDX-License-Identifier: Apache-2.0
//! Saved views expose their names and admitted camera poses as neutral state.

use crate::views::{CameraProjection, ViewProjection};
use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::ids::IdentityKey;
use cadmpeg_ir::presentation::{
    CameraState, PresentationDocument, PresentationId, PresentationState, PresentationStateKind,
    ViewPresentation,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::units::{FiniteVector, NonzeroVector};
use cadmpeg_ir::CadIr;
use std::collections::BTreeMap;

const EPS_CAMERA_FRAME: f64 = 1e-10;

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.into_iter().zip(b).map(|(a, b)| a * b).sum()
}

fn close(a: [f64; 3], b: [f64; 3]) -> bool {
    a.into_iter()
        .zip(b)
        .all(|(a, b)| (a - b).abs() <= EPS_CAMERA_FRAME)
}

fn orientation(viewport: &CameraProjection) -> Option<NonzeroVector<4>> {
    let camera_x = viewport.x_axis.map(cadmpeg_ir::scalar::FiniteReal::get);
    let camera_y = viewport.y_axis.map(cadmpeg_ir::scalar::FiniteReal::get);
    let camera_z = viewport.z_axis.map(cadmpeg_ir::scalar::FiniteReal::get);
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
            viewport.direction.map(cadmpeg_ir::scalar::FiniteReal::get),
            camera_z.map(|value| -value),
        )
        || !close(
            viewport.up.map(cadmpeg_ir::scalar::FiniteReal::get),
            camera_y,
        )
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
    sources: Vec<ViewProjection>,
    ir: &mut CadIr,
) -> Result<Vec<LossNote>, CodecError> {
    for source in &sources {
        let _object = ctx.enter_nested("copy native field object")?;
        if source.has_attributes {
            let _attributes = ctx.enter_nested("copy native field object")?;
        }
        if source.camera.is_some() {
            let _viewport = ctx.enter_nested("copy native field object")?;
            let _vector = ctx.enter_nested("copy native field sequence")?;
        }
    }
    let mut losses = Vec::new();
    for (ordinal, source) in sources.into_iter().enumerate() {
        let native_id = source.native_id;
        let view_id = id("view", ordinal)?;
        let mut properties = BTreeMap::new();
        if !source.name.trim().is_empty() {
            property(&mut properties, "name", source.name)?;
        }
        for (name, value) in [
            ("show_construction_grid", source.guides.construction_grid),
            ("show_construction_axes", source.guides.construction_axes),
            ("show_world_axes", source.guides.world_axes),
        ] {
            property(&mut properties, name, value.to_string())?;
        }
        let legacy_mode = source.legacy_display_mode;
        let modern_mode = source.modern_display_mode;
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
        if let Some(viewport) = source.camera {
            let camera =
                FiniteVector::new(viewport.location.map(cadmpeg_ir::scalar::FiniteReal::get))
                    .zip(orientation(&viewport));
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
            } else {
                losses.push(crate::loss::RhinoLossCode::PresentationRecordDropped.note(format!("{native_id}: camera frame has no admitted neutral rigid pose; typed viewport retained")));
            }
        }
    }
    Ok(losses)
}

#[cfg(test)]
mod tests;
