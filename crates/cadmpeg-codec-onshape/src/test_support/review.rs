// SPDX-License-Identifier: Apache-2.0
//! Stored synthetic bundle editing and validation adapters.
use cadmpeg_core::json;
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};

pub(crate) fn members(bytes: &[u8], edit: impl FnOnce(&mut BTreeMap<String, Vec<u8>>)) -> Vec<u8> {
    let mut source = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut members = BTreeMap::new();
    for index in 0..source.len() {
        let mut member = source.by_index(index).unwrap();
        let mut bytes = Vec::new();
        member.read_to_end(&mut bytes).unwrap();
        members.insert(member.name().unwrap().into_owned(), bytes);
    }
    edit(&mut members);
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in members {
        output
            .start_file(
                name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .unwrap();
        output.write_all(&bytes).unwrap();
    }
    output.finish().unwrap().into_inner()
}
pub(crate) fn edit(bytes: &[u8], mut edit: impl FnMut(&str, &mut Value)) -> Vec<u8> {
    members(bytes, |members| {
        for (path, bytes) in members.iter_mut().filter(|(path, _)| {
            path.rsplit_once('.')
                .is_some_and(|(_, extension)| extension == "json")
        }) {
            let mut value = json::from_slice(bytes).unwrap();
            edit(path, &mut value);
            *bytes = json::to_vec(&value).unwrap();
        }
    })
}
pub(crate) fn decode(bytes: &[u8]) -> DecodeResult {
    super::codec()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap()
}
pub(crate) fn wire(result: &DecodeResult) -> Value {
    let mut wire = cadmpeg_test_support::wire::in_memory_json(result.ir());
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .sort_by_key(|f| f["ordinal"].as_u64());
    wire
}
pub(crate) fn validate(result: DecodeResult) {
    let (ir, report, fidelity) = result.into_parts();
    let validation =
        cadmpeg_ir::validate_neutral_with_source_fidelity(&ir, &fidelity, report.losses).unwrap();
    assert!(validation.is_ok(), "{validation:#?}");
}
pub(crate) fn parameter<'a>(feature: &'a mut Value, name: &str) -> &'a mut Value {
    feature["parameters"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|p| p["parameterId"] == name)
        .unwrap()
}
pub(crate) fn variable(name: &str, id: &str, kind: &str, expression: &str) -> Value {
    let key = if kind == "ANGLE" {
        "angleValue"
    } else {
        "lengthValue"
    };
    json!({"featureId":id,"featureType":"assignVariable","parameters":[
        {"parameterId":"name","value":name},{"parameterId":"variableType","value":kind},
        {"parameterId":key,"expression":expression}]})
}
pub(crate) fn two_body_step() -> Vec<u8> {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let geometry = json::to_value(&ir.geometry).unwrap();
    let mut other = geometry.clone();
    rename_geometry(&mut other);
    let mut geometry = geometry;
    for (arena, values) in other.as_object().unwrap() {
        geometry[arena]
            .as_array_mut()
            .unwrap()
            .extend(values.as_array().unwrap().iter().cloned());
    }
    ir.geometry = json::from_value(geometry).unwrap();
    let plan = cadmpeg_codec_step::StepCodec::default()
        .plan(EncodeInput::new(&ir, None).with_target(TargetRequest::Explicit("step:ap214")))
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    bytes
}

fn rename_geometry(value: &mut Value) {
    match value {
        Value::String(text) => *text = text.replacen(":cube:", ":second-cube:", 1),
        Value::Array(values) => {
            for value in values {
                rename_geometry(value);
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                rename_geometry(value);
            }
        }
        _ => {}
    }
}
