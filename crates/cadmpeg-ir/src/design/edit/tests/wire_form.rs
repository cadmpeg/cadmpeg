// SPDX-License-Identifier: Apache-2.0
//! Positions, transformations and constructions rewrite in the wire form.

use super::super::wire::{clear_transformations, exchange_constructions, re_anchor};
use super::*;

#[test]
fn positions_and_constructions_rewrite_in_the_wire_form() {
    let from = positions(&[after(2)]).unwrap();
    let to = serde_json::to_value(after(1)).unwrap();
    let mut value = json!({"at": {"kind": "pinned", "value": {"kind": "after", "value": {"kind": "feature", "value": feature(2).as_str()}}}, "picks": {"kind": "unread"}});
    let mut anchored = value.clone();
    assert!(retarget(&mut value, &from, &to));
    assert_eq!(
        value["at"]["value"]["value"]["value"],
        json!(feature(1).as_str())
    );
    assert!(re_anchor(&mut anchored, &from));
    assert_eq!(anchored["at"], json!({"kind": "current"}));

    let mut moved = json!({"steps": [{"kind": "translate", "value": {"kind": "parameter", "value": parameter_id(1).as_str()}}], "resolved": [1.0]});
    let mut unbound = moved.clone();
    unbound["steps"][0]["value"]["value"] = json!(parameter_id(2).as_str());
    let bound = BTreeSet::from([parameter_id(1).as_str().to_owned()]);
    assert!(clear_transformations(&mut moved, &bound));
    assert!(moved.get("resolved").is_none());
    assert!(!clear_transformations(&mut unbound, &bound));

    let mut construction = json!({
        "inputs": [{"kind": "x"}], "after": [],
        "resolved": [1.0], "resolved_in": {"c2": [2.0]},
        "nested": {"inputs": [], "after": [], "resolved": [3.0], "resolved_in": {}},
    });
    assert!(exchange_constructions(&mut construction, "c1", "c2"));
    assert_eq!(construction["resolved"], json!([2.0]));
    assert_eq!(construction["resolved_in"], json!({"c1": [1.0]}));
    assert_eq!(
        construction["nested"]["resolved"],
        json!([3.0]),
        "a construction with no inputs or steps keeps its value"
    );
}
