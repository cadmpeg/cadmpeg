// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeSet;

use serde_json::{json, Value};

use super::super::{decodes_agree, Allowances, Mode};
use super::{stub_extents, unnamed_removals};
use cadmpeg_ir::codec::write::ExportLossCode;

fn before() -> Value {
    json!({
        "model": {"features": [
            {"id": "x:m:pad#1", "name": "Pad", "length": "x:source:part#1"},
            {"id": "x:m:pocket#2", "base": "x:n:sketch#3"},
            {"id": "x:m:group#4", "members": ["x:n:sketch#3", "x:m:pad#1"]},
        ]},
        "native": {"x": {
            "unknowns": [{"id": "x:source:part#1"}],
            "sketches": [{"id": "x:n:sketch#3", "carrier": {"part": "x:source:part#1"}}],
        }},
    })
}

#[test]
fn b1_stubs_their_only_field_and_their_dependants_may_go() {
    let allowances = Allowances::export(&before()).unwrap();
    let after = json!({
        "model": {"features": [
            {"id": "y:m:pad#1", "name": "Pad"},
            {"id": "y:m:group#4", "members": ["y:m:pad#1"]},
        ]},
        "native": {"x": {"unknowns": [], "sketches": []}},
    });
    let missing = decodes_agree(&before(), &after, &allowances, Mode::Same).unwrap();
    let expected: BTreeSet<String> = ["x:m:pocket#2", "x:n:sketch#3", "x:source:part#1"]
        .map(str::to_owned)
        .into();
    assert_eq!(missing, expected);
}

#[test]
fn a_record_with_a_stub_field_keeps_its_other_fields() {
    let allowances = Allowances::export(&before()).unwrap();
    let after = json!({
        "model": {"features": [
            {"id": "y:m:pad#1"},
            {"id": "y:m:group#4", "members": ["y:m:pad#1"]},
        ]},
        "native": {"x": {"unknowns": [], "sketches": []}},
    });
    assert!(decodes_agree(&before(), &after, &allowances, Mode::Same).is_err());
}

#[test]
fn default_allowances_keep_every_record() {
    let after =
        json!({"model": {"features": []}, "native": {"x": {"unknowns": [], "sketches": []}}});
    assert!(decodes_agree(&before(), &after, &Allowances::default(), Mode::Same).is_err());
}

#[test]
fn every_removal_must_be_named_by_its_loss() {
    let fidelity = json!({"retained_records": {"x:source:part#1": {
        "stream": "file", "offset": 40, "bytes": {"retention": "inline", "data": "MTIz"}}}});
    let stubs = stub_extents(&before(), &fidelity);
    assert_eq!(stubs["x:source:part#1"], ("file".to_owned(), 40, 3));
    let removed: BTreeSet<String> = ["x:source:part#1", "x:n:sketch#3"]
        .map(str::to_owned)
        .into();
    assert_eq!(unnamed_removals(&removed, &stubs, &[]).len(), 2);
    let named = [
        ExportLossCode::SourceUndecodedOmitted.note("stream file offset 40 length 3"),
        ExportLossCode::NativeRecordsOmitted.note("1 native record(s): x:n:sketch#3"),
    ];
    assert!(unnamed_removals(&removed, &stubs, &named).is_empty());
}

#[test]
fn an_allowed_record_the_output_keeps_still_pairs_with_its_list_entries() {
    let allowances = Allowances::export(&before()).unwrap();
    let missing = decodes_agree(&before(), &before(), &allowances, Mode::Same).unwrap();
    assert!(missing.is_empty(), "{missing:?}");
}
