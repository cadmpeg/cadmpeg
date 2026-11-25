// SPDX-License-Identifier: Apache-2.0
//! What the full decoder reads back from a written part, compared with the
//! document the write must read back as: the decoded source with the edits
//! the write carries applied ([`expected`]). Each difference is removed,
//! by writing again with the stored record of the edited feature that causes
//! it, or named by a loss that names its record.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::Decoded;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{Feature, FeatureId, ParameterId};
use cadmpeg_ir::source_fidelity::CodecDigests;
use serde_json::Value;

use super::brep_agreement::stale_after_edit;
use super::edits::{
    listed, ConstructionLeaf, FeatureClaims, UnwrittenEdits, UnwrittenPart, RESTATES,
};
use crate::history::extrude_direction::{
    reversal_flag, settled_travel, written_reversal_flag, REVERSE, REVERSE_DECODED,
};
use crate::writer::SemanticOutput;

pub(crate) mod document;

use document::{agree, agrees_with_reported, disagreement, Change, Difference, Document, Pairing};

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
    /// Differences the write of the unedited source reads back as well, by
    /// arena: no held-back edit removes them, so they are named at once.
    pub(super) defects: Vec<String>,
    /// Why the written part is not compared with the document: it does not
    /// decode again or does not serialize, and no written edit is left to
    /// hold back.
    pub(super) unread: Option<String>,
    /// The features whose tree place (`ordinal`) or output bodies
    /// (`outputs`) read back other than the document states, with that
    /// field: no loss calls them written.
    pub(crate) misread: HashSet<(FeatureId, String)>,
    /// What reads back other than the document states and no edit explains.
    unexplained: Vec<String>,
    /// The definition each claimed feature must read back with, in wire
    /// form: its source definition with each edit as the claims judge it.
    definitions: HashMap<FeatureId, Value>,
}

/// The relative difference up to which two real numbers read back the same:
/// a millimetre value stored in metres moves in its last bits.
const READ_BACK_RELATIVE_TOLERANCE: f64 = 1.0e-9;

/// Whether real numbers `left` and `right` are equal up to the read-back
/// tolerance.
fn close(left: f64, right: f64) -> bool {
    (left - right).abs() <= READ_BACK_RELATIVE_TOLERANCE * left.abs().max(right.abs()).max(1.0)
}

/// The numbers of `text` and the text around them, each number as `#`.
fn numbers_of(text: &str) -> (String, Vec<f64>) {
    let mut skeleton = String::with_capacity(text.len());
    let mut numbers = Vec::new();
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let signed = bytes[index] == b'-'
            && bytes.get(index + 1).is_some_and(u8::is_ascii_digit)
            && (index == 0 || !bytes[index - 1].is_ascii_alphanumeric());
        if bytes[index].is_ascii_digit() || signed {
            let start = index;
            index += 1;
            while index < bytes.len()
                && (bytes[index].is_ascii_digit()
                    || (bytes[index] == b'.'
                        && bytes.get(index + 1).is_some_and(u8::is_ascii_digit)))
            {
                index += 1;
            }
            if let Ok(number) = text[start..index].parse::<f64>() {
                numbers.push(number);
                skeleton.push('#');
                continue;
            }
            skeleton.push_str(&text[start..index]);
            continue;
        }
        let character = text[index..].chars().next().unwrap_or_default();
        skeleton.push(character);
        index += character.len_utf8().max(1);
    }
    (skeleton, numbers)
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
    digests: &CodecDigests,
    baseline_key: Option<cadmpeg_ir::hash::digest::Sha256Digest>,
    write: &mut Writer<'_>,
    round_trip: &dyn Fn(&Decoded) -> Option<Vec<u8>>,
) -> Result<(WrittenPass, ReadBack), CodecError> {
    let mut judge = Judge::new(input, digests, round_trip);
    judge.baseline_key = baseline_key;
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
    /// The codec digests the decode of `input` recorded.
    digests: &'a CodecDigests,
    /// Content digest of the retained source records and digests, used to
    /// memoize the unmodified source round-trip baseline across writes.
    baseline_key: Option<cadmpeg_ir::hash::digest::Sha256Digest>,
    round_trip: &'a dyn Fn(&Decoded) -> Option<Vec<u8>>,
    stated: Option<Document>,
    /// The decoded source document, from the first pass that states one.
    source: Option<Document>,
    /// The differences of the source document written with no edit; `None`
    /// until a pass needs them.
    baseline: Option<Vec<Difference>>,
}

impl<'a> Judge<'a> {
    fn new(
        input: &'a CadIr,
        digests: &'a CodecDigests,
        round_trip: &'a dyn Fn(&Decoded) -> Option<Vec<u8>>,
    ) -> Self {
        Self {
            input,
            digests,
            baseline_key: None,
            round_trip,
            stated: None,
            source: None,
            baseline: None,
        }
    }

    /// Judges one pass. A claim that asks to withhold its feature needs no
    /// decode: that part is written again.
    fn read(&mut self, (bytes, output): &WrittenPass) -> ReadBack {
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
        let decoded = match crate::decode::decode_for_read_back(bytes) {
            Ok(decoded) => decoded,
            Err(error) => {
                return ReadBack::unreadable(claims, format!("does not decode again ({error})"));
            }
        };
        let round_trip = self.round_trip;
        // The writer's own losses that name a record as omitted or as
        // holding the source state without the edit, and the parameters
        // the driven-dimension omission loss counts.
        let naming = output
            .losses
            .iter()
            .chain(&output.unwritten.notes())
            .filter(|loss| {
                let code = loss.code.local_code();
                code == "edit.dropped" || code.ends_with("omitted")
            })
            .map(|loss| loss.message.clone())
            .chain(
                crate::resolved_features::write_prepare::omitted_driven_parameters(
                    self.input,
                    self.digests,
                )
                .iter()
                .map(|id| id.as_str().to_owned()),
            )
            .collect::<Vec<_>>();
        let baseline_key = self.baseline_key.clone();
        self.judge(
            claims,
            &output.written,
            (&source.ir, &output.coverage.omitted(self.input), &naming),
            decoded.ir(),
            None,
            &mut |stated| baseline_of_cached(baseline_key.as_ref(), round_trip, source, stated),
        )
    }

    /// Judges `written`, the written part decoded, against the document the
    /// write must read back as. `unwritten` holds the edits the write states
    /// it does not carry; `source` is the decoded source document, the
    /// labels of the arenas the write omits, whose `arena.omitted` losses
    /// name their records, and the messages of the writer's losses that
    /// name a record as omitted or as dropping its edit. `settled` holds the
    /// claimed features whose travel reads back as the stored solid settles
    /// it; `None` finds them ([`settled_by_solid`]).
    fn judge(
        &mut self,
        claims: &ReadBackClaims,
        unwritten: &UnwrittenEdits,
        (source, omitted, naming): (&CadIr, &[&str], &[String]),
        written: &CadIr,
        settled: Option<&HashSet<FeatureId>>,
        baseline: &mut dyn FnMut(&Document) -> Vec<Difference>,
    ) -> ReadBack {
        if self.stated.is_none() {
            self.stated = Document::of(self.input);
        }
        if self.source.is_none() {
            self.source = Document::of(source);
        }
        let (Some(stated), Some(source_records), Some(read)) = (
            self.stated.as_ref(),
            self.source.as_ref(),
            Document::of(written),
        ) else {
            return ReadBack::unreadable(claims, "does not serialize".to_owned());
        };
        let expected = expected(self.input, stated, source_records, claims, unwritten);
        let pairing = document::pair(&expected, &read);
        let settled = settled.cloned().unwrap_or_else(|| {
            settled_by_solid(
                self.input,
                source,
                claims,
                &unwritten.kept_definitions,
                written,
                &pairing,
            )
        });
        let mut read_back = ReadBack::of_claims(&claims.features, written, &pairing, &settled);
        let scope = Scope::of(
            claims,
            (&expected, source_records),
            (omitted, naming),
            (&settled, &read_back.definitions, &pairing),
        );
        let differences = document::differences(&expected, &read, &pairing);
        // The parameters a variant edit adds or drops, by id and by the form
        // a key of an unpaired record reports, and the claimed feature of
        // each.
        let reshaped = differences
            .iter()
            .filter(|difference| difference.arena == "model.parameters")
            .filter_map(
                |difference| match scope.classify(difference, &HashMap::new()) {
                    Class::Reshaped(id, _) => {
                        let parameter = difference.id.clone()?;
                        Some([
                            (document::reported_form(&parameter), id.clone()),
                            (parameter, id),
                        ])
                    }
                    _ => None,
                },
            )
            .flatten()
            .collect::<HashMap<_, _>>();
        let mut claim_changes = HashMap::<FeatureId, Vec<String>>::new();
        let mut general = Vec::new();
        for difference in differences {
            match scope.classify(&difference, &reshaped) {
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
                Class::General => general.push(difference),
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
        read_back.misread = general
            .iter()
            .filter(|difference| difference.arena == "model.features")
            .filter_map(|difference| {
                let field = difference.segments().into_iter().next()?;
                let id = FeatureId::mint(difference.id.clone()?).ok()?;
                matches!(field.as_str(), "ordinal" | "outputs").then_some((id, field))
            })
            .collect();
        if general.is_empty() {
            return read_back;
        }
        if self.baseline.is_none() {
            self.baseline = Some(baseline(source_records));
        }
        let base = self.baseline.as_deref().unwrap_or_default();
        let (defects, unexplained): (Vec<_>, Vec<_>) = general
            .into_iter()
            .partition(|difference| base.iter().any(|base| same_difference(base, difference)));
        read_back.defects = by_arena(&defects.iter().map(Difference::label).collect::<Vec<_>>());
        read_back.unexplained = unexplained.iter().map(Difference::label).collect();
        read_back
    }
}

/// The difference labels `labels` by arena: the arena, how many, and the
/// first of them. A label that names no arena stays as it is.
pub(super) fn by_arena(labels: &[String]) -> Vec<String> {
    let mut arenas = std::collections::BTreeMap::<&str, Vec<String>>::new();
    let mut other = Vec::new();
    for label in labels {
        match label.split_once(' ') {
            Some((arena, _)) if arena.contains('.') || arena == "document" => {
                arenas.entry(arena).or_default().push(label.clone());
            }
            _ => other.push(label.clone()),
        }
    }
    arenas
        .into_iter()
        .map(|(arena, labels)| {
            format!(
                "{arena}: {} difference(s) ({})",
                labels.len(),
                listed(&labels)
            )
        })
        .chain(other)
        .collect()
}

/// The document `input`, whose records are `stated`, as the written part
/// must read back: the decoded source document, whose records are `source`,
/// with the edits the write carries applied. An edit the write states it
/// does not carry reads back as the source: the dependencies of a claimed
/// feature, the parts of a parameter edit and the configuration parameter
/// values in `unwritten`. A renamed feature is stated by its edited name
/// where the decoder restates the name. The construction leaves of a claimed
/// feature are judged by its claims ([`ReadBack::of_claims`]), and the
/// records a carried edit reaches by [`Scope::substitutes`].
fn expected(
    input: &CadIr,
    stated: &Document,
    source: &Document,
    claims: &ReadBackClaims,
    unwritten: &UnwrittenEdits,
) -> Document {
    fn restore(expected: &mut Document, source: &Document, arena: &str, id: &str, field: &str) {
        let original = source
            .record(arena, id)
            .and_then(|record| record.get(field))
            .cloned();
        if let Some(Value::Object(record)) = expected.record_mut(arena, id) {
            match original {
                Some(value) => {
                    record.insert(field.to_owned(), value);
                }
                None => {
                    record.remove(field);
                }
            }
        }
    }
    let mut expected = stated.clone();
    for claim in &claims.features {
        if claim.dependencies_unwritten {
            restore(
                &mut expected,
                source,
                "model.features",
                claim.id.as_str(),
                "dependencies",
            );
        }
    }
    for (id, part) in &unwritten.parameters {
        let fields: &[&str] = match part {
            UnwrittenPart::Whole => &["value", "expression", "display"],
            UnwrittenPart::Value => &["value"],
            UnwrittenPart::Expression => &["expression"],
        };
        for field in fields {
            restore(
                &mut expected,
                source,
                "model.parameters",
                id.as_str(),
                field,
            );
        }
    }
    for (configuration, parameter) in &unwritten.configuration_values {
        let original = source
            .record("model.configurations", configuration)
            .and_then(|record| record.get("parameter_values"))
            .and_then(|values| values.get(parameter.as_str()))
            .cloned();
        if let Some(Value::Object(values)) = expected
            .record_mut("model.configurations", configuration)
            .and_then(|record| record.get_mut("parameter_values"))
        {
            match original {
                Some(value) => {
                    values.insert(parameter.as_str().to_owned(), value);
                }
                None => {
                    values.remove(parameter.as_str());
                }
            }
        }
    }
    for feature in &input.model.features {
        let (Some(new), Some(old)) = (
            feature.name.as_deref(),
            source
                .record("model.features", feature.id.as_str())
                .and_then(|record| record.get("name"))
                .and_then(Value::as_str),
        ) else {
            continue;
        };
        if old != new {
            expected.rename_feature(feature.id.as_str(), old, new);
        }
    }
    expected
}

/// The claimed features whose travel and `reverse_decoded` mark, which the
/// decoder settles against the stored solid, read back as that solid
/// decides: the stale-solid loss of the write names each, so the difference
/// is named. Each is an extrusion whose edit reaches the written record and
/// that shapes the solid, and whose reversal flag, the record form of its
/// travel, reads back from `written` as the encoder writes it from the
/// caller's feature. `kept` holds the features written with their stored
/// definition record.
fn settled_by_solid(
    input: &CadIr,
    source: &CadIr,
    claims: &ReadBackClaims,
    kept: &HashSet<FeatureId>,
    written: &CadIr,
    pairing: &Pairing,
) -> HashSet<FeatureId> {
    fn feature<'ir>(ir: &'ir CadIr, id: &FeatureId) -> Option<&'ir Feature> {
        ir.model.features.iter().find(|feature| &feature.id == id)
    }
    claims
        .features
        .iter()
        .filter(|claim| {
            let (Some(original), Some(current), Some(read)) = (
                feature(source, &claim.id),
                feature(input, &claim.id),
                claim.read(written, pairing),
            ) else {
                return false;
            };
            settled_travel(&claim.source_definition).is_some()
                && stale_after_edit(
                    original,
                    current,
                    &input.model.features,
                    !kept.contains(&claim.id),
                )
                && reversal_flag(&read.source_properties) == written_reversal_flag(current)
        })
        .map(|claim| claim.id.clone())
        .collect()
}

fn baseline_of_cached(
    key: Option<&cadmpeg_ir::hash::digest::Sha256Digest>,
    round_trip: &dyn Fn(&Decoded) -> Option<Vec<u8>>,
    source: &Decoded,
    stated: &Document,
) -> Vec<Difference> {
    type CachedDifferences = (cadmpeg_ir::hash::digest::Sha256Digest, Vec<Difference>);
    static BASELINE_CACHE: std::sync::Mutex<Vec<CachedDifferences>> =
        std::sync::Mutex::new(Vec::new());

    if let Some(key) = key {
        if let Ok(mut guard) = BASELINE_CACHE.lock() {
            if let Some((_, cached)) = guard.iter().find(|(cached_key, _)| cached_key == key) {
                return cached.clone();
            }
            let computed = baseline_of(round_trip, source, stated);
            if guard.len() >= 2 {
                guard.remove(0);
            }
            guard.push((key.clone(), computed.clone()));
            return computed;
        }
    }
    baseline_of(round_trip, source, stated)
}

/// The differences between `source`, the decoded source document whose
/// records are `stated`, and its write with no edit, decoded again.
fn baseline_of(
    round_trip: &dyn Fn(&Decoded) -> Option<Vec<u8>>,
    source: &Decoded,
    stated: &Document,
) -> Vec<Difference> {
    let Some(bytes) = round_trip(source) else {
        return Vec::new();
    };
    let Ok(decoded) = crate::decode::decode_for_read_back(&bytes) else {
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

/// How a difference between the expected document and the written part is
/// removed or named.
enum Class {
    /// The claims judge it, or a loss of the write names it.
    Explained,
    /// A change of claimed feature `0` beyond its edits: the feature is held
    /// back, or named when no edit of it is written.
    Claim(FeatureId, String),
    /// A parameter a variant edit of feature `0` adds or drops, named.
    Reshaped(FeatureId, String),
    /// Held back when holding back the written edits removes it, else named.
    General,
}

/// The records an edit of the write reaches, and what the edits state.
struct Scope<'a> {
    claims: HashMap<&'a str, &'a FeatureClaims>,
    /// Parameters the parameter edits reach.
    affected: HashSet<String>,
    /// Parameters of a claimed feature.
    owned: HashSet<String>,
    /// Features that own an affected parameter.
    owners: HashSet<String>,
    /// Every id above, each claimed feature and each model record the caller
    /// changed against the source: a design record or an entry keyed by one
    /// may restate the edit.
    touched: HashSet<String>,
    /// Each carried edit of a number: the source value and the edited one.
    numbers: Vec<(f64, f64)>,
    /// Each carried edit of a text: the source text and the edited one.
    texts: Vec<(String, String)>,
    /// The arenas the write omits, as the document names them.
    omitted: HashSet<String>,
    /// The messages of the writer's losses that name a record as omitted or
    /// as dropping its edit.
    naming: &'a [String],
    /// The definition each claimed feature must read back with.
    definitions: &'a HashMap<FeatureId, Value>,
    /// The claimed feature that owns each parameter of a claimed feature.
    parameter_owners: HashMap<String, &'a str>,
    pairing: &'a Pairing,
    /// The claimed extrusions whose edit clears the reversal the decoder
    /// read.
    cleared: HashSet<&'a str>,
    /// The claimed features whose travel reads back as the stored solid
    /// settles it.
    settled: &'a HashSet<FeatureId>,
}

impl<'a> Scope<'a> {
    fn of(
        claims: &'a ReadBackClaims,
        (expected, source): (&Document, &Document),
        (omitted, naming): (&[&str], &'a [String]),
        (settled, definitions, pairing): (
            &'a HashSet<FeatureId>,
            &'a HashMap<FeatureId, Value>,
            &'a Pairing,
        ),
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
        let owned = expected.owned_by("model.parameters", &claimed_ids);
        let parameter_owners = claims
            .features
            .iter()
            .flat_map(|claim| {
                expected
                    .owned_by(
                        "model.parameters",
                        &HashSet::from([claim.id.as_str().to_owned()]),
                    )
                    .into_iter()
                    .map(move |parameter| (parameter, claim.id.as_str()))
            })
            .collect::<HashMap<_, _>>();
        let owners = affected
            .iter()
            .filter_map(|id| {
                let parameter = expected.record("model.parameters", id)?;
                let owner = parameter.get("owner")?;
                owner
                    .as_str()
                    .or_else(|| owner.get("value").and_then(Value::as_str))
                    .map(str::to_owned)
            })
            .collect::<HashSet<_>>();
        let changed = expected.changed_model_records(source);
        let mut numbers = Vec::new();
        let mut texts = Vec::new();
        for edit in claims
            .features
            .iter()
            .flat_map(|claim| &claim.edits)
            .filter(|edit| edit.written)
        {
            match (&edit.source, &edit.edited) {
                (Value::Number(old), Value::Number(new)) => {
                    if let (Some(old), Some(new)) = (old.as_f64(), new.as_f64()) {
                        numbers.push((old, new));
                    }
                }
                (Value::String(old), Value::String(new)) => texts.push((old.clone(), new.clone())),
                _ => {}
            }
        }
        for id in &affected {
            let (Some(old), Some(new)) = (
                source.record("model.parameters", id),
                expected.record("model.parameters", id),
            ) else {
                continue;
            };
            if let (Some(old), Some(new)) = (
                old.pointer("/value/value").and_then(Value::as_f64),
                new.pointer("/value/value").and_then(Value::as_f64),
            ) {
                if !close(old, new) {
                    numbers.push((old, new));
                }
            }
            if let (Some(old), Some(new)) = (
                old.get("expression").and_then(Value::as_str),
                new.get("expression").and_then(Value::as_str),
            ) {
                if old != new {
                    texts.push((old.to_owned(), new.to_owned()));
                }
            }
        }
        let cleared = claims
            .features
            .iter()
            .filter(|claim| claim.clears_reversal)
            .map(|claim| claim.id.as_str())
            .collect();
        let touched = claimed_ids
            .iter()
            .chain(&affected)
            .chain(&owned)
            .chain(&owners)
            .chain(&changed)
            .cloned()
            .collect();
        Self {
            claims: claimed,
            affected,
            owned,
            owners,
            touched,
            numbers,
            texts,
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
            naming,
            definitions,
            parameter_owners,
            pairing,
            cleared,
            settled,
        }
    }

    /// How `difference` is removed or named. `reshaped` holds the
    /// parameters a variant edit adds or drops, as a key reports them, and
    /// the claimed feature of each.
    fn classify(&self, difference: &Difference, reshaped: &HashMap<String, FeatureId>) -> Class {
        // The `arena.omitted` loss names the arena and its record count.
        if self.omitted.contains(&difference.arena) {
            return Class::Explained;
        }
        let id = difference.id.as_deref().unwrap_or_default();
        if !id.is_empty() && self.naming.iter().any(|message| names_id(message, id)) {
            return Class::Explained;
        }
        let segments = difference.segments();
        let field = segments.first().map(String::as_str).unwrap_or_default();
        let owner = difference.owner.as_deref().unwrap_or_default();
        let restating =
            difference.arena.starts_with("design.") || difference.arena == "model.configurations";
        if let Some(claim) = [id, owner]
            .into_iter()
            .chain(segments.iter().map(String::as_str))
            .find_map(|key| reshaped.get(key.trim_end_matches('\'')))
            .filter(|_| restating)
        {
            return Class::Reshaped(claim.clone(), format!("{RESTATES}{}", difference.label()));
        }
        if let Some(class) = self.configured_state(difference, &segments) {
            return class;
        }
        match difference.arena.as_str() {
            "model.features" => {
                if let Some(claim) = self.claims.get(id) {
                    return self.claimed_feature(claim, difference, &segments);
                }
                if self.owners.contains(id)
                    && matches!(field, "definition" | "source_properties" | "source_content")
                {
                    return self.restated(difference);
                }
            }
            "model.parameters" => {
                let added = matches!(difference.change, Change::Added { .. });
                if self.owned.contains(id) || (added && self.claims.contains_key(owner)) {
                    if let Some(claim) = self.claims.get(owner) {
                        return self.owned_parameter(claim, difference, field);
                    }
                }
                if self.affected.contains(id) && matches!(field, "value" | "expression" | "display")
                {
                    return self.restated(difference);
                }
            }
            arena
                if arena.starts_with("design.")
                    && (self.touched.contains(id) || self.touched.contains(owner)) =>
            {
                return self.restated(difference);
            }
            _ => {}
        }
        if segments
            .iter()
            .any(|segment| self.touched.contains(segment))
        {
            return self.restated(difference);
        }
        Class::General
    }

    /// A difference of claimed feature `claim`'s own record. Its definition
    /// and its edited source properties are judged by the claims, and its
    /// reshaped content and source properties are named by the reshape
    /// loss. A change of what its record states from the edit (its written
    /// dependencies, its content, a source property it drops) holds the
    /// feature back; a change of another field takes the general route.
    fn claimed_feature(
        &self,
        claim: &FeatureClaims,
        difference: &Difference,
        segments: &[String],
    ) -> Class {
        let (stated, written) = match &difference.change {
            Change::Leaf {
                stated, written, ..
            } => (stated, written),
            Change::Removed => {
                return Class::Claim(claim.id.clone(), "its record missing".into());
            }
            Change::Added { .. } => {
                return Class::Claim(claim.id.clone(), "a second record".into());
            }
        };
        match segments.first().map(String::as_str).unwrap_or_default() {
            "definition" => Class::Explained,
            "dependencies" if claim.dependencies_unwritten => Class::General,
            "source_content" if claim.reshapes() => {
                Class::Reshaped(claim.id.clone(), "restates its record content".into())
            }
            "source_properties" if segments.len() == 2 => {
                let key = &segments[1];
                if self.travel_record(claim, key, written.is_none())
                    || claim.edits_source_property(key)
                {
                    return Class::Explained;
                }
                match (stated, written) {
                    (Some(_), None) if claim.reshapes() => {
                        Class::Reshaped(claim.id.clone(), format!("drops source property {key}"))
                    }
                    (Some(_), None) => Class::Claim(
                        claim.id.clone(),
                        format!("its source property {key} missing"),
                    ),
                    (None, Some(_)) if claim.reshapes() => {
                        Class::Reshaped(claim.id.clone(), format!("adds source property {key}"))
                    }
                    // A written edit of a lane-derived group writes the
                    // whole group into the record, which the decoder then
                    // reads; the claims judge the definition it states.
                    (_, Some(_))
                        if claim.any_written()
                            && crate::records::LANE_DERIVED_GROUPS
                                .iter()
                                .any(|group| group.properties.contains(&key.as_str())) =>
                    {
                        Class::Explained
                    }
                    _ => self.restated(difference),
                }
            }
            "dependencies" | "source_content" => Class::Claim(
                claim.id.clone(),
                format!(
                    "its {} changed",
                    segments.first().map(String::as_str).unwrap_or_default()
                ),
            ),
            _ => Class::General,
        }
    }

    /// Whether a difference of `claim`'s source property `key`, which the
    /// written part `dropped` or not, is the record form of its travel as the
    /// write states it: the reversal flag that an edit clearing the reversal
    /// drops, or the `reverse_decoded` mark of a travel the stored solid
    /// settles, which the stale-solid loss names.
    fn travel_record(&self, claim: &FeatureClaims, key: &str, dropped: bool) -> bool {
        match key {
            REVERSE => dropped && self.cleared.contains(claim.id.as_str()),
            REVERSE_DECODED => self.settled.contains(&claim.id),
            _ => false,
        }
    }

    /// A difference of a parameter of claimed feature `claim`: its value,
    /// expression and display restate the edit. A value edit adds and drops
    /// no parameter; a variant edit's added or dropped parameter is named.
    fn owned_parameter(
        &self,
        claim: &FeatureClaims,
        difference: &Difference,
        field: &str,
    ) -> Class {
        let name = difference.name.as_deref().unwrap_or("<unnamed>");
        let change = match difference.change {
            Change::Leaf { .. } if matches!(field, "expression" | "value" | "display") => {
                return self.restated(difference);
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

    /// A difference of a record a carried edit reaches: the edit applied
    /// when the leaf reads back as [`Self::substitutes`] states, else a
    /// difference to remove or name.
    fn restated(&self, difference: &Difference) -> Class {
        if let Change::Leaf {
            stated: Some(stated),
            written: Some(written),
            ..
        } = &difference.change
        {
            if self.substitutes(stated, written) {
                return Class::Explained;
            }
        }
        match self.reshaping_claim(difference) {
            Some(claim) => Class::Reshaped(
                claim.id.clone(),
                format!("{RESTATES}{}", difference.label()),
            ),
            None => Class::General,
        }
    }

    /// The claim whose variant edit reshapes the record `difference` is of:
    /// a claimed feature, a parameter of one, or a record keyed by either.
    fn reshaping_claim(&self, difference: &Difference) -> Option<&'a FeatureClaims> {
        let claim_of = |key: &str| -> Option<&'a FeatureClaims> {
            match self.claims.get(key) {
                Some(claim) => Some(*claim),
                None => self.claims.get(self.parameter_owners.get(key)?).copied(),
            }
        };
        let segments = difference.segments();
        let keys = [difference.id.as_deref(), difference.owner.as_deref()]
            .into_iter()
            .flatten()
            .chain(segments.iter().map(String::as_str))
            .collect::<Vec<_>>();
        keys.into_iter()
            .find_map(claim_of)
            .filter(|claim| claim.reshapes())
    }

    /// A difference of a configuration's state of a claimed feature's
    /// definition: that state reads back as the definition the claims judge
    /// the feature must read back with, or holds the feature back.
    fn configured_state(&self, difference: &Difference, segments: &[String]) -> Option<Class> {
        let [states, feature, definition, rest @ ..] = segments else {
            return None;
        };
        if difference.arena != "model.configurations"
            || states.as_str() != "feature_states"
            || definition.as_str() != "definition"
        {
            return None;
        }
        let (claim, expected) = self
            .claims
            .get(feature.as_str())
            .and_then(|claim| Some((claim, self.definitions.get(&claim.id)?)))?;
        let mut pointer = String::new();
        for segment in rest {
            pointer.push('/');
            pointer.push_str(&segment.replace('~', "~0").replace('/', "~1"));
        }
        let Change::Leaf { written, .. } = &difference.change else {
            return None;
        };
        let agrees = match (expected.pointer(&pointer), written) {
            (Some(expected), Some(written)) => {
                agrees_with_reported(expected, written, self.pairing)
            }
            (None, None) => true,
            _ => false,
        };
        Some(if agrees {
            Class::Explained
        } else if claim.reshapes() {
            Class::Reshaped(
                claim.id.clone(),
                format!("{RESTATES}{}", difference.label()),
            )
        } else {
            Class::General
        })
    }

    /// Whether `written` is `stated` with a carried edit applied: the source
    /// number of a carried edit read back as its edited number, the source
    /// text of a carried text edit read back as its edited text, or a text
    /// whose numbers each read back scaled as a carried number edit scales
    /// its value, with the rest of the text the same.
    fn substitutes(&self, stated: &Value, written: &Value) -> bool {
        substitutes(&self.numbers, &self.texts, stated, written)
    }
}

/// Whether `message` names record id `id`: the id, followed by no letter,
/// digit or `:` that would make it another id.
fn names_id(message: &str, id: &str) -> bool {
    message.match_indices(id).any(|(at, _)| {
        !message[at + id.len()..]
            .starts_with(|character: char| character.is_ascii_alphanumeric() || character == ':')
    })
}

/// Whether `written` is `stated` with one of the carried edits `numbers`
/// (source and edited number) and `texts` (source and edited text) applied:
/// the source number read back as the edited one, the source text read
/// back as the edited one, or a text with the rest of the text the same
/// whose numbers each read back unchanged or as a number edit replaces
/// them, or each scaled as one number edit scales its value.
fn substitutes(
    numbers: &[(f64, f64)],
    texts: &[(String, String)],
    stated: &Value,
    written: &Value,
) -> bool {
    match (stated, written) {
        (Value::Number(stated), Value::Number(written)) => {
            let (Some(stated), Some(written)) = (stated.as_f64(), written.as_f64()) else {
                return false;
            };
            numbers
                .iter()
                .any(|(old, new)| close(stated, *old) && close(written, *new))
        }
        (Value::String(stated), Value::String(written)) => {
            if texts
                .iter()
                .any(|(old, new)| old == stated && new == written)
            {
                return true;
            }
            let (stated_text, stated_numbers) = numbers_of(stated);
            let (written_text, written_numbers) = numbers_of(written);
            let pairs = stated_numbers.iter().zip(&written_numbers);
            // Each changed number takes a carried number edit of its own.
            let mut unused = numbers.to_vec();
            let substituted = pairs.clone().all(|(stated, written)| {
                close(*stated, *written)
                    || unused
                        .iter()
                        .position(|(old, new)| close(*stated, *old) && close(*written, *new))
                        .map(|used| unused.swap_remove(used))
                        .is_some()
            });
            stated_text == written_text
                && stated_numbers.len() == written_numbers.len()
                && pairs
                    .clone()
                    .any(|(stated, written)| !close(*stated, *written))
                && (substituted
                    || numbers.iter().any(|(old, new)| {
                        *old != 0.0
                            && pairs.clone().all(|(stated, written)| {
                                close(*stated, *written) || close(*written, stated * new / old)
                            })
                    }))
        }
        _ => false,
    }
}

impl ReadBack {
    /// Judges `claims`, none of which asks to withhold its feature, against
    /// `written`, the written part decoded, found through `pairing`. Each
    /// leaf compares exactly through the pairing. The travel of a feature of
    /// `settled` and its `reverse_decoded` mark read back as the stored solid
    /// settles them.
    fn of_claims(
        claims: &[FeatureClaims],
        written: &CadIr,
        pairing: &Pairing,
        settled: &HashSet<FeatureId>,
    ) -> Self {
        let mut read_back = Self::default();
        for claim in claims {
            let read = claim.read(written, pairing).and_then(|read| {
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
            let travel = settled
                .contains(&claim.id)
                .then(|| settled_travel(&claim.source_definition))
                .flatten();
            let settled_leaf = |leaf: &ConstructionLeaf| match (travel, leaf) {
                (Some(travel), ConstructionLeaf::Definition(pointer)) => pointer
                    .strip_prefix(travel)
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with('/')),
                (Some(_), ConstructionLeaf::SourceProperty(key)) => key.as_str() == REVERSE_DECODED,
                _ => false,
            };
            let mut expected = claim.source_definition.clone();
            let mut failed = false;
            let mut neither = Vec::new();
            for edit in &claim.edits {
                let got = match &edit.leaf {
                    ConstructionLeaf::Definition(pointer) => {
                        let got = read_definition.pointer(pointer).cloned();
                        put(
                            &mut expected,
                            pointer,
                            got.as_ref().map(|got| pairing.stated_ids(got)),
                        );
                        got.unwrap_or(Value::Null)
                    }
                    ConstructionLeaf::SourceProperty(key) => read
                        .source_properties
                        .get(key)
                        .map_or(Value::Null, |value| Value::String(value.clone())),
                    ConstructionLeaf::Dependencies => continue,
                };
                let as_edited = agree(&edit.edited, &got, pairing);
                if edit.written {
                    failed |= !as_edited && !settled_leaf(&edit.leaf);
                } else if !agree(&edit.source, &got, pairing) {
                    if as_edited {
                        read_back
                            .carried
                            .insert((claim.id.clone(), edit.label.clone()));
                    } else {
                        neither.push(edit.label.clone());
                    }
                }
            }
            if let Some(travel) = travel {
                put(
                    &mut expected,
                    travel,
                    read_definition
                        .pointer(travel)
                        .map(|got| pairing.stated_ids(got)),
                );
            }
            if let Some(places) = disagreement(&expected, &read_definition, pairing) {
                failed = true;
                neither.push(format!("the rest of its definition (at {places})"));
            }
            read_back.definitions.insert(claim.id.clone(), expected);
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

    /// A pass whose written part is not compared with the document, because
    /// it `reason` (does not decode again, or does not serialize): each
    /// written edit is held back, and with none written the reason is named.
    fn unreadable(claims: &ReadBackClaims, reason: String) -> Self {
        let withhold = claims
            .features
            .iter()
            .filter(|claim| claim.any_written())
            .map(|claim| {
                (
                    claim.id.clone(),
                    format!(
                        "SLDPRT feature {} written as edited gives a part that {reason}",
                        claim.id
                    ),
                )
            })
            .collect::<Vec<_>>();
        if withhold.is_empty() {
            Self {
                unread: Some(reason),
                ..Self::default()
            }
        } else {
            Self {
                withhold,
                ..Self::default()
            }
        }
    }

    fn withholds(&self, id: &FeatureId) -> bool {
        self.withhold.iter().any(|(withheld, _)| withheld == id)
    }
}

#[cfg(test)]
mod tests;
