// SPDX-License-Identifier: Apache-2.0
//! Each exactness field path of a decode names a field of the record with its
//! id in the written CADIR document (§10).

use std::collections::BTreeMap;

use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::annotations::AnnotationTarget;
use serde_json::Value;

use crate::test_support::test_om::segment_om_record_area_with_input_store_payload;
use crate::test_support::test_prt::{composed_feature_history_prt, prt_with_named_payloads};

/// The records of the neutral arenas of a written document, by id.
fn written_records(document: &Value) -> BTreeMap<&str, &Value> {
    let mut records = BTreeMap::new();
    for home in ["geometry", "model", "design", "process"] {
        let arenas = document[home]
            .as_object()
            .into_iter()
            .flat_map(|home| home.values());
        for record in arenas.filter_map(Value::as_array).flatten() {
            if let Some(id) = record.get("id").and_then(Value::as_str) {
                records.insert(id, record);
            }
        }
    }
    records
}

fn names_a_field(mut value: &Value, path: &str) -> bool {
    for component in path.split('.') {
        let next = match value {
            Value::Object(object) => object.get(component),
            Value::Array(array) => component
                .parse::<usize>()
                .ok()
                .and_then(|index| array.get(index)),
            _ => None,
        };
        let Some(next) = next else {
            return false;
        };
        value = next;
    }
    true
}

#[test]
fn exactness_field_paths_name_fields_of_the_written_records() {
    let mut checked = Vec::new();
    for bytes in [
        composed_feature_history_prt(),
        prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            segment_om_record_area_with_input_store_payload(),
        )]),
    ] {
        let result = super::decode(bytes);
        let document = json::to_value(result.ir()).expect("serialize the document");
        let records = written_records(&document);
        for (key, note) in result.source_fidelity().annotations.exactness() {
            let AnnotationTarget::Entity(id) = key.target() else {
                continue;
            };
            let Some(record) = records.get(id) else {
                continue;
            };
            for path in note.fields().keys() {
                assert!(
                    names_a_field(record, path.as_str()),
                    "{id}: `{path}` names no field of {record}"
                );
                checked.push(format!("{id} {path}"));
            }
        }
    }
    for restated in [
        "nx:om-entry-0:parameter#399 scope",
        "nx:om-entry-0:parameter#399 kind",
        "nx:om-entry-0:parameter#399 value",
    ] {
        assert!(checked.iter().any(|path| path == restated), "{checked:?}");
    }
}
