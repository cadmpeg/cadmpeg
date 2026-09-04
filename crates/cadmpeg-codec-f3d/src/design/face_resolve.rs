// SPDX-License-Identifier: Apache-2.0
//! Resolve face-selection operands and extrude start planes.

use crate::design::dimensions::{planar_point, sketch_normal_sign};
use crate::design::edge_resolve::feature_input_topology_id;
use crate::design::feature_project::design_angle_unit;
use crate::ids::{self, native_stream, neutral_feature_id};
use crate::records::{
    DesignBodyRecipeOperand, DesignConstructionOperandGroup, DesignEdgeOperand,
    DesignExtrudeExtent, DesignExtrudeFaceRole, DesignExtrudePrologue, DesignFaceOperand,
    DesignParameter, DesignParameterScope, DesignSketchPlacement, SketchCurveGeometry,
    SketchCurveIdentity, SketchPoint,
};
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::{HashMap, HashSet};

const EPS_FACE_RESOLVE_SKETCH_CURVE_IS_SPATIAL_E9: f64 = 1.0e-9;

/// Admit the legacy reference-aware face-target frame that omits its zero
/// `Side1Offset` owner and parameter.
pub(crate) fn extrude_omits_zero_side_one_offset(
    scope: &DesignParameterScope,
    prologue: &DesignExtrudePrologue,
    side_one_offset_count: usize,
) -> bool {
    side_one_offset_count == 0
        && scope.class_tag == "330"
        && scope.paired_class_tag == "258"
        && scope.frame_length == 476
        && matches!(
            prologue,
            DesignExtrudePrologue::ReferenceAware {
                extent: DesignExtrudeExtent::OneSidedToFace,
                ..
            }
        )
}

pub(crate) fn resolved_face_group(
    group: &DesignConstructionOperandGroup,
    operands: &[DesignFaceOperand],
) -> Option<cadmpeg_ir::features::FaceSelection> {
    let stream = native_stream(&group.id)?;
    let mut faces = Vec::with_capacity(group.members.len());
    for record_index in &group.members {
        let mut matches = operands.iter().filter(|operand| {
            native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == group.scope_record_index
                && operand.record_index == *record_index
        });
        let operand = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        if operand.resolved_active_face.is_none()
            && operand.resolved_face_slots.is_empty()
            && operand.alternate_selector_candidate_faces.is_empty()
            && (!operand.preceding_candidate_faces.is_empty()
                || !operand.changed_candidate_faces.is_empty()
                || !operand.historical_support_contexts.is_empty())
        {
            return None;
        }
        let operand_faces = resolved_face_operand(operand)?;
        for face in operand_faces {
            if !faces.contains(&face) {
                faces.push(face);
            }
        }
    }
    (!faces.is_empty()).then(|| cadmpeg_ir::features::FaceSelection::Resolved {
        faces,
        native: group.id.clone(),
    })
}

/// Resolve a bounded-face group whose active lane is fully named by its own
/// recipe selector, but whose generation does not emit historical slots.
///
/// This is intentionally separate from `resolved_face_group`: a multi-face
/// bounded recipe normally needs a slot or history proof. The legacy Draft
/// form admitted here has a complete structured recipe, no alternate or
/// historical candidates, and no candidate outside the recipe's own
/// selector. Those invariants make the active lane the selected face set.
pub(crate) fn resolved_explicit_bounded_face_group(
    group: &DesignConstructionOperandGroup,
    operands: &[DesignFaceOperand],
) -> Option<cadmpeg_ir::features::FaceSelection> {
    let stream = native_stream(&group.id)?;
    let mut faces = Vec::with_capacity(group.members.len());
    for record_index in &group.members {
        let mut matches = operands.iter().filter(|operand| {
            native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == group.scope_record_index
                && operand.record_index == *record_index
        });
        let operand = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        let candidate_faces = if operand.resolved_face_slots.is_empty() {
            explicit_bounded_face_candidates(operand)?
        } else {
            resolved_face_operand(operand)?
        };
        for face in candidate_faces {
            if !faces.contains(&face) {
                faces.push(face);
            }
        }
    }
    (!faces.is_empty()).then(|| cadmpeg_ir::features::FaceSelection::Resolved {
        faces,
        native: group.id.clone(),
    })
}

/// Resolve a direct scope face selection when every direct operand proves the
/// same current face set through stable input-topology slots.
pub(crate) fn resolved_direct_face_selection(
    scope: &DesignParameterScope,
    operands: &[DesignFaceOperand],
) -> Option<cadmpeg_ir::features::FaceSelection> {
    use cadmpeg_ir::features::FaceSelection;

    let stream = native_stream(&scope.id)?;
    let mut matching = operands
        .iter()
        .filter(|operand| {
            native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == scope.record_index
                && operand.group_record_index.is_none()
                && operand.group_member_ordinal.is_none()
                && operand.recipe_kind == crate::records::ConstructionRecipeKind::BoundedFace
                && usize::try_from(operand.scope_reference_ordinal)
                    .ok()
                    .and_then(|ordinal| scope.reference_members.get(ordinal))
                    == Some(&operand.record_index)
        })
        .collect::<Vec<_>>();
    matching.sort_by_key(|operand| operand.scope_reference_ordinal);
    if matching.is_empty()
        || matching
            .iter()
            .any(|operand| operand.resolved_face_slots.is_empty())
    {
        return None;
    }
    let mut faces = resolved_face_operand(matching[0])?;
    faces.sort_by(|left, right| left.0.cmp(&right.0));
    if faces.is_empty() {
        return None;
    }
    for operand in &matching[1..] {
        let mut candidate = resolved_face_operand(operand)?;
        candidate.sort_by(|left, right| left.0.cmp(&right.0));
        if candidate != faces {
            return None;
        }
    }
    Some(FaceSelection::Resolved {
        faces,
        native: scope.id.clone(),
    })
}

/// Resolve a face operand whose exact preceding topology proves one face.
///
/// This path is for single-face operands whose recipe carries the selected face
/// in its persistent-reference lane but whose active candidate lane is not a
/// current-face slot. The caller must still admit the operand's exact recipe
/// form; this helper only applies the unique historical-face proof.
pub(crate) fn resolved_historical_face_operand(
    scope: &DesignParameterScope,
    operand: &DesignFaceOperand,
) -> Option<cadmpeg_ir::features::FaceSelection> {
    let previous_state_id = scope.previous_history_state_id?;
    let face_slot = resolve_face_operand_history_candidates(operand)?;
    historical_face_selection_with_native(
        scope,
        previous_state_id,
        vec![face_slot],
        operand.id.clone(),
    )
}

/// Resolve the complete input-state body boundaries selected by a body-recipe
/// group. Persistent-reference candidate faces identify each body; they do
/// not define a partial target boundary.
pub(crate) fn resolved_body_recipe_selection(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    operands: &[DesignBodyRecipeOperand],
) -> Option<cadmpeg_ir::features::FaceSelection> {
    if group.scope_record_index != scope.record_index
        || group.extrude_role.is_some()
        || group.extrude_face_role.is_some()
        || group.members.is_empty()
    {
        return None;
    }
    let stream = native_stream(&group.id)?;
    if native_stream(&scope.id) != Some(stream) {
        return None;
    }
    let mut faces = Vec::new();
    let mut state_id = None;
    let mut member_records = HashSet::new();
    for (ordinal, record_index) in group.members.iter().enumerate() {
        if !member_records.insert(*record_index) {
            return None;
        }
        let ordinal = u32::try_from(ordinal).ok()?;
        let mut matches = operands.iter().filter(|operand| {
            native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == group.scope_record_index
                && operand.owner.group() == Some((group.record_index, ordinal))
                && operand.record_index == *record_index
        });
        let operand = matches.next()?;
        if matches.next().is_some()
            || operand.references.is_empty()
            || operand.resolved_body_slot.is_none()
            || operand.resolved_body_face_slots.is_empty()
        {
            return None;
        }
        let operand_state_id = operand.resolved_body_state_id?;
        match state_id {
            None => state_id = Some(operand_state_id),
            Some(expected) if expected == operand_state_id => {}
            Some(_) => return None,
        }
        for face in &operand.resolved_body_face_slots {
            if !faces.contains(face) {
                faces.push(*face);
            }
        }
    }
    historical_face_selection_in_state(scope, group, state_id?, faces)
}

/// Resolve the complete input-state body boundaries selected by an Extrude
/// target-shape group. Persistent-reference candidate faces identify each
/// body; they do not define a partial target boundary.
pub(crate) fn resolved_body_recipe_shape(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    operands: &[DesignBodyRecipeOperand],
) -> Option<cadmpeg_ir::features::FaceSelection> {
    if crate::design::design_feature_family(&scope.kind)
        != Some(crate::design::DesignFeatureFamily::Extrude)
        || group.role != 0x0000_0005_0000_0000
    {
        return None;
    }
    resolved_body_recipe_selection(scope, group, operands)
}

pub(crate) fn resolved_profile_face_group(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    operands: &[DesignFaceOperand],
) -> Option<cadmpeg_ir::features::ProfileRef> {
    use cadmpeg_ir::features::ProfileRef;

    let selection = resolved_historical_face_group(scope, group, operands)?;
    let cadmpeg_ir::features::FaceSelection::Historical {
        state,
        faces,
        native,
    } = selection
    else {
        return None;
    };
    Some(ProfileRef::HistoricalFaces {
        state,
        faces,
        native: vec![native],
    })
}

/// Return the top-level profile groups of one Extrude operand hierarchy.
///
/// A profile group named by another profile group's member table is a child
/// selection, not an additional profile consumed by the Extrude. The complete
/// hierarchy must be acyclic, and each child can have exactly one parent.
pub(crate) fn extrude_profile_group_roots<'a>(
    scope: &DesignParameterScope,
    groups: &'a [DesignConstructionOperandGroup],
) -> Option<Vec<&'a DesignConstructionOperandGroup>> {
    use crate::records::DesignExtrudeOperandRole;

    let stream = native_stream(&scope.id)?;
    let mut profile_groups = groups
        .iter()
        .filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
                && group.extrude_role == Some(DesignExtrudeOperandRole::Profile)
        })
        .collect::<Vec<_>>();
    profile_groups.sort_by_key(|group| group.scope_reference_ordinal);
    if profile_groups.windows(2).any(|groups| {
        groups[0].scope_reference_ordinal == groups[1].scope_reference_ordinal
            || groups[0].record_index == groups[1].record_index
    }) {
        return None;
    }

    let groups_by_record = profile_groups
        .iter()
        .map(|group| (group.record_index, *group))
        .collect::<HashMap<_, _>>();
    if groups_by_record.len() != profile_groups.len() {
        return None;
    }
    let mut parent_by_child = HashMap::new();
    for parent in &profile_groups {
        for member in &parent.members {
            let Some(child) = groups_by_record.get(member) else {
                continue;
            };
            if child.scope_reference_ordinal <= parent.scope_reference_ordinal
                || parent_by_child
                    .insert(child.record_index, parent.record_index)
                    .is_some()
            {
                return None;
            }
        }
    }
    let roots = profile_groups
        .iter()
        .copied()
        .filter(|group| !parent_by_child.contains_key(&group.record_index))
        .collect::<Vec<_>>();
    if !profile_groups.is_empty() && roots.is_empty() {
        return None;
    }

    let mut visited = HashSet::new();
    if roots
        .iter()
        .any(|root| !visit_extrude_profile_group(root, &groups_by_record, &mut visited))
        || visited.len() != profile_groups.len()
    {
        return None;
    }
    Some(roots)
}

fn visit_extrude_profile_group(
    group: &DesignConstructionOperandGroup,
    groups_by_record: &HashMap<u32, &DesignConstructionOperandGroup>,
    visited: &mut HashSet<u32>,
) -> bool {
    visited.insert(group.record_index)
        && group.members.iter().all(|member| {
            groups_by_record
                .get(member)
                .is_none_or(|child| visit_extrude_profile_group(child, groups_by_record, visited))
        })
}

/// Resolve each member of an Extrude profile group to one exact leaf operand.
///
/// Direct members name face operands. A member can instead name a child
/// profile group when that complete child hierarchy contains exactly one leaf
/// operand. This preserves the parent member cardinality used by historical
/// selection proofs.
pub(crate) fn extrude_profile_group_operand_indices(
    root: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignFaceOperand],
) -> Option<Vec<usize>> {
    use crate::records::DesignExtrudeOperandRole;

    let stream = native_stream(&root.id)?;
    let profile_groups = groups
        .iter()
        .filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == root.scope_record_index
                && group.extrude_role == Some(DesignExtrudeOperandRole::Profile)
        })
        .collect::<Vec<_>>();
    let groups_by_record = profile_groups
        .iter()
        .map(|group| (group.record_index, *group))
        .collect::<HashMap<_, _>>();
    if groups_by_record.len() != profile_groups.len()
        || groups_by_record.get(&root.record_index).copied() != Some(root)
    {
        return None;
    }

    let mut visited_groups = HashSet::new();
    collect_extrude_profile_group_operands(
        root,
        stream,
        &groups_by_record,
        operands,
        &mut visited_groups,
    )
}

fn collect_extrude_profile_group_operands(
    group: &DesignConstructionOperandGroup,
    stream: &str,
    groups_by_record: &HashMap<u32, &DesignConstructionOperandGroup>,
    operands: &[DesignFaceOperand],
    visited_groups: &mut HashSet<u32>,
) -> Option<Vec<usize>> {
    if group.members.is_empty() || !visited_groups.insert(group.record_index) {
        return None;
    }
    let mut indices = Vec::with_capacity(group.members.len());
    for (ordinal, record_index) in group.members.iter().enumerate() {
        let ordinal = u32::try_from(ordinal).ok()?;
        let direct = operands
            .iter()
            .enumerate()
            .filter(|(_, operand)| {
                native_stream(&operand.id) == Some(stream)
                    && operand.scope_record_index == group.scope_record_index
                    && operand.group_record_index == Some(group.record_index)
                    && operand.group_member_ordinal == Some(ordinal)
                    && operand.record_index == *record_index
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let child = groups_by_record.get(record_index).copied();
        let index = match (direct.as_slice(), child) {
            ([index], None) => *index,
            ([], Some(child)) if child.scope_reference_ordinal > group.scope_reference_ordinal => {
                let child_indices = collect_extrude_profile_group_operands(
                    child,
                    stream,
                    groups_by_record,
                    operands,
                    visited_groups,
                )?;
                let [index] = child_indices.as_slice() else {
                    return None;
                };
                *index
            }
            _ => return None,
        };
        if indices.contains(&index) {
            return None;
        }
        indices.push(index);
    }
    Some(indices)
}

/// Whether every root member is one exact paired-reference face subgroup.
pub(crate) fn is_paired_extrude_profile_aggregate(
    root: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignFaceOperand],
) -> bool {
    use crate::records::{ConstructionRecipeKind, DesignExtrudeOperandRole};

    let Some(stream) = native_stream(&root.id) else {
        return false;
    };
    !root.members.is_empty()
        && root.members.iter().all(|record_index| {
            let mut children = groups.iter().filter(|group| {
                native_stream(&group.id) == Some(stream)
                    && group.scope_record_index == root.scope_record_index
                    && group.record_index == *record_index
                    && group.scope_reference_ordinal > root.scope_reference_ordinal
                    && group.extrude_role == Some(DesignExtrudeOperandRole::Profile)
            });
            let Some(child) = children.next() else {
                return false;
            };
            if children.next().is_some() {
                return false;
            }
            let [operand_record_index] = child.members.as_slice() else {
                return false;
            };
            let mut leaves = operands.iter().filter(|operand| {
                native_stream(&operand.id) == Some(stream)
                    && operand.scope_record_index == root.scope_record_index
                    && operand.group_record_index == Some(child.record_index)
                    && operand.group_member_ordinal == Some(0)
                    && operand.record_index == *operand_record_index
                    && operand.recipe_kind == ConstructionRecipeKind::BoundedFace
                    && crate::design::decode::dimension_frames::is_paired_recipe_reference_frame(
                        &operand.recipe_prefix_bytes,
                    )
            });
            matches!((leaves.next(), leaves.next()), (Some(_), None))
        })
}

/// Resolve one top-level Extrude profile group through its exact leaf operands.
///
/// A complete active bounded-face lane is represented directly when the
/// operand has no historical lane. Otherwise the selected faces must be
/// proven in the consuming feature's preceding topology.
pub(crate) fn resolved_extrude_profile_face_group(
    scope: &DesignParameterScope,
    root: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignFaceOperand],
) -> Option<cadmpeg_ir::features::ProfileRef> {
    use cadmpeg_ir::features::ProfileRef;

    let indices = extrude_profile_group_operand_indices(root, groups, operands)?;
    if let Some(faces) = resolved_extrude_profile_active_faces(&indices, operands) {
        return Some(ProfileRef::Faces(faces));
    }
    let mut faces = Vec::new();
    for index in indices {
        let slots = &operands.get(index)?.resolved_face_slots;
        if slots.is_empty() {
            return None;
        }
        for slot in slots {
            if !faces.contains(slot) {
                faces.push(*slot);
            }
        }
    }
    let selection = historical_face_selection(scope, root, faces)?;
    let cadmpeg_ir::features::FaceSelection::Historical {
        state,
        faces,
        native,
    } = selection
    else {
        return None;
    };
    Some(ProfileRef::HistoricalFaces {
        state,
        faces,
        native: vec![native],
    })
}

fn resolved_extrude_profile_active_faces(
    indices: &[usize],
    operands: &[DesignFaceOperand],
) -> Option<Vec<cadmpeg_ir::ids::FaceId>> {
    let mut faces = Vec::new();
    for index in indices {
        let operand = operands.get(*index)?;
        if operand.recipe_kind != crate::records::ConstructionRecipeKind::BoundedFace
            || operand.candidate_faces.is_empty()
            || !operand.unreferenced_candidate_faces.is_empty()
            || !operand.alternate_selector_candidate_faces.is_empty()
            || !operand.preceding_candidate_faces.is_empty()
            || !operand.changed_candidate_faces.is_empty()
            || !operand.historical_support_contexts.is_empty()
            || !operand.resolved_face_slots.is_empty()
            || operand.resolved_active_face.is_some()
        {
            return None;
        }
        let Some(crate::design::decode::operands::FaceRecipeProgramKind::Counted { header_value }) =
            crate::design::decode::operands::face_recipe_program_kind(&operand.recipe_program)
        else {
            return None;
        };
        if operand.recipe_nodes.len() != header_value
            || operand.recipe_node_offsets.len() != operand.recipe_nodes.len()
        {
            return None;
        }
        for face in &operand.candidate_faces {
            if !faces.contains(face) {
                faces.push(face.clone());
            }
        }
    }
    (!faces.is_empty()).then_some(faces)
}

/// Resolve a Loft section whose members use the edge-recipe envelope.
///
/// These members describe the selected face through a common persistent
/// selector/token clause. The clause must identify one direct preceding face
/// in every member, and its complete boundary must have exactly one edge per
/// group member. Any competing common clause or incomplete topology context
/// keeps the native group unresolved.
pub(crate) fn resolved_loft_edge_profile_group(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    operands: &[DesignEdgeOperand],
) -> Option<cadmpeg_ir::features::ProfileRef> {
    if scope.kind != "Loft"
        || !matches!(group.role, 0x41_0000_0000 | 0x43_0000_0000)
        || group.members.is_empty()
        || !group.lost_edge_references.is_empty()
    {
        return None;
    }
    let previous_state_id = scope.previous_history_state_id?;
    let stream = native_stream(&group.id)?;
    let group_ordinal = usize::try_from(group.scope_reference_ordinal).ok()?;
    let mut member_ids = HashSet::new();
    if group
        .members
        .iter()
        .any(|member| !member_ids.insert(*member))
    {
        return None;
    }
    if scope.reference_members.get(group_ordinal) != Some(&group.record_index) {
        return None;
    }
    let member_operands = group
        .members
        .iter()
        .enumerate()
        .map(|(ordinal, record_index)| {
            let scope_ordinal = group
                .scope_reference_ordinal
                .checked_add(1)?
                .checked_add(u32::try_from(ordinal).ok()?)?;
            if scope
                .reference_members
                .get(group_ordinal.checked_add(ordinal.checked_add(1)?)?)
                != Some(record_index)
            {
                return None;
            }
            let mut matches = operands.iter().filter(|operand| {
                native_stream(&operand.id) == Some(stream)
                    && operand.scope_record_index == group.scope_record_index
                    && operand.record_index == *record_index
            });
            let operand = matches.next()?;
            if matches.next().is_some()
                || operand.scope_reference_ordinal != scope_ordinal
                || operand.recipe_state_id != Some(previous_state_id)
                || operand.recipe_structure.is_none()
                || operand.surface_patch_recipe_structure.is_some()
            {
                return None;
            }
            Some(operand)
        })
        .collect::<Option<Vec<_>>>()?;
    let face_slot = loft_edge_profile_face_slot(group.members.len(), &member_operands)?;
    let selection = historical_face_selection(scope, group, vec![face_slot])?;
    let cadmpeg_ir::features::FaceSelection::Historical {
        state,
        faces,
        native,
    } = selection
    else {
        return None;
    };
    Some(cadmpeg_ir::features::ProfileRef::HistoricalFaces {
        state,
        faces,
        native: vec![native],
    })
}

fn loft_edge_profile_face_slot(
    member_count: usize,
    operands: &[&DesignEdgeOperand],
) -> Option<i64> {
    if member_count == 0 || operands.len() != member_count {
        return None;
    }
    let mut common_clauses = operands
        .first()?
        .recipe_references
        .iter()
        .filter(|reference| {
            reference.candidate_faces.len() == 1 && reference.alternate_selector_faces.is_empty()
        })
        .map(|reference| {
            (
                reference.selector,
                reference.token.clone(),
                reference.design_reference,
            )
        })
        .collect::<Vec<_>>();
    common_clauses.sort_unstable();
    common_clauses.dedup();
    common_clauses.retain(|(selector, token, design_reference)| {
        operands.iter().all(|operand| {
            operand
                .recipe_references
                .iter()
                .filter(|reference| {
                    reference.selector == *selector
                        && reference.token == *token
                        && reference.design_reference == *design_reference
                        && reference.candidate_faces.len() == 1
                        && reference.alternate_selector_faces.is_empty()
                })
                .count()
                == 1
        })
    });
    let [(selector, token, design_reference)] = common_clauses.as_slice() else {
        return None;
    };
    let evidence = operands
        .iter()
        .map(|operand| {
            if operand.recipe_references.len() != operand.recipe_reference_contexts.len()
                || operand.candidate_faces.is_empty()
                || operand.preceding_candidate_faces.is_empty()
                || operand.result_candidate_faces.is_empty()
            {
                return None;
            }
            let target_ordinal = operand
                .recipe_references
                .iter()
                .enumerate()
                .filter(|(_, reference)| {
                    reference.selector == *selector
                        && reference.token == *token
                        && reference.design_reference == *design_reference
                        && reference.candidate_faces.len() == 1
                        && reference.alternate_selector_faces.is_empty()
                })
                .map(|(ordinal, _)| ordinal)
                .next()?;
            let mut target = None;
            for (ordinal, (reference, context)) in operand
                .recipe_references
                .iter()
                .zip(&operand.recipe_reference_contexts)
                .enumerate()
            {
                if context.reference_ordinal != u32::try_from(ordinal).ok()? {
                    return None;
                }
                let [candidate] = reference.candidate_faces.as_slice() else {
                    return None;
                };
                let [preceding_face] = context.preceding_faces.as_slice() else {
                    return None;
                };
                if preceding_face != candidate {
                    return None;
                }
                let slot = preceding_face.0.rsplit_once('#')?.1.parse::<i64>().ok()?;
                if !operand.preceding_candidate_faces.contains(candidate)
                    || !operand.result_candidate_faces.contains(candidate)
                {
                    return None;
                }
                let [preceding_boundary] = context.preceding_face_boundaries.as_slice() else {
                    return None;
                };
                let boundary_edge_count =
                    boundary_edge_count(std::slice::from_ref(&preceding_boundary))?;
                if preceding_boundary.face_slot != slot {
                    return None;
                }
                let [result_face] = context.result_faces.as_slice() else {
                    return None;
                };
                let [result_boundary] = context.result_face_boundaries.as_slice() else {
                    return None;
                };
                if result_face != candidate || result_boundary.face_slot != slot {
                    return None;
                }
                let [support_slot] = context.preceding_support_face_slots.as_slice() else {
                    return None;
                };
                let [support_boundary] = context.preceding_support_face_boundaries.as_slice()
                else {
                    return None;
                };
                if *support_slot != slot || support_boundary.face_slot != slot {
                    return None;
                }
                if ordinal == target_ordinal {
                    target = Some((candidate.clone(), slot, boundary_edge_count));
                }
            }
            target
        })
        .collect::<Option<Vec<_>>>()?;
    let (face, slot, boundary_edge_count) = evidence.first()?;
    if *boundary_edge_count != member_count
        || evidence.iter().any(|(candidate, candidate_slot, count)| {
            candidate != face || candidate_slot != slot || count != boundary_edge_count
        })
    {
        return None;
    }
    Some(*slot)
}

pub(crate) fn resolved_historical_face_group(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    operands: &[DesignFaceOperand],
) -> Option<cadmpeg_ir::features::FaceSelection> {
    let faces = historical_face_group_slots(group, operands, false)?;
    historical_face_selection(scope, group, faces)
}

fn historical_face_selection(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    faces: Vec<i64>,
) -> Option<cadmpeg_ir::features::FaceSelection> {
    let previous_state_id = scope.previous_history_state_id?;
    historical_face_selection_in_state(scope, group, previous_state_id, faces)
}

fn historical_face_selection_in_state(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    previous_state_id: i64,
    faces: Vec<i64>,
) -> Option<cadmpeg_ir::features::FaceSelection> {
    historical_face_selection_with_native(scope, previous_state_id, faces, group.id.clone())
}

fn historical_face_selection_with_native(
    scope: &DesignParameterScope,
    previous_state_id: i64,
    faces: Vec<i64>,
    native: String,
) -> Option<cadmpeg_ir::features::FaceSelection> {
    use cadmpeg_ir::features::FaceSelection;

    if faces.is_empty() {
        return None;
    }
    let feature = neutral_feature_id(scope);
    let feature_key = feature
        .0
        .split_once('#')
        .map_or(feature.0.as_str(), |(_, key)| key);
    Some(FaceSelection::Historical {
        state: feature_input_topology_id(&feature, previous_state_id),
        faces: faces
            .into_iter()
            .map(|face| {
                ids::history_input_face_id(
                    &ids::history_input_prefix(feature_key, previous_state_id),
                    face,
                )
            })
            .collect(),
        native,
    })
}

/// Resolve `SplitFace` target groups whose bounded-face member run can include
/// complete nested support recipes without their own active candidate lanes.
/// A nested support recipe contributes only when history proves its preceding
/// face slots. An unresolved support recipe remains a context member. Every
/// other member must prove its preceding face slots, and at least one member
/// must contribute a face.
pub(crate) fn resolved_historical_split_face_target_group(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    operands: &[DesignFaceOperand],
) -> Option<cadmpeg_ir::features::FaceSelection> {
    if scope.kind != "SplitFace" || group.role != 0x0000_0010_0000_0000 {
        return None;
    }
    let faces = historical_face_group_slots(group, operands, true)?;
    historical_face_selection(scope, group, faces)
}

/// Resolve a `SplitFace` target from the operation transition when the member
/// recipes do not provide a complete per-member proof.
///
/// A `SplitFace` transition keeps each selected input face at the same stable
/// slot and marks it `updated`. The target group is complete only when that
/// updated-face set has the same cardinality as the group, every updated slot
/// is present in at least one member's preceding candidate lane, and all
/// members have a nonempty preceding lane. Members with no updated candidate
/// are context records and do not add a target face.
pub(crate) fn resolved_historical_split_face_target_group_with_updated_faces(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    operands: &[DesignFaceOperand],
    updated_face_slots: &[i64],
) -> Option<cadmpeg_ir::features::FaceSelection> {
    if scope.kind != "SplitFace" || group.role != 0x0000_0010_0000_0000 {
        return None;
    }
    resolved_historical_split_face_target_group(scope, group, operands).or_else(|| {
        let faces = split_face_updated_target_slots(scope, group, operands, updated_face_slots)?;
        historical_face_selection(scope, group, faces)
    })
}

fn split_face_updated_target_slots(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    operands: &[DesignFaceOperand],
    updated_face_slots: &[i64],
) -> Option<Vec<i64>> {
    if scope.kind != "SplitFace"
        || group.role != 0x0000_0010_0000_0000
        || updated_face_slots.is_empty()
        || updated_face_slots.len() != group.members.len()
    {
        return None;
    }
    let updated = updated_face_slots.iter().copied().collect::<HashSet<_>>();
    if updated.len() != updated_face_slots.len() {
        return None;
    }
    let stream = native_stream(&group.id)?;
    let mut represented = HashSet::new();
    let mut faces = Vec::new();
    for (ordinal, record_index) in group.members.iter().enumerate() {
        let ordinal = u32::try_from(ordinal).ok()?;
        let mut matches = operands.iter().filter(|operand| {
            native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == group.scope_record_index
                && operand.group_record_index == Some(group.record_index)
                && operand.group_member_ordinal == Some(ordinal)
                && operand.record_index == *record_index
                && operand.recipe_kind == crate::records::ConstructionRecipeKind::BoundedFace
        });
        let operand = matches.next()?;
        if matches.next().is_some() || operand.preceding_candidate_faces.is_empty() {
            return None;
        }
        for face in &operand.preceding_candidate_faces {
            let slot = face
                .0
                .rsplit_once('#')
                .and_then(|(_, slot)| slot.parse().ok())?;
            if updated.contains(&slot) && represented.insert(slot) {
                faces.push(slot);
            }
        }
    }
    (represented == updated).then_some(faces)
}

fn historical_face_group_slots(
    group: &DesignConstructionOperandGroup,
    operands: &[DesignFaceOperand],
    allow_split_face_context_members: bool,
) -> Option<Vec<i64>> {
    let stream = native_stream(&group.id)?;
    let mut faces = Vec::with_capacity(group.members.len());
    let mut contributing_members = 0;
    for (ordinal, record_index) in group.members.iter().enumerate() {
        let ordinal = u32::try_from(ordinal).ok()?;
        let mut matches = operands.iter().filter(|operand| {
            native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == group.scope_record_index
                && operand.group_record_index == Some(group.record_index)
                && operand.group_member_ordinal == Some(ordinal)
                && operand.record_index == *record_index
        });
        let operand = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        let member_slots = if operand.resolved_face_slots.is_empty() {
            if allow_split_face_context_members {
                if let Some(slots) = split_face_complete_candidate_slots(operand) {
                    Some(slots)
                } else if is_split_face_context_member(operand) {
                    continue;
                } else {
                    return None;
                }
            } else {
                return None;
            }
        } else {
            Some(operand.resolved_face_slots.clone())
        }?;
        contributing_members += 1;
        for face in member_slots {
            if !faces.contains(&face) {
                faces.push(face);
            }
        }
    }
    (contributing_members > 0 && !faces.is_empty()).then_some(faces)
}

fn split_face_complete_candidate_slots(operand: &DesignFaceOperand) -> Option<Vec<i64>> {
    complete_counted_face_recipe(operand)?;
    let faces = resolved_face_operand(operand)?;
    let slots = faces
        .iter()
        .map(|face| face.0.rsplit_once('#')?.1.parse::<i64>().ok())
        .collect::<Option<Vec<_>>>()?;
    let preceding = operand
        .preceding_candidate_faces
        .iter()
        .filter_map(|face| face.0.rsplit_once('#')?.1.parse::<i64>().ok())
        .collect::<HashSet<_>>();
    (!slots.is_empty() && slots.iter().all(|slot| preceding.contains(slot))).then_some(slots)
}

fn is_split_face_context_member(operand: &DesignFaceOperand) -> bool {
    operand.recipe_kind == crate::records::ConstructionRecipeKind::BoundedFace
        && crate::design::decode::operands::face_recipe_program_kind(&operand.recipe_program)
            .is_some_and(|kind| {
                matches!(
                    kind,
                    crate::design::decode::operands::FaceRecipeProgramKind::Counted { .. }
                )
            })
        && !operand.recipe_nodes.is_empty()
        && operand
            .recipe_nodes
            .iter()
            .all(|node| node.recipe_structure.is_some())
        && face_operand_candidates(operand).is_empty()
        && operand.alternate_selector_candidate_faces.is_empty()
        && operand.preceding_candidate_faces.is_empty()
        && operand.changed_candidate_faces.is_empty()
        && operand.recipe_references.iter().any(|reference| {
            !reference.candidate_faces.is_empty() || !reference.alternate_selector_faces.is_empty()
        })
}

fn resolved_face_operand(operand: &DesignFaceOperand) -> Option<Vec<cadmpeg_ir::ids::FaceId>> {
    if let Some(face) = &operand.resolved_active_face {
        return Some(vec![face.clone()]);
    }
    if !operand.resolved_face_slots.is_empty() {
        let active_candidates = operand
            .candidate_faces
            .iter()
            .chain(&operand.unreferenced_candidate_faces)
            .chain(&operand.alternate_selector_candidate_faces)
            .collect::<Vec<_>>();
        if active_candidates.is_empty() {
            return Some(
                operand
                    .resolved_face_slots
                    .iter()
                    .map(|slot| {
                        cadmpeg_ir::ids::FaceId::mint(ids::brep_entity_id(slot))
                            .expect("identity grammar")
                    })
                    .collect(),
            );
        }
        return operand
            .resolved_face_slots
            .iter()
            .map(|slot| {
                active_candidates
                    .iter()
                    .find(|face| {
                        face.0
                            .rsplit_once('#')
                            .and_then(|(_, ordinal)| ordinal.parse::<i64>().ok())
                            == Some(*slot)
                    })
                    .map(|face| (*face).clone())
            })
            .collect();
    }
    let candidates = face_operand_candidates(operand);
    if !operand.alternate_selector_candidate_faces.is_empty() {
        return Some(candidates.to_vec());
    }
    if operand.recipe_kind == crate::records::ConstructionRecipeKind::Face {
        let mut referenced = Vec::new();
        for reference in &operand.recipe_references {
            for face in &reference.candidate_faces {
                if !referenced.contains(face) {
                    referenced.push(face.clone());
                }
            }
        }
        if !referenced.is_empty() {
            return Some(referenced);
        }
    }
    if !operand.unreferenced_candidate_faces.is_empty()
        && complete_counted_face_recipe(operand).is_some()
    {
        return Some(candidates.to_vec());
    }
    let [face] = candidates else { return None };
    Some(vec![face.clone()])
}

fn explicit_bounded_face_candidates(
    operand: &DesignFaceOperand,
) -> Option<Vec<cadmpeg_ir::ids::FaceId>> {
    if operand.recipe_kind != crate::records::ConstructionRecipeKind::BoundedFace
        || !operand.resolved_face_slots.is_empty()
        || !operand.alternate_selector_candidate_faces.is_empty()
        || !operand.historical_support_contexts.is_empty()
        || complete_counted_face_recipe(operand).is_none()
    {
        return None;
    }
    if !operand.candidate_faces.is_empty() && operand.unreferenced_candidate_faces.is_empty() {
        return Some(operand.candidate_faces.clone());
    }

    let alternate_references = operand
        .recipe_references
        .iter()
        .filter(|reference| !reference.alternate_selector_faces.is_empty())
        .map(|reference| reference.design_reference)
        .collect::<HashSet<_>>();
    let candidate_set = operand.candidate_faces.iter().collect::<HashSet<_>>();
    let mut lanes = HashMap::<i64, Vec<cadmpeg_ir::ids::FaceId>>::new();
    for reference in &operand.recipe_references {
        if alternate_references.contains(&reference.design_reference) {
            continue;
        }
        for face in &reference.candidate_faces {
            if candidate_set.is_empty() || candidate_set.contains(face) {
                let lane = lanes.entry(reference.design_reference).or_default();
                if !lane.contains(face) {
                    lane.push(face.clone());
                }
            }
        }
    }
    let mut lanes = lanes
        .into_values()
        .filter(|lane| !lane.is_empty())
        .collect::<Vec<_>>();
    for lane in &mut lanes {
        lane.sort_by(|left, right| left.0.cmp(&right.0));
    }
    lanes.sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));
    let [lane, next @ ..] = lanes.as_slice() else {
        return None;
    };
    (next.first().is_none_or(|other| other.len() < lane.len())).then(|| lane.clone())
}

fn complete_counted_face_recipe(operand: &DesignFaceOperand) -> Option<usize> {
    if operand.recipe_kind != crate::records::ConstructionRecipeKind::BoundedFace {
        return None;
    }
    let Some(crate::design::decode::operands::FaceRecipeProgramKind::Counted { header_value }) =
        crate::design::decode::operands::face_recipe_program_kind(&operand.recipe_program)
    else {
        return None;
    };
    if operand.recipe_nodes.is_empty()
        || !operand
            .recipe_nodes
            .iter()
            .all(|node| node.recipe_structure.is_some())
    {
        return None;
    }
    let boundary_edge_count =
        unique_preceding_face_boundaries(&operand.historical_support_contexts)
            .and_then(|boundaries| boundary_edge_count(&boundaries));
    (operand.recipe_nodes.len() == header_value || boundary_edge_count == Some(header_value))
        .then_some(header_value)
}

/// Return the result-face lane of a legacy bounded-face recipe.
///
/// Older Extrude envelopes do not carry a separate lane ordinal. The
/// construction recipe's source `record_index` is the Design reference used
/// to build the aggregate result-face candidate lane. All persistent recipe
/// references carrying that Design reference therefore belong to the lane,
/// regardless of their position among context references. The rule is
/// admitted only when the counted envelope and node run are complete, the
/// operand has no alternate or unreferenced lane, and the selected references
/// agree with the aggregate candidate set.
pub(crate) fn legacy_face_recipe_reference_candidates(
    operand: &DesignFaceOperand,
    recipe_record_index: i32,
) -> Option<Vec<cadmpeg_ir::ids::FaceId>> {
    if operand.recipe_kind != crate::records::ConstructionRecipeKind::BoundedFace
        || !matches!(
            crate::design::decode::operands::face_recipe_program_kind(&operand.recipe_program),
            Some(crate::design::decode::operands::FaceRecipeProgramKind::Counted { .. })
        )
        || operand.recipe_nodes.is_empty()
        || operand.recipe_node_offsets.len() != operand.recipe_nodes.len()
        || !operand.unreferenced_candidate_faces.is_empty()
        || !operand.alternate_selector_candidate_faces.is_empty()
    {
        return None;
    }
    let mut candidates = operand
        .recipe_references
        .iter()
        .filter(|reference| reference.design_reference == i64::from(recipe_record_index))
        .flat_map(|reference| {
            if reference.candidate_faces.is_empty() {
                reference.alternate_selector_faces.iter()
            } else {
                reference.candidate_faces.iter()
            }
        })
        .cloned()
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| left.0.cmp(&right.0));
    candidates.dedup();
    if !operand.candidate_faces.is_empty()
        && operand.candidate_faces.iter().collect::<HashSet<_>>()
            != candidates.iter().collect::<HashSet<_>>()
    {
        return None;
    }
    (!candidates.is_empty()).then_some(candidates)
}

pub(crate) fn resolve_face_operand_history_candidates(operand: &DesignFaceOperand) -> Option<i64> {
    let Some(direct) = unique_face_operand_history_candidate(operand) else {
        return resolve_face_operand_support_candidate(operand);
    };
    if !historical_face_operand_candidates(operand).contains(direct)
        && nested_bounded_face_history_candidates(operand)
            .is_none_or(|candidates| !candidates.contains(direct))
    {
        return None;
    }
    direct.0.rsplit_once('#')?.1.parse().ok()
}

pub(crate) fn resolve_face_operand_history_candidate_from(
    operand: &DesignFaceOperand,
    candidates: &[cadmpeg_ir::ids::FaceId],
) -> Option<i64> {
    let Some(direct) = unique_face_operand_history_candidate(operand) else {
        return resolve_face_operand_support_candidate(operand);
    };
    candidates
        .contains(direct)
        .then(|| direct.0.rsplit_once('#')?.1.parse().ok())
        .flatten()
}

fn unique_face_operand_history_candidate(
    operand: &DesignFaceOperand,
) -> Option<&cadmpeg_ir::ids::FaceId> {
    match operand.preceding_candidate_faces.as_slice() {
        [face] => Some(face),
        _ => match operand.changed_candidate_faces.as_slice() {
            [face] => Some(face),
            _ => None,
        },
    }
}

pub(crate) fn resolve_bounded_face_history_candidates(
    operand: &DesignFaceOperand,
) -> Option<Vec<i64>> {
    if operand.recipe_kind != crate::records::ConstructionRecipeKind::BoundedFace {
        return None;
    }
    if let Some(candidate) = convergent_effective_face_support(operand) {
        return Some(candidate);
    }
    let header_value = complete_counted_face_recipe(operand)?;
    bounded_face_candidate_by_boundary_cardinality(
        header_value,
        &operand.historical_support_contexts,
    )
}

pub(crate) fn resolve_stable_bounded_face_history_set(
    operand: &DesignFaceOperand,
) -> Option<Vec<i64>> {
    complete_counted_face_recipe(operand)?;
    let mut active_faces = Vec::with_capacity(operand.preceding_candidate_faces.len());
    for face in &operand.preceding_candidate_faces {
        let slot = face.0.rsplit_once('#')?.1.parse::<i64>().ok()?;
        if active_faces.contains(&slot) {
            return None;
        }
        active_faces.push(slot);
    }
    stable_face_support_set(&active_faces, &operand.historical_support_contexts)
}

/// Resolve the selected input faces of an unhealed `SurfaceDeleteFace`.
///
/// This operation changes the selected faces in place rather than preserving
/// a stable support set. The bounded recipe is still admissible only when its
/// complete historical lane proves one preceding face per active candidate,
/// every such face is changed by the operation, and each context has a
/// complete boundary. A changed-face count alone is not sufficient: unrelated
/// topology changes must not become a face selection.
pub(crate) fn resolve_surface_delete_face_history_set(
    operand: &DesignFaceOperand,
) -> Option<Vec<i64>> {
    counted_face_recipe_frame(operand)?;
    let active_faces = unique_stable_face_slots(&operand.preceding_candidate_faces)?;
    let changed_faces = unique_stable_face_slots(&operand.changed_candidate_faces)?;
    if active_faces.is_empty() || changed_faces != active_faces {
        return None;
    }
    if operand.historical_support_contexts.len() != active_faces.len() {
        return None;
    }
    let mut covered = HashSet::with_capacity(active_faces.len());
    for context in &operand.historical_support_contexts {
        if !active_faces.contains(&context.active_face_slot)
            || !covered.insert(context.active_face_slot)
            || context.preceding_face_slots != [context.active_face_slot]
            || context.changed_preceding_face_slots != [context.active_face_slot]
        {
            return None;
        }
        let boundaries = valid_preceding_face_boundaries(context)?;
        let [boundary] = boundaries.as_slice() else {
            return None;
        };
        if boundary.face_slot != context.active_face_slot {
            return None;
        }
    }
    (covered.len() == active_faces.len()).then_some(active_faces)
}

fn unique_stable_face_slots(faces: &[cadmpeg_ir::ids::FaceId]) -> Option<Vec<i64>> {
    let mut slots = faces
        .iter()
        .map(|face| face.0.rsplit_once('#')?.1.parse::<i64>().ok())
        .collect::<Option<Vec<_>>>()?;
    if slots.iter().any(|slot| *slot < 0) {
        return None;
    }
    slots.sort_unstable();
    let unique = slots.windows(2).all(|pair| pair[0] != pair[1]);
    unique.then_some(slots)
}

fn counted_face_recipe_frame(operand: &DesignFaceOperand) -> Option<usize> {
    if operand.recipe_kind != crate::records::ConstructionRecipeKind::BoundedFace {
        return None;
    }
    let Some(crate::design::decode::operands::FaceRecipeProgramKind::Counted { header_value }) =
        crate::design::decode::operands::face_recipe_program_kind(&operand.recipe_program)
    else {
        return None;
    };
    if operand.recipe_nodes.len() != header_value
        || operand.recipe_nodes.is_empty()
        || operand
            .recipe_nodes
            .iter()
            .any(|node| !matches!(node.program.as_slice(), [-1, -1, 2, ..]))
    {
        return None;
    }
    Some(header_value)
}

fn stable_face_support_set(
    active_faces: &[i64],
    contexts: &[crate::records::DesignHistoricalFaceSupportContext],
) -> Option<Vec<i64>> {
    if active_faces.is_empty()
        || contexts.len() != active_faces.len()
        || active_faces.iter().collect::<HashSet<_>>().len() != active_faces.len()
    {
        return None;
    }
    let mut covered = HashSet::with_capacity(active_faces.len());
    for context in contexts {
        if !active_faces.contains(&context.active_face_slot)
            || !covered.insert(context.active_face_slot)
            || context.preceding_face_slots != [context.active_face_slot]
            || !context.changed_preceding_face_slots.is_empty()
        {
            return None;
        }
    }
    Some(active_faces.to_vec())
}

fn convergent_effective_face_support(operand: &DesignFaceOperand) -> Option<Vec<i64>> {
    let active_faces = effective_historical_face_slots(
        face_operand_candidates(operand),
        &operand.historical_support_contexts,
    )?;
    convergent_face_support(&active_faces, &operand.historical_support_contexts)
}

/// Return candidate slots that have a complete historical support context.
///
/// A persistent-reference lane can contain revisions that are absent from the
/// retained history topology. Those revisions are not effective candidates for
/// the common-support rule. The history binder omits them when it builds the
/// support contexts, so the context-active set is the effective subset. Keep
/// the subset admission explicit and reject a context that is not in the
/// operand's candidate lane.
fn effective_historical_face_slots(
    candidates: &[cadmpeg_ir::ids::FaceId],
    contexts: &[crate::records::DesignHistoricalFaceSupportContext],
) -> Option<Vec<i64>> {
    let mut candidate_slots = candidates
        .iter()
        .map(|face| face.0.rsplit_once('#')?.1.parse::<i64>().ok())
        .collect::<Option<Vec<_>>>()?;
    candidate_slots.sort_unstable();
    candidate_slots.dedup();

    let mut active_faces = contexts
        .iter()
        .map(|context| context.active_face_slot)
        .collect::<Vec<_>>();
    active_faces.sort_unstable();
    active_faces.dedup();
    (!active_faces.is_empty()
        && active_faces
            .iter()
            .all(|slot| candidate_slots.binary_search(slot).is_ok()))
    .then_some(active_faces)
}

fn convergent_face_support(
    active_faces: &[i64],
    support_contexts: &[crate::records::DesignHistoricalFaceSupportContext],
) -> Option<Vec<i64>> {
    if active_faces.is_empty() {
        return None;
    }
    let mut contexts = support_contexts.iter();
    let first = contexts.next()?;
    let mut support = first.preceding_face_slots.clone();
    support.sort_unstable();
    support.dedup();
    if support.is_empty() {
        return None;
    }
    let mut covered = vec![first.active_face_slot];
    for context in contexts {
        let mut candidate = context.preceding_face_slots.clone();
        candidate.sort_unstable();
        candidate.dedup();
        if candidate != support {
            return None;
        }
        covered.push(context.active_face_slot);
    }
    covered.sort_unstable();
    covered.dedup();
    (covered == active_faces).then_some(support)
}

fn bounded_face_candidate_by_boundary_cardinality(
    header_value: usize,
    contexts: &[crate::records::DesignHistoricalFaceSupportContext],
) -> Option<Vec<i64>> {
    let mut candidates = contexts
        .iter()
        .filter_map(|context| {
            let boundaries = valid_preceding_face_boundaries(context)?;
            let boundary_edge_count = boundary_edge_count(&boundaries)?;
            let faces = boundaries
                .iter()
                .map(|boundary| boundary.face_slot)
                .collect::<Vec<_>>();
            (boundary_edge_count == header_value).then_some(faces)
        })
        .collect::<Vec<_>>();
    if let Some(boundaries) = unique_preceding_face_boundaries(contexts) {
        if boundary_edge_count(&boundaries) == Some(header_value) {
            candidates.push(
                boundaries
                    .iter()
                    .map(|boundary| boundary.face_slot)
                    .collect(),
            );
        }
    }
    candidates.sort_unstable();
    candidates.dedup();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

fn valid_preceding_face_boundaries(
    context: &crate::records::DesignHistoricalFaceSupportContext,
) -> Option<Vec<&crate::records::DesignHistoricalFaceBoundaryContext>> {
    let mut expected_faces = context.preceding_face_slots.clone();
    expected_faces.sort_unstable();
    expected_faces.dedup();
    if expected_faces.is_empty() {
        return None;
    }
    let mut boundaries = context.preceding_face_boundaries.iter().collect::<Vec<_>>();
    if boundaries.iter().any(|boundary| boundary.loops.is_empty()) {
        return None;
    }
    boundaries.sort_unstable_by_key(|boundary| boundary.face_slot);
    if boundaries
        .windows(2)
        .any(|pair| pair[0].face_slot == pair[1].face_slot)
        || boundaries
            .iter()
            .map(|boundary| boundary.face_slot)
            .collect::<Vec<_>>()
            != expected_faces
    {
        return None;
    }
    Some(boundaries)
}

fn unique_preceding_face_boundaries(
    contexts: &[crate::records::DesignHistoricalFaceSupportContext],
) -> Option<Vec<&crate::records::DesignHistoricalFaceBoundaryContext>> {
    let mut active_faces = HashSet::new();
    let mut boundaries_by_face = HashMap::new();
    for context in contexts {
        if !active_faces.insert(context.active_face_slot) {
            return None;
        }
        for boundary in valid_preceding_face_boundaries(context)? {
            if let Some(previous) = boundaries_by_face.insert(boundary.face_slot, boundary) {
                if previous != boundary {
                    return None;
                }
            }
        }
    }
    let mut boundaries = boundaries_by_face.into_values().collect::<Vec<_>>();
    boundaries.sort_unstable_by_key(|boundary| boundary.face_slot);
    (!boundaries.is_empty()).then_some(boundaries)
}

fn boundary_edge_count(
    boundaries: &[&crate::records::DesignHistoricalFaceBoundaryContext],
) -> Option<usize> {
    boundaries.iter().try_fold(0usize, |total, boundary| {
        boundary.loops.iter().try_fold(total, |total, loop_| {
            (!loop_.edge_slots.is_empty() && loop_.edge_slots.len() == loop_.coedge_slots.len())
                .then(|| total.checked_add(loop_.edge_slots.len()))
                .flatten()
        })
    })
}

fn resolve_face_operand_support_candidate(operand: &DesignFaceOperand) -> Option<i64> {
    let reference = operand.recipe_references.first()?;
    let active_faces = if reference.candidate_faces.is_empty() {
        &reference.alternate_selector_faces
    } else {
        &reference.candidate_faces
    };
    let active_slots = active_faces
        .iter()
        .filter_map(|face| face.0.rsplit_once('#')?.1.parse::<i64>().ok())
        .collect::<HashSet<_>>();
    if active_slots.is_empty() {
        return None;
    }
    let mut candidates = operand
        .historical_support_contexts
        .iter()
        .filter(|context| active_slots.contains(&context.active_face_slot))
        .flat_map(|context| {
            if context.changed_preceding_face_slots.is_empty() {
                context.preceding_face_slots.iter()
            } else {
                context.changed_preceding_face_slots.iter()
            }
        })
        .copied()
        .collect::<Vec<_>>();
    candidates.sort_unstable();
    candidates.dedup();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(*candidate)
}

pub(crate) fn face_operand_candidates(operand: &DesignFaceOperand) -> &[cadmpeg_ir::ids::FaceId] {
    if !operand.alternate_selector_candidate_faces.is_empty() {
        &operand.alternate_selector_candidate_faces
    } else if operand.unreferenced_candidate_faces.is_empty() {
        &operand.candidate_faces
    } else {
        &operand.unreferenced_candidate_faces
    }
}

/// Return the active face identities that can participate in historical
/// resolution. A single-face recipe names its selected face in the recipe
/// reference even when the broader persistent-tag set also contains faces
/// excluded from the operand's unreferenced candidate lane.
pub(crate) fn historical_face_operand_candidates(
    operand: &DesignFaceOperand,
) -> Vec<cadmpeg_ir::ids::FaceId> {
    if operand.recipe_kind == crate::records::ConstructionRecipeKind::Face {
        let mut referenced = operand
            .recipe_references
            .iter()
            .flat_map(|reference| {
                reference
                    .candidate_faces
                    .iter()
                    .chain(&reference.alternate_selector_faces)
            })
            .cloned()
            .collect::<Vec<_>>();
        referenced.sort_by(|left, right| left.0.cmp(&right.0));
        referenced.dedup();
        if !referenced.is_empty() {
            return referenced;
        }
    }
    face_operand_candidates(operand).to_vec()
}

/// Return nested persistent-reference faces for a complete bounded-face
/// recipe whose own active candidate lanes are empty. The nested faces are
/// topology supports, not selected faces; callers must map them through the
/// historical support graph and prove a unique preceding target.
pub(crate) fn nested_bounded_face_history_candidates(
    operand: &DesignFaceOperand,
) -> Option<Vec<cadmpeg_ir::ids::FaceId>> {
    complete_counted_face_recipe(operand)?;
    if !operand.candidate_faces.is_empty()
        || !operand.unreferenced_candidate_faces.is_empty()
        || !operand.alternate_selector_candidate_faces.is_empty()
    {
        return None;
    }
    let mut candidates = operand
        .recipe_references
        .iter()
        .flat_map(|reference| {
            reference
                .candidate_faces
                .iter()
                .chain(&reference.alternate_selector_faces)
        })
        .cloned()
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| left.0.cmp(&right.0));
    candidates.dedup();
    (!candidates.is_empty()).then_some(candidates)
}

/// Return active B-rep faces for the legacy `FromFace` envelope whose counted
/// bounded recipe contains support references but no active face lane.
///
/// This is a candidate-generation proof, not a selection proof. The caller
/// must reduce the returned faces to one plane coincident with the profile
/// sketch before binding it to the operand.
fn extrude_start_plane_geometry_candidates(
    group: &DesignConstructionOperandGroup,
    operands: &[DesignFaceOperand],
    faces: &[cadmpeg_ir::topology::Face],
) -> Option<Vec<cadmpeg_ir::ids::FaceId>> {
    let [record_index] = group.members.as_slice() else {
        return None;
    };
    let mut matching = operands.iter().filter(|operand| {
        native_stream(&operand.id) == native_stream(&group.id)
            && operand.scope_record_index == group.scope_record_index
            && operand.record_index == *record_index
    });
    let operand = matching.next()?;
    if matching.next().is_some()
        || !face_operand_candidates(operand).is_empty()
        || !operand.resolved_face_slots.is_empty()
        || operand.resolved_active_face.is_some()
        || nested_bounded_face_history_candidates(operand).is_none()
    {
        return None;
    }
    Some(faces.iter().map(|face| face.id.clone()).collect())
}

fn extrude_profile_sketch_id(
    profile: &cadmpeg_ir::features::ProfileRef,
) -> Option<&cadmpeg_ir::sketches::SketchId> {
    use cadmpeg_ir::features::ProfileRef;

    match profile {
        ProfileRef::Sketch(sketch)
        | ProfileRef::SketchProfiles { sketch, .. }
        | ProfileRef::SketchRegions { sketch, .. }
        | ProfileRef::SketchEntities { sketch, .. }
        | ProfileRef::SketchSelection { sketch, .. } => Some(sketch),
        ProfileRef::Native(_)
        | ProfileRef::Unresolved(_)
        | ProfileRef::Feature(_)
        | ProfileRef::Generated { .. }
        | ProfileRef::SpatialSketchProfiles { .. }
        | ProfileRef::SpatialSketchSelection { .. }
        | ProfileRef::HistoricalFaces { .. }
        | ProfileRef::Faces(_) => None,
    }
}

/// Geometry and native records used to resolve selected-face Extrude inputs.
pub(crate) struct ExtrudeFaceResolution<'a> {
    pub faces: &'a [cadmpeg_ir::topology::Face],
    pub surfaces: &'a [cadmpeg_ir::geometry::Surface],
    pub groups: &'a [DesignConstructionOperandGroup],
    pub operands: &'a mut [DesignFaceOperand],
    pub linear_tolerance: f64,
    pub angular_tolerance: f64,
}

pub(crate) fn bind_extrude_start_planes(
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    resolution: &mut ExtrudeFaceResolution<'_>,
) {
    use cadmpeg_ir::features::{ExtrudeStart, FaceSelection, FeatureDefinition};

    for feature in features {
        let FeatureDefinition::Extrude { profile, start, .. } = &mut feature.definition else {
            continue;
        };
        let Some(sketch_id) = extrude_profile_sketch_id(profile) else {
            continue;
        };
        let Some(sketch) = sketches.iter().find(|sketch| sketch.id == *sketch_id) else {
            continue;
        };
        let ExtrudeStart::FromFace {
            face: FaceSelection::Native(native),
            offset,
        } = start
        else {
            continue;
        };
        let retained_offset = *offset;
        let mut matching_groups = resolution.groups.iter().filter(|group| group.id == *native);
        let Some(group) = matching_groups.next() else {
            continue;
        };
        if matching_groups.next().is_some()
            || group.extrude_face_role != Some(DesignExtrudeFaceRole::Start)
        {
            continue;
        }
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let mut candidates = Vec::new();
        for record_index in &group.members {
            let mut matching_operands = resolution.operands.iter().filter(|operand| {
                native_stream(&operand.id) == Some(stream)
                    && operand.scope_record_index == group.scope_record_index
                    && operand.record_index == *record_index
            });
            let Some(operand) = matching_operands.next() else {
                candidates.clear();
                break;
            };
            if matching_operands.next().is_some() {
                candidates.clear();
                break;
            }
            candidates.extend(face_operand_candidates(operand).iter().cloned());
        }
        candidates.sort_by(|left, right| left.0.cmp(&right.0));
        candidates.dedup();
        if candidates.is_empty() {
            if let Some(geometry_candidates) = extrude_start_plane_geometry_candidates(
                group,
                resolution.operands,
                resolution.faces,
            ) {
                candidates = geometry_candidates;
            }
        }
        let coincident = candidates
            .into_iter()
            .filter(|candidate| {
                face_coincident_with_sketch(
                    candidate,
                    sketch,
                    resolution.faces,
                    resolution.surfaces,
                    resolution.linear_tolerance,
                    resolution.angular_tolerance,
                )
            })
            .collect::<Vec<_>>();
        if let [face] = coincident.as_slice() {
            if retain_face_operand_resolution(group, resolution.operands, face) {
                *start = ExtrudeStart::FromFace {
                    face: FaceSelection::Resolved {
                        faces: vec![face.clone()],
                        native: native.clone(),
                    },
                    offset: retained_offset,
                };
            }
        }
    }
}

/// Resolve a legacy Extrude target face from a unique forward planar face.
///
/// Some bounded-face target operands retain only support references after
/// history projection. A target face is still exact when one referenced face
/// is planar, parallel to the sweep direction, and lies strictly ahead of
/// the profile plane. Ambiguous, nonplanar, and non-forward candidates remain
/// native.
pub(crate) fn bind_extrude_target_faces(
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    resolution: &mut ExtrudeFaceResolution<'_>,
) {
    use cadmpeg_ir::features::{ExtrudeDirection, ExtrudeExtent, FeatureDefinition};

    for feature in features {
        let FeatureDefinition::Extrude {
            profile,
            direction,
            extent,
            ..
        } = &mut feature.definition
        else {
            continue;
        };
        let Some(sketch_id) = extrude_profile_sketch_id(profile) else {
            continue;
        };
        let Some(sketch) = sketches.iter().find(|sketch| sketch.id == *sketch_id) else {
            continue;
        };
        let Some((sketch_origin, profile_normal, _)) = sketch.resolved_placement() else {
            continue;
        };
        let sweep_direction = match direction {
            ExtrudeDirection::ProfileNormal => profile_normal,
            ExtrudeDirection::ReversedProfileNormal => profile_normal.scale(-1.0),
            ExtrudeDirection::Explicit { vector, .. } => *vector,
            ExtrudeDirection::Unresolved => continue,
        };
        if !sweep_direction.x.is_finite()
            || !sweep_direction.y.is_finite()
            || !sweep_direction.z.is_finite()
            || sweep_direction.norm() <= 0.0
        {
            continue;
        }
        match extent {
            ExtrudeExtent::OneSided { side } => bind_extrude_target_face(
                &mut side.termination,
                sketch_origin,
                sweep_direction,
                resolution,
            ),
            ExtrudeExtent::TwoSided { first, second } => {
                bind_extrude_target_face(
                    &mut first.termination,
                    sketch_origin,
                    sweep_direction,
                    resolution,
                );
                bind_extrude_target_face(
                    &mut second.termination,
                    sketch_origin,
                    sweep_direction.scale(-1.0),
                    resolution,
                );
            }
            ExtrudeExtent::Symmetric { .. } => {}
        }
    }
}

fn bind_extrude_target_face(
    termination: &mut cadmpeg_ir::features::LinearTermination,
    sketch_origin: Point3,
    sweep_direction: Vector3,
    resolution: &mut ExtrudeFaceResolution<'_>,
) {
    use cadmpeg_ir::features::{FaceSelection, LinearTermination};

    let LinearTermination::ToFace {
        face: FaceSelection::Native(native),
        ..
    } = termination
    else {
        return;
    };
    let native = native.clone();
    let mut matching_groups = resolution.groups.iter().filter(|group| {
        group.id == native && group.extrude_face_role == Some(DesignExtrudeFaceRole::Termination)
    });
    let Some(group) = matching_groups.next() else {
        return;
    };
    if matching_groups.next().is_some() {
        return;
    }
    let Some(face) =
        extrude_target_plane_candidate(group, resolution, sketch_origin, sweep_direction)
    else {
        return;
    };
    let offset = match termination {
        LinearTermination::ToFace { offset, .. } => *offset,
        _ => return,
    };
    if retain_face_operand_resolution(group, resolution.operands, &face) {
        *termination = LinearTermination::ToFace {
            face: FaceSelection::Resolved {
                faces: vec![face],
                native,
            },
            offset,
        };
    }
}

fn extrude_target_plane_candidate(
    group: &DesignConstructionOperandGroup,
    resolution: &ExtrudeFaceResolution<'_>,
    sketch_origin: Point3,
    sweep_direction: Vector3,
) -> Option<cadmpeg_ir::ids::FaceId> {
    let [record_index] = group.members.as_slice() else {
        return None;
    };
    let stream = native_stream(&group.id)?;
    let mut matching_operands = resolution.operands.iter().filter(|operand| {
        native_stream(&operand.id) == Some(stream)
            && operand.scope_record_index == group.scope_record_index
            && operand.record_index == *record_index
    });
    let operand = matching_operands.next()?;
    if matching_operands.next().is_some() {
        return None;
    }
    let mut candidates = face_operand_candidates(operand).to_vec();
    if candidates.is_empty() {
        candidates = nested_bounded_face_history_candidates(operand)?;
    }
    candidates.sort_by(|left, right| left.0.cmp(&right.0));
    candidates.dedup();
    let direction_length = sweep_direction.norm();
    let mut matches = candidates
        .into_iter()
        .filter_map(|candidate| {
            let face = resolution.faces.iter().find(|face| face.id == candidate)?;
            let surface = resolution
                .surfaces
                .iter()
                .find(|surface| surface.id == face.surface)?;
            let cadmpeg_ir::geometry::SurfaceGeometry::Plane { origin, normal, .. } =
                &surface.geometry
            else {
                return None;
            };
            if !parallel_vectors(*normal, sweep_direction, resolution.angular_tolerance) {
                return None;
            }
            let distance =
                origin.vector_from(sketch_origin).dot(sweep_direction) / direction_length;
            (distance > resolution.linear_tolerance).then_some(candidate)
        })
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| left.0.cmp(&right.0));
    matches.dedup();
    let [face] = matches.as_slice() else {
        return None;
    };
    Some(face.clone())
}

pub(crate) fn retain_face_operand_resolution(
    group: &DesignConstructionOperandGroup,
    operands: &mut [DesignFaceOperand],
    face: &cadmpeg_ir::ids::FaceId,
) -> bool {
    let Some(stream) = native_stream(&group.id) else {
        return false;
    };
    let mut matches = operands.iter_mut().filter(|operand| {
        native_stream(&operand.id) == Some(stream)
            && operand.scope_record_index == group.scope_record_index
            && group.members.contains(&operand.record_index)
            && (face_operand_candidates(operand).contains(face)
                || (face_operand_candidates(operand).is_empty()
                    && operand.resolved_face_slots.is_empty()
                    && operand.resolved_active_face.is_none()
                    && nested_bounded_face_history_candidates(operand).is_some()))
    });
    let Some(operand) = matches.next() else {
        return false;
    };
    if matches.next().is_some() {
        return false;
    }
    let geometry_bound = face_operand_candidates(operand).is_empty()
        && operand.resolved_face_slots.is_empty()
        && operand.resolved_active_face.is_none()
        && nested_bounded_face_history_candidates(operand).is_some();
    if geometry_bound {
        operand.resolved_active_face = Some(face.clone());
        return true;
    }
    let Some(slot) = face
        .0
        .rsplit_once('#')
        .and_then(|(_, slot)| slot.parse::<i64>().ok())
    else {
        return false;
    };
    if !operand.resolved_face_slots.is_empty() && operand.resolved_face_slots != [slot] {
        return false;
    }
    operand.resolved_face_slots = vec![slot];
    true
}

pub(crate) fn face_coincident_with_sketch(
    candidate: &cadmpeg_ir::ids::FaceId,
    sketch: &cadmpeg_ir::sketches::Sketch,
    faces: &[cadmpeg_ir::topology::Face],
    surfaces: &[cadmpeg_ir::geometry::Surface],
    linear_tolerance: f64,
    angular_tolerance: f64,
) -> bool {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let Some(face) = faces.iter().find(|face| face.id == *candidate) else {
        return false;
    };
    let Some(surface) = surfaces.iter().find(|surface| surface.id == face.surface) else {
        return false;
    };
    let SurfaceGeometry::Plane { origin, normal, .. } = &surface.geometry else {
        return false;
    };
    let Some((sketch_origin, sketch_normal, _)) = sketch.resolved_placement() else {
        return false;
    };
    parallel_vectors(*normal, sketch_normal, angular_tolerance)
        && point_plane_distance(*origin, sketch_origin, sketch_normal) <= linear_tolerance
}

fn parallel_vectors(left: Vector3, right: Vector3, tolerance: f64) -> bool {
    let left_length = left.norm();
    let right_length = right.norm();
    let cross_length = left.cross(right).norm();
    left_length > 0.0
        && right_length > 0.0
        && cross_length <= tolerance * left_length * right_length
}

fn point_plane_distance(point: Point3, origin: Point3, normal: Vector3) -> f64 {
    let normal_length = normal.norm();
    if normal_length == 0.0 {
        return f64::INFINITY;
    }
    point.vector_from(origin).dot(normal).abs() / normal_length
}

pub(crate) fn design_angle(parameter: &DesignParameter) -> Option<cadmpeg_ir::features::Angle> {
    (parameter.unit.as_deref().is_some_and(design_angle_unit)
        && parameter.evaluated_value.is_finite())
    .then_some(cadmpeg_ir::features::Angle(parameter.evaluated_value))
}

pub(crate) fn valid_chamfer_spec(spec: &cadmpeg_ir::features::ChamferSpec) -> bool {
    use cadmpeg_ir::features::ChamferSpec;

    match spec {
        ChamferSpec::Distance { distance } => distance.0 > 0.0,
        ChamferSpec::TwoDistances { first, second } => first.0 > 0.0 && second.0 > 0.0,
        ChamferSpec::DistanceAngle { distance, angle } => {
            distance.0 > 0.0 && angle.0 > 0.0 && angle.0 < std::f64::consts::PI
        }
        ChamferSpec::Unresolved
        | ChamferSpec::UnresolvedDistance
        | ChamferSpec::UnresolvedTwoDistances
        | ChamferSpec::UnresolvedDistanceAngle => false,
    }
}

/// Length scale from a placement's stored origin to the neutral length unit.
/// The 201/329-byte frames store the origin in the neutral unit directly; the
/// `EntityGenesis`-flavor 213/341-byte frames and the member-run head record
/// of a feature-owned sketch store it in centimetres while their sketch point
/// and curve records carry values ten times the centimetre value, so the
/// origin scales by ten to stay commensurate with the entities.
pub(crate) fn placement_origin_scale(placement: &DesignSketchPlacement) -> f64 {
    if placement.member_run_head || matches!(placement.frame_length, 213 | 341) {
        10.0
    } else {
        1.0
    }
}

pub(crate) fn sketch_curve_is_spatial(curve: &SketchCurveIdentity) -> bool {
    match curve.geometry.as_ref() {
        Some(SketchCurveGeometry::Line { start, end, .. }) => {
            !(planar_point(start) && planar_point(end))
        }
        Some(SketchCurveGeometry::Arc {
            center,
            normal,
            reference_direction,
            ..
        }) => {
            !(planar_point(center)
                && reference_direction.z.abs() <= EPS_FACE_RESOLVE_SKETCH_CURVE_IS_SPATIAL_E9
                && sketch_normal_sign(normal).is_some())
        }
        Some(SketchCurveGeometry::Nurbs { control_points, .. }) => {
            control_points.iter().any(|point| !planar_point(point))
        }
        None => false,
    }
}

pub(crate) fn sketch_point_depth(point: &SketchPoint) -> Option<f64> {
    point.depth.is_finite().then_some(point.depth)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::records::{
        DesignConstructionOperandGroup, DesignEdgeOperand, DesignEdgeRecipeReferenceContext,
        DesignEdgeRecipeStructure, DesignFaceRecipeNode, DesignHistoricalFaceBoundaryContext,
        DesignHistoricalFaceLoopContext, DesignHistoricalFaceSupportContext, DesignParameterScope,
        DesignRecipeReference,
    };

    use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
    use cadmpeg_ir::ids::FaceId;
    use cadmpeg_ir::ids::{ShellId, SurfaceId};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::sketches::{Sketch, SketchId};
    use cadmpeg_ir::topology::{Face, Sense};

    fn face(slot: i64) -> FaceId {
        FaceId::mint(format!("f3d:brep:entity#{slot}")).expect("identity grammar")
    }

    #[test]
    fn explicit_bounded_face_group_uses_only_its_owned_candidate_lane() {
        let mut operand: DesignFaceOperand = serde_json::from_value(serde_json::json!({
            "id": "f3d:test:face-operand#200",
            "scope_record_index": 100,
            "scope_reference_ordinal": 0,
            "group_record_index": 150,
            "group_member_ordinal": 0,
            "record_index": 200,
            "byte_offset": 0,
            "class_tag": "346",
            "paired_byte_offset": 325,
            "paired_class_tag": "262",
            "recipe_record_index": 201,
            "recipe_record_byte_offset": 0,
            "recipe_id": "f3d:test:recipe#201",
            "recipe_prefix_offset": 0,
            "recipe_prefix_bytes": "",
            "recipe_references": [],
            "recipe_kind": "bounded_face",
            "recipe_program_offset": 0,
            "recipe_program": [0, -1, 1],
            "recipe_node_offsets": [0],
            "recipe_nodes": [{
                "byte_offset": 0,
                "end_byte_offset": 12,
                "program": [0, -1, 1],
                "recipe_structure": {
                    "root": 0,
                    "prelude": [0, 0],
                    "sides": [
                        {"field_count": 1, "header_value": 0, "payload_entry_count": 0, "payload_prefix": [], "scalars": [], "entries": []},
                        {"field_count": 1, "header_value": 0, "payload_entry_count": 0, "payload_prefix": [], "scalars": [], "entries": []}
                    ],
                    "postlude": []
                }
            }],
            "candidate_faces": ["f3d:brep:entity#10", "f3d:brep:entity#20"],
            "unreferenced_candidate_faces": [],
            "alternate_selector_candidate_faces": [],
            "preceding_candidate_faces": [],
            "changed_candidate_faces": [],
            "historical_support_contexts": [],
            "resolved_face_slots": [],
            "next_record_index": 202,
            "next_byte_offset": 100
        }))
        .expect("legacy bounded-face operand");
        operand.recipe_references = vec![
            reference(10, "selected-a", 201),
            reference(20, "selected-b", 201),
        ];

        let mut group: DesignConstructionOperandGroup = serde_json::from_value(serde_json::json!({
            "id": "f3d:test:construction-group#150",
            "scope_record_index": 100,
            "scope_reference_ordinal": 0,
            "record_index": 150,
            "byte_offset": 0,
            "class_tag": "346",
            "role": 0x0000_0010_0000_0000_u64,
            "members": [200],
            "member_offsets": [0],
            "frame": {
                "member_count_offset": 0,
                "opaque_index": 1,
                "opaque_index_offset": 0,
                "opaque_scalar": 0.0,
                "opaque_scalar_offset": 0,
                "variant": false
            },
            "role_offset": 0,
            "paired_class_tag": "262",
            "paired_byte_offset": 325,
            "next_record_index": 151,
            "next_byte_offset": 0
        }))
        .expect("legacy Draft face group");

        assert_eq!(
            resolved_explicit_bounded_face_group(&group, &[operand.clone()]),
            Some(cadmpeg_ir::features::FaceSelection::Resolved {
                faces: vec![face(10), face(20)],
                native: group.id.clone(),
            })
        );
        group.extrude_role = Some(crate::records::DesignExtrudeOperandRole::Profile);
        let scope: DesignParameterScope = serde_json::from_value(serde_json::json!({
            "id": "f3d:test:scope#100",
            "byte_offset": 0,
            "class_tag": "304",
            "record_index": 100,
            "frame_length": 300,
            "kind": "Extrude",
            "kind_offset": 0,
            "feature_ordinal": 1,
            "feature_ordinal_offset": 0,
            "history_state_id": 2,
            "history_state_id_offset": 0,
            "previous_history_state_id": 1,
            "previous_history_state_id_offset": 0,
            "reference_count_offset": 0,
            "reference_members": [150],
            "reference_member_offsets": [0],
            "paired_class_tag": "258",
            "paired_byte_offset": 300
        }))
        .expect("Extrude scope");
        assert_eq!(
            resolved_extrude_profile_face_group(
                &scope,
                &group,
                std::slice::from_ref(&group),
                &[operand.clone()],
            ),
            Some(cadmpeg_ir::features::ProfileRef::Faces(vec![
                face(10),
                face(20),
            ]))
        );
        operand.resolved_active_face =
            Some(FaceId::mint("f3d:brep/legacy/entity#30").expect("identity grammar"));
        assert_eq!(
            resolved_face_group(&group, std::slice::from_ref(&operand)),
            Some(cadmpeg_ir::features::FaceSelection::Resolved {
                faces: vec![FaceId::mint("f3d:brep/legacy/entity#30").expect("identity grammar")],
                native: group.id.clone(),
            })
        );
        operand.resolved_active_face = None;
        operand.preceding_candidate_faces = vec![face(10), face(20)];
        assert_eq!(
            resolved_face_group(&group, std::slice::from_ref(&operand)),
            None
        );
        operand.preceding_candidate_faces.clear();

        operand.candidate_faces.clear();
        operand.recipe_references = vec![
            reference(30, "context", 202),
            reference(10, "selected-a", 201),
            reference(20, "selected-b", 201),
        ];
        operand.candidate_faces = vec![face(10), face(20)];
        assert_eq!(
            legacy_face_recipe_reference_candidates(&operand, 201),
            Some(vec![face(10), face(20)])
        );
        operand.candidate_faces.clear();
        assert_eq!(
            resolved_explicit_bounded_face_group(&group, &[operand.clone()]),
            Some(cadmpeg_ir::features::FaceSelection::Resolved {
                faces: vec![face(10), face(20)],
                native: group.id.clone(),
            })
        );
        assert_eq!(
            legacy_face_recipe_reference_candidates(&operand, 201),
            Some(vec![face(10), face(20)])
        );
        assert!(legacy_face_recipe_reference_candidates(&operand, 999).is_none());
        operand.recipe_references.remove(0);
        operand.recipe_references[0]
            .alternate_selector_faces
            .push(face(30));
        assert!(resolved_explicit_bounded_face_group(&group, &[operand]).is_none());
    }

    #[test]
    fn split_face_updated_transition_resolves_legacy_target_group() {
        fn operand(record_index: u32, ordinal: u32, preceding: &[i64]) -> DesignFaceOperand {
            let preceding = preceding.iter().copied().map(face).collect::<Vec<_>>();
            serde_json::from_value(serde_json::json!({
                "id": format!("f3d:test:face-operand#{record_index}"),
                "scope_record_index": 100,
                "scope_reference_ordinal": 1,
                "group_record_index": 150,
                "group_member_ordinal": ordinal,
                "record_index": record_index,
                "byte_offset": 0,
                "class_tag": "277",
                "paired_byte_offset": 407,
                "paired_class_tag": "258",
                "recipe_record_index": record_index + 3,
                "recipe_record_byte_offset": 0,
                "recipe_id": format!("f3d:test:recipe#{record_index}"),
                "recipe_prefix_offset": 0,
                "recipe_prefix_bytes": "",
                "recipe_references": [],
                "recipe_kind": "bounded_face",
                "recipe_program_offset": 0,
                "recipe_program": [0, -1, 2],
                "recipe_node_offsets": [],
                "recipe_nodes": [],
                "candidate_faces": preceding,
                "preceding_candidate_faces": preceding,
                "next_record_index": record_index + 4,
                "next_byte_offset": 0
            }))
            .expect("legacy SplitFace target operand")
        }

        let scope: DesignParameterScope = serde_json::from_value(serde_json::json!({
            "id": "f3d:test:scope#100",
            "byte_offset": 0,
            "class_tag": "277",
            "record_index": 100,
            "frame_length": 407,
            "kind": "SplitFace",
            "kind_offset": 0,
            "feature_ordinal": 1,
            "feature_ordinal_offset": 0,
            "history_state_id": 50,
            "history_state_id_offset": 0,
            "previous_history_state_id": 49,
            "previous_history_state_id_offset": 0,
            "reference_count_offset": 0,
            "reference_members": [150, 200, 201, 202],
            "reference_member_offsets": [0, 0, 0, 0],
            "paired_class_tag": "258",
            "paired_byte_offset": 407
        }))
        .expect("legacy SplitFace scope");
        let group: DesignConstructionOperandGroup = serde_json::from_value(serde_json::json!({
            "id": "f3d:test:construction-group#150",
            "scope_record_index": 100,
            "scope_reference_ordinal": 1,
            "record_index": 150,
            "byte_offset": 0,
            "class_tag": "262",
            "members": [200, 201, 202],
            "member_offsets": [0, 0, 0],
            "frame": {
                "member_count_offset": 0,
                "opaque_index": 1,
                "opaque_index_offset": 0,
                "opaque_scalar": 0.0,
                "opaque_scalar_offset": 0,
                "variant": false
            },
            "role": 0x0000_0010_0000_0000_u64,
            "role_offset": 0,
            "paired_class_tag": "258",
            "paired_byte_offset": 0
        }))
        .expect("legacy SplitFace target group");
        let operands = vec![
            operand(200, 0, &[10, 20]),
            operand(201, 1, &[20, 30]),
            operand(202, 2, &[30]),
        ];

        let selection = resolved_historical_split_face_target_group_with_updated_faces(
            &scope,
            &group,
            &operands,
            &[10, 20, 30],
        )
        .expect("updated target transition proof");
        let cadmpeg_ir::features::FaceSelection::Historical {
            state,
            faces,
            native,
        } = selection
        else {
            panic!("expected historical SplitFace target");
        };
        assert_eq!(
            state,
            feature_input_topology_id(&neutral_feature_id(&scope), 49)
        );
        assert_eq!(native, group.id);
        assert_eq!(
            faces
                .iter()
                .map(|face| face.0.rsplit_once(':').unwrap().1)
                .collect::<Vec<_>>(),
            ["10", "20", "30"]
        );
        assert!(
            resolved_historical_split_face_target_group_with_updated_faces(
                &scope,
                &group,
                &operands,
                &[10, 20]
            )
            .is_none()
        );
        assert!(
            resolved_historical_split_face_target_group_with_updated_faces(
                &scope,
                &group,
                &operands,
                &[10, 20, 40]
            )
            .is_none()
        );
    }

    fn boundary(slot: i64, edge_count: usize) -> DesignHistoricalFaceBoundaryContext {
        DesignHistoricalFaceBoundaryContext {
            face_slot: slot,
            loops: vec![DesignHistoricalFaceLoopContext {
                loop_slot: slot + 1_000,
                coedge_slots: (0..edge_count)
                    .map(|ordinal| i64::try_from(ordinal).expect("test ordinal"))
                    .collect(),
                edge_slots: (0..edge_count)
                    .map(|ordinal| i64::try_from(ordinal).expect("test ordinal") + 2_000)
                    .collect(),
                vertex_slots: Vec::new(),
                point_slots: Vec::new(),
                positions: Vec::new(),
            }],
        }
    }

    fn reference(slot: i64, token: &str, design_reference: i64) -> DesignRecipeReference {
        DesignRecipeReference {
            selector: 1,
            selector_offset: 0,
            token: token.into(),
            token_offset: 0,
            design_reference,
            design_reference_offset: 0,
            candidate_faces: vec![face(slot)],
            candidate_edges: Vec::new(),
            alternate_selector_faces: Vec::new(),
            alternate_selector_edges: Vec::new(),
        }
    }

    fn reference_context(
        ordinal: u32,
        slot: i64,
        edge_count: usize,
    ) -> DesignEdgeRecipeReferenceContext {
        let face = face(slot);
        let boundary = boundary(slot, edge_count);
        let edges = boundary.loops[0].edge_slots.clone();
        DesignEdgeRecipeReferenceContext {
            reference_ordinal: ordinal,
            result_faces: vec![face.clone()],
            result_face_boundaries: vec![boundary.clone()],
            result_shared_edge_slots: edges.clone(),
            preceding_faces: vec![face],
            preceding_face_boundaries: vec![boundary.clone()],
            preceding_support_face_slots: vec![slot],
            preceding_support_face_boundaries: vec![boundary],
            shared_edge_slots: edges,
            changed_shared_edge_slots: Vec::new(),
            changed_reference_edge_slots: Vec::new(),
        }
    }

    fn edge_operand(
        record_index: u32,
        target_slot: i64,
        target_ordinal: usize,
        target_edge_count: usize,
        context_slot: i64,
    ) -> DesignEdgeOperand {
        let target_reference = reference(target_slot, "target", 308);
        let context_reference = reference(context_slot, &format!("context-{record_index}"), 308);
        let references = if target_ordinal == 0 {
            vec![target_reference, context_reference]
        } else {
            vec![context_reference, target_reference]
        };
        let contexts = if target_ordinal == 0 {
            vec![
                reference_context(0, target_slot, target_edge_count),
                reference_context(1, context_slot, 2),
            ]
        } else {
            vec![
                reference_context(0, context_slot, 2),
                reference_context(1, target_slot, target_edge_count),
            ]
        };
        let mut operand: DesignEdgeOperand = serde_json::from_value(serde_json::json!({
            "id": format!("f3d:test:edge-operand#{record_index}"),
            "scope_record_index": 1811,
            "scope_reference_ordinal": 3 + record_index - 1,
            "record_index": record_index,
            "byte_offset": 0,
            "class_tag": "376",
            "paired_byte_offset": 0,
            "paired_class_tag": "260",
            "recipe_record_index": record_index + 3,
            "recipe_record_byte_offset": 0,
            "recipe_id": "f3d:test:recipe",
            "recipe_prefix_offset": 0,
            "recipe_prefix_bytes": "",
            "recipe_references": [],
            "recipe_program_offset": 0,
            "recipe_program": [-1, -1, 2],
            "next_record_index": record_index + 4,
            "next_byte_offset": 0
        }))
        .expect("edge recipe operand");
        operand.recipe_references = references;
        operand.recipe_structure = Some(DesignEdgeRecipeStructure {
            root: 2,
            sides: Vec::new(),
        });
        operand.recipe_reference_contexts = contexts;
        operand.recipe_state_id = Some(6);
        operand.candidate_faces = operand
            .recipe_references
            .iter()
            .flat_map(|reference| reference.candidate_faces.iter().cloned())
            .collect();
        operand.preceding_candidate_faces = operand.candidate_faces.clone();
        operand.result_candidate_faces = operand.candidate_faces.clone();
        operand
    }

    fn append_reference(
        operand: &mut DesignEdgeOperand,
        ordinal: u32,
        slot: i64,
        token: &str,
        design_reference: i64,
        edge_count: usize,
    ) {
        let candidate = face(slot);
        operand
            .recipe_references
            .push(reference(slot, token, design_reference));
        operand
            .recipe_reference_contexts
            .push(reference_context(ordinal, slot, edge_count));
        operand.candidate_faces.push(candidate.clone());
        operand.preceding_candidate_faces.push(candidate.clone());
        operand.result_candidate_faces.push(candidate);
    }

    fn loft_scope() -> DesignParameterScope {
        serde_json::from_value(serde_json::json!({
            "id": "f3d:test:scope#1811",
            "byte_offset": 0,
            "class_tag": "272",
            "record_index": 1811,
            "frame_length": 0,
            "kind": "Loft",
            "kind_offset": 0,
            "feature_ordinal": 1,
            "feature_ordinal_offset": 0,
            "history_state_id": 7,
            "history_state_id_offset": 0,
            "previous_history_state_id": 6,
            "previous_history_state_id_offset": 0,
            "reference_count_offset": 0,
            "reference_members": [1000, 1001, 1819, 1, 2, 3],
            "reference_member_offsets": [0, 0, 0, 0, 0, 0],
            "paired_class_tag": "274",
            "paired_byte_offset": 0
        }))
        .expect("Loft scope")
    }

    fn loft_group() -> DesignConstructionOperandGroup {
        serde_json::from_value(serde_json::json!({
            "id": "f3d:test:group#111045",
            "scope_record_index": 1811,
            "scope_reference_ordinal": 2,
            "record_index": 1819,
            "byte_offset": 0,
            "class_tag": "267",
            "members": [1, 2, 3],
            "member_offsets": [0, 0, 0],
            "frame": {
                "member_count_offset": 0,
                "opaque_index": 252,
                "opaque_index_offset": 0,
                "opaque_scalar": 0.0,
                "opaque_scalar_offset": 0,
                "variant": false
            },
            "role": 287_762_808_832_i64,
            "role_offset": 0,
            "paired_class_tag": "260",
            "paired_byte_offset": 0
        }))
        .expect("Loft group")
    }

    fn support(active: i64, faces: &[(i64, usize)]) -> DesignHistoricalFaceSupportContext {
        DesignHistoricalFaceSupportContext {
            active_face_slot: active,
            surface_slot: active + 1_000,
            preceding_face_slots: faces.iter().map(|(face, _)| *face).collect(),
            preceding_face_boundaries: faces
                .iter()
                .map(|(face, edge_count)| DesignHistoricalFaceBoundaryContext {
                    face_slot: *face,
                    loops: vec![DesignHistoricalFaceLoopContext {
                        loop_slot: face + 2_000,
                        coedge_slots: (0..*edge_count)
                            .map(|ordinal| i64::try_from(ordinal).expect("test ordinal"))
                            .collect(),
                        edge_slots: (0..*edge_count)
                            .map(|ordinal| i64::try_from(ordinal).expect("test ordinal") + 10_000)
                            .collect(),
                        vertex_slots: Vec::new(),
                        point_slots: Vec::new(),
                        positions: Vec::new(),
                    }],
                })
                .collect(),
            changed_preceding_face_slots: Vec::new(),
        }
    }

    #[test]
    fn bounded_face_cardinality_resolves_one_complete_predecessor_set() {
        let contexts = [
            support(10, &[(100, 12)]),
            support(11, &[(101, 4), (102, 4)]),
            support(12, &[(101, 4), (102, 4)]),
        ];
        assert_eq!(
            bounded_face_candidate_by_boundary_cardinality(8, &contexts),
            Some(vec![101, 102])
        );
        assert_eq!(
            bounded_face_candidate_by_boundary_cardinality(12, &contexts),
            Some(vec![100])
        );
        assert_eq!(
            bounded_face_candidate_by_boundary_cardinality(20, &contexts),
            Some(vec![100, 101, 102])
        );
    }

    #[test]
    fn bounded_face_cardinality_rejects_conflicting_complete_sets() {
        let contexts = [support(10, &[(100, 4)]), support(11, &[(101, 4)])];
        assert_eq!(
            bounded_face_candidate_by_boundary_cardinality(4, &contexts),
            None
        );
    }

    #[test]
    fn effective_faces_resolve_only_with_complete_convergent_support() {
        let contexts = [
            support(10, &[(100, 4)]),
            support(11, &[(100, 4)]),
            support(12, &[(100, 4)]),
        ];
        assert_eq!(
            convergent_face_support(&[10, 11, 12], &contexts),
            Some(vec![100])
        );
        assert_eq!(convergent_face_support(&[10, 11, 12, 13], &contexts), None);

        let conflicting = [support(10, &[(100, 4)]), support(11, &[(101, 4)])];
        assert_eq!(convergent_face_support(&[10, 11], &conflicting), None);
    }

    #[test]
    fn effective_historical_faces_exclude_unmapped_revisions() {
        let contexts = [
            support(10, &[(100, 4)]),
            support(11, &[(100, 4)]),
            support(12, &[(100, 4)]),
        ];
        let candidates = [face(10), face(11), face(12), face(13)];
        assert_eq!(
            effective_historical_face_slots(&candidates, &contexts),
            Some(vec![10, 11, 12])
        );
        assert_eq!(
            effective_historical_face_slots(&[face(10), face(11)], &contexts),
            None
        );
    }

    #[test]
    fn stable_bounded_face_set_preserves_each_proven_predecessor() {
        let active_faces = [10, 11];
        let mut contexts = vec![support(10, &[(10, 4)]), support(11, &[(11, 4)])];

        assert_eq!(
            stable_face_support_set(&active_faces, &contexts),
            Some(vec![10, 11])
        );
        contexts[1].preceding_face_slots = vec![12];
        assert_eq!(stable_face_support_set(&active_faces, &contexts), None);
    }

    #[test]
    fn surface_delete_face_history_set_requires_complete_changed_one_to_one_support() {
        let node = DesignFaceRecipeNode {
            byte_offset: 0,
            end_byte_offset: 12,
            program: vec![-1, -1, 2],
            recipe_structure: None,
        };
        let mut operand: DesignFaceOperand = serde_json::from_value(serde_json::json!({
            "id": "f3d:test:surface-delete-face-operand#1",
            "scope_record_index": 10,
            "scope_reference_ordinal": 1,
            "group_record_index": 20,
            "group_member_ordinal": 0,
            "record_index": 21,
            "byte_offset": 0,
            "class_tag": "282",
            "paired_byte_offset": 12,
            "paired_class_tag": "259",
            "recipe_record_index": 22,
            "recipe_record_byte_offset": 24,
            "recipe_id": "f3d:test:recipe#22",
            "recipe_prefix_offset": 0,
            "recipe_prefix_bytes": "",
            "recipe_references": [],
            "recipe_kind": "bounded_face",
            "recipe_program_offset": 0,
            "recipe_program": [0, -1, 1],
            "recipe_node_offsets": [0],
            "recipe_nodes": [],
            "next_record_index": 23,
            "next_byte_offset": 36,
            "candidate_faces": [
                "f3d:brep:entity#10",
                "f3d:brep:entity#11",
                "f3d:brep:entity#12"
            ],
            "preceding_candidate_faces": [
                "f3d:brep:entity#10",
                "f3d:brep:entity#11"
            ],
            "changed_candidate_faces": [
                "f3d:brep:entity#10",
                "f3d:brep:entity#11"
            ]
        }))
        .expect("surface-delete-face operand");
        operand.recipe_nodes = vec![node];
        operand.historical_support_contexts =
            vec![support(10, &[(10, 4)]), support(11, &[(11, 4)])];
        for context in &mut operand.historical_support_contexts {
            context.changed_preceding_face_slots = vec![context.active_face_slot];
        }

        assert_eq!(
            resolve_surface_delete_face_history_set(&operand),
            Some(vec![10, 11])
        );

        operand.historical_support_contexts[1]
            .changed_preceding_face_slots
            .clear();
        assert_eq!(resolve_surface_delete_face_history_set(&operand), None);

        operand.historical_support_contexts[1].changed_preceding_face_slots = vec![11];
        operand.historical_support_contexts[1].preceding_face_slots = vec![10, 11];
        assert_eq!(resolve_surface_delete_face_history_set(&operand), None);
    }

    #[test]
    fn loft_edge_profile_requires_one_common_face_clause() {
        let first = edge_operand(1, 100, 0, 3, 200);
        let second = edge_operand(2, 100, 1, 3, 201);
        let third = edge_operand(3, 100, 0, 3, 202);
        let members = [&first, &second, &third];
        assert_eq!(loft_edge_profile_face_slot(3, &members), Some(100));

        let different_target = edge_operand(4, 101, 1, 3, 203);
        let mismatched = [&first, &different_target, &third];
        assert_eq!(loft_edge_profile_face_slot(3, &mismatched), None);

        let mut ambiguous = first.clone();
        append_reference(&mut ambiguous, 2, 300, "alternate", 309, 2);
        let mut ambiguous_second = second.clone();
        append_reference(&mut ambiguous_second, 2, 300, "alternate", 309, 2);
        let mut ambiguous_third = third.clone();
        append_reference(&mut ambiguous_third, 2, 300, "alternate", 309, 2);
        let ambiguous_members = [&ambiguous, &ambiguous_second, &ambiguous_third];
        assert_eq!(loft_edge_profile_face_slot(3, &ambiguous_members), None);
    }

    #[test]
    fn loft_edge_profile_projects_the_proven_face_into_history() {
        let scope = loft_scope();
        let group = loft_group();
        let feature = crate::ids::neutral_feature_id(&scope);
        let feature_key = feature
            .0
            .split_once('#')
            .map_or(feature.0.as_str(), |(_, key)| key);
        let expected_state = crate::design::edge_resolve::feature_input_topology_id(&feature, 6);
        let expected_face = crate::ids::history_input_face_id(
            &crate::ids::history_input_prefix(feature_key, 6),
            100,
        );
        let operands = vec![
            edge_operand(1, 100, 0, 3, 200),
            edge_operand(2, 100, 1, 3, 201),
            edge_operand(3, 100, 0, 3, 202),
        ];
        assert!(matches!(
            super::resolved_loft_edge_profile_group(&scope, &group, &operands),
            Some(cadmpeg_ir::features::ProfileRef::HistoricalFaces {
                state,
                faces,
                native,
            }) if state == expected_state && faces == [expected_face] && native == [group.id]
        ));
    }

    #[test]
    fn extrude_start_plane_geometry_fallback_requires_complete_nested_recipe() {
        let operand: DesignFaceOperand = serde_json::from_value(serde_json::json!({
            "id": "f3d:test:face-operand#200",
            "scope_record_index": 100,
            "scope_reference_ordinal": 0,
            "group_record_index": 150,
            "group_member_ordinal": 0,
            "record_index": 200,
            "byte_offset": 0,
            "class_tag": "271",
            "paired_byte_offset": 325,
            "paired_class_tag": "261",
            "recipe_record_index": 201,
            "recipe_record_byte_offset": 0,
            "recipe_id": "f3d:test:recipe#201",
            "recipe_prefix_offset": 0,
            "recipe_prefix_bytes": "",
            "recipe_references": [{
                "selector": 1,
                "selector_offset": 0,
                "token": "support",
                "token_offset": 0,
                "design_reference": 201,
                "design_reference_offset": 0,
                "candidate_faces": ["f3d:brep:entity#10"]
            }],
            "recipe_kind": "bounded_face",
            "recipe_program_offset": 0,
            "recipe_program": [0, -1, 1],
            "recipe_node_offsets": [0],
            "recipe_nodes": [{
                "byte_offset": 0,
                "end_byte_offset": 12,
                "program": [0, -1, 1],
                "recipe_structure": {
                    "root": 0,
                    "prelude": [0, 0],
                    "sides": [
                        {"field_count": 1, "header_value": 0, "scalars": [], "payload_prefix": [], "payload_entry_count": 0, "entries": []},
                        {"field_count": 1, "header_value": 0, "scalars": [], "payload_prefix": [], "payload_entry_count": 0, "entries": []}
                    ],
                    "postlude": []
                }
            }],
            "candidate_faces": [],
            "unreferenced_candidate_faces": [],
            "alternate_selector_candidate_faces": [],
            "preceding_candidate_faces": [],
            "changed_candidate_faces": [],
            "historical_support_contexts": [],
            "resolved_face_slots": [],
            "next_record_index": 202,
            "next_byte_offset": 100
        }))
        .expect("nested bounded-face operand");
        let group: DesignConstructionOperandGroup = serde_json::from_value(serde_json::json!({
            "id": "f3d:test:construction-group#150",
            "scope_record_index": 100,
            "scope_reference_ordinal": 0,
            "record_index": 150,
            "byte_offset": 0,
            "class_tag": "338",
            "role": 0,
            "members": [200],
            "member_offsets": [0],
            "frame": {
                "member_count_offset": 0,
                "opaque_index": 0,
                "opaque_index_offset": 0,
                "opaque_scalar": 0.0,
                "opaque_scalar_offset": 0,
                "variant": false
            },
            "role_offset": 0,
            "paired_class_tag": "261",
            "paired_byte_offset": 0
        }))
        .expect("FromFace group");
        let faces = vec![Face {
            id: face(10),
            shell: ShellId::mint("shell").expect("identity grammar"),
            surface: SurfaceId::mint("surface").expect("identity grammar"),
            sense: Sense::Forward,
            loops: Vec::new().into(),
            name: None,
            color: None,
            tolerance: None,
        }];

        assert_eq!(
            extrude_start_plane_geometry_candidates(&group, std::slice::from_ref(&operand), &faces,),
            Some(vec![face(10)])
        );
        let mut bound = operand.clone();
        assert!(retain_face_operand_resolution(
            &group,
            std::slice::from_mut(&mut bound),
            &face(10)
        ));
        assert_eq!(bound.resolved_active_face, Some(face(10)));

        let mut incomplete = operand;
        incomplete.recipe_nodes.clear();
        assert!(extrude_start_plane_geometry_candidates(&group, &[incomplete], &faces).is_none());
    }

    #[test]
    fn selected_face_start_requires_unique_sketch_plane_coincidence() {
        let sketch = Sketch {
            id: SketchId("sketch".into()),
            name: None,
            configuration: None,
            visible: None,
            placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
                origin: Point3::new(0.0, 0.0, 2.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            profiles: Vec::new(),
            native_ref: None,
        };
        let face = |id: &str, surface: &str| Face {
            id: FaceId::mint(id).expect("identity grammar"),
            shell: ShellId::mint("shell").expect("identity grammar"),
            surface: SurfaceId::mint(surface).expect("identity grammar"),
            sense: Sense::Forward,
            loops: Vec::new().into(),
            name: None,
            color: None,
            tolerance: None,
        };
        let plane = |id: &str, origin: Point3, normal: Vector3| Surface {
            id: SurfaceId::mint(id).expect("identity grammar"),
            geometry: SurfaceGeometry::Plane {
                origin,
                normal,
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        };
        let faces = [
            face("coincident", "surface-coincident"),
            face("offset", "surface-offset"),
            face("tilted", "surface-tilted"),
        ];
        let surfaces = [
            plane(
                "surface-coincident",
                Point3::new(5.0, -3.0, 2.0),
                Vector3::new(0.0, 0.0, -2.0),
            ),
            plane(
                "surface-offset",
                Point3::new(0.0, 0.0, 2.1),
                Vector3::new(0.0, 0.0, 1.0),
            ),
            plane(
                "surface-tilted",
                Point3::new(0.0, 0.0, 2.0),
                Vector3::new(0.0, 1.0, 0.0),
            ),
        ];

        assert!(crate::design::face_resolve::face_coincident_with_sketch(
            &faces[0].id,
            &sketch,
            &faces,
            &surfaces,
            1.0e-6,
            1.0e-10,
        ));
        for candidate in &faces[1..] {
            assert!(!crate::design::face_resolve::face_coincident_with_sketch(
                &candidate.id,
                &sketch,
                &faces,
                &surfaces,
                1.0e-6,
                1.0e-10,
            ));
        }
    }

    fn target_plane_operand(candidates: &[i64]) -> DesignFaceOperand {
        let candidate_faces = candidates
            .iter()
            .map(|slot| face(*slot).0)
            .collect::<Vec<_>>();
        serde_json::from_value(serde_json::json!({
            "id": "f3d:test:face-operand#200",
            "scope_record_index": 100,
            "scope_reference_ordinal": 0,
            "group_record_index": 150,
            "group_member_ordinal": 0,
            "record_index": 200,
            "byte_offset": 0,
            "class_tag": "271",
            "paired_byte_offset": 325,
            "paired_class_tag": "261",
            "recipe_record_index": 201,
            "recipe_record_byte_offset": 0,
            "recipe_id": "f3d:test:recipe#201",
            "recipe_prefix_offset": 0,
            "recipe_prefix_bytes": "",
            "recipe_references": [],
            "recipe_kind": "face",
            "recipe_program_offset": 0,
            "recipe_program": [],
            "recipe_node_offsets": [],
            "recipe_nodes": [],
            "candidate_faces": candidate_faces,
            "unreferenced_candidate_faces": [],
            "alternate_selector_candidate_faces": [],
            "preceding_candidate_faces": [],
            "changed_candidate_faces": [],
            "historical_support_contexts": [],
            "resolved_face_slots": [],
            "next_record_index": 202,
            "next_byte_offset": 100
        }))
        .expect("target face operand")
    }

    fn target_face_group() -> DesignConstructionOperandGroup {
        serde_json::from_value(serde_json::json!({
            "id": "f3d:test:construction-group#150",
            "scope_record_index": 100,
            "scope_reference_ordinal": 0,
            "record_index": 150,
            "byte_offset": 0,
            "class_tag": "338",
            "members": [200],
            "member_offsets": [0],
            "frame": {
                "member_count_offset": 0,
                "auxiliary_record_indices": [],
                "auxiliary_record_offsets": [],
                "auxiliary_paths": [],
                "trailing_record_indices": [],
                "trailing_record_offsets": [],
                "trailing_transforms": [],
                "trailing_dual_transforms": [],
                "trailing_flags": [],
                "opaque_index": 0,
                "opaque_index_offset": 0,
                "opaque_scalar": 0.0,
                "opaque_scalar_offset": 0,
                "variant": false
            },
            "role": 0,
            "extrude_role": "faces",
            "extrude_face_role": "termination",
            "role_offset": 0,
            "paired_class_tag": "261",
            "paired_byte_offset": 0
        }))
        .expect("target face group")
    }

    #[test]
    fn extrude_target_geometry_requires_one_forward_parallel_plane() {
        const TARGET_LINEAR_TOLERANCE: f64 = 1.0e-9;
        const TARGET_ANGULAR_TOLERANCE: f64 = 1.0e-9;

        let face_with_surface = |slot: i64, surface: &str| Face {
            id: face(slot),
            shell: ShellId::mint("shell").expect("identity grammar"),
            surface: SurfaceId::mint(surface).expect("identity grammar"),
            sense: Sense::Forward,
            loops: Vec::new().into(),
            name: None,
            color: None,
            tolerance: None,
        };
        let plane = |id: &str, origin: Point3, normal: Vector3| Surface {
            id: SurfaceId::mint(id).expect("identity grammar"),
            geometry: SurfaceGeometry::Plane {
                origin,
                normal,
                u_axis: Vector3::new(0.0, 1.0, 0.0),
            },
            source_object: None,
        };
        let faces = [
            face_with_surface(1, "surface-forward"),
            face_with_surface(2, "surface-forward-2"),
            face_with_surface(3, "surface-backward"),
            face_with_surface(4, "surface-tilted"),
        ];
        let surfaces = [
            plane(
                "surface-forward",
                Point3::new(3.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            ),
            plane(
                "surface-forward-2",
                Point3::new(5.0, 0.0, 0.0),
                Vector3::new(-1.0, 0.0, 0.0),
            ),
            plane(
                "surface-backward",
                Point3::new(-2.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            ),
            plane(
                "surface-tilted",
                Point3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
            ),
        ];
        let group = target_face_group();
        let origin = Point3::new(0.0, 0.0, 0.0);
        let sweep_direction = Vector3::new(1.0, 0.0, 0.0);
        let candidate = |candidates: &[i64]| {
            let mut operands = vec![target_plane_operand(candidates)];
            let resolution = ExtrudeFaceResolution {
                faces: &faces,
                surfaces: &surfaces,
                groups: &[],
                operands: &mut operands,
                linear_tolerance: TARGET_LINEAR_TOLERANCE,
                angular_tolerance: TARGET_ANGULAR_TOLERANCE,
            };
            extrude_target_plane_candidate(&group, &resolution, origin, sweep_direction)
        };

        assert_eq!(candidate(&[1, 3, 4]), Some(face(1)));
        assert!(candidate(&[1, 2, 3, 4]).is_none());
        assert!(candidate(&[3, 4]).is_none());
    }
}
