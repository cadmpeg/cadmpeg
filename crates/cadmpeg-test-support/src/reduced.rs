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

/// Apply STEP's positive-radius and positive-semi-angle surface charts.
#[must_use]
pub fn step_surface_chart(value: &Value) -> Value {
    let mut geometry = value.clone();
    let Some(object) = geometry.as_object_mut() else {
        return geometry;
    };
    let radius_field = match object.get("kind").and_then(Value::as_str) {
        Some("sphere") => Some("radius"),
        Some("torus") => Some("minor_radius"),
        _ => None,
    };
    if let Some(field) = radius_field {
        if let Some(radius) = object.get(field).and_then(Value::as_f64) {
            if radius < 0.0 {
                object.insert(field.to_owned(), Value::from(-radius));
            }
        }
        return geometry;
    }
    if object.get("kind").and_then(Value::as_str) != Some("cone") {
        return geometry;
    }
    let Some(angle) = object.get("half_angle").and_then(Value::as_f64) else {
        return geometry;
    };
    if angle >= 0.0 {
        return geometry;
    }
    let Some(axis) = object.get_mut("axis").and_then(Value::as_object_mut) else {
        return geometry;
    };
    let coordinates = ["x", "y", "z"].map(|key| axis.get(key).and_then(Value::as_f64));
    let [Some(x), Some(y), Some(z)] = coordinates else {
        return geometry;
    };
    axis.insert("x".into(), Value::from(-x));
    axis.insert("y".into(), Value::from(-y));
    axis.insert("z".into(), Value::from(-z));
    object.insert("half_angle".into(), Value::from(-angle));
    geometry
}

/// Apply STEP's extended timestamp syntax to a document date attribute.
#[must_use]
pub fn step_timestamp_attribute(value: &Value) -> Value {
    let mut attribute = value.clone();
    let Some(record) = attribute.as_object_mut() else {
        return attribute;
    };
    if record
        .get("target")
        .and_then(|target| target.get("kind"))
        .and_then(Value::as_str)
        != Some("document")
        || !matches!(
            record.get("name").and_then(Value::as_str),
            Some("generation_date" | "model_date" | "CreationDate" | "LastModifiedDate")
        )
    {
        return attribute;
    }
    let Some([entry]) = record
        .get_mut("values")
        .and_then(Value::as_array_mut)
        .map(Vec::as_mut_slice)
    else {
        return attribute;
    };
    if entry.get("kind").and_then(Value::as_str) != Some("string") {
        return attribute;
    }
    let Some(text) = entry.get("value").and_then(Value::as_str) else {
        return attribute;
    };
    let Some((date, time)) = text.split_once('.') else {
        return attribute;
    };
    if date.len() != 8
        || time.len() != 6
        || !date.bytes().chain(time.bytes()).all(|b| b.is_ascii_digit())
    {
        return attribute;
    }
    let parsed = [
        &date[0..4],
        &date[4..6],
        &date[6..8],
        &time[0..2],
        &time[2..4],
        &time[4..6],
    ]
    .map(str::parse::<u32>);
    let [Ok(year), Ok(month), Ok(day), Ok(hour), Ok(minute), Ok(second)] = parsed else {
        return attribute;
    };
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(400) || (year.is_multiple_of(4) && !year.is_multiple_of(100)) => {
            29
        }
        2 => 28,
        _ => return attribute,
    };
    if day == 0
        || day > days
        || hour > 24
        || minute > 59
        || second > 60
        || (hour == 24 && (minute != 0 || second != 0))
    {
        return attribute;
    }
    let timestamp = format!(
        "{}-{}-{}T{}:{}:{}",
        &date[0..4],
        &date[4..6],
        &date[6..8],
        &time[0..2],
        &time[2..4],
        &time[4..6]
    );
    entry["value"] = Value::String(timestamp);
    attribute
}

/// Standard unrepresentable probe for a quantum storage. `Normalized` rows
/// need a writer-supplied probe: only the writer knows its normalization.
#[must_use]
pub fn quantum_probe(storage: ReducedStorage) -> Option<Value> {
    match storage {
        ReducedStorage::U8Channel => Some(Value::from(0.1)),
        ReducedStorage::F32 => Some(Value::from(0.1)),
        ReducedStorage::Decimal(places) => {
            let probe = 1.0 + 10.0_f64.powi(-places.cast_signed() - 1);
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
    let Some(probe_value) = writer_probe.or_else(|| quantum_probe(probe.storage)) else {
        return vec![format!(
            "the normalized row {} needs a writer probe",
            probe.path
        )];
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
            let (Some(got), Some(stored)) = (got.as_f64(), stored.as_f64()) else {
                findings.push(format!("the re-decode on {} is not a number", probe.path));
                return findings;
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
