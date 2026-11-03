// SPDX-License-Identifier: Apache-2.0
//! Dimension-style tables re-emitted from held typed records.
//!
//! A V5 record without style-extra userdata and a modern record whose
//! controls hold only the minor-0 fields round-trip exactly: every other
//! control is an admitted scalar. A modern minor-1-or-later record carries
//! named children (`scale_value`, `font_characteristics`, `text_mask`) the
//! decoder retains as digests only, and a V5 record with style-extra carries
//! class userdata the writer cannot frame, so both stay omitted with a
//! per-record loss.

use super::{
    anonymous, class_wrapper, component, crc_chunk_with_direct, records, utf16, wire_index, CadIr,
    Map, Value,
};
use crate::loss::RhinoLossCode;
use crate::writer::WriteError;
use cadmpeg_ir::report::loss::LossNote;

const DIMSTYLE_RECORD: u32 = 0x2000_8075;

/// Minor-0 modern controls, in wire order: every key a modern record may
/// carry without a named child or a minor-1-or-later scalar.
const MINOR0_CONTROLS: [&str; 43] = [
    "legacy_override_parent_count",
    "has_field_overrides",
    "field_override_bits",
    "tolerance_format",
    "tolerance_resolution",
    "tolerance_upper",
    "tolerance_lower",
    "tolerance_height_scale",
    "baseline_spacing_mm",
    "draw_text_mask_legacy",
    "mask_fill_type_legacy",
    "mask_color_legacy",
    "dimension_scale",
    "dimension_scale_source",
    "source_dimension_style_uuid",
    "color_sources",
    "colors",
    "plot_color_sources",
    "plot_colors",
    "plot_weight_sources",
    "extension_line_plot_weight_mm",
    "dimension_line_plot_weight_mm",
    "fixed_extension_length_mm",
    "fixed_extension_length_enabled",
    "text_rotation_radians",
    "alternate_tolerance_resolution",
    "tolerance_text_height_fraction",
    "suppress_arrow_1",
    "suppress_arrow_2",
    "text_move_leader",
    "arc_length_symbol",
    "stack_text_height_fraction",
    "stack_format",
    "alternate_rounding",
    "rounding",
    "angular_rounding",
    "alternate_zero_suppression",
    "obsolete_tolerance_zero_suppression",
    "zero_suppression",
    "angular_zero_suppression",
    "alternate_below",
    "arrow_types",
    "arrow_block_uuids",
];

pub(in crate::writer) struct DimStyleExport {
    pub(in crate::writer) records: Vec<Vec<u8>>,
    pub(in crate::writer) written: std::collections::BTreeSet<String>,
    pub(in crate::writer) losses: Vec<LossNote>,
}

fn text<'a>(fields: &'a Map<String, Value>, name: &str) -> Result<&'a str, WriteError> {
    fields
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| WriteError::Omit(format!("Rhino dimension style {name} needs a string")))
}

fn finite(fields: &Map<String, Value>, name: &str) -> Result<f64, WriteError> {
    fields
        .get(name)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .ok_or_else(|| WriteError::Omit(format!("Rhino dimension style {name} must be finite")))
}

fn flag(fields: &Map<String, Value>, name: &str) -> Result<bool, WriteError> {
    fields
        .get(name)
        .and_then(Value::as_bool)
        .ok_or_else(|| WriteError::Omit(format!("Rhino dimension style {name} needs a boolean")))
}

fn wide(fields: &Map<String, Value>, name: &str) -> Result<u32, WriteError> {
    u32::try_from(
        fields
            .get(name)
            .and_then(Value::as_u64)
            .ok_or_else(|| WriteError::Omit(format!("Rhino dimension style {name} needs a u32")))?,
    )
    .map_err(|_| WriteError::Omit(format!("Rhino dimension style {name} exceeds u32")))
}

fn narrow(fields: &Map<String, Value>, name: &str) -> Result<i32, WriteError> {
    i32::try_from(
        fields.get(name).and_then(Value::as_i64).ok_or_else(|| {
            WriteError::Omit(format!("Rhino dimension style {name} needs an i32"))
        })?,
    )
    .map_err(|_| WriteError::Omit(format!("Rhino dimension style {name} exceeds i32")))
}

fn color4(value: Option<&Value>, record: &str, name: &str) -> Result<[u8; 4], WriteError> {
    let values = value.and_then(Value::as_array).ok_or_else(|| {
        WriteError::Omit(format!(
            "dimension style {record} {name} needs four channels"
        ))
    })?;
    if values.len() != 4 {
        return Err(WriteError::Omit(format!(
            "dimension style {record} {name} needs four channels"
        )));
    }
    let mut out = [0; 4];
    for (target, value) in out.iter_mut().zip(values) {
        *target = u8::try_from(value.as_u64().ok_or_else(|| {
            WriteError::Omit(format!(
                "dimension style {record} {name} needs byte channels"
            ))
        })?)
        .map_err(|_| WriteError::Omit(format!("dimension style {record} {name} exceeds a byte")))?;
    }
    Ok(out)
}

fn uuid_bytes(value: Option<&Value>, record: &str, name: &str) -> Result<[u8; 16], WriteError> {
    let value = value.filter(|value| !value.is_null());
    let Some(text) = value.and_then(Value::as_str) else {
        return Ok([0; 16]);
    };
    crate::presentation::parse_uuid_text(text)
        .map(crate::wire::Uuid::to_wire)
        .ok_or_else(|| WriteError::Omit(format!("dimension style {record} {name} is not a UUID")))
}

fn byte2(controls: &Map<String, Value>, name: &str, record: &str) -> Result<[u8; 2], WriteError> {
    let values = controls
        .get(name)
        .and_then(Value::as_array)
        .ok_or_else(|| {
            WriteError::Omit(format!(
                "dimension style {record} {name} needs two channels"
            ))
        })?;
    if values.len() != 2 {
        return Err(WriteError::Omit(format!(
            "dimension style {record} {name} needs two channels"
        )));
    }
    let mut out = [0; 2];
    for (target, value) in out.iter_mut().zip(values) {
        *target = u8::try_from(value.as_u64().ok_or_else(|| {
            WriteError::Omit(format!(
                "dimension style {record} {name} needs byte channels"
            ))
        })?)
        .map_err(|_| WriteError::Omit(format!("dimension style {record} {name} exceeds a byte")))?;
    }
    Ok(out)
}

fn uuid_list(
    fields: &Map<String, Value>,
    name: &str,
    record: &str,
    count: usize,
) -> Result<Vec<[u8; 16]>, WriteError> {
    let values = fields
        .get(name)
        .and_then(Value::as_array)
        .filter(|values| values.len() == count)
        .ok_or_else(|| {
            WriteError::Omit(format!("Rhino dimension style {name} needs {count} UUIDs"))
        })?;
    values
        .iter()
        .map(|value| uuid_bytes(Some(value), record, name))
        .collect()
}

fn u32_list(
    fields: &Map<String, Value>,
    name: &str,
    record: &str,
    count: usize,
) -> Result<Vec<u32>, WriteError> {
    let values = fields
        .get(name)
        .and_then(Value::as_array)
        .filter(|values| values.len() == count)
        .ok_or_else(|| {
            WriteError::Omit(format!(
                "Rhino dimension style {name} needs {count} u32 values"
            ))
        })?;
    values
        .iter()
        .map(|value| {
            u32::try_from(value.as_u64().ok_or_else(|| {
                WriteError::Omit(format!("dimension style {record} {name} needs u32 values"))
            })?)
            .map_err(|_| WriteError::Omit(format!("dimension style {record} {name} exceeds u32")))
        })
        .collect()
}

fn color_list(
    fields: &Map<String, Value>,
    name: &str,
    record: &str,
    count: usize,
) -> Result<Vec<[u8; 4]>, WriteError> {
    let values = fields
        .get(name)
        .and_then(Value::as_array)
        .filter(|values| values.len() == count)
        .ok_or_else(|| {
            WriteError::Omit(format!("Rhino dimension style {name} needs {count} colors"))
        })?;
    values
        .iter()
        .map(|value| color4(Some(value), record, name))
        .collect()
}

fn control<'a>(
    controls: &'a Map<String, Value>,
    record: &str,
    name: &str,
) -> Result<&'a Value, WriteError> {
    controls.get(name).ok_or_else(|| {
        WriteError::Omit(format!("dimension style {record} control {name} is absent"))
    })
}

fn control_finite(
    controls: &Map<String, Value>,
    record: &str,
    name: &str,
) -> Result<f64, WriteError> {
    control(controls, record, name)?
        .as_f64()
        .filter(|value| value.is_finite())
        .ok_or_else(|| {
            WriteError::Omit(format!(
                "dimension style {record} control {name} must be finite"
            ))
        })
}

fn control_flag(
    controls: &Map<String, Value>,
    record: &str,
    name: &str,
) -> Result<bool, WriteError> {
    control(controls, record, name)?.as_bool().ok_or_else(|| {
        WriteError::Omit(format!(
            "dimension style {record} control {name} needs a boolean"
        ))
    })
}

fn control_wide(
    controls: &Map<String, Value>,
    record: &str,
    name: &str,
) -> Result<u32, WriteError> {
    u32::try_from(control(controls, record, name)?.as_u64().ok_or_else(|| {
        WriteError::Omit(format!(
            "dimension style {record} control {name} needs a u32"
        ))
    })?)
    .map_err(|_| {
        WriteError::Omit(format!(
            "dimension style {record} control {name} exceeds u32"
        ))
    })
}

fn control_narrow(
    controls: &Map<String, Value>,
    record: &str,
    name: &str,
) -> Result<i32, WriteError> {
    i32::try_from(control(controls, record, name)?.as_i64().ok_or_else(|| {
        WriteError::Omit(format!(
            "dimension style {record} control {name} needs an i32"
        ))
    })?)
    .map_err(|_| {
        WriteError::Omit(format!(
            "dimension style {record} control {name} exceeds i32"
        ))
    })
}

fn override_bits(controls: &Map<String, Value>, record: &str) -> Result<Vec<u8>, WriteError> {
    if !control_flag(controls, record, "has_field_overrides")? {
        return Ok(Vec::new());
    }
    let values = controls
        .get("field_override_bits")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            WriteError::Omit(format!(
                "dimension style {record} field_override_bits needs a byte array"
            ))
        })?;
    values
        .iter()
        .map(|value| {
            u8::try_from(value.as_u64().ok_or_else(|| {
                WriteError::Omit(format!(
                    "dimension style {record} field_override_bits needs bytes"
                ))
            })?)
            .map_err(|_| {
                WriteError::Omit(format!(
                    "dimension style {record} field_override_bits exceeds a byte"
                ))
            })
        })
        .collect()
}

/// Whether a modern record holds only minor-0 controls with admitted values:
/// no named-child digest and no key outside [`MINOR0_CONTROLS`].
fn minor0(controls: &Map<String, Value>) -> bool {
    controls.iter().all(|(key, value)| {
        MINOR0_CONTROLS.contains(&key.as_str())
            && !value
                .as_object()
                .is_some_and(|fields| fields.contains_key("sha256"))
    })
}

fn controls_of<'a>(
    fields: &'a Map<String, Value>,
    record: &str,
) -> Result<&'a Map<String, Value>, WriteError> {
    fields
        .get("controls")
        .and_then(Value::as_object)
        .ok_or_else(|| WriteError::Omit(format!("dimension style {record} has no typed controls")))
}

fn minor0_controls(
    record: &str,
    controls: &Map<String, Value>,
    rest: &mut Vec<u8>,
) -> Result<(), WriteError> {
    rest.extend(control_wide(controls, record, "legacy_override_parent_count")?.to_le_bytes());
    let overrides = override_bits(controls, record)?;
    rest.push(u8::from(control_flag(
        controls,
        record,
        "has_field_overrides",
    )?));
    if control_flag(controls, record, "has_field_overrides")? {
        rest.extend(
            i32::try_from(overrides.len())
                .map_err(|_| {
                    WriteError::Omit(format!(
                        "dimension style {record} field_override_bits exceed i32"
                    ))
                })?
                .to_le_bytes(),
        );
        rest.extend(overrides);
    }
    rest.extend(control_wide(controls, record, "tolerance_format")?.to_le_bytes());
    rest.extend(control_narrow(controls, record, "tolerance_resolution")?.to_le_bytes());
    for name in [
        "tolerance_upper",
        "tolerance_lower",
        "tolerance_height_scale",
        "baseline_spacing_mm",
    ] {
        rest.extend(control_finite(controls, record, name)?.to_le_bytes());
    }
    rest.push(u8::from(control_flag(
        controls,
        record,
        "draw_text_mask_legacy",
    )?));
    rest.extend(control_wide(controls, record, "mask_fill_type_legacy")?.to_le_bytes());
    rest.extend(color4(
        controls.get("mask_color_legacy"),
        record,
        "mask_color_legacy",
    )?);
    rest.extend(control_finite(controls, record, "dimension_scale")?.to_le_bytes());
    rest.extend(control_narrow(controls, record, "dimension_scale_source")?.to_le_bytes());
    rest.extend(uuid_bytes(
        controls.get("source_dimension_style_uuid"),
        record,
        "source_dimension_style_uuid",
    )?);
    rest.extend(color4(
        controls.get("color_sources"),
        record,
        "color_sources",
    )?);
    for value in color_list(controls, "colors", record, 4)? {
        rest.extend(value);
    }
    rest.extend(color4(
        controls.get("plot_color_sources"),
        record,
        "plot_color_sources",
    )?);
    for value in color_list(controls, "plot_colors", record, 4)? {
        rest.extend(value);
    }
    rest.extend(byte2(controls, "plot_weight_sources", record)?);
    for name in [
        "extension_line_plot_weight_mm",
        "dimension_line_plot_weight_mm",
        "fixed_extension_length_mm",
    ] {
        rest.extend(control_finite(controls, record, name)?.to_le_bytes());
    }
    rest.push(u8::from(control_flag(
        controls,
        record,
        "fixed_extension_length_enabled",
    )?));
    rest.extend(control_finite(controls, record, "text_rotation_radians")?.to_le_bytes());
    rest.extend(control_narrow(controls, record, "alternate_tolerance_resolution")?.to_le_bytes());
    rest.extend(control_finite(controls, record, "tolerance_text_height_fraction")?.to_le_bytes());
    rest.push(u8::from(control_flag(
        controls,
        record,
        "suppress_arrow_1",
    )?));
    rest.push(u8::from(control_flag(
        controls,
        record,
        "suppress_arrow_2",
    )?));
    rest.extend(control_narrow(controls, record, "text_move_leader")?.to_le_bytes());
    rest.extend(control_narrow(controls, record, "arc_length_symbol")?.to_le_bytes());
    rest.extend(control_finite(controls, record, "stack_text_height_fraction")?.to_le_bytes());
    rest.extend(control_wide(controls, record, "stack_format")?.to_le_bytes());
    for name in ["alternate_rounding", "rounding", "angular_rounding"] {
        rest.extend(control_finite(controls, record, name)?.to_le_bytes());
    }
    for name in [
        "alternate_zero_suppression",
        "obsolete_tolerance_zero_suppression",
        "zero_suppression",
        "angular_zero_suppression",
    ] {
        rest.extend(control_wide(controls, record, name)?.to_le_bytes());
    }
    rest.push(u8::from(control_flag(controls, record, "alternate_below")?));
    for value in u32_list(controls, "arrow_types", record, 3)? {
        rest.extend(value.to_le_bytes());
    }
    for value in uuid_list(controls, "arrow_block_uuids", record, 3)? {
        rest.extend(value);
    }
    Ok(())
}

fn modern(
    record: &cadmpeg_ir::NativeRecord,
    fields: &Map<String, Value>,
    index: i32,
) -> Result<Vec<u8>, WriteError> {
    let controls = controls_of(fields, record.id())?;
    if !minor0(controls) {
        return Err(WriteError::Omit(
            "modern controls past minor 0 carry named children the decoder retains as digests only"
                .into(),
        ));
    }
    let mut inner = component(
        record.id(),
        index,
        text(fields, "name")?,
        fields.get("source_uuid"),
    )
    .map_err(WriteError::Fail)?;
    for name in [
        "extension_line_extension_mm",
        "extension_line_offset_mm",
        "arrow_size_mm",
        "leader_arrow_size_mm",
        "center_mark_size_mm",
        "text_gap_mm",
        "text_height_mm",
    ] {
        inner.extend(finite(fields, name)?.to_le_bytes());
    }
    for name in ["text_display_mode", "angle_format", "length_format"] {
        inner.extend(wide(fields, name)?.to_le_bytes());
    }
    for name in ["angle_resolution", "length_resolution", "text_style_index"] {
        inner.extend(narrow(fields, name)?.to_le_bytes());
    }
    inner.extend(finite(fields, "length_factor")?.to_le_bytes());
    inner.push(u8::from(flag(fields, "alternate_enabled")?));
    inner.extend(finite(fields, "alternate_length_factor")?.to_le_bytes());
    inner.extend(wide(fields, "alternate_length_format")?.to_le_bytes());
    inner.extend(narrow(fields, "alternate_length_resolution")?.to_le_bytes());
    for name in ["prefix", "suffix", "alternate_prefix", "alternate_suffix"] {
        inner.extend(utf16(text(fields, name)?).map_err(WriteError::Fail)?);
    }
    inner.extend(finite(fields, "dimension_line_extension_mm")?.to_le_bytes());
    inner.push(u8::from(flag(fields, "suppress_extension_line_1")?));
    inner.push(u8::from(flag(fields, "suppress_extension_line_2")?));
    inner.extend(uuid_bytes(
        fields.get("parent_style_uuid"),
        record.id(),
        "parent_style_uuid",
    )?);
    minor0_controls(record.id(), controls, &mut inner)?;
    anonymous(1, 0, &inner).map_err(WriteError::Fail)
}

fn v5_version(controls: &Map<String, Value>, record: &str) -> Result<i32, WriteError> {
    let version = controls
        .get("v5_version")
        .and_then(Value::as_object)
        .ok_or_else(|| WriteError::Omit(format!("dimension style {record} has no V5 version")))?;
    let major = version
        .get("major")
        .and_then(Value::as_u64)
        .ok_or_else(|| WriteError::Omit(format!("dimension style {record} has no V5 major")))?;
    let minor = version
        .get("minor")
        .and_then(Value::as_u64)
        .ok_or_else(|| WriteError::Omit(format!("dimension style {record} has no V5 minor")))?;
    if major != 1 || minor > 5 {
        return Err(WriteError::Omit(format!(
            "dimension style {record} V5 version {major}.{minor} is past the typed minor 5"
        )));
    }
    i32::try_from(minor)
        .map_err(|_| WriteError::Omit(format!("dimension style {record} V5 minor exceeds i32")))
}

fn v5_scales(
    record: &str,
    fields: &Map<String, Value>,
    controls: &Map<String, Value>,
    minor: i32,
    payload: &mut Vec<u8>,
) -> Result<(), WriteError> {
    if minor >= 2 {
        payload.extend(control_finite(controls, record, "v5_length_factor")?.to_le_bytes());
        for name in ["prefix", "suffix"] {
            payload.extend(utf16(text(fields, name)?).map_err(WriteError::Fail)?);
        }
        payload.push(u8::from(flag(fields, "alternate_enabled")?));
        payload.extend(finite(fields, "alternate_length_factor")?.to_le_bytes());
        payload.extend(wide(fields, "alternate_length_format")?.to_le_bytes());
        payload.extend(narrow(fields, "alternate_length_resolution")?.to_le_bytes());
        payload.extend(control_wide(controls, record, "v5_alternate_angle_format")?.to_le_bytes());
        payload.extend(
            control_narrow(controls, record, "v5_alternate_angle_resolution")?.to_le_bytes(),
        );
        for name in ["alternate_prefix", "alternate_suffix"] {
            payload.extend(utf16(text(fields, name)?).map_err(WriteError::Fail)?);
        }
        payload.extend(control_wide(controls, record, "v5_unused")?.to_le_bytes());
    }
    if minor >= 3 {
        payload.extend(uuid_bytes(
            fields.get("source_uuid"),
            record,
            "source_uuid",
        )?);
    }
    if minor >= 4 {
        payload.extend(finite(fields, "dimension_line_extension_mm")?.to_le_bytes());
    }
    if minor >= 5 {
        payload.extend(finite(fields, "leader_arrow_size_mm")?.to_le_bytes());
        payload.extend(control_narrow(controls, record, "v5_leader_arrow_type")?.to_le_bytes());
        payload.push(u8::from(flag(fields, "suppress_extension_line_1")?));
        payload.push(u8::from(flag(fields, "suppress_extension_line_2")?));
    }
    Ok(())
}

fn v5(
    record: &cadmpeg_ir::NativeRecord,
    fields: &Map<String, Value>,
    index: i32,
) -> Result<Vec<u8>, WriteError> {
    if fields.get("v5_extra").is_some() {
        return Err(WriteError::Omit(
            "V5 style-extra is class userdata the writer cannot frame".into(),
        ));
    }
    let controls = controls_of(fields, record.id())?;
    let minor = v5_version(controls, record.id())?;
    let minor_byte = u8::try_from(minor).map_err(|_| {
        WriteError::Omit(format!(
            "dimension style {} V5 minor {minor} exceeds a nibble",
            record.id()
        ))
    })?;
    let mut payload = vec![0x10 | minor_byte];
    payload.extend(index.to_le_bytes());
    payload.extend(utf16(text(fields, "name")?).map_err(WriteError::Fail)?);
    for name in [
        "extension_line_extension_mm",
        "extension_line_offset_mm",
        "arrow_size_mm",
        "center_mark_size_mm",
        "text_gap_mm",
    ] {
        payload.extend(finite(fields, name)?.to_le_bytes());
    }
    payload.extend(wide(fields, "text_display_mode")?.to_le_bytes());
    payload.extend(control_narrow(controls, record.id(), "v5_arrow_type")?.to_le_bytes());
    payload.extend(control_narrow(controls, record.id(), "v5_angular_units")?.to_le_bytes());
    payload.extend(wide(fields, "length_format")?.to_le_bytes());
    payload.extend(wide(fields, "angle_format")?.to_le_bytes());
    payload.extend(narrow(fields, "length_resolution")?.to_le_bytes());
    payload.extend(narrow(fields, "angle_resolution")?.to_le_bytes());
    payload.extend(narrow(fields, "text_style_index")?.to_le_bytes());
    if minor >= 1 {
        payload.extend(finite(fields, "text_height_mm")?.to_le_bytes());
    }
    v5_scales(record.id(), fields, controls, minor, &mut payload)?;
    Ok(payload)
}

fn wrapped(
    _record: &cadmpeg_ir::NativeRecord,
    payload: &[u8],
    class: [u8; 16],
) -> Result<Vec<u8>, WriteError> {
    crc_chunk_with_direct(
        DIMSTYLE_RECORD,
        &class_wrapper(class, payload).map_err(WriteError::Fail)?,
        &[],
    )
    .map_err(WriteError::Fail)
}

pub(in crate::writer) fn styles(
    ir: &CadIr,
    version: crate::RhinoArchiveVersion,
) -> Result<DimStyleExport, WriteError> {
    let modern_target = version != crate::RhinoArchiveVersion::V5;
    let mut output = DimStyleExport {
        records: Vec::new(),
        written: std::collections::BTreeSet::new(),
        losses: Vec::new(),
    };
    for (position, record) in records(ir, "dimension_styles").iter().enumerate() {
        let index = wire_index(position).map_err(WriteError::Fail)?;
        let fields = record.fields()?;
        let v5_record = fields
            .get("controls")
            .and_then(Value::as_object)
            .is_some_and(|controls| controls.contains_key("v5_version"));
        // Archives below 60 carry `ON_V5x_DimStyle` records and archives at
        // 60 and later carry modern `ON_DimStyle` records (`docs/formats/
        // rhino_3dm.md` § on the V5 class); a record of the other era has no
        // encoding on this target.
        if v5_record == modern_target {
            output
                .losses
                .push(RhinoLossCode::WriterAttributesOmitted.note(format!(
                    "{}: a {} dimension style has no encoding on an archive-{} target",
                    record.id(),
                    if v5_record { "V5" } else { "modern" },
                    if modern_target { "60-or-later" } else { "50" },
                )));
            continue;
        }
        let written = if v5_record {
            v5(record, &fields, index).map(|payload| {
                wrapped(
                    record,
                    &payload,
                    crate::wire::Uuid::to_wire(crate::presentation::V5_DIMSTYLE),
                )
            })
        } else {
            modern(record, &fields, index).map(|payload| {
                wrapped(
                    record,
                    &payload,
                    crate::wire::Uuid::to_wire(crate::presentation::DIMSTYLE),
                )
            })
        };
        match written {
            Ok(payload) => match payload {
                Ok(record_bytes) => {
                    output.records.push(record_bytes);
                    output.written.insert(record.id().to_owned());
                }
                Err(error) => return Err(error),
            },
            Err(WriteError::Omit(reason)) => output.losses.push(
                RhinoLossCode::WriterAttributesOmitted.note(format!("{}: {reason}", record.id())),
            ),
            Err(error) => return Err(error),
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests;
