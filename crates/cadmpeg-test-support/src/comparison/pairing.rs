// SPDX-License-Identifier: Apache-2.0
//! Graph-aware matching for source and target records.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde_json::Value;

use super::{
    cross,
    graph::{self, Document},
};

fn collect_field_paths(value: &Value, path: &mut Vec<String>, paths: &mut BTreeSet<Vec<String>>) {
    match value {
        Value::Object(fields) => {
            for (key, child) in fields {
                path.push(key.clone());
                paths.insert(path.clone());
                collect_field_paths(child, path, paths);
                path.pop();
            }
        }
        Value::Array(items) => {
            path.push("[]".to_owned());
            for child in items {
                collect_field_paths(child, path, paths);
            }
            path.pop();
        }
        _ => {}
    }
}

fn retain_field_paths(value: &mut Value, path: &mut Vec<String>, paths: &BTreeSet<Vec<String>>) {
    match value {
        Value::Object(fields) => fields.retain(|key, child| {
            path.push(key.clone());
            let keep = paths.contains(path);
            if keep {
                retain_field_paths(child, path, paths);
            }
            path.pop();
            keep
        }),
        Value::Array(items) => {
            path.push("[]".to_owned());
            for child in items {
                retain_field_paths(child, path, paths);
            }
            path.pop();
        }
        _ => {}
    }
}

fn refinement_target(before: &Document, after: &Document) -> Document {
    let mut paths = BTreeMap::<String, BTreeSet<Vec<String>>>::new();
    for record in &before.records {
        collect_field_paths(
            &record.value,
            &mut Vec::new(),
            paths.entry(record.arena.clone()).or_default(),
        );
    }
    let mut target = after.clone();
    for record in &mut target.records {
        if let Some(paths) = paths.get(&record.arena) {
            retain_field_paths(&mut record.value, &mut Vec::new(), paths);
        }
    }
    target
}

fn references_agree_with_pairs(
    left: &Value,
    right: &Value,
    before: &Document,
    after: &Document,
    pairs: &BTreeMap<usize, usize>,
    owners: &BTreeMap<usize, usize>,
) -> bool {
    match (left, right) {
        (Value::String(source), Value::String(target)) => {
            let Some(&source) = before.ids.get(source) else {
                return true;
            };
            let Some(&target) = after.ids.get(target) else {
                return false;
            };
            pairs.get(&source).is_none_or(|paired| *paired == target)
                && owners.get(&target).is_none_or(|owner| *owner == source)
        }
        (Value::Object(source), Value::Object(target)) => source.iter().all(|(key, value)| {
            before.ids.contains_key(key)
                || target.get(key).is_none_or(|other| {
                    references_agree_with_pairs(value, other, before, after, pairs, owners)
                })
        }),
        (Value::Array(source), Value::Array(target)) => {
            let aligned = super::cross::aligned(source, target)
                .unwrap_or_else(|| source.iter().collect());
            aligned
                .into_iter()
                .zip(target)
                .all(|(a, b)| references_agree_with_pairs(a, b, before, after, pairs, owners))
        }
        _ => true,
    }
}

/// A reference into a record: its owner, its path, and its path with the
/// array indices counted over the non-null items only.
type Incoming = (usize, Vec<String>, Vec<String>);

fn incoming_references(document: &Document) -> Vec<Vec<Incoming>> {
    fn visit(
        value: &Value,
        owner: usize,
        paths: &mut (Vec<String>, Vec<String>),
        ids: &BTreeMap<String, usize>,
        incoming: &mut [Vec<Incoming>],
    ) {
        match value {
            Value::String(id) => {
                if let Some(index) = ids.get(id) {
                    incoming[*index].push((owner, paths.0.clone(), paths.1.clone()));
                }
            }
            Value::Object(fields) => {
                for (key, child) in fields {
                    paths.0.push(key.clone());
                    paths.1.push(key.clone());
                    visit(child, owner, paths, ids, incoming);
                    paths.0.pop();
                    paths.1.pop();
                }
            }
            Value::Array(items) => {
                let mut compact = 0_usize;
                for (index, child) in items.iter().enumerate() {
                    paths.0.push(index.to_string());
                    paths.1.push(compact.to_string());
                    visit(child, owner, paths, ids, incoming);
                    paths.0.pop();
                    paths.1.pop();
                    compact += usize::from(!child.is_null());
                }
            }
            _ => {}
        }
    }
    let mut incoming = vec![Vec::new(); document.records.len()];
    for (owner, record) in document.records.iter().enumerate() {
        visit(
            &record.value,
            owner,
            &mut (Vec::new(), Vec::new()),
            &document.ids,
            &mut incoming,
        );
    }
    incoming
}

/// Whether every paired owner of `left` holds `right` where it holds `left`.
/// A `left` path may count the indices of its arrays over all items or over
/// the non-null items, since a writer replaces or omits a nulled reference.
fn incoming_agrees_with_pairs(
    left: usize,
    right: usize,
    source: &[Vec<Incoming>],
    target: &[Vec<Incoming>],
    pairs: &BTreeMap<usize, usize>,
) -> bool {
    source[left].iter().all(|(owner, path, compact)| {
        pairs.get(owner).is_none_or(|paired| {
            target[right].iter().any(|(candidate, target_path, _)| {
                candidate == paired && (target_path == path || target_path == compact)
            })
        })
    })
}

/// The first record whose colour class holds more than one record on either
/// side, paired with the first record of that colour on the other side.
///
/// Refinement cannot split twins, such as identical bodies, so pairing each
/// twin record independently can join the records of one twin to those of
/// another. Individualizing one pair and refining again splits the twins
/// consistently.
fn first_ambiguous_match(
    before: &Document,
    after: &Document,
    left: &[String],
    right: &[String],
) -> Option<(usize, usize)> {
    let mut sizes = BTreeMap::<(&str, &str), (usize, usize)>::new();
    for (record, colour) in before.records.iter().zip(left) {
        sizes
            .entry((record.arena.as_str(), colour.as_str()))
            .or_default()
            .0 += 1;
    }
    for (record, colour) in after.records.iter().zip(right) {
        sizes
            .entry((record.arena.as_str(), colour.as_str()))
            .or_default()
            .1 += 1;
    }
    before
        .records
        .iter()
        .zip(left)
        .enumerate()
        .find_map(|(index, (record, colour))| {
            let (lefts, rights) = sizes[&(record.arena.as_str(), colour.as_str())];
            if rights == 0 || (lefts < 2 && rights < 2) {
                return None;
            }
            let other = after
                .records
                .iter()
                .zip(right)
                .position(|(row, other)| row.arena == record.arena && other == colour)?;
            Some((index, other))
        })
}

/// Pair equal graph colours first, then find compatible unmatched records.
pub(super) fn pair(
    before: &Document,
    after: &Document,
    cross: bool,
    include_incoming: bool,
) -> BTreeMap<usize, usize> {
    let refined = cross.then(|| refinement_target(before, after));
    let target = refined.as_ref().unwrap_or(after);
    let mut individual = Vec::new();
    let (before_colours, after_colours) = loop {
        let (left, right) = graph::colours(before, target, include_incoming, &individual);
        match first_ambiguous_match(before, after, &left, &right) {
            Some(pair) => individual.push(pair),
            None => break (left, right),
        }
    };
    let mut pairs = BTreeMap::new();
    let mut used = BTreeSet::new();
    let mut first_owners = BTreeMap::new();
    let incoming_before = incoming_references(before);
    let incoming_after = incoming_references(after);

    for (index, record) in before.records.iter().enumerate() {
        let equal = after
            .records
            .iter()
            .enumerate()
            .filter(|(other, row)| {
                !used.contains(other)
                    && record.arena == row.arena
                    && before_colours[index] == after_colours[*other]
            })
            .map(|(other, _)| other)
            .collect::<Vec<_>>();
        // Structurally identical records tie on colour; the one the already
        // paired referrers and referents select keeps duplicates apart.
        let chosen = equal
            .iter()
            .copied()
            .find(|other| {
                incoming_agrees_with_pairs(index, *other, &incoming_before, &incoming_after, &pairs)
                    && references_agree_with_pairs(
                        &record.value,
                        &after.records[*other].value,
                        before,
                        after,
                        &pairs,
                        &first_owners,
                    )
            })
            .or_else(|| equal.first().copied());
        if let Some(other) = chosen {
            pairs.insert(index, other);
            used.insert(other);
            first_owners.insert(other, index);
        }
    }
    if pairs.len() == before.records.len() {
        return pairs;
    }
    let mut locked: BTreeSet<usize> = pairs.keys().copied().collect();
    let mut owners: BTreeMap<usize, usize> =
        pairs.iter().map(|(left, right)| (*right, *left)).collect();
    let left_shapes: Vec<_> = before
        .records
        .iter()
        .map(|record| unranked_shape(&record.value, &before.ids))
        .collect();
    let right_shapes: Vec<_> = after
        .records
        .iter()
        .map(|record| unranked_shape(&record.value, &after.ids))
        .collect();
    let candidates: Vec<Vec<usize>> = before
        .records
        .iter()
        .enumerate()
        .map(|(left, record)| {
            if locked.contains(&left) {
                return Vec::new();
            }
            after
                .records
                .iter()
                .enumerate()
                .filter_map(|(right, row)| {
                    (record.arena == row.arena
                        && !owners.contains_key(&right)
                        && agree_shapes(&left_shapes[left], &right_shapes[right], cross).is_ok())
                    .then_some(right)
                })
                .collect()
        })
        .collect();
    let nulled: Vec<bool> = before
        .records
        .iter()
        .map(|record| holds_null(&record.value))
        .collect();
    if cross {
        loop {
            let mut claims = BTreeMap::<usize, Vec<usize>>::new();
            let mut ambiguous = None;
            for (left, candidate) in candidates.iter().enumerate() {
                if pairs.contains_key(&left) {
                    continue;
                }
                let mut available = candidate.iter().copied().filter(|right| {
                    !owners.contains_key(right)
                        && references_agree_with_pairs(
                            &before.records[left].value,
                            &after.records[*right].value,
                            before,
                            after,
                            &pairs,
                            &owners,
                        )
                        && incoming_agrees_with_pairs(
                            left,
                            *right,
                            &incoming_before,
                            &incoming_after,
                            &pairs,
                        )
                });
                match (available.next(), available.next()) {
                    (Some(right), None) => claims.entry(right).or_default().push(left),
                    (Some(right), Some(_)) => {
                        ambiguous.get_or_insert((left, right));
                    }
                    _ => {}
                }
            }
            let mut changed = false;
            let mut contested = None;
            for (right, claimants) in claims {
                // A nulled reference agrees with any value, so a record that
                // states every reference outranks one that does not.
                let mut exact = claimants.iter().filter(|left| !nulled[**left]);
                if let (true, Some(left), None) = (claimants.len() > 1, exact.next(), exact.next())
                {
                    contested.get_or_insert((*left, right));
                }
                if let [left] = claimants.as_slice() {
                    if !references_agree_with_pairs(
                        &before.records[*left].value,
                        &after.records[right].value,
                        before,
                        after,
                        &pairs,
                        &owners,
                    ) || !incoming_agrees_with_pairs(
                        *left,
                        right,
                        &incoming_before,
                        &incoming_after,
                        &pairs,
                    ) {
                        continue;
                    }
                    pairs.insert(*left, right);
                    owners.insert(right, *left);
                    locked.insert(*left);
                    changed = true;
                }
            }
            // Twins, such as identical bodies, leave every record with
            // several candidates. Pair the first such record with its first
            // candidate; the references of that pair then single out the
            // partners of its twin's other records.
            if !changed {
                let Some((left, right)) = contested.or(ambiguous) else {
                    break;
                };
                pairs.insert(left, right);
                owners.insert(right, left);
                locked.insert(left);
            }
        }
    }
    let fixed_pairs = pairs.clone();
    let fixed_owners = owners.clone();
    for enforce_references in [true, false]
        .into_iter()
        .filter(|enforce| cross || !*enforce)
    {
        for start in 0..before.records.len() {
            if pairs.contains_key(&start) {
                continue;
            }
            let mut queue = VecDeque::from([start]);
            let mut seen_left = BTreeSet::from([start]);
            let mut parent = BTreeMap::new();
            let mut free = None;
            while let Some(left) = queue.pop_front() {
                for &right in &candidates[left] {
                    if parent.contains_key(&right)
                        || owners
                            .get(&right)
                            .is_some_and(|owner| locked.contains(owner))
                        || (enforce_references
                            && !references_agree_with_pairs(
                                &before.records[left].value,
                                &after.records[right].value,
                                before,
                                after,
                                &fixed_pairs,
                                &fixed_owners,
                            ))
                        || (enforce_references
                            && !incoming_agrees_with_pairs(
                                left,
                                right,
                                &incoming_before,
                                &incoming_after,
                                &fixed_pairs,
                            ))
                    {
                        continue;
                    }
                    parent.insert(right, left);
                    if let Some(owner) = owners.get(&right).copied() {
                        if seen_left.insert(owner) {
                            queue.push_back(owner);
                        }
                    } else {
                        free = Some(right);
                        break;
                    }
                }
                if free.is_some() {
                    break;
                }
            }
            while let Some(right) = free {
                let left = parent[&right];
                free = pairs.insert(left, right);
                owners.insert(right, left);
                if let Some(previous) = free {
                    owners.remove(&previous);
                }
            }
        }
    }
    pairs
}

fn holds_null(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Array(items) => items.iter().any(holds_null),
        Value::Object(fields) => fields.values().any(holds_null),
        _ => false,
    }
}

/// The unranked record comparison of the active mode.
fn agree_shapes(left: &Value, right: &Value, cross: bool) -> Result<(), String> {
    if cross {
        cross::shapes_agree(left, right)
    } else {
        cadmpeg_ir::compare::values_agree(left, right)
    }
}

fn unranked_shape(value: &Value, identities: &BTreeMap<String, usize>) -> Value {
    let mut value = value.clone();
    if let Value::Object(fields) = &mut value {
        fields.remove("ordinal");
    }
    graph::shape(&value, identities, false)
}
