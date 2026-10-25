// SPDX-License-Identifier: Apache-2.0
use serde_json::json;

use super::{named_home, native_kind_justified, native_kinds, p_closure_findings, Link};

fn layout(start: u64, end: u64) -> serde_json::Value {
    json!({"stream": "s", "offset": 0, "byte_len": 99, "pointers": {}, "payloads": {},
           "reads": {"s": [{"start": start, "end": end}]}})
}

#[test]
fn a_named_record_linked_by_its_home_passes() {
    let document = json!({
        "model": {"attributes": [{"id": "a", "native_ref": "u"}]},
        "native": {"rhino": {"user_strings": [{"id": "u"}]}},
    });
    assert!(p_closure_findings(&document, &json!({})).is_empty());
}

#[test]
fn a_named_record_linked_only_from_another_arena_fails() {
    let document = json!({
        "model": {"features": [{"id": "f", "native_ref": "u"}]},
        "native": {"rhino": {"user_strings": [{"id": "u"}]}},
    });
    assert_eq!(p_closure_findings(&document, &json!({})).len(), 1);
}

#[test]
fn a_consumed_record_links_through_its_owner_and_reads() {
    let document = json!({
        "model": {"features": [{"id": "f", "native_ref": "o"}]},
        "native": {"inventor": {"pm_dc_features": [], "pm_dc_units": [{"id": "x", "owner": "o"}]}},
    });
    let overlapping = json!({"record_layout": {"f": layout(0, 10), "x": layout(5, 6)}});
    assert!(p_closure_findings(&document, &overlapping).is_empty());
    let disjoint = json!({"record_layout": {"f": layout(0, 4), "x": layout(5, 6)}});
    assert_eq!(p_closure_findings(&document, &disjoint).len(), 1);
}

#[test]
fn a_record_without_reads_is_b1() {
    let document = json!({"native": {"rhino": {"user_strings": [{"id": "u"}]}}});
    let fidelity = json!({"record_layout": {"u": {"stream": "s", "offset": 0, "byte_len": 4,
                          "reads": {}, "pointers": {}, "payloads": {}}}});
    assert!(p_closure_findings(&document, &fidelity).is_empty());
}

#[test]
fn one_neutral_record_never_links_two_owned_records() {
    let document = json!({
        "model": {"attributes": [{"id": "a", "native_ref": "o"}]},
        "native": {"rhino": {"user_strings": [{"id": "u1", "owner": "o"}, {"id": "u2", "owner": "o"}]}},
    });
    let fidelity =
        json!({"record_layout": {"a": layout(0, 10), "u1": layout(1, 2), "u2": layout(3, 4)}});
    assert_eq!(p_closure_findings(&document, &fidelity).len(), 1);
}

#[test]
fn prefixed_named_arenas_match_by_prefix() {
    assert_eq!(
        named_home("nx", "material_texture_slots").map(|row| row.link),
        Some(Link::Own)
    );
    assert!(named_home("nx", "materials").is_none());
}

#[test]
fn native_kinds_name_their_arena_and_kind() {
    let document = json!({"model": {
        "features": [{"id": "f", "definition": {"definition": "native", "kind": "Fem::FemAnalysis"}}],
        "sketch_constraints": [{"id": "c", "definition": {"kind": "native", "native_kind": "Linear"}}],
    }});
    let kinds: Vec<(String, String)> = native_kinds(&document).into_iter().collect();
    assert_eq!(
        kinds,
        [
            ("features".to_owned(), "Fem::FemAnalysis".to_owned()),
            ("sketch_constraints".to_owned(), "Linear".to_owned()),
        ]
    );
}

#[test]
fn only_the_named_native_homes_are_justified() {
    assert!(native_kind_justified(
        "fcstd",
        ("features", "Fem::FemAnalysis")
    ));
    assert!(!native_kind_justified("fcstd", ("features", "App::Link")));
    assert!(native_kind_justified(
        "f3d",
        ("features", "sheet_metal_settings")
    ));
    assert!(!native_kind_justified("f3d", ("features", "Form")));
    assert!(!native_kind_justified(
        "creo",
        ("features", "CurveFromEquation")
    ));
}
