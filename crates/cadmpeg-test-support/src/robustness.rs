// SPDX-License-Identifier: Apache-2.0
//! Injected-error, capacity-helper and `max_entities` harnesses (#217 §5.0).

use std::collections::BTreeSet;

use cadmpeg_core::decode::ResourceLimits;
use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{CapacityField, EncodeError, EncodeInput, Encoder};
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::CadIr;
use serde_json::Value;

/// One neutral-level error injection: mutates the document JSON and names
/// the affected record. Returns `None` when the seed holds no injection
/// site; that injection is inapplicable, not failed.
pub struct Injection {
    /// Short name for findings, such as `dangling-reference`.
    pub name: &'static str,
    /// Mutates `document`; the returned id names the affected record.
    pub inject: fn(&mut Value) -> Option<String>,
}

fn record_ids(document: &Value) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    if let Some(model) = document.get("model").and_then(Value::as_object) {
        for rows in model.values() {
            for row in rows.as_array().into_iter().flatten() {
                if let Some(id) = row.get("id").and_then(Value::as_str) {
                    ids.insert(id.to_owned());
                }
            }
        }
    }
    ids
}

fn repoint_id_value(document: &mut Value, ids: &BTreeSet<String>) -> Option<String> {
    fn walk(value: &mut Value, ids: &BTreeSet<String>) -> bool {
        match value {
            Value::Object(fields) => {
                for (key, child) in fields {
                    if key != "id" {
                        if let Value::String(text) = child {
                            if ids.contains(text) {
                                *text = "injected:missing:record#0".to_owned();
                                return true;
                            }
                        }
                        if walk(child, ids) {
                            return true;
                        }
                    }
                }
                false
            }
            Value::Array(items) => {
                for item in items {
                    if walk(item, ids) {
                        return true;
                    }
                }
                false
            }
            _ => false,
        }
    }
    let ids_snapshot = ids.clone();
    let model = document.get_mut("model")?.as_object_mut()?;
    for rows in model.values_mut() {
        for row in rows.as_array_mut()?.iter_mut() {
            let id = row.get("id")?.as_str()?.to_owned();
            if walk(row, &ids_snapshot) {
                return Some(id);
            }
        }
    }
    None
}

/// Standard injections against any populated seed.
#[must_use]
pub fn standard_injections() -> Vec<Injection> {
    vec![
        Injection {
            name: "dangling-reference",
            inject: |document| {
                let ids = record_ids(document);
                repoint_id_value(document, &ids)
            },
        },
        Injection {
            name: "duplicate-id",
            inject: |document| {
                let model = document.get_mut("model")?.as_object_mut()?;
                for rows in model.values_mut() {
                    let rows = rows.as_array_mut()?;
                    if rows.len() >= 2 {
                        let second = rows[1].get("id")?.as_str()?.to_owned();
                        let first_id = rows[0].get("id")?.as_str()?.to_owned();
                        rows[0]
                            .as_object_mut()?
                            .insert("id".to_owned(), Value::String(second));
                        return Some(first_id);
                    }
                }
                None
            },
        },
        Injection {
            name: "recompute-flag",
            inject: |document| {
                let features = document
                    .get_mut("model")?
                    .get_mut("features")?
                    .as_array_mut()?;
                for feature in features.iter_mut() {
                    let record = feature.as_object_mut()?;
                    if record.get("needs_recompute")?.as_bool() == Some(false) {
                        record.insert("needs_recompute".to_owned(), Value::Bool(true));
                        return record.get("id")?.as_str().map(str::to_owned);
                    }
                }
                None
            },
        },
    ]
}

/// Findings for injected validation errors over one seed: each applicable
/// injection must invalidate the document, yet the write succeeds with a
/// loss naming the affected record (#217 §5.0).
#[must_use]
pub fn injected_error_findings(
    seed: &CadIr,
    encoder: &dyn Encoder,
    request: TargetRequest<'_>,
    injections: &[Injection],
) -> Vec<String> {
    let mut findings = Vec::new();
    for injection in injections {
        let document = match serde_json::to_value(seed) {
            Ok(document) => document,
            Err(error) => {
                findings.push(format!("the seed does not serialize: {error}"));
                continue;
            }
        };
        let mut edited = document.clone();
        let Some(record) = (injection.inject)(&mut edited) else {
            continue;
        };
        let ir = match CadIr::from_json(&edited.to_string()) {
            Ok(ir) => ir,
            Err(error) => {
                findings.push(format!(
                    "the {} injection does not parse: {error}",
                    injection.name
                ));
                continue;
            }
        };
        match cadmpeg_ir::validate_neutral(&ir, Vec::new()) {
            Ok(report)
                if report
                    .findings
                    .iter()
                    .any(|finding| finding.severity == Severity::Error) => {}
            Ok(_) => {
                findings.push(format!(
                    "the {} injection reports no validation Error on {record}",
                    injection.name
                ));
                continue;
            }
            Err(error) => {
                findings.push(format!(
                    "the {} injection refuses validation: {error}",
                    injection.name
                ));
                continue;
            }
        }
        let planned = encoder.plan(EncodeInput::new(&ir, None), request);
        let plan = match planned {
            Ok(plan) => plan,
            Err(error) => {
                findings.push(format!(
                    "the {} write refuses {record}: {error}",
                    injection.name
                ));
                continue;
            }
        };
        let mut bytes = Vec::new();
        let report = match plan.write_to(&mut bytes) {
            Ok(report) => report,
            Err(error) => {
                findings.push(format!(
                    "the {} write refuses {record}: {error}",
                    injection.name
                ));
                continue;
            }
        };
        if !report
            .losses
            .iter()
            .any(|loss| loss.message.contains(&record))
        {
            findings.push(format!(
                "the {} write names {record} in no loss",
                injection.name
            ));
        }
    }
    findings
}

/// One capacity row under test: the writer's row plus its helper seam.
pub struct CapacityRow {
    /// The writer's declared row.
    pub field: &'static CapacityField,
    /// Writes `value` through the helper alone.
    pub write_at: Box<dyn Fn(u64) -> Result<Vec<u8>, EncodeError>>,
    /// Reads the stored value back through the decoder.
    pub read_back: Box<dyn Fn(&[u8]) -> u64>,
}

/// Findings for capacity helpers: `limit` writes and reads back, `limit + 1`
/// returns the row's `TargetCapacity` (#217 §5.0).
#[must_use]
pub fn capacity_findings(rows: &[CapacityRow]) -> Vec<String> {
    let mut findings = Vec::new();
    for row in rows {
        match (row.write_at)(row.field.limit) {
            Ok(bytes) => {
                if (row.read_back)(&bytes) != row.field.limit {
                    findings.push(format!(
                        "the {} helper does not read back {}",
                        row.field.field, row.field.limit
                    ));
                }
            }
            Err(error) => {
                findings.push(format!(
                    "the {} helper refuses its own limit {}: {error}",
                    row.field.field, row.field.limit
                ));
                continue;
            }
        }
        match (row.write_at)(row.field.limit + 1) {
            Ok(_) => {
                findings.push(format!(
                    "the {} helper writes past its limit {}",
                    row.field.field, row.field.limit
                ));
            }
            Err(EncodeError::TargetCapacity {
                field,
                limit,
                value,
            }) => {
                if field.field != row.field.field
                    || limit != row.field.limit
                    || value != row.field.limit + 1
                {
                    findings.push(format!(
                        "the {} helper reports another row's capacity",
                        row.field.field
                    ));
                }
            }
            Err(error) => {
                findings.push(format!(
                    "the {} helper reports {error} instead of TargetCapacity",
                    row.field.field
                ));
            }
        }
    }
    findings
}

/// Findings for the `max_entities` ceiling: planning below the golden's
/// record count refuses with the session's limit, while the desktop ceiling
/// plans (#217 §5.0).
#[must_use]
pub fn max_entities_findings(
    seed: &CadIr,
    encoder: &dyn Encoder,
    request: TargetRequest<'_>,
    record_count: u64,
) -> Vec<String> {
    let mut findings = Vec::new();
    if record_count == 0 {
        return vec!["the seed holds no records".to_owned()];
    }
    let control = encoder.plan(EncodeInput::new(seed, None), request);
    if control.is_err() {
        return vec!["the control write without a ceiling refuses".to_owned()];
    }
    let mut limits = ResourceLimits::desktop();
    limits.max_entities = record_count - 1;
    let input = EncodeInput {
        ir: seed,
        fidelity: None,
        limits,
    };
    match encoder.plan(input, request) {
        Ok(_) => {
            findings.push(format!(
                "planning with max_entities {} writes {record_count} records",
                record_count - 1
            ));
        }
        Err(error) => {
            let text = error.to_string();
            if !text.contains("max_entities")
                && !matches!(error, cadmpeg_core::CodecError::ResourceLimit(_))
            {
                findings.push(format!(
                    "planning with max_entities {} refuses without the limit: {error}",
                    record_count - 1
                ));
            }
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::{capacity_findings, standard_injections, CapacityRow};
    use cadmpeg_ir::codec::write::{CapacityField, EncodeError};
    use serde_json::json;

    const ROW: CapacityField = CapacityField {
        field: "count",
        limit: 7,
        source: "three bits",
        helper: "test::count",
    };

    fn write_at(value: u64) -> Result<Vec<u8>, EncodeError> {
        if value > ROW.limit {
            return Err(EncodeError::TargetCapacity {
                field: &ROW,
                limit: ROW.limit,
                value,
            });
        }
        Ok(vec![value as u8])
    }

    #[test]
    fn capacity_helper_passes_at_its_limit_and_refuses_above() {
        let rows = [CapacityRow {
            field: &ROW,
            write_at: Box::new(write_at),
            read_back: Box::new(|bytes| u64::from(bytes[0])),
        }];
        assert!(capacity_findings(&rows).is_empty());
    }

    #[test]
    fn capacity_helper_names_a_past_limit_write() {
        let rows = [CapacityRow {
            field: &ROW,
            write_at: Box::new(|_| Ok(Vec::new())),
            read_back: Box::new(|_| 0),
        }];
        let findings = capacity_findings(&rows);
        assert!(
            findings
                .iter()
                .any(|finding| finding.contains("past its limit")),
            "{findings:?}"
        );
    }

    #[test]
    fn standard_injections_find_sites_in_a_golden_shape() {
        let mut document = json!({
            "model": {
                "occurrences": [
                    {"id": "a:model:occurrence#1", "prototype": {"Local": {"definition": "a:model:product#1"}}},
                ],
                "product_definitions": [
                    {"id": "a:model:product#1"},
                    {"id": "a:model:product#2"},
                ],
            }
        });
        let injections = standard_injections();
        let dangling = injections
            .iter()
            .find(|injection| injection.name == "dangling-reference")
            .expect("dangling injection");
        assert_eq!(
            (dangling.inject)(&mut document),
            Some("a:model:occurrence#1".to_owned())
        );
        let duplicate = injections
            .iter()
            .find(|injection| injection.name == "duplicate-id")
            .expect("duplicate injection");
        let mut second = document.clone();
        assert!((duplicate.inject)(&mut second).is_some());
    }
}
