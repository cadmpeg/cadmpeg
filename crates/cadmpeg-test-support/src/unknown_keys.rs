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

/// The values the key is probed with, one per JSON value kind. A node refuses
/// only when it refuses all seven: a free-form map typed by its value kind
/// accepts one spelling and refuses the others, and one probe value alone
/// cannot see that. A map whose values are arrays, for one, is visible only to
/// the `[]` probe.
fn probe_values() -> [Value; 7] {
    [
        Value::Null,
        Value::from(1),
        Value::from(1.5),
        Value::from("zz"),
        Value::Bool(true),
        Value::Object(Map::new()),
        Value::Array(Vec::new()),
    ]
}

/// One object shape found in a document: where it sits and what it holds.
struct Shape {
    /// Path with every array index replaced by `#`.
    normalised: String,
    /// Path to the concrete node the key is inserted into.
    concrete: Vec<Step>,
}

/// The identity of a shape: its normalised path, its key set, and the value of
/// every string-valued key. The tag value separates two internally tagged
/// variants that share a key set.
type ShapeIdentity = (String, Vec<String>, Vec<(String, String)>);

/// One step of a concrete JSON path.
#[derive(Clone)]
enum Step {
    /// Object member.
    Key(String),
    /// Array element.
    Index(usize),
}

/// What one sweep of a document found.
pub struct SweptShapes {
    /// The normalised path of every distinct shape that accepts
    /// [`UNKNOWN_KEY`].
    pub accepting: BTreeSet<String>,
    /// The number of distinct shapes probed.
    pub swept: usize,
    /// The number of shapes probed in the full document because their graft
    /// did not read back on its own.
    pub fallbacks: usize,
}

/// Every distinct shape of `ir` that accepts [`UNKNOWN_KEY`].
///
/// `ir` is a serialized `CadIr`. A document that does not read back at all
/// yields the serde error, which the caller accounts for; it is never a silent
/// skip.
///
/// Each shape is probed in a graft: an empty document carrying only the path
/// that reaches the shape, with the array element the shape sits in as the one
/// element of its array. Unknown-key refusal is a property of the node's
/// `Deserialize` implementation, which reads no sibling context, so the verdict
/// in the graft is the verdict in the full document. A graft that does not read
/// back on its own states a document the shape cannot be lifted out of, and that
/// shape is probed in the full document instead.
///
/// # Errors
///
/// Returns the serde error when `ir` does not read back as a `CadIr`.
pub fn accepting_shapes(ir: &Value) -> Result<SweptShapes, serde_json::Error> {
    serde_json::from_value::<CadIr>(ir.clone())?;
    let mut shapes = Vec::new();
    let mut seen = BTreeSet::new();
    collect_shapes(ir, &mut Vec::new(), &mut seen, &mut shapes);
    let swept = shapes.len();
    let empty = serde_json::to_value(CadIr::empty())?;
    let mut accepting = BTreeSet::new();
    let mut fallbacks = 0;
    for shape in shapes {
        let grafted = grafted_document(ir, &empty, &shape.concrete)
            .filter(|(graft, _)| serde_json::from_value::<CadIr>(graft.clone()).is_ok());
        let (document, path) = match grafted {
            Some(grafted) => grafted,
            None => {
                fallbacks += 1;
                (ir.clone(), shape.concrete.clone())
            }
        };
        for value in probe_values() {
            let mut probe = document.clone();
            insert_unknown_key(&mut probe, &path, value);
            if serde_json::from_value::<CadIr>(probe).is_ok() {
                accepting.insert(shape.normalised.clone());
                break;
            }
        }
    }
    Ok(SweptShapes {
        accepting,
        swept,
        fallbacks,
    })
}

/// The document one shape is probed in, with the path that reaches the shape
/// inside it.
///
/// The graft is `empty` carrying the shape's own array element, or its own
/// top-level subtree when the shape sits in no array. `None` states a path the
/// graft cannot carry.
fn grafted_document(ir: &Value, empty: &Value, path: &[Step]) -> Option<(Value, Vec<Step>)> {
    if path.is_empty() {
        return Some((empty.clone(), Vec::new()));
    }
    let mut graft = empty.clone();
    match path.iter().position(|step| matches!(step, Step::Index(_))) {
        Some(depth) => {
            let element = value_at(ir, &path[..=depth])?;
            set_member(
                &mut graft,
                &path[..depth],
                Value::Array(vec![element.clone()]),
            )?;
            let mut probe_path = path.to_vec();
            let step = probe_path.get_mut(depth)?;
            *step = Step::Index(0);
            Some((graft, probe_path))
        }
        None => {
            let subtree = value_at(ir, &path[..1])?;
            set_member(&mut graft, &path[..1], subtree.clone())?;
            Some((graft, path.to_vec()))
        }
    }
}

/// The value `path` names, or `None` when the document does not carry it.
fn value_at<'a>(value: &'a Value, path: &[Step]) -> Option<&'a Value> {
    let mut node = value;
    for step in path {
        node = match (step, node) {
            (Step::Key(key), Value::Object(fields)) => fields.get(key)?,
            (Step::Index(index), Value::Array(items)) => items.get(*index)?,
            _ => return None,
        };
    }
    Some(node)
}

/// Writes `value` at `path`, which every step of names an object member, and
/// creates the objects the path needs. `None` states a step the document
/// carries as something other than an object.
fn set_member(document: &mut Value, path: &[Step], value: Value) -> Option<()> {
    let (Step::Key(member), parents) = path.split_last()? else {
        return None;
    };
    let mut node = document;
    for step in parents {
        let Step::Key(key) = step else {
            return None;
        };
        node = node
            .as_object_mut()?
            .entry(key.clone())
            .or_insert_with(|| Value::Object(Map::new()));
    }
    node.as_object_mut()?.insert(member.clone(), value);
    Some(())
}

/// Records one shape per distinct (normalised path, key set) pair.
fn collect_shapes(
    value: &Value,
    path: &mut Vec<Step>,
    seen: &mut BTreeSet<ShapeIdentity>,
    found: &mut Vec<Shape>,
) {
    match value {
        Value::Object(fields) => {
            let normalised = normalise(path);
            if !normalised.starts_with("/native") {
                let keys: Vec<String> = fields.keys().cloned().collect();
                let tags: Vec<(String, String)> = fields
                    .iter()
                    .filter_map(|(key, field)| {
                        field.as_str().map(|text| (key.clone(), text.to_owned()))
                    })
                    .collect();
                if seen.insert((normalised.clone(), keys, tags)) {
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
fn insert_unknown_key(value: &mut Value, path: &[Step], probe: Value) {
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
    fields.insert(UNKNOWN_KEY.into(), probe);
}
