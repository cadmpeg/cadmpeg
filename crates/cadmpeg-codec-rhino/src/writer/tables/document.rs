// SPDX-License-Identifier: Apache-2.0
//! Properties regenerated through their current neutral attribute links.

use super::super::WriteError;
use super::{
    color, crc_chunk, integer, records, short_chunk, utf16, wire_index, CadIr, CodecError,
    LossNote, Map, RhinoLossCode, Value,
};
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};

struct MissingPositions<'a>(&'a [Option<AttributeValue>]);

impl std::fmt::Display for MissingPositions<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[")?;
        let mut separator = "";
        for (position, value) in self.0.iter().enumerate() {
            if value.is_none() {
                write!(f, "{separator}{position}")?;
                separator = ", ";
            }
        }
        f.write_str("]")
    }
}

fn unreadable_attribute_loss(attribute: &SourceAttribute) -> Option<LossNote> {
    attribute.values.iter().any(Option::is_none).then(|| {
        RhinoLossCode::WriterAttributesOmitted.note(format!(
            "attribute {} is omitted because its scalar target cannot hold None positions {}",
            attribute.id,
            MissingPositions(&attribute.values)
        ))
    })
}

pub(in crate::writer) fn uncarried_attribute_losses(ir: &CadIr, losses: &mut Vec<LossNote>) {
    for attribute in &ir.model.attributes {
        let document_carrier = attribute.native_ref.as_deref().is_some_and(|id| {
            ["revisions", "document_notes", "applications"]
                .into_iter()
                .any(|arena| records(ir, arena).iter().any(|record| record.id() == id))
        });
        if document_carrier {
            continue;
        }
        if let Some(loss) = unreadable_attribute_loss(attribute) {
            losses.push(loss);
        } else if attribute.values.iter().flatten().any(|value| {
            matches!(
                value,
                AttributeValue::Map(_)
                    | AttributeValue::Expression(_)
                    | AttributeValue::SubReference { .. }
                    | AttributeValue::ExternalReference { .. }
                    | AttributeValue::EmptyReference
                    | AttributeValue::Json(_)
            )
        }) {
            losses.push(RhinoLossCode::WriterAttributesOmitted.note(format!(
                "attribute {} has no exact Rhino attribute carrier for its structured value",
                attribute.id
            )));
        } else {
            losses.push(RhinoLossCode::WriterAttributesOmitted.note(format!(
                "attribute {} ({} value) has no Rhino attribute carrier and is omitted",
                attribute.id, attribute.name
            )));
        }
    }
}

fn current_fields(
    ir: &CadIr,
    arena: &str,
    record: &cadmpeg_ir::NativeRecord,
) -> Result<Map<String, Value>, WriteError> {
    let mut fields = record.fields()?;
    let prefix = format!("rhino.{arena}.");
    let mut linked = std::collections::BTreeSet::new();
    for attribute in ir
        .model
        .attributes
        .iter()
        .filter(|attribute| attribute.native_ref.as_deref() == Some(record.id()))
    {
        let role = attribute
            .name
            .strip_prefix(prefix.as_str())
            .filter(|role| match arena {
                "document_notes" => matches!(
                    *role,
                    "html" | "text" | "visible" | "window_rectangle" | "locked"
                ),
                "revisions" => matches!(
                    *role,
                    "created_by"
                        | "created_utc_fields"
                        | "last_edited_by"
                        | "last_edited_utc_fields"
                        | "revision_count"
                ),
                "applications" => matches!(*role, "name" | "url" | "details"),
                _ => false,
            })
            .ok_or_else(|| {
                WriteError::Omit(format!(
                    "attribute {} has no current Rhino {arena} field role",
                    attribute.id
                ))
            })?;
        if attribute.target != AttributeTarget::Document || !linked.insert(role) {
            return Err(WriteError::Omit(format!(
                "attribute {} has an invalid or duplicate current Rhino {arena}.{role} binding",
                attribute.id
            )));
        }
        let value = match attribute.values.as_slice() {
            [Some(AttributeValue::String(value))] => Value::String(value.clone()),
            [Some(AttributeValue::Boolean(value))] => Value::Bool(*value),
            [Some(AttributeValue::Integer(value))] => (*value).into(),
            [Some(AttributeValue::Float(value))] => {
                serde_json::to_value(value).map_err(CodecError::malformed)?
            }
            [Some(AttributeValue::Vector(values))] => {
                serde_json::to_value(values).map_err(CodecError::malformed)?
            }
            _ => {
                return Err(WriteError::Omit(format!(
                    "attribute {} has no single current Rhino {arena}.{role} field value",
                    attribute.id
                )))
            }
        };
        fields.insert(role.to_owned(), value);
    }
    Ok(fields)
}

fn boolean(fields: &Map<String, Value>, name: &str) -> Result<bool, WriteError> {
    fields
        .get(name)
        .and_then(Value::as_bool)
        .ok_or_else(|| WriteError::Omit(format!("Rhino document {name} needs a boolean value")))
}

fn text(fields: &Map<String, Value>, name: &str, output: &mut Vec<u8>) -> Result<(), WriteError> {
    let value = fields
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| WriteError::Omit(format!("Rhino document {name} needs a string value")))?;
    output.extend(utf16(value)?);
    Ok(())
}

fn integer_vector(
    fields: &Map<String, Value>,
    name: &str,
    count: usize,
    output: &mut Vec<u8>,
) -> Result<(), WriteError> {
    let values = fields
        .get(name)
        .and_then(Value::as_array)
        .filter(|values| values.len() == count)
        .ok_or_else(|| {
            WriteError::Omit(format!(
                "Rhino document {name} needs {count} integer fields"
            ))
        })?;
    for value in values {
        let value = value
            .as_i64()
            .or_else(|| {
                value
                    .as_f64()
                    .filter(|value| value.fract() == 0.0)
                    .and_then(cadmpeg_core::convert::truncate_f64_to_i64)
            })
            .and_then(|value| i32::try_from(value).ok())
            .ok_or_else(|| {
                WriteError::Omit(format!(
                    "Rhino document {name} field is outside its integer lane"
                ))
            })?;
        output.extend(value.to_le_bytes());
    }
    Ok(())
}

pub(in crate::writer) struct PropertyExport {
    pub(in crate::writer) records: Vec<Vec<u8>>,
    pub(in crate::writer) regenerated: std::collections::BTreeSet<String>,
    pub(in crate::writer) written: std::collections::BTreeSet<String>,
    pub(in crate::writer) losses: Vec<LossNote>,
    /// Held `document_settings` records whose `archive_file_name` is re-emitted.
    pub(in crate::writer) settings_written: std::collections::BTreeSet<String>,
    /// Held `previews` records re-emitted from their linked asset bytes.
    pub(in crate::writer) previews_written: std::collections::BTreeSet<String>,
}

fn property(arena: &str, fields: &Map<String, Value>) -> Result<Vec<u8>, WriteError> {
    let mut payload = vec![if arena == "document_notes" {
        0x11
    } else {
        0x10
    }];
    let typecode = match arena {
        "revisions" => {
            text(fields, "created_by", &mut payload)?;
            integer_vector(fields, "created_utc_fields", 8, &mut payload)?;
            text(fields, "last_edited_by", &mut payload)?;
            integer_vector(fields, "last_edited_utc_fields", 8, &mut payload)?;
            let count = fields
                .get("revision_count")
                .and_then(Value::as_i64)
                .and_then(|count| i32::try_from(count).ok())
                .ok_or_else(|| {
                    WriteError::Omit("Rhino document revision_count needs an i32 value".into())
                })?;
            payload.extend(count.to_le_bytes());
            0x2000_8021
        }
        "document_notes" => {
            payload.extend(i32::from(boolean(fields, "html")?).to_le_bytes());
            text(fields, "text", &mut payload)?;
            payload.extend(i32::from(boolean(fields, "visible")?).to_le_bytes());
            integer_vector(fields, "window_rectangle", 4, &mut payload)?;
            payload.push(u8::from(boolean(fields, "locked")?));
            0x2000_8022
        }
        _ => {
            for name in ["name", "url", "details"] {
                text(fields, name, &mut payload)?;
            }
            0x2000_8024
        }
    };
    Ok(crc_chunk(typecode, &payload)?)
}

pub(in crate::writer) fn properties(ir: &CadIr) -> Result<PropertyExport, WriteError> {
    let mut output = PropertyExport {
        records: Vec::new(),
        regenerated: std::collections::BTreeSet::new(),
        written: std::collections::BTreeSet::new(),
        losses: Vec::new(),
        settings_written: std::collections::BTreeSet::new(),
        previews_written: std::collections::BTreeSet::new(),
    };
    for arena in ["revisions", "document_notes", "applications"] {
        for record in records(ir, arena) {
            let mut unreadable = false;
            for attribute in ir
                .model
                .attributes
                .iter()
                .filter(|attribute| attribute.native_ref.as_deref() == Some(record.id()))
            {
                if let Some(loss) = unreadable_attribute_loss(attribute) {
                    output.losses.push(loss);
                    unreadable = true;
                }
            }
            if unreadable {
                continue;
            }
            let encoded =
                current_fields(ir, arena, record).and_then(|fields| property(arena, &fields));
            match encoded {
                Ok(encoded) => {
                    let unchanged = match property(arena, &record.fields()?) {
                        Ok(native) => native == encoded,
                        Err(WriteError::Omit(_)) => false,
                        Err(error) => return Err(error),
                    };
                    output.records.push(encoded);
                    if unchanged {
                        output.written.insert(record.id().to_owned());
                    } else {
                        output.regenerated.insert(record.id().to_owned());
                    }
                }
                Err(WriteError::Omit(reason)) => output.losses.push(
                    RhinoLossCode::WriterAttributesOmitted
                        .note(format!("{}: {reason}", record.id())),
                ),
                Err(error) => return Err(error),
            }
        }
    }
    for record in records(ir, "document_settings") {
        if let Some(value) = record
            .field("archive_file_name")
            .and_then(|value| value.as_str().map(str::to_owned))
        {
            output
                .records
                .push(crc_chunk(0x2000_8027, &utf16(&value)?)?);
            output.settings_written.insert(record.id().to_owned());
        }
    }
    let previews = previews(ir)?;
    output.records.extend(previews.records);
    output.previews_written = previews.written;
    output.losses.extend(previews.losses);
    Ok(output)
}

/// Preview-table records authored from the linked preview asset bytes.
///
/// A held preview record re-emits its linked asset's bytes verbatim under
/// its stored compression typecode, so embedded checksums stay valid.
struct PreviewExport {
    records: Vec<Vec<u8>>,
    written: std::collections::BTreeSet<String>,
    losses: Vec<LossNote>,
}

fn previews(ir: &CadIr) -> Result<PreviewExport, WriteError> {
    use cadmpeg_ir::assets::AssetContent;
    let mut output = PreviewExport {
        records: Vec::new(),
        written: std::collections::BTreeSet::new(),
        losses: Vec::new(),
    };
    for record in records(ir, "previews") {
        let fields = record.fields()?;
        let compressed = fields
            .get("compressed")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let mut linked = ir
            .model
            .assets
            .iter()
            .filter(|asset| asset.native_ref.as_deref() == Some(record.id()));
        let Some(asset) = linked.next() else {
            continue;
        };
        if linked.next().is_some() {
            output
                .losses
                .push(RhinoLossCode::WriterAttributesOmitted.note(format!(
                    "{}: more than one asset names this preview",
                    record.id()
                )));
            continue;
        }
        let AssetContent::Embedded { data } = &asset.content else {
            output
                .losses
                .push(RhinoLossCode::WriterAttributesOmitted.note(format!(
                    "asset {} has no embedded preview bytes and is not written",
                    asset.id
                )));
            continue;
        };
        let typecode = if compressed { 0x2000_8025 } else { 0x2000_8023 };
        output
            .records
            .push(super::super::crc_chunk(typecode, data.as_bytes())?);
        output.written.insert(record.id().to_owned());
    }
    for asset in &ir.model.assets {
        if !matches!(asset.content, AssetContent::Embedded { .. }) {
            continue;
        }
        if let Some(id) = asset.native_ref.as_deref() {
            if records(ir, "previews")
                .iter()
                .any(|record| record.id() == id)
                && !output.written.contains(id)
            {
                output
                    .losses
                    .push(RhinoLossCode::WriterAttributesOmitted.note(format!(
                        "asset {} names preview {id} with no written bytes",
                        asset.id
                    )));
            }
        }
    }
    Ok(output)
}

pub(in crate::writer) struct SettingsExport {
    pub(in crate::writer) records: Vec<Vec<u8>>,
    /// Held `document_settings` records re-emitted from a neutral resolution.
    pub(in crate::writer) regenerated: std::collections::BTreeSet<String>,
    /// Held `document_settings` records re-emitted from their typed fields.
    pub(in crate::writer) written: std::collections::BTreeSet<String>,
}

/// The settings-table records the writer authors from held typed fields.
pub(in crate::writer) struct TableSettingsExport {
    pub(in crate::writer) records: Vec<Vec<u8>>,
    /// Held native records re-emitted from their typed fields.
    pub(in crate::writer) written: std::collections::BTreeSet<String>,
    pub(in crate::writer) losses: Vec<LossNote>,
}

fn bytes4(fields: &Map<String, Value>, name: &str) -> Result<[u8; 4], WriteError> {
    let value: cadmpeg_ir::native::bytes::NativeBytes<[u8; 4]> =
        serde_json::from_value(fields.get(name).cloned().unwrap_or(Value::Null))
            .map_err(|_| WriteError::Omit(format!("Rhino setting {name} needs four bytes")))?;
    let bytes: &[u8] = value.as_ref();
    Ok([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn finite_field(fields: &Map<String, Value>, name: &str) -> Result<f64, WriteError> {
    fields
        .get(name)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .ok_or_else(|| WriteError::Omit(format!("Rhino setting {name} must be finite")))
}

fn flag_field(fields: &Map<String, Value>, name: &str) -> bool {
    fields.get(name).and_then(Value::as_bool).unwrap_or(false)
}

fn annotation_settings_record(fields: &Map<String, Value>) -> Result<Vec<u8>, WriteError> {
    let minor = if fields.get("use_dimension_layer").is_some()
        || fields.get("dimension_layer_uuid").is_some()
    {
        4
    } else if fields.get("model_space_annotation_scaling").is_some()
        || fields.get("layout_space_annotation_scaling").is_some()
    {
        3
    } else if fields.get("world_view_hatch_scale").is_some()
        || fields.get("hatch_scaling").is_some()
    {
        2
    } else if fields.get("world_view_text_scale").is_some()
        || fields.get("annotation_scaling").is_some()
    {
        1
    } else {
        0
    };
    let mut body = vec![0x10 | minor];
    for name in [
        "dimension_scale",
        "text_height_mm",
        "extension_line_extension_mm",
        "extension_line_offset_mm",
        "arrow_length_mm",
        "arrow_width_mm",
        "center_mark_mm",
    ] {
        body.extend(finite_field(fields, name)?.to_le_bytes());
    }
    body.extend(
        u32::try_from(integer(Some(fields), "dimension_units", 0))
            .map_err(|_| WriteError::Omit("Rhino dimension units exceed u32".into()))?
            .to_le_bytes(),
    );
    for name in [
        "arrow_type",
        "angular_units",
        "length_format",
        "angle_format",
    ] {
        body.extend(integer(Some(fields), name, 0).to_le_bytes());
    }
    body.extend(
        u32::try_from(integer(Some(fields), "obsolete_text_alignment", 0))
            .map_err(|_| WriteError::Omit("Rhino obsolete text alignment exceeds u32".into()))?
            .to_le_bytes(),
    );
    body.extend(integer(Some(fields), "resolution", 0).to_le_bytes());
    body.extend(utf16(
        fields
            .get("font_face")
            .and_then(Value::as_str)
            .ok_or_else(|| WriteError::Omit("Rhino setting font_face needs a string".into()))?,
    )?);
    if minor >= 1 {
        body.extend(finite_field(fields, "world_view_text_scale")?.to_le_bytes());
        body.push(u8::from(flag_field(fields, "annotation_scaling")));
    }
    if minor >= 2 {
        body.extend(finite_field(fields, "world_view_hatch_scale")?.to_le_bytes());
        body.push(u8::from(flag_field(fields, "hatch_scaling")));
    }
    if minor >= 3 {
        body.push(u8::from(flag_field(
            fields,
            "model_space_annotation_scaling",
        )));
        body.push(u8::from(flag_field(
            fields,
            "layout_space_annotation_scaling",
        )));
    }
    if minor >= 4 {
        body.push(u8::from(flag_field(fields, "use_dimension_layer")));
        body.extend(
            fields
                .get("dimension_layer_uuid")
                .and_then(Value::as_str)
                .and_then(crate::presentation::parse_uuid_text)
                .filter(|uuid| !uuid.is_nil())
                .map_or([0; 16], crate::wire::Uuid::to_wire),
        );
    }
    Ok(body)
}

fn grid_defaults_record(fields: &Map<String, Value>) -> Result<Vec<u8>, WriteError> {
    let mut body = vec![0x10];
    for name in ["grid_spacing_mm", "snap_spacing_mm"] {
        body.extend(finite_field(fields, name)?.to_le_bytes());
    }
    for name in ["grid_line_count", "thick_line_frequency"] {
        body.extend(integer(Some(fields), name, 0).to_le_bytes());
    }
    for name in ["show_grid", "show_grid_axes", "show_world_axes"] {
        body.extend(i32::from(flag_field(fields, name)).to_le_bytes());
    }
    Ok(body)
}

fn render_settings_record(
    fields: &Map<String, Value>,
    version: crate::RhinoArchiveVersion,
) -> Result<(Vec<u8>, Vec<std::ops::Range<usize>>), WriteError> {
    let modern_only = flag_field(fields, "transparent_background")
        || fields
            .get("rendering_source")
            .is_some_and(|value| !value.is_null())
        || fields.get("force_viewport_aspect_ratio").is_some()
        || fields
            .get("obsolete_focal_blur")
            .is_some_and(|value| !value.is_null())
        || ["specific_viewport", "named_view", "snapshot"]
            .iter()
            .any(|name| {
                fields
                    .get(*name)
                    .and_then(Value::as_str)
                    .is_some_and(|value| !value.is_empty())
            });
    if modern_only && version.value() < 60 {
        return Err(WriteError::Omit(
            "render settings need a post-50 form the target archive cannot state".into(),
        ));
    }
    // The modern grammar requires image dpi, unit system, and bottom color.
    // A record that holds them takes the modern form; a legacy-shaped record
    // keeps the legacy version its fields support.
    let modern_mandatory = ["image_dpi", "image_unit_system", "background_bottom_color"]
        .iter()
        .all(|name| fields.get(*name).is_some_and(|value| !value.is_null()));
    let modern = modern_only || (modern_mandatory && version.value() >= 60);
    if modern {
        let minor = if fields.get("force_viewport_aspect_ratio").is_some() {
            3
        } else if fields
            .get("rendering_source")
            .is_some_and(|value| !value.is_null())
            || fields
                .get("named_view")
                .is_some_and(|value| !value.is_null())
            || fields
                .get("specific_viewport")
                .is_some_and(|value| !value.is_null())
            || fields.get("snapshot").is_some_and(|value| !value.is_null())
        {
            2
        } else if fields
            .get("obsolete_focal_blur")
            .is_some_and(|value| !value.is_null())
        {
            1
        } else {
            0
        };
        let mut inner = Vec::new();
        inner.push(u8::from(flag_field(fields, "custom_image_size")));
        inner.extend(integer(Some(fields), "image_width_pixels", 0).to_le_bytes());
        inner.extend(integer(Some(fields), "image_height_pixels", 0).to_le_bytes());
        inner.extend(finite_field(fields, "image_dpi")?.to_le_bytes());
        inner.extend(
            u32::try_from(integer(Some(fields), "image_unit_system", 0))
                .map_err(|_| WriteError::Omit("Rhino image unit system exceeds u32".into()))?
                .to_le_bytes(),
        );
        inner.extend(bytes4(fields, "ambient_light")?);
        inner.extend(integer(Some(fields), "background_style", 0).to_le_bytes());
        inner.extend(bytes4(fields, "background_color")?);
        inner.extend(bytes4(fields, "background_bottom_color")?);
        inner.extend(utf16(
            fields
                .get("background_bitmap_path")
                .and_then(Value::as_str)
                .unwrap_or(""),
        )?);
        for name in [
            "use_hidden_lights",
            "depth_cue",
            "flat_shade",
            "render_backfaces",
            "render_points",
            "render_curves",
            "render_isoparams",
            "render_mesh_edges",
            "render_annotations",
            "scale_background_to_fit",
            "transparent_background",
        ] {
            inner.push(u8::from(flag_field(fields, name)));
        }
        inner.extend(integer(Some(fields), "antialias_style", 0).to_le_bytes());
        inner.extend(integer(Some(fields), "shadowmap_style", 0).to_le_bytes());
        let size = fields
            .get("shadowmap_size_pixels")
            .and_then(Value::as_array)
            .filter(|values| values.len() == 2)
            .ok_or_else(|| WriteError::Omit("Rhino shadowmap size needs two pixels".into()))?;
        for value in size {
            inner.extend(
                value
                    .as_i64()
                    .and_then(|value| i32::try_from(value).ok())
                    .ok_or_else(|| WriteError::Omit("Rhino shadowmap size exceeds i32".into()))?
                    .to_le_bytes(),
            );
        }
        inner.extend(finite_field(fields, "shadowmap_offset_mm")?.to_le_bytes());
        if minor >= 1 {
            let blur = fields
                .get("obsolete_focal_blur")
                .and_then(Value::as_array)
                .filter(|values| values.len() == 5)
                .ok_or_else(|| WriteError::Omit("Rhino focal blur needs five components".into()))?;
            let mut numbers = blur.iter().map(|value| value.as_f64().unwrap_or(0.0));
            for (position, value) in numbers.by_ref().enumerate() {
                // The endpoints persist as i32; the middle three persist as f64.
                if position == 0 || position == 4 {
                    if value.fract() != 0.0 || !(i32::MIN as f64..=i32::MAX as f64).contains(&value)
                    {
                        return Err(WriteError::Omit(
                            "Rhino focal blur endpoint exceeds i32".into(),
                        ));
                    }
                    inner.extend((value as i32).to_le_bytes());
                } else {
                    if !value.is_finite() {
                        return Err(WriteError::Omit(
                            "Rhino focal blur value is non-finite".into(),
                        ));
                    }
                    inner.extend(value.to_le_bytes());
                }
            }
        }
        if minor >= 2 {
            inner.extend(integer(Some(fields), "rendering_source", 0).to_le_bytes());
            for name in ["specific_viewport", "named_view", "snapshot"] {
                inner.extend(utf16(
                    fields.get(name).and_then(Value::as_str).unwrap_or(""),
                )?);
            }
        }
        if minor >= 3 {
            inner.push(u8::from(flag_field(fields, "force_viewport_aspect_ratio")));
        }
        let body = super::anonymous(1, minor, &inner)?;
        let child = 0..body.len();
        Ok((body, vec![child]))
    } else {
        let dpi = fields
            .get("image_dpi")
            .is_some_and(|value| !value.is_null());
        let units = fields
            .get("image_unit_system")
            .is_some_and(|value| !value.is_null());
        let bottom = fields
            .get("background_bottom_color")
            .is_some_and(|value| !value.is_null());
        let fit = flag_field(fields, "scale_background_to_fit");
        if dpi != units || (bottom && !(dpi && units)) || (fit && !(dpi && units && bottom)) {
            return Err(WriteError::Omit(
                "render settings mix present and absent legacy version fields".into(),
            ));
        }
        let version_number = if fit {
            103
        } else if bottom {
            102
        } else if dpi {
            101
        } else {
            100
        };
        let mut body = (version_number as i32).to_le_bytes().to_vec();
        body.extend(i32::from(flag_field(fields, "custom_image_size")).to_le_bytes());
        body.extend(integer(Some(fields), "image_width_pixels", 0).to_le_bytes());
        body.extend(integer(Some(fields), "image_height_pixels", 0).to_le_bytes());
        body.extend(bytes4(fields, "ambient_light")?);
        body.extend(integer(Some(fields), "background_style", 0).to_le_bytes());
        body.extend(bytes4(fields, "background_color")?);
        body.extend(utf16(
            fields
                .get("background_bitmap_path")
                .and_then(Value::as_str)
                .unwrap_or(""),
        )?);
        for name in [
            "use_hidden_lights",
            "depth_cue",
            "flat_shade",
            "render_backfaces",
            "render_points",
            "render_curves",
            "render_isoparams",
            "render_mesh_edges",
            "render_annotations",
        ] {
            body.extend(i32::from(flag_field(fields, name)).to_le_bytes());
        }
        body.extend(integer(Some(fields), "antialias_style", 0).to_le_bytes());
        body.extend(integer(Some(fields), "shadowmap_style", 0).to_le_bytes());
        let size = fields
            .get("shadowmap_size_pixels")
            .and_then(Value::as_array)
            .filter(|values| values.len() == 2)
            .ok_or_else(|| WriteError::Omit("Rhino shadowmap size needs two pixels".into()))?;
        for value in size {
            body.extend(
                value
                    .as_i64()
                    .and_then(|value| i32::try_from(value).ok())
                    .ok_or_else(|| WriteError::Omit("Rhino shadowmap size exceeds i32".into()))?
                    .to_le_bytes(),
            );
        }
        body.extend(finite_field(fields, "shadowmap_offset_mm")?.to_le_bytes());
        if version_number >= 101 {
            body.extend(finite_field(fields, "image_dpi")?.to_le_bytes());
            body.extend(
                u32::try_from(integer(Some(fields), "image_unit_system", 0))
                    .map_err(|_| WriteError::Omit("Rhino image unit system exceeds u32".into()))?
                    .to_le_bytes(),
            );
        }
        if version_number >= 102 {
            body.extend(bytes4(fields, "background_bottom_color")?);
        }
        if version_number >= 103 {
            body.push(u8::from(flag_field(fields, "scale_background_to_fit")));
        }
        Ok((body, Vec::new()))
    }
}

/// Author the annotation-settings, grid-defaults, and render-settings
/// records the decoder parses into typed fields, in the writer's millimetre
/// document units. Records the decoder retains without typed semantics stay
/// dropped with the shared native omission loss.
pub(in crate::writer) fn table_settings(
    ir: &CadIr,
    version: crate::RhinoArchiveVersion,
) -> Result<TableSettingsExport, WriteError> {
    let mut output = TableSettingsExport {
        records: Vec::new(),
        written: std::collections::BTreeSet::new(),
        losses: Vec::new(),
    };
    for record in records(ir, "annotation_settings") {
        let fields = record.fields()?;
        match annotation_settings_record(&fields) {
            Ok(body) => {
                output.records.push(crc_chunk(0x2000_8034, &body)?);
                output.written.insert(record.id().to_owned());
            }
            Err(WriteError::Omit(reason)) => output.losses.push(
                RhinoLossCode::WriterAttributesOmitted.note(format!("{}: {reason}", record.id())),
            ),
            Err(error) => return Err(error),
        }
    }
    for record in records(ir, "grid_defaults") {
        let fields = record.fields()?;
        match grid_defaults_record(&fields) {
            Ok(body) => {
                output.records.push(crc_chunk(0x2000_803f, &body)?);
                output.written.insert(record.id().to_owned());
            }
            Err(WriteError::Omit(reason)) => output.losses.push(
                RhinoLossCode::WriterAttributesOmitted.note(format!("{}: {reason}", record.id())),
            ),
            Err(error) => return Err(error),
        }
    }
    for record in records(ir, "render_settings") {
        let fields = record.fields()?;
        match render_settings_record(&fields, version) {
            Ok((body, children)) => {
                let ranges = crate::chunks::direct_checksum_ranges(&(0..body.len()), &children)
                    .map_err(|error| WriteError::Fail(CodecError::malformed(error.to_string())))?;
                let mut direct = Vec::new();
                for range in &ranges {
                    direct.extend_from_slice(&body[range.clone()]);
                }
                output
                    .records
                    .push(super::crc_chunk_with_direct(0x2000_803d, &body, &direct)?);
                output.written.insert(record.id().to_owned());
            }
            Err(WriteError::Omit(reason)) => output.losses.push(
                RhinoLossCode::WriterAttributesOmitted.note(format!("{}: {reason}", record.id())),
            ),
            Err(error) => return Err(error),
        }
    }
    Ok(output)
}

pub(in crate::writer) fn settings(
    ir: &CadIr,
    materials: &super::materials::MaterialPlan,
    losses: &mut Vec<LossNote>,
) -> Result<SettingsExport, CodecError> {
    let mut output = SettingsExport {
        records: Vec::new(),
        regenerated: std::collections::BTreeSet::new(),
        written: std::collections::BTreeSet::new(),
    };
    for setting in records(ir, "document_settings") {
        let fields = setting.fields()?;
        let mut neutral = false;
        let before = output.records.len();
        if let Some(url) = fields.get("model_url").and_then(Value::as_str) {
            output.records.push(crc_chunk(0x2000_8131, &utf16(url)?)?);
        }
        if let Some(value) = fields.get("current_wire_density").and_then(Value::as_i64) {
            output.records.push(short_chunk(0xa000_003c, value));
        }
        if let Some(index) = fields.get("current_font_index").and_then(Value::as_i64) {
            output.records.push(short_chunk(0xa000_0132, index));
        }
        if fields
            .get("current_color")
            .is_some_and(|value| !value.is_null())
        {
            let mut body = color(Some(&fields), "current_color")?.to_vec();
            body.extend(integer(Some(&fields), "current_color_source", 0).to_le_bytes());
            output.records.push(crc_chunk(0x2000_803a, &body)?);
        }
        if let Some(index) = fields.get("current_layer_index").and_then(Value::as_i64) {
            if let Some(record) = records(ir, "layers").iter().find(|record| {
                record
                    .field("archive_index")
                    .and_then(|value| value.as_i64())
                    == Some(index)
            }) {
                if let Some(index) = ir
                    .model
                    .presentation_layers
                    .iter()
                    .position(|layer| layer.native_ref.as_deref() == Some(record.id()))
                {
                    output
                        .records
                        .push(short_chunk(0xa000_0038, i64::from(wire_index(index)?)));
                    neutral = true;
                }
            }
        }
        if let Some(index) = fields.get("current_material_index").and_then(Value::as_i64) {
            if let Some(record) = records(ir, "materials").iter().find(|record| {
                record
                    .field("archive_index")
                    .and_then(|value| value.as_i64())
                    == Some(index)
            }) {
                match materials.native_targets(record.id()) {
                    [material_index] => {
                        let mut body = material_index.to_le_bytes().to_vec();
                        body.extend(integer(Some(&fields),"current_material_source",0).to_le_bytes());
                        output.records.push(crc_chunk(0x2000_8039,&body)?);
                        neutral = true;
                    }
                    [] => {},
                    _ => losses.push(RhinoLossCode::WriterAttributesOmitted.note(format!("document setting {} /current_material_index {} -> absent; native material {} has multiple authored appearances {:?}, so no unique target exists",setting.id(),index,record.id(),super::materials::MaterialPlan::target_names(ir,record.id())))),
                }
            }
        }
        if output.records.len() == before {
            continue;
        }
        if neutral {
            output.regenerated.insert(setting.id().to_owned());
        } else {
            output.written.insert(setting.id().to_owned());
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests;
