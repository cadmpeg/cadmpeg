// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeSet;

use serde_json::json;

use super::{
    equation_identities, expression_text_findings, feature_ordinal_findings,
    freecad_stale_findings, never_stale_findings, solidworks_stale_findings,
};

#[test]
fn dense_ordinals_pass_in_any_record_order() {
    let document = json!({"model": {"features": [
        {"id": "b", "ordinal": 1}, {"id": "a", "ordinal": 0}, {"id": "c", "ordinal": 2}
    ]}});
    assert!(feature_ordinal_findings(&document).is_empty());
}

#[test]
fn a_gap_or_a_repeat_in_ordinals_fails() {
    let gap =
        json!({"model": {"features": [{"id": "a", "ordinal": 0}, {"id": "b", "ordinal": 2}]}});
    assert_eq!(feature_ordinal_findings(&gap).len(), 1);
    let repeat =
        json!({"model": {"features": [{"id": "a", "ordinal": 0}, {"id": "b", "ordinal": 0}]}});
    assert_eq!(feature_ordinal_findings(&repeat).len(), 1);
}

#[test]
fn a_decoder_without_a_staleness_flag_writes_false() {
    let fresh = json!({"model": {"features": [{"id": "a", "needs_recompute": false}]}});
    assert!(never_stale_findings(&fresh).is_empty());
    let stale = json!({"model": {"features": [{"id": "a", "needs_recompute": true}]}});
    assert_eq!(never_stale_findings(&stale).len(), 1);
}

#[test]
fn a_text_token_that_spells_a_parameter_fails() {
    let document = json!({"model": {
        "features": [{"id": "f", "name": "Sketch1"}],
        "parameters": [
            {"id": "p", "name": "D1", "owner": "f", "expression": {"tokens": [{"kind": "text", "value": "2 mm"}]}},
            {"id": "q", "name": "D2", "expression": {"tokens": [
                {"kind": "text", "value": " D1@Sketch1 "}, {"kind": "parameter", "value": "p"}
            ]}},
        ],
    }});
    let findings = expression_text_findings(&document, &BTreeSet::new());
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("parameters/1/expression"));
}

#[test]
fn a_native_equation_identity_in_text_fails() {
    let document = json!({"model": {"parameters": [
        {"id": "p", "name": "D1", "expression": {"tokens": [{"kind": "text", "value": "EQ7"}]}}
    ]}});
    assert!(expression_text_findings(&document, &BTreeSet::new()).is_empty());
    let ids = BTreeSet::from(["EQ7".to_owned()]);
    assert_eq!(expression_text_findings(&document, &ids).len(), 1);
}

#[test]
fn equation_identities_come_from_properties_and_native_fields() {
    let document = json!({
        "model": {"parameters": [{"id": "p", "properties": {"EquationId": "EQ1"}}]},
        "native": {"sldprt": {"rows": [{"id": "n", "equation_id": "EQ2"}]}},
    });
    let found: Vec<String> = equation_identities(&document).into_iter().collect();
    assert_eq!(found, ["EQ1", "EQ2"]);
}

#[test]
fn a_freecad_feature_is_stale_exactly_when_its_object_is_touched() {
    let document = json!({
        "model": {"features": [
            {"id": "a", "native_ref": "o1", "needs_recompute": true},
            {"id": "b", "native_ref": "o2", "needs_recompute": true},
            {"id": "c", "native_ref": "o3", "needs_recompute": false},
        ]},
        "native": {"fcstd": {"objects": [
            {"id": "o1", "attributes": {"Touched": "1"}},
            {"id": "o2", "attributes": {}},
            {"id": "o3", "attributes": {"Touched": "1"}},
        ]}},
    });
    assert_eq!(freecad_stale_findings(&document).len(), 2);
}

#[test]
fn solidworks_staleness_follows_the_active_configuration() {
    let stale = json!({
        "source": {"attributes": {"sw_configuration_0_needs_update": "YES"}},
        "model": {"configurations": [{"id": "k", "active": true, "source_index": 0}],
                  "features": [{"id": "f", "needs_recompute": true}]},
    });
    assert!(solidworks_stale_findings(&stale).is_empty());
    let fresh = json!({
        "source": {"attributes": {"sw_configuration_0_needs_update": "NO"}},
        "model": {"configurations": [{"id": "k", "active": true, "source_index": 0}],
                  "features": [{"id": "f", "needs_recompute": true}]},
    });
    assert_eq!(solidworks_stale_findings(&fresh).len(), 1);
}
