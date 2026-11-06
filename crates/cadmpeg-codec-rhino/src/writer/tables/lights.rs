// SPDX-License-Identifier: Apache-2.0
//! Light geometry and photometric controls have a native typed inverse.

use super::{
    class_wrapper, color, crc_chunk_with_direct, records, short_chunk, utf16, uuid, wire_index,
    CadIr, Map, Value,
};
use crate::loss::RhinoLossCode;
use crate::writer::WriteError;
use cadmpeg_ir::report::loss::LossNote;
use std::collections::BTreeSet;

mod attributes;

const LIGHT_CLASS: [u8; 16] = [
    0x13, 0x85, 0xa0, 0x85, 0x83, 0xf3, 0xd3, 0x11, 0xbf, 0xe7, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];

fn scalar(fields: &Map<String, Value>, name: &str) -> Result<f64, WriteError> {
    fields
        .get(name)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .ok_or_else(|| WriteError::Omit(format!("Rhino light {name} must be finite")))
}

fn vector(fields: &Map<String, Value>, name: &str, output: &mut Vec<u8>) -> Result<(), WriteError> {
    let values = fields
        .get(name)
        .and_then(Value::as_array)
        .filter(|values| values.len() == 3)
        .ok_or_else(|| WriteError::Omit(format!("Rhino light {name} needs three coordinates")))?;
    for value in values {
        output.extend(
            value
                .as_f64()
                .filter(|value| value.is_finite())
                .ok_or_else(|| {
                    WriteError::Omit(format!("Rhino light {name} coordinate must be finite"))
                })?
                .to_le_bytes(),
        );
    }
    Ok(())
}

pub(in crate::writer) struct LightExport {
    pub(in crate::writer) records: Vec<Vec<u8>>,
    pub(in crate::writer) written: BTreeSet<String>,
    pub(in crate::writer) losses: Vec<LossNote>,
}

pub(in crate::writer) fn lights(ir: &CadIr) -> Result<LightExport, WriteError> {
    let mut output = LightExport {
        records: Vec::new(),
        written: BTreeSet::new(),
        losses: Vec::new(),
    };
    for record in records(ir, "lights") {
        match light(record, output.records.len()) {
            Ok((payload, reassigned, attribute_loss)) => {
                output.records.push(payload);
                let links = record.field("links").is_some_and(|value| {
                    !value.is_null() && value.as_array().is_none_or(|links| !links.is_empty())
                });
                if attribute_loss.is_some() || links {
                    let mut omitted = Vec::new();
                    if let Some(reason) = attribute_loss {
                        omitted.push(format!("attributes: {reason}"));
                    }
                    if links {
                        omitted.push("links".to_owned());
                    }
                    output
                        .losses
                        .push(RhinoLossCode::WriterAttributesOmitted.note(format!(
                            "light {} omitted {}",
                            record.id(),
                            omitted.join(", ")
                        )));
                } else if reassigned.is_none() {
                    output.written.insert(record.id().to_owned());
                }
                if let Some((source, written)) = reassigned {
                    output
                        .losses
                        .push(RhinoLossCode::WriterLightIndexReassigned.note(format!(
                            "light {} archive index {source} is written as {written}",
                            record.id()
                        )));
                }
            }
            Err(WriteError::Omit(reason)) => output.losses.push(
                RhinoLossCode::WriterLightOmitted
                    .note(format!("light {} omitted: {reason}", record.id())),
            ),
            Err(error) => return Err(error),
        }
    }
    Ok(output)
}

fn light(
    record: &cadmpeg_ir::NativeRecord,
    index: usize,
) -> Result<(Vec<u8>, Option<(i32, i32)>, Option<String>), WriteError> {
    let fields = record.fields().map_err(|error| match error {
        cadmpeg_ir::native::NativeConvertError::Resource(
            cadmpeg_core::CodecError::ResourceLimit(limit),
        ) => WriteError::Encode(limit.into()),
        cadmpeg_ir::native::NativeConvertError::Resource(error) => WriteError::Fail(error),
        error => WriteError::Omit(error.to_string()),
    })?;
    let mut payload = vec![0x12];
    let enabled = fields
        .get("enabled")
        .and_then(Value::as_bool)
        .ok_or_else(|| WriteError::Omit("Rhino light enabled must be boolean".into()))?;
    let style = fields
        .get("style")
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .ok_or_else(|| WriteError::Omit("Rhino light style must fit a native integer".into()))?;
    payload.extend(i32::from(enabled).to_le_bytes());
    payload.extend(style.to_le_bytes());
    for name in ["intensity", "watts"] {
        payload.extend(scalar(&fields, name)?.to_le_bytes());
    }
    for name in ["ambient", "diffuse", "specular"] {
        if fields.get(name).is_none_or(Value::is_null) {
            return Err(WriteError::Omit(format!("Rhino light {name} is missing")));
        }
        payload.extend(
            color(Some(&fields), name).map_err(|error| WriteError::Omit(error.to_string()))?,
        );
    }
    for name in ["direction", "location"] {
        vector(&fields, name, &mut payload)?;
    }
    for name in ["spot_angle_degrees", "spot_exponent"] {
        payload.extend(scalar(&fields, name)?.to_le_bytes());
    }
    vector(&fields, "attenuation", &mut payload)?;
    payload.extend(scalar(&fields, "shadow_intensity")?.to_le_bytes());
    let written_index = wire_index(index)?;
    let source_index = fields
        .get("archive_index")
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .ok_or_else(|| {
            WriteError::Omit("Rhino light archive_index must fit a native integer".into())
        })?;
    payload.extend(written_index.to_le_bytes());
    if fields
        .get("source_uuid")
        .and_then(Value::as_str)
        .and_then(crate::presentation::parse_uuid_text)
        .is_none()
    {
        return Err(WriteError::Omit(
            "Rhino light source_uuid is invalid".into(),
        ));
    }
    payload.extend(uuid(fields.get("source_uuid"), record.id()));
    let name = fields
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.contains('\0'))
        .ok_or_else(|| WriteError::Omit("Rhino light name is not writable".into()))?;
    payload.extend(utf16(name)?);
    for name in ["length", "width"] {
        vector(&fields, name, &mut payload)?;
    }
    payload.extend(scalar(&fields, "hotspot")?.to_le_bytes());
    let mut body = class_wrapper(LIGHT_CLASS, &payload)?;
    let mut attribute_loss = None;
    if let Some(value) = fields.get("attributes").filter(|value| !value.is_null()) {
        match attributes::encode(value) {
            Ok(attributes) => body.extend(crc_chunk_with_direct(
                0x0200_8061,
                &attributes,
                &attributes,
            )?),
            Err(WriteError::Omit(reason)) => attribute_loss = Some(reason),
            Err(error) => return Err(error),
        }
    }
    body.extend(short_chunk(0x8200_006f, 0));
    let payload = crc_chunk_with_direct(0x2000_8060, &body, &[])?;
    Ok((
        payload,
        (source_index != written_index).then_some((source_index, written_index)),
        attribute_loss,
    ))
}

#[cfg(test)]
mod tests {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::{CadIr, NativeRecord};
    use serde_json::{json, Value};
    use std::io::Cursor;

    #[test]
    fn invalid_light_names_its_record_and_keeps_a_valid_sibling() {
        let valid = json!({
            "name": "valid light", "archive_index": 0,
            "source_uuid": "11111111-1111-1111-1111-111111111111", "enabled": true, "style": 0,
            "intensity": 1.0, "watts": 1.0,
            "ambient": {"r": 0.0, "g": 0.0, "b": 0.0, "a": 1.0},
            "diffuse": {"r": 1.0, "g": 1.0, "b": 1.0, "a": 1.0},
            "specular": {"r": 1.0, "g": 1.0, "b": 1.0, "a": 1.0},
            "direction": [0.0, 0.0, -1.0], "location": [0.0, 0.0, 0.0],
            "spot_angle_degrees": 45.0, "spot_exponent": 0.0,
            "attenuation": [1.0, 0.0, 0.0], "shadow_intensity": 1.0,
            "length": [1.0, 0.0, 0.0], "width": [0.0, 1.0, 0.0], "hotspot": 0.5
        });
        let record = |id: &str, fields: Value| {
            NativeRecord::new(
                id.try_into().expect("light identity"),
                serde_json::from_value(fields).expect("light fields"),
            )
            .expect("light record")
        };
        let mut records = Vec::new();
        for (name, value) in [
            ("intensity", Value::Null),
            ("enabled", json!("yes")),
            ("style", json!(2_147_483_648_u64)),
            ("archive_index", json!("not-an-index")),
            ("ambient", Value::Null),
            ("name", json!("invalid\0name")),
            ("source_uuid", json!("not-a-uuid")),
        ] {
            let mut invalid = valid.clone();
            invalid
                .as_object_mut()
                .unwrap()
                .insert(name.to_owned(), value);
            records.push(record(&format!("rhino:test:light#invalid-{name}"), invalid));
        }
        let mut partial = valid.clone();
        partial["source_uuid"] = json!("22222222-2222-2222-2222-222222222222");
        partial["name"] = json!("light with attributes");
        partial["archive_index"] = json!(5);
        partial["attributes"] = json!({"name": "held attribute"});
        partial["links"] = json!(["rhino:document:layer#linked"]);
        let mut attributed = valid.clone();
        attributed["source_uuid"] = json!("33333333-3333-3333-3333-333333333333");
        attributed["name"] = json!("attributed light");
        attributed["archive_index"] = json!(2);
        attributed["links"] = json!([]);
        attributed["attributes"] = json!({
            "source_uuid":"44444444-4444-4444-4444-444444444444", "name":"light attributes", "url":"https://example.test/",
            "layer_index":-1, "material_index":-1, "linetype_index":-1, "color":"01020304", "visible":false,
            "object_mode":2, "decoration":1, "wire_density":3, "color_source":1, "linetype_source":0,
            "material_source":0, "plot_color_source":1, "plot_weight_source":1, "plot_color":"05060708",
            "plot_weight_mm":0.25, "group_indexes":[], "display_materials":[], "active_space":0,
            "viewport_uuid":null, "display_order":7, "clipping_proof":false, "clipping_plane_uuids":[],
            "hatch_pattern_index":-1, "section_hatch_scale":2.0, "section_hatch_rotation":0.5,
            "linetype_pattern_scale":1.5, "hatch_background":"090a0b0c", "hatch_boundary_visible":true,
            "section_fill_rule":1, "clipping_plane_label_style":2, "detail_background_visible":true, "rendering_materials":[]
        });
        let expected_attributes = attributed["attributes"].clone();
        records.push(record("rhino:test:light#valid", valid));
        records.push(record("rhino:test:light#z-partial", partial));
        records.push(record("rhino:test:light#zz-attributed", attributed));
        let mut ir = CadIr::empty();
        ir.native
            .namespace_mut("rhino")
            .arenas_mut()
            .insert("lights".into(), records);
        ir.finalize();
        for target in [
            "rhino:archive-50",
            "rhino:archive-60",
            "rhino:archive-70",
            "rhino:archive-80",
        ] {
            let mut bytes = Vec::new();
            let report = crate::RhinoCodec
                .plan(EncodeInput::new(&ir, None), TargetRequest::Explicit(target))
                .and_then(|plan| plan.write_to(&mut bytes))
                .expect("write surviving light");
            for name in [
                "intensity",
                "enabled",
                "style",
                "archive_index",
                "ambient",
                "name",
                "source_uuid",
            ] {
                assert!(report.losses.iter().any(|loss| {
                    loss.code == crate::loss::RhinoLossCode::WriterLightOmitted.kind()
                        && loss
                            .message
                            .contains(&format!("rhino:test:light#invalid-{name}"))
                }));
                assert_eq!(
                    report.native_dispositions.record_disposition(
                        "rhino",
                        "lights",
                        &format!("rhino:test:light#invalid-{name}")
                    ),
                    cadmpeg_ir::codec::write::NativeDisposition::Dropped
                );
            }
            assert_eq!(
                report.native_dispositions.record_disposition(
                    "rhino",
                    "lights",
                    "rhino:test:light#valid"
                ),
                cadmpeg_ir::codec::write::NativeDisposition::Written
            );
            assert!(report.losses.iter().any(|loss| {
                loss.code == crate::loss::RhinoLossCode::WriterLightIndexReassigned.kind()
                    && loss.message.contains("rhino:test:light#z-partial")
            }));
            assert_eq!(
                report.native_dispositions.record_disposition(
                    "rhino",
                    "lights",
                    "rhino:test:light#zz-attributed"
                ),
                cadmpeg_ir::codec::write::NativeDisposition::Written
            );
            assert_eq!(
                report.native_dispositions.record_disposition(
                    "rhino",
                    "lights",
                    "rhino:test:light#z-partial"
                ),
                cadmpeg_ir::codec::write::NativeDisposition::Dropped
            );
            assert!(report.losses.iter().any(|loss| {
                loss.code == crate::loss::RhinoLossCode::WriterAttributesOmitted.kind()
                    && loss.message.contains("rhino:test:light#z-partial")
                    && loss.message.contains("attributes:")
                    && loss.message.contains("links")
            }));
            let decoded = crate::RhinoCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .expect("decode surviving light");
            assert!(decoded
                .report()
                .losses
                .iter()
                .all(|loss| { loss.code != crate::loss::RhinoLossCode::IntegrityFailure.kind() }));
            let lights = &decoded
                .ir()
                .native
                .namespace("rhino")
                .expect("rhino")
                .arenas()["lights"];
            assert_eq!(lights.len(), 3);
            assert_eq!(lights[0].field("name"), Some(json!("valid light")));
            assert_eq!(lights[0].field("archive_index"), Some(json!(0)));
            assert_eq!(
                lights[2].field("attributes"),
                Some(expected_attributes.clone())
            );
        }
    }
}
