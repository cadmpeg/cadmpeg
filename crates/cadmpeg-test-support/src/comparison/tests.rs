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
fn resolved_insert_body_self_references_follow_feature_identity() {
    let feature = |id: &str, native_ref: &str, selection: &str| {
        json!({
            "id": id,
            "native_ref": native_ref,
            "definition": {"definition": "insert_bodies", "bodies": {
                "kind": "resolved", "value": {"native": selection}
            }}
        })
    };
    let before = json!({"model": {"features": [feature("a", "scope-a", "scope-a")]}, "native": {}});
    let after = json!({"model": {"features": [feature("b", "scope-b", "scope-b")]}, "native": {}});
    assert!(accepts(&before, &after));
    let changed = json!({"model": {"features": [feature("b", "scope-b", "independent-selection")]}, "native": {}});
    assert!(!accepts(&before, &changed));
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
fn cross_pairing_reassigns_a_flexible_match_to_preserve_a_required_match() {
    let before = document(&json!([
        {"id":"a", "kind":"line"},
        {"id":"b", "kind":"line", "coordinate":1}
    ]));
    let after = document(&json!([
        {"id":"x", "kind":"line", "coordinate":1, "extra":true},
        {"id":"y", "kind":"line", "coordinate":2, "extra":true}
    ]));
    assert_decodes_agree(
        &before,
        &after,
        &Allowances::default(),
        Mode::Cross(CrossProjection::default()),
    );
}

#[test]
fn cross_pairing_uses_source_fields_for_graph_refinement() {
    let before = json!({"model": {
        "points": [{"id":"p1","coordinate":1},{"id":"p2","coordinate":2}],
        "edges": [{"id":"e1","point":"p1"},{"id":"e2","point":"p2"}],
        "coedges": [{"id":"c1","edge":"e1"},{"id":"c2","edge":"e2"}]
    }, "native": {}});
    let after = json!({"model": {
        "points": [{"id":"q1","coordinate":1,"extra":true},{"id":"q2","coordinate":2,"extra":true}],
        "edges": [{"id":"f2","point":"q2","extra":true},{"id":"f1","point":"q1","extra":true}],
        "coedges": [{"id":"d2","edge":"f2","extra":true},{"id":"d1","edge":"f1","extra":true}]
    }, "native": {}});
    assert_decodes_agree(
        &before,
        &after,
        &Allowances::default(),
        Mode::Cross(CrossProjection::default()),
    );
    let mut changed = after.clone();
    changed["model"]["edges"][1]["point"] = json!("q2");
    assert!(agree(
        &before,
        &changed,
        &Allowances::default(),
        Mode::Cross(CrossProjection::default())
    )
    .is_err());
}

#[test]
fn cross_pairing_uses_unique_point_matches_to_pair_edges() {
    let before = json!({"model": {
        "points": [{"id":"p1","coordinate":1},{"id":"p2","coordinate":2},
                   {"id":"p3","coordinate":3,"extra":true}],
        "edges": [{"id":"e1","point":"p1"},{"id":"e2","point":"p2"}],
        "coedges": [{"id":"c1","edge":"e1"},{"id":"c2","edge":"e2"}]
    }, "native": {}});
    let after = json!({"model": {
        "points": [{"id":"q1","coordinate":1,"extra":true},{"id":"q2","coordinate":2,"extra":true},
                   {"id":"q3","coordinate":3,"extra":true}],
        "edges": [{"id":"f2","point":"q2"},{"id":"f1","point":"q1"}],
        "coedges": [{"id":"d2","edge":"f2"},{"id":"d1","edge":"f1"}]
    }, "native": {}});
    assert_decodes_agree(
        &before,
        &after,
        &Allowances::default(),
        Mode::Cross(CrossProjection::default()),
    );
    let mut changed = after.clone();
    changed["model"]["edges"][1]["point"] = json!("q2");
    assert!(agree(
        &before,
        &changed,
        &Allowances::default(),
        Mode::Cross(CrossProjection::default())
    )
    .is_err());
}

#[test]
fn cross_pairing_uses_matched_coedges_to_pair_ambiguous_pcurves() {
    let before = json!({"model": {
        "points": [{"id":"p1","coordinate":1},{"id":"p2","coordinate":2},
                   {"id":"p3","coordinate":3,"extra":true}],
        "edges": [{"id":"e1","point":"p1"},{"id":"e2","point":"p2"}],
        "pcurves": [{"id":"u1","parameter":1},{"id":"u2","parameter":1},
                    {"id":"u3","parameter":2,"extra":true}],
        "coedges": [{"id":"c1","edge":"e1","pcurve":"u1"},
                    {"id":"c2","edge":"e2","pcurve":"u2"}]
    }, "native": {}});
    let after = json!({"model": {
        "points": [{"id":"q1","coordinate":1,"extra":true},{"id":"q2","coordinate":2,"extra":true},
                   {"id":"q3","coordinate":3,"extra":true}],
        "edges": [{"id":"f2","point":"q2"},{"id":"f1","point":"q1"}],
        "pcurves": [{"id":"v2","parameter":1,"extra":true},{"id":"v1","parameter":1,"extra":true},
                    {"id":"v3","parameter":2,"extra":true}],
        "coedges": [{"id":"d2","edge":"f2","pcurve":"v2"},
                    {"id":"d1","edge":"f1","pcurve":"v1"}]
    }, "native": {}});
    assert_decodes_agree(
        &before,
        &after,
        &Allowances::default(),
        Mode::Cross(CrossProjection::default()),
    );
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
        absent_fields: BTreeSet::new(),
        regenerated: BTreeSet::new(),
        scope_expanded: false,
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
fn references_pair_identical_carriers_when_arena_order_changes() {
    let before = json!({"model":{"surfaces":[{"id":"a","value":1},{"id":"b","value":1}],"faces":[{"id":"f","surface":"a"}]}});
    let after = json!({"model":{"surfaces":[{"id":"y","value":1},{"id":"x","value":1}],"faces":[{"id":"g","surface":"x"}]}});
    assert!(accepts(&before, &after));
    let mut changed = after;
    changed["model"]["faces"][0]["surface"] = json!("missing");
    assert!(!accepts(&before, &changed));
}

#[test]
fn added_reference_does_not_prevent_pairing() {
    let before = json!({"model":{"surfaces":[{"id":"a","value":1},{"id":"b","value":1}],"faces":[{"id":"f","surface":"a","value":1}]}});
    let after = json!({"model":{"surfaces":[{"id":"x","value":1},{"id":"y","value":1}],"faces":[{"id":"g","surface":"x","value":1},{"id":"extra","surface":"x","value":2}]}});
    assert!(accepts(&before, &after));
}

#[test]
fn parameter_ordinals_rank_within_each_owner() {
    let before = json!({"model":{"features":[{"id":"a","kind":"one"},{"id":"b","kind":"two"}],"parameters":[{"id":"p","owner":"a","ordinal":20,"value":1},{"id":"q","owner":"b","ordinal":5,"value":2}]}});
    let after = json!({"model":{"features":[{"id":"x","kind":"one"},{"id":"y","kind":"two"}],"parameters":[{"id":"r","owner":"y","ordinal":90,"value":2},{"id":"s","owner":"x","ordinal":3,"value":1}]}});
    assert!(accepts(&before, &after));
}

#[test]
fn occurrence_ordinals_rank_within_each_parent() {
    let before = json!({"model":{"occurrences":[
        {"id":"a","parent":{"kind":"root"},"ordinal":0},
        {"id":"b","parent":{"kind":"occurrence","occurrence":"a"},"ordinal":1}]}});
    let after = json!({"model":{"occurrences":[
        {"id":"x","parent":{"kind":"root"},"ordinal":0},
        {"id":"y","parent":{"kind":"occurrence","occurrence":"x"},"ordinal":0}]}});
    assert!(accepts(&before, &after));
    let siblings = json!({"model":{"occurrences":[
        {"id":"a","parent":{"kind":"root"},"ordinal":0,"name":"first"},
        {"id":"b","parent":{"kind":"root"},"ordinal":1,"name":"second"}]}});
    let mut swapped = siblings.clone();
    swapped["model"]["occurrences"][0]["ordinal"] = json!(1);
    swapped["model"]["occurrences"][1]["ordinal"] = json!(0);
    assert!(!accepts(&siblings, &swapped));
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
        absent_fields: BTreeSet::new(),
        regenerated: BTreeSet::new(),
        scope_expanded: false,
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
fn nested_metadata_records_pair_when_their_identities_change() {
    let before = json!({"model": {}, "native": {"sldprt": {"document_metadata": [{
        "id": "document", "attributes": [{"id": "old", "name": "Part"}],
        "pmi": [{"id": "pmi-old", "attribute": "old"}]
    }]}}});
    let after = json!({"model": {}, "native": {"sldprt": {"document_metadata": [{
        "id": "document-new", "attributes": [{"id": "new", "name": "Part"}],
        "pmi": [{"id": "pmi-new", "attribute": "new"}]
    }]}}});
    assert!(accepts(&before, &after));
    let mut changed = after;
    changed["native"]["sldprt"]["document_metadata"][0]["pmi"][0]["attribute"] = json!("other");
    assert!(!accepts(&before, &changed));
}

#[test]
fn nested_sketch_carriers_pair_by_content_and_preserve_references() {
    let before = json!({"model": {"sketch_entities": [{"id": "entity", "geometry_ref": "curve", "endpoint_refs": ["point"]}]}, "native": {"sldprt": {"feature_input_lanes": [{"id": "lane", "nested_sketch_carriers": [{"parent": "lane", "points": [{"id": "point", "position": 1.0}], "curves": [{"id": "curve", "radius": 2.0}]}]}]}}});
    let after = json!({"model": {"sketch_entities": [{"id": "entity2", "geometry_ref": "curve2", "endpoint_refs": ["point2"]}]}, "native": {"sldprt": {"feature_input_lanes": [{"id": "lane2", "nested_sketch_carriers": [{"parent": "lane2", "points": [{"id": "point2", "position": 1.0}], "curves": [{"id": "curve2", "radius": 2.0}]}]}]}}});
    assert!(accepts(&before, &after));
    let mut changed = after.clone();
    changed["native"]["sldprt"]["feature_input_lanes"][0]["nested_sketch_carriers"][0]["curves"]
        [0]["radius"] = json!(3.0);
    assert!(!accepts(&before, &changed));
    let mut missing = after;
    missing["native"]["sldprt"]["feature_input_lanes"][0]["nested_sketch_carriers"][0]["points"] =
        json!([]);
    assert!(!accepts(&before, &missing));
}

#[test]
fn removing_metadata_parent_removes_its_embedded_records() {
    let before = json!({"model": {}, "native": {"sldprt": {"document_metadata": [{
        "id": "document", "attributes": [{"id": "attribute", "name": "Part"}],
        "pmi": [], "semantic_annotations": []
    }]}}});
    let after = json!({"model": {}, "native": {"sldprt": {"document_metadata": []}}});
    let allowances = Allowances::removed(BTreeSet::from(["document".into()]));
    assert!(agree(&before, &after, &allowances, Mode::Same).is_ok());
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

#[test]
fn retained_stub_reference_keeps_its_native_record_paired() {
    let document = json!({"model": {}, "native": {"sldprt": {
        "unknowns": [{"id": "stub"}],
        "pmi_dimensions": [{"id": "dimension", "parent": "stub", "value": 0.025}]
    }}});
    let allowances = Allowances::export(&document, &json!({})).expect("export allowances");
    assert!(allowances
        .removed_field_ids()
        .contains(&("dimension".into(), "parent".into())));
    assert!(
        super::unpaired_after_allowances(&document, &document, &allowances)
            .expect("compare retained records")
            .is_empty()
    );
    let changed = json!({"model": {}, "native": {"sldprt": {
        "unknowns": [{"id": "stub"}],
        "pmi_dimensions": [{"id": "dimension", "parent": "different", "value": 0.025}]
    }}});
    assert!(agree(&document, &changed, &allowances, Mode::Same).is_err());
}

#[test]
fn a_removed_stub_does_not_unpair_a_different_retained_reference() {
    let before = json!({"model": {}, "native": {"sldprt": {
        "unknowns": [{"id": "retained"}, {"id": "removed"}],
        "pmi_dimensions": [{"id": "dimension", "parent": "retained", "value": 0.025}]
    }}});
    let after = json!({"model": {}, "native": {"sldprt": {
        "unknowns": [{"id": "retained"}],
        "pmi_dimensions": [{"id": "dimension", "parent": "retained", "value": 0.025}]
    }}});
    let allowances = Allowances::export(&before, &json!({})).expect("export allowances");
    assert_eq!(
        super::unpaired_after_allowances(&before, &after, &allowances)
            .expect("compare retained records"),
        BTreeSet::from(["removed".to_owned()])
    );
}

#[test]
fn identical_twin_bodies_pair_consistently_when_their_records_interleave() {
    let before = json!({"model":{
        "bodies":[{"id":"b1","edges":["e1a","e1b"]},{"id":"b2","edges":["e2a","e2b"]}],
        "edges":[{"id":"e1a","x":0.0},{"id":"e1b","x":1.0},{"id":"e2a","x":0.0},{"id":"e2b","x":1.0}]}});
    let after = json!({"model":{
        "bodies":[{"id":"p","edges":["k1","k2"]},{"id":"q","edges":["k0","k3"]}],
        "edges":[{"id":"k0","x":0.0},{"id":"k1","x":0.0},{"id":"k2","x":1.0},{"id":"k3","x":1.0}]}});
    assert!(agree(&before, &after, &Allowances::default(), cross(&[])).is_ok());
    let mut perturbed = after.clone();
    for edge in [0, 3] {
        perturbed["model"]["edges"][edge]["x"] = json!(1.0e-13 + f64::from(u8::from(edge == 3)));
    }
    assert!(agree(&before, &perturbed, &Allowances::default(), cross(&[])).is_ok());
    let mut joined = after;
    joined["model"]["bodies"][1]["edges"] = json!(["k1", "k3"]);
    assert!(agree(&before, &joined, &Allowances::default(), cross(&[])).is_err());
}
