// SPDX-License-Identifier: Apache-2.0
use serde_json::json;

use super::{dangling_token_findings, scope_findings, unrepresented_findings};

#[test]
fn scope_must_hold_populated_and_requested_classes() {
    let report = json!({"transfer": {"scope": {"model": true, "design": false}}, "losses": []});
    let geometry =
        json!({"scope": {"model": true, "design": false}, "model": {"faces": [{"id": "x"}]}});
    assert!(scope_findings(&geometry, &report, &["model"]).is_empty());
    assert_eq!(
        scope_findings(&geometry, &report, &["model", "design"]).len(),
        1
    );
    let lying =
        json!({"scope": {"model": true, "design": false}, "model": {"features": [{"id": "f"}]}});
    assert_eq!(scope_findings(&lying, &report, &["model"]).len(), 1);
    let excused = json!({"transfer": {"scope": {"model": true, "design": false}},
                         "losses": [{"code": {"code": "depth.not-delivered"}}]});
    assert!(scope_findings(&geometry, &excused, &["model", "design"]).is_empty());
    let other = json!({"transfer": {"scope": {"model": true, "design": true}}, "losses": []});
    assert_eq!(scope_findings(&geometry, &other, &["model"]).len(), 1);
}

#[test]
fn a_token_must_name_a_parameter() {
    let document = json!({"model": {"parameters": [
        {"id": "p", "expression": {"tokens": [{"kind": "parameter", "value": "q"}]}}
    ]}});
    assert_eq!(dangling_token_findings(&document).len(), 1);
}

#[test]
fn unrepresented_must_equal_the_retained_untyped_union() {
    let matching = json!({
        "unrepresented": ["model"],
        "retained_records": {"r": {"untyped": ["model"]}, "s": {"untyped": []}},
    });
    assert!(unrepresented_findings(&matching).is_empty());
    let missing = json!({
        "unrepresented": [],
        "retained_records": {"r": {"untyped": ["design"]}},
    });
    assert_eq!(unrepresented_findings(&missing).len(), 1);
    let extra = json!({
        "unrepresented": ["model", "design"],
        "retained_records": {"r": {"untyped": ["model"]}},
    });
    assert_eq!(unrepresented_findings(&extra).len(), 1);
    assert!(unrepresented_findings(&json!(null)).is_empty());
}
