// SPDX-License-Identifier: Apache-2.0

use super::{normalise, object_at_mut, probe_key, sweep, Step, UNKNOWN_KEY};
use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::{json, Map, Value};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    #[serde(rename = "items")]
    _items: Vec<Branch>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Branch {
    Strict {
        #[serde(rename = "inner")]
        _inner: StrictValue,
    },
    Loose {
        #[serde(rename = "inner")]
        _inner: LooseValue,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictValue {
    #[serde(rename = "value")]
    _value: u32,
}

#[derive(Deserialize)]
struct LooseValue {
    #[serde(rename = "value")]
    _value: u32,
}

#[test]
fn equal_child_keys_under_different_parent_variants_are_both_probed() {
    for kinds in [["strict", "loose"], ["loose", "strict"]] {
        let document = json!({"items": [
            {"kind": kinds[0], "inner": {"value": 1}},
            {"kind": kinds[1], "inner": {"value": 2}},
        ]});
        let swept = sweep(&document, &json!({"items": []}), |probe| {
            Document::deserialize(probe).is_ok()
        })
        .expect("every path comes from the document");
        assert_eq!(
            swept.accepting.into_iter().collect::<Vec<_>>(),
            ["/items/#/inner"],
        );
        assert_eq!(swept.swept, 5);
    }
}

#[test]
fn missing_probe_target_is_an_error_instead_of_a_skipped_probe() {
    let mut document = json!({"items": [null]});
    for path in [
        vec![Step::Key("missing".into())],
        vec![Step::Key("items".into()), Step::Index(1)],
        vec![Step::Key("items".into()), Step::Index(0)],
    ] {
        assert!(object_at_mut(&mut document, &path).is_err());
    }
}

#[test]
fn source_member_names_cannot_alias_path_separators() {
    assert_eq!(
        normalise(&[Step::Key("a/b~c".into()), Step::Index(0)]),
        "/a~1b~0c/#",
    );
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExistingSentinelDocument {
    #[serde(rename = "items")]
    _items: Vec<ExistingSentinelItem>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExistingSentinelItem {
    #[serde(rename = "value")]
    _value: u32,
    #[serde(rename = "zz_bogus")]
    _sentinel: u32,
}

#[test]
fn an_existing_sentinel_member_is_preserved_while_probing() {
    let document = json!({"items": [{"value": 1, "zz_bogus": 7}]});
    assert!(
        ExistingSentinelDocument::deserialize(&document).is_ok(),
        "the sentinel fixture must be admitted before the sweep runs"
    );
    let swept = sweep(&document, &json!({"items": []}), |probe| {
        ExistingSentinelDocument::deserialize(probe).is_ok()
    })
    .expect("every path comes from the document");

    assert!(
        swept.accepting.is_empty(),
        "the extra suffixed key is denied"
    );
}

#[test]
fn probe_key_uses_the_first_absent_suffix() {
    let fields = Map::from_iter([
        (UNKNOWN_KEY.to_owned(), Value::Null),
        (format!("{UNKNOWN_KEY}_"), Value::Null),
    ]);
    assert_eq!(probe_key(&fields), format!("{UNKNOWN_KEY}__"));
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StructuredMapDocument<T> {
    #[serde(rename = "parameters")]
    _parameters: BTreeMap<String, T>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StructuredMapValue {
    #[serde(rename = "required")]
    _required: StrictValue,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum StructuredMapVariant {
    Bound {
        #[serde(rename = "target")]
        _target: StrictValue,
    },
}

#[test]
fn populated_structured_enum_and_nonempty_array_maps_are_detected() {
    fn check<T: serde::de::DeserializeOwned>(value: Value) {
        let document = json!({"parameters": {"depth": value}});
        assert!(StructuredMapDocument::<T>::deserialize(&document).is_ok());
        for baseline in super::probe_values() {
            let mut probe = document.clone();
            probe["parameters"][UNKNOWN_KEY] = baseline;
            assert!(StructuredMapDocument::<T>::deserialize(&probe).is_err());
        }
        let swept = sweep(&document, &json!({"parameters": {}}), |probe| {
            StructuredMapDocument::<T>::deserialize(probe).is_ok()
        })
        .expect("every path comes from the document");
        assert_eq!(swept.accepting, BTreeSet::from(["/parameters".to_owned()]));
    }

    check::<StructuredMapValue>(json!({"required": {"value": 12}}));
    check::<StructuredMapVariant>(json!({"kind": "bound", "target": {"value": 12}}));
    check::<[StrictValue; 1]>(json!([{"value": 12}]));
}

#[test]
fn structured_map_owners_and_children_still_deny_unknown_fields() {
    let document = json!({"parameters": {"depth": {"required": {"value": 12}}}});
    for path in ["", "/parameters/depth", "/parameters/depth/required"] {
        let mut probe = document.clone();
        probe
            .pointer_mut(path)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(UNKNOWN_KEY.to_owned(), json!({"required": {"value": 12}}));
        assert!(StructuredMapDocument::<StructuredMapValue>::deserialize(&probe).is_err());
    }
}

#[test]
fn a_structured_existing_sentinel_remains_unchanged_in_all_probes() {
    let existing = json!({"required": {"value": 7}});
    let mut document = json!({"parameters": {}});
    document["parameters"][UNKNOWN_KEY] = existing.clone();
    let before = document.clone();
    // The callback is Fn, so record observation separately without changing
    // any document content supplied to the reader.
    let observed = std::cell::Cell::new(false);
    let swept = sweep(&document, &json!({"parameters": {}}), |probe| {
        if probe["parameters"].get(format!("{UNKNOWN_KEY}_")).is_some() {
            assert_eq!(probe["parameters"][UNKNOWN_KEY], existing);
            observed.set(true);
        }
        StructuredMapDocument::<StructuredMapValue>::deserialize(probe).is_ok()
    })
    .expect("every path comes from the document");
    assert!(observed.get());
    assert_eq!(swept.accepting, BTreeSet::from(["/parameters".to_owned()]));
    assert_eq!(document, before);
}
