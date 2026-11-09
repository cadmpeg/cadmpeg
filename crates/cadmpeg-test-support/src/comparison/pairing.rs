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
            let aligned =
                super::cross::aligned(source, target).unwrap_or_else(|| source.iter().collect());
            aligned
                .into_iter()
                .zip(target)
                .all(|(a, b)| references_agree_with_pairs(a, b, before, after, pairs, owners))
        }
        _ => true,
    }
}

/// A reference into a record from one of its owners.
#[derive(Clone)]
struct Incoming {
    owner: usize,
    /// The path to the reference, counting list indices over every item.
    path: Vec<String>,
    /// The path, counting list indices over the items that are not nulled.
    compact: Vec<String>,
    /// The item count of the innermost list that holds the reference, over
    /// every item and over the items that are not nulled.
    lengths: Option<(usize, usize)>,
}

impl Incoming {
    /// Whether `other`, a reference from the partner of this owner, sits
    /// where this one does. A writer replaces a nulled item in place, which
    /// keeps the list length, or omits it, which shortens the list.
    fn matches(&self, other: &Self) -> bool {
        match (self.lengths, other.lengths) {
            (Some((every, _)), Some((target, _))) if target == every => other.path == self.path,
            (Some((_, kept)), Some((target, _))) if target == kept => other.path == self.compact,
            _ => other.path == self.path,
        }
    }
}

fn incoming_references(document: &Document) -> Vec<Vec<Incoming>> {
    struct Walk<'a> {
        owner: usize,
        path: Vec<String>,
        compact: Vec<String>,
        lengths: Option<(usize, usize)>,
        ids: &'a BTreeMap<String, usize>,
    }
    fn visit(value: &Value, walk: &mut Walk<'_>, incoming: &mut [Vec<Incoming>]) {
        match value {
            Value::String(id) => {
                if let Some(index) = walk.ids.get(id) {
                    incoming[*index].push(Incoming {
                        owner: walk.owner,
                        path: walk.path.clone(),
                        compact: walk.compact.clone(),
                        lengths: walk.lengths,
                    });
                }
            }
            Value::Object(fields) => {
                for (key, child) in fields {
                    walk.path.push(key.clone());
                    walk.compact.push(key.clone());
                    visit(child, walk, incoming);
                    walk.path.pop();
                    walk.compact.pop();
                }
            }
            Value::Array(items) => {
                let kept = items
                    .iter()
                    .filter(|item| !super::cross::nulled(item))
                    .count();
                let outer = walk.lengths.replace((items.len(), kept));
                let mut compact = 0_usize;
                for (index, child) in items.iter().enumerate() {
                    walk.path.push(index.to_string());
                    walk.compact.push(compact.to_string());
                    visit(child, walk, incoming);
                    walk.path.pop();
                    walk.compact.pop();
                    compact += usize::from(!super::cross::nulled(child));
                }
                walk.lengths = outer;
            }
            _ => {}
        }
    }
    let mut incoming = vec![Vec::new(); document.records.len()];
    for (owner, record) in document.records.iter().enumerate() {
        let mut walk = Walk {
            owner,
            path: Vec::new(),
            compact: Vec::new(),
            lengths: None,
            ids: &document.ids,
        };
        visit(&record.value, &mut walk, &mut incoming);
    }
    incoming
}

/// Whether every paired owner of `left` holds `right` where it holds `left`.
fn incoming_agrees_with_pairs(
    left: usize,
    right: usize,
    source: &[Vec<Incoming>],
    target: &[Vec<Incoming>],
    pairs: &BTreeMap<usize, usize>,
) -> bool {
    source[left].iter().all(|reference| {
        pairs.get(&reference.owner).is_none_or(|paired| {
            target[right]
                .iter()
                .any(|other| other.owner == *paired && reference.matches(other))
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
        .map(|record| unranked_shape(record, &before.ids))
        .collect();
    let right_shapes: Vec<_> = after
        .records
        .iter()
        .map(|record| unranked_shape(record, &after.ids))
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
                let available: Vec<usize> = candidate
                    .iter()
                    .copied()
                    .filter(|right| {
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
                    })
                    .collect();
                match available.as_slice() {
                    [right] => claims.entry(*right).or_default().push(left),
                    [_, _, ..] => {
                        // Twins pair in sequence: the earliest record takes
                        // its earliest candidate.
                        let right = available
                            .iter()
                            .copied()
                            .min_by_key(|right| (rank(&after.records[*right]), *right))
                            .unwrap_or(available[0]);
                        let key = (rank(&before.records[left]), left);
                        if ambiguous.is_none_or(|(_, _, best)| key < best) {
                            ambiguous = Some((left, right, key));
                        }
                    }
                    [] => {}
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
                let Some((left, right)) =
                    contested.or(ambiguous.map(|(left, right, _)| (left, right)))
                else {
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

/// A record's sequence position, when it states one.
fn rank(record: &graph::Record) -> Option<u64> {
    record
        .value
        .get(graph::rank_key(&record.arena))
        .and_then(Value::as_u64)
}

fn unranked_shape(record: &graph::Record, identities: &BTreeMap<String, usize>) -> Value {
    let mut value = record.value.clone();
    if let Value::Object(fields) = &mut value {
        fields.remove(graph::rank_key(&record.arena));
    }
    graph::shape(&value, identities, false)
}
