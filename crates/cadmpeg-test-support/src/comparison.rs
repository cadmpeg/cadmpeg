// SPDX-License-Identifier: Apache-2.0
//! Record conservation under identity changes, with the §5.3 pairing rule.
//!
//! This comparison reads document JSON, never retained source bytes. The
//! existing `contract::assert_written_arenas_agree` keeps its own callers.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

mod export;
mod given_up;
mod graph;

pub use export::{allowed_removals_by_arena, stub_extents, unnamed_removals};
pub use given_up::DecodeSnapshot;

use graph::Document;

/// Facts removed by a witnessed projection before record comparison.
///
/// An empty value permits additions and identity changes, but no lost content.
/// Loss messages and writer coverage alone never populate this set.
#[derive(Debug, Default)]
pub struct Allowances {
    removed: BTreeSet<String>,
    removed_fields: BTreeSet<(String, String)>,
}

/// Which document content the comparison covers.
#[derive(Debug, Clone, Copy)]
pub enum Mode {
    /// Compare every neutral arena and every typed native namespace.
    Same,
}

/// Assert conservation of decoded document records under identity changes.
///
/// Extra records may appear. Required records, their values and their reference
/// aliasing must survive. Only `source`, `unknowns`, and a record's provenance
/// keys are outside this comparison. Ordinals compare by rank in their arena,
/// with separate ranks for each parameter owner.
///
/// # Panics
///
/// Panics with the first missing record or value disagreement. The inputs must
/// be document objects with an object-valued `model` and identified arena rows.
pub fn assert_decodes_agree(before: &Value, after: &Value, allowances: &Allowances, mode: Mode) {
    if let Err(error) = decodes_agree(before, after, allowances, mode) {
        panic!("decoded documents disagree: {error}");
    }
}

/// Compare as [`assert_decodes_agree`] does, and return the allowed records of
/// `before` that `after` does not hold.
///
/// # Errors
///
/// Returns the first missing record or value disagreement.
pub fn decodes_agree(
    before: &Value,
    after: &Value,
    allowances: &Allowances,
    mode: Mode,
) -> Result<BTreeSet<String>, String> {
    // An allowance prunes `before`; it applies only to content that `after`
    // does not hold. Field removals apply only when keeping them fails. Record
    // removals prune only the allowed records that `after` does not pair.
    let mut result = Err(String::new());
    for fields in [&BTreeSet::new(), &allowances.removed_fields] {
        result = agree_pruned(before, after, &allowances.removed, fields, mode);
        if result.is_ok() || allowances.removed_fields.is_empty() {
            break;
        }
    }
    result
}

fn agree_pruned(
    before: &Value,
    after: &Value,
    permitted: &BTreeSet<String>,
    fields: &BTreeSet<(String, String)>,
    mode: Mode,
) -> Result<BTreeSet<String>, String> {
    let unpruned = agree_once(before, after, fields, &BTreeSet::new(), None, mode);
    if let Ok(missing) = &unpruned {
        if missing.is_subset(permitted) {
            return unpruned;
        }
    }
    let unpaired = pairing_gaps(before, after, fields)?;
    let removed = unpaired
        .intersection(permitted)
        .cloned()
        .collect::<BTreeSet<_>>();
    agree_once(before, after, fields, &removed, Some(&removed), mode)
}

/// The records of `before` that `after` does not pair, with no pruning.
fn pairing_gaps(
    before: &Value,
    after: &Value,
    fields: &BTreeSet<(String, String)>,
) -> Result<BTreeSet<String>, String> {
    unpaired_only(&without_fields(before, fields), after)
}

fn unpaired_only(before: &Value, after: &Value) -> Result<BTreeSet<String>, String> {
    let before = Document::read(before, &BTreeSet::new())?;
    let after = Document::read(after, &BTreeSet::new())?;
    let pairs = pair(&before, &after);
    Ok(before
        .records
        .iter()
        .enumerate()
        .filter(|(index, _)| !pairs.contains_key(index))
        .map(|(_, record)| record.id.clone())
        .collect())
}

/// Colour pairing, then greedy `values_agree` pairing, in arena order.
fn pair(before: &Document, after: &Document) -> BTreeMap<usize, usize> {
    let (before_colours, after_colours) = graph::colours(before, after);
    let mut pairs = BTreeMap::new();
    let mut used = BTreeSet::new();
    // Colour pairing is deterministic in arena order and never deduplicates.
    for (index, record) in before.records.iter().enumerate() {
        if let Some((other, _)) = after.records.iter().enumerate().find(|(other, row)| {
            !used.contains(other)
                && record.arena == row.arena
                && before_colours[index] == after_colours[*other]
        }) {
            pairs.insert(index, other);
            used.insert(other);
        }
    }
    // Colours are fingerprints, not a floating-point equivalence relation.
    // This pass also handles values on opposite sides of a rounding boundary.
    for (index, record) in before.records.iter().enumerate() {
        if pairs.contains_key(&index) {
            continue;
        }
        if let Some((other, _)) = after.records.iter().enumerate().find(|(other, row)| {
            !used.contains(other)
                && record.arena == row.arena
                && cadmpeg_ir::compare::values_agree(
                    &unranked_shape(&record.value, &before.ids),
                    &unranked_shape(&row.value, &after.ids),
                )
                .is_ok()
        }) {
            pairs.insert(index, other);
            used.insert(other);
        }
    }
    pairs
}

/// One comparison: `pruned` records lose their list entries in `before`, and
/// every unpaired record of `before` must lie in `permitted`; `None` permits
/// no unpaired record.
fn agree_once(
    before: &Value,
    after: &Value,
    fields: &BTreeSet<(String, String)>,
    pruned: &BTreeSet<String>,
    permitted: Option<&BTreeSet<String>>,
    mode: Mode,
) -> Result<BTreeSet<String>, String> {
    let Mode::Same = mode;
    cadmpeg_ir::compare::values_agree(&header(before)?, &header(after)?)?;
    let before = without_fields(before, fields);
    let mut before = Document::read(&before, pruned)?;
    let mut after = Document::read(after, &BTreeSet::new())?;
    let pairs = pair(&before, &after);
    let mut missing = BTreeSet::new();
    for (index, record) in before.records.iter().enumerate() {
        if pairs.contains_key(&index) {
            continue;
        }
        if !permitted.is_some_and(|permitted| permitted.contains(&record.id)) {
            return Err(format!(
                "{}: required record {} is missing",
                record.arena, record.id
            ));
        }
        missing.insert(record.id.clone());
    }
    // Additions and witnessed removals do not shift the relative positions of
    // the records that the two documents actually pair.
    before.rank_selected(&pairs.keys().copied().collect());
    after.rank_selected(&pairs.values().copied().collect());
    let mut identities = after
        .records
        .iter()
        .map(|record| (record.id.clone(), format!("added:{}", record.id)))
        .collect::<BTreeMap<_, _>>();
    for (index, other) in &pairs {
        identities.insert(
            after.records[*other].id.clone(),
            before.records[*index].id.clone(),
        );
    }
    for (index, other) in pairs {
        let record = &before.records[index];
        let mapped = remap(&after.records[other].value, &identities, true);
        cadmpeg_ir::compare::values_agree(&record.value, &mapped)
            .map_err(|error| format!("{} {}: {error}", record.arena, record.id))?;
    }
    Ok(missing)
}

/// `document` without the allowed (record id, field) removals.
fn without_fields(document: &Value, fields: &BTreeSet<(String, String)>) -> Value {
    let mut document = document.clone();
    if fields.is_empty() {
        return document;
    }
    let mut prune = |rows: &mut Value| {
        for row in rows.as_array_mut().into_iter().flatten() {
            let Some(id) = row.get("id").and_then(Value::as_str).map(str::to_owned) else {
                continue;
            };
            if let Some(record) = row.as_object_mut() {
                record.retain(|key, _| !fields.contains(&(id.clone(), key.clone())));
            }
        }
    };
    if let Some(model) = document.get_mut("model").and_then(Value::as_object_mut) {
        model.values_mut().for_each(&mut prune);
    }
    if let Some(native) = document.get_mut("native").and_then(Value::as_object_mut) {
        for arenas in native.values_mut().filter_map(Value::as_object_mut) {
            arenas.values_mut().for_each(&mut prune);
        }
    }
    document
}

fn unranked_shape(value: &Value, identities: &BTreeMap<String, usize>) -> Value {
    let mut value = value.clone();
    if let Value::Object(fields) = &mut value {
        fields.remove("ordinal");
    }
    graph::shape(&value, identities, false)
}

fn header(document: &Value) -> Result<Value, String> {
    let mut fields = document
        .as_object()
        .ok_or("the document is not an object")?
        .clone();
    for key in ["model", "native", "source", "unknowns"] {
        fields.remove(key);
    }
    Ok(Value::Object(fields))
}

// A record's member names are fields, even when one equals a record id.
// Nested maps can carry reference keys and use the global identity mapping.
fn remap(value: &Value, identities: &BTreeMap<String, String>, record_fields: bool) -> Value {
    match value {
        Value::String(text) => identities
            .get(text)
            .map_or_else(|| value.clone(), |id| Value::String(id.clone())),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|value| remap(value, identities, false))
                .collect(),
        ),
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, value)| {
                    (
                        if record_fields {
                            key.clone()
                        } else {
                            identities.get(key).unwrap_or(key).clone()
                        },
                        remap(value, identities, false),
                    )
                })
                .collect(),
        ),
        _ => value.clone(),
    }
}

#[cfg(test)]
mod tests;
