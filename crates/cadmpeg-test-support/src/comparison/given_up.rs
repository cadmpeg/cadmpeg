// SPDX-License-Identifier: Apache-2.0
//! §4.5 given-up allowances, built from the `record.unreadable` losses.
//!
//! A loss alone never licenses a removal. Each `record.unreadable` loss names
//! the records its part dropped in its structured `dropped` field, and the
//! allowances close that set under rule 2: a record that needs a dropped
//! record is dropped too, transitively, while a record that lists one loses
//! only that entry (the comparison prunes list entries itself). A loss with
//! no `dropped` field contributes nothing, and a removal no loss names fails
//! the comparison, exactly as §5.3 requires.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde_json::Value;

use super::{export::required_references, graph::Document, Allowances};

/// The loss code the shared part helper charges for one given-up part, in
/// display form. On the wire a loss carries its code as an object whose
/// `code` field holds the local code; some callers pin the display form.
const UNREADABLE: &str = "decode/record.unreadable";

/// The wire form of the part give-up code.
const UNREADABLE_WIRE: &str = "record.unreadable";

/// Whether a loss JSON charges the one part give-up the helper reports.
fn is_unreadable(loss: &Value) -> bool {
    match loss.get("code") {
        Some(Value::String(code)) => code == UNREADABLE,
        Some(Value::Object(code)) => {
            code.get("code").and_then(Value::as_str) == Some(UNREADABLE_WIRE)
        }
        _ => false,
    }
}

impl Allowances {
    /// Build the §4.5 allowances from a damaged decode's losses.
    ///
    /// `before` is the document whose records may have been removed; the
    /// `decode_report` is the `DecodeReport` JSON of the damaged decode.
    ///
    /// # Errors
    ///
    /// Refuses a document without an object-valued model or identified rows,
    /// a report without a losses array, a `record.unreadable` loss with no
    /// structured source span, and a `dropped` field that is not an array of
    /// strings.
    pub fn given_up(before: &Value, decode_report: &Value) -> Result<Self, String> {
        let records = Document::read(before, &BTreeSet::new())?;
        let Some(losses) = decode_report.get("losses").and_then(Value::as_array) else {
            return Err("the decode report has no losses array".into());
        };
        let mut dropped = BTreeSet::new();
        let mut absent = BTreeSet::new();
        for loss in losses {
            if !is_unreadable(loss) {
                continue;
            }
            let span = loss.get("provenance");
            if span.is_none_or(|span| span.get("offset").and_then(Value::as_u64).is_none()) {
                return Err("a record.unreadable loss has no structured source span".into());
            }
            if let Some(entries) = loss.get("dropped") {
                let Some(entries) = entries.as_array() else {
                    return Err(
                        "a record.unreadable loss names its dropped records outside an array".into(),
                    );
                };
                for entry in entries {
                    let Some(id) = entry.as_str() else {
                        return Err(
                            "a record.unreadable loss names a non-string dropped record".into(),
                        );
                    };
                    dropped.insert(id.to_owned());
                }
            }
            let Some(fields) = loss.get("dropped_fields") else {
                continue;
            };
            let Some(fields) = fields.as_array() else {
                return Err(
                    "a record.unreadable loss names its absent fields outside an array".into(),
                );
            };
            for entry in fields {
                let record = entry.get("record").and_then(Value::as_str);
                let field = entry.get("field").and_then(Value::as_str);
                match (record, field) {
                    (Some(record), Some(field)) => {
                        absent.insert((record.to_owned(), field.to_owned()));
                    }
                    _ => {
                        return Err(
                            "a record.unreadable loss names an absent field without a record and a field"
                                .into(),
                        );
                    }
                }
            }
        }
        let mut users = BTreeMap::<String, BTreeSet<String>>::new();
        for record in &records.records {
            let mut required = BTreeSet::new();
            required_references(&record.value, &records.ids, &mut required);
            for target in required {
                users.entry(target).or_default().insert(record.id.clone());
            }
        }
        let mut removed = dropped;
        let mut queue = removed.iter().cloned().collect::<VecDeque<_>>();
        while let Some(target) = queue.pop_front() {
            for owner in users.get(&target).into_iter().flatten() {
                if removed.insert(owner.clone()) {
                    queue.push_back(owner.clone());
                }
            }
        }
        Ok(Self {
            removed,
            removed_fields: BTreeSet::new(),
            absent_fields: absent,
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::Allowances;

    fn before() -> serde_json::Value {
        json!({"model": {"points": [{"id": "p", "value": 1}]}, "native": {}})
    }

    fn unreadable(provenance: serde_json::Value, dropped: serde_json::Value) -> serde_json::Value {
        let mut loss = serde_json::Map::new();
        loss.insert("code".to_owned(), json!("decode/record.unreadable"));
        loss.insert("severity".to_owned(), json!("error"));
        loss.insert(
            "message".to_owned(),
            json!("part step.simple_entity_instance stream=\"\" offset=3 length=14: test"),
        );
        loss.insert("provenance".to_owned(), provenance);
        loss.insert("dropped".to_owned(), dropped);
        serde_json::Value::Object(loss)
    }

    fn spanned() -> serde_json::Value {
        json!({"format": "step", "offset": 3, "byte_len": 14})
    }

    fn wired(provenance: serde_json::Value, dropped: serde_json::Value) -> serde_json::Value {
        let mut code = serde_json::Map::new();
        code.insert("code".to_owned(), json!("record.unreadable"));
        code.insert("kind".to_owned(), json!("decode_diagnostic"));
        code.insert("namespace".to_owned(), json!("decode"));
        code.insert("scope".to_owned(), json!("namespaced"));
        code.insert("severity".to_owned(), json!("error"));
        let mut loss = serde_json::Map::new();
        loss.insert("code".to_owned(), serde_json::Value::Object(code));
        loss.insert("severity".to_owned(), json!("error"));
        loss.insert(
            "message".to_owned(),
            json!("part step.simple_entity_instance stream=\"\" offset=3 length=14: test"),
        );
        loss.insert("provenance".to_owned(), provenance);
        if !dropped.is_null() {
            loss.insert("dropped".to_owned(), dropped);
        }
        serde_json::Value::Object(loss)
    }

    #[test]
    fn losses_without_dropped_ids_build_empty_allowances() {
        let report = json!({"losses": [
            unreadable(spanned(), json!([])),
        ]});
        let allowances = Allowances::given_up(&before(), &report).expect("structured losses");
        assert!(super::super::agree(
            &before(),
            &json!({"model": {"points": []}, "native": {}}),
            &allowances,
            super::super::Mode::Same,
        )
        .is_err());
    }

    #[test]
    fn a_dropped_record_may_go() {
        let report = json!({"losses": [unreadable(spanned(), json!(["p"]))]});
        let allowances = Allowances::given_up(&before(), &report).expect("named drop");
        super::super::assert_decodes_agree(
            &before(),
            &json!({"model": {"points": []}, "native": {}}),
            &allowances,
            super::super::Mode::Same,
        );
    }

    #[test]
    fn a_record_needing_a_dropped_record_goes_with_it() {
        let before = json!({"model": {"points": [
            {"id": "p", "value": 1},
            {"id": "q", "at": "p"},
            {"id": "r", "carrier": {"part": "q"}},
        ]}});
        let after = json!({"model": {"points": []}});
        let report = json!({"losses": [unreadable(spanned(), json!(["p"]))]});
        let allowances = Allowances::given_up(&before, &report).expect("named drop");
        assert!(allowances.removed.contains("q"));
        assert!(allowances.removed.contains("r"));
        super::super::assert_decodes_agree(&before, &after, &allowances, super::super::Mode::Same);
    }

    #[test]
    fn a_record_listing_a_dropped_record_loses_only_that_entry() {
        let before = json!({"model": {
            "faces": [{"id": "f", "value": 1}],
            "shells": [{"id": "s", "members": ["f"]}],
        }});
        let after = json!({"model": {
            "faces": [],
            "shells": [{"id": "t", "members": []}],
        }});
        let report = json!({"losses": [unreadable(spanned(), json!(["f"]))]});
        let allowances = Allowances::given_up(&before, &report).expect("named drop");
        assert!(!allowances.removed.contains("s"));
        super::super::assert_decodes_agree(&before, &after, &allowances, super::super::Mode::Same);
    }

    #[test]
    fn a_vanished_record_no_loss_names_fails() {
        let report = json!({"losses": [unreadable(spanned(), json!(["elsewhere"]))]});
        let allowances = Allowances::given_up(&before(), &report).expect("structured losses");
        assert!(super::super::agree(
            &before(),
            &json!({"model": {"points": []}, "native": {}}),
            &allowances,
            super::super::Mode::Same,
        )
        .is_err());
    }

    #[test]
    fn an_unreadable_loss_without_a_span_is_refused() {
        let report = json!({"losses": [unreadable(json!(null), json!([]))]});
        assert!(Allowances::given_up(&before(), &report).is_err());
    }

    #[test]
    fn a_dropped_field_outside_an_array_is_refused() {
        let report = json!({"losses": [unreadable(spanned(), json!("p"))]});
        assert!(Allowances::given_up(&before(), &report).is_err());
    }

    #[test]
    fn a_report_without_a_losses_array_is_refused() {
        assert!(Allowances::given_up(&before(), &json!({})).is_err());
    }

    #[test]
    fn other_loss_codes_contribute_nothing() {
        let report = json!({"losses": [
            {"code": "decode/depth.not-delivered", "severity": "error", "message": "test"},
        ]});
        assert!(Allowances::given_up(&before(), &report).is_ok());
    }

    #[test]
    fn a_wire_shaped_loss_names_its_dropped_records() {
        let report = json!({"losses": [wired(spanned(), json!(["p"]))]});
        let allowances = Allowances::given_up(&before(), &report).expect("named drop");
        assert!(allowances.removed.contains("p"));
        super::super::assert_decodes_agree(
            &before(),
            &json!({"model": {"points": []}, "native": {}}),
            &allowances,
            super::super::Mode::Same,
        );
    }

    #[test]
    fn a_wire_shaped_loss_without_dropped_ids_builds_empty_allowances() {
        let report = json!({"losses": [wired(spanned(), serde_json::Value::Null)]});
        let allowances = Allowances::given_up(&before(), &report).expect("structured losses");
        assert!(allowances.removed.is_empty());
    }

    #[test]
    fn a_wire_shaped_loss_of_another_code_contributes_nothing() {
        let mut loss = wired(spanned(), serde_json::Value::Null);
        loss["code"]["code"] = json!("parse.record-omitted");
        let report = json!({"losses": [loss]});
        let allowances = Allowances::given_up(&before(), &report).expect("other code");
        assert!(allowances.removed.is_empty());
    }

    fn fielded(provenance: serde_json::Value, fields: serde_json::Value) -> serde_json::Value {
        let mut loss = unreadable(provenance, json!([]));
        loss["dropped_fields"] = fields;
        loss
    }

    fn fielded_report(fields: serde_json::Value) -> serde_json::Value {
        json!({"losses": [fielded(spanned(), fields)]})
    }

    fn fielded_before() -> serde_json::Value {
        json!({"model": {"points": [
            {"id": "p", "value": 1, "kept": 2},
        ]}, "native": {}})
    }

    #[test]
    fn a_named_field_may_go_missing() {
        let report = fielded_report(json!([{"record": "p", "field": "value"}]));
        let allowances = Allowances::given_up(&fielded_before(), &report).expect("named field");
        assert!(allowances.absent_field_ids().contains(&("p".to_owned(), "value".to_owned())));
        super::super::assert_decodes_agree(
            &fielded_before(),
            &json!({"model": {"points": [{"id": "q", "kept": 2}]}, "native": {}}),
            &allowances,
            super::super::Mode::Same,
        );
    }

    #[test]
    fn a_named_field_may_be_null_or_empty() {
        let report = fielded_report(json!([
            {"record": "p", "field": "value"},
            {"record": "q", "field": "tags"},
        ]));
        let before = json!({"model": {"points": [
            {"id": "p", "value": 1},
            {"id": "q", "tags": ["a"]},
        ]}, "native": {}});
        let allowances = Allowances::given_up(&before, &report).expect("named fields");
        super::super::assert_decodes_agree(
            &before,
            &json!({"model": {"points": [
                {"id": "p", "value": serde_json::Value::Null},
                {"id": "q", "tags": []},
            ]}, "native": {}}),
            &allowances,
            super::super::Mode::Same,
        );
    }

    #[test]
    fn a_named_field_holding_a_partial_value_still_fails() {
        let report = fielded_report(json!([{"record": "p", "field": "value"}]));
        let allowances = Allowances::given_up(&fielded_before(), &report).expect("named field");
        assert!(super::super::agree(
            &fielded_before(),
            &json!({"model": {"points": [{"id": "p", "value": 3, "kept": 2}]}, "native": {}}),
            &allowances,
            super::super::Mode::Same,
        )
        .is_err());
    }

    #[test]
    fn an_unnamed_field_must_agree() {
        let report = fielded_report(json!([{"record": "p", "field": "value"}]));
        let allowances = Allowances::given_up(&fielded_before(), &report).expect("named field");
        assert!(super::super::agree(
            &fielded_before(),
            &json!({"model": {"points": [{"id": "q", "kept": 4}]}, "native": {}}),
            &allowances,
            super::super::Mode::Same,
        )
        .is_err());
    }

    #[test]
    fn absent_fields_outside_an_array_are_refused() {
        let report = fielded_report(json!("p.value"));
        assert!(Allowances::given_up(&before(), &report).is_err());
    }

    #[test]
    fn an_absent_field_without_a_record_and_a_field_is_refused() {
        let report = fielded_report(json!([{"record": "p"}]));
        assert!(Allowances::given_up(&before(), &report).is_err());
        let report = fielded_report(json!([{"field": "value"}]));
        assert!(Allowances::given_up(&before(), &report).is_err());
        let report = fielded_report(json!(["p"]));
        assert!(Allowances::given_up(&before(), &report).is_err());
    }
}
