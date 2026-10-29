// SPDX-License-Identifier: Apache-2.0
use serde_json::json;

use super::{is_source_layout_key, property_map_findings};

#[test]
fn check_2_keys_are_recognised() {
    for key in [
        "layout_sha256",
        "record_count",
        "stream.offset",
        "section.header",
        "layer.name",
        "raw_bytes",
    ] {
        assert!(is_source_layout_key(key), "{key}");
    }
    for key in ["Label", "MapMode", "description", "account"] {
        assert!(!is_source_layout_key(key), "{key}");
    }
}

#[test]
fn a_native_field_and_an_identity_value_fail() {
    let document = json!({
        "model": {"parameters": [{"id": "nx:p", "native_ref": "nx:n", "properties": {
            "unit": "mm", "target": "nx:n", "Note": "kept"
        }}]},
        "native": {"nx": {"rows": [{"id": "nx:n", "unit": "mm"}]}},
    });
    let findings = property_map_findings("nx", &document);
    assert_eq!(findings.len(), 2, "{findings:?}");
    assert!(findings.iter().all(|finding| !finding.contains("Note")));
}

#[test]
fn freecad_source_properties_are_empty_and_owned_property_records_are_native() {
    let document = json!({
        "model": {"features": [{"id": "fcstd:f", "native_ref": "fcstd:o",
                                 "source_properties": {"Label": "Box"}}]},
        "native": {"fcstd": {"properties": [{"id": "fcstd:p", "owner": "fcstd:o", "name": "Label"}],
                             "objects": [{"id": "fcstd:o"}]}},
    });
    let findings = property_map_findings("fcstd", &document);
    assert_eq!(findings.len(), 2, "{findings:?}");
}
