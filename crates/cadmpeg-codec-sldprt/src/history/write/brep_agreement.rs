// SPDX-License-Identifier: Apache-2.0
//! Agreement between edited construction history and the output B-rep.
//!
//! The writer cannot regenerate a solid. An edit that shapes it is written as
//! edited over the stored solid, and `writer.solid-stale-after-edit` names it.
//! The baseline of every comparison is the retained source image decoded
//! again, which is the document the caller edited. Without that image, the
//! decoder's stamped digests name the kinds of record that differ from the
//! decoded source, and the writer cannot tell whether the edit shapes the
//! solid.

use std::collections::{BTreeSet, HashSet};
use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::features::parameters::{DesignConfiguration, DesignParameter};
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation, ParameterId};
use cadmpeg_ir::report::loss::LossNote;

use super::edits::{UnwrittenEdits, UnwrittenPart};
use crate::history::hash::{configuration_hash, feature_hash, history_hash, parameter_hash};
use crate::loss::SldprtLossCode;
use crate::native::SldprtNative;

/// The losses of one write that the stored solid and the synthesized
/// configuration snapshot cause: one stale-solid loss per construction edit
/// that shapes the solid, and one loss per snapshot edit the model does not
/// state. The writer has no modeling kernel, so the output keeps the stored
/// solid and its caches as they were before the edit. `unwritten` names the
/// edits the write does not carry: the construction edit of a feature in its
/// `kept_definitions` is not written, and a part
/// of a parameter edit it names does not reach the written history, so no
/// stale-solid loss calls them written. `misread` holds the features whose
/// tree place (`ordinal`) or output bodies (`outputs`) the written part reads
/// back other than the document states; the read-back names them, and no
/// stale-solid loss calls them written. Without the retained source image,
/// [`imageless_losses`] names what it can tell.
pub(crate) fn stale_solid_losses(
    ir: &CadIr,
    native: Option<&SldprtNative>,
    source_image: Option<&[u8]>,
    unwritten: &UnwrittenEdits,
    misread: &HashSet<(FeatureId, String)>,
) -> Result<Vec<LossNote>, CodecError> {
    let kept_definitions = &unwritten.kept_definitions;
    let (Some(source), Some(native)) = (ir.source.as_ref(), native) else {
        return Ok(Vec::new());
    };
    let hash_arena = DecodeArena::new();
    let (hash_ctx, _) = DecodeContext::from_root_bytes(&[], &hash_arena, &DecodePolicy::service())?;
    let features_changed = differs(source, "sldprt_neutral_feature_local_sha256", || {
        feature_hash(&hash_ctx, &ir.model)
    })?;
    let parameters_changed = differs(source, "sldprt_neutral_parameter_local_sha256", || {
        parameter_hash(&hash_ctx, &ir.model.parameters)
    })?;
    let native_changed = differs(source, "sldprt_native_history_sha256", || {
        history_hash(&hash_ctx, &native.feature_histories)
    })?;
    let Some(source_image) = source_image else {
        return imageless_losses(
            ir,
            source,
            &hash_ctx,
            [
                (features_changed, "features"),
                (parameters_changed, "parameters"),
                (native_changed, "native history records"),
            ],
        );
    };
    let configurations_changed =
        differs(source, "sldprt_neutral_configuration_local_sha256", || {
            configuration_hash(&hash_ctx, &ir.model.configurations)
        })?;
    if !(features_changed || parameters_changed || native_changed || configurations_changed) {
        return Ok(Vec::new());
    }
    let decoded = crate::SldprtCodec
        .decode(&mut Cursor::new(source_image), &DecodeOptions::default())
        .map_err(|failure| match failure {
            cadmpeg_ir::codec::DecodeFailure::Codec(error) => error,
            other => CodecError::malformed(format_args!(
                "SLDPRT retained source cannot be decoded: {other}"
            )),
        })?;
    let baseline = decoded.ir();
    let mut losses = Vec::new();
    // Features a model-level loss names; an active configuration with stated
    // states mirrors the model, so its state edit of the same feature is the
    // same edit.
    let mut named = HashSet::new();
    if features_changed {
        feature_edits(
            ir,
            baseline,
            &hash_ctx,
            (kept_definitions, misread),
            &mut named,
            &mut losses,
        )?;
    }
    if parameters_changed {
        parameter_edits(ir, baseline, unwritten, &mut losses);
    }
    if native_changed {
        native_edits(ir, native, baseline, &mut losses)?;
    }
    if configurations_changed {
        configuration_design_edits(ir, baseline, &named, unwritten, &mut losses);
    }
    Ok(losses)
}

/// The losses of a write without the retained source image, which leaves no
/// decoded baseline: one loss naming the kinds of record whose stamped digest
/// moved, and one per difference between the synthesized active snapshot and
/// the model it restates. `changed` holds the feature, parameter and native
/// history digests.
fn imageless_losses(
    ir: &CadIr,
    source: &SourceMeta,
    hash_ctx: &DecodeContext<'_>,
    changed: [(bool, &'static str); 3],
) -> Result<Vec<LossNote>, CodecError> {
    // The decoder stamps the configuration digest before it synthesizes the
    // active snapshot.
    let configurations_changed =
        differs(source, "sldprt_neutral_configuration_local_sha256", || {
            configuration_hash(
                hash_ctx,
                &crate::writer::configurations_without_synthesized_snapshot(ir),
            )
        })?;
    let edited = changed
        .into_iter()
        .chain([(configurations_changed, "configurations")])
        .filter_map(|(moved, records)| moved.then_some(records))
        .collect::<Vec<_>>();
    let mut losses = Vec::new();
    let records = match edited.as_slice() {
        [] => None,
        [one] => Some((*one).to_owned()),
        [rest @ .., last] => Some(format!("{} and {last}", rest.join(", "))),
    };
    if let Some(records) = records {
        losses.push(stale(format!(
            "The {records} differ from the decoded source. The stored solid is written as decoded, and without the retained source image the writer cannot tell whether the edit shapes it."
        )));
    }
    losses.extend(imageless_snapshot_edits(ir));
    Ok(losses)
}

/// One loss per parameter value and feature state in which the synthesized
/// active snapshot differs from the model. Without a baseline the writer
/// cannot tell which side was edited; it writes the model either way.
fn imageless_snapshot_edits(ir: &CadIr) -> Vec<LossNote> {
    let Some(configuration) = ir
        .source
        .as_ref()
        .and_then(|source| {
            source
                .attributes
                .get("sldprt_configuration_snapshot_synthesized")
        })
        .and_then(|id| {
            ir.model
                .configurations
                .iter()
                .find(|configuration| configuration.id.as_str() == id.as_str())
        })
    else {
        return Vec::new();
    };
    let model = crate::decode::model_snapshot(&ir.model);
    let configuration_label = configuration_label(configuration);
    let mut losses = Vec::new();
    for id in configuration
        .parameter_values
        .keys()
        .chain(model.parameter_values.keys())
        .collect::<BTreeSet<_>>()
    {
        if configuration.parameter_values.get(id) != model.parameter_values.get(id) {
            losses.push(snapshot_unwritten(format!(
                "The value of parameter {} in configuration {configuration_label} differs between the configuration snapshot the decoder synthesized from the model and the model. The writer does not write that snapshot, and the written part restates it from the model.",
                parameter_id_label(id, &ir.model.parameters)
            )));
        }
    }
    for id in configuration
        .feature_states
        .keys()
        .chain(model.feature_states.keys())
        .collect::<BTreeSet<_>>()
    {
        if configuration.feature_states.get(id) != model.feature_states.get(id) {
            let label = ir
                .model
                .features
                .iter()
                .find(|feature| &feature.id == id)
                .map_or_else(|| id.as_str().to_owned(), feature_label);
            losses.push(snapshot_unwritten(format!(
                "The state of feature {label} in configuration {configuration_label} differs between the configuration snapshot the decoder synthesized from the model and the model. The writer does not write that snapshot, and the written part restates it from the model."
            )));
        }
    }
    losses
}

/// Whether the digest `current` computes differs from the baseline digest the
/// decoder stamped as `attribute`; `false` without a stamped baseline.
fn differs(
    source: &SourceMeta,
    attribute: &str,
    current: impl FnOnce() -> Result<String, CodecError>,
) -> Result<bool, CodecError> {
    match source.attributes.get(attribute) {
        Some(expected) => Ok(expected != &current()?),
        None => Ok(false),
    }
}

fn feature_edits(
    ir: &CadIr,
    source: &CadIr,
    hash_ctx: &DecodeContext<'_>,
    (kept_definitions, misread): (&HashSet<FeatureId>, &HashSet<(FeatureId, String)>),
    named: &mut HashSet<FeatureId>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let misread_field =
        |id: &FeatureId, field: &str| misread.contains(&(id.clone(), field.to_owned()));
    let baseline = &source.model.features;
    // Whether a construction edit, a removal or an output edit explains the
    // changed feature digest; only an unexplained change is a tree edit.
    let mut explained = false;
    for feature in &ir.model.features {
        let Some(original) = baseline.iter().find(|original| original.id == feature.id) else {
            explained = true;
            if feature_affects_brep(feature, &ir.model.features) {
                named.insert(feature.id.clone());
                losses.push(stale(format!(
                    "Feature {} is added to the written history, and the stored solid does not include it.",
                    feature_label(feature)
                )));
            }
            continue;
        };
        if original.evaluation.outputs() != feature.evaluation.outputs() {
            explained = true;
            named.insert(feature.id.clone());
            if misread_field(&feature.id, "outputs") {
                continue;
            }
            losses.push(stale(format!(
                "Feature {} is written with its edited output bodies, and the stored solid shows the state before the edit.",
                feature_label(feature)
            )));
            continue;
        }
        let definition_written = !kept_definitions.contains(&feature.id);
        if !definition_written
            && (original.evaluation.definition() != feature.evaluation.definition()
                || original.dependencies != feature.dependencies
                || original.source_properties != feature.source_properties)
        {
            // The definition edit is not written; an unwritten-edit loss
            // names it.
            explained = true;
        }
        if !construction_changed(original, feature, definition_written) {
            continue;
        }
        explained = true;
        if stale_after_edit(original, feature, &ir.model.features, definition_written) {
            let edit = match feature.suppressed {
                Some(true) if original.suppressed != feature.suppressed => {
                    "written suppressed as edited"
                }
                Some(false) if original.suppressed != feature.suppressed => {
                    "written unsuppressed as edited"
                }
                _ => "written as edited",
            };
            named.insert(feature.id.clone());
            losses.push(stale(format!(
                "Feature {} is {edit}, and the stored solid shows the state before the edit.",
                feature_label(feature)
            )));
        }
    }
    for removed in baseline.iter().filter(|original| {
        !ir.model
            .features
            .iter()
            .any(|feature| feature.id == original.id)
    }) {
        explained = true;
        if feature_affects_brep(removed, baseline) {
            named.insert(removed.id.clone());
            losses.push(stale(format!(
                "Feature {} is removed from the written history, and the stored solid still includes it.",
                feature_label(removed)
            )));
        }
    }
    if explained {
        return Ok(());
    }
    let mut naming_only = ir.model.clone();
    for feature in &mut naming_only.features {
        if let Some(original) = baseline.iter().find(|original| original.id == feature.id) {
            feature.name.clone_from(&original.name);
        }
    }
    let residual_change = match ir
        .source
        .as_ref()
        .and_then(|meta| meta.attributes.get("sldprt_neutral_feature_local_sha256"))
    {
        Some(expected) => expected != &feature_hash(hash_ctx, &naming_only)?,
        None => false,
    };
    let shaping = ir
        .model
        .features
        .iter()
        .filter(|feature| feature_affects_brep(feature, &ir.model.features))
        .collect::<Vec<_>>();
    if !residual_change || shaping.is_empty() {
        return Ok(());
    }
    let moved = shaping
        .iter()
        .filter(|feature| {
            baseline
                .iter()
                .find(|original| original.id == feature.id)
                .is_some_and(|original| {
                    original.ordinal != feature.ordinal || original.native_ref != feature.native_ref
                })
        })
        .collect::<Vec<_>>();
    if moved.is_empty() {
        losses.push(stale(
            "The written feature tree is edited in order or placement, and the stored solid shows the state before the edit.",
        ));
    }
    for feature in moved
        .into_iter()
        .filter(|feature| !misread_field(&feature.id, "ordinal"))
    {
        losses.push(stale(format!(
            "Feature {} is written at its edited place in the feature tree, and the stored solid shows the state before the edit.",
            feature_label(feature)
        )));
    }
    Ok(())
}

fn parameter_edits(
    ir: &CadIr,
    source: &CadIr,
    unwritten: &UnwrittenEdits,
    losses: &mut Vec<LossNote>,
) {
    let baseline = &source.model.parameters;
    for parameter in &ir.model.parameters {
        let part = unwritten.parameters.get(&parameter.id).copied();
        if part == Some(UnwrittenPart::Whole) {
            continue;
        }
        let original = baseline.iter().find(|original| original.id == parameter.id);
        if original.is_some_and(|original| !driving_parameter_changed(original, parameter, part)) {
            continue;
        }
        let driven = driven_features(parameter, ir);
        if !driven.is_empty() {
            // The written part of the edit: the value where the expression
            // is not written.
            let written = match (part, parameter.value.as_ref()) {
                (Some(UnwrittenPart::Expression), Some(value)) => {
                    format!(
                        "value {}",
                        crate::history::literals::format_parameter_value(value)
                    )
                }
                _ => parameter.expression.clone(),
            };
            losses.push(stale(format!(
                "Parameter {} is written as edited ({written}), and the stored solid shows {} before the edit.",
                parameter_label(parameter),
                features_label(&driven)
            )));
        }
    }
    for removed in baseline.iter().filter(|original| {
        !unwritten.parameters.contains_key(&original.id)
            && !ir
                .model
                .parameters
                .iter()
                .any(|parameter| parameter.id == original.id)
    }) {
        let driven = driven_features(removed, ir);
        if !driven.is_empty() {
            losses.push(stale(format!(
                "Parameter {} is removed from the written history, and the stored solid shows {} before the edit.",
                parameter_label(removed),
                features_label(&driven)
            )));
        }
    }
}

fn native_edits(
    ir: &CadIr,
    native: &SldprtNative,
    source: &CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let source_native = source
        .native
        .namespace("sldprt")
        .map(|namespace| SldprtNative::load(namespace).map_err(CodecError::from))
        .transpose()?;
    let baseline = source_native
        .iter()
        .flat_map(|native| &native.feature_histories)
        .flat_map(|history| &history.features)
        .collect::<Vec<_>>();
    let records = native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
        .collect::<Vec<_>>();
    for feature in &records {
        let original = baseline.iter().find(|original| original.id == feature.id);
        let modeling = ir
            .model
            .features
            .iter()
            .find(|neutral| neutral.native_ref.as_deref() == Some(feature.id.as_str()))
            .is_none_or(|neutral| feature_affects_brep(neutral, &ir.model.features));
        if modeling
            && original.is_none_or(|original| native_construction_changed(original, feature))
        {
            losses.push(stale(format!(
                "Native feature {} is written as edited, and the stored solid shows the state before the edit.",
                native_label(feature)
            )));
        }
    }
    for removed in baseline
        .iter()
        .filter(|original| !records.iter().any(|feature| feature.id == original.id))
    {
        let neutral = ir
            .model
            .features
            .iter()
            .chain(&source.model.features)
            .find(|feature| feature.native_ref.as_deref() == Some(removed.id.as_str()));
        if neutral.is_none_or(|feature| feature_affects_brep(feature, &source.model.features)) {
            losses.push(stale(format!(
                "Native feature {} is removed from the written history, and the stored solid still includes it.",
                native_label(removed)
            )));
        }
    }
    Ok(())
}

fn configuration_design_edits(
    ir: &CadIr,
    baseline: &CadIr,
    named: &HashSet<FeatureId>,
    unwritten: &UnwrittenEdits,
    losses: &mut Vec<LossNote>,
) {
    // The writer writes no synthesized snapshot; the written part decodes with
    // the snapshot restated from the written model.
    let synthesized = ir
        .source
        .as_ref()
        .and_then(|source| {
            source
                .attributes
                .get("sldprt_configuration_snapshot_synthesized")
        })
        .map(|id| (id, crate::decode::model_snapshot(&ir.model)));
    for configuration in &ir.model.configurations {
        let original = baseline
            .model
            .configurations
            .iter()
            .find(|candidate| candidate.id == configuration.id);
        let configuration_label = configuration_label(configuration);
        let mirror = synthesized
            .as_ref()
            .filter(|(id, _)| configuration.id.as_str() == id.as_str())
            .map(|(_, snapshot)| snapshot);
        let parameter_ids = configuration
            .parameter_values
            .keys()
            .chain(configuration.parameter_overrides.keys())
            .chain(
                original
                    .into_iter()
                    .flat_map(|value| value.parameter_values.keys()),
            )
            .chain(
                original
                    .into_iter()
                    .flat_map(|value| value.parameter_overrides.keys()),
            )
            .collect::<BTreeSet<_>>();
        for id in parameter_ids {
            let mut value_edited = original.is_none_or(|value| {
                configuration.parameter_values.get(id) != value.parameter_values.get(id)
            });
            if let Some(mirror) = mirror.filter(|_| value_edited) {
                value_edited = false;
                if configuration.parameter_values.get(id) != mirror.parameter_values.get(id) {
                    losses.push(snapshot_unwritten(format!(
                        "The value of parameter {} in configuration {configuration_label} is edited only in the configuration snapshot the decoder synthesized from the model. The writer does not write that snapshot, and the written configuration states the value of the model parameter.",
                        parameter_id_label(
                            id,
                            ir.model.parameters.iter().chain(&baseline.model.parameters)
                        )
                    )));
                }
            }
            let override_edited = original.is_none_or(|value| {
                configuration.parameter_overrides.get(id) != value.parameter_overrides.get(id)
            });
            if unwritten
                .configuration_values
                .contains(&(configuration.id.as_str().to_owned(), id.clone()))
            {
                value_edited = false;
            }
            if !(value_edited || override_edited) {
                continue;
            }
            let parameter = ir
                .model
                .parameters
                .iter()
                .chain(&baseline.model.parameters)
                .find(|parameter| &parameter.id == id);
            match parameter {
                None => losses.push(stale(format!(
                    "Parameter {} in configuration {configuration_label} is written as edited, and that configuration's stored solid shows the state before the edit.",
                    id.as_str()
                ))),
                Some(parameter) => {
                    let driven = driven_features(parameter, ir);
                    if !driven.is_empty() {
                        losses.push(stale(format!(
                            "Parameter {} in configuration {configuration_label} is written as edited, and that configuration's stored solid shows {} before the edit.",
                            parameter_label(parameter),
                            features_label(&driven)
                        )));
                    }
                }
            }
        }
        let feature_ids = configuration
            .feature_states
            .keys()
            .chain(
                original
                    .into_iter()
                    .flat_map(|value| value.feature_states.keys()),
            )
            .collect::<BTreeSet<_>>();
        for id in feature_ids {
            let edited = original.is_none_or(|value| {
                configuration.feature_states.get(id) != value.feature_states.get(id)
            });
            if !edited {
                continue;
            }
            let current = ir.model.features.iter().find(|feature| &feature.id == id);
            let baseline_feature = baseline
                .model
                .features
                .iter()
                .find(|feature| &feature.id == id);
            let label = current
                .or(baseline_feature)
                .map_or_else(|| id.as_str().to_owned(), feature_label);
            if let Some(mirror) = mirror {
                if configuration.feature_states.get(id) != mirror.feature_states.get(id) {
                    losses.push(snapshot_unwritten(format!(
                        "The state of feature {label} in configuration {configuration_label} is edited only in the configuration snapshot the decoder synthesized from the model. The writer does not write that snapshot, and the written configuration states the feature as the model states it."
                    )));
                }
                continue;
            }
            if configuration.active && named.contains(id) {
                continue;
            }
            let modeling = current
                .map(|feature| feature_affects_brep(feature, &ir.model.features))
                .or_else(|| {
                    baseline_feature
                        .map(|feature| feature_affects_brep(feature, &baseline.model.features))
                })
                .unwrap_or(true);
            if modeling {
                losses.push(stale(format!(
                    "Feature {label} in configuration {configuration_label} is written with its edited state, and that configuration's stored solid shows the state before the edit."
                )));
            }
        }
    }
}

/// One stale-solid loss.
fn stale(message: impl Into<String>) -> LossNote {
    SldprtLossCode::WriterSolidStaleAfterEdit.note(message)
}

/// One loss for a synthesized-snapshot edit that the model does not state.
fn snapshot_unwritten(message: impl Into<String>) -> LossNote {
    SldprtLossCode::WriterConfigurationSnapshotEditUnwritten.note(message)
}

fn parameter_id_label<'a>(
    id: &ParameterId,
    parameters: impl IntoIterator<Item = &'a DesignParameter>,
) -> String {
    parameters
        .into_iter()
        .find(|parameter| &parameter.id == id)
        .map_or_else(|| id.as_str().to_owned(), parameter_label)
}

fn configuration_label(configuration: &DesignConfiguration) -> String {
    match configuration.name.as_deref() {
        Some(name) => format!("\"{name}\" ({})", configuration.id.as_str()),
        None => configuration.id.as_str().to_owned(),
    }
}

fn feature_label(feature: &Feature) -> String {
    match feature.name.as_deref() {
        Some(name) if !name.trim().is_empty() => format!("\"{name}\" ({})", feature.id.as_str()),
        _ => feature.id.as_str().to_owned(),
    }
}

fn features_label(features: &[&Feature]) -> String {
    let labels = features
        .iter()
        .map(|feature| feature_label(feature))
        .collect::<Vec<_>>();
    match labels.as_slice() {
        [one] => format!("feature {one}"),
        _ => format!("features {}", labels.join(", ")),
    }
}

fn parameter_label(parameter: &DesignParameter) -> String {
    format!("\"{}\" ({})", parameter.name, parameter.id.as_str())
}

fn native_label(feature: &crate::records::Feature) -> String {
    if feature.name.trim().is_empty() {
        feature.id.clone()
    } else {
        format!("\"{}\" ({})", feature.name, feature.id)
    }
}

fn native_construction_changed(
    original: &crate::records::Feature,
    current: &crate::records::Feature,
) -> bool {
    original.xml_tag != current.xml_tag
        || original.tree_parent != current.tree_parent
        || original.source_id != current.source_id
        || original.ordinal != current.ordinal
        || original.kind != current.kind
        || original.input_class != current.input_class
        || original.suppressed != current.suppressed
        || original.parameters != current.parameters
        || original.dimension_properties != current.dimension_properties
        || original.properties != current.properties
        || original.text != current.text
        || original.content != current.content
}

fn feature_affects_brep(feature: &Feature, features: &[Feature]) -> bool {
    let mut visited = HashSet::new();
    let mut pending = vec![feature.id.clone()];
    while let Some(id) = pending.pop() {
        if !visited.insert(id.clone()) {
            continue;
        }
        let Some(current) = features.iter().find(|candidate| candidate.id == id) else {
            return true;
        };
        if feature_directly_affects_brep(current) {
            return true;
        }
        pending.extend(
            features
                .iter()
                .filter(|candidate| candidate.dependencies.contains(&id))
                .map(|candidate| candidate.id.clone()),
        );
    }
    false
}

fn feature_directly_affects_brep(feature: &Feature) -> bool {
    if !feature.evaluation.outputs().is_empty() {
        return true;
    }
    let operation = match feature.evaluation.definition() {
        FeatureDefinition::Operation(operation)
        | FeatureDefinition::PostProcess { operation, .. } => operation,
    };
    !matches!(
        operation,
        FeatureOperation::TreeNode { .. }
            | FeatureOperation::MeshGenerateFaceGroups { .. }
            | FeatureOperation::MeshCombineFaceGroups { .. }
            | FeatureOperation::CosmeticThread { .. }
            | FeatureOperation::ReferenceImage { .. }
            | FeatureOperation::Decal { .. }
            | FeatureOperation::DatumPrincipalPlane { .. }
            | FeatureOperation::DatumPlane { .. }
            | FeatureOperation::DatumThreePointPlane { .. }
            | FeatureOperation::DatumOffsetPlane { .. }
            | FeatureOperation::DatumAxis { .. }
            | FeatureOperation::DatumPoint { .. }
            | FeatureOperation::DatumCoordinateSystem { .. }
    )
}

/// Whether the stale-solid loss of a write names the construction edit of
/// `current`, a feature of `features`, against its source feature
/// `original`: the edit reaches the written record (`definition_written`
/// states whether its definition does) and the feature shapes the solid.
pub(crate) fn stale_after_edit(
    original: &Feature,
    current: &Feature,
    features: &[Feature],
    definition_written: bool,
) -> bool {
    construction_changed(original, current, definition_written)
        && feature_affects_brep(current, features)
}

/// Whether the written construction of `current` differs from `original`.
/// The definition, the dependencies and the source properties count only
/// when the definition is written: a stored definition record states its
/// stored operands and properties.
fn construction_changed(original: &Feature, current: &Feature, definition_written: bool) -> bool {
    (definition_written
        && (original.evaluation.definition() != current.evaluation.definition()
            || original.dependencies != current.dependencies
            || original.source_properties != current.source_properties))
        || original.suppressed != current.suppressed
        || original.source_tag != current.source_tag
        || original.source_text != current.source_text
        || original.source_content != current.source_content
}

/// Whether the written part of a parameter edit changes what drives the
/// solid; `unwritten` is the part of the edit the write does not carry.
fn driving_parameter_changed(
    original: &DesignParameter,
    current: &DesignParameter,
    unwritten: Option<UnwrittenPart>,
) -> bool {
    (unwritten != Some(UnwrittenPart::Expression) && original.expression != current.expression)
        || (unwritten != Some(UnwrittenPart::Value) && original.value != current.value)
        || original.dependencies != current.dependencies
        || original.properties != current.properties
        || original.owner != current.owner
}

/// The features that shape the solid and that `parameter` drives, through its
/// owner or through any parameter whose expression depends on it.
fn driven_features<'a>(parameter: &DesignParameter, ir: &'a cadmpeg_ir::CadIr) -> Vec<&'a Feature> {
    let mut driven = Vec::new();
    push_shaping_owner(&mut driven, parameter.owner.as_ref(), ir);
    let mut visited = HashSet::new();
    let mut pending = vec![parameter.id.clone()];
    while let Some(id) = pending.pop() {
        if !visited.insert(id.clone()) {
            continue;
        }
        for candidate in &ir.model.parameters {
            if candidate.id == id
                || candidate
                    .dependencies
                    .iter()
                    .any(|dependency| dependency == &id)
            {
                push_shaping_owner(&mut driven, candidate.owner.as_ref(), ir);
                pending.push(candidate.id.clone());
            }
        }
    }
    driven
}

fn push_shaping_owner<'a>(
    driven: &mut Vec<&'a Feature>,
    owner: Option<&FeatureId>,
    ir: &'a cadmpeg_ir::CadIr,
) {
    let Some(feature) = owner.and_then(|owner| {
        ir.model
            .features
            .iter()
            .find(|feature| &feature.id == owner)
    }) else {
        return;
    };
    if feature_affects_brep(feature, &ir.model.features)
        && !driven.iter().any(|known| known.id == feature.id)
    {
        driven.push(feature);
    }
}
