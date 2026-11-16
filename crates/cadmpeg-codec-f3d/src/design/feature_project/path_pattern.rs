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
use cadmpeg_core::CodecError;
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
) -> Result<FeatureDefinition, CodecError> {
    let projected = match project_path_pattern(scope, inputs.construction_groups, parameters) {
        Some(projected) => projected,
        None => project_partial_path_pattern(
            scope,
            inputs.construction_groups,
            parameters,
            inputs.face_operands,
            inputs.entity_selection_operands,
            inputs.scopes,
            authored_ordinals,
        )?,
    };
    losses.extend(projected.losses);
    Ok(projected.definition)
}

pub(super) fn project_partial_path_pattern(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    parameters: &[(u32, &DesignParameter)],
    faces: &[crate::records::topology::face::DesignFaceOperand],
    selections: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    scopes: &[DesignParameterScope],
    authored_ordinals: &HashMap<(&str, u32), u64>,
) -> Result<ProjectedPathPattern, CodecError> {
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
    let candidates = scope_groups
        .iter()
        .copied()
        .filter(|group| group.role() != DesignOperandRole::ROLE_0X5)
        .collect::<Vec<_>>();
    let declared_path = scope
        .path_pattern_settings()
        .and_then(|settings| settings.path_group_record_index);
    let unclosed = scope
        .unclosed_construction_operand_groups
        .iter()
        .copied()
        .filter(|index| Some(*index) != declared_path)
        .collect::<Vec<_>>();
    let seed_group = (unclosed.is_empty())
        .then(|| unique_feature_match(candidates.iter().copied()))
        .flatten();
    let seeds = match seed_group.filter(|group| aligned(group) && !group.members().is_empty()) {
        Some(group) if group.role() == DesignOperandRole::BODIES_B => {
            vec![PatternSeed::Bodies(BodySelection::Native(group.id.clone()))]
        }
        Some(group) if group.role() == DesignOperandRole::ROLE_0X10 => {
            vec![PatternSeed::Faces(FaceSelection::Native(group.id.clone()))]
        }
        Some(group) if group.role() == DesignOperandRole::BODIES_A => partial_face_role_seeds(
            scope,
            group,
            faces,
            selections,
            scopes,
            authored_ordinals,
            &mut losses,
        ),
        _ => {
            let mut retained = candidates
                .iter()
                .map(|group| (group.scope_reference_ordinal, group.id.clone()))
                .collect::<Vec<_>>();
            if let Some(stream) = stream {
                for index in unclosed {
                    for (ordinal, _) in scope
                        .reference_members()
                        .values()
                        .enumerate()
                        .filter(|(_, value)| **value == index)
                    {
                        if let Ok(ordinal) = u32::try_from(ordinal) {
                            retained.push((ordinal, format!("{stream}:design-record#{index}")));
                        }
                    }
                }
            }
            retained.sort_by_key(|(ordinal, _)| *ordinal);
            losses.push(F3dLossCode::FeaturePathPatternSeedUnresolved.note(format!(
                "PathPattern scope {} retains {} candidate seed groups; their roles or membership remain unresolved.", scope.id, retained.len()
            )));
            retained
                .into_iter()
                .map(|(_, id)| PatternSeed::Native(id))
                .collect()
        }
    };
    let parameter =
        |ordinal, kind| {
            unique_feature_match(parameters.iter().filter(|(index, parameter)| {
                *index == ordinal && parameter.source_kind_str() == kind
            }))
            .map(|(_, parameter)| *parameter)
        };
    let source_count = parameter(0, COUNT_SOURCE_KIND)
        .filter(|value| value.unit().is_none())
        .map(|value| value.evaluated_value().get())
        .filter(|count| *count >= 1.0 && count.fract() == 0.0);
    let count = source_count.and_then(cadmpeg_core::convert::truncate_f64_to_u32);
    if source_count.is_some() && count.is_none() {
        losses.push(F3dLossCode::FeaturePathPatternCountOutOfRange.note(format!(
            "PathPattern scope {} retains source count {:?} in its parameter; it exceeds the neutral instance-count range.", scope.id, source_count
        )));
    }
    let distance = parameter(1, DISTANCE_SOURCE_KIND).and_then(design_positive_length);
    if let Some(parameter) = parameter(1, DISTANCE_SOURCE_KIND).filter(|parameter| {
        parameter
            .unit()
            .is_some_and(|unit| super::design_length_unit(unit.value.as_str()))
            && parameter.evaluated_value().get() <= 0.0
    }) {
        losses.push(F3dLossCode::FeaturePathPatternDistanceNonpositive.note(format!(
            "PathPattern scope {} retains source distance {} in parameter {}; neutral positive spacing is unavailable.", scope.id, parameter.evaluated_value().get(), parameter.id
        )));
    }
    let settings = scope.path_pattern_settings();
    if settings.is_none_or(|value| {
        value.settings_record_index.is_none()
            || value.total_extent.is_none()
            || value.flipped.is_none()
            || value.path_oriented.is_none()
    }) {
        losses.push(
            F3dLossCode::FeaturePathPatternSettingsUnresolved.note(format!(
                "PathPattern scope {} retains partial or unavailable placement controls.",
                scope.id
            )),
        );
    }
    if settings
        .and_then(|value| value.start_parameter.as_ref())
        .is_none()
    {
        losses.push(F3dLossCode::FeaturePathPatternStartUnresolved.note(format!(
            "PathPattern scope {} has no uniquely decoded finite path-start parameter.",
            scope.id
        )));
    }
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
    Ok(ProjectedPathPattern {
        definition: FeatureDefinition::Operation(FeatureOperation::Pattern {
            seeds,
            pattern: PatternKind::new(PatternTransform::CurveDrivenPartial {
                parameters: decoded,
            })
            .map_err(CodecError::malformed)?,
        }),
        losses,
    })
}

fn partial_face_role_seeds(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    faces: &[crate::records::topology::face::DesignFaceOperand],
    selections: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    scopes: &[DesignParameterScope],
    authored_ordinals: &HashMap<(&str, u32), u64>,
    losses: &mut Vec<LossNote>,
) -> Vec<PatternSeed> {
    let Some(stream) = native_stream(&scope.id) else {
        return Vec::new();
    };
    let pattern_ordinal = authored_ordinals.get(&(stream, scope.record_index));
    group.members().iter().enumerate().map(|(ordinal, member)| {
        let native = format!("{stream}:design-record#{}", member.value);
        let ordinal = u32::try_from(ordinal).ok();
        let matching_faces = faces.iter().filter(|face| {
            native_stream(&face.id) == Some(stream)
                && face.scope_record_index == scope.record_index
                && face.group_record_index() == Some(group.record_index)
                && face.group_member_ordinal() == ordinal
                && face.record_index() == member.value
        }).collect::<Vec<_>>();
        let face = match matching_faces.as_slice() {
            [face] if !face.resolved_face_slots.is_empty() => scope.previous_history_state_id().and_then(|state| {
                crate::design::face_resolve::historical_face_selection_with_native(scope, state, face.resolved_face_slots.clone(), face.id.clone())
            }),
            _ => None,
        };
        let feature = unique_feature_match(selections.iter().filter(|selection| {
            native_stream(&selection.id) == Some(stream)
                && selection.scope_record_index == scope.record_index
                && selection.group_record_index == group.record_index
                && Some(selection.group_member_ordinal) == ordinal
                && selection.record_index() == member.value
        })).and_then(|selection| u32::try_from(selection.compact_identity()?).ok())
            .and_then(|index| unique_feature_match(scopes.iter().filter(|candidate| {
                native_stream(&candidate.id) == Some(stream)
                    && candidate.record_index == index
                    && candidate.record_index != scope.record_index
                    && candidate.history_state_id().is_some()
            }))).filter(|candidate| authored_ordinals.get(&(stream, candidate.record_index)).zip(pattern_ordinal).is_some_and(|(candidate, pattern)| candidate < pattern));
        match (face, feature, matching_faces.is_empty()) {
            (Some(face), None, _) => PatternSeed::Faces(face),
            (None, Some(feature), true) => {
                losses.push(F3dLossCode::FeaturePathPatternFeatureSeedAssumed.note(format!(
                    "PathPattern scope {} reads member {} as preceding feature {}.", scope.id, member.value, feature.id
                )));
                PatternSeed::Feature(crate::ids::neutral_feature_id(feature))
            }
            _ => {
                losses.push(F3dLossCode::FeaturePathPatternSeedUnresolved.note(format!(
                    "PathPattern scope {} retains seed member {} with unresolved or competing face and feature readings.", scope.id, member.value
                )));
                PatternSeed::Native(native)
            }
        }
    }).collect()
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
