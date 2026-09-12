// SPDX-License-Identifier: Apache-2.0
//! One unknown key, inserted into every distinct object shape of a document.
//!
//! A document-reachable type states the keys it owns. This sweep proves that
//! by shape rather than by inspection: it walks a serialized `CadIr`, dedupes
//! the object shapes it finds by normalised path and key set, inserts one
//! unknown key into each, and reports the shapes that still read back.
//!
//! The codec-private `/native` subtree is free-form by design and is skipped.

use std::collections::BTreeSet;

use cadmpeg_ir::CadIr;
use serde_json::{Map, Value};

/// The key inserted into every swept shape.
pub const UNKNOWN_KEY: &str = "zz_bogus";

/// One object shape found in a document: where it sits and what it holds.
struct Shape {
    /// Path with every array index replaced by `#`.
    normalised: String,
    /// Path to the concrete node the key is inserted into.
    concrete: Vec<Step>,
}

/// One step of a concrete JSON path.
#[derive(Clone)]
enum Step {
    /// Object member.
    Key(String),
    /// Array element.
    Index(usize),
}

/// Every distinct shape of `ir` that accepts [`UNKNOWN_KEY`], by normalised
/// path, together with the number of shapes swept.
///
/// `ir` is a serialized `CadIr`. A document that does not read back at all
/// yields no shapes and a zero count; a caller that snapshots an elided
/// `native` arena sees that and skips the document.
#[must_use]
pub fn accepting_shapes(ir: &Value) -> (BTreeSet<String>, usize) {
    if serde_json::from_value::<CadIr>(ir.clone()).is_err() {
        return (BTreeSet::new(), 0);
    }
    let mut shapes = Vec::new();
    let mut seen = BTreeSet::new();
    collect_shapes(ir, &mut Vec::new(), &mut seen, &mut shapes);
    let swept = shapes.len();
    let mut accepting = BTreeSet::new();
    for shape in shapes {
        let mut probe = ir.clone();
        insert_unknown_key(&mut probe, &shape.concrete);
        if serde_json::from_value::<CadIr>(probe).is_ok() {
            accepting.insert(shape.normalised);
        }
    }
    (accepting, swept)
}

/// Records one shape per distinct (normalised path, key set) pair.
fn collect_shapes(
    value: &Value,
    path: &mut Vec<Step>,
    seen: &mut BTreeSet<(String, Vec<String>)>,
    found: &mut Vec<Shape>,
) {
    match value {
        Value::Object(fields) => {
            let normalised = normalise(path);
            if !normalised.starts_with("/native") {
                let keys: Vec<String> = fields.keys().cloned().collect();
                if seen.insert((normalised.clone(), keys)) {
                    found.push(Shape {
                        normalised,
                        concrete: path.clone(),
                    });
                }
            }
            for (key, field) in fields {
                path.push(Step::Key(key.clone()));
                collect_shapes(field, path, seen, found);
                path.pop();
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                path.push(Step::Index(index));
                collect_shapes(item, path, seen, found);
                path.pop();
            }
        }
        _ => {}
    }
}

/// The path with every array index replaced by `#`.
fn normalise(path: &[Step]) -> String {
    let mut text = String::new();
    for step in path {
        text.push('/');
        match step {
            Step::Key(key) => text.push_str(key),
            Step::Index(_) => text.push('#'),
        }
    }
    text
}

/// Inserts the unknown key into the object the path names.
fn insert_unknown_key(value: &mut Value, path: &[Step]) {
    let mut node = value;
    for step in path {
        node = match (step, node) {
            (Step::Key(key), Value::Object(fields)) => {
                fields.get_mut(key).expect("the swept path key is live")
            }
            (Step::Index(index), Value::Array(items)) => {
                items.get_mut(*index).expect("the swept path index is live")
            }
            _ => panic!("the swept path does not match the document shape"),
        };
    }
    let fields: &mut Map<String, Value> =
        node.as_object_mut().expect("the swept node is an object");
    fields.insert(UNKNOWN_KEY.into(), Value::Bool(true));
}
