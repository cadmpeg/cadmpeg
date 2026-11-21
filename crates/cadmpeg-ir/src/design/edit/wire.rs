// SPDX-License-Identifier: Apache-2.0
//! Walks over a record's wire form: the ids it mentions, the states it reads,
//! and rewrites of its positions, measures, and constructions. Each walk
//! keeps its own stack.

use std::collections::BTreeSet;

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{Map, Value as Json};

use super::{malformed, EditError};
use crate::design::refs::Position;

/// A state a record reads geometry from.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum StateRead {
    /// A fixed state: a `Pinned` anchor, or another stated position.
    At(Position),
    /// The state entering the feature that holds the record (`Input`).
    Input,
    /// The current geometry of the scope (`Current`, or a measure of the
    /// whole `Model`).
    Current,
}

/// The ids a record mentions, as values or map keys, and the states it reads.
#[derive(Debug, Default)]
pub(super) struct Reads {
    pub(super) mentions: BTreeSet<String>,
    pub(super) states: Vec<StateRead>,
}

fn kind(object: &Map<String, Json>) -> Option<&str> {
    object.get("kind").and_then(Json::as_str)
}

fn unit_arm(value: &Json, arm: &str) -> bool {
    value
        .as_object()
        .is_some_and(|object| object.len() == 1 && kind(object) == Some(arm))
}

/// The position an object spells, where it spells one: `After` of a member,
/// or `Start` of a container that `is_container` admits.
fn position(object: &Map<String, Json>, is_container: &dyn Fn(&str) -> bool) -> Option<Position> {
    if object.len() != 2 {
        return None;
    }
    let value = object.get("value")?;
    let admitted = match kind(object)? {
        "after" => value.as_object().is_some_and(|member| {
            member.len() == 2
                && matches!(kind(member), Some("feature" | "container"))
                && member.get("value").is_some_and(Json::is_string)
        }),
        "start" => value.as_str().is_some_and(is_container),
        _ => false,
    };
    if !admitted {
        return None;
    }
    serde_json::from_value(Json::Object(object.clone())).ok()
}

fn is_neutral(object: &Map<String, Json>) -> bool {
    object.len() == 3
        && object.contains_key("text")
        && object.contains_key("refs")
        && object.contains_key("measures")
}

/// Collects the ids a wire value mentions and the states it reads.
pub(super) fn reads(value: &Json, is_container: &dyn Fn(&str) -> bool) -> Reads {
    let mut found = Reads::default();
    let mut stack = vec![value];
    while let Some(node) = stack.pop() {
        match node {
            Json::String(text) => {
                found.mentions.insert(text.clone());
            }
            Json::Array(items) => stack.extend(items),
            Json::Object(object) => {
                if let Some(at) = position(object, is_container) {
                    found.states.push(StateRead::At(at));
                }
                if object.contains_key("picks") {
                    match object.get("at") {
                        Some(anchor) if unit_arm(anchor, "input") => {
                            found.states.push(StateRead::Input);
                        }
                        Some(anchor) if unit_arm(anchor, "current") => {
                            found.states.push(StateRead::Current);
                        }
                        _ => {}
                    }
                }
                if is_neutral(object) {
                    let measures = object.get("measures").and_then(Json::as_array);
                    for measure in measures.into_iter().flatten() {
                        if measure.get("of").is_some_and(|of| unit_arm(of, "model")) {
                            found.states.push(StateRead::Current);
                        }
                    }
                }
                for (key, item) in object {
                    found.mentions.insert(key.clone());
                    stack.push(item);
                }
            }
            Json::Null | Json::Bool(_) | Json::Number(_) => {}
        }
    }
    found
}

/// The ids a record mentions and the states it reads.
pub(super) fn record_reads<T: Serialize>(
    record: &T,
    is_container: &dyn Fn(&str) -> bool,
) -> Result<Reads, EditError> {
    let json = crate::hash::finite_json::to_json_value(record).map_err(malformed)?;
    Ok(reads(&json, is_container))
}

/// Writes a record, lets `edit` change its wire form, and reads it back
/// through its strict reader where `edit` reports a change.
pub(super) fn rewrite<T: Serialize + DeserializeOwned>(
    record: &mut T,
    edit: impl FnOnce(&mut Json) -> Result<bool, EditError>,
) -> Result<bool, EditError> {
    let mut json = crate::hash::finite_json::to_json_value(&*record).map_err(malformed)?;
    if !edit(&mut json)? {
        return Ok(false);
    }
    // Reading text shares the document reader's monomorphized deserializers.
    *record = serde_json::from_str(&json.to_string()).map_err(malformed)?;
    Ok(true)
}

/// The wire forms of positions.
pub(super) fn positions(positions: &[Position]) -> Result<Vec<Json>, EditError> {
    positions
        .iter()
        .map(|position| crate::hash::finite_json::to_json_value(position).map_err(malformed))
        .collect()
}

/// Replaces every position of `from` with `to`, and reports whether it
/// replaced one.
pub(super) fn retarget(value: &mut Json, from: &[Json], to: &Json) -> bool {
    let mut changed = false;
    let mut stack = vec![value];
    while let Some(node) = stack.pop() {
        if from.contains(node) {
            node.clone_from(to);
            changed = true;
            continue;
        }
        match node {
            Json::Array(items) => stack.extend(items.iter_mut()),
            Json::Object(object) => stack.extend(object.values_mut()),
            Json::Null | Json::Bool(_) | Json::Number(_) | Json::String(_) => {}
        }
    }
    changed
}

/// Re-anchors every selection pinned to a position of `from` to `Current`
/// (§5.8), and reports whether it re-anchored one.
pub(super) fn re_anchor(value: &mut Json, from: &[Json]) -> bool {
    let mut changed = false;
    let mut stack = vec![value];
    while let Some(node) = stack.pop() {
        if let Json::Object(object) = node {
            let pinned = object.len() == 2
                && kind(object) == Some("pinned")
                && object.get("value").is_some_and(|at| from.contains(at));
            if pinned {
                *node = serde_json::json!({ "kind": "current" });
                changed = true;
                continue;
            }
        }
        match node {
            Json::Array(items) => stack.extend(items.iter_mut()),
            Json::Object(object) => stack.extend(object.values_mut()),
            Json::Null | Json::Bool(_) | Json::Number(_) | Json::String(_) => {}
        }
    }
    changed
}

/// Every string a wire value holds, map keys included.
fn strings(value: &Json) -> Vec<&str> {
    let mut found = Vec::new();
    let mut stack = vec![value];
    while let Some(node) = stack.pop() {
        match node {
            Json::String(text) => found.push(text.as_str()),
            Json::Array(items) => stack.extend(items),
            Json::Object(object) => {
                for (key, item) in object {
                    found.push(key.as_str());
                    stack.push(item);
                }
            }
            Json::Null | Json::Bool(_) | Json::Number(_) => {}
        }
    }
    found
}

/// Removes the `resolved` cache of every pick that lists a record of
/// `gone` (§5.8), and reports whether it removed one.
pub(super) fn clear_resolved(value: &mut Json, gone: &BTreeSet<String>) -> bool {
    let mut changed = false;
    let mut stack = vec![value];
    while let Some(node) = stack.pop() {
        match node {
            Json::Array(items) => stack.extend(items.iter_mut()),
            Json::Object(object) => {
                let stale = object.contains_key("target")
                    && object.get("resolved").is_some_and(|resolved| {
                        resolved.is_array() && strings(resolved).iter().any(|id| gone.contains(*id))
                    });
                if stale {
                    object.remove("resolved");
                    changed = true;
                }
                stack.extend(object.values_mut());
            }
            Json::Null | Json::Bool(_) | Json::Number(_) | Json::String(_) => {}
        }
    }
    changed
}

/// Removes the `resolved` of every `Transformation` one of whose steps names
/// a parameter of `bound` (§6.5), and reports whether it removed one.
pub(super) fn clear_transformations(value: &mut Json, bound: &BTreeSet<String>) -> bool {
    let mut changed = false;
    let mut stack = vec![value];
    while let Some(node) = stack.pop() {
        match node {
            Json::Array(items) => stack.extend(items.iter_mut()),
            Json::Object(object) => {
                let transformation = object.get("steps").is_some_and(Json::is_array)
                    && object.contains_key("resolved")
                    && object.keys().all(|key| key == "steps" || key == "resolved");
                if transformation
                    && object
                        .get("steps")
                        .is_some_and(|steps| strings(steps).iter().any(|id| bound.contains(*id)))
                {
                    object.remove("resolved");
                    changed = true;
                }
                stack.extend(object.values_mut());
            }
            Json::Null | Json::Bool(_) | Json::Number(_) | Json::String(_) => {}
        }
    }
    changed
}

/// One target of a model-class record (§8): the ids of the entities it
/// names, and the state it names them in, `None` for the current geometry.
#[derive(Debug)]
pub(super) struct Target {
    pub(super) ids: Vec<String>,
    pub(super) state: Option<Json>,
}

impl Target {
    /// Whether the target lies in the deleted extent, whose records are
    /// `lost`, or in a deleted evaluation, whose `StateRef`s are `states`.
    pub(super) fn deleted(&self, lost: &BTreeSet<String>, states: &[Json]) -> bool {
        match &self.state {
            Some(state) => states.contains(state),
            None => !self.ids.is_empty() && self.ids.iter().all(|id| lost.contains(id)),
        }
    }
}

/// The entity ids a wire value holds: its strings of the id grammar
/// `<format>:<scope>:<kind>#<key>` (§2.2), not its tags and field names.
fn entity_ids(value: &Json) -> Vec<String> {
    strings(value)
        .into_iter()
        .filter(|text| {
            text.split_once('#')
                .is_some_and(|(space, key)| !key.is_empty() && space.matches(':').count() == 2)
        })
        .map(str::to_owned)
        .collect()
}

/// The instance target an object spells (`entity`, with `path` and `state`
/// where stated), or the declared target of a declared measure (`Bodies`, or
/// `State`).
pub(super) fn target(object: &Map<String, Json>) -> Option<Target> {
    let instance = object.contains_key("entity")
        && object
            .keys()
            .all(|key| matches!(key.as_str(), "entity" | "path" | "state"));
    if instance {
        return Some(Target {
            ids: entity_ids(object.get("entity")?),
            state: object.get("state").cloned(),
        });
    }
    match kind(object)? {
        "bodies" if object.len() == 2 => Some(Target {
            ids: entity_ids(object.get("bodies")?),
            state: None,
        }),
        "state"
            if object.contains_key("of")
                && object
                    .keys()
                    .all(|key| matches!(key.as_str(), "kind" | "configuration" | "of")) =>
        {
            let mut state = object.clone();
            state.remove("kind");
            Some(Target {
                ids: Vec::new(),
                state: Some(Json::Object(state)),
            })
        }
        _ => None,
    }
}

/// Every target a model-class record states.
pub(super) fn targets(record: &Json) -> Vec<Target> {
    let mut found = Vec::new();
    let mut stack = vec![record];
    while let Some(node) = stack.pop() {
        match node {
            Json::Array(items) => stack.extend(items),
            Json::Object(object) => match target(object) {
                Some(target) => found.push(target),
                None => stack.extend(object.values()),
            },
            Json::Null | Json::Bool(_) | Json::Number(_) | Json::String(_) => {}
        }
    }
    found
}

/// Whether a kept record's wire form names a record of `gone` by id, or a
/// record of `lost`, the deleted part of the current geometry, outside a
/// target into another state. A pick by an `Entity` name stays where its
/// name mentions a deleted record (D51), and a witness holds no reference,
/// so neither is read.
pub(super) fn names_deleted(
    value: &Json,
    gone: &BTreeSet<String>,
    lost: &BTreeSet<String>,
) -> bool {
    let mut stack = vec![(value, false)];
    while let Some((node, elsewhere)) = stack.pop() {
        match node {
            Json::String(text) => {
                if gone.contains(text) || (!elsewhere && lost.contains(text)) {
                    return true;
                }
            }
            Json::Array(items) => stack.extend(items.iter().map(|item| (item, elsewhere))),
            Json::Object(object) => {
                let elsewhere =
                    elsewhere || object.get("state").is_some_and(|state| !state.is_null());
                for (key, item) in object {
                    if gone.contains(key) || (!elsewhere && lost.contains(key)) {
                        return true;
                    }
                    let named = key == "target"
                        && item
                            .as_object()
                            .is_some_and(|target| kind(target) == Some("entity"));
                    if named || key == "witness" {
                        continue;
                    }
                    stack.push((item, elsewhere));
                }
            }
            Json::Null | Json::Bool(_) | Json::Number(_) => {}
        }
    }
    false
}

/// Removes `cached` from every measure of every neutral expression for which
/// `stale` holds, and reports whether it removed one.
pub(super) fn clear_measures(value: &mut Json, stale: &mut dyn FnMut(&Json) -> bool) -> bool {
    let mut changed = false;
    let mut stack = vec![value];
    while let Some(node) = stack.pop() {
        match node {
            Json::Array(items) => stack.extend(items.iter_mut()),
            Json::Object(object) => {
                if is_neutral(object) {
                    let measures = object.get_mut("measures").and_then(Json::as_array_mut);
                    for measure in measures.into_iter().flatten() {
                        if measure.get("cached").is_some() && stale(measure) {
                            if let Some(fields) = measure.as_object_mut() {
                                fields.remove("cached");
                                changed = true;
                            }
                        }
                    }
                    continue;
                }
                stack.extend(object.values_mut());
            }
            Json::Null | Json::Bool(_) | Json::Number(_) | Json::String(_) => {}
        }
    }
    changed
}

fn is_construction(object: &Map<String, Json>) -> bool {
    object.get("inputs").is_some_and(Json::is_array)
        && object.get("after").is_some_and(Json::is_array)
        && object.get("resolved_in").is_some_and(Json::is_object)
}

/// For every construction whose `resolved` is a cache (one with inputs or
/// steps), moves `resolved` into `resolved_in` under `outgoing`, and takes
/// `incoming`'s entry as `resolved`, or no `resolved` where it has none
/// (§5.9). Reports whether it changed one.
pub(super) fn exchange_constructions(value: &mut Json, outgoing: &str, incoming: &str) -> bool {
    let mut changed = false;
    let mut stack = vec![value];
    while let Some(node) = stack.pop() {
        match node {
            Json::Array(items) => stack.extend(items.iter_mut()),
            Json::Object(object) => {
                let cached = is_construction(object)
                    && ["inputs", "after"].iter().any(|key| {
                        object
                            .get(*key)
                            .and_then(Json::as_array)
                            .is_some_and(|items| !items.is_empty())
                    });
                if cached {
                    let previous = object.remove("resolved");
                    if let Some(Json::Object(by_configuration)) = object.get_mut("resolved_in") {
                        let next = by_configuration.remove(incoming);
                        match previous {
                            Some(value) => {
                                by_configuration.insert(outgoing.to_owned(), value);
                            }
                            None => {
                                by_configuration.remove(outgoing);
                            }
                        }
                        if let Some(value) = next {
                            object.insert("resolved".to_owned(), value);
                        }
                    }
                    changed = true;
                }
                stack.extend(object.values_mut());
            }
            Json::Null | Json::Bool(_) | Json::Number(_) | Json::String(_) => {}
        }
    }
    changed
}
