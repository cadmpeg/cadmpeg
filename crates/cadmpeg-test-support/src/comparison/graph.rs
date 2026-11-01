// SPDX-License-Identifier: Apache-2.0
//! Arena rows, reference colours, and provenance-free comparison values.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::hash::digest::Sha256Digest;
use serde_json::{Number, Value};

pub(super) struct Record {
    pub(super) arena: String,
    pub(super) id: String,
    pub(super) value: Value,
}

pub(super) struct Document {
    pub(super) records: Vec<Record>,
    pub(super) ids: BTreeMap<String, usize>,
}

impl Document {
    pub(super) fn read(document: &Value, removed: &BTreeSet<String>) -> Result<Self, String> {
        let model = document
            .get("model")
            .and_then(Value::as_object)
            .ok_or("the document has no object-valued model")?;
        let mut records = Vec::new();
        for (arena, rows) in model {
            append(&mut records, &format!("model/{arena}"), rows, removed)?;
        }
        if let Some(native) = document.get("native") {
            for (namespace, arenas) in native.as_object().ok_or("native is not an object")? {
                for (arena, rows) in arenas
                    .as_object()
                    .ok_or("native namespace is not an object")?
                {
                    append(
                        &mut records,
                        &format!("native/{namespace}/{arena}"),
                        rows,
                        removed,
                    )?;
                }
            }
        }
        let mut ids = BTreeMap::new();
        for (index, record) in records.iter().enumerate() {
            if ids.insert(record.id.clone(), index).is_some() {
                return Err(format!("duplicate record identity {}", record.id));
            }
        }
        Ok(Self { records, ids })
    }

    pub(super) fn rank_selected(&mut self, selected: &BTreeSet<usize>) {
        let mut groups = BTreeMap::<(String, Option<String>), Vec<(u64, usize)>>::new();
        for index in selected {
            let record = &self.records[*index];
            if let Some(ordinal) = record.value.get("ordinal").and_then(Value::as_u64) {
                let owner = (record.arena == "model/parameters")
                    .then(|| {
                        record
                            .value
                            .get("owner")
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                    })
                    .flatten();
                groups
                    .entry((record.arena.clone(), owner))
                    .or_default()
                    .push((ordinal, *index));
            }
        }
        for group in groups.values_mut() {
            group.sort_unstable();
            for (rank, (_, index)) in group.iter().enumerate() {
                self.records[*index].value["ordinal"] = Value::Number(Number::from(
                    u64::try_from(rank).expect("selected rank fits the admitted u64 arena rank"),
                ));
            }
        }
    }
}

fn append(
    records: &mut Vec<Record>,
    arena: &str,
    rows: &Value,
    removed: &BTreeSet<String>,
) -> Result<(), String> {
    let mut groups = BTreeMap::<Option<String>, Vec<(u64, usize)>>::new();
    let first = records.len();
    for row in rows
        .as_array()
        .ok_or_else(|| format!("{arena} is not an arena array"))?
    {
        let mut value = row
            .as_object()
            .ok_or_else(|| format!("{arena} has a non-object record"))?
            .clone();
        let id = value
            .remove("id")
            .and_then(|id| id.as_str().map(str::to_owned))
            .ok_or_else(|| format!("{arena} has a record without a string id"))?;
        value.remove("native_ref");
        value.remove("source");
        // D21 makes the source-object association identity-only, so a
        // writer-minted identity is not content.
        value.remove("source_object");
        if let Some(ordinal) = value.get("ordinal") {
            let ordinal = ordinal
                .as_u64()
                .ok_or_else(|| format!("{arena} has a non-integer ordinal"))?;
            let owner = (arena == "model/parameters")
                .then(|| {
                    value
                        .get("owner")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .flatten();
            groups
                .entry(owner)
                .or_default()
                .push((ordinal, records.len()));
        }
        records.push(Record {
            arena: arena.to_owned(),
            id,
            value: Value::Object(value),
        });
    }
    for group in groups.values_mut() {
        group.sort_unstable();
        for (rank, (_, index)) in group.iter().enumerate() {
            let rank = u64::try_from(rank).map_err(|_| "ordinal rank exceeds u64")?;
            records[*index].value["ordinal"] = Value::Number(Number::from(rank));
        }
    }
    for record in &mut records[first..] {
        filter_lists(&mut record.value, removed);
    }
    Ok(())
}

fn filter_lists(value: &mut Value, removed: &BTreeSet<String>) {
    match value {
        Value::Array(values) => {
            values.retain(|value| !refers_to_removed(value, removed));
            for value in values {
                filter_lists(value, removed);
            }
        }
        Value::Object(fields) => {
            for value in fields.values_mut() {
                filter_lists(value, removed);
            }
        }
        _ => {}
    }
}

fn refers_to_removed(value: &Value, removed: &BTreeSet<String>) -> bool {
    match value {
        Value::String(id) => removed.contains(id),
        // A nested list loses its own removed entries, not its enclosing list
        // holder and the holder's independent values.
        Value::Array(_) => false,
        Value::Object(fields) => fields
            .iter()
            .any(|(key, value)| removed.contains(key) || refers_to_removed(value, removed)),
        _ => false,
    }
}

/// Retain record field names, JSON types and nested map reference keys.
pub(super) fn shape(value: &Value, ids: &BTreeMap<String, usize>, rounded: bool) -> Value {
    shape_value(value, ids, rounded, true)
}

fn shape_value(
    value: &Value,
    ids: &BTreeMap<String, usize>,
    rounded: bool,
    record_fields: bool,
) -> Value {
    match value {
        Value::String(id) if ids.contains_key(id) => serde_json::json!({"reference": true}),
        Value::Object(fields) => {
            let mut entries = fields
                .iter()
                .map(|(key, value)| {
                    Value::Array(vec![
                        if record_fields {
                            Value::String(key.clone())
                        } else {
                            shape_value(&Value::String(key.clone()), ids, rounded, false)
                        },
                        shape_value(value, ids, rounded, false),
                    ])
                })
                .collect::<Vec<_>>();
            entries.sort_by_cached_key(|entry| {
                serde_json::to_string(entry).expect("JSON entry serializes")
            });
            serde_json::json!({"object": entries})
        }
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|value| shape_value(value, ids, rounded, false))
                .collect(),
        ),
        Value::Number(number) if rounded => {
            let value = number.as_f64().expect("finite JSON number");
            let quantum = 10.0_f64.powf(value.abs().max(1.0).log10().floor() - 8.0);
            let rounded = (value / quantum).round() * quantum;
            Value::Number(
                Number::from_f64(if rounded == 0.0 { 0.0 } else { rounded })
                    .expect("finite rounded colour"),
            )
        }
        _ => value.clone(),
    }
}

fn outgoing(
    value: &Value,
    ids: &BTreeMap<String, usize>,
    path: &str,
    edges: &mut Vec<(String, usize)>,
) {
    match value {
        Value::String(id) => {
            if let Some(index) = ids.get(id) {
                edges.push((path.to_owned(), *index));
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                outgoing(value, ids, &format!("{path}/{index}"), edges);
            }
        }
        Value::Object(fields) => {
            for (key, value) in fields {
                // The root object's keys are record fields, not reference keys.
                let field = if let Some(index) = ids.get(key).filter(|_| !path.is_empty()) {
                    edges.push((format!("{path}/{{key}}"), *index));
                    "{reference-key}"
                } else {
                    key
                };
                outgoing(value, ids, &format!("{path}/{field}"), edges);
            }
        }
        _ => {}
    }
}

fn hash(value: &Value) -> String {
    Sha256Digest::digest(&serde_json::to_vec(value).expect("JSON colour serializes")).into()
}

fn initial(document: &Document) -> Vec<String> {
    document
        .records
        .iter()
        .map(|record| hash(&shape(&record.value, &document.ids, true)))
        .collect()
}

fn refine(document: &Document, zero: &[String], previous: &[String]) -> Vec<String> {
    document
        .records
        .iter()
        .enumerate()
        .map(|(index, record)| {
            let mut edges = Vec::new();
            outgoing(&record.value, &document.ids, "", &mut edges);
            let mut neighbours = edges
                .into_iter()
                .map(|(path, referent)| (path, &previous[referent]))
                .collect::<Vec<_>>();
            neighbours.sort_unstable();
            hash(&serde_json::json!([zero[index], neighbours]))
        })
        .collect()
}

fn counts(
    before: &Document,
    after: &Document,
    left: &[String],
    right: &[String],
) -> BTreeMap<String, usize> {
    let mut colours = BTreeMap::<String, BTreeSet<&str>>::new();
    for (record, colour) in before
        .records
        .iter()
        .zip(left)
        .chain(after.records.iter().zip(right))
    {
        colours
            .entry(record.arena.clone())
            .or_default()
            .insert(colour);
    }
    colours
        .into_iter()
        .map(|(arena, colours)| (arena, colours.len()))
        .collect()
}

pub(super) fn colours(before: &Document, after: &Document) -> (Vec<String>, Vec<String>) {
    let left_zero = initial(before);
    let right_zero = initial(after);
    let mut left = left_zero.clone();
    let mut right = right_zero.clone();
    let mut previous = counts(before, after, &left, &right);
    loop {
        let next_left = refine(before, &left_zero, &left);
        let next_right = refine(after, &right_zero, &right);
        let next = counts(before, after, &next_left, &next_right);
        let settled = next
            .iter()
            .all(|(arena, count)| previous.get(arena).is_some_and(|old| count <= old));
        left = next_left;
        right = next_right;
        if settled {
            return (left, right);
        }
        previous = next;
    }
}
