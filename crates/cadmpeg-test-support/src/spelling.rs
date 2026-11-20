// SPDX-License-Identifier: Apache-2.0
//! One spelling for each value of a document (`docs/cadir-v11.md` §2.5).
//!
//! A reader that admits a second spelling of a value lets two documents that
//! state the same thing differ in their bytes. This sweep reads each object of
//! a document through its document route, in a graft as
//! [`crate::unknown_keys`] does, after one of three edits that each state a
//! second spelling:
//!
//! * a key that another object of the same shape states, and this one leaves
//!   out, written `null`: an omitted `Option` refuses `null`;
//! * a key whose value is an empty list or map, deleted: a collection key is
//!   always written;
//! * the object's first key, stated twice: a repeated key is refused.
//!
//! A shape is an object's path with array indices removed, with the `kind` of
//! each enclosing object, so two variants under one path are two shapes. The
//! codec-private `/native` subtree is free-form by design and is skipped.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::CadIr;
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::unknown_keys::{
    grafted_document, member_graft, normalise, object_at_mut, Step, FREE_FORM_SHAPES,
};

/// The keys every shape is seen with, and the shapes already probed.
#[derive(Default)]
pub struct SpellingSweep {
    keys: BTreeMap<String, BTreeSet<String>>,
    probed: BTreeSet<String>,
    /// The number of edited documents read.
    pub probes: usize,
    /// The number of shapes edited in the full document because neither
    /// their array element nor their top-level member reads back on its own.
    pub fallbacks: usize,
}

impl SpellingSweep {
    /// Records the keys each object of `ir` states, for the `null` edit of
    /// every later [`Self::sweep`].
    pub fn learn(&mut self, ir: &Value) {
        for node in nodes(ir) {
            if let Some(fields) = node_object(ir, &node.path) {
                self.keys
                    .entry(node.shape)
                    .or_default()
                    .extend(fields.keys().cloned());
            }
        }
    }

    /// Reads every edit of each shape of `ir` not yet probed, and checks that
    /// `ir` writes back as itself.
    ///
    /// Returns each second spelling `ir` admits, with the path and edit that
    /// states it.
    ///
    /// # Errors
    ///
    /// Returns the serde error when `ir` does not read back as a `CadIr`.
    pub fn sweep(&mut self, ir: &Value) -> Result<BTreeSet<String>, serde_json::Error> {
        let mut accepting = BTreeSet::new();
        let read = CadIr::deserialize(ir)?;
        let canonical = canonical_text(&read)?;
        let written: Value = serde_json::from_str(&canonical)?;
        if let Some(at) = first_difference(ir, &written) {
            accepting.insert(format!("{at}: the document does not write back as itself"));
        }
        if canonical_text(&CadIr::from_json(&canonical)?)? != canonical {
            accepting.insert("its canonical text does not write back byte for byte".to_owned());
        }
        let empty = cadmpeg_ir::hash::finite_json::to_json_value(&CadIr::empty())?;
        for node in nodes(ir) {
            let Some(fields) = node_object(ir, &node.path) else {
                continue;
            };
            let signature = signature(&node.shape, fields);
            if !self.probed.insert(signature) {
                continue;
            }
            let reads_back = |graft: &Value| CadIr::deserialize(graft).is_ok();
            let (document, path) = grafted_document(ir, &empty, &node.path)
                .filter(|(graft, _)| reads_back(graft))
                .or_else(|| {
                    member_graft(ir, &empty, &node.path)
                        .filter(reads_back)
                        .map(|graft| (graft, node.path.clone()))
                })
                .unwrap_or_else(|| {
                    self.fallbacks += 1;
                    (ir.clone(), node.path.clone())
                });
            let at = normalise(&node.path);
            let free_form = FREE_FORM_SHAPES.contains(&at.as_str());
            if !free_form {
                let absent = self
                    .keys
                    .get(&node.shape)
                    .into_iter()
                    .flatten()
                    .filter(|key| !fields.contains_key(*key))
                    .cloned()
                    .collect::<Vec<_>>();
                for key in absent {
                    let mut edited = document.clone();
                    object_at_mut(&mut edited, &path)?.insert(key.clone(), Value::Null);
                    self.probe(&edited, &mut accepting, || {
                        format!("{at}: `{key}` written null")
                    });
                }
                for (key, value) in fields {
                    if is_empty_collection(value) {
                        let mut edited = document.clone();
                        object_at_mut(&mut edited, &path)?.remove(key);
                        self.probe(&edited, &mut accepting, || {
                            format!("{at}: empty `{key}` left out")
                        });
                    }
                }
            }
            if let Some(key) = fields.keys().next() {
                let mut text = String::new();
                write_repeating(&document, &path, &mut text);
                self.probes += 1;
                if serde_json::from_str::<CadIr>(&text).is_ok() {
                    accepting.insert(format!("{at}: `{key}` stated twice"));
                }
            }
        }
        Ok(accepting)
    }

    fn probe(
        &mut self,
        document: &Value,
        accepting: &mut BTreeSet<String>,
        edit: impl FnOnce() -> String,
    ) {
        self.probes += 1;
        if CadIr::deserialize(document).is_ok() {
            accepting.insert(edit());
        }
    }
}

/// One object of a document: its concrete path and its shape.
struct Node {
    path: Vec<Step>,
    shape: String,
}

/// Every non-native object of `ir`.
fn nodes(ir: &Value) -> Vec<Node> {
    let mut found = Vec::new();
    collect(ir, &mut Vec::new(), &mut String::new(), &mut found);
    found
}

fn collect(value: &Value, path: &mut Vec<Step>, shape: &mut String, found: &mut Vec<Node>) {
    let parent = shape.len();
    match value {
        Value::Object(fields) => {
            if matches!(path.first(), Some(Step::Key(key)) if key == "native") {
                return;
            }
            if let Some(Value::String(kind)) = fields.get("kind") {
                shape.push('[');
                shape.push_str(kind);
                shape.push(']');
            }
            found.push(Node {
                path: path.clone(),
                shape: shape.clone(),
            });
            for (key, field) in fields {
                let member = shape.len();
                shape.push('/');
                shape.push_str(&key.replace('~', "~0").replace('/', "~1"));
                path.push(Step::Key(key.clone()));
                collect(field, path, shape, found);
                path.pop();
                shape.truncate(member);
            }
        }
        Value::Array(items) => {
            shape.push_str("/#");
            for (index, item) in items.iter().enumerate() {
                path.push(Step::Index(index));
                collect(item, path, shape, found);
                path.pop();
            }
        }
        _ => {}
    }
    shape.truncate(parent);
}

fn node_object<'a>(ir: &'a Value, path: &[Step]) -> Option<&'a Map<String, Value>> {
    let mut node = ir;
    for step in path {
        node = match (step, node) {
            (Step::Key(key), Value::Object(fields)) => fields.get(key)?,
            (Step::Index(index), Value::Array(items)) => items.get(*index)?,
            _ => return None,
        };
    }
    node.as_object()
}

/// The shape with the keys the object states, marking each empty collection.
fn signature(shape: &str, fields: &Map<String, Value>) -> String {
    let mut text = shape.to_owned();
    for (key, value) in fields {
        text.push(' ');
        text.push_str(key);
        if is_empty_collection(value) {
            text.push_str("=0");
        }
    }
    text
}

fn canonical_text(ir: &CadIr) -> Result<String, serde_json::Error> {
    ir.to_canonical_json()
        .map_err(<serde_json::Error as serde::ser::Error>::custom)
}

/// Whether two numbers state one value of a number field.
///
/// A golden harness stores an `f32` field either widened to `f64` or in its
/// shortest `f32` digits, so two numbers that round to one `f32`, one of them
/// exactly, state the same `f32`.
fn numbers_agree(left: &serde_json::Number, right: &serde_json::Number) -> bool {
    if left == right {
        return true;
    }
    let narrow = |number: &serde_json::Number| number.to_string().parse::<f32>().ok();
    let (Some(wide_left), Some(wide_right)) = (left.as_f64(), right.as_f64()) else {
        return false;
    };
    let (Some(narrow_left), Some(narrow_right)) = (narrow(left), narrow(right)) else {
        return false;
    };
    narrow_left == narrow_right
        && (f64::from(narrow_left) == wide_left || f64::from(narrow_right) == wide_right)
}

/// The JSON pointer of the first value `read` and `written` differ at.
///
/// Each pair of values is compared once, and the pointer is written only for
/// the difference found.
pub(crate) fn first_difference(read: &Value, written: &Value) -> Option<String> {
    match (read, written) {
        (Value::Object(left), Value::Object(right)) => left
            .iter()
            .find_map(|(key, left)| match right.get(key) {
                Some(right) => first_difference(left, right).map(|at| format!("/{key}{at}")),
                None => Some(format!("/{key}")),
            })
            .or_else(|| {
                right
                    .keys()
                    .find(|key| !left.contains_key(*key))
                    .map(|key| format!("/{key}"))
            }),
        (Value::Array(left), Value::Array(right)) if left.len() == right.len() => left
            .iter()
            .zip(right)
            .enumerate()
            .find_map(|(index, (left, right))| {
                first_difference(left, right).map(|at| format!("/{index}{at}"))
            }),
        (Value::Number(left), Value::Number(right)) => {
            (!numbers_agree(left, right)).then(String::new)
        }
        _ => (read != written).then(String::new),
    }
}

fn is_empty_collection(value: &Value) -> bool {
    match value {
        Value::Array(items) => items.is_empty(),
        Value::Object(fields) => fields.is_empty(),
        _ => false,
    }
}

/// Writes `value` as JSON text, stating the first key of the object at `path`
/// twice.
pub(crate) fn write_repeating(value: &Value, path: &[Step], text: &mut String) {
    match value {
        Value::Object(fields) => {
            text.push('{');
            let repeat = path.is_empty();
            let mut first = true;
            for (index, (key, field)) in fields.iter().enumerate() {
                let times = if repeat && index == 0 { 2 } else { 1 };
                for _ in 0..times {
                    if !first {
                        text.push(',');
                    }
                    first = false;
                    text.push_str(&Value::String(key.clone()).to_string());
                    text.push(':');
                    let rest = match path.split_first() {
                        Some((Step::Key(step), rest)) if step == key => Some(rest),
                        _ => None,
                    };
                    match rest {
                        Some(rest) => write_repeating(field, rest, text),
                        None => text.push_str(&field.to_string()),
                    }
                }
            }
            text.push('}');
        }
        Value::Array(items) => {
            text.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    text.push(',');
                }
                match path.split_first() {
                    Some((Step::Index(step), rest)) if *step == index => {
                        write_repeating(item, rest, text);
                    }
                    _ => text.push_str(&item.to_string()),
                }
            }
            text.push(']');
        }
        _ => text.push_str(&value.to_string()),
    }
}

#[cfg(test)]
mod tests;
