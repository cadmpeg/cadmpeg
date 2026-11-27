// SPDX-License-Identifier: Apache-2.0
//! A decoder's projection of its records into the v11 design layer.
//!
//! The projection reads no source byte: it restates records the decoder
//! already read. A projection that fails is given up as one part (§3.3 of
//! the CADIR v11 specification): the design layer stays as it was before the
//! projection, and one `record.unreadable` loss names the layer.

use std::collections::BTreeSet;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::annotations::{AnnotationBuilder, AnnotationKey, ExactnessNote};
use crate::document::CadIr;
use crate::provenance::Exactness;
use crate::report::loss::LossNote;
use crate::report::Severity;
use crate::unknown::part::record_unreadable_kind;

use self::fields::{
    ArrangementField, FeatureField, Field, ModelConfigurationField, ModelFeatureField,
    ModelParameterField, ParameterField,
};

pub mod fields;
pub mod restated;
pub mod sketch;

/// Runs `project`, the `codec` decoder's projection of the v11 design layer
/// of `ir`, as one part.
///
/// Where `project` fails with an error that does not end the session, `ir`
/// holds the design records and product definitions it held before the
/// projection, `losses` holds the losses it held before the projection and
/// one `record.unreadable` that names the layer, and the model holds the
/// restatements it held before the projection. The geometry and the model
/// records stand. Where `project` succeeds, each design feature that
/// restates a model feature under its identity states the fields it leaves
/// out ([`restated::restate_features`]).
///
/// # Errors
///
/// An error that ends the session: a refusal in force, a Session-scoped
/// resource refusal, or an I/O error; and a resource refusal while the
/// restatements are stated.
pub fn project_layer(
    ctx: &DecodeContext<'_>,
    codec: &str,
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
    project: impl FnOnce(&mut CadIr, &mut Vec<LossNote>) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    let design = ir.design.clone();
    let definitions = ir.model.product_definitions.clone();
    let restated = ir.model.restated.clone();
    let reported = losses.len();
    if let Err(error) = ctx.run_part(|| project(ir, losses))? {
        ir.design = design;
        ir.model.product_definitions = definitions;
        ir.model.restated = restated;
        losses.truncate(reported);
        losses.push(
            LossNote::new(
                record_unreadable_kind(),
                format!("{codec} v11 design layer is not stated: {error}"),
            )
            .with_severity(Severity::Error),
        );
    } else {
        restated::restate_features(ctx, ir)?;
    }
    Ok(())
}

/// The model fields a decoder's projection reads to build each field of a
/// design record. A design field with no model field is the design record's
/// own.
#[derive(Clone, Copy)]
pub struct Restatement {
    /// The model feature fields each design feature field is built from.
    pub feature: fn(FeatureField) -> &'static [ModelFeatureField],
    /// The model parameter fields each design parameter field is built from.
    pub parameter: fn(ParameterField) -> &'static [ModelParameterField],
    /// The model configuration fields each field of an arrangement that
    /// restates the configuration is built from.
    pub arrangement: fn(ArrangementField) -> &'static [ModelConfigurationField],
    /// The fields of each model configuration that an arrangement of a
    /// scope restates which the scope's `active_arrangements` is built from.
    pub active_arrangements: &'static [ModelConfigurationField],
}

/// The less exact of `left` and `right`, in the declared order of
/// [`Exactness`].
fn least_exact(left: Exactness, right: Exactness) -> Exactness {
    let rank = |exactness| match exactness {
        Exactness::ByteExact => 0,
        Exactness::Derived => 1,
        Exactness::Inferred => 2,
        Exactness::Unknown => 3,
    };
    if rank(right) > rank(left) {
        right
    } else {
        left
    }
}

/// The mark of the model field `key` of `note`: the least exact of its own
/// mark (else the entity exactness) and the marks on its sub-paths.
fn model_mark(note: &ExactnessNote, key: &str) -> Exactness {
    sub_marks(note, key).map(|(_, exactness)| exactness).fold(
        note.fields().get(key).copied().unwrap_or(note.entity()),
        least_exact,
    )
}

/// The marks of `note` on sub-paths of `field`, with their path.
fn sub_marks<'n>(
    note: &'n ExactnessNote,
    field: &'n str,
) -> impl Iterator<Item = (&'n str, Exactness)> + 'n {
    note.fields().iter().filter_map(move |(path, exactness)| {
        let path = path.as_str();
        path.strip_prefix(field)?
            .starts_with('.')
            .then_some((path, *exactness))
    })
}

/// The field overrides of `note` on a design record whose fields are
/// `stated`, built from the model fields `inputs` names.
///
/// A model field counts at the least exact of its mark (else the entity
/// exactness) and the marks on its sub-paths. A stated design field takes the
/// least exact of its model fields and the mark on its own name. Where that
/// name is also a model field, the marks on its sub-paths fold into the field
/// too, since the two records can shape the field differently; and where
/// `inputs` does not list that model field, those marks count at no more
/// than the entity exactness, since they may describe the model field. Under
/// a name only the design record has, a sub-path mark is kept at no more than
/// its field. Marks on a design field the record does not state, and model
/// marks no stated field reads, leave.
fn restated_fields<D: Field, M: Field>(
    note: &ExactnessNote,
    stated: impl IntoIterator<Item = (D, bool)>,
    inputs: fn(D) -> &'static [M],
) -> Vec<(String, Exactness)> {
    let entity = note.entity();
    let input = |model: &M| model_mark(note, model.key());
    let mut overrides = Vec::new();
    for (field, stated) in stated {
        if !stated {
            continue;
        }
        let key = field.key();
        let shared = M::parse(key).is_some();
        let unlisted = shared && !inputs(field).iter().any(|model| model.key() == key);
        let own = note
            .fields()
            .get(key)
            .copied()
            .into_iter()
            .chain(
                sub_marks(note, key)
                    .filter(|_| shared)
                    .map(|(_, exactness)| exactness),
            )
            .map(|mark| {
                if unlisted {
                    least_exact(mark, entity)
                } else {
                    mark
                }
            });
        let exactness = inputs(field)
            .iter()
            .map(input)
            .chain(own)
            .reduce(least_exact)
            .unwrap_or(entity);
        if exactness != entity {
            overrides.push((key.to_owned(), exactness));
        }
        if !shared {
            overrides.extend(
                sub_marks(note, key)
                    .map(|(path, mark)| (path.to_owned(), least_exact(mark, exactness))),
            );
        }
    }
    overrides
}

/// Restates onto each design feature, parameter and arrangement of `ir` the
/// exactness of the model feature, parameter or configuration with its id,
/// through the model fields `restatement` names for each design field. The
/// CADIR wire carries the design record in place of the model record (§10).
///
/// Each scope's `active_arrangements` takes the least exact mark of the
/// fields `restatement.active_arrangements` names of each configuration that
/// an arrangement of the scope restates. Where that mark is less exact than
/// the scope's key, the id of its definition, it is stated there as the mark
/// of the field `active_arrangements` (interim: §3.2 does not say which of
/// the two records with that id a field path names).
///
/// # Errors
///
/// A resource refusal while the overrides are stated.
pub fn restate_exactness(
    ir: &CadIr,
    annotations: &mut AnnotationBuilder,
    restatement: Restatement,
) -> Result<(), CodecError> {
    let modeled = ir
        .model
        .features
        .iter()
        .map(|feature| feature.id.as_str())
        .chain(
            ir.model
                .parameters
                .iter()
                .map(|parameter| parameter.id.as_str()),
        )
        .collect::<BTreeSet<_>>();
    let configurations = ir
        .model
        .configurations
        .iter()
        .map(|configuration| configuration.id.as_str())
        .collect::<BTreeSet<_>>();
    let exactness = annotations.annotations().exactness();
    let mut scopes = Vec::new();
    for scope in &ir.design.scopes {
        let mark = ir
            .design
            .arrangements
            .iter()
            .filter(|arrangement| {
                arrangement.scope == scope.definition
                    && configurations.contains(arrangement.id.as_str())
            })
            .flat_map(|arrangement| {
                let note = exactness.get(arrangement.id.as_str());
                restatement.active_arrangements.iter().map(move |field| {
                    note.map_or(Exactness::ByteExact, |note| model_mark(note, field.key()))
                })
            })
            .reduce(least_exact);
        let key = AnnotationKey::from(&scope.definition);
        let entity = exactness
            .get(key.as_str())
            .map_or(Exactness::ByteExact, ExactnessNote::entity);
        if let Some(mark) = mark.filter(|mark| least_exact(*mark, entity) != entity) {
            scopes.push((key, mark));
        }
    }
    let mut restated = Vec::new();
    for feature in &ir.design.features {
        if let (true, Some(note)) = (
            modeled.contains(feature.id.as_str()),
            exactness.get(feature.id.as_str()),
        ) {
            let overrides =
                restated_fields(note, FeatureField::stated(feature), restatement.feature);
            restated.push((AnnotationKey::from(&feature.id), note.entity(), overrides));
        }
    }
    for parameter in &ir.design.parameters {
        if let (true, Some(note)) = (
            modeled.contains(parameter.id.as_str()),
            exactness.get(parameter.id.as_str()),
        ) {
            let overrides = restated_fields(
                note,
                ParameterField::stated(parameter),
                restatement.parameter,
            );
            restated.push((AnnotationKey::from(&parameter.id), note.entity(), overrides));
        }
    }
    for arrangement in &ir.design.arrangements {
        if let (true, Some(note)) = (
            configurations.contains(arrangement.id.as_str()),
            exactness.get(arrangement.id.as_str()),
        ) {
            let overrides = restated_fields(
                note,
                ArrangementField::stated(arrangement),
                restatement.arrangement,
            );
            restated.push((
                AnnotationKey::from(&arrangement.id),
                note.entity(),
                overrides,
            ));
        }
    }
    let keys = restated
        .iter()
        .map(|(key, ..)| key.as_str())
        .collect::<BTreeSet<_>>();
    annotations.retain_exactness(|owner| !keys.contains(owner));
    for (key, entity, overrides) in restated {
        annotations.exactness(&key, entity);
        for (field, exactness) in overrides {
            annotations.field_exactness(&key, field, exactness)?;
        }
    }
    for (key, mark) in scopes {
        annotations.field_exactness(&key, "active_arrangements", mark)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
