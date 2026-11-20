// SPDX-License-Identifier: Apache-2.0
//! Probes of a value's own spelling, beside the keys of a struct position:
//! the name of an enum whose variants hold no data stated as the one-key
//! object `{"<name>": null}`, and an array that states each member once (a
//! set, a pair-map, or a member list) stated with two members swapped.
//!
//! A swapped pair is a second spelling when the readers accept it and write
//! the array back in another order. A set and a pair-map refuse it; a member
//! list in source order writes it back as stated.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use super::generate::{value_at, value_at_mut, Known, Sample};
use super::schema::{
    definition, enum_names, is_definition, is_name_enum, reference, types, Schema,
};
use super::{Read, SchemaSweepReport};
use crate::spelling::first_difference;
use crate::unknown_keys::Step;

/// The pairs of candidate members one array tries for a sample that holds
/// two of them.
const PAIR_TRIES: usize = 24;

/// Reads the sample of every data-free enum definition with its name stated
/// as `{"<name>": null}`.
pub(super) fn probe_names(
    schema: &Schema<'_>,
    samples: &BTreeMap<String, Sample>,
    read: Read<'_>,
    report: &mut SchemaSweepReport,
) {
    for (location, sample) in samples {
        if !is_definition(location) || !is_name_enum(schema.node(location)) {
            continue;
        }
        let Some(Value::String(name)) = sample.value() else {
            report
                .unprobed
                .push(format!("#{location}: the sample states no name"));
            continue;
        };
        let object = Value::Object(Map::from_iter([(name.clone(), Value::Null)]));
        let mut document = sample.document.clone();
        if let Some(value) = value_at_mut(&mut document, &sample.path) {
            *value = object;
        }
        report.names += 1;
        report.reads += 1;
        if read(&document.to_string()).is_ok() {
            report.accepting.insert(format!(
                "#{location}: `{{\"{name}\": null}}` reads as `{name}`"
            ));
        }
    }
}

/// Reads every array position that states each member once with its first
/// two members swapped.
pub(super) fn probe_orders(
    schema: &Schema<'_>,
    samples: &BTreeMap<String, Sample>,
    known: &Known,
    read: Read<'_>,
    report: &mut SchemaSweepReport,
) {
    let locations = schema.distinct_arrays();
    let mut instances = BTreeMap::<String, Vec<(&Sample, Vec<Step>)>>::new();
    for sample in samples.values() {
        schema.walk_where(
            "",
            &sample.document,
            &mut Vec::new(),
            &[],
            &|location| locations.contains(location),
            &mut |location, path, _| {
                instances
                    .entry(location.to_owned())
                    .or_default()
                    .push((sample, path.to_vec()));
            },
        );
    }
    for location in &locations {
        let Some(found) = instances.get(location) else {
            report
                .unprobed
                .push(format!("#{location}: no document reaches it"));
            continue;
        };
        let Some((document, path)) = two_members(schema, location, found, known, read, report)
        else {
            report
                .unprobed
                .push(format!("#{location}: no document holds two of its members"));
            continue;
        };
        let mut swapped = document;
        if let Some(Value::Array(items)) = value_at_mut(&mut swapped, &path) {
            items.swap(0, 1);
        }
        report.reads += 1;
        match read(&swapped.to_string()) {
            Err(_) => report.sets += 1,
            Ok(written) => match (value_at(&swapped, &path), value_at(&written, &path)) {
                (Some(stated), Some(written)) if first_difference(stated, written).is_none() => {
                    report.ordered += 1;
                }
                _ => {
                    report.accepting.insert(format!(
                        "#{location}: two members out of order are read and written back sorted"
                    ));
                }
            },
        }
    }
}

/// An accepted document whose array at the returned path holds two members,
/// in the order the readers write them: an instance that holds two, or an
/// instance whose array is replaced by two candidate members.
fn two_members(
    schema: &Schema<'_>,
    location: &str,
    found: &[(&Sample, Vec<Step>)],
    known: &Known,
    read: Read<'_>,
    report: &mut SchemaSweepReport,
) -> Option<(Value, Vec<Step>)> {
    let held = |sample: &Sample, path: &[Step]| {
        value_at(&sample.document, path)
            .and_then(Value::as_array)
            .map_or(0, Vec::len)
    };
    if let Some((sample, path)) = found.iter().find(|(sample, path)| held(sample, path) >= 2) {
        return Some((sample.document.clone(), path.clone()));
    }
    let (sample, path) = found
        .iter()
        .min_by_key(|(sample, _)| sample.document.to_string().len())?;
    let candidates = candidates(schema, location, found, known);
    let mut tries = 0;
    for (index, first) in candidates.iter().enumerate() {
        for second in &candidates[index + 1..] {
            if tries == PAIR_TRIES {
                return None;
            }
            tries += 1;
            for pair in [[first, second], [second, first]] {
                let mut document = sample.document.clone();
                *value_at_mut(&mut document, path)? =
                    Value::Array(Vec::from(pair.map(Clone::clone)));
                report.reads += 1;
                let Ok(written) = read(&document.to_string()) else {
                    continue;
                };
                let Some(written @ Value::Array(items)) = value_at(&written, path) else {
                    continue;
                };
                if items.len() == 2 {
                    if let Some(array) = value_at_mut(&mut document, path) {
                        *array = written.clone();
                    }
                    return Some((document, path.clone()));
                }
            }
        }
    }
    None
}

/// The members an array at `location` can hold: those its instances hold,
/// and others of its member type; for a pair, each instance's pair under
/// other keys of its key type.
fn candidates(
    schema: &Schema<'_>,
    location: &str,
    found: &[(&Sample, Vec<Step>)],
    known: &Known,
) -> Vec<Value> {
    let held = found
        .iter()
        .filter_map(|(sample, path)| value_at(&sample.document, path))
        .filter_map(Value::as_array)
        .flatten()
        .cloned()
        .collect::<Vec<_>>();
    let mut found = held.clone();
    let member = format!("{location}/items");
    found.extend(others(schema, &member, &held, known));
    let key = format!("{member}/prefixItems/0");
    if schema.has_node(&key) {
        let keys = held
            .iter()
            .filter_map(|pair| pair.get(0).cloned())
            .collect::<Vec<_>>();
        for key in others(schema, &key, &keys, known) {
            for pair in &held {
                if let Some([_, value]) = pair.as_array().map(Vec::as_slice) {
                    found.push(Value::Array(vec![key.clone(), value.clone()]));
                }
            }
        }
    }
    let mut seen = BTreeSet::new();
    found.retain(|value| seen.insert(value.to_string()));
    found
}

/// Values of the node at `location` beside `held`: those known for it,
/// every name of a data-free enum, and the integer after each held one.
fn others(schema: &Schema<'_>, location: &str, held: &[Value], known: &Known) -> Vec<Value> {
    let node = schema.node(location);
    let at = reference(node).map_or_else(|| location.to_owned(), definition);
    let mut found = known.get(&at).cloned().unwrap_or_default();
    let target = schema.node(&at);
    if is_name_enum(target) {
        found.extend(enum_names(target));
    }
    if types(target).contains(&"integer") {
        found.extend(
            held.iter()
                .filter_map(Value::as_u64)
                .filter_map(|value| value.checked_add(1))
                .map(Value::from),
        );
    }
    found
}
