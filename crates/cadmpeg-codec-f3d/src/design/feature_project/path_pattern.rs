// SPDX-License-Identifier: Apache-2.0
//! `PathPattern` scope projection.

use super::{design_positive_length, unique_feature_match};
use crate::ids::native_stream;
use crate::loss::F3dLossCode;
use crate::records::{
    feature::scope::DesignParameterScope, parameters::DesignParameter,
    topology::construction::DesignConstructionOperandGroup,
    topology::extrude_selection::DesignOperandRole,
};
use cadmpeg_ir::features::{
    patterns::{PatternKind, PatternSeed, PatternTransform},
    BodySelection, FaceSelection, FeatureDefinition, FeatureOperation, PathRef,
};
use cadmpeg_ir::report::loss::LossNote;
use std::collections::HashMap;

/// Source kind of the parameter that carries the instance count.
const COUNT_SOURCE_KIND: &str = "countU";
/// Source kind of the parameter that carries the path distance.
const DISTANCE_SOURCE_KIND: &str = "PathPatternDelta";

/// Pattern definition of a `PathPattern` scope and the losses its readings carry.
pub(super) struct ProjectedPathPattern {
    pub(super) definition: FeatureDefinition,
    pub(super) losses: Vec<LossNote>,
}

#[doc = "Neutral `PathPattern` definition with independently retained unresolved inputs."]
pub(super) fn path_pattern_definition(
    scope: &DesignParameterScope,
    inputs: &super::ProjectInputs<'_>,
    parameters: &[(u32, &DesignParameter)],
    authored_ordinals: &HashMap<(&str, u32), u64>,
    losses: &mut Vec<LossNote>,
) -> FeatureDefinition {
    let projected = project_path_pattern(scope, inputs.construction_groups, parameters)
        .unwrap_or_else(|| {
            project_partial_path_pattern(
                scope,
                inputs.construction_groups,
                parameters,
                inputs.entity_selection_operands,
                inputs.scopes,
                authored_ordinals,
            )
        });
    losses.extend(projected.losses);
    projected.definition
}

pub(super) fn project_partial_path_pattern(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    parameters: &[(u32, &DesignParameter)],
    selections: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    scopes: &[DesignParameterScope],
    authored_ordinals: &HashMap<(&str, u32), u64>,
) -> ProjectedPathPattern {
    use cadmpeg_ir::features::patterns::curve_driven::CurveDrivenPatternParameters;

    let mut losses = Vec::new();
    let stream = native_stream(&scope.id);
    let scope_groups = groups
        .iter()
        .filter(|group| {
            stream.is_some()
                && native_stream(&group.id) == stream
                && group.scope_record_index == scope.record_index
                && scope
                    .reference_members()
                    .values()
                    .any(|index| *index == group.record_index)
        })
        .collect::<Vec<_>>();
    let aligned = |group: &DesignConstructionOperandGroup| {
        let Some(start) = usize::try_from(group.scope_reference_ordinal).ok() else {
            return false;
        };
        let Some(end) = start
            .checked_add(1)
            .and_then(|start| start.checked_add(group.members().len()))
        else {
            return false;
        };
        scope
            .reference_members()
            .values_in(start..end)
            .is_some_and(|mut references| {
                references.next() == Some(&group.record_index)
                    && references
                        .copied()
                        .eq(group.members().iter().map(|member| member.value))
            })
    };
    let path_group = unique_feature_match(scope_groups.iter().copied().filter(|group| {
        group.role() == DesignOperandRole::ROLE_0X5
            && !group.members().is_empty()
            && aligned(group)
            && scope
                .path_pattern_settings()
                .and_then(|settings| settings.path_group_record_index)
                .is_none_or(|index| index == group.record_index)
    }));
    let seed_group = unique_feature_match(scope_groups.iter().copied().filter(|group| {
        group.role() != DesignOperandRole::ROLE_0X5 && !group.members().is_empty()
    }));
    let seeds = seed_group.filter(|group| aligned(group)).and_then(|group| {
        match group.role() {
            DesignOperandRole::BODIES_B => Some(vec![PatternSeed::Bodies(BodySelection::Native(group.id.clone()))]),
            DesignOperandRole::ROLE_0X10 => Some(vec![PatternSeed::Faces(FaceSelection::Native(group.id.clone()))]),
            DesignOperandRole::BODIES_A => {
                let stream = stream?;
                let ordinal = *authored_ordinals.get(&(stream, scope.record_index))?;
                let seeds = super::pattern_seeds::entity_selection_feature_seeds(scope, group, selections, scopes)?;
                if !seeds.iter().all(|seed| match seed {
                    PatternSeed::Feature(id) => scopes.iter().any(|candidate| {
                        native_stream(&candidate.id) == Some(stream)
                            && crate::ids::neutral_feature_id(candidate) == *id
                            && authored_ordinals.get(&(stream, candidate.record_index))
                                .is_some_and(|candidate_ordinal| *candidate_ordinal < ordinal)
                    }),
                    _ => false,
                }) {
                    return None;
                }
                losses.push(F3dLossCode::FeaturePathPatternFeatureSeedAssumed.note(format!(
                    "PathPattern scope {} reads its role-0x4 compact identities as preceding feature-scope indices in the owning Design stream.", scope.id
                )));
                Some(seeds)
            }
            _ => None,
        }
    }).unwrap_or_else(|| {
        let native = seed_group.map_or(&scope.id, |group| &group.id).clone();
        losses.push(F3dLossCode::FeaturePathPatternSeedUnresolved.note(format!(
            "PathPattern scope {} retains native seed {native}; its seed entity kind remains unresolved.", scope.id
        )));
        vec![PatternSeed::Native(native)]
    });
    let parameter =
        |ordinal, kind| {
            unique_feature_match(parameters.iter().filter(|(index, parameter)| {
                *index == ordinal && parameter.source_kind_str() == kind
            }))
            .map(|(_, parameter)| *parameter)
        };
    let count = parameter(0, COUNT_SOURCE_KIND)
        .filter(|value| value.unit().is_none())
        .map(|value| value.evaluated_value().get())
        .filter(|count| *count >= 1.0 && count.fract() == 0.0)
        .and_then(cadmpeg_core::convert::truncate_f64_to_u32);
    let distance = parameter(1, DISTANCE_SOURCE_KIND).and_then(design_positive_length);
    let settings = scope.path_pattern_settings();
    if path_group.is_none() {
        losses.push(F3dLossCode::FeaturePathPatternPathUnresolved.note(format!(
            "PathPattern scope {} retains its declared native path reference when available; a unique scoped path group remains unresolved.", scope.id
        )));
    }
    if settings.is_some_and(|settings| settings.reordered_owners) {
        losses.push(F3dLossCode::FeaturePathPatternReorderedSettingsAssumed.note(format!(
            "PathPattern scope {} reads the fixed controls of its 298-byte typed settings frame by the shared 267-byte settings layout; both parameter references match the leading owners.", scope.id
        )));
    }
    let mut decoded = CurveDrivenPatternParameters {
        path: path_group
            .map(|group| PathRef::Native(group.id.clone()))
            .or_else(|| {
                let index = settings?.path_group_record_index?;
                let stream = stream?;
                Some(PathRef::Native(format!("{stream}:design-record#{index}")))
            }),
        count,
        distance,
        total_extent: settings
            .and_then(|value| value.total_extent.as_ref())
            .map(|value| value.value),
        reversed: settings
            .and_then(|value| value.flipped.as_ref())
            .map(|value| value.value),
        path_oriented: settings
            .and_then(|value| value.path_oriented.as_ref())
            .map(|value| value.value),
        start_parameter: settings
            .and_then(|value| value.start_parameter.as_ref())
            .map(|value| value.value),
        ..CurveDrivenPatternParameters::default()
    };
    decoded.spacing = distance
        .zip(decoded.total_extent)
        .and_then(|(distance, total_extent)| {
            if total_extent {
                let intervals = count?.checked_sub(1).filter(|count| *count > 0)?;
                cadmpeg_ir::scalar::PositiveLength::new(distance.get() / f64::from(intervals))
            } else {
                Some(distance)
            }
        });
    losses.push(F3dLossCode::FeaturePathPatternDefinitionPartial.note(format!(
        "PathPattern scope {} retains independently decoded curve-driven inputs and controls; remaining placement and instance-table semantics stay unresolved.", scope.id
    )));
    ProjectedPathPattern {
        definition: FeatureDefinition::Operation(FeatureOperation::Pattern {
            seeds,
            pattern: PatternKind::new(PatternTransform::CurveDrivenPartial {
                parameters: decoded,
            })
            .expect("positive optional PathPattern count"),
        }),
        losses,
    }
}

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
    let settings = scope.path_pattern_settings().filter(|settings| {
        settings.path_group_record_index == Some(path_group.record_index)
            && settings.total_extent.is_some()
            && settings.flipped.is_some()
            && settings.path_oriented.is_some()
            && settings.start_parameter.is_some()
    });
    let curve_driven = settings
        .zip(parameter(0, COUNT_SOURCE_KIND))
        .zip(parameter(1, DISTANCE_SOURCE_KIND))
        .and_then(|((settings, count), distance)| {
            let count = count
                .unit()
                .is_none()
                .then(|| count.evaluated_value().get())
                .filter(|count| *count >= 1.0 && count.fract() == 0.0)
                .and_then(cadmpeg_core::convert::truncate_f64_to_u32)?;
            let distance = design_positive_length(distance)?;
            let spacing = if settings.total_extent.as_ref()?.value {
                let intervals = count.checked_sub(1).filter(|intervals| *intervals > 0)?;
                cadmpeg_ir::scalar::PositiveLength::new(distance.get() / f64::from(intervals))?
            } else {
                distance
            };
            PatternKind::new(PatternTransform::CurveDriven {
                path: Some(PathRef::Native(path_group.id.clone())),
                spacing,
                count,
            })
            .ok()
        });
    let mut dropped = Vec::new();
    if let Some(settings) = settings.filter(|_| curve_driven.is_some()) {
        if settings.flipped.as_ref().is_some_and(|value| value.value) {
            dropped.push("the flipped direction".to_owned());
        }
        if settings
            .path_oriented
            .as_ref()
            .is_some_and(|value| value.value)
        {
            dropped.push("the path-direction orientation".to_owned());
        }
        if let Some(parameter) = settings
            .start_parameter
            .as_ref()
            .filter(|value| value.value.get() != 0.0)
        {
            dropped.push(format!(
                "the start point at path parameter {}",
                parameter.value.get()
            ));
        }
    }
    let losses = if dropped.is_empty() {
        Vec::new()
    } else {
        vec![
            F3dLossCode::FeaturePathPatternPlacementNotTransferred.note(format!(
                "PathPattern scope {} drops {}: a curve-driven pattern has no field for them.",
                scope.id,
                dropped.join(", ")
            )),
        ]
    };
    Some(ProjectedPathPattern {
        definition: FeatureDefinition::Operation(FeatureOperation::Pattern {
            seeds: vec![seed],
            pattern: curve_driven.unwrap_or(PatternKind::UNRESOLVED_CURVE_DRIVEN),
        }),
        losses,
    })
}
