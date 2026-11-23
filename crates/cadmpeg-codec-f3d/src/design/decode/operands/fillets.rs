// SPDX-License-Identifier: Apache-2.0
//! Fillet radius groups and fixed fillet assignments.

use crate::design::{design_feature_family, DesignFeatureFamily};
use crate::ids::native_stream;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::{DesignParameter, DesignParameterOwner};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::records::topology::fillet::{DesignFilletRadiusGroup, DesignFilletRadiusLaw};
use std::collections::HashMap;

/// Pair Fillet construction-operand groups with their radius inputs.
pub(super) fn push_fillet_radius_group(
    out: &mut Vec<DesignFilletRadiusGroup>,
    stream: &str,
    group_ordinal: u32,
    group: &DesignConstructionOperandGroup,
    law: DesignFilletRadiusLaw,
    tangency_weight_parameter_record_index: Option<u32>,
) {
    const SUFFIX: &str = ":design-fillet-radius-group#";

    let edge_operand_record_indices = group
        .members()
        .iter()
        .map(|member| member.value)
        .collect::<Vec<_>>();
    let id = format!("{stream}{SUFFIX}{}", group.record_index);
    out.push(DesignFilletRadiusGroup {
        id,
        scope_record_index: group.scope_record_index,
        group_ordinal,
        group_record_index: group.record_index,
        edge_operand_record_indices,
        law,
        tangency_weight_parameter_record_index,
    });
}

/// The edge group and the face group of a Fillet selection set that stores its
/// face selections in a separate group, in scope-reference order.
pub(crate) fn fillet_edge_and_face_groups<'a>(
    scope_groups: &[&'a DesignConstructionOperandGroup],
) -> Option<[&'a DesignConstructionOperandGroup; 2]> {
    let [first, second] = scope_groups else {
        return None;
    };
    let roles = [first.role(), second.role()];
    (roles.contains(&DesignOperandRole::BODIES_B) && roles.contains(&DesignOperandRole::ROLE_0X10))
        .then_some([*first, *second])
}

/// Whether fixed scalar laws leave several construction groups without a pairing.
pub(crate) fn fixed_fillet_assignment_is_unresolved(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
) -> bool {
    let Some(fixed) = scope.fixed_fillet_parameters() else {
        return false;
    };
    let stream = native_stream(&scope.id);
    let scope_groups = groups
        .iter()
        .filter(|group| {
            native_stream(&group.id) == stream && group.scope_record_index == scope.record_index
        })
        .collect::<Vec<_>>();
    scope_groups.len() > 1
        && scope_groups.len() != fixed.groups.len()
        && !(fixed.groups.len() == 1 && fillet_edge_and_face_groups(&scope_groups).is_some())
}

/// Whether a parameter-owned Fillet scope has operand groups, owned
/// parameters, neither fixed lanes nor an admitted set list, and no radius
/// assignment for any of its groups.
pub(crate) fn fillet_set_list_is_unadmitted(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    radius_groups: &[DesignFilletRadiusGroup],
    owners: &[DesignParameterOwner],
) -> bool {
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Fillet)
        || scope.fixed_fillet_parameters().is_some()
        || scope.fillet_sets().is_some()
    {
        return false;
    }
    let stream = native_stream(&scope.id);
    owners
        .iter()
        .any(|owner| native_stream(owner.id()) == stream && scope.names_owner(owner))
        && groups.iter().any(|group| {
            native_stream(&group.id) == stream && group.scope_record_index == scope.record_index
        })
        && radius_groups.iter().all(|group| {
            native_stream(&group.id) != stream || group.scope_record_index != scope.record_index
        })
}

pub(crate) fn fillet_set_list_losses(
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    radius_groups: &[DesignFilletRadiusGroup],
    owners: &[DesignParameterOwner],
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    scopes
        .iter()
        .filter(|scope| fillet_set_list_is_unadmitted(scope, groups, radius_groups, owners))
        .map(|scope| {
            crate::loss::F3dLossCode::FeatureFilletSetListNative.note(format!(
                "scope {} retains its operand groups and owned parameters because its fillet set list is not an admitted form",
                scope.id
            ))
        })
        .collect()
}

pub(crate) fn fixed_fillet_assignment_losses(
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    scopes.iter().filter(|scope| fixed_fillet_assignment_is_unresolved(scope, groups))
        .map(|scope| crate::loss::F3dLossCode::FeatureFixedFilletAssignmentNative.note(format!(
            "scope {} retains its fixed Fillet scalar lanes and operand groups because their radius assignment is unresolved", scope.id
        )))
        .collect()
}

pub(crate) fn decode_fillet_radius_groups(
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    owners: &[DesignParameterOwner],
    parameters: &[DesignParameter],
) -> Vec<DesignFilletRadiusGroup> {
    let mut parameter_index = HashMap::new();
    for parameter in parameters {
        let Some(stream) = native_stream(&parameter.id) else {
            continue;
        };
        let key = (stream, parameter.record_index);
        parameter_index.insert(key, parameter);
    }
    let mut out = Vec::new();
    for scope in scopes
        .iter()
        .filter(|scope| design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Fillet))
    {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let mut scope_groups = groups
            .iter()
            .filter(|group| {
                native_stream(&group.id) == Some(stream)
                    && group.scope_record_index == scope.record_index
            })
            .collect::<Vec<_>>();
        if scope.fillet_sets().is_some_and(|sets| {
            sets.projects_short_sets(
                scope_groups
                    .iter()
                    .filter(|group| !group.members().is_empty())
                    .map(|group| group.record_index),
            )
        }) {
            continue;
        }
        scope_groups.sort_by_key(|group| group.scope_reference_ordinal);
        let mut owned_parameters = owners
            .iter()
            .filter(|owner| native_stream(owner.id()) == Some(stream) && scope.names_owner(owner))
            .filter_map(|owner| {
                Some((
                    owner.local_ordinal(),
                    *parameter_index.get(&(stream, owner.parameter_record_index()))?,
                ))
            })
            .collect::<Vec<_>>();
        owned_parameters.sort_by_key(|(ordinal, _)| *ordinal);
        let radii = owned_parameters
            .iter()
            .filter_map(|(_, parameter)| parameter.is_fillet_radius().then_some(*parameter))
            .collect::<Vec<_>>();
        let weights = owned_parameters
            .iter()
            .filter_map(|(_, parameter)| {
                (parameter.source_kind() == "TangencyWeight").then_some(*parameter)
            })
            .collect::<Vec<_>>();
        if owned_parameters.len() == radii.len() + weights.len()
            && scope_groups.len() == radii.len()
            && (weights.is_empty() || weights.len() == scope_groups.len())
        {
            for (ordinal, (group, radius)) in scope_groups.into_iter().zip(radii).enumerate() {
                let Ok(group_ordinal) = u32::try_from(ordinal) else {
                    continue;
                };
                push_fillet_radius_group(
                    &mut out,
                    stream,
                    group_ordinal,
                    group,
                    DesignFilletRadiusLaw::Constant {
                        radius_parameter_record_index: radius.record_index,
                    },
                    weights.get(ordinal).map(|parameter| parameter.record_index),
                );
            }
            continue;
        }
        // Several edge sets with face selections: the edge groups pair with
        // the radii in scope-reference order. A face group states no radius
        // of its own and receives no assignment.
        let edge_groups = scope_groups
            .iter()
            .copied()
            .filter(|group| group.role() == DesignOperandRole::BODIES_B)
            .collect::<Vec<_>>();
        let face_group_count = scope_groups
            .iter()
            .filter(|group| group.role() == DesignOperandRole::ROLE_0X10)
            .count();
        if edge_groups.len() > 1
            && face_group_count > 0
            && edge_groups.len() + face_group_count == scope_groups.len()
            && owned_parameters.len() == radii.len() + weights.len()
            && edge_groups.len() == radii.len()
            && (weights.is_empty() || weights.len() == edge_groups.len())
        {
            for (ordinal, (group, radius)) in edge_groups.into_iter().zip(radii).enumerate() {
                let Ok(group_ordinal) = u32::try_from(ordinal) else {
                    continue;
                };
                push_fillet_radius_group(
                    &mut out,
                    stream,
                    group_ordinal,
                    group,
                    DesignFilletRadiusLaw::Constant {
                        radius_parameter_record_index: radius.record_index,
                    },
                    weights.get(ordinal).map(|parameter| parameter.record_index),
                );
            }
            continue;
        }
        if let (Some(pair), [radius]) =
            (fillet_edge_and_face_groups(&scope_groups), radii.as_slice())
        {
            if owned_parameters.len() == 1 + weights.len() && weights.len() <= 1 {
                for (ordinal, group) in (0_u32..).zip(pair) {
                    push_fillet_radius_group(
                        &mut out,
                        stream,
                        ordinal,
                        group,
                        DesignFilletRadiusLaw::Constant {
                            radius_parameter_record_index: radius.record_index,
                        },
                        weights.first().map(|parameter| parameter.record_index),
                    );
                }
                continue;
            }
        }
        if scope_groups.len() > 1 {
            if let Some(assignments) =
                super::fillet_variable_groups::assign(stream, &scope_groups, &owned_parameters)
            {
                out.extend(assignments);
            }
            continue;
        }
        let [group] = scope_groups.as_slice() else {
            continue;
        };
        let chord_lengths = owned_parameters
            .iter()
            .filter_map(|(_, parameter)| {
                (parameter.source_kind() == "ChordLen").then_some(parameter.record_index)
            })
            .collect::<Vec<_>>();
        // TangencyWeight is optional for the chordal law; older records carry
        // only the required ChordLen input.
        if (weights.is_empty() && owned_parameters.len() == 1)
            || (weights.len() == 1 && owned_parameters.len() == 2)
        {
            let [chord_length] = chord_lengths.as_slice() else {
                continue;
            };
            push_fillet_radius_group(
                &mut out,
                stream,
                0,
                group,
                DesignFilletRadiusLaw::Chordal {
                    chord_length_parameter_record_index: *chord_length,
                },
                weights.first().map(|parameter| parameter.record_index),
            );
            continue;
        }
        let asymmetric_offsets = |kind: &str| {
            owned_parameters
                .iter()
                .filter_map(|(_, parameter)| {
                    (parameter.source_kind() == kind).then_some(parameter.record_index)
                })
                .collect::<Vec<_>>()
        };
        let (offset_one, offset_two) = (
            asymmetric_offsets("EdgeOffset1"),
            asymmetric_offsets("EdgeOffset2"),
        );
        if owned_parameters.len() == 3 {
            if let ([offset_one], [offset_two], [weight]) = (
                offset_one.as_slice(),
                offset_two.as_slice(),
                weights.as_slice(),
            ) {
                push_fillet_radius_group(
                    &mut out,
                    stream,
                    0,
                    group,
                    DesignFilletRadiusLaw::Asymmetric {
                        offset_one_parameter_record_index: *offset_one,
                        offset_two_parameter_record_index: *offset_two,
                    },
                    Some(weight.record_index),
                );
                continue;
            }
        }
        let records = |kind: &str| {
            owned_parameters
                .iter()
                .filter_map(|(_, parameter)| {
                    (parameter.source_kind() == kind).then_some(parameter.record_index)
                })
                .collect::<Vec<_>>()
        };
        let (start, end, middle_radii, middle_parameters) = (
            records("StartRadius"),
            records("EndRadius"),
            records("MidRadius"),
            records("MidParams"),
        );
        let ([start], [end]) = (start.as_slice(), end.as_slice()) else {
            continue;
        };
        // TangencyWeight is optional for the variable-radius law. Older
        // records carry only the endpoint and midpoint radius parameters.
        let variable_parameter_count =
            2 + middle_radii.len() + middle_parameters.len() + weights.len();
        if middle_radii.len() != middle_parameters.len()
            || weights.len() > 1
            || owned_parameters.len() != variable_parameter_count
        {
            continue;
        }
        let middle = middle_radii
            .into_iter()
            .zip(middle_parameters)
            .map(|(radius_parameter_record_index, parameter_record_index)| {
                crate::records::topology::fillet::DesignFilletMidpoint {
                    radius_parameter_record_index,
                    parameter_record_index,
                }
            })
            .collect::<Vec<_>>();
        push_fillet_radius_group(
            &mut out,
            stream,
            0,
            group,
            DesignFilletRadiusLaw::Variable {
                start_radius_parameter_record_index: *start,
                end_radius_parameter_record_index: *end,
                middle,
            },
            weights.first().map(|parameter| parameter.record_index),
        );
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Remove fixed Fillet interpretations of frames that are indexed parameter owners.
pub(crate) fn disambiguate_fixed_fillet_parameters(
    scopes: &mut [DesignParameterScope],
    owners: &[DesignParameterOwner],
) {
    for scope in scopes {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        if owners
            .iter()
            .any(|owner| native_stream(owner.id()) == Some(stream) && scope.names_owner(owner))
        {
            if let crate::records::feature::scope::DesignScopePayloadMut::Fillet(slot) =
                scope.payload_mut()
            {
                slot.fixed = None;
            }
        }
    }
}
