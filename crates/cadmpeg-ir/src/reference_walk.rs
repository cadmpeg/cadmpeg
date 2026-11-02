// SPDX-License-Identifier: Apache-2.0
//! Identity edges shared by graph queries and fragment closure.

use serde_json::Value;

/// Visit string fields that resolve to an indexed entity. Return false to stop.
pub fn visit_edges<T: Copy>(
    value: &Value,
    path: &str,
    skip_top_id: bool,
    lookup: &impl Fn(&str) -> Vec<T>,
    visit: &mut impl FnMut(&str, T) -> bool,
) -> bool {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if skip_top_id && key == "id" {
                    continue;
                }
                let child_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                if !visit_edges(child, &child_path, false, lookup, visit) {
                    return false;
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                if !visit_edges(item, path, false, lookup, visit) {
                    return false;
                }
            }
        }
        Value::String(s) => {
            if path.is_empty() {
                return true;
            }
            for record in lookup(s) {
                if !visit(path, record) {
                    return false;
                }
            }
        }
        _ => {}
    }
    true
}
