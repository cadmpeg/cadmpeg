// SPDX-License-Identifier: Apache-2.0
use super::fixture;
use cadmpeg_core::serde_erase::json;
use serde_json::Value;
use std::io::{Cursor, Read, Write};

pub(crate) fn base_fixture() -> Vec<u8> {
    fixture("default-plane-regions")
}

pub(crate) fn changed(edit: impl FnMut(&mut Value)) -> Vec<u8> {
    changed_fixture(&base_fixture(), edit)
}

pub(crate) fn changed_fixture(fixture: &[u8], mut edit: impl FnMut(&mut Value)) -> Vec<u8> {
    let mut archive = zip::ZipArchive::new(Cursor::new(fixture)).unwrap();
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..archive.len() {
        let mut member = archive.by_index(i).unwrap();
        let name = member.name().unwrap().into_owned();
        let mut bytes = Vec::new();
        member.read_to_end(&mut bytes).unwrap();
        if name.ends_with("features.json") {
            let mut response = json::from_slice(&bytes).unwrap();
            edit(&mut response);
            bytes = json::to_vec(&response).unwrap();
        }
        output
            .start_file(name.as_str(), zip::write::SimpleFileOptions::default())
            .unwrap();
        output.write_all(&bytes).unwrap();
    }
    output.finish().unwrap().into_inner()
}

pub(crate) fn selected(response: &mut Value) -> &mut Value {
    &mut response["features"][1]["parameters"][2]["queries"][0]
}

/// `fixture` with `variables` placed before its features. Every later feature moves down by
/// as many rollback positions, so the body details captured at those rollbacks move with it.
pub(crate) fn with_variables(
    fixture: &[u8],
    variables: &[Value],
    mut edit: impl FnMut(&mut Value),
) -> Vec<u8> {
    let shift = cadmpeg_core::decode::u64_from_index(variables.len());
    let mut archive = zip::ZipArchive::new(Cursor::new(fixture)).unwrap();
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..archive.len() {
        let mut member = archive.by_index(i).unwrap();
        let mut name = member.name().unwrap().into_owned();
        let mut bytes = Vec::new();
        member.read_to_end(&mut bytes).unwrap();
        if name.ends_with("features.json") {
            let mut response: Value = json::from_slice(&bytes).unwrap();
            edit(&mut response);
            let rows = response["features"].as_array_mut().unwrap();
            for (at, row) in variables.iter().enumerate() {
                rows.insert(at, row.clone());
            }
            bytes = json::to_vec(&response).unwrap();
        } else if name.ends_with("bodydetails/index.json") {
            let indices: Vec<u64> = json::from_slice(&bytes).unwrap();
            let moved: Vec<u64> = indices.iter().map(|i| i + shift).collect();
            bytes = json::to_vec(&moved).unwrap();
        } else if let Some((folder, file)) = name.rsplit_once("bodydetails/") {
            let index: u64 = file.strip_suffix(".json").unwrap().parse().unwrap();
            name = format!("{folder}bodydetails/{}.json", index + shift);
        }
        output
            .start_file(name.as_str(), zip::write::SimpleFileOptions::default())
            .unwrap();
        output.write_all(&bytes).unwrap();
    }
    output.finish().unwrap().into_inner()
}

pub(crate) fn variable(
    feature: &str,
    name: &str,
    kind: &str,
    key: &str,
    expression: &str,
) -> Value {
    cadmpeg_core::json!({"featureId":feature,"featureType":"assignVariable","parameters":[
        {"parameterId":"mode","value":"ASSIGNED"},{"parameterId":"variableType","value":kind},
        {"parameterId":"name","value":name},{"parameterId":key,"expression":expression}]})
}
