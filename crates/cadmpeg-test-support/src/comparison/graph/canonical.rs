// SPDX-License-Identifier: Apache-2.0
//! Optional-key omissions proved by the actual typed neutral list editors.

use super::{filter_lists, Record};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::{EntityRewrite, Model};
use cadmpeg_ir::schema::rewrite::typed::{prune, RewriteIdentities};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
enum Step {
    Field(String),
    Item(usize),
}
struct Omission {
    path: Vec<Step>,
    expected: Value,
}

pub(super) fn apply(
    original: &Value,
    removed: &BTreeSet<String>,
    records: &mut [Record],
) -> Result<(), String> {
    if removed.is_empty() {
        return Ok(());
    }
    let Some(raw_model) = original.get("model") else {
        return Ok(());
    };
    // The generic graph comparator also accepts synthetic/open JSON graphs.
    // An untyped graph grants no serde-derived omission permissions.
    let Ok(model) = serde_json::from_value::<Model>(raw_model.clone()) else {
        return Ok(());
    };
    let mut canonical_before = serde_json::to_value(&model).map_err(|e| e.to_string())?;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .map_err(|e| e.to_string())?;
    let mut canonical_model = Model::default();
    canonical_model
        .extend_rewritten(model, &mut Lists { ctx: &ctx, removed })
        .map_err(|e| e.to_string())?;
    let canonical_after = serde_json::to_value(canonical_model).map_err(|e| e.to_string())?;
    filter_lists(&mut canonical_before, removed);
    let mut omissions = BTreeMap::<String, Vec<Omission>>::new();
    for (arena, rows) in canonical_before
        .as_object()
        .ok_or("typed model is not an object")?
    {
        let Some(after_rows) = canonical_after.get(arena).and_then(Value::as_array) else {
            continue;
        };
        let after_by_id = after_rows
            .iter()
            .filter_map(|row| Some((row.get("id")?.as_str()?, row)))
            .collect::<BTreeMap<_, _>>();
        for row in rows.as_array().ok_or("typed model arena is not an array")? {
            let Some(id) = row.get("id").and_then(Value::as_str) else {
                continue;
            };
            let Some(after) = after_by_id.get(id) else {
                continue;
            };
            collect(
                row,
                after,
                &mut Vec::new(),
                omissions.entry(id.to_owned()).or_default(),
            );
        }
    }
    for record in records {
        for omission in omissions.get(&record.id).into_iter().flatten() {
            remove(&mut record.value, &omission.path, &omission.expected);
        }
    }
    Ok(())
}

struct Lists<'ctx, 'arena> {
    ctx: &'ctx DecodeContext<'arena>,
    removed: &'ctx BTreeSet<String>,
}
impl EntityRewrite for Lists<'_, '_> {
    type Error = CodecError;
    fn rewrite<T: RewriteIdentities>(&mut self, mut entity: T) -> Result<T, CodecError> {
        if prune::analyze(&entity, self.ctx, self.removed)?
            == prune::ReferencePruneEffect::ListsChanged
        {
            let effect = entity.prune_reference_lists(self.ctx, self.removed)?;
            if !effect.can_apply() {
                return Err(CodecError::malformed(format!(
                    "typed comparison list edit refused: {effect:?}"
                )));
            }
        }
        Ok(entity)
    }
}

fn collect(before: &Value, after: &Value, path: &mut Vec<Step>, out: &mut Vec<Omission>) {
    match (before, after) {
        (Value::Object(before), Value::Object(after)) => {
            for (field, value) in before {
                path.push(Step::Field(field.clone()));
                if let Some(current) = after.get(field) {
                    collect(value, current, path, out);
                } else {
                    out.push(Omission {
                        path: path.clone(),
                        expected: value.clone(),
                    });
                }
                path.pop();
            }
        }
        (Value::Array(before), Value::Array(after)) if before.len() == after.len() => {
            for (index, (before, after)) in before.iter().zip(after).enumerate() {
                path.push(Step::Item(index));
                collect(before, after, path, out);
                path.pop();
            }
        }
        _ => {}
    }
}

fn remove(value: &mut Value, path: &[Step], expected: &Value) {
    let Some((step, rest)) = path.split_first() else {
        return;
    };
    match step {
        Step::Field(field) => {
            if rest.is_empty() {
                if let Some(fields) = value.as_object_mut() {
                    // Extra unrelated fields in an otherwise empty wrapper
                    // cannot disappear through typed serde reconstruction.
                    if fields.get(field) == Some(expected) {
                        fields.remove(field);
                    }
                }
            } else if let Some(child) = value.get_mut(field) {
                remove(child, rest, expected);
            }
        }
        Step::Item(index) => {
            if let Some(child) = value.get_mut(*index) {
                remove(child, rest, expected);
            }
        }
    }
}

#[cfg(test)]
mod tests;
