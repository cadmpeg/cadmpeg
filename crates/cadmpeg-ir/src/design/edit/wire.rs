// SPDX-License-Identifier: Apache-2.0
//! Walks over a record's wire form: the ids it mentions, the states it reads,
//! and rewrites of its positions, measures, and constructions. Each walk
//! keeps its own stack.

use std::collections::BTreeSet;

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{Map, Value as Json};

use super::{malformed, EditError};
use crate::design::refs::{Member, Position};

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
    let json = serde_json::to_value(record).map_err(malformed)?;
    Ok(reads(&json, is_container))
}

/// Writes a record, lets `edit` change its wire form, and reads it back
/// through its strict reader where `edit` reports a change.
pub(super) fn rewrite<T: Serialize + DeserializeOwned>(
    record: &mut T,
    edit: impl FnOnce(&mut Json) -> Result<bool, EditError>,
) -> Result<bool, EditError> {
    let mut json = serde_json::to_value(&*record).map_err(malformed)?;
    if !edit(&mut json)? {
        return Ok(false);
    }
    *record = serde_json::from_value(json).map_err(malformed)?;
    Ok(true)
}

/// Replaces every `After(removed)` position with `to`, and reports whether
/// it replaced one.
pub(super) fn retarget(
    value: &mut Json,
    removed: &Member,
    to: &Position,
) -> Result<bool, EditError> {
    let from = serde_json::to_value(Position::After(removed.clone())).map_err(malformed)?;
    let to = serde_json::to_value(to).map_err(malformed)?;
    let mut changed = false;
    let mut stack = vec![value];
    while let Some(node) = stack.pop() {
        if *node == from {
            node.clone_from(&to);
            changed = true;
            continue;
        }
        match node {
            Json::Array(items) => stack.extend(items.iter_mut()),
            Json::Object(object) => stack.extend(object.values_mut()),
            Json::Null | Json::Bool(_) | Json::Number(_) | Json::String(_) => {}
        }
    }
    Ok(changed)
}

/// Whether a wire value names `After(member)` anywhere.
pub(super) fn names_after(value: &Json, member: &Member) -> Result<bool, EditError> {
    let from = serde_json::to_value(Position::After(member.clone())).map_err(malformed)?;
    let mut stack = vec![value];
    while let Some(node) = stack.pop() {
        if *node == from {
            return Ok(true);
        }
        match node {
            Json::Array(items) => stack.extend(items),
            Json::Object(object) => stack.extend(object.values()),
            Json::Null | Json::Bool(_) | Json::Number(_) | Json::String(_) => {}
        }
    }
    Ok(false)
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
