// SPDX-License-Identifier: Apache-2.0
//! `PathPattern` scope projection.

use super::{design_positive_length, native_scope_definition, unique_feature_match};
use crate::ids::native_stream;
use crate::loss::F3dLossCode;
use crate::records::{
    feature::scope::DesignParameterScope, parameters::DesignParameter,
    topology::construction::DesignConstructionOperandGroup,
    topology::extrude_selection::DesignOperandRole,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    patterns::{PatternKind, PatternSeed, PatternTransform},
    BodySelection, FaceSelection, FeatureDefinition, FeatureOperation, PathRef,
};
use cadmpeg_ir::report::loss::LossNote;

/// Source kind of the parameter that carries the instance count.
const COUNT_SOURCE_KIND: &str = "countU";
/// Source kind of the parameter that carries the path distance.
const DISTANCE_SOURCE_KIND: &str = "PathPatternDelta";

/// Pattern definition of a `PathPattern` scope and the losses its readings carry.
pub(super) struct ProjectedPathPattern {
    pub(super) definition: FeatureDefinition,
    pub(super) losses: Vec<LossNote>,
}

/// Definition of a `PathPattern` scope: its projected pattern, whose losses
/// join `losses`, or its native definition.
pub(super) fn path_pattern_definition(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    parameters: &[(u32, &DesignParameter)],
    losses: &mut Vec<LossNote>,
) -> Result<FeatureDefinition, CodecError> {
    match project_path_pattern(scope, groups, parameters) {
        Some(projected) => {
            losses.extend(projected.losses);
            Ok(projected.definition)
        }
        None => native_scope_definition(scope, parameters),
    }
}

/// Project a `PathPattern` scope from its two construction groups and its
/// count and distance parameters. Groups outside the scope's form leave it
/// native; unusable parameters leave the curve-driven form unresolved.
pub(super) fn project_path_pattern(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    parameters: &[(u32, &DesignParameter)],
) -> Option<ProjectedPathPattern> {
    if !scope.unclosed_construction_operand_groups.is_empty() {
        return None;
    }
    let stream = native_stream(&scope.id)?;
    let scope_groups = groups
        .iter()
        .filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
        })
        .collect::<Vec<_>>();
    let [first, second] = scope_groups.as_slice() else {
        return None;
    };
    let (seed_group, path_group) = if first.scope_reference_ordinal < second.scope_reference_ordinal
    {
        (*first, *second)
    } else {
        (*second, *first)
    };
    let seed_members = seed_group.members().len();
    let seed_ordinal = usize::try_from(seed_group.scope_reference_ordinal).ok()?;
    let path_ordinal = usize::try_from(path_group.scope_reference_ordinal).ok()?;
    let seed_end = seed_ordinal.checked_add(1)?.checked_add(seed_members)?;
    if seed_ordinal != 1
        || seed_members == 0
        || path_ordinal < seed_end
        || path_group.role() != DesignOperandRole::ROLE_0X5
        || path_group.members().is_empty()
        || !seed_group
            .members()
            .iter()
            .map(|member| member.value)
            .eq(scope
                .reference_members()
                .values_in(seed_ordinal + 1..seed_end)?
                .copied())
    {
        return None;
    }
    let seed = match seed_group.role() {
        DesignOperandRole::BODIES_B => {
            PatternSeed::Bodies(BodySelection::Native(seed_group.id.clone()))
        }
        DesignOperandRole::ROLE_0X10 => {
            PatternSeed::Faces(FaceSelection::Native(seed_group.id.clone()))
        }
        _ => return None,
    };
    let parameter = |local_ordinal: u32, source_kind: &str| {
        unique_feature_match(parameters.iter().filter(|(ordinal, parameter)| {
            *ordinal == local_ordinal && parameter.source_kind_str() == source_kind
        }))
        .map(|(_, parameter)| *parameter)
    };
    let curve_driven = parameter(0, COUNT_SOURCE_KIND)
        .zip(parameter(1, DISTANCE_SOURCE_KIND))
        .and_then(|(count, distance)| {
            let count = count
                .unit()
                .is_none()
                .then(|| count.evaluated_value().get())
                .filter(|count| *count >= 1.0 && count.fract() == 0.0)
                .and_then(cadmpeg_core::convert::truncate_f64_to_u32)?;
            PatternKind::new(PatternTransform::CurveDriven {
                path: Some(PathRef::Native(path_group.id.clone())),
                spacing: design_positive_length(distance)?,
                count,
            })
            .ok()
        });
    let losses = if curve_driven.is_some() {
        vec![
            F3dLossCode::FeaturePathPatternCountAssumed.note(format!(
                "PathPattern scope {} reads its `{COUNT_SOURCE_KIND}` parameter as the instance count including the seed (DR-290).",
                scope.id
            )),
            F3dLossCode::FeaturePathPatternSpacingAssumed.note(format!(
                "PathPattern scope {} reads its `{DISTANCE_SOURCE_KIND}` parameter as the arc-length spacing between adjacent instances (DR-290).",
                scope.id
            )),
            F3dLossCode::FeaturePathPatternPlacementAssumed.note(format!(
                "PathPattern scope {} places its first instance at the seed and repeats along the stored path direction (DR-290).",
                scope.id
            )),
        ]
    } else {
        Vec::new()
    };
    Some(ProjectedPathPattern {
        definition: FeatureDefinition::Operation(FeatureOperation::Pattern {
            seeds: vec![seed],
            pattern: curve_driven.unwrap_or(PatternKind::UNRESOLVED_CURVE_DRIVEN),
        }),
        losses,
    })
}
