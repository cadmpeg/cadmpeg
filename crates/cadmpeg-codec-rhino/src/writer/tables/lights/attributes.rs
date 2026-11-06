// SPDX-License-Identifier: Apache-2.0
//! Tagged attributes for light-table records.

use cadmpeg_ir::native::bytes::NativeBytes;
use serde_json::{Map, Value};

use crate::writer::WriteError;

fn field<'a>(fields: &'a Map<String, Value>, name: &str) -> Result<&'a Value, WriteError> {
    fields
        .get(name)
        .ok_or_else(|| WriteError::Omit(format!("light attribute {name} is missing")))
}

fn integer(fields: &Map<String, Value>, name: &str) -> Result<i32, WriteError> {
    field(fields, name)?
        .as_i64()
        .and_then(|value| i32::try_from(value).ok())
        .ok_or_else(|| WriteError::Omit(format!("light attribute {name} must fit i32")))
}

fn byte(fields: &Map<String, Value>, name: &str) -> Result<u8, WriteError> {
    field(fields, name)?
        .as_u64()
        .and_then(|value| u8::try_from(value).ok())
        .ok_or_else(|| WriteError::Omit(format!("light attribute {name} must fit u8")))
}

fn string<'a>(fields: &'a Map<String, Value>, name: &str) -> Result<&'a str, WriteError> {
    field(fields, name)?
        .as_str()
        .filter(|value| !value.contains('\0'))
        .ok_or_else(|| WriteError::Omit(format!("light attribute {name} is not writable")))
}

fn color(fields: &Map<String, Value>, name: &str) -> Result<[u8; 4], WriteError> {
    let value: NativeBytes<[u8; 4]> = serde_json::from_value(field(fields, name)?.clone())
        .map_err(|error| WriteError::Omit(format!("light attribute {name}: {error}")))?;
    Ok(value.into_inner())
}

fn absent_or_empty(value: &Value) -> bool {
    value.is_null() || value.as_array().is_some_and(Vec::is_empty)
}

fn linetype_index(ir: &cadmpeg_ir::CadIr, source: i32) -> Result<i32, WriteError> {
    if source < 0 {
        return Ok(source);
    }
    let mut matches = super::records(ir, "linetypes")
        .iter()
        .enumerate()
        .filter(|(_, record)| {
            record
                .field("archive_index")
                .and_then(|value| value.as_i64())
                == Some(i64::from(source))
        });
    let index = matches
        .next()
        .map(|(index, _)| index)
        .ok_or_else(|| WriteError::Omit(format!("light linetype index {source} is unresolved")))?;
    if matches.next().is_some() {
        return Err(WriteError::Omit(format!(
            "light linetype index {source} is ambiguous"
        )));
    }
    Ok(super::wire_index(index)?)
}

fn layer_index(ir: &cadmpeg_ir::CadIr, source: i32) -> Result<i32, WriteError> {
    if source < 0 {
        return Ok(source);
    }
    let mut source_records = super::records(ir, "layers").iter().filter(|record| {
        record
            .field("archive_index")
            .and_then(|value| value.as_i64())
            == Some(i64::from(source))
    });
    let record = source_records
        .next()
        .ok_or_else(|| WriteError::Omit(format!("light layer index {source} is unresolved")))?;
    if source_records.next().is_some() {
        return Err(WriteError::Omit(format!(
            "light layer index {source} is ambiguous"
        )));
    }
    let mut layers = ir
        .model
        .presentation_layers
        .iter()
        .enumerate()
        .filter(|(_, layer)| layer.native_ref.as_deref() == Some(record.id()));
    let index = layers
        .next()
        .map(|(index, _)| index)
        .ok_or_else(|| WriteError::Omit(format!("light layer index {source} is not emitted")))?;
    if layers.next().is_some() {
        return Err(WriteError::Omit(format!(
            "light layer index {source} has multiple outputs"
        )));
    }
    Ok(super::wire_index(index)?)
}

fn material_index(
    ir: &cadmpeg_ir::CadIr,
    materials: &super::super::materials::MaterialPlan,
    source: i32,
) -> Result<i32, WriteError> {
    if source < 0 {
        return Ok(source);
    }
    let mut records = super::records(ir, "materials").iter().filter(|record| {
        record
            .field("archive_index")
            .and_then(|value| value.as_i64())
            == Some(i64::from(source))
    });
    let record = records
        .next()
        .ok_or_else(|| WriteError::Omit(format!("light material index {source} is unresolved")))?;
    if records.next().is_some() {
        return Err(WriteError::Omit(format!(
            "light material index {source} is ambiguous"
        )));
    }
    match materials.native_targets(record.id()) {
        [index] => Ok(*index),
        [] => Err(WriteError::Omit(format!(
            "light material index {source} is not emitted"
        ))),
        _ => Err(WriteError::Omit(format!(
            "light material index {source} has multiple outputs"
        ))),
    }
}

fn group_index(ir: &cadmpeg_ir::CadIr, source: i32) -> Result<i32, WriteError> {
    let records = super::records(ir, "groups");
    let mut matching = records.iter().filter(|record| {
        record
            .field("archive_index")
            .and_then(|value| value.as_i64())
            == Some(i64::from(source))
    });
    let record = matching
        .next()
        .ok_or_else(|| WriteError::Omit(format!("light group index {source} is unresolved")))?;
    if matching.next().is_some() {
        return Err(WriteError::Omit(format!(
            "light group index {source} is ambiguous"
        )));
    }
    let index = records
        .iter()
        .filter(|record| super::super::native::group_resolves(ir, record))
        .position(|candidate| candidate.id() == record.id())
        .ok_or_else(|| WriteError::Omit(format!("light group index {source} is not emitted")))?;
    Ok(super::wire_index(index)?)
}

fn hatch_index(ir: &cadmpeg_ir::CadIr, source: i32) -> Result<i32, WriteError> {
    if source < 0 {
        return Ok(source);
    }
    let records = super::records(ir, "hatch_patterns");
    let mut matching = records.iter().filter(|record| {
        record
            .field("archive_index")
            .and_then(|value| value.as_i64())
            == Some(i64::from(source))
    });
    let record = matching.next().ok_or_else(|| {
        WriteError::Omit(format!("light hatch pattern index {source} is unresolved"))
    })?;
    if matching.next().is_some() {
        return Err(WriteError::Omit(format!(
            "light hatch pattern index {source} is ambiguous"
        )));
    }
    let export = super::super::hatches::patterns(ir)?;
    let index = records
        .iter()
        .filter(|record| export.written.contains(record.id()))
        .position(|candidate| candidate.id() == record.id())
        .ok_or_else(|| {
            WriteError::Omit(format!("light hatch pattern index {source} is not emitted"))
        })?;
    Ok(super::wire_index(index)?)
}

fn rendering_material(value: &Value) -> Result<Vec<u8>, WriteError> {
    let fields = value
        .as_object()
        .ok_or_else(|| WriteError::Omit("light rendering material must be an object".into()))?;
    let uuid = |name: &str| -> Result<[u8; 16], WriteError> {
        field(fields, name)?
            .as_str()
            .and_then(crate::presentation::parse_uuid_text)
            .map(crate::wire::Uuid::to_wire)
            .ok_or_else(|| WriteError::Omit(format!("light rendering material {name} is invalid")))
    };
    let mut body = uuid("plugin_uuid")?.to_vec();
    body.extend(uuid("front_material_uuid")?);
    body.extend(0_i32.to_le_bytes());
    let minor = match fields.get("material_source") {
        None | Some(Value::Null) => {
            if fields
                .get("back_material_uuid")
                .is_some_and(|value| !value.is_null())
            {
                return Err(WriteError::Omit(
                    "light rendering material back UUID needs a source".into(),
                ));
            }
            0
        }
        Some(_) => {
            let back = match fields.get("back_material_uuid") {
                None | Some(Value::Null) => [0; 16],
                Some(_) => uuid("back_material_uuid")?,
            };
            body.extend(back);
            body.extend([byte(fields, "material_source")?, 0, 0, 0]);
            1
        }
    };
    Ok(super::super::anonymous(1, minor, &body)?)
}

pub(super) struct EncodedAttributes {
    pub(super) body: Vec<u8>,
    pub(super) direct: Vec<u8>,
    pub(super) change: Option<String>,
}

pub(super) fn encode(
    value: &Value,
    ir: &cadmpeg_ir::CadIr,
    materials: &super::super::materials::MaterialPlan,
) -> Result<EncodedAttributes, WriteError> {
    const FIELDS: &[&str] = &[
        "source_uuid",
        "name",
        "url",
        "layer_index",
        "material_index",
        "linetype_index",
        "color",
        "visible",
        "object_mode",
        "decoration",
        "wire_density",
        "color_source",
        "linetype_source",
        "material_source",
        "plot_color_source",
        "plot_weight_source",
        "plot_color",
        "plot_weight_mm",
        "group_indexes",
        "display_materials",
        "active_space",
        "viewport_uuid",
        "display_order",
        "clipping_proof",
        "clipping_plane_uuids",
        "hatch_pattern_index",
        "section_hatch_scale",
        "section_hatch_rotation",
        "linetype_pattern_scale",
        "hatch_background",
        "hatch_boundary_visible",
        "detail_background_visible",
        "section_fill_rule",
        "clipping_plane_label_style",
        "rendering_materials",
        "rendering_mappings",
        "casts_shadows",
        "receives_shadows",
        "advanced_texture_preview",
        "user_strings",
        "attribute_user_strings",
        "custom_render_mesh",
        "mesh_modifiers",
    ];
    let fields = value
        .as_object()
        .ok_or_else(|| WriteError::Omit("light attributes must be an object".into()))?;
    if let Some(name) = fields.keys().find(|name| !FIELDS.contains(&name.as_str())) {
        return Err(WriteError::Omit(format!(
            "light attribute {name} has no encoder"
        )));
    }
    for name in [
        "rendering_mappings",
        "user_strings",
        "attribute_user_strings",
    ] {
        if fields
            .get(name)
            .is_some_and(|value| !absent_or_empty(value))
        {
            return Err(WriteError::Omit(format!(
                "light attribute {name} is not writable yet"
            )));
        }
    }
    for name in ["custom_render_mesh", "mesh_modifiers"] {
        if fields.get(name).is_some_and(|value| !value.is_null()) {
            return Err(WriteError::Omit(format!(
                "light attribute {name} is not writable yet"
            )));
        }
    }
    let id = crate::presentation::parse_uuid_text(string(fields, "source_uuid")?)
        .ok_or_else(|| WriteError::Omit("light attribute source_uuid is invalid".into()))?;
    let source_layer = integer(fields, "layer_index")?;
    let written_layer = layer_index(ir, source_layer)?;
    let source_material = integer(fields, "material_index")?;
    let written_material = material_index(ir, materials, source_material)?;
    let source_hatch = integer(fields, "hatch_pattern_index")?;
    let written_hatch = hatch_index(ir, source_hatch)?;
    let source_linetype = integer(fields, "linetype_index")?;
    let written_linetype = linetype_index(ir, source_linetype)?;
    let source_groups = field(fields, "group_indexes")?
        .as_array()
        .ok_or_else(|| WriteError::Omit("light attribute group_indexes must be a list".into()))?
        .iter()
        .map(|value| {
            value
                .as_i64()
                .and_then(|value| i32::try_from(value).ok())
                .ok_or_else(|| WriteError::Omit("light group index must fit i32".into()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let written_groups = source_groups
        .iter()
        .map(|index| group_index(ir, *index))
        .collect::<Result<Vec<_>, _>>()?;
    let mut body = vec![0x2d];
    body.extend(id.to_wire());
    body.extend(written_layer.to_le_bytes());
    for (tag, name) in [(1, "name"), (2, "url")] {
        body.push(tag);
        body.extend(super::utf16(string(fields, name)?)?);
    }
    body.push(3);
    body.extend(written_linetype.to_le_bytes());
    body.push(4);
    body.extend(written_material.to_le_bytes());
    let material_refs = field(fields, "rendering_materials")?
        .as_array()
        .ok_or_else(|| WriteError::Omit("light rendering_materials must be a list".into()))?;
    let mut rendering_child = None;
    if !material_refs.is_empty()
        || [
            "casts_shadows",
            "receives_shadows",
            "advanced_texture_preview",
        ]
        .iter()
        .any(|name| fields.get(*name).is_some_and(|value| !value.is_null()))
    {
        let flag = |name: &str, default: bool| -> Result<bool, WriteError> {
            match fields.get(name) {
                None | Some(Value::Null) => Ok(default),
                Some(value) => value
                    .as_bool()
                    .filter(|value| *value != default)
                    .ok_or_else(|| {
                        WriteError::Omit(format!(
                            "light attribute {name} must use its non-default boolean"
                        ))
                    }),
            }
        };
        let casts = flag("casts_shadows", true)?;
        let receives = flag("receives_shadows", true)?;
        let preview = flag("advanced_texture_preview", false)?;
        let mut rendering = super::wire_index(material_refs.len())?
            .to_le_bytes()
            .to_vec();
        let mut children = Vec::new();
        for material in material_refs {
            let start = rendering.len();
            rendering.extend(rendering_material(material)?);
            children.push(start..rendering.len());
        }
        rendering.extend(0_i32.to_le_bytes());
        rendering.extend([u8::from(casts), u8::from(receives), u8::from(preview)]);
        body.push(5);
        let start = body.len();
        body.extend(super::super::anonymous_with_children(
            1, 3, &rendering, &children,
        )?);
        rendering_child = Some(start..body.len());
    }
    for (tag, name) in [(6, "color"), (7, "plot_color")] {
        body.push(tag);
        body.extend(color(fields, name)?);
    }
    let weight = field(fields, "plot_weight_mm")?
        .as_f64()
        .filter(|value| value.is_finite())
        .ok_or_else(|| WriteError::Omit("light attribute plot_weight_mm must be finite".into()))?;
    body.push(8);
    body.extend(weight.to_le_bytes());
    body.extend([9, byte(fields, "decoration")?]);
    body.push(10);
    body.extend(integer(fields, "wire_density")?.to_le_bytes());
    let visible = field(fields, "visible")?
        .as_bool()
        .ok_or_else(|| WriteError::Omit("light attribute visible must be boolean".into()))?;
    body.extend([11, u8::from(visible)]);
    for (tag, name) in [
        (12, "object_mode"),
        (13, "color_source"),
        (14, "plot_color_source"),
        (15, "plot_weight_source"),
        (16, "material_source"),
        (17, "linetype_source"),
    ] {
        body.extend([tag, byte(fields, name)?]);
    }
    if !written_groups.is_empty() {
        body.push(18);
        body.extend(super::wire_index(written_groups.len())?.to_le_bytes());
        for index in &written_groups {
            body.extend(index.to_le_bytes());
        }
    }
    body.extend([19, byte(fields, "active_space")?]);
    if let Some(value) = fields.get("viewport_uuid").filter(|value| !value.is_null()) {
        let id = value
            .as_str()
            .and_then(crate::presentation::parse_uuid_text)
            .ok_or_else(|| WriteError::Omit("light attribute viewport_uuid is invalid".into()))?;
        body.push(20);
        body.extend(id.to_wire());
    }
    if let Some(value) = fields.get("display_materials") {
        let pairs = value.as_array().ok_or_else(|| {
            WriteError::Omit("light attribute display_materials must be a list".into())
        })?;
        if !pairs.is_empty() {
            let count = i32::try_from(pairs.len())
                .map_err(|_| WriteError::Omit("light display material count exceeds i32".into()))?;
            body.push(21);
            body.extend(count.to_le_bytes());
            for pair in pairs {
                let pair = pair
                    .as_array()
                    .filter(|pair| pair.len() == 2)
                    .ok_or_else(|| {
                        WriteError::Omit("light display material needs two UUIDs".into())
                    })?;
                for value in pair {
                    let id = value
                        .as_str()
                        .and_then(crate::presentation::parse_uuid_text)
                        .ok_or_else(|| {
                            WriteError::Omit("light display material UUID is invalid".into())
                        })?;
                    body.extend(id.to_wire());
                }
            }
        }
    }
    body.push(22);
    body.extend(integer(fields, "display_order")?.to_le_bytes());
    let proof = fields.get("clipping_proof").map_or(Ok(false), |value| {
        value.as_bool().ok_or_else(|| {
            WriteError::Omit("light attribute clipping_proof must be boolean".into())
        })
    })?;
    let ids = fields
        .get("clipping_plane_uuids")
        .map_or(Ok(&[][..]), |value| {
            value.as_array().map(Vec::as_slice).ok_or_else(|| {
                WriteError::Omit("light attribute clipping_plane_uuids must be a list".into())
            })
        })?;
    if proof || !ids.is_empty() {
        let count = i32::try_from(ids.len())
            .map_err(|_| WriteError::Omit("light clipping UUID count exceeds i32".into()))?;
        let mut list = count.to_le_bytes().to_vec();
        for value in ids {
            let id = value
                .as_str()
                .and_then(crate::presentation::parse_uuid_text)
                .ok_or_else(|| WriteError::Omit("light clipping UUID is invalid".into()))?;
            list.extend(id.to_wire());
        }
        body.extend([28, u8::from(proof)]);
        body.extend(super::super::anonymous(1, 0, &list)?);
    }
    body.push(30);
    body.extend(written_hatch.to_le_bytes());
    for (tag, name) in [
        (31, "section_hatch_scale"),
        (32, "section_hatch_rotation"),
        (33, "linetype_pattern_scale"),
    ] {
        if let Some(value) = fields.get(name) {
            let value = value
                .as_f64()
                .filter(|value| value.is_finite())
                .ok_or_else(|| {
                    WriteError::Omit(format!("light attribute {name} must be finite"))
                })?;
            body.push(tag);
            body.extend(value.to_le_bytes());
        }
    }
    if fields.contains_key("hatch_background") {
        body.push(34);
        body.extend(color(fields, "hatch_background")?);
    }
    if let Some(value) = fields.get("hatch_boundary_visible") {
        let value = value.as_bool().ok_or_else(|| {
            WriteError::Omit("light attribute hatch_boundary_visible must be boolean".into())
        })?;
        body.extend([35, u8::from(value)]);
    }
    for (tag, name) in [
        (37, "section_fill_rule"),
        (40, "clipping_plane_label_style"),
    ] {
        if fields.contains_key(name) {
            body.extend([tag, byte(fields, name)?]);
        }
    }
    if let Some(value) = fields
        .get("detail_background_visible")
        .filter(|value| !value.is_null())
    {
        if value != &Value::Bool(true) {
            return Err(WriteError::Omit(
                "light attribute detail_background_visible must be true when present".into(),
            ));
        }
        body.extend([42, 1]);
    }
    body.push(0);
    let mut changes = Vec::new();
    if source_layer != written_layer {
        changes.push(format!(
            "layer index {source_layer} is written as {written_layer}"
        ));
    }
    if source_material != written_material {
        changes.push(format!(
            "material index {source_material} is written as {written_material}"
        ));
    }
    if source_linetype != written_linetype {
        changes.push(format!(
            "linetype index {source_linetype} is written as {written_linetype}"
        ));
    }
    if source_hatch != written_hatch {
        changes.push(format!(
            "hatch pattern index {source_hatch} is written as {written_hatch}"
        ));
    }
    for (source, written) in source_groups.iter().zip(&written_groups) {
        if source != written {
            changes.push(format!("group index {source} is written as {written}"));
        }
    }
    let change = (!changes.is_empty()).then(|| changes.join(", "));
    let direct = if let Some(child) = rendering_child {
        let mut direct = body[..child.start].to_vec();
        direct.extend(&body[child.end..]);
        direct
    } else {
        body.clone()
    };
    Ok(EncodedAttributes {
        body,
        direct,
        change,
    })
}
