// SPDX-License-Identifier: Apache-2.0
use super::fixture;
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
            let mut response = serde_json::from_slice(&bytes).unwrap();
            edit(&mut response);
            bytes = serde_json::to_vec(&response).unwrap();
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
