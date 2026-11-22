// SPDX-License-Identifier: Apache-2.0
//! The records of the states of configurations that are not active.
//!
//! Every `Config-N-Partition` decodes into one B-rep, so the model records
//! first name the bodies of every configuration. The current geometry holds
//! the active configuration's state alone (§5.5), and the design projection
//! states each other configuration's stored records in that configuration's
//! evaluation. [`retire`] then removes them from the geometry, with what the
//! model says of them: the body membership and feature outputs that the
//! evaluation states, and the presentation records on them, which a
//! model-class target cannot yet name in an evaluation (interim, §8).

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::appearance::AppearanceTarget;
use cadmpeg_ir::design::evaluation::StateBase;
use cadmpeg_ir::document::{CadIr, Topology};
use cadmpeg_ir::features::parameters::{ConfigurationEvaluation, DesignConfiguration};
use cadmpeg_ir::features::ConfigurationId;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::AnnotationBuilder;

use crate::design_v11::states::remove_records;
use crate::loss::SldprtLossCode;

/// Removes the `retired` geometry records, which the evaluations of
/// configurations that are not active hold, with every model reference to
/// them: [`retire_model`]; the appearance bindings and tessellations on
/// them, and the appearances only those bindings used, with one loss per
/// configuration; their annotations and the unknown-record links to them.
pub(super) fn retire(
    ir: &mut CadIr,
    annotations: &mut Annotations,
    unknowns: &mut [UnknownRecord],
    retired: &BTreeMap<String, ConfigurationId>,
    losses: &mut Vec<LossNote>,
) {
    if retired.is_empty() {
        return;
    }
    retire_model(ir, retired);
    let gone = |id: &str| retired.contains_key(id);
    let mut presentation = BTreeMap::<&ConfigurationId, (usize, usize)>::new();
    let mut tessellations = BTreeMap::new();
    for tessellation in &ir.model.tessellations {
        let configuration = tessellation
            .body
            .iter()
            .map(cadmpeg_ir::ids::BodyId::as_str)
            .chain(
                tessellation
                    .faces
                    .iter()
                    .map(cadmpeg_ir::ids::FaceId::as_str),
            )
            .find_map(|id| retired.get(id));
        if let Some(configuration) = configuration {
            presentation.entry(configuration).or_default().1 += 1;
            tessellations.insert(tessellation.id.as_str().to_owned(), configuration);
        }
    }
    let mut dropped = tessellations.keys().cloned().collect::<BTreeSet<_>>();
    let mut unbound = BTreeSet::new();
    for binding in &ir.model.appearance_bindings {
        let configuration = match &binding.target.entity {
            AppearanceTarget::Tessellation(tessellation)
            | AppearanceTarget::TriangleGroup { tessellation, .. } => {
                tessellations.get(tessellation).copied()
            }
            target => target_id(target).and_then(|id| retired.get(id)),
        };
        if let Some(configuration) = configuration {
            presentation.entry(configuration).or_default().0 += 1;
            dropped.insert(binding.id.as_str().to_owned());
            unbound.insert(binding.appearance.clone());
        }
    }
    for (configuration, (bindings, tessellations)) in presentation {
        losses.push(
            SldprtLossCode::ConfigStatePresentationUnstated.note(format!(
            "configuration {configuration}: {bindings} appearance binding(s) and {tessellations} \
             tessellation(s) on the records of its state are not stated, because a model target \
             cannot name a state that is not current; the faces of its state keep their colours."
        )),
        );
    }
    ir.model
        .appearance_bindings
        .retain(|binding| !dropped.contains(binding.id.as_str()));
    ir.model
        .tessellations
        .retain(|tessellation| !dropped.contains(tessellation.id.as_str()));
    for binding in &ir.model.appearance_bindings {
        unbound.remove(&binding.appearance);
    }
    ir.model.appearances.retain(|appearance| {
        let only_dropped = unbound.contains(&appearance.id);
        if only_dropped {
            dropped.insert(appearance.id.as_str().to_owned());
        }
        !only_dropped
    });

    for record in unknowns.iter_mut() {
        record
            .links_mut()
            .retain(|link| !gone(link) && !dropped.contains(link));
    }
    let mut keep = |id: &str| !gone(id) && !dropped.contains(id);
    annotations.retain_provenance(&mut keep);
    let mut builder = AnnotationBuilder::resume(std::mem::take(annotations));
    builder.retain_exactness(keep);
    *annotations = builder.build();
}

/// The id of the geometry record an appearance target names.
fn target_id(target: &AppearanceTarget) -> Option<&str> {
    match target {
        AppearanceTarget::Body(id) => Some(id.as_str()),
        AppearanceTarget::Face(id) => Some(id.as_str()),
        AppearanceTarget::Edge(id) => Some(id.as_str()),
        AppearanceTarget::Vertex(id) => Some(id.as_str()),
        AppearanceTarget::Surface(id) => Some(id.as_str()),
        AppearanceTarget::Curve(id) => Some(id.as_str()),
        AppearanceTarget::Point(id) => Some(id.as_str()),
        AppearanceTarget::Occurrence(_)
        | AppearanceTarget::Tessellation(_)
        | AppearanceTarget::TriangleGroup { .. }
        | AppearanceTarget::Record { .. }
        | AppearanceTarget::Source { .. } => None,
    }
}

/// Removes the `retired` geometry records from `ir.geometry`, the feature
/// outputs and configuration feature-state outputs that name them, and the
/// body list of each configuration whose bodies they are, since its
/// evaluation states them.
fn retire_model(ir: &mut CadIr, retired: &BTreeMap<String, ConfigurationId>) {
    let gone = |id: &str| retired.contains_key(id);
    remove_records(&mut ir.geometry, retired);
    for feature in &mut ir.model.features {
        if feature
            .evaluation
            .outputs()
            .iter()
            .any(|body| gone(body.as_str()))
        {
            let kept = feature
                .evaluation
                .outputs()
                .iter()
                .filter(|body| !gone(body.as_str()))
                .cloned()
                .collect();
            feature.evaluation.set_outputs(kept);
        }
    }
    for configuration in &mut ir.model.configurations {
        if configuration
            .bodies
            .as_deref()
            .is_some_and(|bodies| bodies.iter().any(|body| gone(body.as_str())))
        {
            configuration.bodies = None;
        }
        for state in configuration.feature_states.values_mut() {
            if let ConfigurationEvaluation::Active { outputs } = &mut state.evaluation {
                outputs.retain(|body| !gone(body.as_str()));
            }
        }
    }
}

/// The state the design stores for a model configuration whose body list
/// [`retire_model`] removed: the records of the evaluation, based on
/// `Empty`, of the design configuration with the same native record.
pub(crate) fn stored_state<'a>(
    ir: &'a CadIr,
    configuration: &DesignConfiguration,
) -> Option<(&'a ConfigurationId, &'a Topology)> {
    if configuration.bodies.is_some() {
        return None;
    }
    let native_ref = configuration.native_ref.as_ref()?;
    let design = ir
        .design
        .configurations
        .iter()
        .find(|design| design.native_ref.as_ref() == Some(native_ref))?;
    ir.design.evaluations.iter().find_map(|evaluation| {
        let content = evaluation.content.as_ref()?;
        (evaluation.configuration.as_ref() == Some(&design.id) && content.base == StateBase::Empty)
            .then_some((&design.id, &content.delta.records))
    })
}

#[cfg(test)]
mod tests;
