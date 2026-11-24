// SPDX-License-Identifier: Apache-2.0
//! The records of the states of configurations that are not active.
//!
//! Every `Config-N-Partition` decodes into one B-rep, so the model records
//! first name the bodies of every configuration. The current geometry holds
//! the active configuration's state alone (§5.5), and the design projection
//! states each other configuration's stored records in that configuration's
//! evaluation. [`retire`] then removes them from the geometry, with what the
//! model says of them: the body membership, which the evaluation states; the
//! document feature outputs that name them, which state the current
//! geometry; the configuration feature-state outputs that name them, which
//! the evaluation's name table states where a face of the body carries a
//! name of the feature (§5.7), with one loss per configuration for the
//! outputs no such name states; and the appearance bindings on them, which a
//! model target cannot yet name in a state that is not current (interim,
//! §8), with one loss per configuration. The display tables bind to the
//! active configuration's faces only, so none names a retired record.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::appearance::AppearanceTarget;
use cadmpeg_ir::design::evaluation::{StateBase, StateDelta};
use cadmpeg_ir::design::names::TopoName;
use cadmpeg_ir::design::refs::EntityRef;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::parameters::{ConfigurationEvaluation, DesignConfiguration};
use cadmpeg_ir::features::{ConfigurationId, FeatureId};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::AnnotationBuilder;

use crate::design_v11::states::remove_records;
use crate::loss::SldprtLossCode;

/// Removes the `retired` geometry records, which the evaluations of
/// configurations that are not active hold, with every model reference to
/// them: [`retire_model`], with one loss per configuration for the feature
/// outputs on them that its evaluation's face names do not state; the
/// appearance bindings on them, and the
/// appearances only those bindings used, with one loss per configuration;
/// their annotations and the unknown-record links to them.
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
    let gone = |id: &str| retired.contains_key(id);
    let mut dropped = BTreeSet::new();
    let mut unbound = BTreeSet::new();
    {
        let colours = ir
            .geometry
            .faces
            .iter()
            .filter_map(|face| Some((face.id.as_str(), face.color?)))
            .collect::<HashMap<_, _>>();
        let base_colours = ir
            .model
            .appearances
            .iter()
            .map(|appearance| (&appearance.id, appearance.base_color))
            .collect::<HashMap<_, _>>();
        // Per configuration: the bindings, and the face bindings whose colour
        // the face record states.
        let mut bindings = BTreeMap::<&ConfigurationId, (usize, usize)>::new();
        for binding in &ir.model.appearance_bindings {
            let Some((id, configuration)) =
                target_id(&binding.target.entity).and_then(|id| Some((id, retired.get(id)?)))
            else {
                continue;
            };
            let on_face = matches!(binding.target.entity, AppearanceTarget::Face(_))
                && colours.get(id).is_some_and(|colour| {
                    base_colours.get(&binding.appearance).copied().flatten() == Some(*colour)
                });
            let counts = bindings.entry(configuration).or_default();
            counts.0 += 1;
            counts.1 += usize::from(on_face);
            dropped.insert(binding.id.as_str().to_owned());
            unbound.insert(binding.appearance.clone());
        }
        for (configuration, (count, on_faces)) in bindings {
            losses.push(
                SldprtLossCode::ConfigStatePresentationUnstated.note(format!(
                "configuration {configuration}: {count} appearance binding(s) on the records of \
                 its state are not stated, because a model target cannot name a state that is \
                 not current; {on_faces} of them state the colour that their face's record \
                 keeps."
            )),
            );
        }
    }
    for (configuration, count) in retire_model(ir, retired) {
        losses.push(SldprtLossCode::ConfigStateOutputsUnstated.note(format!(
            "configuration {configuration}: {count} feature output(s) on the bodies of its state \
             are not stated: a model feature state cannot name a body of a state that is not \
             current, and the configuration's evaluation names that feature on no face of that \
             body."
        )));
    }
    ir.model
        .appearance_bindings
        .retain(|binding| !dropped.contains(binding.id.as_str()));
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
        | AppearanceTarget::Unresolved => None,
    }
}

/// Removes the `retired` geometry records from `ir.geometry`, the feature
/// outputs and configuration feature-state outputs that name them, and the
/// body list of each configuration whose bodies they are, since its
/// evaluation states them. Returns, per configuration whose state holds the
/// bodies, the number of configuration feature-state outputs removed whose
/// feature the configuration's evaluation names on no face of the body.
fn retire_model(
    ir: &mut CadIr,
    retired: &BTreeMap<String, ConfigurationId>,
) -> BTreeMap<ConfigurationId, usize> {
    let gone = |id: &str| retired.contains_key(id);
    let named = named_producers(ir, retired);
    let mut unstated = BTreeMap::<ConfigurationId, usize>::new();
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
        for (feature, state) in &mut configuration.feature_states {
            if let ConfigurationEvaluation::Active { outputs } = &mut state.evaluation {
                for body in outputs.iter() {
                    let Some(owner) = retired.get(body.as_str()) else {
                        continue;
                    };
                    let stated = named
                        .get(&(owner.clone(), body.clone()))
                        .is_some_and(|features| features.contains(feature));
                    if !stated {
                        *unstated.entry(owner.clone()).or_default() += 1;
                    }
                }
                outputs.retain(|body| !gone(body.as_str()));
            }
        }
    }
    unstated
}

/// Per configuration of `retired` and body of its state, the features that
/// the names of the body's faces in the configuration's evaluations name
/// (§5.7).
fn named_producers(
    ir: &CadIr,
    retired: &BTreeMap<String, ConfigurationId>,
) -> BTreeMap<(ConfigurationId, BodyId), BTreeSet<FeatureId>> {
    let configurations = retired.values().collect::<BTreeSet<_>>();
    let mut named = BTreeMap::<_, BTreeSet<FeatureId>>::new();
    for evaluation in &ir.design.evaluations {
        let (Some(configuration), Some(content)) = (&evaluation.configuration, &evaluation.content)
        else {
            continue;
        };
        if !configurations.contains(configuration) {
            continue;
        }
        let records = &content.delta.records;
        let region_body = records
            .regions
            .iter()
            .map(|region| (&region.id, &region.body))
            .collect::<HashMap<_, _>>();
        let shell_body = records
            .shells
            .iter()
            .filter_map(|shell| Some((&shell.id, *region_body.get(&shell.region)?)))
            .collect::<HashMap<_, _>>();
        let face_body = records
            .faces
            .iter()
            .filter_map(|face| Some((&face.id, *shell_body.get(&face.shell)?)))
            .collect::<HashMap<_, _>>();
        for (entity, entry) in &content.delta.names {
            let EntityRef::Face(face) = entity else {
                continue;
            };
            let (Some(body), Some(name)) = (
                face_body.get(face),
                entry.as_ref().and_then(|entry| entry.name.as_ref()),
            ) else {
                continue;
            };
            let features = named
                .entry((configuration.clone(), (*body).clone()))
                .or_default();
            name_features(name, &mut |feature| {
                features.insert(feature.clone());
            });
        }
    }
    named
}

/// The features a face's name states made it: the feature of the name and of
/// each name it was made from. The names of other entities that a name holds
/// (the faces a blend joins, beside an edge, or at a vertex) are not followed.
fn name_features(name: &TopoName, found: &mut impl FnMut(&FeatureId)) {
    match name {
        TopoName::Generated { feature, .. } | TopoName::Blend { feature, .. } => found(feature),
        TopoName::Instance { feature, of, .. } => {
            found(feature);
            name_features(of, found);
        }
        TopoName::Modified { feature, from } => {
            found(feature);
            name_features(from, found);
        }
        TopoName::Split { feature, from, .. } => {
            if let Some(feature) = feature {
                found(feature);
            }
            name_features(from, found);
        }
        TopoName::Merged { feature, from } => {
            if let Some(feature) = feature {
                found(feature);
            }
            for from in from {
                name_features(from, found);
            }
        }
        TopoName::Bounded { .. } | TopoName::Meeting { .. } | TopoName::Root { .. } => {}
    }
}

/// The state the design stores for a model configuration whose body list
/// [`retire_model`] removed: the delta (its records and names) of the
/// evaluation, based on `Empty`, of the design configuration with the same
/// native record.
pub(crate) fn stored_state<'a>(
    ir: &'a CadIr,
    configuration: &DesignConfiguration,
) -> Option<(&'a ConfigurationId, &'a StateDelta)> {
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
            .then_some((&design.id, &content.delta))
    })
}

#[cfg(test)]
mod tests;
