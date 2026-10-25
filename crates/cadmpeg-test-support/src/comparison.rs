// SPDX-License-Identifier: Apache-2.0
//! Record conservation under identity changes, with the §5.3 pairing rule.
//!
//! This comparison reads document JSON, never retained source bytes. The
//! existing `contract::assert_written_arenas_agree` keeps its own callers.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

pub mod given_up;
mod graph;
mod scope;

pub use given_up::DecodeSnapshot;

use graph::Document;

/// Facts removed by a witnessed projection before record comparison.
///
/// An empty value permits additions and identity changes, but no lost content.
/// Loss messages and writer coverage alone never populate this set.
#[derive(Debug, Default)]
pub struct Allowances {
    removed: BTreeSet<String>,
    changed: BTreeSet<String>,
    fallbacks: BTreeSet<String>,
    given_up: bool,
    lost_classes: BTreeSet<cadmpeg_ir::document::ArenaClass>,
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
    if let Err(error) = agree(before, after, allowances, mode) {
        panic!("decoded documents disagree: {error}");
    }
}

fn agree(before: &Value, after: &Value, allowances: &Allowances, mode: Mode) -> Result<(), String> {
    let Mode::Same = mode;
    let (before_header, after_header) = scope::headers(before, after, allowances)?;
    cadmpeg_ir::compare::values_agree(&before_header, &after_header)?;
    let original_before = before;
    let mut before = Document::read(before, &BTreeSet::new())?;
    let mut after = Document::read(after, &BTreeSet::new())?;
    if allowances.given_up {
        given_up::check_changes(
            &before,
            &after,
            &allowances.changed,
            &allowances.fallbacks,
            &allowances.removed,
            original_before,
        )?;
    }
    before.prune_records(&allowances.removed, original_before)?;
    before.exclude_records(&allowances.changed);
    after.exclude_records(&allowances.changed);
    before.exclude_records(&allowances.fallbacks);
    after.exclude_records(&allowances.fallbacks);
    // Unique identities and arenas were validated above. Exact same-identity
    // values provide a constructive pairing; additions are already permitted
    // by Mode::Same. Any difference keeps the full identity-independent path.
    if before.records.iter().all(|record| {
        after.ids.get(&record.id).is_some_and(|index| {
            let current = &after.records[*index];
            record.arena == current.arena && record.value == current.value
        })
    }) {
        return Ok(());
    }
    let (before_colours, after_colours) = graph::colours(&before, &after);
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
    for (index, record) in before.records.iter().enumerate() {
        if !pairs.contains_key(&index) && !allowances.removed.contains(&record.id) {
            return Err(format!(
                "{}: required record {} is missing",
                record.arena, record.id
            ));
        }
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
    Ok(())
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
