// SPDX-License-Identifier: Apache-2.0
//! Generate native history and input lanes from neutral design records.

use std::collections::{BTreeMap, HashSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    DistinctMembers, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation, FeatureId,
    FeatureOperation, FeatureTreeNodeRole, SketchFeatureBinding, TreeChildren,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::sketches::{SketchGeometryDefinition, SpatialSketchGeometryDefinition};
use cadmpeg_ir::{Annotations, CadIr};

use crate::loss::SldprtLossCode;
use crate::native::SldprtNative;

pub(super) fn prepare(
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
    compressed_sketch_configurations: &std::collections::BTreeSet<String>,
) -> Result<Option<SldprtNative>, CodecError> {
    if !ir.scope.design {
        return Ok(ir
            .native
            .namespace("sldprt")
            .map(SldprtNative::load)
            .transpose()?);
    }
    ensure_owners(ir)?;
    super::reduce_design(ir, losses)?;
    let mut native = ir
        .native
        .namespace("sldprt")
        .map(SldprtNative::load)
        .transpose()?;
    crate::history::write::features::synthesize_neutral_features(
        &ir.model,
        &ir.model.parameters,
        &ir.model.bodies,
        &mut native,
    )?;
    sketches(ir, native.as_ref(), losses)?;
    // A dropped sketch retains its feature and solved output, with an explicit loss.
    crate::history::write::features::synthesize_neutral_features(
        &ir.model,
        &ir.model.parameters,
        &ir.model.bodies,
        &mut native,
    )?;
    crate::resolved_features::write_prepare::prepare_sketches_for_write(
        ir,
        &mut native,
        compressed_sketch_configurations,
    )?;
    crate::history::write::parameters::synchronize_parameter_records(ir, &mut native)?;
    let all_bodies = ir
        .model
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect::<Vec<_>>();
    let configured = ir
        .model
        .configurations
        .iter()
        .flat_map(|configuration| configuration.bodies.as_deref().unwrap_or_default())
        .cloned()
        .collect::<HashSet<_>>();
    let unconfigured = all_bodies
        .into_iter()
        .filter(|body| !configured.contains(body))
        .collect::<Vec<_>>();
    let has_active = ir
        .model
        .configurations
        .iter()
        .any(|configuration| configuration.active);
    for (index, configuration) in ir.model.configurations.iter_mut().enumerate() {
        if configuration
            .name
            .as_ref()
            .is_none_or(|name| name.trim().is_empty())
        {
            configuration.name = Some(configuration.id.as_str().into());
            losses.push(SldprtLossCode::WriterFieldsOmitted.note(format!(
                "configuration {} uses its identity as the required native name",
                configuration.id
            )));
        }
        if !has_active && index == 0 {
            configuration.active = true;
            losses.push(SldprtLossCode::WriterFieldsOmitted.note(format!(
                "configuration {} was selected as the required native active configuration",
                configuration.id
            )));
        }
        if index == 0 && !unconfigured.is_empty() {
            let mut bodies = configuration
                .bodies
                .take()
                .map_or_else(Vec::new, |bodies| bodies.iter().cloned().collect());
            bodies.extend(unconfigured.iter().cloned());
            configuration.bodies = Some(bodies.try_into().map_err(CodecError::malformed)?);
            losses.push(SldprtLossCode::WriterFieldsOmitted.note(format!(
                "configuration {} carries {} body(s) without configuration membership",
                configuration.id,
                unconfigured.len()
            )));
        }
    }
    super::super::assign_configuration_indices(&mut ir.model.configurations)?;
    for configuration in &mut ir.model.configurations {
        for feature in configuration.feature_states.keys() {
            losses.push(SldprtLossCode::WriterDesignReduced.note(format!(
                "configuration {} feature {feature}: structural override has no native lane serializer", configuration.id
            )));
        }
        configuration.feature_states.clear();
        for parameter in configuration.parameter_overrides.keys() {
            losses.push(SldprtLossCode::WriterDesignReduced.note(format!(
                "configuration {} parameter {parameter}: expression override omitted; its evaluated scalar is retained when representable", configuration.id
            )));
        }
        configuration.parameter_overrides.clear();
        configuration.parameter_values.retain(|parameter, value| {
            if matches!(value, cadmpeg_ir::features::ParameterValue::Length(_) | cadmpeg_ir::features::ParameterValue::Angle(_)) { return true; }
            losses.push(SldprtLossCode::WriterDesignReduced.note(format!(
                "configuration {} parameter {parameter}: this value has no native configuration scalar encoding", configuration.id
            )));
            false
        });
    }
    crate::history::write::configurations::synchronize_configuration_records(
        ir,
        &mut native,
        &Annotations::default(),
    )?;
    if let Some(native) = native.as_mut() {
        materialize_configuration_associations(ir, native, losses)?;
        crate::resolved_features::write_feature_parameters::synthesize(ir, native)?;
    }
    Ok(native)
}

/// Native configuration records are authored even for partition-only rows.
/// Reflect that actual new association in the prepared document so the normal
/// changed-configuration accounting reports the representation change.
fn materialize_configuration_associations(
    ir: &mut CadIr,
    native: &SldprtNative,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    for configuration in &mut ir.model.configurations {
        if configuration.native_ref.is_some() {
            continue;
        }
        let slot = configuration
            .properties
            .get("id")
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(configuration.ordinal);
        let mut matching = native
            .feature_histories
            .iter()
            .flat_map(|history| &history.configurations)
            .filter(|record| {
                let record_slot = record
                    .properties
                    .get("id")
                    .and_then(|value| value.parse::<u32>().ok())
                    .unwrap_or(record.ordinal);
                record_slot == slot
                    && record.source_index == configuration.source_index
                    && Some(record.name.as_str()) == configuration.name.as_deref()
            });
        let record = matching.next().ok_or_else(|| {
            CodecError::UnsupportedContent(format!(
                "configuration {} has no authored native counterpart",
                configuration.id
            ))
        })?;
        if matching.next().is_some() {
            return Err(CodecError::UnsupportedContent(format!(
                "configuration {} has multiple authored native counterparts",
                configuration.id
            )));
        }
        configuration.native_ref = Some(record.id.clone());
        losses.push(
            SldprtLossCode::WriterConfigurationAssociationGenerated.note(format!(
                "configuration {} acquired authored native association {} at slot {slot}",
                configuration.id, record.id
            )),
        );
    }
    Ok(())
}

fn ensure_owners(ir: &mut CadIr) -> Result<(), CodecError> {
    if ir
        .model
        .parameters
        .iter()
        .any(|parameter| parameter.owner.is_none())
    {
        let id = append_feature(
            ir,
            "Global parameters",
            FeatureOperation::TreeNode {
                role: FeatureTreeNodeRole::Equations,
                children: TreeChildren::default(),
            },
        )?;
        for parameter in &mut ir.model.parameters {
            if parameter.owner.is_none() {
                parameter.owner = Some(id.clone());
            }
        }
    }
    for sketch in ir.model.sketches.clone() {
        if !ir.model.features.iter().any(|feature| matches!(feature.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Sketch { sketch: SketchFeatureBinding::Planar(Some(candidate)) }) if candidate == &sketch.id)) {
            append_feature(ir, sketch.name.as_deref().unwrap_or("Sketch"), FeatureOperation::Sketch { sketch: SketchFeatureBinding::Planar(Some(sketch.id)) })?;
        }
    }
    for sketch in ir.model.spatial_sketches.clone() {
        if !ir.model.features.iter().any(|feature| matches!(feature.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::SpatialSketch { sketch: Some(candidate) }) if candidate == &sketch.id)) {
            append_feature(ir, sketch.name.as_deref().unwrap_or("Spatial sketch"), FeatureOperation::SpatialSketch { sketch: Some(sketch.id) })?;
        }
    }
    Ok(())
}

fn append_feature(
    ir: &mut CadIr,
    name: &str,
    operation: FeatureOperation,
) -> Result<FeatureId, CodecError> {
    let mut ordinal = 0;
    for feature in &ir.model.features {
        // (O-capacity): a construction ordinal cannot exceed the u64 document field.
        ordinal = ordinal.max(
            feature
                .ordinal
                .checked_add(1)
                .ok_or_else(|| CodecError::NotImplemented("feature ordinal exceeds u64".into()))?,
        );
    }
    let mut identity_ordinal = ordinal;
    let id = loop {
        let candidate = FeatureId::mint(format!("generated:sldprt:feature#{identity_ordinal}"))
            .map_err(|error| {
                CodecError::malformed(format_args!("generated feature identity: {error}"))
            })?;
        if !ir
            .model
            .features
            .iter()
            .any(|feature| feature.id == candidate)
        {
            break candidate;
        }
        // (O-capacity): all generated identity suffixes through u64::MAX are occupied.
        identity_ordinal = identity_ordinal.checked_add(1).ok_or_else(|| {
            CodecError::NotImplemented("generated feature identity exceeds u64".into())
        })?;
    };
    ir.model.features.push(Feature {
        id: id.clone(),
        ordinal,
        name: Some(name.into()),
        suppressed: Some(false),
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        needs_recompute: false,
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(operation)),
        native_ref: None,
    });
    Ok(id)
}

fn sketches(
    ir: &mut CadIr,
    native: Option<&SldprtNative>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let original = ir.clone();
    let try_sketch = |candidate: &CadIr| {
        let mut native = native.cloned();
        crate::resolved_features::write_prepare::prepare_sketches_for_write(
            candidate,
            &mut native,
            &std::collections::BTreeSet::new(),
        )
    };
    let mut dropped = HashSet::new();
    for sketch in ir.model.sketches.clone() {
        let mut candidate = ir.clone();
        candidate.model.sketches = vec![sketch.clone()];
        candidate
            .model
            .sketch_entities
            .retain(|entity| entity.sketch == sketch.id);
        candidate.model.sketch_constraints.clear();
        candidate.model.spatial_sketches.clear();
        candidate.model.spatial_sketch_entities.clear();
        candidate.model.spatial_sketch_constraints.clear();
        match try_sketch(&candidate) {
            Ok(()) => {}
            Err(CodecError::UnsupportedContent(reason)) => {
                let mut profiles = Vec::new();
                for profile in &sketch.profiles {
                    let mut part = candidate.clone();
                    part.model.sketches[0].profiles = vec![profile.clone()]
                        .try_into()
                        .map_err(CodecError::malformed)?;
                    part.model
                        .sketch_entities
                        .retain(|entity| profile.iter().any(|use_| &use_.entity == entity.id()));
                    match try_sketch(&part) {
                        Ok(()) => profiles.push(profile.clone()),
                        Err(CodecError::UnsupportedContent(_)) => {}
                        Err(error) => return Err(error),
                    }
                }
                losses.push(SldprtLossCode::WriterDesignReduced.note(format!(
                    "sketch {}: unsupported profile geometry omitted: {reason}",
                    sketch.id
                )));
                if profiles.is_empty() {
                    dropped.insert(sketch.id.clone());
                    continue;
                }
                ir.model.sketch_entities.retain(|entity| {
                    entity.sketch != sketch.id
                        || profiles
                            .iter()
                            .flatten()
                            .any(|use_| &use_.entity == entity.id())
                        || matches!(
                            entity.geometry.definition(),
                            SketchGeometryDefinition::Point { .. }
                        )
                });
                if let Some(value) = ir
                    .model
                    .sketches
                    .iter_mut()
                    .find(|value| value.id == sketch.id)
                {
                    value.profiles = profiles.try_into().map_err(CodecError::malformed)?;
                }
            }
            Err(error) => return Err(error),
        }
    }
    ir.model
        .sketches
        .retain(|sketch| !dropped.contains(&sketch.id));
    ir.model
        .sketch_entities
        .retain(|entity| !dropped.contains(&entity.sketch));
    ir.model
        .sketch_constraints
        .retain(|constraint| !dropped.contains(&constraint.sketch));
    for feature in &mut ir.model.features {
        if matches!(feature.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Sketch { sketch: SketchFeatureBinding::Planar(Some(sketch)) }) if dropped.contains(sketch))
        {
            super::reduce_feature_to_solved(feature)?;
        }
    }
    // Native spatial lanes hold point and line coordinates without a B-rep profile.
    // Relations without a generated native form are reported by arena accounting.
    ir.model.spatial_sketch_constraints.clear();
    ir.model.spatial_sketch_entities.retain(|entity| {
        let supported = matches!(
            entity.geometry.definition(),
            SpatialSketchGeometryDefinition::Point { .. }
                | SpatialSketchGeometryDefinition::Line { .. }
        );
        if !supported {
            losses.push(SldprtLossCode::WriterDesignReduced.note(format!(
                "spatial sketch entity {} has no native point/line encoding",
                entity.id()
            )));
        }
        supported
    });
    let empty_spatial = ir
        .model
        .spatial_sketches
        .iter()
        .filter(|sketch| {
            !ir.model
                .spatial_sketch_entities
                .iter()
                .any(|entity| entity.sketch == sketch.id)
        })
        .map(|sketch| sketch.id.clone())
        .collect::<HashSet<_>>();
    ir.model
        .spatial_sketches
        .retain(|sketch| !empty_spatial.contains(&sketch.id));
    for feature in &mut ir.model.features {
        if matches!(feature.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::SpatialSketch { sketch: Some(sketch) }) if empty_spatial.contains(sketch))
        {
            losses.push(SldprtLossCode::WriterDesignReduced.note(format!(
                "feature {}: spatial sketch has no writable geometry",
                feature.id
            )));
            super::reduce_feature_to_solved(feature)?;
        }
    }
    let constraints = std::mem::take(&mut ir.model.sketch_constraints);
    for constraint in constraints {
        ir.model.sketch_constraints.push(constraint.clone());
        match try_sketch(ir) {
            Ok(()) => {}
            Err(CodecError::UnsupportedContent(reason)) => {
                ir.model.sketch_constraints.pop();
                losses.push(SldprtLossCode::WriterDesignReduced.note(format!(
                    "sketch constraint {} omitted: {reason}",
                    constraint.id
                )));
            }
            Err(CodecError::Malformed(reason)) => {
                // Keep the C precondition for a relation already inconsistent
                // with its authored solved geometry. Missing operands introduced
                // by profile reduction are an export loss instead.
                let mut authored = original.clone();
                authored.model.sketch_constraints = vec![constraint.clone()];
                if let Err(error @ CodecError::Malformed(_)) = try_sketch(&authored) {
                    return Err(error);
                }
                ir.model.sketch_constraints.pop();
                losses.push(SldprtLossCode::WriterDesignReduced.note(format!(
                    "sketch constraint {} lost an omitted operand: {reason}",
                    constraint.id
                )));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
