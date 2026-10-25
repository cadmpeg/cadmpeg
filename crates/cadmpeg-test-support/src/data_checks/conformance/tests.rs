// SPDX-License-Identifier: Apache-2.0
use serde_json::json;

use super::{b1_spans, b1_unrepresented, dangling_token_findings, scope_findings};

fn part(offset: u64, byte_len: u64, framing: serde_json::Value, state: &str) -> serde_json::Value {
    json!({"stream": "s", "offset": offset, "byte_len": byte_len, "framing_bytes": framing,
           "level": "x", "parent": null, "state": {"state": state}})
}

fn layout(offset: u64, byte_len: u64, reads: serde_json::Value) -> serde_json::Value {
    json!({"stream": "s", "offset": offset, "byte_len": byte_len, "reads": {"s": reads},
           "pointers": {}, "payloads": {}})
}

#[test]
fn unread_unframed_part_bytes_are_b1() {
    let fidelity = json!({
        "parts": {"p": part(0, 10, json!([{"start": 0, "end": 2}]), "decoded")},
        "record_layout": {"r": layout(0, 10, json!([{"start": 2, "end": 6}]))},
    });
    assert_eq!(b1_spans(&fidelity)["s"], vec![6..10]);
}

#[test]
fn a_given_up_part_is_b1_outside_its_framing_even_where_read() {
    let fidelity = json!({
        "parts": {"p": part(0, 10, json!([{"start": 0, "end": 1}]), "given_up")},
        "record_layout": {"r": layout(0, 10, json!([{"start": 0, "end": 10}]))},
    });
    assert_eq!(b1_spans(&fidelity)["s"], vec![1..10]);
}

#[test]
fn span_classes_come_from_the_neutral_records_that_link_the_owner() {
    let document = json!({"model": {"features": [{"id": "f", "native_ref": "r"}]}});
    let fidelity = json!({
        "parts": {"p": part(0, 10, json!([]), "decoded")},
        "record_layout": {"r": layout(0, 10, json!([{"start": 0, "end": 8}]))},
    });
    assert_eq!(
        b1_unrepresented(&document, &fidelity)
            .into_iter()
            .collect::<Vec<_>>(),
        ["design"]
    );
    let unowned = json!({"parts": {"p": part(0, 10, json!([]), "decoded")}});
    assert_eq!(b1_unrepresented(&document, &unowned).len(), 2);
    let read = json!({
        "parts": {"p": part(0, 10, json!([]), "decoded")},
        "record_layout": {"r": layout(0, 10, json!([{"start": 0, "end": 10}]))},
    });
    assert!(b1_unrepresented(&document, &read).is_empty());
}

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
