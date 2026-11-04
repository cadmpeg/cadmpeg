// SPDX-License-Identifier: Apache-2.0
//! Record conservation under identity changes, with the §5.3 pairing rule.
//!
//! This comparison reads document JSON, never retained source bytes. The
//! existing `contract::assert_written_arenas_agree` keeps its own callers.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

mod cross;
mod export;
mod given_up;
mod graph;

use graph::Document;

/// Facts removed by a witnessed projection before record comparison.
///
/// An empty value permits additions and identity changes, but no lost content.
/// Loss messages and writer coverage alone never populate this set.
#[derive(Debug, Default)]
pub struct Allowances {
    removed: BTreeSet<String>,
    removed_fields: BTreeSet<(String, String)>,
    /// Fields of records that contain a given-up part (#215 rule 1). The
    /// comparison refuses a held value in one and otherwise prunes it.
    absent_fields: BTreeSet<(String, String)>,
}

impl Allowances {
    /// Records the comparison may drop (#232 §5.1 harness).
    #[must_use]
    pub fn removed_ids(&self) -> &BTreeSet<String> {
        &self.removed
    }

    /// An explicit drop set (#232 §5.2 harness): only these records may be
    /// unpaired. Every other record of `before` must pair.
    #[must_use]
    pub fn removed(ids: BTreeSet<String>) -> Self {
        Self {
            removed: ids,
            removed_fields: BTreeSet::new(),
            absent_fields: BTreeSet::new(),
        }
    }

    /// Fields the comparison may drop (#232 §5.1 harness).
    #[must_use]
    pub fn removed_field_ids(&self) -> &BTreeSet<(String, String)> {
        &self.removed_fields
    }

    /// Fields of records that contain a given-up part (#215 rule 1). The
    /// comparison refuses a held value in one; an absent, null or empty
    /// field otherwise compares as pruned. Named by record identity, so
    /// both decodes must mint the same record ids, as the §4.5 suite does.
    #[must_use]
    pub fn absent_field_ids(&self) -> &BTreeSet<(String, String)> {
        &self.absent_fields
    }
}

/// Records of `before` that `after` does not pair, with no pruning (#232
/// §5.1 harness names each one in an export loss).
///
/// For `Cross` the projection applies to both documents before pairing, as
/// `agree` does.
///
/// # Errors
///
/// Refuses a document without an object-valued model or identified rows.
pub fn unpaired_before_ids(
    before: &Value,
    after: &Value,
    mode: &Mode,
) -> Result<BTreeSet<String>, String> {
    match mode {
        Mode::Same => pairing_gaps(before, after, &BTreeSet::new(), false),
        Mode::Cross(projection) => {
            let before = project(before, projection);
            let after = project(&after, projection);
            pairing_gaps(&before, &after, &BTreeSet::new(), true)
        }
    }
}

/// Content a cross-format comparison projects away before pairing.
///
/// The projection names record keys the target cannot represent (§5.2). Both
/// documents lose those keys before colouring, so only the remaining content
/// pairs.
#[derive(Debug, Clone, Default)]
pub struct CrossProjection {
    dropped_keys: BTreeSet<String>,
}

impl CrossProjection {
    /// Project away every record key in `dropped_keys` on both documents.
    #[must_use]
    pub fn new(dropped_keys: BTreeSet<String>) -> Self {
        Self { dropped_keys }
    }
}

/// Which document content the comparison covers.
#[derive(Debug, Clone)]
pub enum Mode {
    /// Compare every neutral arena and every typed native namespace.
    Same,
    /// Compare neutral arenas only, after dropping the projection keys.
    ///
    /// A key present only in `after` is ignored; a key present only in
    /// `before` fails.
    Cross(CrossProjection),
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

/// Compare as [`assert_decodes_agree`] does, without panicking (#232 harness).
pub fn agree(
    before: &Value,
    after: &Value,
    allowances: &Allowances,
    mode: Mode,
) -> Result<(), String> {
    match mode {
        Mode::Same => {
            cadmpeg_ir::compare::values_agree(&header(before)?, &header(after)?)?;
            agree_pruned(before, after, allowances, false)
        }
        Mode::Cross(projection) => {
            // Two formats never share units, tolerances or source metadata, so
            // the document header is outside a cross-format comparison.
            let before = project(before, &projection);
            let after = project(after, &projection);
            agree_pruned(&before, &after, allowances, true)
        }
    }
}

/// One comparison, pruning only the allowances `after` does not hold.
///
/// An allowance prunes `before`; it applies only to content that `after` does
/// not hold. Field removals apply only when keeping the field fails. Record
/// removals prune only the allowed records that `after` does not pair.
fn agree_pruned(
    before: &Value,
    after: &Value,
    allowances: &Allowances,
    cross: bool,
) -> Result<(), String> {
    verify_absent_fields(after, &allowances.absent_fields)?;
    let before = without_fields(before, &allowances.absent_fields);
    let after = without_fields(after, &allowances.absent_fields);
    let mut result = Err(String::new());
    for fields in [&BTreeSet::new(), &allowances.removed_fields] {
        result = agree_once_pruned(&before, &after, &allowances.removed, fields, cross);
        if result.is_ok() || allowances.removed_fields.is_empty() {
            break;
        }
    }
    result
}

/// Refuse a held value in a field a given-up part licenses absent (#215).
///
/// A named field must be absent, null, empty, or an empty string in `after`;
/// a value read from half the part's bytes still fails here. A record `after`
/// does not hold needs no field allowance.
fn verify_absent_fields(
    after: &Value,
    absent: &BTreeSet<(String, String)>,
) -> Result<(), String> {
    if absent.is_empty() {
        return Ok(());
    }
    let present = Document::read(after, &BTreeSet::new())?;
    let held = present
        .records
        .iter()
        .map(|record| (record.id.clone(), record))
        .collect::<BTreeMap<_, _>>();
    for (id, field) in absent {
        let Some(record) = held.get(id) else {
            continue;
        };
        let held_value = record.value.get(field);
        let absent = match held_value {
            None | Some(Value::Null) => true,
            Some(Value::Array(values)) => values.is_empty(),
            Some(Value::String(text)) => text.is_empty(),
            Some(_) => false,
        };
        if !absent {
            return Err(format!(
                "{} {id}: field {field} holds a value although a record.unreadable loss names it absent",
                record.arena,
            ));
        }
    }
    Ok(())
}

fn agree_once_pruned(
    before: &Value,
    after: &Value,
    permitted: &BTreeSet<String>,
    fields: &BTreeSet<(String, String)>,
    cross: bool,
) -> Result<(), String> {
    let unpruned = agree_once(before, after, fields, &BTreeSet::new(), None, cross);
    if let Ok(()) = &unpruned {
        return unpruned;
    }
    let unpaired = pairing_gaps(before, after, fields, cross)?;
    let removed = unpaired
        .intersection(permitted)
        .cloned()
        .collect::<BTreeSet<_>>();
    agree_once(before, after, fields, &removed, Some(&removed), cross)
}

/// The records of `before` that `after` does not pair, with no pruning.
fn pairing_gaps(
    before: &Value,
    after: &Value,
    fields: &BTreeSet<(String, String)>,
    cross: bool,
) -> Result<BTreeSet<String>, String> {
    unpaired_only(&without_fields(before, fields), after, cross)
}

fn unpaired_only(before: &Value, after: &Value, cross: bool) -> Result<BTreeSet<String>, String> {
    let before = Document::read(before, &BTreeSet::new())?;
    let after = Document::read(after, &BTreeSet::new())?;
    let pairs = pair(&before, &after, cross);
    Ok(before
        .records
        .iter()
        .enumerate()
        .filter(|(index, _)| !pairs.contains_key(index))
        .map(|(_, record)| record.id.clone())
        .collect())
}

/// The records of each document that the pairing leaves unpaired, with no
/// pruning. The cross-closure root-synthesis normalization reads the
/// after-side set to tell added roots from reparented ones.
pub(crate) fn unpaired_both(
    before: &Value,
    after: &Value,
    cross: bool,
) -> Result<(BTreeSet<String>, BTreeSet<String>), String> {
    let before = Document::read(before, &BTreeSet::new())?;
    let after = Document::read(after, &BTreeSet::new())?;
    let pairs = pair(&before, &after, cross);
    let used: BTreeSet<usize> = pairs.values().copied().collect();
    Ok((
        before
            .records
            .iter()
            .enumerate()
            .filter(|(index, _)| !pairs.contains_key(index))
            .map(|(_, record)| record.id.clone())
            .collect(),
        after
            .records
            .iter()
            .enumerate()
            .filter(|(index, _)| !used.contains(index))
            .map(|(_, record)| record.id.clone())
            .collect(),
    ))
}

/// Colour pairing, then greedy agreement pairing, in arena order.
///
/// The greedy pass compares with the cross relation in `Cross` mode so that
/// an `after`-only key does not block pairing.
fn pair(before: &Document, after: &Document, cross: bool) -> BTreeMap<usize, usize> {
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
                && agree_shapes(
                    &unranked_shape(&record.value, &before.ids),
                    &unranked_shape(&row.value, &after.ids),
                    cross,
                )
                .is_ok()
        }) {
            pairs.insert(index, other);
            used.insert(other);
        }
    }
    pairs
}

/// The unranked record comparison of the active mode.
fn agree_shapes(left: &Value, right: &Value, cross: bool) -> Result<(), String> {
    if cross {
        cross::shapes_agree(left, right)
    } else {
        cadmpeg_ir::compare::values_agree(left, right)
    }
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
    cross: bool,
) -> Result<(), String> {
    let before = without_fields(before, fields);
    let mut before = Document::read(&before, pruned)?;
    let mut after = Document::read(after, &BTreeSet::new())?;
    let pairs = pair(&before, &after, cross);
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
        if cross {
            cross::values_agree(&record.value, &mapped)
        } else {
            cadmpeg_ir::compare::values_agree(&record.value, &mapped)
        }
        .map_err(|error| format!("{} {}: {error}", record.arena, record.id))?;
    }
    Ok(())
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

/// `document` with no native namespaces and no projected record keys.
fn project(document: &Value, projection: &CrossProjection) -> Value {
    let mut document = document.clone();
    if let Some(object) = document.as_object_mut() {
        object.insert("native".to_owned(), Value::Object(serde_json::Map::new()));
    }
    if projection.dropped_keys.is_empty() {
        return document;
    }
    if let Some(model) = document.get_mut("model").and_then(Value::as_object_mut) {
        for rows in model.values_mut() {
            for row in rows.as_array_mut().into_iter().flatten() {
                if let Some(record) = row.as_object_mut() {
                    record.retain(|key, _| key == "id" || !projection.dropped_keys.contains(key));
                }
            }
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
