// SPDX-License-Identifier: Apache-2.0
//! What the full decoder reads back from a written part: each construction
//! edit the write claims, and every other feature and parameter of the part.
//! A write whose part reads back changed where no edit touches it is written
//! again with the edited features' stored records.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    FeatureDefinition, FeatureId, FeatureOperation, ParameterId, SketchFeatureBinding,
};
use serde_json::Value;

use super::edits::{listed, ConstructionLeaf, FeatureClaims};

/// The construction edits of a write, and what the rest of the caller's
/// document must read back as.
#[derive(Default)]
pub(crate) struct ReadBackClaims {
    pub(super) features: Vec<FeatureClaims>,
    /// Features that state their source feature in every field but the name,
    /// under the same tree parent, and own no parameter an edit affects: each
    /// reads back as the document states it.
    pub(super) unedited: HashSet<FeatureId>,
    /// Parameters the caller edited, and each parameter whose dependencies
    /// reach one: the parameter edit judges their read-back.
    pub(super) affected_parameters: HashSet<ParameterId>,
}

/// What the full decoder reads back from a written part for the claims of
/// the write.
#[derive(Default)]
pub(crate) struct ReadBack {
    /// Features to write again with their stored definition record, and why:
    /// an edit the write calls written does not read back as edited, or the
    /// part reads back changed where no edit touches it.
    pub(crate) withhold: Vec<(FeatureId, String)>,
    /// Edits called unwritten that read back as edited.
    pub(super) carried: HashSet<(FeatureId, String)>,
    /// Edits called unwritten that read back as neither the source nor the
    /// edit, by feature.
    pub(super) neither: Vec<(FeatureId, Vec<String>)>,
    /// Records that read back other than the document states while no
    /// written edit is left to keep.
    pub(super) changed: Vec<String>,
}

/// The relative difference up to which two real numbers read back the same:
/// a millimetre value stored in metres moves in its last bits.
const READ_BACK_RELATIVE_TOLERANCE: f64 = 1.0e-9;

/// Whether `left` and `right` are equal up to the read-back tolerance on each
/// real number; with `ids_move`, every two native ids count as equal.
pub(crate) fn same_read_back(left: &Value, right: &Value, ids_move: bool) -> bool {
    match (left, right) {
        (Value::Number(left), Value::Number(right)) => match (left.as_f64(), right.as_f64()) {
            (Some(left), Some(right)) => {
                (left - right).abs()
                    <= READ_BACK_RELATIVE_TOLERANCE * left.abs().max(right.abs()).max(1.0)
            }
            _ => left == right,
        },
        (Value::String(left), Value::String(right)) if ids_move => {
            left == right || (left.starts_with("sldprt:") && right.starts_with("sldprt:"))
        }
        (Value::Array(left), Value::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| same_read_back(left, right, ids_move))
        }
        (Value::Object(left), Value::Object(right)) => {
            left.len() == right.len()
                && left.iter().all(|(key, value)| {
                    right
                        .get(key)
                        .is_some_and(|other| same_read_back(value, other, ids_move))
                })
        }
        _ => left == right,
    }
}

/// Sets the place `pointer` of `value` to `leaf`, or removes it for `None`.
fn put(value: &mut Value, pointer: &str, leaf: Option<Value>) {
    let Some((parent, key)) = pointer.rsplit_once('/') else {
        if let Some(leaf) = leaf {
            *value = leaf;
        }
        return;
    };
    let key = key.replace("~1", "/").replace("~0", "~");
    match (value.pointer_mut(parent), leaf) {
        (Some(Value::Object(fields)), Some(leaf)) => {
            fields.insert(key, leaf);
        }
        (Some(Value::Object(fields)), None) => {
            fields.remove(&key);
        }
        (Some(Value::Array(items)), Some(leaf)) => {
            if let Some(slot) = key
                .parse::<usize>()
                .ok()
                .and_then(|index| items.get_mut(index))
            {
                *slot = leaf;
            }
        }
        _ => {}
    }
}

/// The reason a feature whose written edit does not read back as edited is
/// written again with its stored record.
fn unread(id: &FeatureId) -> String {
    format!("SLDPRT feature {id} does not read back as edited from records that state its edit")
}

impl ReadBack {
    /// Judges `claims` against `bytes`, the written part of the caller's
    /// document `input`, decoded with the full decoder. A claim that asks to
    /// withhold its feature needs no decode: that part is written again.
    pub(crate) fn of_written(input: &CadIr, claims: &ReadBackClaims, bytes: &[u8]) -> Self {
        use cadmpeg_ir::codec::{Codec, DecodeOptions};

        if claims.features.is_empty() {
            return Self::default();
        }
        let withhold = claims
            .features
            .iter()
            .filter(|claim| claim.withhold)
            .map(|claim| (claim.id.clone(), unread(&claim.id)))
            .collect::<Vec<_>>();
        if !withhold.is_empty() {
            return Self {
                withhold,
                ..Self::default()
            };
        }
        let decoded = crate::SldprtCodec
            .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
            .ok();
        let written = decoded.as_ref().map(cadmpeg_ir::codec::DecodeResult::ir);
        let mut read_back = Self::of_claims(&claims.features, written);
        if let Some(written) = written {
            read_back.judge_part(input, claims, written);
        }
        read_back
    }

    /// Judges `claims`, none of which asks to withhold its feature, against
    /// `written`, the written part decoded; `None` when it does not decode.
    fn of_claims(claims: &[FeatureClaims], written: Option<&CadIr>) -> Self {
        let mut read_back = Self::default();
        for claim in claims {
            let read = written
                .and_then(|written| claim.read(written))
                .and_then(|read| {
                    Some((
                        read,
                        serde_json::to_value(read.evaluation.definition()).ok()?,
                    ))
                });
            let Some((read, read_definition)) = read else {
                if claim.any_written() {
                    read_back
                        .withhold
                        .push((claim.id.clone(), unread(&claim.id)));
                }
                continue;
            };
            let mut expected = claim.source_definition.clone();
            let mut failed = false;
            let mut neither = Vec::new();
            for edit in &claim.edits {
                let got = match &edit.leaf {
                    ConstructionLeaf::Definition(pointer) => {
                        let got = read_definition.pointer(pointer).cloned();
                        put(&mut expected, pointer, got.clone());
                        got.unwrap_or(Value::Null)
                    }
                    ConstructionLeaf::SourceProperty(key) => read
                        .source_properties
                        .get(key)
                        .map_or(Value::Null, |value| Value::String(value.clone())),
                    ConstructionLeaf::Dependencies => continue,
                };
                let as_edited = same_read_back(&got, &edit.edited, true);
                if edit.written {
                    failed |= !as_edited;
                } else if !same_read_back(&got, &edit.source, true) {
                    if as_edited {
                        read_back
                            .carried
                            .insert((claim.id.clone(), edit.label.clone()));
                    } else {
                        neither.push(edit.label.clone());
                    }
                }
            }
            if !same_read_back(&expected, &read_definition, true) {
                failed = true;
                neither.push("the rest of its definition".into());
            }
            if failed && claim.any_written() {
                read_back
                    .withhold
                    .push((claim.id.clone(), unread(&claim.id)));
            } else if !neither.is_empty() {
                read_back.neither.push((claim.id.clone(), neither));
            }
        }
        read_back
    }

    /// Compares every feature and parameter of `written` with the caller's
    /// document `input`. A change of a claimed feature beyond its edits
    /// withholds that feature; a change of a record no edit touches withholds
    /// every feature with a written edit. With nothing left to withhold, each
    /// change is named.
    fn judge_part(&mut self, input: &CadIr, claims: &ReadBackClaims, written: &CadIr) {
        let stated = PartSummary::of(input);
        let read = PartSummary::of(written);
        let claimed = claims
            .features
            .iter()
            .filter_map(|claim| Some((stated.feature_keys.get(&claim.id)?.as_str(), claim)))
            .collect::<HashMap<_, _>>();
        let mut claim_changes = BTreeMap::<&str, Vec<String>>::new();
        let mut unclaimed = Vec::new();
        for (key, value) in &stated.features {
            let got = read.features.get(key);
            if let Some(claim) = claimed.get(key.as_str()) {
                // A claimed feature that does not read back by name is judged
                // by its claims, which match it by ordinal as well.
                if let Some(got) = got {
                    let changes = claimed_changes(claim, value, got);
                    if !changes.is_empty() {
                        claim_changes.entry(key).or_default().extend(changes);
                    }
                }
            } else if stated
                .feature_ids
                .get(key)
                .is_some_and(|id| claims.unedited.contains(id))
                && got.is_none_or(|got| !same_read_back(value, got, false))
            {
                unclaimed.push(label(key));
            }
        }
        unclaimed.extend(
            read.features
                .keys()
                .filter(|key| !stated.features.contains_key(*key))
                .map(|key| format!("{} (added)", label(key))),
        );
        for (key, value) in &stated.parameters {
            let Some(parameter) = stated.parameter_meta.get(key) else {
                continue;
            };
            if claims.affected_parameters.contains(&parameter.id) {
                continue;
            }
            let got = read.parameters.get(key);
            let owner = parameter
                .owner
                .as_deref()
                .and_then(|owner| claimed.get_key_value(owner));
            if owner.is_some_and(|(_, claim)| claim.reshapes()) {
                continue;
            }
            match (owner.map(|(owner, _)| *owner), got) {
                (Some(owner), None) => claim_changes
                    .entry(owner)
                    .or_default()
                    .push(format!("its parameter {} missing", parameter.name)),
                (Some(owner), Some(got)) => {
                    if !same_except(value, got, &["expression", "value", "display"]) {
                        claim_changes
                            .entry(owner)
                            .or_default()
                            .push(format!("its parameter {} changed", parameter.name));
                    }
                }
                (None, got) => {
                    if got.is_none_or(|got| !same_read_back(value, got, false)) {
                        unclaimed.push(label(key));
                    }
                }
            }
        }
        for key in read
            .parameters
            .keys()
            .filter(|key| !stated.parameters.contains_key(*key))
        {
            let Some(parameter) = read.parameter_meta.get(key) else {
                continue;
            };
            match parameter
                .owner
                .as_deref()
                .and_then(|owner| claimed.get_key_value(owner))
            {
                Some((_, claim)) if claim.reshapes() => {}
                Some((owner, _)) => claim_changes
                    .entry(owner)
                    .or_default()
                    .push(format!("its parameter {} added", parameter.name)),
                None => unclaimed.push(format!("{} (added)", label(key))),
            }
        }
        for (key, changes) in claim_changes {
            let Some(claim) = claimed.get(key) else {
                continue;
            };
            if claim.any_written() {
                if !self.withholds(&claim.id) {
                    self.withhold.push((
                        claim.id.clone(),
                        format!(
                            "SLDPRT feature {} reads back from records that state its edit with {}",
                            claim.id,
                            listed(&changes)
                        ),
                    ));
                }
            } else {
                self.changed
                    .push(format!("{} ({})", label(key), listed(&changes)));
            }
        }
        if unclaimed.is_empty() || !self.withhold.is_empty() {
            // A feature withheld in this pass may be what changed the rest:
            // the next pass judges the part again.
            return;
        }
        let written = claims
            .features
            .iter()
            .filter(|claim| claim.any_written())
            .collect::<Vec<_>>();
        if written.is_empty() {
            self.changed.extend(unclaimed);
            return;
        }
        for claim in written {
            self.withhold.push((
                claim.id.clone(),
                format!(
                    "SLDPRT feature {} written as edited changes {} on read-back, which no edit touches",
                    claim.id,
                    listed(&unclaimed)
                ),
            ));
        }
    }

    fn withholds(&self, id: &FeatureId) -> bool {
        self.withhold.iter().any(|(withheld, _)| withheld == id)
    }
}

/// The changes of a claimed feature beyond its edits: its suppression, tree
/// parent, source tag, unedited dependencies, and, for value edits, each
/// unedited source property that no longer reads back. The definition is
/// judged by the claims; property values and content mirror the edited
/// records, and an edit that reshapes the definition reshapes its record.
fn claimed_changes(claim: &FeatureClaims, stated: &Value, got: &Value) -> Vec<String> {
    let mut fields = vec![
        ("suppressed", "suppression"),
        ("parent", "tree parent"),
        ("source_tag", "source tag"),
    ];
    if !claim.dependencies_edited {
        fields.push(("dependencies", "dependencies"));
    }
    let mut changes = fields
        .into_iter()
        .filter(|(field, _)| {
            !same_read_back(
                stated.get(*field).unwrap_or(&Value::Null),
                got.get(*field).unwrap_or(&Value::Null),
                false,
            )
        })
        .map(|(_, name)| format!("its {name} changed"))
        .collect::<Vec<_>>();
    if claim.reshapes() {
        return changes;
    }
    if let (Some(Value::Object(stated)), Some(Value::Object(got))) = (
        stated.get("source_properties"),
        got.get("source_properties"),
    ) {
        changes.extend(
            stated
                .keys()
                .filter(|key| !got.contains_key(*key) && !claim.edits_source_property(key))
                .map(|key| format!("its source property {key} missing")),
        );
    }
    changes
}

/// Whether two objects agree on every field but `skipped`.
fn same_except(stated: &Value, got: &Value, skipped: &[&str]) -> bool {
    let (Value::Object(stated), Value::Object(got)) = (stated, got) else {
        return same_read_back(stated, got, false);
    };
    stated
        .keys()
        .chain(got.keys())
        .filter(|key| !skipped.contains(&key.as_str()))
        .all(|key| {
            same_read_back(
                stated.get(key).unwrap_or(&Value::Null),
                got.get(key).unwrap_or(&Value::Null),
                false,
            )
        })
}

/// What a loss calls the record of summary key `key`.
fn label(key: &str) -> String {
    if let Some(name) = key.strip_prefix("P:F:") {
        format!("parameter {name}")
    } else if let Some(name) = key.strip_prefix("P:?.") {
        format!("parameter {name}")
    } else if let Some(name) = key.strip_prefix("F:") {
        format!("feature {name}")
    } else {
        key.to_owned()
    }
}

/// One parameter of a summary: its id, owner feature key and name.
pub(crate) struct ParameterMeta {
    id: ParameterId,
    owner: Option<String>,
    name: String,
}

/// The features and parameters of a document keyed by name, because ids move
/// across a write: features as `F:<name>`, parameters as
/// `P:F:<owner name>.<name>`, a repeated key with `#<n>` in history order.
/// Every id a record states is replaced by the key of what it names.
pub(crate) struct PartSummary {
    pub(crate) features: BTreeMap<String, Value>,
    pub(crate) parameters: BTreeMap<String, Value>,
    feature_ids: BTreeMap<String, FeatureId>,
    feature_keys: HashMap<FeatureId, String>,
    parameter_meta: BTreeMap<String, ParameterMeta>,
}

/// `key`, or `key#<n>` for its `n`th use in `seen`.
fn counted(key: String, seen: &mut HashMap<String, usize>) -> String {
    let count = seen.entry(key.clone()).or_default();
    *count += 1;
    if *count == 1 {
        key
    } else {
        format!("{key}#{count}")
    }
}

impl PartSummary {
    pub(crate) fn of(ir: &CadIr) -> Self {
        let mut features = ir.model.features.iter().collect::<Vec<_>>();
        features.sort_by_key(|feature| feature.ordinal);
        let mut names = HashMap::<String, String>::new();
        let mut feature_ids = BTreeMap::new();
        let mut feature_keys = HashMap::new();
        let mut seen = HashMap::new();
        for feature in &features {
            let name = feature.name.clone().unwrap_or_else(|| "<unnamed>".into());
            let key = format!("F:{}", counted(name, &mut seen));
            names.insert(feature.id.as_str().to_owned(), key.clone());
            feature_ids.insert(key.clone(), feature.id.clone());
            feature_keys.insert(feature.id.clone(), key);
        }
        let mut parameter_meta = BTreeMap::new();
        let mut parameter_keys = Vec::new();
        let mut seen = HashMap::new();
        for parameter in &ir.model.parameters {
            let owner = parameter
                .owner
                .as_ref()
                .and_then(|owner| names.get(owner.as_str()))
                .cloned();
            let key = counted(
                format!("P:{}.{}", owner.as_deref().unwrap_or("?"), parameter.name),
                &mut seen,
            );
            names.insert(parameter.id.as_str().to_owned(), key.clone());
            parameter_keys.push(key.clone());
            parameter_meta.insert(
                key,
                ParameterMeta {
                    id: parameter.id.clone(),
                    owner,
                    name: parameter.name.clone(),
                },
            );
        }
        for feature in &features {
            if let FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: SketchFeatureBinding::Planar(Some(sketch)),
            }) = feature.evaluation.definition()
            {
                if let Some(owner) = names.get(feature.id.as_str()).cloned() {
                    names.insert(sketch.as_str().to_owned(), format!("S:{owner}"));
                }
            }
        }
        let wire = |value: Result<Value, serde_json::Error>| value.unwrap_or(Value::Null);
        let features = features
            .iter()
            .filter_map(|feature| {
                let value = serde_json::json!({
                    "definition": wire(serde_json::to_value(feature.evaluation.definition())),
                    "suppressed": feature.suppressed,
                    "parent": ir.model.feature_parent(&feature.id).map(FeatureId::as_str),
                    "dependencies": feature
                        .dependencies
                        .iter()
                        .map(FeatureId::as_str)
                        .collect::<Vec<_>>(),
                    "source_tag": feature.source_tag,
                    "source_properties": wire(serde_json::to_value(&feature.source_properties)),
                    "source_content": feature.source_content.len(),
                });
                Some((
                    names.get(feature.id.as_str())?.clone(),
                    canonical(value, &names),
                ))
            })
            .collect();
        let parameters = ir
            .model
            .parameters
            .iter()
            .zip(parameter_keys)
            .map(|(parameter, key)| {
                let value = serde_json::json!({
                    "expression": parameter.expression,
                    "value": wire(serde_json::to_value(&parameter.value)),
                    "display": wire(serde_json::to_value(parameter.display)),
                    "properties": wire(serde_json::to_value(&parameter.properties)),
                    "bound": parameter.native_ref.is_some(),
                    "dependencies": parameter
                        .dependencies
                        .iter()
                        .map(ParameterId::as_str)
                        .collect::<Vec<_>>(),
                });
                (key, canonical(value, &names))
            })
            .collect();
        Self {
            features,
            parameters,
            feature_ids,
            feature_keys,
            parameter_meta,
        }
    }
}

/// Replaces each id by the key of what it names, and numbers the block and
/// lane offsets that a native token embeds, which a write moves.
fn canonical(value: Value, names: &HashMap<String, String>) -> Value {
    match value {
        Value::String(text) => Value::String(canonical_text(text, names)),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| canonical(item, names))
                .collect(),
        ),
        Value::Object(entries) => Value::Object(
            entries
                .into_iter()
                .map(|(key, value)| (canonical_text(key, names), canonical(value, names)))
                .collect(),
        ),
        other => other,
    }
}

/// `text` with an id replaced by its key. A native token keeps what precedes
/// its `#`; without one, each run of four or more digits between a `:` or `,`
/// and a `:` or the end is an offset, numbered by first appearance so that
/// two references to one offset stay one.
fn canonical_text(text: String, names: &HashMap<String, String>) -> String {
    if let Some(name) = names.get(&text) {
        return name.clone();
    }
    if !text.starts_with("sldprt:") {
        return text;
    }
    if let Some((prefix, _)) = text.split_once('#') {
        return format!("{prefix}#");
    }
    let mut offsets = Vec::<&str>::new();
    let mut out = String::with_capacity(text.len());
    let mut after_separator = false;
    for piece in text.split_inclusive([':', ',']) {
        let (segment, separator) = match piece.char_indices().last() {
            Some((at, separator @ (':' | ','))) => (&piece[..at], Some(separator)),
            _ => (piece, None),
        };
        let offset = after_separator
            && matches!(separator, Some(':') | None)
            && segment.len() >= 4
            && segment.bytes().all(|byte| byte.is_ascii_digit());
        if offset {
            let index = offsets
                .iter()
                .position(|seen| *seen == segment)
                .unwrap_or_else(|| {
                    offsets.push(segment);
                    offsets.len() - 1
                });
            out.push('N');
            out.push_str(&index.to_string());
        } else {
            out.push_str(segment);
        }
        if let Some(separator) = separator {
            out.push(separator);
        }
        after_separator = separator.is_some();
    }
    out
}

#[cfg(test)]
mod tests;
