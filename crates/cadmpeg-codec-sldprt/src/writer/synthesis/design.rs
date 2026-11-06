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
use crate::writer::write_error::WriteError;

pub(super) fn prepare(
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
    repair: bool,
) -> Result<Option<SldprtNative>, WriteError> {
    if !ir.scope.design {
        return Ok(ir
            .native
            .namespace("sldprt")
            .map(SldprtNative::load)
            .transpose()?);
    }
    ensure_owners(ir)?;
    super::reduce_design(ir, losses, repair)?;
    let mut native = ir
        .native
        .namespace("sldprt")
        .map(SldprtNative::load)
        .transpose()?;
    let orphaned =
        crate::history::write::parameters::orphaned_scalar_parameters(ir, native.as_ref())
            .map_err(|error| {
                cadmpeg_core::CodecError::malformed(format_args!(
                    "SLDPRT orphaned scalar scan failed: {error}"
                ))
            })?;
    synthesize_neutral_features_repairing(ir, &mut native, &orphaned, losses, repair)?;
    sketches(ir, native.as_ref(), losses)?;
    // A dropped sketch retains its feature and solved output, with an explicit loss.
    synthesize_neutral_features_repairing(ir, &mut native, &orphaned, losses, repair)?;
    crate::resolved_features::write_prepare::prepare_sketches_for_write(ir, &mut native)?;
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
    let mut written_sketch_states = HashSet::new();
    for configuration in &ir.model.configurations {
        let slot = crate::history::write::configurations::configuration_slot(
            &configuration.properties,
            configuration.ordinal,
        )
        .to_string();
        for (feature, state) in &configuration.feature_states {
            let authored = match state.definition() {
                Some(FeatureDefinition::Operation(FeatureOperation::Sketch {
                    sketch: SketchFeatureBinding::Planar(Some(sketch)),
                })) => {
                    ir.model.sketches.iter().any(|record| {
                        record.id == *sketch
                            && record.configuration.as_deref() == Some(slot.as_str())
                    }) && ir.model.features.iter().any(|candidate| {
                        candidate.id == *feature
                            && crate::resolved_features::write_prepare::feature_owns_planar_sketch(
                                ir, candidate, sketch,
                            )
                    })
                }
                Some(FeatureDefinition::Operation(FeatureOperation::SpatialSketch {
                    sketch: Some(sketch),
                })) => {
                    ir.model.spatial_sketches.iter().any(|record| {
                        record.id == *sketch
                            && record.configuration.as_deref() == Some(slot.as_str())
                    }) && ir.model.features.iter().any(|candidate| {
                        candidate.id == *feature
                            && crate::resolved_features::write_prepare::feature_owns_spatial_sketch(
                                ir, candidate, sketch,
                            )
                    })
                }
                _ => false,
            };
            let lane = native.as_ref().is_some_and(|native| {
                native.feature_input_lanes.iter().any(|lane| {
                    lane.configuration.as_deref() == Some(slot.as_str())
                        && lane.id.starts_with("sldprt:generated:")
                        && !lane.native_payload.is_empty()
                })
            });
            if authored && lane {
                written_sketch_states.insert((configuration.id.clone(), feature.clone()));
            }
        }
    }
    let written_operations = match native.as_mut() {
        Some(native) => {
            crate::resolved_features::write_feature_parameters::synthesize_configuration_operations(
                ir, native,
            )?
        }
        None => HashSet::new(),
    };
    for configuration in &mut ir.model.configurations {
        for feature in configuration.feature_states.keys() {
            if !written_sketch_states.contains(&(configuration.id.clone(), feature.clone()))
                && !written_operations.contains(&(configuration.id.clone(), feature.clone()))
            {
                losses.push(SldprtLossCode::WriterDesignReduced.note(format!(
                    "configuration {} feature {feature}: structural override has no native lane serializer", configuration.id
                )));
            }
        }
        configuration.feature_states.clear();
        for (parameter, expression) in &configuration.parameter_overrides {
            let expected = configuration.parameter_values.get(parameter).or_else(|| {
                ir.model
                    .parameters
                    .iter()
                    .find(|candidate| candidate.id == *parameter)
                    .and_then(|candidate| candidate.value.as_ref())
            });
            let value = expected.and_then(|expected| expression.literal_value_as(expected));
            let native_expression = value.as_ref().and_then(|value| match value {
                cadmpeg_ir::features::ParameterValue::Length(value) => {
                    Some(crate::history::literals::format_length_mm(*value))
                }
                cadmpeg_ir::features::ParameterValue::Angle(value) => {
                    Some(crate::history::literals::format_angle_rad(*value))
                }
                cadmpeg_ir::features::ParameterValue::Real(value) => Some(value.get().to_string()),
                cadmpeg_ir::features::ParameterValue::Integer(value) => {
                    cadmpeg_core::convert::f64_from_i64(*value).map(|value| value.to_string())
                }
                _ => None,
            });
            if let Some(value) = value.filter(|value| {
                native_expression.as_ref().is_some_and(|text| {
                    *expression == cadmpeg_ir::features::Expression::text(text.clone())
                }) && configuration
                    .parameter_values
                    .get(parameter)
                    .is_none_or(|stored| stored == value)
            }) {
                configuration
                    .parameter_values
                    .insert(parameter.clone(), value);
            } else {
                losses.push(SldprtLossCode::WriterDesignReduced.note(format!(
                    "configuration {} parameter {parameter}: expression override omitted; its evaluated scalar is retained when representable", configuration.id
                )));
            }
        }
        configuration.parameter_values.retain(|parameter, value| {
            if match value {
                cadmpeg_ir::features::ParameterValue::Length(_)
                | cadmpeg_ir::features::ParameterValue::Angle(_)
                | cadmpeg_ir::features::ParameterValue::Real(_)
                | cadmpeg_ir::features::ParameterValue::Boolean(_) => true,
                cadmpeg_ir::features::ParameterValue::Integer(value) =>
                    cadmpeg_core::convert::f64_from_i64(*value).is_some(),
                cadmpeg_ir::features::ParameterValue::String(_) => false,
            } { return true; }
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
        true,
    )?;
    if let Some(native) = native.as_mut() {
        crate::resolved_features::write_feature_parameters::synthesize(ir, native)?;
    }
    Ok(native)
}

/// Author native history records, repairing stale dependency edges from the
/// canonical operation projection instead of refusing (#237).
///
/// A retained native projection is authoritative for the edges its operation
/// carries (review E). When the first attempt reports an edge mismatch and
/// `repair` holds, the wrapper recomputes the mismatched edges from the
/// just-built native projection, charges one loss per repaired feature, and
/// rebuilds once. Any other failure, an unrepairable mismatch, a repeated
/// mismatch, or `repair` refused propagates unchanged.
///
/// `repair` holds only for source-less synthesis: with a retained source
/// image the mismatch names an edit the patch path declined, and the refusal
/// stands so the caller decides (the reviewed refusal pin). Without one there
/// is no retained truth to decline against, so the canonical side is written
/// with its loss.
pub(super) fn synthesize_neutral_features_repairing(
    ir: &mut CadIr,
    native: &mut Option<SldprtNative>,
    orphaned: &std::collections::HashSet<cadmpeg_ir::features::ParameterId>,
    losses: &mut Vec<LossNote>,
    repair: bool,
) -> Result<
    (
        Vec<crate::history::write::features::FeatureInputRename>,
        std::collections::BTreeSet<String>,
    ),
    WriteError,
> {
    match crate::history::write::features::synthesize_neutral_features(
        &ir.model,
        &ir.model.parameters,
        &ir.model.bodies,
        native,
        orphaned,
    ) {
        Err(error) if repair && !matches!(error, WriteError::Capacity { .. }) => {
            let Some(built) = native.as_ref() else {
                return Err(error);
            };
            let record_ids =
                crate::history::write::features::feature_record_ids(&ir.model.features);
            let projected = crate::history::write::project_features_with_native_inputs(
                built,
                &ir.model.sketches,
            )?;
            let mismatches = crate::history::write::features::dependency_mismatches(
                &ir.model.features,
                &record_ids,
                &projected,
            )?;
            let mut repaired = false;
            for (index, _, canonical) in &mismatches {
                let Some(canonical) = canonical else {
                    continue;
                };
                let feature = &mut ir.model.features[*index];
                let stale = feature.dependencies.len();
                feature.dependencies = canonical.clone();
                repaired = true;
                losses.push(SldprtLossCode::WriterDesignReduced.note(format!(
                    "feature {}: dependencies recomputed from the canonical operation projection; {stale} stale edge(s) replaced",
                    feature.id
                )));
            }
            if !repaired {
                return Err(error);
            }
            crate::history::write::features::synthesize_neutral_features(
                &ir.model,
                &ir.model.parameters,
                &ir.model.bodies,
                native,
                orphaned,
            )
        }
        ok => ok,
    }
}

const GLOBAL_PARAMETERS: &str = "Global parameters";

/// The generated "Global parameters" owner (#237 §7.10), appended on first use.
pub(super) fn generated_global_owner(ir: &mut CadIr) -> Result<FeatureId, CodecError> {
    if let Some(id) = find_generated_global_owner(ir) {
        return Ok(id);
    }
    append_feature(
        ir,
        GLOBAL_PARAMETERS,
        FeatureOperation::TreeNode {
            role: FeatureTreeNodeRole::Equations,
            children: TreeChildren::default(),
        },
    )
}

/// The generated "Global parameters" owner, when the document has one.
pub(super) fn find_generated_global_owner(ir: &CadIr) -> Option<FeatureId> {
    ir.model
        .features
        .iter()
        .find(|feature| {
            feature.native_ref.is_none()
                && feature.name.as_deref() == Some(GLOBAL_PARAMETERS)
                && feature.id.as_str().starts_with(GENERATED_FEATURE_PREFIX)
                && matches!(
                    feature.evaluation.definition(),
                    FeatureDefinition::Operation(FeatureOperation::TreeNode {
                        role: FeatureTreeNodeRole::Equations,
                        ..
                    })
                )
        })
        .map(|feature| feature.id.clone())
}

fn ensure_owners(ir: &mut CadIr) -> Result<(), CodecError> {
    if ir
        .model
        .parameters
        .iter()
        .any(|parameter| parameter.owner.is_none())
    {
        let id = generated_global_owner(ir)?;
        for parameter in &mut ir.model.parameters {
            if parameter.owner.is_none() {
                parameter.owner = Some(id.clone());
            }
        }
    }
    for sketch in ir.model.sketches.clone() {
        if ir
            .model
            .sketch_constraints
            .iter()
            .any(|constraint| constraint.sketch == sketch.id)
            && !ir.model.features.iter().any(|feature| {
                crate::resolved_features::write_prepare::feature_owns_planar_sketch(
                    ir, feature, &sketch.id,
                )
            })
        {
            append_feature(
                ir,
                sketch.name.as_deref().unwrap_or("Sketch"),
                FeatureOperation::Sketch {
                    sketch: SketchFeatureBinding::Planar(Some(sketch.id)),
                },
            )?;
        }
    }
    for sketch in ir.model.spatial_sketches.clone() {
        if !ir.model.features.iter().any(|feature| {
            crate::resolved_features::write_prepare::feature_owns_spatial_sketch(
                ir, feature, &sketch.id,
            )
        }) {
            append_feature(
                ir,
                sketch.name.as_deref().unwrap_or("Spatial sketch"),
                FeatureOperation::SpatialSketch {
                    sketch: Some(sketch.id),
                },
            )?;
        }
    }
    Ok(())
}

const GENERATED_FEATURE_PREFIX: &str = "generated:sldprt:feature#";

fn append_feature(
    ir: &mut CadIr,
    name: &str,
    operation: FeatureOperation,
) -> Result<FeatureId, CodecError> {
    let ordinal = match ir
        .model
        .features
        .iter()
        .map(|feature| feature.ordinal)
        .max()
    {
        None => 0,
        Some(maximum) if maximum < u64::MAX => maximum + 1,
        Some(_) => {
            let mut order = (0..ir.model.features.len()).collect::<Vec<_>>();
            order.sort_by_key(|index| ir.model.features[*index].ordinal);
            for (rank, index) in order.into_iter().enumerate() {
                ir.model.features[index].ordinal = cadmpeg_core::decode::u64_from_index(rank);
            }
            cadmpeg_core::decode::u64_from_index(ir.model.features.len())
        }
    };
    let used = ir
        .model
        .features
        .iter()
        .map(|feature| feature.id.as_str())
        .collect::<HashSet<_>>();
    let suffixes = std::iter::once(ordinal)
        .chain(0..=cadmpeg_core::decode::u64_from_index(ir.model.features.len()));
    let id = suffixes
        .map(|suffix| format!("{GENERATED_FEATURE_PREFIX}{suffix}"))
        .find(|candidate| !used.contains(candidate.as_str()))
        .ok_or_else(|| CodecError::malformed("cannot allocate a generated feature identity"))?;
    let id = FeatureId::mint(id).map_err(|error| {
        CodecError::malformed(format_args!("generated feature identity: {error}"))
    })?;
    ir.model.features.push(Feature {
        product_definition: None,
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
) -> Result<(), WriteError> {
    let original = ir.clone();
    let try_sketch = |candidate: &CadIr| {
        let mut native = native.cloned();
        crate::resolved_features::write_prepare::prepare_sketches_for_write(candidate, &mut native)
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
            Err(WriteError::Decline(reason)) => {
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
                        Err(WriteError::Decline(_)) => {}
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
    for sketch in &mut ir.model.spatial_sketches {
        for (index, _) in sketch.profiles.drain(..).enumerate() {
            losses.push(SldprtLossCode::WriterFieldsOmitted.note(format!(
                "spatial sketch {} profile {index} has no native profile encoding",
                sketch.id
            )));
        }
        if sketch.visible.take().is_some() {
            losses.push(SldprtLossCode::WriterFieldsOmitted.note(format!(
                "spatial sketch {}: visibility has no native spatial-sketch encoding",
                sketch.id
            )));
        }
    }
    // Native spatial lanes hold point and line coordinates without a B-rep profile.
    // Relations without a generated native form are reported by arena accounting.
    let parameters = &ir.model.parameters;
    let spatial_entities = &ir.model.spatial_sketch_entities;
    ir.model.spatial_sketch_constraints.retain(|constraint| {
        if crate::resolved_features::write_prepare::native_spatial_relation_supported(
            constraint.definition.kind(), parameters,
        ) || matches!(constraint.definition.kind(),
            cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput::PointLineDistance {
                point, line, parameter: cadmpeg_ir::features::Dimension::Parameter(id),
            } if parameters.iter().any(|parameter| &parameter.id == id && parameter.driving && matches!(parameter.value, Some(cadmpeg_ir::features::ParameterValue::Length(_))))
                && spatial_entities.iter().any(|entity| entity.id() == point)
                && spatial_entities.iter().any(|entity| entity.id() == line)
        ) {
            true
        } else {
            losses.push(SldprtLossCode::WriterDesignReduced.note(format!(
                "spatial sketch constraint {} has no native relation serializer",
                constraint.id
            )));
            false
        }
    });
    let mut relation_entities = HashSet::new();
    for constraint in &ir.model.spatial_sketch_constraints {
        if let cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput::PointLineDistance {
            point,
            line,
            ..
        } = constraint.definition.kind()
        {
            relation_entities.insert(point);
            relation_entities.insert(line);
        }
    }
    for entity in &mut ir.model.spatial_sketch_entities {
        if entity.construction && !relation_entities.contains(entity.id()) {
            losses.push(SldprtLossCode::WriterFieldsOmitted.note(format!(
                "spatial sketch entity {}: the independent construction state has no native encoding",
                entity.id()
            )));
            entity.construction = false;
        }
    }
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
            Err(WriteError::Decline(reason)) => {
                ir.model.sketch_constraints.pop();
                losses.push(SldprtLossCode::WriterDesignReduced.note(format!(
                    "sketch constraint {} omitted: {reason}",
                    constraint.id
                )));
            }
            Err(WriteError::Fatal(CodecError::Malformed(reason))) => {
                if reason.contains("is not owned by its sketch feature") {
                    let mut local = constraint.clone();
                    if localize_dimension(ir, &mut local)? {
                        ir.model.sketch_constraints.pop();
                        ir.model.sketch_constraints.push(local);
                        if try_sketch(ir).is_ok() {
                            losses.push(SldprtLossCode::WriterDesignReduced.note(format!(
                                "sketch constraint {}: external parameter binding replaced by its evaluated value",
                                constraint.id
                            )));
                            continue;
                        }
                        ir.model.sketch_constraints.pop();
                        ir.model.sketch_constraints.push(constraint.clone());
                    }
                }
                let mut authored = original.clone();
                authored.model.sketch_constraints = vec![constraint.clone()];
                if let Err(error @ WriteError::Fatal(CodecError::Malformed(_))) =
                    try_sketch(&authored)
                {
                    if !reason.contains("is not satisfied by measured geometry") {
                        return Err(error);
                    }
                }
                ir.model.sketch_constraints.pop();
                losses.push(SldprtLossCode::WriterDesignReduced.note(format!(
                    "sketch constraint {} omitted: {reason}",
                    constraint.id
                )));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn localize_dimension(
    ir: &CadIr,
    constraint: &mut cadmpeg_ir::sketches::SketchConstraint,
) -> Result<bool, CodecError> {
    use cadmpeg_ir::features::{Dimension, DimensionValue};
    use cadmpeg_ir::sketches::SketchConstraintDefinitionInput as Definition;

    fn localize<Q: DimensionValue>(ir: &CadIr, dimension: &mut Dimension<Q>) -> bool {
        if dimension.parameter().is_none() {
            return false;
        }
        let Some(value) = ir.dimension_value(dimension) else {
            return false;
        };
        *dimension = Dimension::Literal(value);
        true
    }

    let mut definition = constraint.definition.kind().clone();
    let changed = match &mut definition {
        Definition::Distance { parameter, .. }
        | Definition::DistanceLoci { parameter, .. }
        | Definition::HorizontalDistance { parameter, .. }
        | Definition::VerticalDistance { parameter, .. } => localize(ir, parameter),
        Definition::DistanceLociValue { distance, .. } => localize(ir, distance),
        Definition::Angle { parameter, .. } => localize(ir, parameter),
        Definition::Radius { parameter, .. } | Definition::Diameter { parameter, .. } => {
            localize(ir, parameter)
        }
        _ => false,
    };
    if changed {
        constraint.definition = definition.try_into().map_err(CodecError::malformed)?;
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::append_feature;
    use cadmpeg_ir::features::FeatureOperation;

    #[test]
    fn generated_owner_follows_a_maximum_input_ordinal() {
        let mut ir = cadmpeg_ir::CadIr::empty();
        let first = append_feature(&mut ir, "First", FeatureOperation::StoredGeometry {}).unwrap();
        ir.model.features[0].ordinal = u64::MAX;
        let second =
            append_feature(&mut ir, "Second", FeatureOperation::StoredGeometry {}).unwrap();
        let first = ir
            .model
            .features
            .iter()
            .find(|feature| feature.id == first)
            .unwrap();
        let second = ir
            .model
            .features
            .iter()
            .find(|feature| feature.id == second)
            .unwrap();
        assert!(first.ordinal < second.ordinal);
    }

    #[test]
    fn generated_owner_finds_a_gap_after_a_maximum_suffix_collision() {
        let mut ir = cadmpeg_ir::CadIr::empty();
        append_feature(&mut ir, "First", FeatureOperation::StoredGeometry {}).unwrap();
        ir.model.features[0].ordinal = u64::MAX - 1;
        ir.model.features[0].id =
            cadmpeg_ir::features::FeatureId::mint(format!("generated:sldprt:feature#{}", u64::MAX))
                .unwrap();
        let id = append_feature(&mut ir, "Second", FeatureOperation::StoredGeometry {}).unwrap();
        assert_ne!(id, ir.model.features[0].id);
        assert!(ir.model.features[0].ordinal < ir.model.features[1].ordinal);
    }

    #[test]
    fn native_history_writes_feature_ordinals_by_rank() {
        let mut ir = cadmpeg_ir::CadIr::empty();
        append_feature(&mut ir, "First", FeatureOperation::StoredGeometry {}).unwrap();
        append_feature(&mut ir, "Second", FeatureOperation::StoredGeometry {}).unwrap();
        ir.model.features[0].ordinal = u64::MAX - 1;
        ir.model.features[1].ordinal = u64::MAX;
        let mut native = None;
        crate::history::write::features::synthesize_neutral_features(
            &ir.model,
            &ir.model.parameters,
            &ir.model.bodies,
            &mut native,
            &std::collections::HashSet::new(),
        )
        .unwrap();
        let native = native.unwrap();
        let features = &native.feature_histories[0].features;
        assert!(
            features
                .iter()
                .find(|feature| feature.name == "First")
                .unwrap()
                .ordinal
                < features
                    .iter()
                    .find(|feature| feature.name == "Second")
                    .unwrap()
                    .ordinal
        );
    }
}
