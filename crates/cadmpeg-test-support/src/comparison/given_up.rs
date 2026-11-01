// SPDX-License-Identifier: Apache-2.0
//! §4.5 given-up allowances, built from the `record.unreadable` losses.
//!
//! A loss alone never licenses a removal. This constructor collects the
//! structured span of every `record.unreadable` loss and refuses a loss with
//! no structured span. No structured field of the loss names a dropped record
//! today — the message carries the part kind, stream, offset, length and
//! error text only — so the allowances stay empty until #215 reports the
//! dropped records in structured form. Until then a removal excused only by
//! a loss fails the comparison, exactly as §5.3 requires.

use std::collections::BTreeSet;

use serde_json::Value;

use super::{graph::Document, Allowances};

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
    /// a report without a losses array, and a `record.unreadable` loss with
    /// no structured source span.
    pub fn given_up(before: &Value, decode_report: &Value) -> Result<Self, String> {
        Document::read(before, &BTreeSet::new())?;
        let Some(losses) = decode_report.get("losses").and_then(Value::as_array) else {
            return Err("the decode report has no losses array".into());
        };
        for loss in losses {
            if loss.get("code").and_then(Value::as_str) != Some(UNREADABLE) {
                continue;
            }
            let span = loss.get("provenance");
            if span.is_none_or(|span| span.get("offset").and_then(Value::as_u64).is_none()) {
                return Err("a record.unreadable loss has no structured source span".into());
            }
        }
        Ok(Self::default())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::Allowances;

    fn before() -> serde_json::Value {
        json!({"model": {"points": [{"id": "p", "value": 1}]}, "native": {}})
    }

    fn unreadable(provenance: serde_json::Value) -> serde_json::Value {
        let mut loss = serde_json::Map::new();
        loss.insert("code".to_owned(), json!("decode/record.unreadable"));
        loss.insert("severity".to_owned(), json!("error"));
        loss.insert(
            "message".to_owned(),
            json!("part step.simple_entity_instance stream=\"\" offset=3 length=14: test"),
        );
        loss.insert("provenance".to_owned(), provenance);
        serde_json::Value::Object(loss)
    }

    #[test]
    fn losses_with_structured_spans_build_empty_allowances() {
        let report = json!({"losses": [
            unreadable(json!({"format": "step", "offset": 3, "byte_len": 14})),
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
    fn an_unreadable_loss_without_a_span_is_refused() {
        let report = json!({"losses": [unreadable(json!(null))]});
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
