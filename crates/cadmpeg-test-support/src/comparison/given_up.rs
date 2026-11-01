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

/// The loss code the shared part helper charges for one given-up part.
const UNREADABLE: &str = "decode/record.unreadable";

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
        for loss in losses {
            if loss.get("code").and_then(Value::as_str) != Some(UNREADABLE) {
                continue;
            }
            let span = loss.get("provenance");
            if span.is_none_or(|span| span.get("offset").and_then(Value::as_u64).is_none()) {
                return Err("a record.unreadable loss has no structured source span".into());
            }
            let Some(entries) = loss.get("dropped") else {
                continue;
            };
            let Some(entries) = entries.as_array() else {
                return Err("a record.unreadable loss names its dropped records outside an array"
                    .into());
            };
            for entry in entries {
                let Some(id) = entry.as_str() else {
                    return Err(
                        "a record.unreadable loss names a non-string dropped record".into()
                    );
                };
                dropped.insert(id.to_owned());
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
}
