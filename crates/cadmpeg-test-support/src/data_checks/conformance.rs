// SPDX-License-Identifier: Apache-2.0
//! The §9.3 cross-codec conformance checks (#219) over serialized decodes.
//!
//! [`b1_unrepresented`] recomputes the §3.1 B1 derivation from the serialized
//! fidelity alone, without the production partition: a stream byte inside a
//! part is B1 when no record layout reads it and no part frames it, and every
//! byte of a given-up part outside its framing is B1.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use cadmpeg_ir::{ArenaClass, ArenaName};
use serde_json::Value;

type Ranges = Vec<Range<u64>>;

const CLASS_NAMES: [&str; 2] = ["model", "design"];

fn range(value: &Value) -> Option<Range<u64>> {
    Some(value.get("start")?.as_u64()?..value.get("end")?.as_u64()?)
}

fn extent(value: &Value) -> Option<(String, Range<u64>)> {
    let stream = value.get("stream")?.as_str()?.to_owned();
    let offset = value.get("offset")?.as_u64()?;
    let length = value.get("byte_len")?.as_u64()?;
    Some((stream, offset..offset.checked_add(length)?))
}

fn merge(mut ranges: Ranges) -> Ranges {
    ranges.retain(|range| range.start < range.end);
    ranges.sort_by_key(|range| (range.start, range.end));
    let mut merged: Ranges = Vec::new();
    for range in ranges {
        match merged.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    merged
}

fn subtract(ranges: &Ranges, removed: &Ranges) -> Ranges {
    let mut result = Vec::new();
    for range in ranges {
        let mut cursor = range.start;
        for cut in removed
            .iter()
            .filter(|cut| cut.start < range.end && range.start < cut.end)
        {
            if cut.start > cursor {
                result.push(cursor..cut.start);
            }
            cursor = cursor.max(cut.end);
        }
        if cursor < range.end {
            result.push(cursor..range.end);
        }
    }
    result
}

/// Maximal B1 spans per stream, from the serialized parts and record layouts.
#[must_use]
pub fn b1_spans(fidelity: &Value) -> BTreeMap<String, Ranges> {
    let mut covered: BTreeMap<String, Ranges> = BTreeMap::new();
    let mut framing: BTreeMap<String, Ranges> = BTreeMap::new();
    let mut given_up: BTreeMap<String, Ranges> = BTreeMap::new();
    let mut reads: BTreeMap<String, Ranges> = BTreeMap::new();
    for part in fidelity
        .get("parts")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|parts| parts.values())
    {
        let Some((stream, bytes)) = extent(part) else {
            continue;
        };
        covered
            .entry(stream.clone())
            .or_default()
            .push(bytes.clone());
        framing.entry(stream.clone()).or_default().extend(
            part.get("framing_bytes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(range),
        );
        if part.pointer("/state/state").and_then(Value::as_str) == Some("given_up") {
            given_up.entry(stream).or_default().push(bytes);
        }
    }
    for layout in fidelity
        .get("record_layout")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|layouts| layouts.values())
    {
        for (stream, ranges) in layout
            .get("reads")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            reads
                .entry(stream.clone())
                .or_default()
                .extend(ranges.as_array().into_iter().flatten().filter_map(range));
        }
    }
    let mut spans = BTreeMap::new();
    for (stream, ranges) in covered {
        let frame = merge(framing.remove(&stream).unwrap_or_default());
        let read = merge(reads.remove(&stream).unwrap_or_default());
        let unread = subtract(&subtract(&merge(ranges), &read), &frame);
        let dropped = subtract(&merge(given_up.remove(&stream).unwrap_or_default()), &frame);
        let all = merge(unread.into_iter().chain(dropped).collect());
        if !all.is_empty() {
            spans.insert(stream, all);
        }
    }
    spans
}

fn class_name(class: ArenaClass) -> &'static str {
    match class {
        ArenaClass::Model => "model",
        ArenaClass::Design => "design",
    }
}

/// The `unrepresented` classes the §3.1 derivation gives for one decode.
#[must_use]
pub fn b1_unrepresented(document: &Value, fidelity: &Value) -> BTreeSet<&'static str> {
    let mut neutral: BTreeMap<&str, &'static str> = BTreeMap::new();
    let mut linked: BTreeMap<&str, BTreeSet<&'static str>> = BTreeMap::new();
    for (arena, rows) in document
        .get("model")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        let Some(class) = ArenaName::parse(arena).map(|name| class_name(name.class())) else {
            continue;
        };
        for row in rows.as_array().into_iter().flatten() {
            if let Some(id) = row.get("id").and_then(Value::as_str) {
                neutral.insert(id, class);
            }
            if let Some(native) = row.get("native_ref").and_then(Value::as_str) {
                linked.entry(native).or_default().insert(class);
            }
        }
    }
    let mut native_owner: BTreeMap<&str, &str> = BTreeMap::new();
    let mut stubs: Vec<(&str, Vec<&str>)> = Vec::new();
    for arenas in document
        .get("native")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|namespaces| namespaces.values())
        .filter_map(Value::as_object)
    {
        for (arena, rows) in arenas {
            for row in rows.as_array().into_iter().flatten() {
                let Some(id) = row.get("id").and_then(Value::as_str) else {
                    continue;
                };
                if let Some(owner) = row.get("owner").and_then(Value::as_str) {
                    native_owner.insert(id, owner);
                }
                if arena == "unknowns" {
                    let links = row
                        .get("links")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .collect();
                    stubs.push((id, links));
                }
            }
        }
    }
    let layouts: Vec<(&str, String, Range<u64>)> = fidelity
        .get("record_layout")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(id, layout)| {
            extent(layout).map(|(stream, bytes)| (id.as_str(), stream, bytes))
        })
        .collect();
    let retained: Vec<(&str, String, Range<u64>)> = fidelity
        .get("retained_records")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(id, record)| {
            let stream = record.get("stream")?.as_str()?.to_owned();
            let offset = record.get("offset")?.as_u64()?;
            let length = retained_length(record.get("bytes")?)?;
            Some((id.as_str(), stream, offset..offset.checked_add(length)?))
        })
        .collect();
    let mut classes = BTreeSet::new();
    for (stream, spans) in b1_spans(fidelity) {
        for span in spans {
            let contains = |(_, other, bytes): &&(&str, String, Range<u64>)| {
                *other == stream && bytes.start <= span.start && span.end <= bytes.end
            };
            let mut owners: BTreeSet<&str> = BTreeSet::new();
            if let Some((id, _, _)) = layouts
                .iter()
                .filter(contains)
                .min_by_key(|(_, _, bytes)| bytes.end - bytes.start)
            {
                owners.insert(id);
                if let Some(owner) = native_owner.get(id) {
                    owners.insert(owner);
                }
            }
            for (stub, links) in &stubs {
                if retained
                    .iter()
                    .filter(contains)
                    .any(|(id, _, _)| id == stub)
                {
                    owners.extend(links.iter().copied());
                }
            }
            let mut span_classes: BTreeSet<&'static str> = BTreeSet::new();
            for owner in &owners {
                if let Some(class) = neutral.get(owner) {
                    span_classes.insert(class);
                }
                if let Some(found) = linked.get(owner) {
                    span_classes.extend(found.iter().copied());
                }
            }
            if span_classes.is_empty() {
                span_classes.extend(CLASS_NAMES);
            }
            classes.extend(span_classes);
        }
    }
    classes
}

fn retained_length(bytes: &Value) -> Option<u64> {
    match bytes.get("retention")?.as_str()? {
        "inline" => {
            let data = bytes.get("data")?.as_str()?;
            let padding = data.bytes().rev().take_while(|byte| *byte == b'=').count();
            u64::try_from(data.len() / 4 * 3 - padding).ok()
        }
        "digest" => bytes.get("byte_len")?.as_u64(),
        _ => None,
    }
}

/// The §9.3 scope and transfer findings for one decode at one requested depth.
///
/// `requested` names the classes the request requires; a missing one needs
/// a `depth.not-delivered` loss.
#[must_use]
pub fn scope_findings(document: &Value, report: &Value, requested: &[&str]) -> Vec<String> {
    let mut findings = Vec::new();
    let scope = document.get("scope");
    for class in CLASS_NAMES {
        let held = scope
            .and_then(|scope| scope.get(class))
            .and_then(Value::as_bool);
        let Some(held) = held else {
            findings.push(format!("scope has no {class} flag"));
            continue;
        };
        let populated = document
            .get("model")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .filter(|(arena, rows)| {
                ArenaName::parse(arena).is_some_and(|name| class_name(name.class()) == class)
                    && rows.as_array().is_some_and(|rows| !rows.is_empty())
            })
            .map(|(arena, _)| arena.as_str())
            .collect::<Vec<_>>();
        if !held && !populated.is_empty() {
            findings.push(format!(
                "scope excludes {class} but {populated:?} hold records"
            ));
        }
        let not_delivered = report
            .get("losses")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .any(|loss| {
                loss.pointer("/code/code").and_then(Value::as_str) == Some("depth.not-delivered")
            });
        if requested.contains(&class) && !held && !not_delivered {
            findings.push(format!("the request requires {class}, scope excludes it, and no depth.not-delivered loss is charged"));
        }
    }
    if report.pointer("/transfer/scope") != scope {
        findings.push(format!(
            "transfer.scope {} differs from ir.scope {}",
            report.pointer("/transfer/scope").unwrap_or(&Value::Null),
            scope.unwrap_or(&Value::Null)
        ));
    }
    findings
}

/// Every expression parameter token names a parameter of the document (Rule D).
#[must_use]
pub fn dangling_token_findings(document: &Value) -> Vec<String> {
    fn walk<'a>(value: &'a Value, found: &mut Vec<&'a str>) {
        match value {
            Value::Object(object) => {
                if let Some(Value::Array(tokens)) = object.get("tokens") {
                    for token in tokens {
                        if token.get("kind").and_then(Value::as_str) == Some("parameter") {
                            if let Some(id) = token.get("value").and_then(Value::as_str) {
                                found.push(id);
                            }
                        }
                    }
                }
                object.values().for_each(|child| walk(child, found));
            }
            Value::Array(items) => items.iter().for_each(|child| walk(child, found)),
            _ => {}
        }
    }
    let parameters: BTreeSet<&str> = document
        .pointer("/model/parameters")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|row| row.get("id")?.as_str())
        .collect();
    let mut findings = Vec::new();
    let mut tokens = Vec::new();
    if let Some(model) = document.get("model") {
        walk(model, &mut tokens);
    }
    for token in tokens {
        if !parameters.contains(token) {
            findings.push(format!("expression token names absent parameter {token}"));
        }
    }
    findings
}

#[cfg(test)]
mod tests;
