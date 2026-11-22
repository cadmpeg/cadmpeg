// SPDX-License-Identifier: Apache-2.0
//! Agreement between edited construction history and the output B-rep.
//!
//! The writer cannot regenerate a solid. An edit that shapes it is written as
//! edited over the stored solid, and `writer.solid-stale-after-edit` names it.

use std::collections::{BTreeSet, HashSet};
use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::parameters::DesignParameter;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
use cadmpeg_ir::report::loss::LossNote;

use crate::history::hash::{
    configuration_feature_state_hash, configuration_hash, configuration_parameter_value_hash,
    feature_hash, history_hash, parameter_hash,
};
use crate::history::parameters::project_parameters;
use crate::loss::SldprtLossCode;

use super::project_features_with_native_inputs;

/// The stale-solid losses of one write: one per construction edit that shapes
/// the solid. The writer has no modeling kernel, so the output keeps the stored
/// solid and its caches as they were before the edit, and each loss names what
/// was edited. The source history is the baseline for native edits.
pub(crate) fn stale_solid_losses(
    ir: &cadmpeg_ir::CadIr,
    native: Option<&crate::native::SldprtNative>,
    source_scan: Option<&crate::container::ContainerScan<'_>>,
) -> Result<Vec<LossNote>, CodecError> {
    let hash_arena = cadmpeg_core::decode::DecodeArena::new();
    let (hash_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        source_scan.map_or(&[][..], |scan| scan.source_image),
        &hash_arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )?;

    let mut losses = Vec::new();
    // Features a model-level loss names; the active configuration mirrors the
    // model, so its state edit of the same feature is the same edit.
    let mut named = HashSet::new();
    let Some(source) = ir.source.as_ref() else {
        return Ok(losses);
    };
    let Some(native) = native else {
        return Ok(losses);
    };
    let neutral_feature_baseline = source.attributes.get("sldprt_neutral_feature_local_sha256");
    let neutral_features_changed = match neutral_feature_baseline {
        Some(baseline) => baseline != &feature_hash(&hash_ctx, &ir.model)?,
        None => false,
    };
    let neutral_parameters_changed = match source
        .attributes
        .get("sldprt_neutral_parameter_local_sha256")
    {
        Some(baseline) => baseline != &parameter_hash(&hash_ctx, &ir.model.parameters)?,
        None => false,
    };
    if neutral_features_changed {
        let baseline = project_features_with_native_inputs(native)?;
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
                losses.push(stale(format!(
                    "Feature {} is written with its edited output bodies, and the stored solid shows the state before the edit.",
                    feature_label(feature)
                )));
                continue;
            }
            if !construction_changed(original, feature) {
                continue;
            }
            explained = true;
            if feature_affects_brep(feature, &ir.model.features) {
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
            if feature_affects_brep(removed, &baseline) {
                named.insert(removed.id.clone());
                losses.push(stale(format!(
                    "Feature {} is removed from the written history, and the stored solid still includes it.",
                    feature_label(removed)
                )));
            }
        }
        if !explained {
            let mut naming_only = ir.model.clone();
            for feature in &mut naming_only.features {
                if let Some(original) = baseline.iter().find(|original| original.id == feature.id) {
                    feature.name.clone_from(&original.name);
                }
            }
            let residual_change = match neutral_feature_baseline {
                Some(expected) => expected != &feature_hash(&hash_ctx, &naming_only)?,
                None => false,
            };
            let shaping = ir
                .model
                .features
                .iter()
                .filter(|feature| feature_affects_brep(feature, &ir.model.features))
                .collect::<Vec<_>>();
            if residual_change && !shaping.is_empty() {
                let moved = shaping
                    .iter()
                    .filter(|feature| {
                        baseline
                            .iter()
                            .find(|original| original.id == feature.id)
                            .is_some_and(|original| {
                                original.ordinal != feature.ordinal
                                    || original.native_ref != feature.native_ref
                            })
                    })
                    .collect::<Vec<_>>();
                if moved.is_empty() {
                    losses.push(stale(
                        "The written feature tree is edited in order or placement, and the stored solid shows the state before the edit.",
                    ));
                }
                for feature in moved {
                    losses.push(stale(format!(
                        "Feature {} is written at its edited place in the feature tree, and the stored solid shows the state before the edit.",
                        feature_label(feature)
                    )));
                }
            }
        }
    }
    if neutral_parameters_changed {
        let lane_bytes = native
            .feature_input_lanes
            .iter()
            .flat_map(|lane| lane.native_payload.iter().copied())
            .collect::<Vec<_>>();
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&lane_bytes, &arena, &DecodePolicy::service())?;
        let baseline = project_parameters(&ctx, &native.feature_histories)?;
        for parameter in &ir.model.parameters {
            let original = baseline.iter().find(|original| original.id == parameter.id);
            if original.is_some_and(|original| !driving_parameter_changed(original, parameter)) {
                continue;
            }
            let driven = driven_features(parameter, ir);
            if !driven.is_empty() {
                losses.push(stale(format!(
                    "Parameter {} is written as edited ({}), and the stored solid shows {} before the edit.",
                    parameter_label(parameter),
                    parameter.expression,
                    features_label(&driven)
                )));
            }
        }
        for removed in baseline.iter().filter(|original| {
            !ir.model
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
    let native_history_changed = match source.attributes.get("sldprt_native_history_sha256") {
        Some(baseline) => baseline != &history_hash(&hash_ctx, &native.feature_histories)?,
        None => false,
    };
    if let Some(scan) = source_scan.filter(|_| native_history_changed) {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(scan.source_image, &arena, &DecodePolicy::service())?;
        let mut annotations = cadmpeg_ir::Annotations::default();
        let mut decode_losses = Vec::new();
        let baseline = crate::history::histories(&ctx, scan, &mut annotations, &mut decode_losses)?;
        for feature in native
            .feature_histories
            .iter()
            .flat_map(|history| &history.features)
        {
            let original = baseline
                .iter()
                .flat_map(|history| &history.features)
                .find(|original| original.id == feature.id);
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
        let removed = baseline
            .iter()
            .flat_map(|history| &history.features)
            .filter(|original| {
                !native
                    .feature_histories
                    .iter()
                    .flat_map(|history| &history.features)
                    .any(|feature| feature.id == original.id)
            })
            .collect::<Vec<_>>();
        if !removed.is_empty() {
            let baseline_features = crate::history::project::project_features(&baseline)?;
            for removed in removed {
                let neutral = ir
                    .model
                    .features
                    .iter()
                    .find(|feature| feature.native_ref.as_deref() == Some(removed.id.as_str()))
                    .or_else(|| {
                        baseline_features.iter().find(|feature| {
                            feature.native_ref.as_deref() == Some(removed.id.as_str())
                        })
                    });
                if neutral.is_none_or(|feature| feature_affects_brep(feature, &baseline_features)) {
                    losses.push(stale(format!(
                        "Native feature {} is removed from the written history, and the stored solid still includes it.",
                        native_label(removed)
                    )));
                }
            }
        }
    } else if native_history_changed
        && native
            .feature_histories
            .iter()
            .flat_map(|history| &history.features)
            .any(|feature| {
                ir.model.features.iter().any(|neutral| {
                    neutral.native_ref.as_deref() == Some(feature.id.as_str())
                        && feature_affects_brep(neutral, &ir.model.features)
                })
            })
    {
        losses.push(stale(
            "The native feature history is edited, and without the retained source image the writer cannot tell which record changed; the stored solid shows the state before the edit.",
        ));
    }
    if let Some(expected) = source
        .attributes
        .get("sldprt_neutral_configuration_local_sha256")
    {
        if expected != &configuration_hash(&hash_ctx, &ir.model.configurations)? {
            if let Some(scan) = source_scan {
                configuration_design_edits(ir, scan, &named, &mut losses)?;
            } else {
                let configurations = crate::writer::configurations_without_synthesized_snapshot(ir);
                if expected == &configuration_hash(&hash_ctx, &configurations)? {
                    return Ok(losses);
                }
                let parameter_values_changed = match source
                    .attributes
                    .get("sldprt_configuration_parameter_values_local_sha256")
                {
                    Some(baseline) => {
                        baseline != &configuration_parameter_value_hash(&hash_ctx, &configurations)?
                    }
                    None => false,
                };
                let feature_states_changed = match source
                    .attributes
                    .get("sldprt_configuration_feature_states_local_sha256")
                {
                    Some(baseline) => {
                        baseline != &configuration_feature_state_hash(&hash_ctx, &configurations)?
                    }
                    None => false,
                };
                if parameter_values_changed
                    || feature_states_changed
                    || configurations
                        .iter()
                        .any(|configuration| !configuration.parameter_overrides.is_empty())
                {
                    losses.push(stale(
                        "A configuration design state is edited, and without the retained source image the writer cannot tell which; the stored configuration solids show the state before the edit.",
                    ));
                }
            }
        }
    }
    Ok(losses)
}

fn configuration_design_edits(
    ir: &cadmpeg_ir::CadIr,
    source_scan: &crate::container::ContainerScan<'_>,
    named: &HashSet<FeatureId>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let source = crate::SldprtCodec
        .decode(
            &mut Cursor::new(source_scan.source_image),
            &DecodeOptions::default(),
        )
        .map_err(|failure| match failure {
            cadmpeg_ir::codec::DecodeFailure::Codec(error) => error,
            other => CodecError::malformed(format_args!(
                "SLDPRT retained configuration source cannot be decoded: {other}"
            )),
        })?;
    let baseline = source.ir();
    for configuration in &ir.model.configurations {
        let original = baseline
            .model
            .configurations
            .iter()
            .find(|candidate| candidate.id == configuration.id);
        let configuration_label = match configuration.name.as_deref() {
            Some(name) => format!("\"{name}\" ({})", configuration.id.as_str()),
            None => configuration.id.as_str().to_owned(),
        };
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
            let changed = original.is_none_or(|value| {
                configuration.parameter_values.get(id) != value.parameter_values.get(id)
                    || configuration.parameter_overrides.get(id)
                        != value.parameter_overrides.get(id)
            });
            if !changed {
                continue;
            }
            let parameter = ir
                .model
                .parameters
                .iter()
                .find(|parameter| &parameter.id == id)
                .or_else(|| {
                    baseline
                        .model
                        .parameters
                        .iter()
                        .find(|parameter| &parameter.id == id)
                });
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
            let changed = original.is_none_or(|value| {
                configuration.feature_states.get(id) != value.feature_states.get(id)
            });
            if !changed || (configuration.active && named.contains(id)) {
                continue;
            }
            let current = ir.model.features.iter().find(|feature| &feature.id == id);
            let baseline_feature = baseline
                .model
                .features
                .iter()
                .find(|feature| &feature.id == id);
            let modeling = current
                .map(|feature| feature_affects_brep(feature, &ir.model.features))
                .or_else(|| {
                    baseline_feature
                        .map(|feature| feature_affects_brep(feature, &baseline.model.features))
                })
                .unwrap_or(true);
            if modeling {
                let label = current
                    .or(baseline_feature)
                    .map_or_else(|| id.as_str().to_owned(), feature_label);
                losses.push(stale(format!(
                    "Feature {label} in configuration {configuration_label} is written with its edited state, and that configuration's stored solid shows the state before the edit."
                )));
            }
        }
    }
    Ok(())
}

/// One stale-solid loss.
fn stale(message: impl Into<String>) -> LossNote {
    SldprtLossCode::WriterSolidStaleAfterEdit.note(message)
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

fn construction_changed(original: &Feature, current: &Feature) -> bool {
    original.evaluation.definition() != current.evaluation.definition()
        || original.suppressed != current.suppressed
        || original.dependencies != current.dependencies
        || original.source_properties != current.source_properties
        || original.source_tag != current.source_tag
        || original.source_text != current.source_text
        || original.source_content != current.source_content
}

fn driving_parameter_changed(original: &DesignParameter, current: &DesignParameter) -> bool {
    original.expression != current.expression
        || original.value != current.value
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
