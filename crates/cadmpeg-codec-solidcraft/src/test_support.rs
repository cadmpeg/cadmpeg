// SPDX-License-Identifier: Apache-2.0
//! Code-built SolidCraft designs for the unit, integration, and golden tests.

use cadmpeg_test_support::zip_fixture::{archive, RawEntry};
use serde_json::{json, Value};

/// The STEP text of the import fixtures.
pub(crate) const STEP_TEXT: &[u8] =
    b"ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\nENDSEC;\nEND-ISO-10303-21;\n";

/// The blob member name of `bytes` with `extension`.
pub(crate) fn blob_name(bytes: &[u8], extension: &str) -> String {
    format!("blobs/{}.{extension}", cadmpeg_ir::hash::sha256_hex(bytes))
}

/// A design document of `format` with one parameter and a box feature.
pub(crate) fn box_document(format: &str) -> Value {
    json!({
        "format": format,
        "name": "Block",
        "units": "mm",
        "params": [{"name": "w", "expr": "20 mm", "unit": "mm", "model": false}],
        "features": [{
            "id": 1,
            "name": "Box1",
            "suppressed": false,
            "type": "box",
            "corner": [0.0, 0.0, 0.0],
            "length": "w",
            "width": "10 mm",
            "height": "5 mm",
            "operation": "new_body"
        }],
        "appearances": {"bodies": {}}
    })
}

/// A design document with one STEP import whose blob is `step_blob`.
pub(crate) fn import_document(step_blob: &str) -> Value {
    json!({
        "format": "solidcraft/57",
        "name": "Imported",
        "units": "mm",
        "params": [],
        "features": [{
            "id": 1,
            "name": "Import1",
            "suppressed": false,
            "type": "import",
            "file": "part.step",
            "step": "",
            "step_blob": step_blob
        }]
    })
}

/// A version 1 container with `document` and the extra members.
pub(crate) fn container(document: &Value, extra: &[(&str, &[u8])]) -> Vec<u8> {
    let document = serde_json::to_vec(document).unwrap();
    let mut entries: Vec<RawEntry> = extra
        .iter()
        .map(|(name, bytes)| RawEntry::stored(name, bytes))
        .collect();
    entries.push(RawEntry::stored("document.json", &document));
    entries.push(RawEntry::stored(
        "container.json",
        br#"{"format":"solidcraft-container/1"}"#,
    ));
    archive(&entries)
}

/// A container whose import blob is present and matches its name.
pub(crate) fn import_container() -> Vec<u8> {
    let name = blob_name(STEP_TEXT, "step");
    container(&import_document(&name), &[(&name, STEP_TEXT)])
}
