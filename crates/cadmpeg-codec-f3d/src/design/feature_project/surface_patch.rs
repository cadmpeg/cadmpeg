// SPDX-License-Identifier: Apache-2.0

use super::{loft_path_from_edge_selection, resolved_loft_path};
use crate::design::edge_resolve::{
    resolved_edge_group, resolved_surface_patch_edge_group, EdgeGroupTransition,
};
use crate::ids::native_stream;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::edge_identity::{DesignEdgeIdentityOperand, DesignEdgeOperand};
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

pub(super) fn project_surface_patch(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    edge_operands: &[DesignEdgeOperand],
    edge_identity_operands: &[DesignEdgeIdentityOperand],
    face_boundaries: &[crate::records::topology::historical_context::AsmFaceBoundary],
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use crate::records::feature::surface_ops::DesignSurfacePatchNormalState;
    use cadmpeg_ir::features::{
        FaceSelection, FeatureDefinition, FeatureOperation, SurfaceBoundary,
    };

    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::SurfacePatch {
        return Ok(None);
    }
    let reverse_normal = match scope.surface_patch_normal() {
        None => None,
        Some(DesignSurfacePatchNormalState::Explicit { reversed }) => Some(reversed.value),
        Some(DesignSurfacePatchNormalState::Unsupported) => return Ok(None),
    };
    let Some(stream) = native_stream(&scope.id) else {
        return Ok(None);
    };
    let mut groups = Vec::new();
    for group in construction_groups.iter().filter(|group| {
        native_stream(&group.id) == Some(stream) && group.scope_record_index == scope.record_index
    }) {
        groups.push(group);
    }
    groups.sort_by(|left, right| {
        left.scope_reference_ordinal
            .cmp(&right.scope_reference_ordinal)
    });

    let Some(grouped_path_frame_length) = u64::try_from(scope.reference_members().len())
        .ok()
        .and_then(|count| count.checked_mul(11))
        .and_then(|count| count.checked_add(277))
    else {
        return Ok(None);
    };
    if scope.frame_length() == grouped_path_frame_length
        && groups
            .first()
            .is_some_and(|group| group.role() == DesignOperandRole::BODIES_A)
    {
        let [group] = groups.as_slice() else {
            return Ok(None);
        };
        if scope.reference_members().len() < 3 {
            return Ok(None);
        }
        let Some(first_reference) = scope.reference_members().values().next() else {
            return Ok(None);
        };
        let Some(edge_references) = scope
            .reference_members()
            .values_in(1..scope.reference_members().len() - 1)
        else {
            return Ok(None);
        };
        if group.scope_reference_ordinal != 0
            || group.record_index != *first_reference
            || group.role() != DesignOperandRole::BODIES_A
            || group.members().is_empty()
            || !group
                .members()
                .iter()
                .map(|member| member.value)
                .eq(edge_references.copied())
        {
            return Ok(None);
        }
        return Ok(Some(FeatureDefinition::Operation(
            FeatureOperation::FilledSurface {
                boundary: SurfaceBoundary::Path(resolved_surface_patch_path(
                    std::slice::from_ref(group),
                    construction_groups,
                    edge_operands,
                    edge_identity_operands,
                    scope,
                    SurfacePatchRecipe::Grouped(face_boundaries),
                    ctx,
                )?),
                support_faces: FaceSelection::Faces(Vec::new()),
                continuity: cadmpeg_ir::features::FilledSurfaceContinuityState::uniform(
                    cadmpeg_ir::features::SurfaceContinuity::Contact,
                ),
                merge_result: Some(false),
                interior_constraints: Vec::new(),
                reverse_normal,
            },
        )));
    }

    let (boundary_groups, interior_groups, unnamed_groups) =
        if let Some(partition) = surface_patch_interior_groups(ctx, scope, &groups)? {
            partition
        } else {
            match groups.first().map(|group| group.role()) {
                Some(DesignOperandRole::PROFILE) => {
                    if !surface_patch_sketch_profiles(scope, &groups) {
                        return Ok(None);
                    }
                }
                Some(DesignOperandRole::BODIES_A) => {
                    if !surface_patch_fixed_paths(ctx, scope, &groups)? {
                        return Ok(None);
                    }
                }
                _ => return Ok(None),
            }
            (groups, Vec::new(), 0)
        };
    let boundary = if let [boundary] = boundary_groups.as_slice() {
        resolved_loft_path(
            boundary,
            construction_groups,
            edge_operands,
            edge_identity_operands,
            scope,
            ctx,
        )?
    } else {
        resolved_surface_patch_path(
            &boundary_groups,
            construction_groups,
            edge_operands,
            edge_identity_operands,
            scope,
            SurfacePatchRecipe::Direct,
            ctx,
        )?
    };
    let mut interior_constraints = Vec::new();
    for group in &interior_groups {
        interior_constraints.push(resolved_loft_path(
            group,
            construction_groups,
            edge_operands,
            edge_identity_operands,
            scope,
            ctx,
        )?);
    }
    if !interior_constraints.is_empty() {
        let references = scope.reference_members().values();
        if !references
            .clone()
            .zip(references.skip(1))
            .all(|(left, right)| left < right)
        {
            losses.push(
                crate::loss::F3dLossCode::SurfacePatchUnorderedReferenceRunsAssumed.note(format!(
                    "SurfacePatch {} has an ordered reference table outside record-index order; its group runs are assumed to keep their consecutive members.",
                    scope.id
                )),
            );
        }
        losses.push(
            crate::loss::F3dLossCode::SurfacePatchInteriorConstraintAssumed.note(format!(
                "SurfacePatch {} projects its {} interior groups as interior constraints that the patch passes through.",
                scope.id,
                interior_constraints.len()
            )),
        );
    }
    if unnamed_groups > 0 {
        losses.push(crate::loss::F3dLossCode::SurfacePatchUnnamedGroupsAssumedInactive.note(
            format!("SurfacePatch {} has {unnamed_groups} interior-role groups outside its interior list; they are treated as inactive.", scope.id),
        ));
    }
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::FilledSurface {
            boundary: SurfaceBoundary::Path(boundary),
            support_faces: FaceSelection::Faces(Vec::new()),
            continuity: cadmpeg_ir::features::NonEmptyMembers::try_from(
                surface_patch_boundary_conditions(scope),
            )
            .map_or_else(
                |_| cadmpeg_ir::features::FilledSurfaceContinuityState::unresolved(),
                cadmpeg_ir::features::FilledSurfaceContinuityState::per_boundary,
            ),
            merge_result: Some(false),
            interior_constraints,
            reverse_normal,
        },
    )))
}

type SurfacePatchPartition<'a> = (
    Vec<&'a DesignConstructionOperandGroup>,
    Vec<&'a DesignConstructionOperandGroup>,
    usize,
);

fn surface_patch_interior_groups<'a>(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    groups: &[&'a DesignConstructionOperandGroup],
) -> Result<Option<SurfacePatchPartition<'a>>, CodecError> {
    let Some(selection) = scope.surface_patch_selection() else {
        return Ok(None);
    };
    let settings = scope.surface_patch_boundaries();
    if selection.interior_groups.is_empty()
        || settings.is_empty()
        || !settings
            .iter()
            .map(|setting| setting.record_index)
            .eq(selection.settings.iter().copied())
    {
        return Ok(None);
    }
    let references = scope.reference_members();
    let mut occupied = ctx.alloc_filled(
        references.len(),
        false,
        "f3d surface-patch reference occupancy",
    )?;
    let mut claim = |ordinal: Option<usize>, record: u32| {
        let Some(ordinal) = ordinal else {
            return false;
        };
        references.values().nth(ordinal) == Some(&record)
            && occupied
                .get_mut(ordinal)
                .is_some_and(|slot| !std::mem::replace(slot, true))
    };
    let group_named = |record: u32, role: DesignOperandRole| {
        groups
            .iter()
            .copied()
            .find(|group| group.record_index == record && group.role() == role)
    };
    let mut boundary_groups = Vec::new();
    for setting in settings {
        let Some(group) = group_named(setting.model_reference, DesignOperandRole::BODIES_A) else {
            return Ok(None);
        };
        let [member] = group.members() else {
            return Ok(None);
        };
        let ordinal = usize::try_from(group.scope_reference_ordinal).ok();
        let settings_ordinal = ordinal.and_then(|ordinal| ordinal.checked_add(2));
        if usize::try_from(setting.scope_reference_ordinal).ok() != settings_ordinal
            || !claim(ordinal, group.record_index)
            || !claim(
                ordinal.and_then(|ordinal| ordinal.checked_add(1)),
                member.value,
            )
            || !claim(settings_ordinal, setting.record_index)
        {
            return Ok(None);
        }
        boundary_groups.push(group);
    }
    let mut interior_groups = Vec::new();
    for record in &selection.interior_groups {
        let Some(group) = group_named(*record, DesignOperandRole::ROLE_0X21) else {
            return Ok(None);
        };
        let ordinal = usize::try_from(group.scope_reference_ordinal).ok();
        if group.members().is_empty() || !claim(ordinal, group.record_index) {
            return Ok(None);
        }
        for (offset, member) in (1..).zip(group.members()) {
            if !claim(
                ordinal.and_then(|ordinal| ordinal.checked_add(offset)),
                member.value,
            ) {
                return Ok(None);
            }
        }
        interior_groups.push(group);
    }
    let mut unnamed_groups = 0;
    for group in groups.iter().copied().filter(|group| {
        group.role() == DesignOperandRole::ROLE_0X21
            && !selection.interior_groups.contains(&group.record_index)
    }) {
        let ordinal = usize::try_from(group.scope_reference_ordinal).ok();
        if group.members().is_empty() || !claim(ordinal, group.record_index) {
            return Ok(None);
        }
        for (offset, member) in (1..).zip(group.members()) {
            if !claim(
                ordinal.and_then(|ordinal| ordinal.checked_add(offset)),
                member.value,
            ) {
                return Ok(None);
            }
        }
        unnamed_groups += 1;
    }
    let tool_ordinal = references
        .values()
        .position(|record| *record == selection.tool_body);
    if !claim(tool_ordinal, selection.tool_body) || occupied.iter().any(|slot| !slot) {
        return Ok(None);
    }
    Ok(Some((boundary_groups, interior_groups, unnamed_groups)))
}

#[derive(Clone, Copy)]
enum SurfacePatchRecipe<'a> {
    Grouped(&'a [crate::records::topology::historical_context::AsmFaceBoundary]),
    Direct,
}

fn resolved_surface_patch_path(
    groups: &[&DesignConstructionOperandGroup],
    all_groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
    identity_operands: &[DesignEdgeIdentityOperand],
    scope: &DesignParameterScope,
    recipe: SurfacePatchRecipe<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<cadmpeg_ir::features::PathRef, CodecError> {
    use cadmpeg_ir::features::PathRef;

    let mut paths = Vec::new();
    for group in groups {
        let selection = if let SurfacePatchRecipe::Grouped(face_boundaries) = recipe {
            resolved_surface_patch_edge_group(
                group,
                all_groups,
                operands,
                identity_operands,
                face_boundaries,
                EdgeGroupTransition {
                    previous_state_id: scope.previous_history_state_id(),
                    feature_id: &crate::ids::neutral_feature_id(scope),
                },
                ctx,
            )
        } else {
            resolved_edge_group(
                group,
                all_groups,
                operands,
                identity_operands,
                scope.previous_history_state_id(),
                &crate::ids::neutral_feature_id(scope),
                ctx,
            )
        }?;
        let path = loft_path_from_edge_selection(&group.id, selection);
        paths.push(path);
    }
    if matches!(recipe, SurfacePatchRecipe::Grouped(_)) && paths.len() == 1 {
        if let Some(path) = paths.pop() {
            return Ok(path);
        }
    }
    if paths.is_empty() {
        return Ok(PathRef::Native(scope.id.clone()));
    }
    if let Some(state) = paths.iter().find_map(|path| {
        let PathRef::HistoricalEdges { state, .. } = path else {
            return None;
        };
        Some(state)
    }) {
        if paths.iter().all(|path| {
            matches!(path,
            PathRef::HistoricalEdges { state: candidate, .. } if candidate == state)
        }) {
            let state = state.clone();
            let mut edges = Vec::new();
            for path in &paths {
                if let PathRef::HistoricalEdges {
                    edges: group_edges, ..
                } = path
                {
                    edges.extend_from_slice(group_edges);
                }
            }
            let members = cadmpeg_ir::features::SelectionMembers::try_from(edges);
            let native = scope.id.clone();
            return Ok(match members {
                Ok(edges) => PathRef::HistoricalEdges {
                    state,
                    edges,
                    native: cadmpeg_core::text::NonBlankString::new(native)
                        .ok_or_else(|| CodecError::malformed("surface patch path is blank"))?,
                },
                Err(_) => PathRef::Native(native),
            });
        }
    }
    if paths.iter().all(|path| matches!(path, PathRef::Edges(_))) {
        let mut edges = Vec::new();
        for path in &paths {
            if let PathRef::Edges(group_edges) = path {
                edges.extend_from_slice(group_edges);
            }
        }
        return Ok(PathRef::Edges(edges));
    }
    Ok(PathRef::Native(scope.id.clone()))
}

/// Project each boundary condition and its resolved continuity weight.
///
/// An unknown continuity or unsupported active weight leaves the complete
/// condition vector unresolved; native settings retain their source values.
pub(super) fn surface_patch_boundary_conditions(
    scope: &DesignParameterScope,
) -> Vec<cadmpeg_ir::features::FilledSurfaceBoundaryCondition> {
    use crate::records::feature::surface_ops::DesignPatchContinuity;
    use cadmpeg_ir::features::FilledSurfaceBoundaryCondition;

    const DEFAULT_WEIGHT: f64 = 0.5;
    const DEFAULT_WEIGHT_SENTINEL: f64 = -1.0;

    let mut conditions = Vec::new();
    for boundary in scope.surface_patch_boundaries() {
        let condition = match boundary.continuity {
            DesignPatchContinuity::Connected => FilledSurfaceBoundaryCondition::Contact {},
            DesignPatchContinuity::Tangent | DesignPatchContinuity::Curvature => {
                let raw = boundary.scale.get();
                let effective = if raw == DEFAULT_WEIGHT_SENTINEL {
                    DEFAULT_WEIGHT
                } else {
                    raw
                };
                let Some(weight) = cadmpeg_ir::scalar::PositiveReal::new(effective) else {
                    return Vec::new();
                };
                if boundary.continuity == DesignPatchContinuity::Tangent {
                    FilledSurfaceBoundaryCondition::Tangent {
                        weight: Some(weight),
                    }
                } else {
                    FilledSurfaceBoundaryCondition::Curvature {
                        weight: Some(weight),
                    }
                }
            }
            DesignPatchContinuity::Unknown(_) => return Vec::new(),
        };
        conditions.push(condition);
    }
    conditions
}

fn surface_patch_sketch_profiles(
    scope: &DesignParameterScope,
    groups: &[&DesignConstructionOperandGroup],
) -> bool {
    let references = scope.reference_members();
    let Some(boundary_references) = references.len().checked_sub(1) else {
        return false;
    };
    if boundary_references == 0
        || boundary_references % 2 != 0
        || groups.len() != boundary_references / 2
        || !scope.surface_patch_boundaries().is_empty()
    {
        return false;
    }
    let Some(tool_body) = references.values().last() else {
        return false;
    };
    groups.iter().enumerate().all(|(index, group)| {
        let ordinal = index * 2;
        usize::try_from(group.scope_reference_ordinal).ok() == Some(ordinal)
            && group.role() == DesignOperandRole::PROFILE
            && group.record_index != *tool_body
            && group
                .members()
                .iter()
                .all(|member| member.value != *tool_body)
            && references.values().nth(ordinal) == Some(&group.record_index)
            && group
                .members()
                .iter()
                .map(|member| member.value)
                .eq(references.values().nth(ordinal + 1).copied())
    })
}

fn surface_patch_fixed_paths(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    groups: &[&DesignConstructionOperandGroup],
) -> Result<bool, CodecError> {
    let Some(reference_count) = scope.reference_members().len().checked_sub(1) else {
        return Ok(false);
    };
    let boundary_count = reference_count / 3;
    if boundary_count == 0
        || reference_count % 3 != 0
        || groups.len() != boundary_count
        || scope.surface_patch_boundaries().len() != boundary_count
    {
        return Ok(false);
    }
    let mut occupied = ctx.alloc_filled(
        scope.reference_members().len(),
        false,
        "f3d surface-patch reference occupancy",
    )?;
    for boundary in groups {
        let Ok(group_ordinal) = usize::try_from(boundary.scope_reference_ordinal) else {
            return Ok(false);
        };
        let Some(member_ordinal) = group_ordinal.checked_add(1) else {
            return Ok(false);
        };
        let Some(settings_ordinal) = group_ordinal.checked_add(2) else {
            return Ok(false);
        };
        let Some(group_reference) = scope.reference_members().values().nth(group_ordinal) else {
            return Ok(false);
        };
        if settings_ordinal >= scope.reference_members().len()
            || boundary.record_index != *group_reference
            || boundary.role() != DesignOperandRole::BODIES_A
            || !boundary
                .members()
                .iter()
                .map(|member| member.value)
                .eq(scope
                    .reference_members()
                    .values()
                    .nth(member_ordinal)
                    .into_iter()
                    .copied())
            || occupied[group_ordinal]
            || occupied[member_ordinal]
            || occupied[settings_ordinal]
        {
            return Ok(false);
        }
        let Some(settings) = scope.surface_patch_boundaries().iter().find(|settings| {
            usize::try_from(settings.scope_reference_ordinal).ok() == Some(settings_ordinal)
        }) else {
            return Ok(false);
        };
        let Some(settings_reference) = scope.reference_members().values().nth(settings_ordinal)
        else {
            return Ok(false);
        };
        if settings.record_index != *settings_reference
            || settings.model_reference != boundary.record_index
        {
            return Ok(false);
        }
        occupied[group_ordinal] = true;
        occupied[member_ordinal] = true;
        occupied[settings_ordinal] = true;
    }
    let mut unoccupied = Vec::new();
    for (ordinal, occupied) in occupied.iter().enumerate() {
        if !occupied {
            unoccupied.push(ordinal);
        }
    }
    let endpoint_unoccupied = unoccupied.as_slice() == [0]
        || scope
            .reference_members()
            .len()
            .checked_sub(1)
            .is_some_and(|last| unoccupied.as_slice() == [last]);
    Ok(endpoint_unoccupied)
}
