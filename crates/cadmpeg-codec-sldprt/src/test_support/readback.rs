// SPDX-License-Identifier: Apache-2.0
//! Read-back comparison for tests, independent of the writer's judgement: the
//! test decodes the written part itself, the written ids are renamed to the
//! ids of the records they pair with, and the IR diff library compares the
//! two documents arena by arena. The native namespaces, which are the
//! encoding, are left out.

use std::collections::HashMap;

use cadmpeg_ir::diff::ArenaKind;
use cadmpeg_ir::CadIr;
use serde_json::Value;

use crate::history::write::read_back::document::{pair, Document};

/// One record that two decodes of a part do not state the same way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) arena: String,
    /// The stated id; for an added record, its written id where it does not
    /// pair.
    pub(crate) id: String,
    pub(crate) change: EntryChange,
    /// An added record's content: its wire form without its own id, each
    /// unpaired written id by its digit-free form.
    pub(crate) content: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EntryChange {
    Added,
    Removed,
    /// The top-level fields that differ.
    Modified(Vec<String>),
}

/// Every record of `stated` that `written` does not state the same way.
pub(crate) fn diff(stated: &CadIr, written: &CadIr) -> Vec<Entry> {
    let left = Document::of(stated).expect("the stated document serializes");
    diff_with_left(stated, &left, written)
}

/// Every record of `stated` (with precomputed [`Document`] `left`) that
/// `written` does not state the same way.
pub(crate) fn diff_with_left(stated: &CadIr, left: &Document, written: &CadIr) -> Vec<Entry> {
    let written_neutral = written
        .to_neutral_json_value()
        .expect("the written document serializes");
    let right = Document::from_neutral_value(written, written_neutral.clone())
        .expect("the written document serializes");
    let pairing = pair(left, &right);
    let mut renames = pairing.renames().clone();
    for id in right.ids().filter(|id| id.starts_with("sldprt:")) {
        if !renames.contains_key(id) {
            renames.insert(id.to_owned(), format!("{id}:written"));
        }
    }
    let renamed = apply_edited_json(
        written,
        written_neutral,
        &mut |value| rename(value, &renames),
        &|id| renames.get(id).cloned().unwrap_or_else(|| id.to_owned()),
    );
    let diff = cadmpeg_ir::diff::diff(stated, &renamed);
    let has_added = diff
        .per_arena
        .iter()
        .any(|arena| !matches!(arena.kind, ArenaKind::Native { .. }) && !arena.added.is_empty());
    let renamed_records =
        has_added.then(|| Document::of(&renamed).expect("the renamed document serializes"));
    let mut entries = Vec::new();
    for arena in diff.per_arena {
        if matches!(arena.kind, ArenaKind::Native { .. }) {
            continue;
        }
        let name = arena.kind.to_string();
        let entry = |id: String, change| Entry {
            arena: name.clone(),
            id,
            change,
            content: None,
        };
        entries.extend(arena.added.into_iter().map(|id| {
            Entry {
                content: renamed_records
                    .as_ref()
                    .and_then(|records| records.find(&id))
                    .map(content),
                ..entry(id, EntryChange::Added)
            }
        }));
        entries.extend(
            arena
                .removed
                .into_iter()
                .map(|id| entry(id, EntryChange::Removed)),
        );
        // A native reference names a record of a native namespace, which is
        // left out.
        entries.extend(arena.modified.into_iter().filter_map(|modified| {
            let fields = modified
                .fields
                .iter()
                .filter(|field| field.as_str() != "native_ref")
                .cloned()
                .collect::<Vec<_>>();
            (!fields.is_empty()).then(|| entry(modified.id, EntryChange::Modified(fields)))
        }));
    }
    // A digest attribute digests encoded bytes; the records they encode are
    // compared instead. A configuration's stale mark is the writer's own
    // (§3.4, States), and `export:evaluation.stale` names the edits it marks.
    entries.extend(
        diff.source
            .attributes
            .into_iter()
            .filter(|change| {
                let key = change.key.as_str();
                !key.ends_with("_sha256") && !key.ends_with("_needs_update")
            })
            .map(|change| Entry {
                arena: "source.attributes".into(),
                id: change.key.to_string(),
                change: EntryChange::Modified(vec!["value".into()]),
                content: None,
            }),
    );
    entries
}

/// `record` as an added entry compares it: its wire form without its own id,
/// each unpaired written id by its digit-free form.
fn content(record: &Value) -> String {
    fn text(text: &str) -> String {
        match text.strip_suffix(":written") {
            Some(id) => id
                .chars()
                .map(|character| {
                    if character.is_ascii_digit() {
                        'N'
                    } else {
                        character
                    }
                })
                .collect(),
            None => text.to_owned(),
        }
    }
    fn shape(value: &Value) -> Value {
        match value {
            Value::String(id) => Value::String(text(id)),
            Value::Array(items) => Value::Array(items.iter().map(shape).collect()),
            Value::Object(entries) => Value::Object(
                entries
                    .iter()
                    .filter(|(key, _)| key.as_str() != "id")
                    .map(|(key, value)| (text(key), shape(value)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }
    shape(record).to_string()
}

/// The entries of `entries` that `baseline` does not hold: the same arena,
/// record and change; an added record by its content, because its written
/// id is the written part's own.
pub(crate) fn beyond(entries: Vec<Entry>, baseline: &[Entry]) -> Vec<Entry> {
    entries
        .into_iter()
        .filter(|entry| !baseline.contains(entry))
        .collect()
}

/// The entries of `entries` that no loss of `messages` names as differing,
/// missing or added: a read-back loss that names the record's id, or an
/// `arena.omitted` loss for its arena.
pub(crate) fn unnamed(entries: Vec<Entry>, messages: &[&str]) -> Vec<Entry> {
    entries
        .into_iter()
        .filter(|entry| {
            let arena = format!("arena `{}`", entry.arena);
            // A read-back loss names a record, or its arena with a count.
            let counted = [
                format!("{}: ", entry.arena),
                format!("model.{}: ", entry.arena),
                format!("geometry.{}: ", entry.arena),
            ];
            !messages.iter().any(|message| {
                (message.contains("reads back")
                    && (message.contains(&entry.id)
                        || counted.iter().any(|arena| message.contains(arena.as_str()))))
                    || (message.contains(&arena) && message.contains("omitted"))
            })
        })
        .collect()
}

/// Every string and key of `value` that `renames` names, renamed.
fn rename(value: &mut Value, renames: &HashMap<String, String>) {
    match value {
        Value::String(text) => {
            if let Some(renamed) = renames.get(text.as_str()) {
                renamed.clone_into(text);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| rename(item, renames)),
        Value::Object(entries) => {
            if entries.keys().any(|key| renames.contains_key(key.as_str())) {
                let taken = std::mem::take(entries);
                for (key, mut item) in taken {
                    rename(&mut item, renames);
                    let key = renames.get(&key).cloned().unwrap_or(key);
                    entries.insert(key, item);
                }
            } else {
                for item in entries.values_mut() {
                    rename(item, renames);
                }
            }
        }
        _ => {}
    }
}

/// What `part` reads back as when the semantic writer writes it with no
/// edit: the differences every edited write of it has as well.
pub(crate) fn baseline(part: &cadmpeg_ir::codec::DecodeResult) -> Vec<Entry> {
    baseline_with_bytes(part).0
}

/// Like [`baseline`], also returning the unedited semantic write's output bytes
/// when the semantic writer accepts the unedited part.
pub(crate) fn baseline_with_bytes(
    part: &cadmpeg_ir::codec::DecodeResult,
) -> (Vec<Entry>, Option<Vec<u8>>) {
    use std::sync::Mutex;

    type CachedBaseline = (String, Vec<Entry>, Option<Vec<u8>>);
    static BASELINES: Mutex<Vec<CachedBaseline>> = Mutex::new(Vec::new());
    let key = part
        .source_fidelity()
        .content_digest
        .as_ref()
        .expect("a decoded part has a content digest")
        .to_string();
    {
        let baselines = BASELINES.lock().expect("baseline cache");
        if let Some((_, entries, bytes)) = baselines.iter().find(|(cached, _, _)| *cached == key) {
            return (entries.clone(), bytes.clone());
        }
    }
    let part = cadmpeg_test_support::EditableDecodeResult::from(part.clone());
    let records = crate::source_records(part.ir(), part.source_fidelity())
        .expect("the part's retained records");
    let mut bytes = Vec::new();
    // A part the semantic writer cannot write unedited has no differences of
    // its own.
    let (entries, unedited_bytes) = crate::writer::write_semantic_with_records(
        part.ir(),
        &part.source_fidelity().annotations,
        &part.source_fidelity().codec_digests,
        &records,
        &[],
        &[],
        &mut bytes,
    )
    .map_or_else(
        |_| (Vec::new(), None),
        |_| {
            let written = crate::decode::decode_for_read_back_cached(&bytes)
                .expect("the unedited write decodes");
            (diff(part.ir(), written.ir()), Some(bytes))
        },
    );
    BASELINES
        .lock()
        .expect("baseline cache")
        .push((key, entries.clone(), unedited_bytes.clone()));
    (entries, unedited_bytes)
}

/// The entries of `entries` that an edit of the features `features` of
/// `stated` does not account for. The edit moves the fields `feature_fields`
/// of the features' own records, the value, expression and display of their
/// parameters, their design records, and the configurations' feature states
/// and parameter values. It adds and drops no record.
pub(crate) fn unaccounted(
    stated: &CadIr,
    features: &[&str],
    feature_fields: &[&str],
    entries: Vec<Entry>,
) -> Vec<String> {
    let owned = stated
        .model
        .parameters
        .iter()
        .filter(|parameter| {
            parameter
                .owner
                .as_ref()
                .is_some_and(|owner| features.contains(&owner.as_str()))
        })
        .map(|parameter| parameter.id.as_str().to_owned())
        .collect::<std::collections::HashSet<_>>();
    let within = |fields: &[String], allowed: &[&str]| {
        fields.iter().all(|field| allowed.contains(&field.as_str()))
    };
    entries
        .into_iter()
        .filter(|entry| {
            let EntryChange::Modified(fields) = &entry.change else {
                return true;
            };
            let own = features.contains(&entry.id.as_str());
            let parameter = owned.contains(&entry.id);
            !match entry.arena.as_str() {
                "features" => own && within(fields, feature_fields),
                "design.features" => own,
                "parameters" => parameter && within(fields, &["expression", "value", "display"]),
                "design.parameters" => parameter,
                "configurations" => within(fields, &["feature_states", "parameter_values"]),
                _ => false,
            }
        })
        .map(|entry| format!("{entry:?}"))
        .collect()
}

/// `ir` as a part written with feature renames `renames` (id, source name,
/// edited name) reads back: each record of a renamed feature states its
/// edited name, and a reference `name@feature` names the feature by it.
pub(crate) fn renamed(ir: &CadIr, renames: &[(String, String, String)]) -> CadIr {
    fn walk(value: &mut Value, renames: &[(String, String, String)]) {
        match value {
            Value::String(text) => {
                for (_, old, new) in renames {
                    *text = text.replace(&format!("@{old}"), &format!("@{new}"));
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|item| walk(item, renames)),
            Value::Object(entries) => {
                let id = entries.get("id").and_then(Value::as_str).map(str::to_owned);
                if let Some((_, old, new)) = renames
                    .iter()
                    .find(|(renamed, _, _)| id.as_deref() == Some(renamed.as_str()))
                {
                    if entries.get("name").and_then(Value::as_str) == Some(old.as_str()) {
                        entries.insert("name".into(), Value::String(new.clone()));
                    }
                }
                entries.values_mut().for_each(|item| walk(item, renames));
            }
            _ => {}
        }
    }
    edited_json(ir, &mut |value| walk(value, renames), &str::to_owned)
}

/// `ir` with `edit` applied to its JSON form and to the JSON form of each
/// arena the CADIR wire omits, which a document read from JSON does not hold,
/// with each arena in id order again after `edit` renames its ids. `id` maps a
/// feature id as `edit` changes it, so each feature-parent relation is kept.
pub(crate) fn edited_json(
    ir: &CadIr,
    edit: &mut dyn FnMut(&mut Value),
    id: &dyn Fn(&str) -> String,
) -> CadIr {
    let value = cadmpeg_core::serde_erase::to_json_value(ir).expect("the document serializes");
    apply_edited_json(ir, value, edit, id)
}

fn apply_edited_json(
    ir: &CadIr,
    mut value: Value,
    edit: &mut dyn FnMut(&mut Value),
    id: &dyn Fn(&str) -> String,
) -> CadIr {
    edit(&mut value);
    let mut edited = CadIr::from_projection(value).expect("the edited document reads");
    macro_rules! arenas {
        ($($arena:ident),*) => {$(
            if !ir.model.$arena.is_empty() {
                let mut value = cadmpeg_core::serde_erase::to_json_value(&ir.model.$arena)
                    .expect("the arena serializes");
                edit(&mut value);
                edited.model.$arena = cadmpeg_core::serde_erase::from_json_value(&value)
                    .expect("the edited arena reads");
            }
        )*};
    }
    arenas!(
        features,
        feature_input_topologies,
        feature_result_topologies,
        configurations,
        parameters,
        sketches,
        sketch_entities,
        sketch_constraints,
        spatial_sketches,
        spatial_sketch_entities,
        spatial_sketch_constraints,
        spreadsheets,
        assembly_joints,
        history_bodies
    );
    edited.finalize();
    for feature in &ir.model.features {
        if let Some(parent) = ir.model.feature_regeneration_parent(&feature.id) {
            let child = cadmpeg_ir::features::FeatureId::mint(id(feature.id.as_str()))
                .expect("an edited feature id");
            let parent = cadmpeg_ir::features::FeatureId::mint(id(parent.as_str()))
                .expect("an edited feature id");
            edited
                .model
                .set_feature_regeneration_parent(&child, &parent)
                .expect("the feature-parent relation is kept");
        }
    }
    edited
}
