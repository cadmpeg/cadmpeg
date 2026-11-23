// SPDX-License-Identifier: Apache-2.0
//! What the full decoder reads back from a written part, compared with the
//! caller's whole document. A write whose part reads back changed where no
//! edit touches it is written again with the stored record of the edited
//! feature that changes it; a change that no stored record removes is named.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::Decoded;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{FeatureId, ParameterId};
use serde_json::Value;

use super::edits::{listed, ConstructionLeaf, FeatureClaims};
use crate::writer::SemanticOutput;

pub(crate) mod document;

use document::{Change, Difference, Document};

/// The construction edits of a write, and the parameters its parameter edits
/// reach.
#[derive(Default)]
pub(crate) struct ReadBackClaims {
    pub(super) features: Vec<FeatureClaims>,
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
    /// The parameters a variant edit adds to or drops from its feature's
    /// record, by feature.
    pub(super) reshaped: Vec<(FeatureId, Vec<String>)>,
    /// Edits called written that do not read back as edited and whose
    /// stored record does not write, each with why.
    pub(super) unkept: Vec<String>,
    /// What reads back other than the document states and no edit explains.
    unexplained: Vec<String>,
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

/// One semantic write: the written part, and what the writer states about it.
pub(crate) type WrittenPass = (Vec<u8>, SemanticOutput);

/// A semantic write of the caller's document with the features it is given
/// written with their stored records, each for its reason.
pub(crate) type Writer<'a> =
    dyn FnMut(&[(FeatureId, String)]) -> Result<WrittenPass, CodecError> + 'a;

/// Writes the caller's document `input` with `write` until the full decoder
/// reads every written edit back as edited and the rest of the part as
/// `input` states it. `round_trip` writes the decoded source document with no
/// edit, or returns `None` when that write fails or gives the source part
/// byte for byte.
///
/// A difference that no edit accounts for is held back when writing every
/// written edit with its stored record removes it: by the one edit whose
/// withholding alone removes it, or else by all of them. A difference that
/// holding back does not remove, and an edit whose stored record does not
/// write, is named.
pub(crate) fn write_until_read_back(
    input: &CadIr,
    write: &mut Writer<'_>,
    round_trip: &dyn Fn(&Decoded) -> Option<Vec<u8>>,
) -> Result<(WrittenPass, ReadBack), CodecError> {
    let mut judge = Judge::new(input, round_trip);
    let mut withheld = Vec::<(FeatureId, String)>::new();
    let mut pass = write(&withheld)?;
    let mut read_back = judge.read(&pass);
    loop {
        let new = read_back
            .withhold
            .iter()
            .filter(|(id, _)| !withheld.iter().any(|(withheld, _)| withheld == id))
            .cloned()
            .collect::<Vec<_>>();
        if !new.is_empty() {
            let mut next = withheld.clone();
            next.extend(new.iter().cloned());
            match write(&next) {
                Ok(written) => {
                    read_back = judge.read(&written);
                    (pass, withheld) = (written, next);
                }
                Err(error) => {
                    read_back.unkept.extend(new.into_iter().map(|(_, reason)| {
                        format!("{reason}; writing it with its stored definition record fails ({error}), so the written record states the edit")
                    }));
                    let unexplained = std::mem::take(&mut read_back.unexplained);
                    read_back.changed.extend(unexplained);
                    return Ok((pass, read_back));
                }
            }
            continue;
        }
        let unexplained = std::mem::take(&mut read_back.unexplained);
        if unexplained.is_empty() {
            return Ok((pass, read_back));
        }
        let suspects = pass
            .1
            .claims
            .features
            .iter()
            .filter(|claim| claim.any_written())
            .map(|claim| claim.id.clone())
            .collect::<Vec<_>>();
        let placeholder = held_back(&suspects, &unexplained);
        let mut all = withheld.clone();
        all.extend(suspects.iter().map(|id| (id.clone(), placeholder.clone())));
        let trial = if suspects.is_empty() {
            None
        } else {
            write(&all).ok().map(|written| {
                let read = judge.read(&written);
                (written, read)
            })
        };
        // A trial with an edit that does not read back is judged again once
        // that edit is withheld too, so it shows nothing about the rest.
        let Some((mut trial, trial_read_back)) = trial.filter(|(_, read)| read.withhold.is_empty())
        else {
            read_back.changed.extend(unexplained);
            return Ok((pass, read_back));
        };
        let removed = unexplained
            .iter()
            .filter(|label| !trial_read_back.unexplained.contains(label))
            .cloned()
            .collect::<Vec<_>>();
        if removed.is_empty() {
            read_back.changed.extend(unexplained);
            return Ok((pass, read_back));
        }
        let mut culprit = None;
        if suspects.len() > 1 {
            for suspect in &suspects {
                let mut one = withheld.clone();
                one.push((
                    suspect.clone(),
                    held_back(std::slice::from_ref(suspect), &removed),
                ));
                let Ok(written) = write(&one) else {
                    continue;
                };
                let read = judge.read(&written);
                if read.withhold.is_empty()
                    && removed
                        .iter()
                        .all(|label| !read.unexplained.contains(label))
                {
                    culprit = Some((one, written, read));
                    break;
                }
            }
        }
        (withheld, pass, read_back) = culprit.unwrap_or_else(|| {
            let reason = held_back(&suspects, &removed);
            for id in &suspects {
                trial.1.unwritten_edits.restate(id, &placeholder, &reason);
            }
            for (id, held) in &mut all {
                if suspects.contains(id) {
                    held.clone_from(&reason);
                }
            }
            (all, trial, trial_read_back)
        });
    }
}

/// The reason the written edits of `features` are written with their stored
/// records: they change `changes` on read-back.
fn held_back(features: &[FeatureId], changes: &[String]) -> String {
    let changes = listed(changes);
    match features {
        [feature] => format!(
            "SLDPRT feature {feature} written as edited changes {changes} on read-back, which no edit touches"
        ),
        _ => format!(
            "SLDPRT features {} written as edited change {changes} on read-back together, which no edit touches",
            features
                .iter()
                .map(FeatureId::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// The caller's document as the read-back compares it, kept across the
/// passes of one write.
struct Judge<'a> {
    input: &'a CadIr,
    round_trip: &'a dyn Fn(&Decoded) -> Option<Vec<u8>>,
    stated: Option<Document>,
    /// The decoded source document, from the first pass that states one.
    source: Option<Document>,
    /// The differences of the source document written with no edit; `None`
    /// until a pass needs them.
    baseline: Option<Vec<Difference>>,
}

impl<'a> Judge<'a> {
    fn new(input: &'a CadIr, round_trip: &'a dyn Fn(&Decoded) -> Option<Vec<u8>>) -> Self {
        Self {
            input,
            round_trip,
            stated: None,
            source: None,
            baseline: None,
        }
    }

    /// Judges one pass. A claim that asks to withhold its feature needs no
    /// decode: that part is written again.
    fn read(&mut self, (bytes, output): &WrittenPass) -> ReadBack {
        use cadmpeg_ir::codec::{Codec, DecodeOptions};

        let claims = &output.claims;
        let Some(source) = output.source.as_ref() else {
            return ReadBack::default();
        };
        let withhold = claims
            .features
            .iter()
            .filter(|claim| claim.withhold)
            .map(|claim| (claim.id.clone(), unread(&claim.id)))
            .collect::<Vec<_>>();
        if !withhold.is_empty() {
            return ReadBack {
                withhold,
                ..ReadBack::default()
            };
        }
        let decoded = crate::SldprtCodec
            .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
            .ok();
        let written = decoded.as_ref().map(cadmpeg_ir::codec::DecodeResult::ir);
        let mut read_back = ReadBack::of_claims(&claims.features, written);
        if let Some(written) = written {
            let round_trip = self.round_trip;
            self.judge_part(
                &mut read_back,
                claims,
                (&source.ir, &output.coverage.omitted(self.input)),
                written,
                &mut |stated| baseline_of(round_trip, source, stated),
            );
        }
        read_back
    }

    /// Compares `written` with the caller's document whole. A change of a
    /// claimed feature or its parameters beyond its edits withholds that
    /// feature. Every other difference that no edit, no arena the write omits
    /// and no unedited write of the source explains is unexplained. `source`
    /// is the decoded source document and the labels of the arenas the write
    /// omits, whose `arena.omitted` losses name their records.
    fn judge_part(
        &mut self,
        read_back: &mut ReadBack,
        claims: &ReadBackClaims,
        (source, omitted): (&CadIr, &[&str]),
        written: &CadIr,
        baseline: &mut dyn FnMut(&Document) -> Vec<Difference>,
    ) {
        if self.stated.is_none() {
            self.stated = Document::of(self.input);
        }
        if self.source.is_none() {
            self.source = Document::of(source);
        }
        let (Some(stated), Some(read)) = (self.stated.as_ref(), Document::of(written)) else {
            return;
        };
        let pairing = document::pair(stated, &read);
        let scope = Scope::of(self.input, claims, stated, self.source.as_ref(), omitted);
        let mut claim_changes = HashMap::<FeatureId, Vec<String>>::new();
        let mut unexplained = Vec::new();
        for difference in document::differences(stated, &read, &pairing) {
            match scope.classify(&difference) {
                Class::Explained => {}
                Class::Claim(id, change) => claim_changes.entry(id).or_default().push(change),
                Class::Reshaped(id, change) => match read_back
                    .reshaped
                    .iter_mut()
                    .find(|(feature, _)| *feature == id)
                {
                    Some((_, changes)) => changes.push(change),
                    None => read_back.reshaped.push((id, vec![change])),
                },
                Class::Unexplained => unexplained.push(difference),
            }
        }
        for claim in &claims.features {
            let Some(changes) = claim_changes.remove(&claim.id) else {
                continue;
            };
            if claim.any_written() {
                if !read_back.withholds(&claim.id) {
                    read_back.withhold.push((
                        claim.id.clone(),
                        format!(
                            "SLDPRT feature {} reads back from records that state its edit with {}",
                            claim.id,
                            listed(&changes)
                        ),
                    ));
                }
            } else {
                read_back
                    .changed
                    .push(format!("feature {} ({})", claim.id, listed(&changes)));
            }
        }
        if unexplained.is_empty() || !read_back.withhold.is_empty() {
            // A feature withheld in this pass may be what changed the rest:
            // the next pass judges the part again.
            return;
        }
        let unexplained = self.unexplained_by_baseline(baseline, &unexplained);
        if unexplained.is_empty() {
            return;
        }
        read_back.unexplained = unexplained;
    }

    /// The labels of `differences` that the source document written with no
    /// edit does not have as well.
    fn unexplained_by_baseline(
        &mut self,
        baseline: &mut dyn FnMut(&Document) -> Vec<Difference>,
        differences: &[Difference],
    ) -> Vec<String> {
        if self.baseline.is_none() {
            self.baseline = Some(self.source.as_ref().map(&mut *baseline).unwrap_or_default());
        }
        let baseline = self.baseline.as_deref().unwrap_or_default();
        differences
            .iter()
            .filter(|difference| {
                !baseline
                    .iter()
                    .any(|base| same_difference(base, difference))
            })
            .map(Difference::label)
            .collect()
    }
}

/// The differences between `source`, the decoded source document whose
/// records are `stated`, and its write with no edit, decoded again.
fn baseline_of(
    round_trip: &dyn Fn(&Decoded) -> Option<Vec<u8>>,
    source: &Decoded,
    stated: &Document,
) -> Vec<Difference> {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};

    let Some(bytes) = round_trip(source) else {
        return Vec::new();
    };
    let Ok(decoded) =
        crate::SldprtCodec.decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
    else {
        return Vec::new();
    };
    let Some(read) = Document::of(decoded.ir()) else {
        return Vec::new();
    };
    let pairing = document::pair(stated, &read);
    document::differences(stated, &read, &pairing)
}

/// Whether `difference` is `base`: the same record and change, with each
/// stated and written value the same.
fn same_difference(base: &Difference, difference: &Difference) -> bool {
    let agree = |left: &Option<Value>, right: &Option<Value>| match (left, right) {
        (Some(left), Some(right)) => cadmpeg_ir::compare::values_agree(left, right).is_ok(),
        (None, None) => true,
        _ => false,
    };
    base.arena == difference.arena
        && match (&base.change, &difference.change) {
            (Change::Removed, Change::Removed) => base.id == difference.id,
            (Change::Added { content: left }, Change::Added { content: right }) => left == right,
            (
                Change::Leaf {
                    pointer,
                    stated,
                    written,
                },
                Change::Leaf {
                    pointer: other_pointer,
                    stated: other_stated,
                    written: other_written,
                },
            ) => {
                base.id == difference.id
                    && pointer == other_pointer
                    && agree(stated, other_stated)
                    && agree(written, other_written)
            }
            _ => false,
        }
}

/// What a difference between the caller's document and the written part is.
enum Class {
    /// The edit or a record that mirrors it.
    Explained,
    /// A change of claimed feature `0` beyond its edits.
    Claim(FeatureId, String),
    /// A parameter a variant edit of feature `0` adds or drops.
    Reshaped(FeatureId, String),
    Unexplained,
}

/// The records an edit of the write touches.
struct Scope<'a> {
    stated: &'a Document,
    source: Option<&'a Document>,
    claims: HashMap<&'a str, &'a FeatureClaims>,
    /// Parameters the parameter edits judge.
    affected: HashSet<String>,
    /// Parameters of a claimed feature.
    owned: HashSet<String>,
    /// Features that own an affected parameter.
    owners: HashSet<String>,
    /// Every id above, each claimed feature and each model record the caller
    /// changed against the source: a design record or a leaf
    /// keyed by one mirrors the edit.
    touched: HashSet<String>,
    /// The source and the caller's name of each renamed feature.
    renames: Vec<(String, String)>,
    /// The arenas the write omits, as the document names them.
    omitted: HashSet<String>,
}

impl<'a> Scope<'a> {
    fn of(
        input: &CadIr,
        claims: &'a ReadBackClaims,
        stated: &'a Document,
        source: Option<&'a Document>,
        omitted: &[&str],
    ) -> Self {
        let claimed = claims
            .features
            .iter()
            .map(|claim| (claim.id.as_str(), claim))
            .collect::<HashMap<_, _>>();
        let affected = claims
            .affected_parameters
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect::<HashSet<_>>();
        let claimed_ids = claimed
            .keys()
            .map(|id| (*id).to_owned())
            .collect::<HashSet<_>>();
        let owned = stated.owned_by("model.parameters", &claimed_ids);
        let owners = input
            .model
            .parameters
            .iter()
            .filter(|parameter| affected.contains(parameter.id.as_str()))
            .filter_map(|parameter| parameter.owner.as_ref())
            .map(|owner| owner.as_str().to_owned())
            .collect::<HashSet<_>>();
        let changed = source
            .map(|source| stated.changed_model_records(source))
            .unwrap_or_default();
        let renames = source
            .map(|source| {
                input
                    .model
                    .features
                    .iter()
                    .filter_map(|feature| {
                        let new = feature.name.as_deref()?;
                        let old = source
                            .record("model.features", feature.id.as_str())?
                            .get("name")?
                            .as_str()?;
                        (old != new).then(|| (old.to_owned(), new.to_owned()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let touched = claimed_ids
            .iter()
            .chain(&affected)
            .chain(&owned)
            .chain(&owners)
            .chain(&changed)
            .cloned()
            .collect();
        Self {
            stated,
            source,
            claims: claimed,
            affected,
            owned,
            owners,
            touched,
            renames,
            omitted: omitted
                .iter()
                .flat_map(|label| {
                    if label.contains('.') {
                        vec![(*label).to_owned()]
                    } else {
                        vec![format!("model.{label}"), format!("geometry.{label}")]
                    }
                })
                .collect(),
        }
    }

    fn classify(&self, difference: &Difference) -> Class {
        if self.omitted.contains(&difference.arena) {
            return Class::Explained;
        }
        let id = difference.id.as_deref().unwrap_or_default();
        let segments = difference.segments();
        let field = segments.first().map(String::as_str).unwrap_or_default();
        let owner = difference.owner.as_deref().unwrap_or_default();
        match difference.arena.as_str() {
            "model.features" => {
                if let Some(claim) = self.claims.get(id) {
                    return self.claimed_feature(claim, difference, &segments);
                }
                if self.owners.contains(id)
                    && matches!(field, "definition" | "source_properties" | "source_content")
                {
                    return Class::Explained;
                }
            }
            "model.parameters" => {
                if self.affected.contains(id) {
                    return Class::Explained;
                }
                let added = matches!(difference.change, Change::Added { .. });
                if self.owned.contains(id) || (added && self.claims.contains_key(owner)) {
                    if let Some(claim) = self.claims.get(owner) {
                        return Self::owned_parameter(claim, difference, field);
                    }
                }
            }
            arena
                if arena.starts_with("design.")
                    && (self.touched.contains(id) || self.touched.contains(owner)) =>
            {
                return Class::Explained;
            }
            _ => {}
        }
        // A key that names an unpaired record is that record's entry; the
        // record's own difference judges it.
        if segments.iter().any(|segment| {
            self.touched.contains(segment) || segment.contains(document::UNPAIRED_DIGITS)
        }) || self.caller_changed(difference)
            || self.renamed(difference)
        {
            return Class::Explained;
        }
        Class::Unexplained
    }

    /// A difference of claimed feature `claim`'s own record: its definition
    /// is judged by the claims, and its content and source property values
    /// mirror the edited records.
    fn claimed_feature(
        &self,
        claim: &FeatureClaims,
        difference: &Difference,
        segments: &[String],
    ) -> Class {
        let Change::Leaf {
            stated, written, ..
        } = &difference.change
        else {
            return Class::Explained;
        };
        match segments.first().map(String::as_str).unwrap_or_default() {
            "definition" | "source_content" => Class::Explained,
            "dependencies" if claim.dependencies_edited => Class::Explained,
            "source_properties" => match segments.get(1) {
                Some(key)
                    if stated.is_some()
                        && written.is_none()
                        && segments.len() == 2
                        && !claim.edits_source_property(key) =>
                {
                    if claim.reshapes() {
                        Class::Reshaped(claim.id.clone(), format!("drops source property {key}"))
                    } else {
                        Class::Claim(
                            claim.id.clone(),
                            format!("its source property {key} missing"),
                        )
                    }
                }
                _ => Class::Explained,
            },
            _ if self.caller_changed(difference) || self.renamed(difference) => Class::Explained,
            field => Class::Claim(claim.id.clone(), format!("its {field} changed")),
        }
    }

    /// A difference of a parameter of claimed feature `claim`: its value,
    /// expression and display mirror the edit. A value edit adds and drops
    /// none; a variant edit's added or dropped parameter is named.
    fn owned_parameter(claim: &FeatureClaims, difference: &Difference, field: &str) -> Class {
        let name = difference.name.as_deref().unwrap_or("<unnamed>");
        let change = match difference.change {
            Change::Leaf { .. } if matches!(field, "expression" | "value" | "display") => {
                return Class::Explained;
            }
            Change::Leaf { .. } => format!("its parameter {name} changed"),
            Change::Removed => format!("its parameter {name} missing"),
            Change::Added { .. } => format!("its parameter {name} added"),
        };
        match difference.change {
            Change::Removed if claim.reshapes() => {
                Class::Reshaped(claim.id.clone(), format!("drops parameter {name}"))
            }
            Change::Added { .. } if claim.reshapes() => {
                Class::Reshaped(claim.id.clone(), format!("adds parameter {name}"))
            }
            _ => Class::Claim(claim.id.clone(), change),
        }
    }

    /// Whether the caller changed the place of `difference` against the
    /// source: the edit itself, which the edit's own write path judges.
    fn caller_changed(&self, difference: &Difference) -> bool {
        let Some(source) = self.source else {
            return false;
        };
        let arena = difference.arena.as_str();
        match (&difference.change, difference.id.as_deref()) {
            (Change::Leaf { pointer, .. }, Some(id)) => {
                let Some(stated) = self.stated.record(arena, id) else {
                    return false;
                };
                let Some(original) = source.record(arena, id) else {
                    return true;
                };
                match (stated.pointer(pointer), original.pointer(pointer)) {
                    (Some(left), Some(right)) => {
                        cadmpeg_ir::compare::values_agree(left, right).is_err()
                    }
                    (None, None) => false,
                    _ => true,
                }
            }
            (Change::Removed, Some(id)) => source.record(arena, id).is_none(),
            (Change::Added { .. }, _) => difference
                .name
                .as_deref()
                .is_some_and(|name| source.named(arena, name) && !self.stated.named(arena, name)),
            _ => false,
        }
    }

    /// Whether `difference` is a text that states a renamed feature by its
    /// caller's name where the document states it by its source name.
    fn renamed(&self, difference: &Difference) -> bool {
        let Change::Leaf {
            stated: Some(Value::String(stated)),
            written: Some(Value::String(written)),
            ..
        } = &difference.change
        else {
            return false;
        };
        let mut text = stated.clone();
        for (old, new) in &self.renames {
            text = text.replace(old.as_str(), new);
        }
        text == *written
    }
}

impl ReadBack {
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

    fn withholds(&self, id: &FeatureId) -> bool {
        self.withhold.iter().any(|(withheld, _)| withheld == id)
    }
}

#[cfg(test)]
mod tests;
