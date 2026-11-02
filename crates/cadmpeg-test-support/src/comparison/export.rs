// SPDX-License-Identifier: Apache-2.0
//! §5.1 and §5.2 export allowances, computed from the first decode alone.
//!
//! A write without fidelity may drop an `unknowns` stub, a record field whose
//! only value is such a stub, and any record that needs a removed record
//! (§4.5 rule 2, W1); a record that lists one loses only that entry. The
//! export report's dropped native dispositions allow the same for the records
//! they name. Losses never add an allowance.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde_json::Value;

use super::{graph::Document, Allowances};

impl Allowances {
    /// Build the §5.1 and §5.2 allowances of a write without fidelity.
    ///
    /// The `export_report` is the `ExportReport` JSON of the write. Its
    /// dropped native dispositions extend the stub closure with the records
    /// they name; a report without dispositions contributes nothing beyond
    /// the stubs.
    ///
    /// # Errors
    ///
    /// Refuses a document without an object-valued model or identified rows.
    pub fn export(before: &Value, export_report: &Value) -> Result<Self, String> {
        let stubs = stub_ids(before);
        let pruned = without_stub_fields(before, &stubs);
        let records = Document::read(&pruned, &BTreeSet::new())?;
        let mut users = BTreeMap::<String, BTreeSet<String>>::new();
        for record in &records.records {
            let mut required = BTreeSet::new();
            required_references(&record.value, &records.ids, &mut required);
            for target in required {
                users.entry(target).or_default().insert(record.id.clone());
            }
        }
        let mut removed = stubs.clone();
        for id in dropped_report_records(before, export_report) {
            removed.insert(id);
        }
        let mut queue = removed.iter().cloned().collect::<VecDeque<_>>();
        while let Some(target) = queue.pop_front() {
            for owner in users.get(&target).into_iter().flatten() {
                if removed.insert(owner.clone()) {
                    queue.push_back(owner.clone());
                }
            }
        }
        Ok(Self {
            removed,
            removed_fields: stub_fields(before, &stubs),
        })
    }
}

/// The `unknowns` stub identities of one decoded document.
fn stub_ids(document: &Value) -> BTreeSet<String> {
    document
        .get("native")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|namespaces| namespaces.values())
        .filter_map(|arenas| arenas.get("unknowns")?.as_array())
        .flatten()
        .filter_map(|row| row.get("id")?.as_str().map(str::to_owned))
        .collect()
}

/// (record id, field) pairs whose value is one stub identity.
fn stub_fields(document: &Value, stubs: &BTreeSet<String>) -> BTreeSet<(String, String)> {
    let mut fields = BTreeSet::new();
    for_each_record(document, |id, record| {
        for (key, value) in record {
            if key != "id" && value.as_str().is_some_and(|text| stubs.contains(text)) {
                fields.insert((id.to_owned(), key.clone()));
            }
        }
    });
    fields
}

/// `document` with every stub-only field removed.
fn without_stub_fields(document: &Value, stubs: &BTreeSet<String>) -> Value {
    let mut pruned = document.clone();
    for_each_record_mut(&mut pruned, |record| {
        record.retain(|key, value| {
            key == "id" || !value.as_str().is_some_and(|text| stubs.contains(text))
        });
    });
    pruned
}

/// Records of `before` whose export disposition is dropped.
///
/// An arena the report does not declare is dropped, matching
/// `NativeDispositions::disposition`. A report without `native_dispositions`
/// names nothing.
fn dropped_report_records(before: &Value, export_report: &Value) -> BTreeSet<String> {
    let Some(namespaces) = export_report
        .get("native_dispositions")
        .and_then(|dispositions| dispositions.get("namespaces"))
        .and_then(Value::as_object)
    else {
        return BTreeSet::new();
    };
    let Some(native) = before.get("native").and_then(Value::as_object) else {
        return BTreeSet::new();
    };
    let mut dropped = BTreeSet::new();
    for (namespace, arenas) in native {
        let Some(arenas) = arenas.as_object() else {
            continue;
        };
        for (arena, rows) in arenas {
            let Some(rows) = rows.as_array() else {
                continue;
            };
            let declared = namespaces
                .get(namespace)
                .and_then(|namespaces| namespaces.get("arenas"))
                .and_then(|arenas| arenas.get(arena));
            let default_dropped = declared
                .and_then(|declared| declared.get("disposition"))
                .and_then(Value::as_str)
                .is_none_or(|disposition| disposition == "dropped");
            let exceptions = declared
                .and_then(|declared| declared.get("records"))
                .and_then(Value::as_object);
            for row in rows {
                let Some(id) = row.get("id").and_then(Value::as_str) else {
                    continue;
                };
                let record_dropped = exceptions
                    .and_then(|exceptions| exceptions.get(id))
                    .and_then(Value::as_str)
                    .is_some_and(|disposition| disposition == "dropped")
                    || (default_dropped
                        && !exceptions.is_some_and(|exceptions| exceptions.contains_key(id)));
                if record_dropped {
                    dropped.insert(id.to_owned());
                }
            }
        }
    }
    dropped
}

/// Records a record needs: a string equal to a record identity outside an
/// array element. Array elements list their referents; the comparison prunes
/// only the affected entry, retaining the holder.
pub(super) fn required_references(
    value: &Value,
    identities: &BTreeMap<String, usize>,
    required: &mut BTreeSet<String>,
) {
    match value {
        Value::String(id) if identities.contains_key(id) => {
            required.insert(id.clone());
        }
        Value::Object(fields) => {
            for (key, value) in fields {
                if identities.contains_key(key) {
                    required.insert(key.clone());
                }
                required_references(value, identities, required);
            }
        }
        Value::Array(_) => {}
        _ => {}
    }
}

fn rows(document: &Value) -> Vec<&Value> {
    let model = document
        .get("model")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|model| model.values());
    let native = document
        .get("native")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|namespaces| namespaces.values())
        .filter_map(Value::as_object)
        .flat_map(|arenas| arenas.values());
    model
        .chain(native)
        .filter_map(Value::as_array)
        .flatten()
        .collect()
}

fn for_each_record(document: &Value, mut visit: impl FnMut(&str, &serde_json::Map<String, Value>)) {
    for row in rows(document) {
        if let (Some(id), Some(record)) = (row.get("id").and_then(Value::as_str), row.as_object()) {
            visit(id, record);
        }
    }
}

fn for_each_record_mut(
    document: &mut Value,
    mut visit: impl FnMut(&mut serde_json::Map<String, Value>),
) {
    let mut visit_rows = |rows: &mut Value| {
        for row in rows.as_array_mut().into_iter().flatten() {
            if let Some(record) = row.as_object_mut() {
                visit(record);
            }
        }
    };
    if let Some(model) = document.get_mut("model").and_then(Value::as_object_mut) {
        model.values_mut().for_each(&mut visit_rows);
    }
    if let Some(native) = document.get_mut("native").and_then(Value::as_object_mut) {
        for arenas in native.values_mut().filter_map(Value::as_object_mut) {
            arenas.values_mut().for_each(&mut visit_rows);
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::Allowances;

    fn report() -> serde_json::Value {
        json!({"native_dispositions": {"namespaces": {}}})
    }

    #[test]
    fn stubs_their_only_field_and_their_dependants_may_go() {
        let before = json!({
            "model": {"points": [
                {"id": "p", "at": "step:source:part#s"},
                {"id": "q", "at": "elsewhere"},
                {"id": "r", "carrier": {"part": "step:source:part#s"}},
            ]},
            "native": {"step": {"unknowns": [{"id": "step:source:part#s"}]}},
        });
        let allowances = Allowances::export(&before, &report()).expect("stub allowances");
        // A stub-only field is pruned from its record; the record stays.
        assert!(!allowances.removed.contains("p"));
        // A nested stub reference needs the stub, so its record goes too.
        assert!(allowances.removed.contains("step:source:part#s"));
        assert!(allowances.removed.contains("r"));
        assert!(!allowances.removed.contains("q"));
        assert_eq!(
            allowances.removed_fields,
            [("p".to_owned(), "at".to_owned())].into()
        );
    }

    #[test]
    fn a_record_with_a_stub_field_keeps_its_record_and_its_other_fields() {
        let before = json!({
            "model": {"points": [{"id": "p", "at": "step:source:part#s", "kept": 1}]},
            "native": {"step": {"unknowns": [{"id": "step:source:part#s"}]}},
        });
        let allowances = Allowances::export(&before, &report()).expect("stub allowances");
        assert!(!allowances.removed.contains("p"));
        assert!(allowances
            .removed_fields
            .contains(&("p".to_owned(), "at".to_owned())));
        assert!(!allowances
            .removed_fields
            .contains(&("p".to_owned(), "kept".to_owned())));
    }

    #[test]
    fn records_in_a_dropped_native_arena_may_go() {
        let before = json!({
            "model": {"points": [{"id": "p", "value": 1}]},
            "native": {"step": {
                "kept": [{"id": "k", "value": 1}],
                "omitted": [{"id": "o", "value": 2}],
            }},
        });
        let report = json!({"native_dispositions": {"namespaces": {"step": {"arenas": {
            "kept": {"disposition": "written"},
            "omitted": {"disposition": "dropped"},
        }}}}});
        let allowances = Allowances::export(&before, &report).expect("report allowances");
        assert!(allowances.removed.contains("o"));
        assert!(!allowances.removed.contains("k"));
        assert!(!allowances.removed.contains("p"));
    }

    #[test]
    fn an_undeclared_arena_is_dropped_when_dispositions_are_present() {
        let before = json!({
            "model": {"points": []},
            "native": {"step": {"absent": [{"id": "o"}]}},
        });
        let report = json!({"native_dispositions": {"namespaces": {"step": {"arenas": {}}}}});
        let allowances = Allowances::export(&before, &report).expect("report allowances");
        assert!(allowances.removed.contains("o"));
    }

    #[test]
    fn a_record_exception_overrides_its_arena_default() {
        let before = json!({
            "model": {"points": []},
            "native": {"step": {"mixed": [{"id": "kept"}, {"id": "gone"}]}},
        });
        let report = json!({"native_dispositions": {"namespaces": {"step": {"arenas": {
            "mixed": {"disposition": "dropped", "records": {"kept": "written"}},
        }}}}});
        let allowances = Allowances::export(&before, &report).expect("report allowances");
        assert!(allowances.removed.contains("gone"));
        assert!(!allowances.removed.contains("kept"));
    }

    #[test]
    fn a_report_without_dispositions_contributes_only_stubs() {
        let before = json!({
            "model": {"points": []},
            "native": {"step": {"omitted": [{"id": "o"}]}},
        });
        let allowances = Allowances::export(&before, &json!({})).expect("stub-only allowances");
        assert!(!allowances.removed.contains("o"));
        assert!(allowances.removed_fields.is_empty());
    }
}
