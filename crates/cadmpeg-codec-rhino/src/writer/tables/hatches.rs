// SPDX-License-Identifier: Apache-2.0
//! Typed hatch lines are emitted in the archive's millimetre units.

use super::{
    anonymous, anonymous_with_children, class_wrapper_with_direct, component,
    crc_chunk_with_direct, records, utf16, wire_index, CadIr, CodecError, Value,
};
use crate::loss::RhinoLossCode;
use crate::writer::WriteError;
use cadmpeg_ir::report::loss::LossNote;
use std::collections::BTreeSet;

const PATTERN_CLASS: [u8; 16] = [
    0x91, 0x7c, 0x4e, 0x06, 0xf6, 0x35, 0x34, 0x47, 0xa4, 0x46, 0x79, 0xff, 0x7c, 0xd6, 0x59, 0xe1,
];

fn scalar(value: Option<&Value>, name: &str) -> Result<f64, WriteError> {
    value
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .ok_or_else(|| WriteError::Omit(format!("Rhino hatch {name} must be finite")))
}

fn line(value: &Value) -> Result<Vec<u8>, WriteError> {
    let mut body = scalar(value.get("angle_radians"), "angle")?
        .to_le_bytes()
        .to_vec();
    for name in ["base_millimeters", "offset_millimeters"] {
        let point = value
            .get(name)
            .and_then(Value::as_array)
            .filter(|values| values.len() == 2)
            .ok_or_else(|| WriteError::Omit(format!("Rhino hatch {name} needs two coordinates")))?;
        for coordinate in point {
            body.extend(scalar(Some(coordinate), name)?.to_le_bytes());
        }
    }
    let dashes = value
        .get("dashes_millimeters")
        .and_then(Value::as_array)
        .ok_or_else(|| WriteError::Omit("Rhino hatch has no typed dash list".into()))?;
    body.extend(wire_index(dashes.len())?.to_le_bytes());
    for dash in dashes {
        body.extend(scalar(Some(dash), "dash")?.to_le_bytes());
    }
    Ok(anonymous(1, 0, &body)?)
}

pub(in crate::writer) struct HatchExport {
    pub(in crate::writer) records: Vec<Vec<u8>>,
    pub(in crate::writer) written: BTreeSet<String>,
    pub(in crate::writer) losses: Vec<LossNote>,
}

pub(in crate::writer) fn patterns(ir: &CadIr) -> Result<HatchExport, WriteError> {
    let mut output = HatchExport {
        records: Vec::new(),
        written: BTreeSet::new(),
        losses: Vec::new(),
    };
    for record in records(ir, "hatch_patterns") {
        match pattern(record, output.records.len()) {
            Ok(payload) => {
                let index = wire_index(output.records.len())?;
                output.records.push(payload);
                output.written.insert(record.id().to_owned());
                if let Some(source) = record
                    .field("archive_index")
                    .and_then(|value| value.as_i64())
                {
                    if source != i64::from(index) {
                        output
                            .losses
                            .push(RhinoLossCode::WriterHatchPatternFieldsReduced.note(format!(
                                "hatch pattern {} archive index {source} is written as {index}",
                                record.id()
                            )));
                    }
                }
                if record.field("pattern_unit_system").is_some()
                    || record.field("always_model_distances").is_some()
                {
                    output
                        .losses
                        .push(RhinoLossCode::WriterHatchPatternFieldsReduced.note(format!(
                            "hatch pattern {} distance settings are omitted by the target archive",
                            record.id()
                        )));
                }
            }
            Err(WriteError::Omit(reason)) => output.losses.push(
                RhinoLossCode::WriterHatchPatternOmitted
                    .note(format!("hatch pattern {} omitted: {reason}", record.id())),
            ),
            Err(error) => return Err(error),
        }
    }
    Ok(output)
}

fn pattern(record: &cadmpeg_ir::NativeRecord, index: usize) -> Result<Vec<u8>, WriteError> {
    let fields = record.fields().map_err(|error| match error {
        cadmpeg_ir::native::NativeConvertError::Resource(CodecError::ResourceLimit(limit)) => {
            WriteError::Encode(limit.into())
        }
        cadmpeg_ir::native::NativeConvertError::Resource(error) => WriteError::Fail(error),
        error => WriteError::Omit(error.to_string()),
    })?;
    let name = fields
        .get("name")
        .and_then(Value::as_str)
        .filter(|value| !value.contains('\0'))
        .ok_or_else(|| WriteError::Omit("Rhino hatch name is not writable".into()))?;
    let description = fields
        .get("description")
        .and_then(Value::as_str)
        .filter(|value| !value.contains('\0'))
        .ok_or_else(|| WriteError::Omit("Rhino hatch description is not writable".into()))?;
    let fill_type = fields
        .get("fill_type")
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .ok_or_else(|| {
            WriteError::Omit("Rhino hatch fill type must fit a native integer".into())
        })?;
    let mut payload = component(
        record.id(),
        wire_index(index)?,
        name,
        fields.get("source_uuid"),
    )?;
    let component_end = payload.len();
    payload.extend(fill_type.to_le_bytes());
    payload.extend(utf16(description)?);
    let lines = fields
        .get("lines")
        .and_then(Value::as_array)
        .ok_or_else(|| WriteError::Omit("Rhino hatch has no typed line list".into()))?;
    let mut list = wire_index(lines.len())?.to_le_bytes().to_vec();
    let mut children = Vec::new();
    for value in lines {
        let start = list.len();
        list.extend(line(value)?);
        children.push(start..list.len());
    }
    let ranges = crate::chunks::direct_checksum_ranges(&(0..list.len()), &children)
        .map_err(|error| WriteError::Omit(error.to_string()))?;
    let mut direct = Vec::new();
    for range in &ranges {
        direct.extend(&list[range]);
    }
    let list_start = payload.len();
    payload.extend(crc_chunk_with_direct(0x4000_8000, &list, &direct)?);
    let list_end = payload.len();
    let payload =
        anonymous_with_children(1, 0, &payload, &[0..component_end, list_start..list_end])?;
    Ok(crc_chunk_with_direct(
        0x2000_8077,
        &class_wrapper_with_direct(PATTERN_CLASS, &payload, &[])?,
        &[],
    )?)
}

#[cfg(test)]
mod tests {
    use cadmpeg_ir::codec::write::{
        target::TargetRequest, EncodeInput, Encoder, NativeDisposition,
    };
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::{CadIr, NativeRecord};
    use serde_json::json;
    use std::io::Cursor;

    #[test]
    fn invalid_pattern_names_its_record_and_keeps_a_valid_sibling() {
        let record = |id: &str, lines| {
            NativeRecord::new(
                id.try_into().unwrap(),
                serde_json::from_value(json!({
                    "name": id, "fill_type": 0, "description": "", "lines": lines,
                    "archive_index": 5, "pattern_unit_system": 2, "always_model_distances": true
                }))
                .unwrap(),
            )
            .unwrap()
        };
        let invalid = "rhino:test:hatch#invalid";
        let valid = "rhino:test:hatch#valid";
        let mut ir = CadIr::empty();
        ir.native.namespace_mut("rhino").arenas_mut().insert(
            "hatch_patterns".into(),
            vec![record(invalid, json!(null)), record(valid, json!([]))],
        );
        ir.finalize();
        let mut bytes = Vec::new();
        let report = crate::RhinoCodec
            .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut bytes))
            .unwrap();
        assert!(report.losses.iter().any(|loss| {
            loss.code == crate::loss::RhinoLossCode::WriterHatchPatternOmitted.kind()
                && loss.message.contains(invalid)
        }));
        assert_eq!(
            report
                .losses
                .iter()
                .filter(|loss| {
                    loss.code == crate::loss::RhinoLossCode::WriterHatchPatternFieldsReduced.kind()
                        && loss.message.contains(valid)
                })
                .count(),
            2
        );
        assert_eq!(
            report
                .native_dispositions
                .record_disposition("rhino", "hatch_patterns", invalid),
            NativeDisposition::Dropped
        );
        assert_eq!(
            report
                .native_dispositions
                .record_disposition("rhino", "hatch_patterns", valid),
            NativeDisposition::Written
        );
        let decoded = crate::RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        let patterns = &decoded.ir().native.namespace("rhino").unwrap().arenas()["hatch_patterns"];
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].field("name"), Some(json!(valid)));
    }
}
