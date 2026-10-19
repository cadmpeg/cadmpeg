// SPDX-License-Identifier: Apache-2.0
//! Source-free saved-view and construction-plane records.

use super::*;
use cadmpeg_ir::presentation::ViewPresentation;
use cadmpeg_ir::report::loss::LossNote;
use std::collections::BTreeSet;

mod attributes;
mod images;
mod viewport;

pub(in crate::writer) struct ViewExport {
    pub(in crate::writer) settings: Vec<Vec<u8>>,
    pub(in crate::writer) regenerated: BTreeSet<String>,
    pub(in crate::writer) losses: Vec<LossNote>,
}

fn object<'a>(fields: &'a Map<String, Value>, name: &str) -> Option<&'a Map<String, Value>> {
    fields.get(name)?.as_object()
}

fn scalar(fields: &Map<String, Value>, name: &str) -> Result<f64, CodecError> {
    fields
        .get(name)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .ok_or_else(|| CodecError::malformed(format!("Rhino view {name} must be finite")))
}

fn flag(fields: &Map<String, Value>, name: &str, default: bool) -> bool {
    fields.get(name).and_then(Value::as_bool).unwrap_or(default)
}

fn text<'a>(fields: &'a Map<String, Value>, name: &str) -> &'a str {
    fields.get(name).and_then(Value::as_str).unwrap_or("")
}

fn vector(
    fields: &Map<String, Value>,
    name: &str,
    count: usize,
    output: &mut Vec<u8>,
) -> Result<(), CodecError> {
    let values = fields
        .get(name)
        .and_then(Value::as_array)
        .filter(|values| values.len() == count)
        .ok_or_else(|| {
            CodecError::malformed(format!("Rhino view {name} needs {count} coordinates"))
        })?;
    for value in values {
        output.extend(
            value
                .as_f64()
                .filter(|value| value.is_finite())
                .ok_or_else(|| {
                    CodecError::malformed(format!("Rhino view {name} has a nonfinite coordinate"))
                })?
                .to_le_bytes(),
        );
    }
    Ok(())
}

fn cplane(fields: &Map<String, Value>) -> Result<Vec<u8>, CodecError> {
    let mut body = vec![0x11];
    for name in [
        "plane_origin_mm",
        "plane_x_axis",
        "plane_y_axis",
        "plane_z_axis",
    ] {
        vector(fields, name, 3, &mut body)?;
    }
    vector(fields, "plane_equation_mm", 4, &mut body)?;
    for name in ["grid_spacing_mm", "snap_spacing_mm"] {
        body.extend(scalar(fields, name)?.to_le_bytes());
    }
    for name in ["grid_line_count", "thick_line_frequency"] {
        body.extend(integer(Some(fields), name, 0).to_le_bytes());
    }
    body.extend(utf16(text(fields, "name"))?);
    body.push(u8::from(flag(fields, "depth_buffer", true)));
    crc_chunk_with_direct(0x2000_813b, &body, &body)
}

fn window(fields: &Map<String, Value>) -> Result<Vec<u8>, CodecError> {
    let mut body = vec![0x11];
    body.extend(i32::from(flag(fields, "maximized", false)).to_le_bytes());
    for name in ["left", "right", "top", "bottom"] {
        body.extend(scalar(fields, name)?.to_le_bytes());
    }
    let floating = fields
        .get("floating_viewport")
        .and_then(Value::as_u64)
        .and_then(|value| u8::try_from(value).ok())
        .ok_or_else(|| CodecError::malformed("Rhino floating viewport flag exceeds u8"))?;
    body.push(floating);
    crc_chunk_with_direct(0x2000_8b3b, &body, &body)
}

fn property<'a>(view: &'a ViewPresentation, name: &str) -> Option<&'a str> {
    view.properties
        .iter()
        .find_map(|(key, value)| (key.as_str() == name).then_some(value.as_str()))
}

fn encode_view(
    ir: &CadIr,
    fields: &Map<String, Value>,
    view: Option<&ViewPresentation>,
) -> Result<Vec<u8>, CodecError> {
    let mut children = Vec::new();
    let name = view
        .and_then(|view| property(view, "name"))
        .unwrap_or_else(|| {
            if view.is_some() {
                ""
            } else {
                text(fields, "name")
            }
        });
    if !name.is_empty() {
        let body = utf16(name)?;
        children.push(crc_chunk_with_direct(0x2000_8a3b, &body, &body)?);
    }
    if let Some(cplane_fields) = object(fields, "construction_plane") {
        children.push(cplane(cplane_fields)?);
    }
    if let Some(viewport_fields) = object(fields, "viewport") {
        children.push(viewport::encode(ir, viewport_fields)?);
    }
    if let Some(window_fields) = object(fields, "window_position") {
        children.push(window(window_fields)?);
    }
    for (name, typecode) in [
        ("show_construction_grid", 0xa000_033b),
        ("show_construction_axes", 0xa000_043b),
        ("show_world_axes", 0xa000_053b),
    ] {
        let value = match view.and_then(|view| property(view, name)) {
            Some("true") => true,
            Some("false") => false,
            Some(_) => {
                return Err(CodecError::malformed(format!(
                    "Rhino view {name} must be true or false"
                )))
            }
            None => {
                if view.is_some() {
                    true
                } else {
                    flag(fields, name, true)
                }
            }
        };
        children.push(short_chunk(typecode, i64::from(value)));
    }
    if let Some(target) = fields
        .get("target_millimeters")
        .filter(|value| !value.is_null())
    {
        let fields = Map::from_iter([("target".into(), target.clone())]);
        let mut body = Vec::new();
        vector(&fields, "target", 3, &mut body)?;
        children.push(crc_chunk_with_direct(0x2000_883b, &body, &body)?);
    }
    let legacy = if let Some(view) = view {
        view.display_mode.as_deref().and_then(|mode| {
            mode.parse::<i64>().ok().or_else(|| {
                crate::presentation::parse_uuid_text(mode).and_then(|_| {
                    property(view, "rhino.legacy_display_mode")
                        .and_then(|value| value.parse::<i64>().ok())
                })
            })
        })
    } else {
        fields.get("legacy_display_mode").and_then(Value::as_i64)
    };
    if let Some(mode) = legacy {
        children.push(short_chunk(0xa000_093b, mode));
    }
    if let Some(attributes_fields) = object(fields, "attributes") {
        children.push(attributes::encode(
            attributes_fields,
            view.and_then(|view| view.display_mode.as_deref()),
        )?);
    }
    if object(fields, "attributes").is_none()
        && view
            .and_then(|view| view.display_mode.as_deref())
            .and_then(crate::presentation::parse_uuid_text)
            .is_some()
    {
        let defaults = Map::from_iter([
            ("view_type".into(), Value::from(0)),
            ("width".into(), Value::from(0.0)),
            ("height".into(), Value::from(0.0)),
        ]);
        children.push(attributes::encode(
            &defaults,
            view.and_then(|view| view.display_mode.as_deref()),
        )?);
    }
    images::append(fields, &mut children)?;
    children.push(short_chunk(TCODE_ENDOFTABLE, 0));
    crc_chunk_with_direct(0x2000_803b, &children.concat(), &[])
}

fn list(typecode: u32, children: &[Vec<u8>]) -> Result<Vec<u8>, CodecError> {
    let count = wire_index(children.len())?.to_le_bytes();
    let mut body = count.to_vec();
    body.extend(children.concat());
    crc_chunk_with_direct(typecode, &body, &count)
}

pub(in crate::writer) fn settings(ir: &CadIr) -> Result<ViewExport, CodecError> {
    let mut result = ViewExport {
        settings: Vec::new(),
        regenerated: BTreeSet::new(),
        losses: Vec::new(),
    };
    let mut named = Vec::new();
    let mut active = Vec::new();
    let mut written_views = BTreeSet::new();
    let mut written_cameras = BTreeSet::new();
    for view in &ir.model.view_presentations {
        let unsupported_display = view.display_mode.as_deref().is_some_and(|mode| {
            mode.parse::<i64>().is_err() && crate::presentation::parse_uuid_text(mode).is_none()
        });
        let unsupported_properties = view.properties.keys().any(|key| {
            !matches!(
                key.as_str(),
                "name"
                    | "show_construction_grid"
                    | "show_construction_axes"
                    | "show_world_axes"
                    | "rhino.legacy_display_mode"
            )
        });
        let fallback_cannot_coexist = property(view, "rhino.legacy_display_mode").is_some()
            && view
                .display_mode
                .as_deref()
                .and_then(crate::presentation::parse_uuid_text)
                .is_none();
        if unsupported_display
            || fallback_cannot_coexist
            || unsupported_properties
            || view.selection_style.is_some()
            || view.line_width.is_some()
            || view.point_size.is_some()
            || view.expanded.is_some()
            || view.visible.is_some()
            || view.object.is_some()
        {
            result.losses.push(
                crate::loss::RhinoLossCode::WriterAttributesOmitted.note(format!(
                    "{}: view controls without exact Rhino saved-view fields are omitted",
                    view.id
                )),
            );
        }
    }
    for record in records(ir, "views") {
        let fields = record.fields()?;
        let view = fields
            .get("neutral_view")
            .and_then(Value::as_str)
            .and_then(|id| {
                ir.model
                    .view_presentations
                    .iter()
                    .find(|view| view.id.as_str() == id)
            });
        if ir.scope.model && view.is_none() {
            result.losses.push(
                crate::loss::RhinoLossCode::WriterAttributesOmitted.note(format!(
                    "{}: its current neutral view is absent",
                    record.id()
                )),
            );
            continue;
        }
        match encode_view(ir, &fields, view) {
            Ok(encoded) => {
                match fields.get("list_kind").and_then(Value::as_str) {
                    Some("active") => active.push(encoded),
                    Some("named") => named.push(encoded),
                    _ => return Err(CodecError::malformed("Rhino view has no valid list kind")),
                };
                result.regenerated.insert(record.id().to_owned());
                if let Some(view) = view {
                    written_views.insert(view.id.as_str());
                }
                if let Some(camera) = object(&fields, "viewport")
                    .and_then(|viewport| viewport.get("neutral_camera"))
                    .and_then(Value::as_str)
                {
                    written_cameras.insert(camera.to_owned());
                }
            }
            Err(CodecError::UnsupportedContent(reason)) => result.losses.push(
                crate::loss::RhinoLossCode::WriterAttributesOmitted
                    .note(format!("{}: {reason}", record.id())),
            ),
            Err(error) => return Err(error),
        }
    }
    for view in &ir.model.view_presentations {
        if written_views.contains(view.id.as_str()) {
            continue;
        }
        // A saved view can carry a name and display controls without a camera
        // viewport. Do not invent a camera-to-view association for authored
        // independent presentation states.
        named.push(encode_view(ir, &Map::new(), Some(view))?);
    }
    for document in &ir.model.presentation_documents {
        if !written_cameras.contains(document.id.as_str()) && !document.states().is_empty() {
            result.losses.push(
                crate::loss::RhinoLossCode::WriterAttributesOmitted.note(format!(
                    "{}: presentation states have no unambiguous Rhino viewport association",
                    document.id
                )),
            );
        }
    }
    if !named.is_empty() {
        result.settings.push(list(0x2000_8036, &named)?);
    }
    if !active.is_empty() {
        result.settings.push(list(0x2000_8037, &active)?);
    }
    let mut cplanes = Vec::new();
    for record in records(ir, "construction_planes") {
        let fields = record.fields()?;
        let plane = object(&fields, "value").ok_or_else(|| {
            CodecError::malformed("Rhino named construction plane has no typed value")
        })?;
        cplanes.push(cplane(plane)?);
        result.regenerated.insert(record.id().to_owned());
    }
    if !cplanes.is_empty() {
        result.settings.push(list(0x2000_8035, &cplanes)?);
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
