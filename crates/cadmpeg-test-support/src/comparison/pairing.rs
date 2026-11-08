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
        (Value::Array(source), Value::Array(target)) => source
            .iter()
            .zip(target)
            .all(|(a, b)| references_agree_with_pairs(a, b, before, after, pairs, owners)),
        _ => true,
    }
}

fn incoming_references(document: &Document) -> Vec<Vec<(usize, Vec<String>)>> {
    fn visit(
        value: &Value,
        owner: usize,
        path: &mut Vec<String>,
        ids: &BTreeMap<String, usize>,
        incoming: &mut [Vec<(usize, Vec<String>)>],
    ) {
        match value {
            Value::String(id) => {
                if let Some(index) = ids.get(id) {
                    incoming[*index].push((owner, path.clone()));
                }
            }
            Value::Object(fields) => {
                for (key, child) in fields {
                    path.push(key.clone());
                    visit(child, owner, path, ids, incoming);
                    path.pop();
                }
            }
            Value::Array(items) => {
                for (index, child) in items.iter().enumerate() {
                    path.push(index.to_string());
                    visit(child, owner, path, ids, incoming);
                    path.pop();
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
            &mut Vec::new(),
            &document.ids,
            &mut incoming,
        );
    }
    incoming
}

fn incoming_agrees_with_pairs(
    left: usize,
    right: usize,
    source: &[Vec<(usize, Vec<String>)>],
    target: &[Vec<(usize, Vec<String>)>],
    pairs: &BTreeMap<usize, usize>,
) -> bool {
    source[left].iter().all(|(owner, path)| {
        pairs.get(owner).is_none_or(|paired| {
            target[right]
                .iter()
                .any(|(candidate, target_path)| candidate == paired && target_path == path)
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
        sizes.entry((record.arena.as_str(), colour.as_str())).or_default().0 += 1;
    }
    for (record, colour) in after.records.iter().zip(right) {
        sizes.entry((record.arena.as_str(), colour.as_str())).or_default().1 += 1;
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
    let incoming_before = incoming_references(before);
    let incoming_after = incoming_references(after);
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
                if let [first, _, ..] = claimants.as_slice() {
                    contested.get_or_insert((*first, right));
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
            // several candidates, or several records with one candidate.
            // Pair the first such record, in the source order that puts
            // required records first, with its first candidate; the
            // references of that pair then single out the partners of its
            // twin's other records.
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
