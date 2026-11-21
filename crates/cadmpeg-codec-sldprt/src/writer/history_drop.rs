// SPDX-License-Identifier: Apache-2.0
//! Design records of another format that the history writer cannot write
//! (§3.4, "Content the target cannot hold"). The history stage is retried
//! without the record a failure names, and without every record that needs
//! it (§3.3), until it writes. A failure that names no design record drops the
//! whole feature history. The current geometry is still written: the writer
//! writes the current partition whatever history it writes.

use std::collections::{BTreeSet, HashSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::report::loss::LossNote;
use serde::Serialize;
use serde_json::Value;

use crate::history::literals::{format_parameter_value, parse_parameter_literal};
use crate::loss::SldprtLossCode;
use crate::native::SldprtNative;

/// Source attributes that state a `SolidWorks` feature-history baseline. A
/// document with either is a `SolidWorks` source, whose history edits take the
/// patch path; only a document with neither is reduced here.
const HISTORY_BASELINES: [&str; 2] = [
    "sldprt_neutral_feature_local_sha256",
    "sldprt_native_history_sha256",
];

/// The design records dropped so far, and their losses.
#[derive(Default)]
pub(super) struct HistoryDrop {
    losses: Vec<LossNote>,
    coarse: Coarse,
}

/// The drop a failure that names no record takes: the first drops the feature
/// history, the next every parameter as well.
#[derive(Default, Clone, Copy)]
enum Coarse {
    #[default]
    None,
    History,
    Parameters,
}

/// The records one failure removes.
#[derive(Default)]
struct Removed {
    features: BTreeSet<String>,
    sketches: BTreeSet<String>,
    constraints: BTreeSet<String>,
    parameters: BTreeSet<String>,
    /// Sketch curves outside every profile of their sketch.
    entities: BTreeSet<String>,
}

impl HistoryDrop {
    /// Whether `ir` is a document whose history this pass may reduce.
    pub(super) fn applies(ir: &CadIr) -> bool {
        ir.source.as_ref().is_none_or(|source| {
            HISTORY_BASELINES
                .iter()
                .all(|name| !source.attributes.contains_key(*name))
        })
    }

    /// Remove the records `error` names and those that need them from `ir`,
    /// and charge them. Returns `error` when nothing is left to remove.
    pub(super) fn degrade(&mut self, ir: &mut CadIr, error: CodecError) -> Result<(), CodecError> {
        if matches!(error, CodecError::ResourceLimit(_) | CodecError::Io(_)) {
            return Err(error);
        }
        let reason = error.to_string();
        let named = named_records(ir, &reason);
        let removed = if named.is_empty() {
            loop {
                let (next, seeds) = match self.coarse {
                    Coarse::None => (Coarse::History, every_design_record(ir)),
                    Coarse::History => (Coarse::Parameters, every_parameter(ir)),
                    Coarse::Parameters => return Err(error),
                };
                self.coarse = next;
                let removed = remove(ir, &seeds);
                if !removed.is_empty() {
                    break removed;
                }
            }
        } else {
            let removed = remove(ir, &named);
            if removed.is_empty() {
                return Err(error);
            }
            removed
        };
        self.losses.extend(removed.notes(&reason));
        Ok(())
    }

    pub(super) fn finish(self) -> Vec<LossNote> {
        self.losses
    }
}

/// Clear each feature and parameter `native_ref` that names no record of the
/// document's `sldprt` namespace. The history writer reads that field as one of
/// its own records (§3.4 W1); another format's reference is written as a
/// generated record.
pub(super) fn clear_foreign_native_refs(ir: &mut CadIr, native: Option<&SldprtNative>) {
    let features = native
        .iter()
        .flat_map(|native| &native.feature_histories)
        .flat_map(|history| &history.features)
        .map(|feature| feature.id.as_str())
        .collect::<HashSet<_>>();
    let scalars = native
        .iter()
        .flat_map(|native| &native.feature_input_lanes)
        .flat_map(|lane| &lane.scalars)
        .map(|scalar| scalar.id.as_str())
        .collect::<HashSet<_>>();
    for feature in &mut ir.model.features {
        if feature
            .native_ref
            .as_deref()
            .is_some_and(|record| !features.contains(record))
        {
            feature.native_ref = None;
        }
    }
    for parameter in &mut ir.model.parameters {
        if parameter
            .native_ref
            .as_deref()
            .is_some_and(|scalar| !scalars.contains(scalar))
        {
            parameter.native_ref = None;
        }
    }
}

/// Write each parameter whose expression is not a literal with its displayed
/// value, and name the expression as left out. The Keywords dimension text
/// holds the displayed value; the equation that drives it belongs to the
/// equation manager in `Contents/Config-N` (`sldprt.md`, Keywords history),
/// whose dimension references hold handle objects the format document does
/// not state, so this writer writes no equation. A parameter with no value
/// the text can hold is dropped and named. The rewritten parameters come
/// back as [`DisplayedValues`], named once the write knows which it keeps.
pub(super) fn equations_as_displayed_values(ir: &mut CadIr) -> (DisplayedValues, Vec<LossNote>) {
    let mut written = Vec::new();
    let mut valueless = Removed::default();
    for parameter in &mut ir.model.parameters {
        if parse_parameter_literal(&parameter.expression).is_some() {
            continue;
        }
        let displayed = parameter
            .value
            .as_ref()
            .map(format_parameter_value)
            .filter(|text| parse_parameter_literal(text).is_some());
        let Some(displayed) = displayed else {
            valueless
                .parameters
                .insert(parameter.id.as_str().to_owned());
            continue;
        };
        written.push((
            parameter.id.as_str().to_owned(),
            format!(
                "{} ({} = {})",
                parameter.id.as_str(),
                parameter.name,
                parameter.expression
            ),
        ));
        parameter.expression = displayed;
        parameter.dependencies = cadmpeg_ir::features::DistinctMembers::default();
    }
    let mut notes = Vec::new();
    if !valueless.is_empty() {
        notes.extend(
            remove(ir, &valueless)
                .notes("an expression with no value a Keywords dimension text can hold"),
        );
    }
    (DisplayedValues(written), notes)
}

/// Parameters written with their displayed value in place of their
/// expression: each parameter's id and the text that names it.
#[derive(Default)]
pub(super) struct DisplayedValues(Vec<(String, String)>);

impl DisplayedValues {
    /// The note naming those of the parameters that `ir` still holds. A
    /// parameter a later drop removed is named by that drop.
    pub(super) fn note(self, ir: &CadIr) -> Option<LossNote> {
        let kept: BTreeSet<&str> = ir
            .model
            .parameters
            .iter()
            .map(|parameter| parameter.id.as_str())
            .collect();
        let written: Vec<String> = self
            .0
            .into_iter()
            .filter(|(id, _)| kept.contains(id.as_str()))
            .map(|(_, text)| text)
            .collect();
        (!written.is_empty()).then(|| {
            SldprtLossCode::WriterParameterExpressionsOmitted.note(format!(
                "the SLDPRT writer writes {} parameter(s) with their displayed value and without \
             the equation that drives it, which belongs to the equation manager this writer \
             does not write: {}",
                written.len(),
                written.join(", ")
            ))
        })
    }
}

/// Drop each `Native` operation of another format, with every record that
/// needs it, and name them: only the operation's own codec writes it (§3.4).
pub(super) fn foreign_native_operations(ir: &mut CadIr) -> Vec<LossNote> {
    let seeds = Removed {
        features: ir
            .model
            .features
            .iter()
            .filter(|feature| {
                !feature.id.as_str().starts_with("sldprt:")
                    && matches!(
                        feature.evaluation.definition(),
                        FeatureDefinition::Operation(FeatureOperation::Native { .. })
                    )
            })
            .map(|feature| feature.id.as_str().to_owned())
            .collect(),
        ..Removed::default()
    };
    if seeds.is_empty() {
        return Vec::new();
    }
    remove(ir, &seeds)
        .notes("a native operation of another format, which only its own codec writes")
}

impl Removed {
    fn is_empty(&self) -> bool {
        self.features.is_empty()
            && self.sketches.is_empty()
            && self.constraints.is_empty()
            && self.parameters.is_empty()
            && self.entities.is_empty()
    }

    fn notes(&self, reason: &str) -> Vec<LossNote> {
        let mut notes = Vec::new();
        let list = |ids: &BTreeSet<String>| ids.iter().cloned().collect::<Vec<_>>().join(", ");
        if !self.features.is_empty() || !self.sketches.is_empty() || !self.parameters.is_empty() {
            notes.push(SldprtLossCode::WriterFeaturesOmitted.note(format!(
                "the SLDPRT history writer cannot write a design record ({reason}); it writes the \
                 current geometry without {} feature(s), {} sketch(es) and {} parameter(s) that \
                 are or need that record: features [{}]; sketches [{}]; parameters [{}]",
                self.features.len(),
                self.sketches.len(),
                self.parameters.len(),
                list(&self.features),
                list(&self.sketches),
                list(&self.parameters),
            )));
        }
        if !self.entities.is_empty() {
            notes.push(SldprtLossCode::WriterFeaturesOmitted.note(format!(
                "the SLDPRT history writer cannot write a sketch curve outside the sketch's \
                 profiles ({reason}); it writes the sketches without {} such curve(s): {}",
                self.entities.len(),
                list(&self.entities),
            )));
        }
        if !self.constraints.is_empty() {
            notes.push(SldprtLossCode::WriterSketchConstraintsOmitted.note(format!(
                "the SLDPRT history writer cannot write a sketch constraint ({reason}); it writes \
                 the sketches without {} constraint(s): {}",
                self.constraints.len(),
                list(&self.constraints),
            )));
        }
        notes
    }
}

/// Design-record ids that occur in `message` as whole tokens (§3.2).
fn named_records(ir: &CadIr, message: &str) -> Removed {
    let tokens = message
        .split_whitespace()
        .map(|token| {
            token.trim_matches(|c: char| {
                matches!(c, ',' | ';' | ':' | '.' | '(' | ')' | '\'' | '"' | '`')
            })
        })
        .collect::<BTreeSet<_>>();
    let model = &ir.model;
    let named = |id: &str| tokens.contains(id);
    let mut seeds = Removed::default();
    seeds.features.extend(
        model
            .features
            .iter()
            .map(|feature| feature.id.as_str())
            .filter(|id| named(id))
            .map(str::to_owned),
    );
    seeds.sketches.extend(
        model
            .sketches
            .iter()
            .map(|sketch| sketch.id.as_str())
            .chain(
                model
                    .spatial_sketches
                    .iter()
                    .map(|sketch| sketch.id.as_str()),
            )
            .filter(|id| named(id))
            .map(str::to_owned),
    );
    for entity in &model.sketch_entities {
        if !named(entity.id().as_str()) {
            continue;
        }
        let profiled = model
            .sketches
            .iter()
            .filter(|sketch| sketch.id == entity.sketch)
            .flat_map(|sketch| sketch.profiles.iter().flatten())
            .any(|entity_use| &entity_use.entity == entity.id());
        if profiled {
            seeds.sketches.insert(entity.sketch.as_str().to_owned());
        } else {
            seeds.entities.insert(entity.id().as_str().to_owned());
        }
    }
    seeds.constraints.extend(
        model
            .sketch_constraints
            .iter()
            .map(|constraint| constraint.id.as_str())
            .chain(
                model
                    .spatial_sketch_constraints
                    .iter()
                    .map(|constraint| constraint.id.as_str()),
            )
            .filter(|id| named(id))
            .map(str::to_owned),
    );
    seeds.parameters.extend(
        model
            .parameters
            .iter()
            .map(|parameter| parameter.id.as_str())
            .filter(|id| named(id))
            .map(str::to_owned),
    );
    // A failure that names a constraint concerns that constraint; the other
    // records it names are its operands.
    if !seeds.constraints.is_empty() {
        return Removed {
            constraints: seeds.constraints,
            ..Removed::default()
        };
    }
    seeds
}

fn every_design_record(ir: &CadIr) -> Removed {
    let model = &ir.model;
    Removed {
        features: model
            .features
            .iter()
            .map(|feature| feature.id.as_str().to_owned())
            .collect(),
        sketches: model
            .sketches
            .iter()
            .map(|sketch| sketch.id.as_str().to_owned())
            .chain(
                model
                    .spatial_sketches
                    .iter()
                    .map(|sketch| sketch.id.as_str().to_owned()),
            )
            .collect(),
        constraints: BTreeSet::new(),
        parameters: BTreeSet::new(),
        entities: BTreeSet::new(),
    }
}

fn every_parameter(ir: &CadIr) -> Removed {
    Removed {
        parameters: ir
            .model
            .parameters
            .iter()
            .map(|parameter| parameter.id.as_str().to_owned())
            .collect(),
        ..Removed::default()
    }
}

/// Remove `seeds` and every record that needs one of them, and return what
/// was removed.
fn remove(ir: &mut CadIr, seeds: &Removed) -> Removed {
    let model = &mut ir.model;
    let features = model
        .features
        .iter()
        .map(|feature| {
            (
                feature.id.as_str().to_owned(),
                is_tree_node(feature),
                value(feature),
            )
        })
        .collect::<Vec<_>>();
    let sketches = model
        .sketches
        .iter()
        .map(|sketch| (sketch.id.as_str().to_owned(), value(sketch)))
        .collect::<Vec<_>>();
    let mut dropped = seeds.features.clone();
    dropped.extend(seeds.sketches.iter().cloned());
    dropped.extend(seeds.entities.iter().cloned());
    // A feature or sketch that names a dropped feature or sketch needs it. A
    // tree node only groups its children, and is kept.
    loop {
        let before = dropped.len();
        for (id, tree_node, record) in &features {
            if !tree_node && !dropped.contains(id) && mentions(record, &dropped) {
                dropped.insert(id.clone());
            }
        }
        for (id, record) in &sketches {
            if !dropped.contains(id) && mentions(record, &dropped) {
                dropped.insert(id.clone());
            }
        }
        if dropped.len() == before {
            break;
        }
    }
    // A sketch that only dropped features place is dropped with them.
    for (id, _) in &sketches {
        if dropped.contains(id) {
            continue;
        }
        let single = BTreeSet::from([id.clone()]);
        let mut placed_by_dropped = false;
        let mut placed_by_kept = false;
        for (feature, _, record) in &features {
            if mentions(record, &single) {
                if dropped.contains(feature) {
                    placed_by_dropped = true;
                } else {
                    placed_by_kept = true;
                }
            }
        }
        if placed_by_dropped && !placed_by_kept {
            dropped.insert(id.clone());
        }
    }
    let mut removed = Removed::default();
    model.features.retain(|feature| {
        let keep = !dropped.contains(feature.id.as_str());
        if !keep {
            removed.features.insert(feature.id.as_str().to_owned());
        }
        keep
    });
    model.sketches.retain(|sketch| {
        let keep = !dropped.contains(sketch.id.as_str());
        if !keep {
            removed.sketches.insert(sketch.id.as_str().to_owned());
        }
        keep
    });
    model.spatial_sketches.retain(|sketch| {
        let keep = !dropped.contains(sketch.id.as_str());
        if !keep {
            removed.sketches.insert(sketch.id.as_str().to_owned());
        }
        keep
    });
    model
        .sketch_entities
        .retain(|entity| !removed.sketches.contains(entity.sketch.as_str()));
    model.sketch_entities.retain(|entity| {
        let keep = !seeds.entities.contains(entity.id().as_str());
        if !keep {
            removed.entities.insert(entity.id().as_str().to_owned());
        }
        keep
    });
    model
        .spatial_sketch_entities
        .retain(|entity| !mentions(&value(entity), &removed.sketches));
    model.sketch_constraints.retain(|constraint| {
        let keep = !removed.sketches.contains(constraint.sketch.as_str())
            && !seeds.constraints.contains(constraint.id.as_str())
            && !mentions(&value(constraint), &removed.entities);
        if !keep && !removed.sketches.contains(constraint.sketch.as_str()) {
            removed
                .constraints
                .insert(constraint.id.as_str().to_owned());
        }
        keep
    });
    model.spatial_sketch_constraints.retain(|constraint| {
        let in_dropped_sketch = mentions(&value(constraint), &removed.sketches);
        let keep = !in_dropped_sketch && !seeds.constraints.contains(constraint.id.as_str());
        if !keep && !in_dropped_sketch {
            removed
                .constraints
                .insert(constraint.id.as_str().to_owned());
        }
        keep
    });
    model
        .feature_input_topologies
        .retain(|row| !removed.features.contains(row.input_of.as_str()));
    model
        .feature_result_topologies
        .retain(|row| !removed.features.contains(row.output_of.as_str()));
    model.parameters.retain(|parameter| {
        let owned = parameter
            .owner
            .as_ref()
            .is_some_and(|owner| removed.features.contains(owner.as_str()));
        let keep = !owned && !seeds.parameters.contains(parameter.id.as_str());
        if !keep {
            removed.parameters.insert(parameter.id.as_str().to_owned());
        }
        keep
    });
    for configuration in &mut model.configurations {
        configuration
            .feature_states
            .retain(|feature, _| !removed.features.contains(feature.as_str()));
        for state in configuration.feature_states.values_mut() {
            state
                .dependencies
                .retain(|feature| !removed.features.contains(feature.as_str()));
        }
        configuration
            .parameter_values
            .retain(|parameter, _| !removed.parameters.contains(parameter.as_str()));
    }
    removed
}

fn is_tree_node(feature: &cadmpeg_ir::features::Feature) -> bool {
    matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::TreeNode { .. })
    )
}

fn value(record: &impl Serialize) -> Value {
    serde_json::to_value(record).unwrap_or(Value::Null)
}

/// Whether a string below the record's own `id` equals one of `ids`.
fn mentions(record: &Value, ids: &BTreeSet<String>) -> bool {
    fn walk(value: &Value, ids: &BTreeSet<String>) -> bool {
        match value {
            Value::String(text) => ids.contains(text),
            Value::Array(items) => items.iter().any(|item| walk(item, ids)),
            Value::Object(map) => map.values().any(|item| walk(item, ids)),
            _ => false,
        }
    }
    match record {
        Value::Object(map) => map.iter().any(|(key, item)| key != "id" && walk(item, ids)),
        other => walk(other, ids),
    }
}

#[cfg(test)]
mod tests;
