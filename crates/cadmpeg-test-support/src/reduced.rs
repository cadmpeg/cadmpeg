// SPDX-License-Identifier: Apache-2.0
//! `REDUCED_FIELDS` row harness (#232 §5.2).
//!
//! For each row the harness writes a value the storage cannot represent and
//! asserts that the re-decode returns the stored value, different from the
//! input: within one quantum for a quantum row, equal to the normalization
//! for a normalization row. A row whose storage does not reduce fails.

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{Encoder, ReducedStorage};
use cadmpeg_ir::CadIr;
use serde_json::Value;

use super::cross_closure::{apply_storage, quantum, ReducedProbe};
use super::writer_closure::Redecode;

/// Standard unrepresentable probe for a quantum storage. `Normalized` rows
/// need a writer-supplied probe: only the writer knows its normalization.
#[must_use]
pub fn quantum_probe(storage: ReducedStorage) -> Option<Value> {
    match storage {
        ReducedStorage::U8Channel => Some(Value::from(0.1)),
        ReducedStorage::F32 => Some(Value::from(0.1)),
        ReducedStorage::Decimal(places) => {
            let probe = 1.0 + 10.0_f64.powi(-(places as i32) - 1);
            Some(Value::from(probe))
        }
        ReducedStorage::Normalized { .. } => None,
    }
}

/// Reads a dot-separated relative path below a record.
fn read_path<'a>(record: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = record;
    for part in path.split('.') {
        current = current.get(part)?;
    }
    Some(current)
}

/// Writes a dot-separated relative path below a record clone.
fn write_path(record: &mut Value, path: &str, probe: Value) -> bool {
    let mut current = record;
    let mut parts = path.split('.').peekable();
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            if let Some(object) = current.as_object_mut() {
                object.insert(part.to_owned(), probe);
                return true;
            }
            return false;
        }
        current = match current.get_mut(part) {
            Some(next) => next,
            None => return false,
        };
    }
    false
}

/// Findings for one row over a seed document: locate the first record
/// holding the row's path, write the probe there, and compare the
/// re-decoded value against the row's storage rule.
#[must_use]
pub fn reduced_row_findings(
    seed: &CadIr,
    encoder: &dyn Encoder,
    request: TargetRequest<'_>,
    redecode: &Redecode<'_>,
    probe: &ReducedProbe,
    writer_probe: Option<Value>,
) -> Vec<String> {
    let mut findings = Vec::new();
    let document = match serde_json::to_value(seed) {
        Ok(document) => document,
        Err(error) => return vec![format!("the seed does not serialize: {error}")],
    };
    let target = probe_target(&document, probe.path);
    let Some((arena, index, _)) = target else {
        return vec![format!("no record holds the reduced path {}", probe.path)];
    };
    let probe_value = match writer_probe.or_else(|| quantum_probe(probe.storage)) {
        Some(value) => value,
        None => {
            return vec![format!(
                "the normalized row {} needs a writer probe",
                probe.path
            )];
        }
    };
    let stored = apply_storage(probe.storage, &probe_value, probe.normalize);
    if stored == probe_value {
        findings.push(format!(
            "the probe for {} is representable; the row does not reduce",
            probe.path
        ));
        return findings;
    }
    let mut edited = document.clone();
    let record = &mut edited["model"][arena.as_str()][index];
    if !write_path(record, probe.path, probe_value.clone()) {
        return vec![format!("the probe does not land on {}", probe.path)];
    }
    let ir = match CadIr::from_json(&edited.to_string()) {
        Ok(ir) => ir,
        Err(error) => {
            return vec![format!("the probed document does not parse: {error}")];
        }
    };
    match cadmpeg_ir::validate_neutral(&ir, Vec::new()) {
        Ok(report) if report.error_count() == 0 => {}
        _ => {
            return vec![format!(
                "the probe on {} invalidates the document",
                probe.path
            )];
        }
    }
    let planned = encoder.plan(
        cadmpeg_ir::codec::write::EncodeInput::new(&ir, None),
        request,
    );
    let plan = match planned {
        Ok(plan) => plan,
        Err(error) => return vec![format!("the probed write refuses: {error}")],
    };
    let mut bytes = Vec::new();
    if let Err(error) = plan.write_to(&mut bytes) {
        return vec![format!("the probed write refuses: {error}")];
    }
    let after = match redecode(&bytes) {
        Ok(after) => after,
        Err(error) => return vec![format!("the probed output does not decode: {error}")],
    };
    let record_id = edited["model"][arena.as_str()][index]["id"]
        .as_str()
        .unwrap_or("")
        .to_owned();
    let got = after
        .ir
        .get("model")
        .and_then(|model| model.get(arena.as_str()))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|row| row.get("id").and_then(Value::as_str) == Some(record_id.as_str()))
        .and_then(|row| read_path(row, probe.path))
        .cloned();
    let Some(got) = got else {
        findings.push(format!("the probed record {record_id} is missing"));
        return findings;
    };
    if got == probe_value {
        findings.push(format!(
            "the re-decode returns the probe on {}; the row does not reduce",
            probe.path
        ));
        return findings;
    }
    match probe.storage {
        ReducedStorage::Normalized { .. } => {
            if got != stored {
                findings.push(format!(
                    "the re-decode on {} is {got}, not the normalization {stored}",
                    probe.path
                ));
            }
        }
        storage => {
            let (got, stored) = match (got.as_f64(), stored.as_f64()) {
                (Some(got), Some(stored)) => (got, stored),
                _ => {
                    findings.push(format!("the re-decode on {} is not a number", probe.path));
                    return findings;
                }
            };
            if (got - stored).abs() > quantum(storage, stored) {
                findings.push(format!(
                    "the re-decode on {} is {got}, outside one quantum of {stored}",
                    probe.path
                ));
            }
        }
    }
    findings
}

/// The first neutral record holding `path`, as (arena, index, id).
fn probe_target(document: &Value, path: &str) -> Option<(String, usize, String)> {
    document
        .get("model")?
        .as_object()?
        .iter()
        .flat_map(|(arena, rows)| {
            rows.as_array()
                .into_iter()
                .flatten()
                .enumerate()
                .filter_map(move |(index, row)| {
                    read_path(row, path)?;
                    let id = row.get("id")?.as_str()?.to_owned();
                    Some((arena.clone(), index, id))
                })
        })
        .next()
}
