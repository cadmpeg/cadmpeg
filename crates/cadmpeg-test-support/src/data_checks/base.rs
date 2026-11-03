// SPDX-License-Identifier: Apache-2.0
//! Base-comparison data checks: 1b, the check 6 base-association rule, and the
//! check 7 base ordinal order (#246 IV.2, #219).
//!
//! Each check compares one head golden against its base form (see
//! [`crate::base`]) and returns one finding per violation, so a caller names
//! every failing record in one run.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

const MAPS: [&str; 2] = ["properties", "source_properties"];

/// Every id-bearing neutral record of `document`, as `(arena, record)`.
fn model_records(document: &Value) -> Vec<(&str, &Value)> {
    document
        .get("model")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|model| model.iter())
        .flat_map(|(arena, rows)| {
            rows.as_array().into_iter().flatten().filter_map(|row| {
                row.get("id")
                    .and_then(Value::as_str)
                    .map(|_| (arena.as_str(), row))
            })
        })
        .collect()
}

/// Every native record id of `document`, across namespaces.
fn native_ids(document: &Value) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    if let Some(native) = document.get("native").and_then(Value::as_object) {
        for arenas in native.values() {
            for rows in arenas
                .as_object()
                .into_iter()
                .flat_map(|space| space.values())
            {
                for row in rows.as_array().into_iter().flatten() {
                    if let Some(id) = row.get("id").and_then(Value::as_str) {
                        ids.insert(id.to_owned());
                    }
                }
            }
        }
    }
    ids
}

/// Fold a field or key spelling for the check 1b name comparison: `.`, `-`
/// and spaces become `_`, camel humps gain `_`, all lowercase.
fn snake(text: &str) -> String {
    let mut folded = String::with_capacity(text.len());
    let mut previous: Option<char> = None;
    for grain in text.chars() {
        if grain == '-' || grain == '.' || grain == ' ' || grain == '/' || grain == '_' {
            folded.push('_');
        } else if grain.is_ascii_uppercase()
            && previous.is_some_and(|found| {
                found != '_' && (found.is_ascii_lowercase() || found.is_ascii_digit())
            })
        {
            folded.push('_');
            folded.push(grain.to_ascii_lowercase());
        } else {
            for lower in grain.to_lowercase() {
                folded.push(lower);
            }
        }
        previous = Some(grain);
    }
    folded
}

/// Whether a typed field value holds a base map value: equal as text, with
/// numbers compared by value and booleans accepting their spellings.
fn value_matches(field: &Value, held: &Value) -> bool {
    match (field, held) {
        (Value::Number(left), Value::Number(right)) => left.as_f64() == right.as_f64(),
        (Value::Number(number), Value::String(text))
        | (Value::String(text), Value::Number(number)) => number.as_f64().is_some_and(|left| {
            text.parse::<f64>().is_ok_and(|right| left == right) || text == &number.to_string()
        }),
        (Value::Bool(left), Value::Bool(right)) => left == right,
        (Value::Bool(flag), Value::String(text)) | (Value::String(text), Value::Bool(flag)) => {
            text.eq_ignore_ascii_case(if *flag { "true" } else { "false" })
        }
        (Value::String(left), Value::String(right)) => left == right,
        (left, right) => left == right,
    }
}

/// Whether `(arena, map, key, value)` is on the codec's §8 deletion list: a
/// property the codec's Phase 3 issue deletes from its record kind's map, so
/// its absence at head is specified, not lost.
fn is_deleted(
    namespace: &str,
    arena: &str,
    map: &str,
    key: &str,
    value: &Value,
    base_native: &BTreeSet<String>,
) -> bool {
    match namespace {
        // #223 §8: `view_bytes`/`view_sha256` leave `Feature.source_properties`.
        "rhino" => {
            arena == "features"
                && map == "source_properties"
                && matches!(key, "view_bytes" | "view_sha256")
        }
        // #224 §8: appearance point/line sizes, every `ViewPresentation`
        // property, and the spreadsheet cell style keys leave `properties`.
        "fcstd" => {
            map == "properties"
                && ((arena == "appearances" && matches!(key, "point_size" | "line_width"))
                    || arena == "view_presentations"
                    || (arena == "parameters"
                        && matches!(
                            key,
                            "alignment"
                                | "style"
                                | "foregroundColor"
                                | "backgroundColor"
                                | "displayUnit"
                        )))
        }
        // #225 §8: the parameter properties the writer reads from the typed
        // native parameter record instead.
        "f3d" => {
            arena == "parameters"
                && map == "properties"
                && matches!(
                    key,
                    "source_kind" | "owner_record_index" | "unit" | "evaluated_scalar"
                )
        }
        // #229 §8: the sketch record named by `native_ref` already holds it.
        "creo" => arena == "parameters" && map == "properties" && key == "definition_id",
        // #226 §8: feature `source_properties` is always empty; the Scope
        // reader moved to the native record.
        "sldprt" => arena == "features" && map == "source_properties" && key == "Scope",
        // #230 §8: the parameter triples, `input_parameter_expression.S.O`,
        // the `parameter_use.N` construction lanes, and every entry whose
        // value is a native record id.
        "nx" => {
            (arena == "parameters"
                && matches!(key, "unit" | "declaration" | "declaration_object_id"))
                || (arena == "input_parameter_expression" && key == "S.O")
                || (arena == "features"
                    && map == "source_properties"
                    && key.starts_with("parameter_use."))
                || ((map == "source_properties"
                    || (arena == "configurations" && map == "properties"))
                    && value
                        .as_str()
                        .is_some_and(|named| base_native.contains(named)))
        }
        _ => false,
    }
}

/// Check 1b (#246): nothing lost. Every `(key, value)` in a base neutral
/// `properties`/`source_properties` map meets one of the four outs, where the
/// head record is the one with the same id: a §8 deletion, the head map, a
/// typed field of the head record or its `native_ref` record, or a native
/// record owned by the head record naming the key.
#[must_use]
pub fn base_property_loss_findings(namespace: &str, base: &Value, head: &Value) -> Vec<String> {
    let head_records: BTreeMap<&str, (&str, &Value)> = model_records(head)
        .into_iter()
        .map(|(arena, record)| {
            (
                record
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("<no id>"),
                (arena, record),
            )
        })
        .collect();
    let head_native: BTreeMap<&str, &Value> = head
        .get("native")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|native| native.values())
        .flat_map(|space| {
            space
                .as_object()
                .into_iter()
                .flat_map(|space| space.values())
        })
        .flat_map(|rows| rows.as_array().into_iter().flatten())
        .filter_map(|row| row.get("id").and_then(Value::as_str).map(|id| (id, row)))
        .collect();
    let base_native = native_ids(base);
    let mut findings = Vec::new();
    for (arena, record) in model_records(base) {
        let id = record
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("<no id>");
        for map in MAPS {
            let Some(entries) = record.get(map).and_then(Value::as_object) else {
                continue;
            };
            for (key, value) in entries {
                if is_deleted(namespace, arena, map, key, value, &base_native) {
                    continue;
                }
                let Some((_, held)) = head_records.get(id) else {
                    findings.push(format!(
                        "{arena} {id}: record absent at head; {map}[{key:?}] = {} lost",
                        truncated(value),
                    ));
                    continue;
                };
                if MAPS.iter().any(|kept| {
                    held.get(kept)
                        .and_then(Value::as_object)
                        .and_then(|entries| entries.get(key))
                        .is_some_and(|found| found == value)
                }) {
                    continue;
                }
                if typed_field_holds(held, key, value)
                    || held
                        .get("native_ref")
                        .and_then(Value::as_str)
                        .and_then(|native| head_native.get(native))
                        .is_some_and(|native| typed_field_holds(native, key, value))
                    || owned_native_holds(head, id, key, value)
                {
                    continue;
                }
                findings.push(format!(
                    "{arena} {id}: {map}[{key:?}] = {} lost",
                    truncated(value),
                ));
            }
        }
    }
    findings
}

/// Whether a top-level typed field of `record` holds the base map entry: the
/// field name, snake-cased, equals the key snake-cased, and the field value
/// equals the map value as text. Identity plumbing (`id`, `native_ref`) and
/// the neutral maps themselves are not typed fields.
fn typed_field_holds(record: &Value, key: &str, value: &Value) -> bool {
    let wanted = snake(key);
    record
        .as_object()
        .into_iter()
        .flatten()
        .any(|(name, field)| {
            !matches!(
                name.as_str(),
                "id" | "native_ref" | "properties" | "source_properties"
            ) && snake(name) == wanted
                && value_matches(field, value)
        })
}

/// Whether a head native record owned by `owner` names the key and holds the
/// value: a `value` (or `FreeCAD` `body`) field equal to the map value as text.
fn owned_native_holds(head: &Value, owner: &str, key: &str, value: &Value) -> bool {
    head.get("native")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|native| native.values())
        .flat_map(|space| {
            space
                .as_object()
                .into_iter()
                .flat_map(|space| space.values())
        })
        .flat_map(|rows| rows.as_array().into_iter().flatten())
        .any(|row| {
            row.get("owner").and_then(Value::as_str) == Some(owner)
                && row.get("name").and_then(Value::as_str) == Some(key)
                && ["value", "body"]
                    .iter()
                    .filter_map(|field| row.get(*field))
                    .any(|held| value_matches(held, value))
        })
}

fn truncated(value: &Value) -> String {
    let text = value.to_string();
    match text.char_indices().nth(120) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text,
    }
}

/// Every string a record subtree holds, for the base-association rule.
fn subtree_strings(value: &Value, found: &mut BTreeSet<String>) {
    match value {
        Value::String(text) => {
            found.insert(text.clone());
        }
        Value::Array(items) => items.iter().for_each(|child| subtree_strings(child, found)),
        Value::Object(fields) => fields
            .values()
            .for_each(|child| subtree_strings(child, found)),
        _ => {}
    }
}

fn strings_of(value: &Value) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    subtree_strings(value, &mut found);
    found
}

/// One JSON path segment, for navigating a base holder to a head slot's path.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    Key(String),
    Index(usize),
}

/// A quantity-valued literal dimension slot of a head holder: its path and
/// its field name (the last key segment). Expression-valued literals (a
/// `Dimension<Expression>` bookkeeping slot, whose value carries `tokens`)
/// are the first rule's text-token domain, not an authored dimension.
fn literal_dimension_slots(holder: &Value) -> Vec<(Vec<Segment>, String)> {
    fn walk(value: &Value, path: &mut Vec<Segment>, slots: &mut Vec<(Vec<Segment>, String)>) {
        match value {
            Value::Object(fields) => {
                if fields.get("kind").and_then(Value::as_str) == Some("literal") {
                    let expression = fields
                        .get("value")
                        .and_then(Value::as_object)
                        .is_some_and(|content| content.contains_key("tokens"));
                    if !expression {
                        let field = path
                            .iter()
                            .rev()
                            .find_map(|segment| match segment {
                                Segment::Key(field) => Some(field.clone()),
                                Segment::Index(_) => None,
                            })
                            .unwrap_or_default();
                        slots.push((path.clone(), field));
                    }
                    return;
                }
                for (key, child) in fields {
                    path.push(Segment::Key(key.clone()));
                    walk(child, path, slots);
                    path.pop();
                }
            }
            Value::Array(items) => {
                for (index, child) in items.iter().enumerate() {
                    path.push(Segment::Index(index));
                    walk(child, path, slots);
                    path.pop();
                }
            }
            _ => {}
        }
    }
    let mut slots = Vec::new();
    walk(holder, &mut Vec::new(), &mut slots);
    slots
}

fn navigate<'a>(record: &'a Value, path: &[Segment]) -> Option<&'a Value> {
    let mut current = record;
    for segment in path {
        current = match segment {
            Segment::Key(key) => current.get(key)?,
            Segment::Index(index) => current.get(index)?,
        };
    }
    Some(current)
}

/// Whether the base associates parameter `parameter` with the dimension slot
/// `(holder, field, path)`: the parameter is owned by the holder (or shares
/// its source record) under the slot's field name, the base holder names the
/// parameter at the slot's own path, or a sketch constraint names it.
fn slot_associated(
    holder_arena: &str,
    holder: &Value,
    parameter: &Value,
    field: &str,
    path: &[Segment],
) -> bool {
    if holder_arena == "sketch_constraints" {
        let Some(identity) = parameter.get("id").and_then(Value::as_str) else {
            return false;
        };
        return strings_of(holder).contains(identity);
    }
    let Some(name) = parameter.get("name").and_then(Value::as_str) else {
        return false;
    };
    if name.is_empty() || field.is_empty() {
        return false;
    }
    let holder_id = holder.get("id").and_then(Value::as_str).unwrap_or("");
    let holder_native = holder
        .get("native_ref")
        .and_then(Value::as_str)
        .unwrap_or("");
    let owned = parameter
        .get("owner")
        .and_then(Value::as_str)
        .is_some_and(|owner| {
            owner == holder_id
                || (!holder_native.is_empty()
                    && parameter.get("native_ref").and_then(Value::as_str) == Some(holder_native))
        });
    if owned && snake(name) == snake(field) {
        return true;
    }
    navigate(holder, path)
        .and_then(Value::as_object)
        .is_some_and(|dimension| {
            dimension.get("kind").and_then(Value::as_str) == Some("parameter")
                && dimension.get("value").and_then(Value::as_str)
                    == parameter.get("id").and_then(Value::as_str)
        })
}

/// Check 6, base-association rule (#246): no operation or constraint dimension
/// is `Literal` at head when a parameter↔dimension association present in the
/// base decode of that golden names it, in any field or map. A base parameter
/// names a head literal slot when it is owned by the slot's holder under the
/// slot's field name, when the base holder binds it at the slot's own path,
/// or when a base sketch constraint names it.
#[must_use]
pub fn base_binding_findings(base: &Value, head: &Value) -> Vec<String> {
    let base_model = base.get("model");
    let parameters: Vec<&Value> = base_model
        .and_then(|model| model.get("parameters"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect();
    if parameters.is_empty() {
        return Vec::new();
    }
    let base_holders: BTreeMap<&str, (&str, &Value)> = ["features", "sketch_constraints"]
        .into_iter()
        .flat_map(|arena| {
            base_model
                .and_then(|model| model.get(arena))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(move |row| {
                    row.get("id")
                        .and_then(Value::as_str)
                        .map(|id| (id, (arena, row)))
                })
        })
        .collect();
    let mut findings = Vec::new();
    let Some(head_model) = head.get("model") else {
        return findings;
    };
    for arena in ["features", "sketch_constraints"] {
        let Some(rows) = head_model.get(arena).and_then(Value::as_array) else {
            continue;
        };
        for holder in rows {
            let Some(id) = holder.get("id").and_then(Value::as_str) else {
                continue;
            };
            let Some((holder_arena, at_base)) = base_holders.get(id) else {
                continue;
            };
            for (path, field) in literal_dimension_slots(holder) {
                let bound = parameters.iter().filter(|parameter| {
                    slot_associated(holder_arena, at_base, parameter, &field, &path)
                });
                let mut names: Vec<String> = bound
                    .map(|parameter| {
                        let name = parameter
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("<no name>");
                        let identity = parameter
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or("<no id>");
                        format!("{name} ({identity})")
                    })
                    .collect();
                names.sort();
                names.dedup();
                for name in names {
                    findings.push(format!(
                        "{arena} {id}: literal {field} but base binds {name}"
                    ));
                }
            }
        }
    }
    findings
}

/// Check 7, base ordinal order (#246): on every base golden, for any two
/// features present at both, the head ordinal order equals the base ordinal
/// order. Ordinals compare pairwise; a tied side carries no order.
#[must_use]
pub fn base_ordinal_findings(base: &Value, head: &Value) -> Vec<String> {
    fn ordinals(document: &Value) -> BTreeMap<&str, u64> {
        document
            .get("model")
            .and_then(|model| model.get("features"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|feature| {
                match (
                    feature.get("id").and_then(Value::as_str),
                    feature.get("ordinal").and_then(Value::as_u64),
                ) {
                    (Some(id), Some(ordinal)) => Some((id, ordinal)),
                    _ => None,
                }
            })
            .collect()
    }
    let at_base = ordinals(base);
    let at_head = ordinals(head);
    let mut common: Vec<&str> = at_head
        .keys()
        .filter(|id| at_base.contains_key(*id))
        .copied()
        .collect();
    common.sort_unstable();
    let mut findings = Vec::new();
    for (position, left) in common.iter().enumerate() {
        for right in &common[position + 1..] {
            let base_order = at_base[left].cmp(&at_base[right]);
            let head_order = at_head[left].cmp(&at_head[right]);
            if base_order != std::cmp::Ordering::Equal
                && head_order != std::cmp::Ordering::Equal
                && base_order != head_order
            {
                findings.push(format!(
                    "{left} and {right}: base order {base_order:?} but head order {head_order:?}",
                ));
            }
        }
    }
    findings
}

#[cfg(test)]
mod tests;
