// SPDX-License-Identifier: Apache-2.0
//! §5.1(a) export allowances, computed from the first decode alone.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use cadmpeg_ir::report::loss::LossNote;
use serde_json::Value;

use super::given_up::required_references;
use super::{graph::Document, Allowances};

const UNDECODED_OMITTED: &str = "source.undecoded-omitted";
const RECORDS_OMITTED: &str = "native.records-omitted";

impl Allowances {
    /// Build the §5.1(a) allowances of a write without fidelity.
    ///
    /// A B1 span is an `unknowns` stub; it may be removed. A record field whose
    /// only value is a stub may be removed. A record that needs a removed record
    /// is removed in turn (§4.5 rule 2, W1); a record that lists one loses only
    /// that entry. Losses never add an allowance.
    ///
    /// # Errors
    /// Refuses a document without an object-valued model or identified rows.
    pub fn export(before: &Value) -> Result<Self, String> {
        let stubs = stub_ids(before);
        let pruned = without_stub_fields(before, &stubs);
        let records = Document::read(&pruned, &BTreeSet::new())?;
        let mut users = BTreeMap::<String, BTreeSet<String>>::new();
        for record in &records.records {
            let mut required = BTreeSet::new();
            required_references(&record.value, &records.ids, &mut required);
            for target in required {
                users.entry(target).or_default().insert(record.id.clone());
            }
        }
        let mut removed = stubs.clone();
        let mut queue = stubs.into_iter().collect::<VecDeque<_>>();
        while let Some(target) = queue.pop_front() {
            for owner in users.get(&target).into_iter().flatten() {
                if removed.insert(owner.clone()) {
                    queue.push_back(owner.clone());
                }
            }
        }
        Ok(Self {
            removed,
            removed_fields: stub_fields(before, &stub_ids(before)),
        })
    }
}

fn stub_ids(document: &Value) -> BTreeSet<String> {
    document
        .get("native")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|namespaces| namespaces.values())
        .filter_map(|arenas| arenas.get("unknowns")?.as_array())
        .flatten()
        .filter_map(|row| Some(row.get("id")?.as_str()?.to_owned()))
        .collect()
}

/// (record id, field) pairs whose value is one stub identity.
fn stub_fields(document: &Value, stubs: &BTreeSet<String>) -> BTreeSet<(String, String)> {
    let mut fields = BTreeSet::new();
    for_each_record(document, |id, record| {
        for (key, value) in record {
            if key != "id" && value.as_str().is_some_and(|text| stubs.contains(text)) {
                fields.insert((id.to_owned(), key.clone()));
            }
        }
    });
    fields
}

fn without_stub_fields(document: &Value, stubs: &BTreeSet<String>) -> Value {
    let mut pruned = document.clone();
    for_each_record_mut(&mut pruned, |record| {
        record.retain(|key, value| {
            key == "id" || !value.as_str().is_some_and(|text| stubs.contains(text))
        });
    });
    pruned
}

fn rows(document: &Value) -> Vec<&Value> {
    let model = document
        .get("model")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|model| model.values());
    let native = document
        .get("native")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|namespaces| namespaces.values())
        .filter_map(Value::as_object)
        .flat_map(|arenas| arenas.values());
    model
        .chain(native)
        .filter_map(Value::as_array)
        .flatten()
        .collect()
}

fn for_each_record(document: &Value, mut visit: impl FnMut(&str, &serde_json::Map<String, Value>)) {
    for row in rows(document) {
        if let (Some(id), Some(record)) = (row.get("id").and_then(Value::as_str), row.as_object()) {
            visit(id, record);
        }
    }
}

fn for_each_record_mut(
    document: &mut Value,
    mut visit: impl FnMut(&mut serde_json::Map<String, Value>),
) {
    let mut visit_rows = |rows: &mut Value| {
        for row in rows.as_array_mut().into_iter().flatten() {
            if let Some(record) = row.as_object_mut() {
                visit(record);
            }
        }
    };
    if let Some(model) = document.get_mut("model").and_then(Value::as_object_mut) {
        model.values_mut().for_each(&mut visit_rows);
    }
    if let Some(native) = document.get_mut("native").and_then(Value::as_object_mut) {
        for arenas in native.values_mut().filter_map(Value::as_object_mut) {
            arenas.values_mut().for_each(&mut visit_rows);
        }
    }
}

/// The number of records of each model arena that §5.1(a) allows a write
/// without fidelity to remove.
#[must_use]
pub fn allowed_removals_by_arena(before: &Value) -> BTreeMap<String, usize> {
    let Ok(allowances) = Allowances::export(before) else {
        return BTreeMap::new();
    };
    let mut counts = BTreeMap::new();
    for (arena, rows) in before
        .get("model")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        let removed = rows
            .as_array()
            .into_iter()
            .flatten()
            .filter(|row| {
                row.get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| allowances.removed.contains(id))
            })
            .count();
        counts.insert(arena.clone(), removed);
    }
    counts
}

/// The source extent of every stub, from the first decode's fidelity.
#[must_use]
pub fn stub_extents(document: &Value, fidelity: &Value) -> BTreeMap<String, (String, u64, u64)> {
    stub_ids(document)
        .into_iter()
        .filter_map(|id| {
            let record = fidelity.pointer("/retained_records")?.get(&id)?;
            let stream = record.get("stream")?.as_str()?.to_owned();
            let offset = record.get("offset")?.as_u64()?;
            let bytes = record.get("bytes")?;
            let length = match bytes.get("retention")?.as_str()? {
                "inline" => {
                    let data = bytes.get("data")?.as_str()?;
                    let padding = data.bytes().rev().take_while(|byte| *byte == b'=').count();
                    u64::try_from(data.len() / 4 * 3 - padding).ok()?
                }
                _ => bytes.get("byte_len")?.as_u64()?,
            };
            Some((id, (stream, offset, length)))
        })
        .collect()
}

/// The removed records of an export that no loss names (§5.1(a), §5.6).
///
/// A removed stub is named by a `source.undecoded-omitted` loss that gives
/// its stream, offset and length; any other removed record by a
/// `native.records-omitted` loss whose message holds its id.
#[must_use]
pub fn unnamed_removals(
    removed: &BTreeSet<String>,
    stubs: &BTreeMap<String, (String, u64, u64)>,
    losses: &[LossNote],
) -> Vec<String> {
    let messages = |code: &str| {
        losses
            .iter()
            .filter(|loss| loss.code.local_code() == code)
            .map(|loss| loss.message.as_str())
            .collect::<Vec<_>>()
    };
    let undecoded = messages(UNDECODED_OMITTED);
    let omitted = messages(RECORDS_OMITTED);
    removed
        .iter()
        .filter(|id| match stubs.get(id.as_str()) {
            Some((stream, offset, length)) => !undecoded.iter().any(|message| {
                message.contains(stream.as_str())
                    && message.contains(&offset.to_string())
                    && message.contains(&length.to_string())
            }),
            None => !omitted.iter().any(|message| message.contains(id.as_str())),
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests;
