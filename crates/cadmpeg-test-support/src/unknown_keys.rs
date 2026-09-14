// SPDX-License-Identifier: Apache-2.0
//! One unknown key, inserted into every non-native object of a document.
//!
//! A document-reachable type states the keys it owns. This sweep proves that
//! by reading each object through its document route. Objects with the same
//! keys can use different readers selected by an enclosing variant.
//!
//! The codec-private `/native` subtree is free-form by design and is skipped.

use std::collections::BTreeSet;

use cadmpeg_ir::CadIr;
use serde::Deserialize;
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
    /// The number of objects probed.
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
/// Each object is probed in a graft: an empty document carrying only the path
/// that reaches the shape, with the array element the shape sits in as the one
/// element of its array. Unknown-key refusal is a property of the node's
/// `Deserialize` implementation. The graft preserves the complete containing
/// array element, including its enclosing tags. A graft that does not read
/// back on its own states a document the shape cannot be lifted out of, and that
/// shape is probed in the full document instead.
///
/// # Errors
///
/// Returns the serde error when `ir` does not read back as a `CadIr`.
pub fn accepting_shapes(ir: &Value) -> Result<SweptShapes, serde_json::Error> {
    CadIr::deserialize(ir)?;
    let empty = serde_json::to_value(CadIr::empty())?;
    sweep(ir, &empty, |document| CadIr::deserialize(document).is_ok())
}

fn sweep(
    ir: &Value,
    empty: &Value,
    accepts: impl Fn(&Value) -> bool,
) -> Result<SweptShapes, serde_json::Error> {
    let mut shapes = Vec::new();
    collect_shapes(ir, &mut Vec::new(), &mut shapes);
    let swept = shapes.len();
    let mut accepting = BTreeSet::new();
    let mut fallbacks = 0;
    for concrete in shapes {
        let grafted = grafted_document(ir, empty, &concrete).filter(|(graft, _)| accepts(graft));
        let (mut document, path) = match grafted {
            Some(grafted) => grafted,
            None => {
                fallbacks += 1;
                (ir.clone(), concrete)
            }
        };
        for value in probe_values() {
            object_at_mut(&mut document, &path)?.insert(UNKNOWN_KEY.into(), value);
            if accepts(&document) {
                accepting.insert(normalise(&path));
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
        return Some((ir.clone(), Vec::new()));
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

/// Records every object, including equal key sets under different readers.
fn collect_shapes(value: &Value, path: &mut Vec<Step>, found: &mut Vec<Vec<Step>>) {
    match value {
        Value::Object(fields) => {
            if matches!(path.first(), Some(Step::Key(key)) if key == "native") {
                return;
            }
            found.push(path.clone());
            for (key, field) in fields {
                path.push(Step::Key(key.clone()));
                collect_shapes(field, path, found);
                path.pop();
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                path.push(Step::Index(index));
                collect_shapes(item, path, found);
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
            Step::Key(key) => text.push_str(&key.replace('~', "~0").replace('/', "~1")),
            Step::Index(_) => text.push('#'),
        }
    }
    text
}

fn object_at_mut<'a>(
    value: &'a mut Value,
    path: &[Step],
) -> Result<&'a mut Map<String, Value>, serde_json::Error> {
    let mut node = value;
    let missing = || {
        <serde_json::Error as serde::de::Error>::custom(format!(
            "unknown-key probe target {} is not an object in its document",
            normalise(path)
        ))
    };
    for step in path {
        let next = match (step, node) {
            (Step::Key(key), Value::Object(fields)) => fields.get_mut(key),
            (Step::Index(index), Value::Array(items)) => items.get_mut(*index),
            _ => None,
        };
        let Some(next) = next else {
            return Err(missing());
        };
        node = next;
    }
    node.as_object_mut().ok_or_else(missing)
}

/// The non-native nodes a document may accept an unknown key on.
///
/// Each is a free-form map whose keys are source names, so it declares no key
/// set for a deny to bind. Every owner declares `deny_unknown_fields`, and that
/// deny is live one level up; the map itself is the node the source fills.
/// Probing every JSON value kind is what makes them visible: a
/// `BTreeMap<String, String>` refuses `true` and accepts `"zz"`, and a map
/// whose values are arrays accepts `[]` alone, so a narrower probe list reads
/// such a node as refusing.
///
/// * `/model/appearance_bindings/#/channels` - `AppearanceBinding::channels`,
///   `BTreeMap<String, String>` of source channel names;
/// * `/model/appearances/#/properties` - `Appearance::properties`,
///   `BTreeMap<String, f64>` of source renderer properties;
/// * `/model/configurations/#/properties` - `DesignConfiguration::properties`,
///   configuration-local named values;
/// * `/model/drawings/#/parameters` - `Drawing::parameters`, retained drawing
///   parameters by source name;
/// * `/model/drawings/#/relationships` - `Drawing::relationships`,
///   `BTreeMap<String, Vec<ReferenceSelection>>`, the source's relationship
///   roles by name, each holding the references that role states;
/// * `/model/features/#/definition/parameters` -
///   `FeatureOperation::Native::parameters`, the native operation's own fields;
/// * `/model/features/#/source_properties` - `Feature::source_properties`,
///   source operation attributes the neutral definition does not consume;
/// * `/model/parameters/#/properties` - `Parameter::properties`, source
///   parameter properties no other field represents;
/// * `/source/attributes` - `SourceMeta::attributes`, format-specific
///   attributes;
/// * `/model/presentation_documents/#/states/#/attributes` and
///   `/model/presentation_documents/#/states/#/kind/value/properties` -
///   `PresentationState::attributes` and the view state's `properties`;
/// * `/model/product_definitions/#/bom_properties` -
///   `ProductDefinition::bom_properties`, the source's bill-of-materials
///   fields;
/// * `/model/semantic_annotations/#/parameters` -
///   `SemanticAnnotation::parameters`, the native note's own fields;
/// * `/model/semantic_annotations/#/references` -
///   `SemanticAnnotation::references`, `BTreeMap<String,
///   Vec<ReferenceSelection>>`, the native note's reference roles by name;
/// * `/model/view_presentations/#/properties` - `ViewPresentation::properties`;
/// * `/source/identity/dialects/primary/declared` and
///   `/source/identity/dialects/extra/#/declared` - `DialectMatch::declared`,
///   the version fields the source declared verbatim.
///
/// Every other accepting node is a failure.
pub const FREE_FORM_SHAPES: &[&str] = &[
    "/model/appearance_bindings/#/channels",
    "/model/appearances/#/properties",
    "/model/configurations/#/properties",
    "/model/drawings/#/parameters",
    "/model/drawings/#/relationships",
    "/model/features/#/definition/parameters",
    "/model/features/#/source_properties",
    "/model/parameters/#/properties",
    "/model/presentation_documents/#/states/#/attributes",
    "/model/presentation_documents/#/states/#/kind/value/properties",
    "/model/product_definitions/#/bom_properties",
    "/model/semantic_annotations/#/parameters",
    "/model/semantic_annotations/#/references",
    "/model/view_presentations/#/properties",
    "/source/attributes",
    "/source/identity/dialects/extra/#/declared",
    "/source/identity/dialects/primary/declared",
];

#[cfg(test)]
mod tests;
