// SPDX-License-Identifier: Apache-2.0
//! Residual-position harness (§3.3).
//!
//! A writer that synthesizes a record writes each unread position with the
//! value the format specification states. Where the specification states no
//! value, it charges `export:record.residual-unavailable` naming the record.
//! No generator script and no table derived from decoder reads supplies
//! these values.

use cadmpeg_ir::report::loss::LossNote;
use serde_json::Value;

/// One residual position under test.
#[derive(Debug, Clone)]
pub struct ResidualSpec {
    /// Model arena of the synthesized record.
    pub arena: &'static str,
    /// Identity of the synthesized record.
    pub record: &'static str,
    /// Dot-separated field path of the residual position within the record.
    pub position: &'static str,
    /// The value the format specification states, or `None` when it states
    /// none and the write must charge `residual-unavailable`.
    pub specified: Option<Value>,
}

/// Reads a dot-separated path below a record.
fn read_path<'a>(record: &'a Value, position: &str) -> Option<&'a Value> {
    let mut current = record;
    for part in position.split('.') {
        current = current.get(part)?;
    }
    Some(current)
}

/// Findings over a synthesized output: every specified position holds the
/// specification's value, and every unspecified one is charged.
#[must_use]
pub fn residual_findings(
    after: &Value,
    losses: &[LossNote],
    specs: &[ResidualSpec],
) -> Vec<String> {
    let mut findings = Vec::new();
    for spec in specs {
        let record = after
            .get("model")
            .and_then(|model| model.get(spec.arena))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find(|row| row.get("id").and_then(Value::as_str) == Some(spec.record));
        let Some(record) = record else {
            findings.push(format!("the synthesized record {} is missing", spec.record));
            continue;
        };
        match &spec.specified {
            Some(wanted) => match read_path(record, spec.position) {
                Some(got) if got == wanted => {}
                Some(got) => {
                    findings.push(format!(
                        "the residual position {}.{} of {} is {got}, not the specified {wanted}",
                        spec.arena, spec.position, spec.record
                    ));
                }
                None => {
                    findings.push(format!(
                        "the residual position {}.{} of {} is absent",
                        spec.arena, spec.position, spec.record
                    ));
                }
            },
            None => {
                let charged = losses.iter().any(|loss| {
                    loss.code.local_code() == "record.residual-unavailable"
                        && loss.message.contains(spec.record)
                });
                if !charged {
                    findings.push(format!(
                        "the unspecified residual position {}.{} of {} charges no residual-unavailable loss",
                        spec.arena, spec.position, spec.record
                    ));
                }
            }
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::{residual_findings, ResidualSpec};
    use cadmpeg_ir::codec::write::ExportLossCode;
    use cadmpeg_ir::report::loss::LossNote;
    use serde_json::json;

    fn loss(code: ExportLossCode, message: &str) -> LossNote {
        code.note(message)
    }

    #[test]
    fn specified_positions_must_match_and_unspecified_must_charge() {
        let after = json!({"model": {"faces": [
            {"id": "test:model:face#1", "flag": 1},
            {"id": "test:model:face#2", "flag": 0},
        ]}});
        let losses = [loss(
            ExportLossCode::RecordResidualUnavailable,
            "test:model:face#2 carries an unstated byte",
        )];
        let specs = [
            ResidualSpec {
                arena: "faces",
                record: "test:model:face#1",
                position: "flag",
                specified: Some(json!(1)),
            },
            ResidualSpec {
                arena: "faces",
                record: "test:model:face#2",
                position: "flag",
                specified: None,
            },
        ];
        assert!(residual_findings(&after, &losses, &specs).is_empty());
        let wrong = [ResidualSpec {
            arena: "faces",
            record: "test:model:face#1",
            position: "flag",
            specified: Some(json!(2)),
        }];
        assert_eq!(residual_findings(&after, &losses, &wrong).len(), 1);
        let uncharged = [ResidualSpec {
            arena: "faces",
            record: "test:model:face#1",
            position: "flag",
            specified: None,
        }];
        assert_eq!(residual_findings(&after, &losses, &uncharged).len(), 1);
    }
}
