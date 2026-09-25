// SPDX-License-Identifier: Apache-2.0
//! Agreement between edited construction history and the output B-rep.

use std::collections::HashSet;
use std::io::Cursor;

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::{DesignParameter, Feature, FeatureDefinition, FeatureOperation};

use crate::history::hash::{configuration_hash, feature_hash, history_hash, parameter_hash};
use crate::history::parameters::project_parameters;

use super::project_features_with_native_inputs;

/// Refuse edited construction controls that this writer cannot evaluate into
/// the output B-rep. The source history is the baseline for native edits.
pub(crate) fn validate(
    ir: &cadmpeg_ir::CadIr,
    native: Option<&crate::native::SldprtNative>,
    source_scan: Option<&crate::container::ContainerScan<'_>>,
) -> Result<(), CodecError> {
    let Some(source) = ir.source.as_ref() else {
        return Ok(());
    };
    let Some(native) = native else {
        return Ok(());
    };
    let neutral_feature_baseline = source.attributes.get("sldprt_neutral_feature_local_sha256");
    let neutral_features_changed = match neutral_feature_baseline {
        Some(baseline) => baseline != &feature_hash(&ir.model)?,
        None => false,
    };
    let neutral_parameters_changed = match source
        .attributes
        .get("sldprt_neutral_parameter_local_sha256")
    {
        Some(baseline) => baseline != &parameter_hash(&ir.model.parameters)?,
        None => false,
    };
    if neutral_features_changed {
        let baseline = project_features_with_native_inputs(native)?;
        let mut nongeometric_construction_changed = false;
        for feature in &ir.model.features {
            let original = baseline.iter().find(|original| original.id == feature.id);
            if original.is_some_and(|original| {
                original.evaluation.outputs() != feature.evaluation.outputs()
            }) {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT writer cannot regenerate B-rep after feature output edit {}",
                    feature.id.as_str()
                )));
            }
            if original.is_none_or(|original| construction_changed(original, feature)) {
                if feature_affects_brep(feature, &ir.model.features) {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT writer cannot regenerate B-rep after feature edit {}",
                        feature.id.as_str()
                    )));
                }
                nongeometric_construction_changed = true;
            }
        }
        if let Some(removed) = baseline.iter().find(|original| {
            feature_affects_brep(original, &baseline)
                && !ir
                    .model
                    .features
                    .iter()
                    .any(|feature| feature.id == original.id)
        }) {
            return Err(CodecError::NotImplemented(format!(
                "SLDPRT writer cannot regenerate B-rep after feature removal {}",
                removed.id.as_str()
            )));
        }
        if !nongeometric_construction_changed {
            let mut naming_only = ir.model.clone();
            for feature in &mut naming_only.features {
                if let Some(original) = baseline.iter().find(|original| original.id == feature.id) {
                    feature.name.clone_from(&original.name);
                }
            }
            let residual_change = match neutral_feature_baseline {
                Some(expected) => expected != &feature_hash(&naming_only)?,
                None => false,
            };
            if residual_change {
                if let Some(feature) = ir
                    .model
                    .features
                    .iter()
                    .find(|feature| feature_affects_brep(feature, &ir.model.features))
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT writer cannot regenerate B-rep after feature result edit {}",
                        feature.id.as_str()
                    )));
                }
            }
        }
    }
    if neutral_parameters_changed {
        let baseline = project_parameters(&native.feature_histories);
        for parameter in &ir.model.parameters {
            let original = baseline.iter().find(|original| original.id == parameter.id);
            if original.is_none_or(|original| driving_parameter_changed(original, parameter))
                && parameter_affects_brep(parameter, ir)
            {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT writer cannot regenerate B-rep after parameter edit {}",
                    parameter.id.as_str()
                )));
            }
        }
        if let Some(removed) = baseline.iter().find(|original| {
            parameter_affects_brep(original, ir)
                && !ir
                    .model
                    .parameters
                    .iter()
                    .any(|parameter| parameter.id == original.id)
        }) {
            return Err(CodecError::NotImplemented(format!(
                "SLDPRT writer cannot regenerate B-rep after parameter removal {}",
                removed.id.as_str()
            )));
        }
    }
    let native_history_changed = match source.attributes.get("sldprt_native_history_sha256") {
        Some(baseline) => baseline != &history_hash(&native.feature_histories)?,
        None => false,
    };
    if let Some(scan) = source_scan.filter(|_| native_history_changed) {
        let mut annotations = cadmpeg_ir::Annotations::default();
        let mut losses = Vec::new();
        let baseline = crate::history::histories(scan, &mut annotations, &mut losses);
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
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT writer cannot regenerate B-rep after native feature edit {}",
                    feature.id
                )));
            }
        }
        if let Some(removed) =
            baseline
                .iter()
                .flat_map(|history| &history.features)
                .find(|original| {
                    !native
                        .feature_histories
                        .iter()
                        .flat_map(|history| &history.features)
                        .any(|feature| feature.id == original.id)
                })
        {
            let baseline_features = crate::history::project::project_features(&baseline)?;
            let neutral = ir
                .model
                .features
                .iter()
                .find(|feature| feature.native_ref.as_deref() == Some(removed.id.as_str()))
                .or_else(|| {
                    baseline_features
                        .iter()
                        .find(|feature| feature.native_ref.as_deref() == Some(removed.id.as_str()))
                });
            if neutral.is_none_or(|feature| feature_affects_brep(feature, &baseline_features)) {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT writer cannot regenerate B-rep after native feature removal {}",
                    removed.id
                )));
            }
        }
    }
    if let (Some(scan), Some(expected)) = (
        source_scan,
        source
            .attributes
            .get("sldprt_neutral_configuration_local_sha256"),
    ) {
        if expected != &configuration_hash(&ir.model.configurations)? {
            validate_configuration_design_edits(ir, scan)?;
        }
    }
    Ok(())
}

fn validate_configuration_design_edits(
    ir: &cadmpeg_ir::CadIr,
    source_scan: &crate::container::ContainerScan<'_>,
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
            .collect::<HashSet<_>>();
        for id in parameter_ids {
            let changed = original.is_none_or(|value| {
                configuration.parameter_values.get(id) != value.parameter_values.get(id)
                    || configuration.parameter_overrides.get(id)
                        != value.parameter_overrides.get(id)
            });
            if changed {
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
                if parameter.is_none_or(|parameter| parameter_affects_brep(parameter, ir)) {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT writer cannot regenerate B-rep after configuration parameter edit {} in {}",
                        id.as_str(),
                        configuration.id.as_str()
                    )));
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
            .collect::<HashSet<_>>();
        for id in feature_ids {
            let changed = original.is_none_or(|value| {
                configuration.feature_states.get(id) != value.feature_states.get(id)
            });
            if changed {
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
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT writer cannot regenerate B-rep after configuration feature edit {} in {}",
                        id.as_str(),
                        configuration.id.as_str()
                    )));
                }
            }
        }
    }
    Ok(())
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

fn parameter_affects_brep(parameter: &DesignParameter, ir: &cadmpeg_ir::CadIr) -> bool {
    if parameter.owner.as_ref().is_some_and(|owner| {
        ir.model
            .features
            .iter()
            .find(|feature| &feature.id == owner)
            .is_some_and(|feature| feature_affects_brep(feature, &ir.model.features))
    }) {
        return true;
    }
    let mut visited = HashSet::new();
    let mut pending = vec![parameter.id.clone()];
    while let Some(id) = pending.pop() {
        if !visited.insert(id.clone()) {
            continue;
        }
        for parameter in &ir.model.parameters {
            if parameter.id == id
                || parameter
                    .dependencies
                    .iter()
                    .any(|dependency| dependency == &id)
            {
                if parameter.owner.as_ref().is_some_and(|owner| {
                    ir.model
                        .features
                        .iter()
                        .find(|feature| &feature.id == owner)
                        .is_some_and(|feature| feature_affects_brep(feature, &ir.model.features))
                }) {
                    return true;
                }
                pending.push(parameter.id.clone());
            }
        }
    }
    false
}
