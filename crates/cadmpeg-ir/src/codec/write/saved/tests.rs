// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::json;

use super::{aligned, history_value, residue};
use crate::design::dimension::{ParameterRef, RefTarget};
use crate::design::expr::ExpressionToken;
use crate::design::parameter::ParameterValue;
use crate::design::units::{Quantity, ValueKind};
use crate::features::parameters::ParameterValue as HistoryValue;
use crate::features::ParameterId;
use crate::scalar::{FiniteReal, Length};

fn unmapped(
    read: &serde_json::Value,
    stored: &serde_json::Value,
    replayed: &serde_json::Value,
) -> Vec<String> {
    let mut out = Vec::new();
    residue(read, stored, replayed, "design", &mut out);
    out
}

#[test]
fn a_value_the_replay_sets_or_leaves_stale_is_mapped() {
    let stored = json!({"parameters": [{"id": "a", "value": 1, "definition": "1"}]});
    let replayed = json!({"parameters": [{"id": "a", "value": 2, "definition": "2"}]});
    let stale = json!({"parameters": [{"id": "a", "value": 1, "definition": "2"}]});
    assert!(unmapped(&replayed, &stored, &replayed).is_empty());
    assert!(unmapped(&stale, &stored, &replayed).is_empty());
}

#[test]
fn a_value_the_replay_leaves_is_an_edit() {
    let stored = json!({"parameters": [{"id": "a", "value": 1, "comment": "x"}]});
    let read = json!({"parameters": [{"id": "a", "value": 1, "comment": "y"}]});
    assert_eq!(
        unmapped(&read, &stored, &stored),
        ["design.parameters[a].comment"]
    );
}

#[test]
fn a_value_other_than_the_stored_and_the_replayed_is_an_edit() {
    let stored = json!({"parameters": [{"id": "a", "value": 1}]});
    let replayed = json!({"parameters": [{"id": "a", "value": 2}]});
    let read = json!({"parameters": [{"id": "a", "value": 3}]});
    assert_eq!(
        unmapped(&read, &stored, &replayed),
        ["design.parameters[a].value"]
    );
}

#[test]
fn records_match_by_id_and_an_added_or_removed_record_is_an_edit() {
    let stored = json!({"features": [{"id": "a"}, {"id": "b"}]});
    let reordered = json!({"features": [{"id": "b"}, {"id": "a"}]});
    assert!(unmapped(&reordered, &stored, &stored).is_empty());
    let changed = json!({"features": [{"id": "a"}, {"id": "c"}]});
    assert_eq!(
        unmapped(&changed, &stored, &stored),
        ["design.features[b]", "design.features[c]"]
    );
}

#[test]
fn a_status_is_derived() {
    let stored = json!({"evaluations": [{"id": "e", "status": "current"}]});
    let read = json!({"evaluations": [{"id": "e", "status": "stale"}]});
    assert!(unmapped(&read, &stored, &stored).is_empty());
}

fn reference(id: &str) -> ExpressionToken<ParameterRef> {
    ExpressionToken::Reference(ParameterRef {
        target: RefTarget::Parameter(ParameterId::mint(id).expect("parameter id")),
        through: None,
    })
}

fn text(part: &str) -> ExpressionToken<ParameterRef> {
    ExpressionToken::Text(part.to_owned())
}

#[test]
fn the_text_tokens_split_the_stored_text_around_each_reference() {
    let tokens = [
        reference("test:design:parameter#a"),
        text(" * 2 + "),
        reference("test:design:parameter#b"),
    ];
    let spelled = aligned(&tokens, "\"D1@Boss\" * 2 + wall").expect("aligned");
    let raws = spelled.iter().map(|(_, raw)| *raw).collect::<Vec<_>>();
    assert_eq!(raws, ["\"D1@Boss\"", "wall"]);
    assert!(aligned(&tokens, "\"D1@Boss\" / 2 + wall").is_none());
    assert!(aligned(
        &[
            reference("test:design:parameter#a"),
            reference("test:design:parameter#b")
        ],
        "ab"
    )
    .is_none());
}

#[test]
fn a_cache_takes_the_variant_the_history_stored() {
    let number = ParameterValue::Number(FiniteReal::new(20.0).expect("finite"));
    let length = HistoryValue::Length(Length::new(12.0).expect("length"));
    assert_eq!(
        history_value(
            Some(&number),
            Some(&length),
            &ValueKind::Quantity(Quantity::REAL)
        ),
        Some(HistoryValue::Length(Length::new(20.0).expect("length")))
    );
    assert_eq!(
        history_value(Some(&number), None, &ValueKind::Quantity(Quantity::LENGTH)),
        Some(HistoryValue::Length(Length::new(20.0).expect("length")))
    );
    assert_eq!(
        history_value(Some(&number), None, &ValueKind::Quantity(Quantity::REAL)),
        Some(HistoryValue::Real(FiniteReal::new(20.0).expect("finite")))
    );
    assert_eq!(
        history_value(None, Some(&length), &ValueKind::Integer),
        None
    );
}
