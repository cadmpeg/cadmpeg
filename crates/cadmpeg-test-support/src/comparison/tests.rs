// SPDX-License-Identifier: Apache-2.0
//! The §5.3 acceptance table, including tolerance-boundary and alias controls.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};

use super::{agree, assert_decodes_agree, Allowances, CrossProjection, Mode};

fn document(rows: &Value) -> Value {
    json!({"model": {"points": rows}, "native": {}})
}

fn accepts(before: &Value, after: &Value) -> bool {
    agree(before, after, &Allowances::default(), Mode::Same).is_ok()
}

#[test]
fn duplicate_record_removed_fails() {
    let before = document(&json!([{"id":"a","x":1.0},{"id":"b","x":1.0}]));
    let after = document(&json!([{"id":"x","x":1.0}]));
    assert!(!accepts(&before, &after));
}

#[test]
fn records_reordered_and_renumbered_pass() {
    let before =
        document(&json!([{"id":"a","ordinal":10,"x":1.0},{"id":"b","ordinal":20,"x":2.0}]));
    let after = document(&json!([{"id":"y","ordinal":99,"x":2.0},{"id":"x","ordinal":40,"x":1.0}]));
    assert_decodes_agree(&before, &after, &Allowances::default(), Mode::Same);
}

#[test]
fn relative_float_difference_at_one_e_minus_thirteen_passes() {
    let before = document(&json!([{"id":"a","x":100.0}]));
    let after = document(&json!([{"id":"x","x":100.0 * (1.0 + 1.0e-13)}]));
    assert!(accepts(&before, &after));
}

#[test]
fn relative_float_difference_at_one_e_minus_nine_fails() {
    let before = document(&json!([{"id":"a","x":100.0}]));
    let after = document(&json!([{"id":"x","x":100.0 * (1.0 + 1.0e-9)}]));
    assert!(!accepts(&before, &after));
}

#[test]
fn missing_native_record_in_same_mode_fails() {
    let before = json!({"model":{},"native":{"test":{"rows":[{"id":"n","value":1}]}}});
    let after = json!({"model":{},"native":{"test":{"rows":[]}}});
    assert!(!accepts(&before, &after));
}

#[test]
fn default_allowances_reject_record_removal() {
    let before = document(&json!([{"id":"a","x":1.0}]));
    let after = document(&json!([]));
    assert!(!accepts(&before, &after));
}

#[test]
fn extra_record_passes() {
    let before = document(&json!([{"id":"a","x":1.0}]));
    let after = document(&json!([{"id":"x","x":1.0},{"id":"y","x":2.0}]));
    assert!(accepts(&before, &after));
}

#[test]
fn replacing_a_typed_variant_with_native_fails() {
    let before = document(&json!([{"id":"a","definition":{"kind":"line","length":2.0}}]));
    let after = document(&json!([{"id":"x","definition":{"kind":"native","length":2.0}}]));
    assert!(!accepts(&before, &after));
}

#[test]
fn tiny_libm_residual_compares_with_zero() {
    let before = document(&json!([{"id":"a","x":6.123_233_995_736_766e-17}]));
    let after = document(&json!([{"id":"x","x":0.0}]));
    assert!(accepts(&before, &after));
}

#[test]
fn tolerated_difference_across_a_rounding_boundary_passes() {
    let boundary = 1.234_567_895;
    let before = document(&json!([{"id":"a","x":boundary * (1.0 - 5.0e-14)}]));
    let after = document(&json!([{"id":"x","x":boundary * (1.0 + 5.0e-14)}]));
    assert_ne!(
        super::graph::shape(
            &before["model"]["points"][0]["x"],
            &BTreeMap::default(),
            true
        ),
        super::graph::shape(
            &after["model"]["points"][0]["x"],
            &BTreeMap::default(),
            true
        )
    );
    assert!(accepts(&before, &after));
}

fn face_removal() -> (Value, Value) {
    (
        json!({"model":{"faces":[{"id":"f1","value":1},{"id":"f2","value":2}],"shells":[{"id":"s","members":[{"face":"f1"},{"face":"f2"}]}]}}),
        json!({"model":{"faces":[{"id":"g1","value":1}],"shells":[{"id":"t","members":[{"face":"g1"}]}]}}),
    )
}

#[test]
fn shell_can_lose_only_an_allowed_face_entry() {
    let (before, after) = face_removal();
    let allowances = Allowances {
        removed: ["f2".to_owned()].into(),
        removed_fields: BTreeSet::new(),
    };
    assert!(agree(&before, &after, &allowances, Mode::Same).is_ok());
}

#[test]
fn shell_cannot_lose_an_unallowed_face_entry() {
    let (before, after) = face_removal();
    assert!(!accepts(&before, &after));
}

#[test]
fn renumbered_reference_graph_preserves_aliasing() {
    let before = json!({"model":{"points":[{"id":"a","x":0.0},{"id":"b","x":0.0}],"edges":[{"id":"e","first":"a","second":"a"}]}});
    let after = json!({"model":{"points":[{"id":"x","x":0.0},{"id":"y","x":0.0}],"edges":[{"id":"z","first":"x","second":"x"}]}});
    assert!(accepts(&before, &after));
    let mut edited = after;
    edited["model"]["edges"][0]["second"] = json!("y");
    assert!(!accepts(&before, &edited));
}

#[test]
fn parameter_ordinals_rank_within_each_owner() {
    let before = json!({"model":{"features":[{"id":"a","kind":"one"},{"id":"b","kind":"two"}],"parameters":[{"id":"p","owner":"a","ordinal":20,"value":1},{"id":"q","owner":"b","ordinal":5,"value":2}]}});
    let after = json!({"model":{"features":[{"id":"x","kind":"one"},{"id":"y","kind":"two"}],"parameters":[{"id":"r","owner":"y","ordinal":90,"value":2},{"id":"s","owner":"x","ordinal":3,"value":1}]}});
    assert!(accepts(&before, &after));
}

#[test]
fn top_level_source_and_unknowns_are_not_record_content() {
    let mut before = document(&json!([{"id":"a","x":1.0,"native_ref":"old","source":"old"}]));
    let mut after = document(&json!([{"id":"x","x":1.0,"native_ref":"new","source":"new"}]));
    before["source"] = json!({"path":"before"});
    before["unknowns"] = json!([1]);
    after["source"] = json!({"path":"after"});
    after["unknowns"] = json!([2, 3]);
    assert!(accepts(&before, &after));
}

#[test]
fn removal_in_a_nested_list_preserves_the_holder() {
    let before = json!({"model":{"faces":[{"id":"f","value":1}],"shells":[{"id":"s","groups":[{"name":"kept","members":["f"]}]}]}});
    let after =
        json!({"model":{"faces":[],"shells":[{"id":"t","groups":[{"name":"kept","members":[]}]}]}});
    let allowances = Allowances {
        removed: ["f".to_owned()].into(),
        removed_fields: BTreeSet::new(),
    };
    assert!(agree(&before, &after, &allowances, Mode::Same).is_ok());
    let mut dropped_holder = after;
    dropped_holder["model"]["shells"][0]["groups"] = json!([]);
    assert!(agree(&before, &dropped_holder, &allowances, Mode::Same).is_err());
}

#[test]
fn reference_keys_follow_the_same_global_identity_map() {
    let before = json!({"model":{"parameters":[{"id":"p","value":1}],"configurations":[{"id":"c","overrides":{"p":2}}]}});
    let after = json!({"model":{"parameters":[{"id":"q","value":1}],"configurations":[{"id":"d","overrides":{"q":2}}]}});
    assert!(accepts(&before, &after));
}

#[test]
fn an_identity_shaped_string_without_a_record_is_still_content() {
    let before = document(&json!([{"id":"a","label":"test:missing#one"}]));
    let after = document(&json!([{"id":"x","label":"test:missing#two"}]));
    assert!(!accepts(&before, &after));
}

#[test]
fn nested_list_order_is_preserved() {
    let before = document(&json!([{"id":"a","values":[1,2]}]));
    let after = document(&json!([{"id":"x","values":[2,1]}]));
    assert!(!accepts(&before, &after));
}

#[test]
fn an_added_record_does_not_shift_the_paired_ordinal_ranks() {
    let before = document(&json!([{"id":"a","ordinal":10,"x":1.0}]));
    let after = document(&json!([{"id":"new","ordinal":0,"x":0.0},{"id":"x","ordinal":1,"x":1.0}]));
    assert!(accepts(&before, &after));
}

#[test]
fn paired_ordinal_order_cannot_be_reversed() {
    let before =
        document(&json!([{"id":"a","ordinal":10,"x":1.0},{"id":"b","ordinal":20,"x":2.0}]));
    let after = document(&json!([{"id":"x","ordinal":1,"x":2.0},{"id":"y","ordinal":2,"x":1.0}]));
    assert!(!accepts(&before, &after));
}

#[test]
fn a_reference_map_key_can_equal_a_record_field_name() {
    let before = json!({"model":{"parameters":[{"id":"value","value":1}],"configurations":[{"id":"c","overrides":{"value":2}}]}});
    let after = json!({"model":{"parameters":[{"id":"x","value":1}],"configurations":[{"id":"d","overrides":{"x":2}}]}});
    assert!(accepts(&before, &after));
}

#[test]
fn a_reference_map_key_cannot_switch_to_a_different_record() {
    let before = json!({"model":{"parameters":[{"id":"a","value":1},{"id":"b","value":2}],"configurations":[{"id":"c","overrides":{"a":3}}]}});
    let after = json!({"model":{"parameters":[{"id":"x","value":1},{"id":"y","value":2}],"configurations":[{"id":"d","overrides":{"y":3}}]}});
    assert!(!accepts(&before, &after));
}

#[test]
fn a_minted_source_object_is_not_content() {
    let before = document(&json!([{"id":"a","x":1.0}]));
    let after = document(
        &json!([{"id":"x","x":1.0,"source_object":{"format":"rhino","object_id":"2a6f0b31-9b8a-4f4e-9c9a-9b8a4f4e9c9a"}}]),
    );
    assert!(accepts(&before, &after));
    let mut changed = after;
    changed["model"]["points"][0]["x"] = json!(2.0);
    assert!(!accepts(&before, &changed));
}

#[test]
fn a_removal_named_only_by_a_loss_still_fails() {
    let before = document(&json!([{"id":"a","x":1.0}]));
    let after = document(&json!([]));
    let report = json!({"losses": [{
        "code": "decode/record.unreadable",
        "severity": "error",
        "message": "part step.simple_entity_instance stream=\"\" offset=0 length=10: test",
        "provenance": {"format": "step", "offset": 0, "byte_len": 10},
    }]});
    let allowances = Allowances::given_up(&before, &report).expect("losses with structured spans");
    assert!(agree(&before, &after, &allowances, Mode::Same).is_err());
}

#[test]
fn an_export_without_stubs_or_dispositions_allows_nothing() {
    let before = document(&json!([{"id":"a","x":1.0}]));
    let after = document(&json!([]));
    let allowances = Allowances::export(&before, &json!({})).expect("a stub-free export");
    assert!(agree(&before, &after, &allowances, Mode::Same).is_err());
}

#[test]
fn an_export_may_drop_a_stub_and_its_dependants() {
    let before = json!({
        "model": {"points": [
            {"id": "p", "at": "step:source:part#s"},
            {"id": "r", "carrier": {"part": "step:source:part#s"}},
        ]},
        "native": {"step": {"unknowns": [{"id": "step:source:part#s"}]}},
    });
    // The stub field goes, its record stays; the nested dependant goes.
    let after = json!({
        "model": {"points": [{"id": "x"}]},
        "native": {"step": {"unknowns": []}},
    });
    let allowances = Allowances::export(&before, &json!({})).expect("stub allowances");
    assert!(agree(&before, &after, &allowances, Mode::Same).is_ok());
}

fn cross(projection: &[&str]) -> Mode {
    Mode::Cross(CrossProjection::new(
        projection.iter().map(|key| (*key).to_owned()).collect(),
    ))
}

#[test]
fn cross_mode_ignores_a_key_only_in_after() {
    let before = document(&json!([{"id":"a","x":1.0}]));
    let after = document(&json!([{"id":"x","x":1.0,"writer_default":true}]));
    assert!(agree(&before, &after, &Allowances::default(), cross(&[])).is_ok());
}

#[test]
fn cross_mode_fails_a_key_only_in_before() {
    let before = document(&json!([{"id":"a","x":1.0,"required":true}]));
    let after = document(&json!([{"id":"x","x":1.0}]));
    assert!(agree(&before, &after, &Allowances::default(), cross(&[])).is_err());
}

#[test]
fn cross_mode_projects_the_named_keys_on_both_sides() {
    let before = document(&json!([{"id":"a","x":1.0,"surface_form":1}]));
    let after = document(&json!([{"id":"x","x":1.0,"surface_form":2}]));
    assert!(agree(&before, &after, &Allowances::default(), cross(&[])).is_err());
    assert!(agree(
        &before,
        &after,
        &Allowances::default(),
        cross(&["surface_form"])
    )
    .is_ok());
}

#[test]
fn cross_mode_skips_the_native_namespaces() {
    let before = json!({"model":{},"native":{"step":{"tables":[{"id":"n","value":1}]}}});
    let after = json!({"model":{},"native":{"iges":{"tables":[]}}});
    assert!(agree(&before, &after, &Allowances::default(), cross(&[])).is_ok());
}
