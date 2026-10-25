// SPDX-License-Identifier: Apache-2.0
//! Structural conservation for records witnessed by actual damaged-source reads.

use super::super::graph::Document;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub(in crate::comparison) fn check_changes(
    previous: &Document,
    current: &Document,
    changed: &BTreeSet<String>,
    fallbacks: &BTreeSet<String>,
    removed: &BTreeSet<String>,
    original: &Value,
) -> Result<(), String> {
    let mut pruned = previous.clone();
    pruned.prune_records(removed, original)?;
    let missing = previous
        .ids
        .keys()
        .filter(|id| !current.ids.contains_key(*id))
        .cloned()
        .collect();
    for record in &current.records {
        if record.arena.ends_with("/unknowns") {
            continue;
        }
        let Some(index) = previous.ids.get(&record.id) else {
            return Err(format!(
                "given-up decode added non-stub record {}",
                record.id
            ));
        };
        let before = &previous.records[*index];
        if before.arena != record.arena {
            return Err(format!("given-up decode moved record {}", record.id));
        }
        if fallbacks.contains(&record.id) || removed.contains(&record.id) {
            let mut expected = pruned.ids.get(&record.id).map_or_else(
                || before.value.clone(),
                |index| pruned.records[*index].value.clone(),
            );
            super::super::graph::filter_lists(&mut expected, removed);
            let count = super::fallback::mask_fields(
                &before.arena,
                &mut expected,
                &record.value,
                &missing,
            )?;
            if count == 0 || cadmpeg_ir::compare::values_agree(&expected, &record.value).is_err() {
                return Err(format!("dependent survivor {} changed outside its typed Native fallback or permitted list losses", record.id));
            }
        }
        if changed.contains(&record.id) {
            let mut old = Shape::default();
            let mut new = Shape::default();
            let mut structure = before.structure.clone();
            super::fallback::mask_fields(
                &before.arena,
                &mut structure,
                &record.structure,
                &missing,
            )?;
            shape(&structure, &mut Vec::new(), &mut old);
            shape(&record.structure, &mut Vec::new(), &mut new);
            if !new.keys.is_subset(&old.keys)
                || !new.tags.is_subset(&old.tags)
                || new
                    .arrays
                    .iter()
                    .any(|(path, len)| *len > old.arrays.get(path).copied().unwrap_or(0))
            {
                return Err(format!(
                    "affected record {} gained a key, variant tag or list element",
                    record.id
                ));
            }
        }
    }
    for id in changed {
        if !current.ids.contains_key(id) {
            return Err(format!(
                "affected survivor {id} disappeared without its removal witness"
            ));
        }
    }
    Ok(())
}

#[derive(Default)]
struct Shape {
    keys: BTreeSet<Vec<Segment>>,
    tags: BTreeSet<(Vec<Segment>, String)>,
    arrays: BTreeMap<Vec<Segment>, usize>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Segment {
    Field(String),
    Item,
}

fn shape(value: &Value, path: &mut Vec<Segment>, out: &mut Shape) {
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                path.push(Segment::Field(key.clone()));
                out.keys.insert(path.clone());
                if matches!(key.as_str(), "type" | "kind" | "tag") {
                    if let Some(tag) = value.as_str() {
                        out.tags.insert((path.clone(), tag.to_owned()));
                    }
                }
                shape(value, path, out);
                path.pop();
            }
        }
        Value::Array(values) => {
            out.arrays
                .entry(path.clone())
                .and_modify(|len| *len = (*len).max(values.len()))
                .or_insert(values.len());
            path.push(Segment::Item);
            for value in values {
                shape(value, path, out);
            }
            path.pop();
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn document(value: Value) -> Document {
        Document::read(&json!({"model": {"values": [value]}}), &BTreeSet::new()).unwrap()
    }

    #[test]
    fn affected_survivor_may_lose_content_but_cannot_gain_structure_or_variants() {
        let previous = document(
            json!({"id":"kept", "body":{"type":"List", "values":[{"type":"Float","value":7},null]}}),
        );
        let changed = BTreeSet::from(["kept".to_owned()]);
        let smaller = document(json!({"id":"kept", "body":null}));
        check_changes(
            &previous,
            &smaller,
            &changed,
            &BTreeSet::new(),
            &BTreeSet::new(),
            &serde_json::json!({"model":{}}),
        )
        .unwrap();
        for invalid in [
            json!({"id":"kept", "bonus":true}),
            json!({"id":"kept", "native_ref":"invented"}),
            json!({"id":"kept", "body":{"type":"Native"}}),
            json!({"id":"kept", "body":{"type":"List", "values":[null,null,null]}}),
        ] {
            assert!(check_changes(
                &previous,
                &document(invalid),
                &changed,
                &BTreeSet::new(),
                &BTreeSet::new(),
                &serde_json::json!({"model":{}}),
            )
            .is_err());
        }
        assert!(check_changes(
            &previous,
            &document(json!({"id":"invented"})),
            &BTreeSet::new(),
            &BTreeSet::new(),
            &BTreeSet::new(),
            &serde_json::json!({"model":{}}),
        )
        .is_err());
    }
}
