// SPDX-License-Identifier: Apache-2.0
//! The asymmetric record comparison of §5.3 `Cross` mode.
//!
//! Object keys the `before` record holds must survive; object keys only the
//! `after` record holds are target detail and are ignored. Numbers, strings,
//! arrays and nulls compare exactly as [`cadmpeg_ir::compare::values_agree`]
//! compares them.

use cadmpeg_ir::compare::{floats_agree, texts_agree};
use serde_json::Value;

/// Compare two mapped records across formats, ignoring `after`-only keys.
///
/// # Errors
///
/// Returns a description of the first disagreement, located by JSON path
/// relative to the records passed in.
pub(super) fn values_agree(left: &Value, right: &Value) -> Result<(), String> {
    let mut path = String::new();
    walk(left, right, &mut path)
}

/// Compare two colour shapes across formats, ignoring `after`-only keys.
///
/// Shapes encode objects as `{"object": [[key, value], ...]}` with sorted
/// entries, references as `{"reference": true}`, and rounded numbers.
/// Entries match as a multiset on their serialized key; every `left` entry
/// must find an unmatched `right` entry. Real arrays keep exact length.
pub(super) fn shapes_agree(left: &Value, right: &Value) -> Result<(), String> {
    let mut path = String::new();
    walk_shape(left, right, &mut path)
}

/// Walks two colour shapes in step; `right`-only object entries are ignored.
fn walk_shape(left: &Value, right: &Value, path: &mut String) -> Result<(), String> {
    match (left, right) {
        (Value::Object(left_map), Value::Object(right_map)) => {
            match (
                left_map.get("object"),
                right_map.get("object"),
                left_map.len(),
                right_map.len(),
            ) {
                (Some(Value::Array(left_entries)), Some(Value::Array(right_entries)), 1, 1) => {
                    entries_agree(left_entries, right_entries, path)
                }
                _ if left_map == right_map => Ok(()),
                _ => Err(disagreement(
                    path,
                    &format!("left {}, right {}", truncate(left), truncate(right)),
                )),
            }
        }
        (Value::Array(left_items), Value::Array(right_items)) => {
            if left_items.len() != right_items.len() {
                return Err(disagreement(
                    path,
                    &format!(
                        "left has {} item(s), right has {}",
                        left_items.len(),
                        right_items.len()
                    ),
                ));
            }
            for (index, (left_child, right_child)) in left_items.iter().zip(right_items).enumerate()
            {
                let restore = path.len();
                path.push('[');
                path.push_str(&index.to_string());
                path.push(']');
                walk_shape(left_child, right_child, path)?;
                path.truncate(restore);
            }
            Ok(())
        }
        (Value::Number(left_number), Value::Number(right_number)) => {
            let agree = match (left_number.as_f64(), right_number.as_f64()) {
                (Some(left_float), Some(right_float))
                    if left_number.is_f64() && right_number.is_f64() =>
                {
                    floats_agree(left_float, right_float)
                }
                _ => left_number == right_number,
            };
            if agree {
                Ok(())
            } else {
                Err(disagreement(
                    path,
                    &format!("left {left_number}, right {right_number}"),
                ))
            }
        }
        (Value::String(left_text), Value::String(right_text)) => {
            if texts_agree(left_text, right_text) {
                Ok(())
            } else {
                Err(disagreement(
                    path,
                    &format!("left {}, right {}", truncate(left), truncate(right)),
                ))
            }
        }
        _ if left == right => Ok(()),
        _ => Err(disagreement(
            path,
            &format!("left {}, right {}", truncate(left), truncate(right)),
        )),
    }
}

/// Every `left` shape entry must match one unmatched `right` entry on its
/// serialized key; matched values compare recursively.
fn entries_agree(left: &[Value], right: &[Value], path: &mut String) -> Result<(), String> {
    let mut used = vec![false; right.len()];
    for left_entry in left {
        let (left_key, left_value) = entry_parts(left_entry)?;
        let mut matched = None;
        for (index, right_entry) in right.iter().enumerate() {
            if used[index] {
                continue;
            }
            let (right_key, right_value) = entry_parts(right_entry)?;
            if left_key == right_key {
                matched = Some((index, right_value));
                break;
            }
        }
        let Some((index, right_value)) = matched else {
            return Err(disagreement(path, "left has an entry right does not"));
        };
        used[index] = true;
        let restore = path.len();
        path.push('[');
        path.push_str(&left_key.to_string());
        path.push(']');
        walk_shape(left_value, right_value, path)?;
        path.truncate(restore);
    }
    Ok(())
}

/// Splits one shape entry into its serialized key and its value shape.
fn entry_parts(entry: &Value) -> Result<(Value, &Value), String> {
    let [key, value] = entry.as_array().map_or(&[][..], Vec::as_slice) else {
        return Err("a colour shape entry is not a key-value pair".into());
    };
    Ok((key.clone(), value))
}

/// Walks two values in step; `right`-only object keys are ignored.
fn walk(left: &Value, right: &Value, path: &mut String) -> Result<(), String> {
    match (left, right) {
        (Value::Object(left_map), Value::Object(right_map)) => {
            for (key, left_child) in left_map {
                let Some(right_child) = right_map.get(key) else {
                    return Err(disagreement(
                        path,
                        &format!("left has key `{key}`, right does not"),
                    ));
                };
                let restore = path.len();
                path.push('.');
                path.push_str(key);
                walk(left_child, right_child, path)?;
                path.truncate(restore);
            }
            Ok(())
        }
        (Value::Array(left_items), Value::Array(right_items)) => {
            if left_items.len() != right_items.len() {
                return Err(disagreement(
                    path,
                    &format!(
                        "left has {} item(s), right has {}",
                        left_items.len(),
                        right_items.len()
                    ),
                ));
            }
            for (index, (left_child, right_child)) in left_items.iter().zip(right_items).enumerate()
            {
                let restore = path.len();
                path.push('[');
                path.push_str(&index.to_string());
                path.push(']');
                walk(left_child, right_child, path)?;
                path.truncate(restore);
            }
            Ok(())
        }
        (Value::Number(left_number), Value::Number(right_number)) => {
            let agree = match (left_number.as_f64(), right_number.as_f64()) {
                (Some(left_float), Some(right_float))
                    if left_number.is_f64() && right_number.is_f64() =>
                {
                    floats_agree(left_float, right_float)
                }
                _ => left_number == right_number,
            };
            if agree {
                Ok(())
            } else {
                Err(disagreement(
                    path,
                    &format!("left {left_number}, right {right_number}"),
                ))
            }
        }
        (Value::String(left_text), Value::String(right_text)) => {
            if texts_agree(left_text, right_text) {
                Ok(())
            } else {
                Err(disagreement(
                    path,
                    &format!("left {}, right {}", truncate(left), truncate(right)),
                ))
            }
        }
        _ if left == right => Ok(()),
        _ => Err(disagreement(
            path,
            &format!("left {}, right {}", truncate(left), truncate(right)),
        )),
    }
}

/// Renders one disagreement with the JSON path that located it.
fn disagreement(path: &str, detail: &str) -> String {
    let location = if path.is_empty() { "<root>" } else { path };
    format!("at `{location}`: {detail}")
}

/// Renders a value for a failure message, bounded so a whole arena cannot land
/// in a panic or a report.
fn truncate(value: &Value) -> String {
    let text = value.to_string();
    let Some((end, last)) = text
        .char_indices()
        .take_while(|(index, _)| *index < 120)
        .last()
    else {
        return text;
    };
    let end = end + last.len_utf8();
    if end < text.len() {
        format!("{}…", &text[..end])
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::values_agree;

    #[test]
    fn an_after_only_key_is_ignored_at_every_level() {
        let left = json!({"a": 1, "nested": {"b": true}});
        let right = json!({"a": 1, "added": 2, "nested": {"b": true, "extra": []}});
        assert!(values_agree(&left, &right).is_ok());
    }

    #[test]
    fn a_before_only_key_fails() {
        let left = json!({"a": 1, "lost": 2});
        let right = json!({"a": 1});
        assert!(values_agree(&left, &right).is_err());
    }

    #[test]
    fn numbers_keep_the_shared_tolerance() {
        let left = json!({"v": 100.0});
        let passing = json!({"v": 100.0 * (1.0 + 1.0e-13)});
        let failing = json!({"v": 100.0 * (1.0 + 1.0e-9)});
        assert!(values_agree(&left, &passing).is_ok());
        assert!(values_agree(&left, &failing).is_err());
    }
}
