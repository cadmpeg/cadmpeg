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
pub(crate) mod graph;
mod pairing;

use graph::Document;
use pairing::pair;

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
    /// Native records the writer regenerated; an edit comparison's expected
    /// native state is stale, so content differences are allowed and identity
    /// maps by id without cascading dependent removal.
    regenerated: BTreeSet<String>,
    /// The export declared scope expansion (e.g. damaged input written as a
    /// valid model archive); scope differences are allowed.
    scope_expanded: bool,
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
            regenerated: BTreeSet::new(),
            scope_expanded: false,
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

    /// Allow additional record identities to be unpaired in `before`. Used by
    /// edit closure when a writer propagates model edits into native carrier
    /// records that the expected JSON does not reflect.
    pub fn permit_removed(&mut self, ids: impl IntoIterator<Item = String>) {
        self.removed.extend(ids);
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
            let after = project(after, projection);
            pairing_gaps(&before, &after, &BTreeSet::new(), true)
        }
    }
}

pub(crate) fn unpaired_after_allowances(
    before: &Value,
    after: &Value,
    allowances: &Allowances,
) -> Result<BTreeSet<String>, String> {
    let full = unpaired_only(before, after, false)?;
    if full.is_empty() {
        return Ok(full);
    }
    let fields = allowances
        .removed_fields
        .iter()
        .filter(|(id, _)| full.contains(id))
        .cloned()
        .collect();
    let before = without_fields(before, &fields);
    let original = unpaired_only(&before, after, false)?;
    let removed: BTreeSet<_> = original
        .intersection(&allowances.removed)
        .cloned()
        .collect();
    let source = Document::read(&before, &removed)?;
    let target = Document::read(after, &BTreeSet::new())?;
    let pairs = pair(&source, &target, false, true);
    let mut unpaired = removed;
    // A regenerated record need not pair, as in `agree_pairing`.
    unpaired.extend(
        source
            .records
            .iter()
            .enumerate()
            .filter(|(index, record)| {
                !pairs.contains_key(index) && !allowances.regenerated.contains(&record.id)
            })
            .map(|(_, record)| record.id.clone()),
    );
    Ok(unpaired)
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
            let mut original = header(before)?;
            let mut recovered = header(after)?;
            for (record, field) in &allowances.absent_fields {
                if record == cadmpeg_ir::report::loss::DOCUMENT_HEADER {
                    let path = field.split('.').collect::<Vec<_>>();
                    let (key, parents) = path.split_last().ok_or("empty header field")?;
                    let recovered_parent = header_parent(&mut recovered, parents)
                        .and_then(Value::as_object_mut)
                        .ok_or("given-up header field has no parent")?;
                    if !matches!(recovered_parent.get(*key), None | Some(Value::Null)) {
                        return Err(format!("header field {field} holds a value although a record.unreadable loss names it absent"));
                    }
                    let original_parent = header_parent(&mut original, parents)
                        .and_then(Value::as_object_mut)
                        .ok_or("given-up header field has no original parent")?;
                    if original_parent.remove(*key).is_none() {
                        return Err("given-up header field has no original value".into());
                    }
                    recovered_parent.remove(*key);
                }
            }
            if allowances.scope_expanded {
                if let (Some(o), Some(r)) = (
                    original.get_mut("scope").and_then(Value::as_object_mut),
                    recovered.get("scope"),
                ) {
                    if o.get("model") == Some(&Value::Bool(false))
                        && r.get("model") == Some(&Value::Bool(true))
                    {
                        o.insert("model".to_owned(), Value::Bool(true));
                    }
                }
            }
            cadmpeg_ir::compare::values_agree(&original, &recovered)?;
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

fn header_parent<'a>(value: &'a mut Value, path: &[&str]) -> Option<&'a mut Value> {
    path.iter()
        .try_fold(value, |value, key| value.get_mut(*key))
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
    let mut before = without_fields(before, &allowances.absent_fields);
    let after = without_fields(after, &allowances.absent_fields);
    if allowances.scope_expanded {
        if let (Some(b), Some(a)) = (
            before.get_mut("scope").and_then(Value::as_object_mut),
            after.get("scope"),
        ) {
            if b.get("model") == Some(&Value::Bool(false))
                && a.get("model") == Some(&Value::Bool(true))
            {
                b.insert("model".to_owned(), Value::Bool(true));
            }
        }
    }
    let mut result = Err(String::new());
    for fields in [&BTreeSet::new(), &allowances.removed_fields] {
        result = agree_once_pruned(
            &before,
            &after,
            &allowances.removed,
            &allowances.regenerated,
            fields,
            cross,
        );
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
fn verify_absent_fields(after: &Value, absent: &BTreeSet<(String, String)>) -> Result<(), String> {
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
    regenerated: &BTreeSet<String>,
    fields: &BTreeSet<(String, String)>,
    cross: bool,
) -> Result<(), String> {
    let unpruned = agree_once(
        before,
        after,
        fields,
        &BTreeSet::new(),
        None,
        regenerated,
        cross,
    );
    if let Ok(()) = &unpruned {
        return unpruned;
    }
    let unpaired = pairing_gaps(before, after, fields, cross)?;
    let mut removed = unpaired
        .intersection(permitted)
        .cloned()
        .collect::<BTreeSet<_>>();
    loop {
        match agree_once(
            before,
            after,
            fields,
            &removed,
            Some(&removed),
            regenerated,
            cross,
        ) {
            Ok(()) => return Ok(()),
            Err(error) => {
                if let Some(id) = error
                    .split(": required record ")
                    .nth(1)
                    .and_then(|rest| rest.strip_suffix(" is missing"))
                {
                    if permitted.contains(id) && removed.insert(id.to_owned()) {
                        continue;
                    }
                }
                return Err(error);
            }
        }
    }
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
    let pairs = pair(&before, &after, cross, true);
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
    let pairs = pair(&before, &after, cross, true);
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

/// One comparison: `pruned` records lose their list entries in `before`, and
/// every unpaired record of `before` must lie in `permitted`; `None` permits
/// no unpaired record.
fn agree_once(
    before: &Value,
    after: &Value,
    fields: &BTreeSet<(String, String)>,
    pruned: &BTreeSet<String>,
    permitted: Option<&BTreeSet<String>>,
    regenerated: &BTreeSet<String>,
    cross: bool,
) -> Result<(), String> {
    let source = without_fields(before, fields);
    let before = Document::read(&source, pruned)?;
    let after = cross_additions_pruned(&source, &before, after, pruned, cross)?;
    if agree_pairing(
        before.clone(),
        after.clone(),
        permitted,
        regenerated,
        cross,
        false,
    )
    .is_ok()
    {
        return Ok(());
    }
    agree_pairing(before, after, permitted, regenerated, cross, true)
}

/// The re-decode read for comparison. In cross mode, a source record the
/// comparison removes loses its list entries, and so does each re-decode
/// record of the same arenas that pairs with no source record: the record a
/// writer wrote in place of a degraded one is an addition (§5.2).
fn cross_additions_pruned(
    source: &Value,
    before: &Document,
    after: &Value,
    pruned: &BTreeSet<String>,
    cross: bool,
) -> Result<Document, String> {
    let read = Document::read(after, &BTreeSet::new())?;
    if !cross || pruned.is_empty() {
        return Ok(read);
    }
    let arenas = Document::read(source, &BTreeSet::new())?
        .records
        .iter()
        .filter(|record| pruned.contains(&record.id))
        .map(|record| record.arena.clone())
        .collect::<BTreeSet<_>>();
    let used = pair(before, &read, cross, true)
        .into_values()
        .collect::<BTreeSet<_>>();
    let added = read
        .records
        .iter()
        .enumerate()
        .filter(|(index, record)| !used.contains(index) && arenas.contains(&record.arena))
        .map(|(_, record)| record.id.clone())
        .collect::<BTreeSet<_>>();
    if added.is_empty() {
        Ok(read)
    } else {
        Document::read(after, &added)
    }
}

fn agree_pairing(
    mut before: Document,
    mut after: Document,
    permitted: Option<&BTreeSet<String>>,
    regenerated: &BTreeSet<String>,
    cross: bool,
    include_incoming: bool,
) -> Result<(), String> {
    let mut pairs = pair(&before, &after, cross, include_incoming);
    // Regenerated records may not pair when expected native is stale; map by id.
    for (index, record) in before.records.iter().enumerate() {
        if pairs.contains_key(&index) || !regenerated.contains(&record.id) {
            continue;
        }
        if let Some(other) = after
            .records
            .iter()
            .position(|candidate| candidate.id == record.id && candidate.arena == record.arena)
        {
            pairs.insert(index, other);
        }
    }
    // A regenerated record can take the identity its new position implies.
    // One that pairs neither by content nor by identity pairs with the next
    // unpaired record of its arena, in arena order, so the records that name
    // it still resolve through the identity mapping.
    let taken: BTreeSet<usize> = pairs.values().copied().collect();
    let mut free = BTreeMap::<&str, std::collections::VecDeque<usize>>::new();
    for (index, record) in after.records.iter().enumerate() {
        if !taken.contains(&index) {
            free.entry(record.arena.as_str())
                .or_default()
                .push_back(index);
        }
    }
    for (index, record) in before.records.iter().enumerate() {
        if pairs.contains_key(&index) || !regenerated.contains(&record.id) {
            continue;
        }
        if let Some(other) = free
            .get_mut(record.arena.as_str())
            .and_then(std::collections::VecDeque::pop_front)
        {
            pairs.insert(index, other);
        }
    }
    for (index, record) in before.records.iter().enumerate() {
        if pairs.contains_key(&index) {
            continue;
        }
        if regenerated.contains(&record.id) {
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
        if regenerated.contains(&record.id) {
            continue;
        }
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
