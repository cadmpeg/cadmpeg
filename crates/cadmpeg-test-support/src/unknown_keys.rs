// SPDX-License-Identifier: Apache-2.0
//! One unknown key, inserted into every non-native object of a document.
//!
//! A document-reachable type states the keys it owns. This sweep proves that
//! by reading each object through its document route. Objects with the same
//! keys can use different readers selected by an enclosing variant.
//!
//! The codec-private `/native` subtree is free-form by design and is skipped.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::{LazyLock, Mutex, PoisonError};

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
pub(crate) fn probe_values() -> [Value; 7] {
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
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Step {
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
    /// The number of objects swept.
    pub swept: usize,
    /// The number of shapes probed in the full document because neither
    /// their array element nor their top-level member reads back on its own.
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
/// back on its own states an element its document checks against other
/// records. That shape is probed in the graft of its whole top-level member,
/// and in the full document when that graft does not read back either.
///
/// A document text with the key at one path always reads the same way. Each
/// such pair is read once in this process, so a graft that two documents
/// share is probed for the first document only.
///
/// # Errors
///
/// Returns the serde error when `ir` does not read back as a `CadIr`.
pub fn accepting_shapes(ir: &Value) -> Result<SweptShapes, serde_json::Error> {
    CadIr::deserialize(ir)?;
    let empty = cadmpeg_ir::hash::finite_json::to_json_value(&CadIr::empty())?;
    let mut probed = PROBED.lock().unwrap_or_else(PoisonError::into_inner);
    sweep(
        ir,
        &empty,
        |document| CadIr::deserialize(document).is_ok(),
        &mut probed,
    )
}

/// The documents [`accepting_shapes`] has read in this process.
static PROBED: LazyLock<Mutex<Probed>> = LazyLock::new(Mutex::default);

/// The documents a sweep has read, by their text.
#[derive(Default)]
struct Probed {
    /// The documents that do not read back.
    unread: HashSet<String>,
    /// The documents that read back, each with the probed paths and whether
    /// the object at each path accepts [`UNKNOWN_KEY`].
    read: HashMap<String, HashMap<Vec<Step>, bool>>,
}

impl Probed {
    /// Whether the document stated by `text` reads back, read through `read`
    /// the first time only.
    fn reads_back(&mut self, text: &str, read: impl FnOnce() -> bool) -> bool {
        if self.read.contains_key(text) {
            return true;
        }
        if self.unread.contains(text) {
            return false;
        }
        if read() {
            self.read.insert(text.to_owned(), HashMap::new());
            true
        } else {
            self.unread.insert(text.to_owned());
            false
        }
    }
}

fn sweep(
    ir: &Value,
    empty: &Value,
    accepts: impl Fn(&Value) -> bool,
    probed: &mut Probed,
) -> Result<SweptShapes, serde_json::Error> {
    let mut shapes = Vec::new();
    collect_shapes(ir, &mut Vec::new(), &mut shapes);
    let mut origins = BTreeMap::<&[Step], Vec<&[Step]>>::new();
    for path in shapes.iter().map(Vec::as_slice) {
        origins.entry(origin(path)).or_default().push(path);
    }
    let mut accepting = BTreeSet::new();
    let mut fallbacks = 0;
    for paths in origins.values() {
        let Lifted {
            document,
            text,
            rebased,
            whole,
        } = lift(ir, empty, paths[0], &accepts, probed);
        if whole {
            fallbacks += paths.len();
        }
        let outcomes = probed.read.entry(text).or_default();
        for path in paths {
            let probe_path = if rebased { rebase(path) } else { path.to_vec() };
            let accepted = match outcomes.get(&probe_path) {
                Some(accepted) => *accepted,
                None => {
                    let accepted = accepts_key(&document, &probe_path, &accepts)?;
                    outcomes.insert(probe_path, accepted);
                    accepted
                }
            };
            if accepted {
                accepting.insert(normalise(path));
            }
        }
    }
    Ok(SweptShapes {
        accepting,
        swept: shapes.len(),
        fallbacks,
    })
}

/// The part of `path` that selects its graft: the steps through its first
/// array index, or its first step when it holds no index.
fn origin(path: &[Step]) -> &[Step] {
    let end = path
        .iter()
        .position(|step| matches!(step, Step::Index(_)))
        .map_or(1, |depth| depth + 1);
    &path[..end.min(path.len())]
}

/// `path` with its first array index set to 0, the path that reaches the
/// shape in its element graft.
fn rebase(path: &[Step]) -> Vec<Step> {
    let mut path = path.to_vec();
    if let Some(step) = path.iter_mut().find(|step| matches!(step, Step::Index(_))) {
        *step = Step::Index(0);
    }
    path
}

/// The document the shapes of one origin are probed in.
struct Lifted {
    document: Value,
    /// The document's text, which keys its probes in [`Probed`].
    text: String,
    /// Whether the document is the element graft, which moves the shapes'
    /// array element to index 0.
    rebased: bool,
    /// Whether the document is the full document.
    whole: bool,
}

/// The first document of the shape at `path` that reads back: its element
/// graft, the graft of its top-level member, or else `ir` itself.
fn lift(
    ir: &Value,
    empty: &Value,
    path: &[Step],
    accepts: &impl Fn(&Value) -> bool,
    probed: &mut Probed,
) -> Lifted {
    let element = grafted_document(ir, empty, path).map(|(graft, _)| (graft, true));
    let member = std::iter::once_with(|| member_graft(ir, empty, path))
        .flatten()
        .map(|graft| (graft, false));
    for (graft, rebased) in element.into_iter().chain(member) {
        let text = graft.to_string();
        if probed.reads_back(&text, || accepts(&graft)) {
            return Lifted {
                document: graft,
                text,
                rebased,
                whole: false,
            };
        }
    }
    Lifted {
        document: ir.clone(),
        text: ir.to_string(),
        rebased: false,
        whole: true,
    }
}

/// Whether the object at `path` of `document` reads back with
/// [`UNKNOWN_KEY`] set to any of the probe values.
fn accepts_key(
    document: &Value,
    path: &[Step],
    accepts: &impl Fn(&Value) -> bool,
) -> Result<bool, serde_json::Error> {
    let mut document = document.clone();
    let key = probe_key(object_at_mut(&mut document, path)?);
    // Keep every value probe independent by replacing the one key this
    // sweep added. The callback only borrows the document, so no other
    // field can carry state from the preceding value.
    for value in probe_values() {
        object_at_mut(&mut document, path)?.insert(key.clone(), value);
        if accepts(&document) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Return a sentinel key that is absent from the object being probed.
///
/// Source maps may legally contain the default sentinel as an ordinary key.
/// Choosing a stable suffix in that case keeps the existing member intact and
/// still inserts a key the surrounding typed object does not declare.
pub(crate) fn probe_key(fields: &Map<String, Value>) -> String {
    let mut candidate = UNKNOWN_KEY.to_owned();
    while fields.contains_key(&candidate) {
        candidate.push('_');
    }
    candidate
}

/// The document one shape is probed in, with the path that reaches the shape
/// inside it.
///
/// The graft is `empty` carrying the shape's own array element, or its own
/// top-level subtree when the shape sits in no array. `None` states a path the
/// graft cannot carry.
pub(crate) fn grafted_document(
    ir: &Value,
    empty: &Value,
    path: &[Step],
) -> Option<(Value, Vec<Step>)> {
    if path.is_empty() {
        return Some((ir.clone(), Vec::new()));
    }
    match path.iter().position(|step| matches!(step, Step::Index(_))) {
        Some(depth) => {
            let element = value_at(ir, &path[..=depth])?;
            let mut graft = empty.clone();
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
        None => member_graft(ir, empty, path).map(|graft| (graft, path.to_vec())),
    }
}

/// `empty` carrying the top-level member of `ir` that `path` starts in. The
/// path that reaches the shape inside it is `path` itself. `None` states the
/// empty path.
pub(crate) fn member_graft(ir: &Value, empty: &Value, path: &[Step]) -> Option<Value> {
    let member = path.get(..1)?;
    let mut graft = empty.clone();
    set_member(&mut graft, member, value_at(ir, member)?.clone())?;
    Some(graft)
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

/// The path with every array index replaced by `#` and every key that is an
/// entity id, a key of a map keyed by id, replaced by `{id}`.
pub(crate) fn normalise(path: &[Step]) -> String {
    let mut text = String::new();
    for step in path {
        text.push('/');
        match step {
            Step::Key(key) if is_entity_id(key) => text.push_str("{id}"),
            Step::Key(key) => text.push_str(&key.replace('~', "~0").replace('/', "~1")),
            Step::Index(_) => text.push('#'),
        }
    }
    text
}

/// Whether `key` has the `<format>:<scope>:<kind>#<key>` form of an entity id.
fn is_entity_id(key: &str) -> bool {
    key.split_once('#').is_some_and(|(head, tail)| {
        !tail.is_empty() && head.split(':').filter(|part| !part.is_empty()).count() >= 3
    })
}

pub(crate) fn object_at_mut<'a>(
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
/// * `/model/drawings/#/parameters` - `Drawing::parameters`, retained drawing
///   parameters by source name;
/// * `/model/drawings/#/relationships` - `Drawing::relationships`,
///   `BTreeMap<String, Vec<ReferenceSelection>>`, the source's relationship
///   roles by name, each holding the references that role states;
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
    "/model/drawings/#/parameters",
    "/model/drawings/#/relationships",
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
