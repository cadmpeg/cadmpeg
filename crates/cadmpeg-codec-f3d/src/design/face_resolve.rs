// SPDX-License-Identifier: Apache-2.0
//! Resolve face-selection operands and extrude start planes.

use crate::design::dimensions::{planar_point, sketch_normal_sign};
use crate::design::edge_resolve::feature_input_topology_id;
use crate::design::feature_project::design_angle_unit;
use crate::ids::{self, native_stream, neutral_feature_id};
use crate::records::{
    feature::{
        extrude::{DesignExtrudeExtent, DesignExtrudePrologue},
        scope::DesignParameterScope,
    },
    parameters::DesignParameter,
    sketch_geometry::{SketchCurveGeometry, SketchCurveIdentity},
    sketch_placement::DesignSketchPlacement,
    topology::{
        body_recipe::DesignBodyRecipeOperand, construction::DesignConstructionOperandGroup,
        edge_identity::DesignEdgeOperand, extrude_selection::DesignExtrudeFaceRole,
        extrude_selection::DesignOperandRole, face::DesignFaceOperand,
    },
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

const EPS_FACE_RESOLVE_SKETCH_CURVE_IS_SPATIAL_E9: f64 = 1.0e-9;
#[cfg(test)]
const EPS_FACE_TEST_TARGET_LINEAR_E9: f64 = 1.0e-9;
#[cfg(test)]
const EPS_FACE_TEST_TARGET_ANGULAR_E9: f64 = 1.0e-9;

fn push_face_item<T>(
    ctx: Option<&DecodeContext<'_>>,
    items: &mut Vec<T>,
    item: T,
    operation: &'static str,
) -> Result<(), CodecError> {
    if let Some(ctx) = ctx {
        ctx.charge_collection_items(1, operation)?;
        items.try_reserve(1).map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    }
    items.push(item);
    Ok(())
}

fn insert_face_map<K: Eq + Hash, V>(
    ctx: Option<&DecodeContext<'_>>,
    entries: &mut HashMap<K, V>,
    key: K,
    value: V,
    operation: &'static str,
) -> Result<Option<V>, CodecError> {
    if !entries.contains_key(&key) {
        if let Some(ctx) = ctx {
            ctx.charge_collection_items(1, operation)?;
            entries.try_reserve(1).map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
        }
    }
    Ok(entries.insert(key, value))
}

fn insert_face_set<T: Eq + Hash>(
    ctx: Option<&DecodeContext<'_>>,
    entries: &mut HashSet<T>,
    value: T,
    operation: &'static str,
) -> Result<bool, CodecError> {
    if !entries.contains(&value) {
        if let Some(ctx) = ctx {
            ctx.charge_collection_items(1, operation)?;
            entries.try_reserve(1).map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
        }
    }
    Ok(entries.insert(value))
}

fn copy_face_text(
    ctx: Option<&DecodeContext<'_>>,
    value: &str,
    operation: &'static str,
) -> Result<String, CodecError> {
    let Some(ctx) = ctx else { return Ok(value.to_owned()); };
    String::from_utf8(ctx.copy_retained(value.as_bytes(), operation)?)
        .map_err(|_| CodecError::malformed("validated face identity is not UTF-8"))
}

fn copy_face_id(
    ctx: Option<&DecodeContext<'_>>,
    face: &cadmpeg_ir::ids::FaceId,
    operation: &'static str,
) -> Result<cadmpeg_ir::ids::FaceId, CodecError> {
    let text = copy_face_text(ctx, face.as_str(), operation)?;
    cadmpeg_ir::ids::FaceId::try_from(text).map_err(CodecError::malformed)
}

fn face_slot_id(
    ctx: Option<&DecodeContext<'_>>,
    slot: i64,
) -> Result<cadmpeg_ir::ids::FaceId, CodecError> {
    let Some(ctx) = ctx else { return Ok(ids::brep_face_id(slot)); };
    const PREFIX: &str = "f3d:brep:entity#";
    let mut magnitude = slot.unsigned_abs();
    let mut digits = 1usize;
    while magnitude >= 10 {
        magnitude /= 10;
        digits += 1;
    }
    let len = PREFIX.len() + digits + usize::from(slot.is_negative());
    let operation = "f3d resolved face slot id";
    ctx.charge_retained(u64::try_from(len).map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?, operation)?;
    let mut text = String::new();
    text.try_reserve(len).map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    text.push_str(PREFIX);
    use std::fmt::Write;
    write!(&mut text, "{slot}").map_err(|_| CodecError::malformed("face slot formatting failed"))?;
    cadmpeg_ir::ids::FaceId::try_from(text).map_err(CodecError::malformed)
}

/// Admit the legacy reference-aware face-target frame that omits its zero
/// `Side1Offset` owner and parameter.
pub(crate) fn extrude_omits_zero_side_one_offset(
    scope: &DesignParameterScope,
    prologue: &DesignExtrudePrologue,
    side_one_offset_count: usize,
) -> bool {
    side_one_offset_count == 0
        && scope.class_tag.as_str() == "330"
        && scope.paired_class_tag.as_str() == "258"
        && scope.frame_length() == 476
        && matches!(
            prologue,
            DesignExtrudePrologue::ReferenceAware {
                extent: DesignExtrudeExtent::OneSidedToFace,
                ..
            }
        )
}

pub(super) fn resolved_face_group(
    ctx: Option<&DecodeContext<'_>>,
    group: &DesignConstructionOperandGroup,
    operands: &[DesignFaceOperand],
) -> Result<Option<cadmpeg_ir::features::FaceSelection>, CodecError> {
    let Some(stream) = native_stream(&group.id) else { return Ok(None); };
    let mut faces = Vec::new();
    for record_index in group.members().iter().map(|member| &member.value) {
        let mut matches = operands.iter().filter(|operand| {
            native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == group.scope_record_index
                && operand.record_index() == *record_index
        });
        let Some(operand) = matches.next() else { return Ok(None); };
        if matches.next().is_some() {
            return Ok(None);
        }
        if operand.resolved_active_face.is_none()
            && operand.resolved_face_slots.is_empty()
            && operand.alternate_selector_candidate_faces.is_empty()
            && (!operand.preceding_candidate_faces.is_empty()
                || !operand.changed_candidate_faces.is_empty()
                || !operand.historical_support_contexts.is_empty())
        {
            return Ok(None);
        }
        let Some(operand_faces) = resolved_face_operand(ctx, operand)? else { return Ok(None); };
        for face in operand_faces {
            if !faces.contains(&face) {
                push_face_item(ctx, &mut faces, face, "f3d resolved face group member")?;
            }
        }
    }
    if faces.is_empty() { return Ok(None); }
    let native = copy_face_text(ctx, &group.id, "f3d resolved face group native id")?;
    Ok(Some(cadmpeg_ir::features::FaceSelection::Resolved {
        faces,
        native,
    }))
}

/// Resolve a bounded-face group whose active lane is fully named by its own
/// recipe selector, but whose generation does not emit historical slots.
///
/// This is intentionally separate from `resolved_face_group`: a multi-face
/// bounded recipe normally needs a slot or history proof. The legacy Draft
/// form admitted here has a complete structured recipe, no alternate or
/// historical candidates, and no candidate outside the recipe's own
/// selector. Those invariants make the active lane the selected face set.
pub(super) fn resolved_explicit_bounded_face_group(
    ctx: Option<&DecodeContext<'_>>,
    group: &DesignConstructionOperandGroup,
    operands: &[DesignFaceOperand],
) -> Result<Option<cadmpeg_ir::features::FaceSelection>, CodecError> {
    let Some(stream) = native_stream(&group.id) else { return Ok(None); };
    let mut faces = Vec::new();
    for record_index in group.members().iter().map(|member| &member.value) {
        let mut matches = operands.iter().filter(|operand| {
            native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == group.scope_record_index
                && operand.record_index() == *record_index
        });
        let Some(operand) = matches.next() else { return Ok(None); };
        if matches.next().is_some() {
            return Ok(None);
        }
        let candidate_faces = if operand.resolved_face_slots.is_empty() {
            let Some(faces) = explicit_bounded_face_candidates(ctx, operand)? else { return Ok(None); };
            faces
        } else {
            let Some(faces) = resolved_face_operand(ctx, operand)? else { return Ok(None); };
            faces
        };
        for face in candidate_faces {
            if !faces.contains(&face) {
                push_face_item(ctx, &mut faces, face, "f3d explicit bounded face group member")?;
            }
        }
    }
    if faces.is_empty() { return Ok(None); }
    let native = copy_face_text(ctx, &group.id, "f3d explicit bounded face group native id")?;
    Ok(Some(cadmpeg_ir::features::FaceSelection::Resolved {
        faces,
        native,
    }))
}

/// Resolve a direct scope face selection when every direct operand proves the
/// same current face set through stable input-topology slots.
pub(super) fn resolved_direct_face_selection(
    ctx: Option<&DecodeContext<'_>>,
    scope: &DesignParameterScope,
    operands: &[DesignFaceOperand],
) -> Result<Option<cadmpeg_ir::features::FaceSelection>, CodecError> {
    use cadmpeg_ir::features::FaceSelection;

    let Some(stream) = native_stream(&scope.id) else { return Ok(None); };
    let mut matching = Vec::new();
    for operand in operands
        .iter()
        .filter(|operand| {
            native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == scope.record_index
                && operand.group_record_index().is_none()
                && operand.group_member_ordinal().is_none()
                && operand.recipe_kind
                    == crate::records::recipes::ConstructionRecipeKind::BoundedFace
                && usize::try_from(operand.scope_reference_ordinal)
                    .ok()
                    .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                    == Some(&operand.record_index())
        }) {
        push_face_item(ctx, &mut matching, operand, "f3d direct face operand")?;
    }
    matching.sort_by_key(|operand| operand.scope_reference_ordinal);
    if matching.is_empty()
        || matching
            .iter()
            .any(|operand| operand.resolved_face_slots.is_empty())
    {
        return Ok(None);
    }
    let Some(mut faces) = resolved_face_operand(ctx, matching[0])? else { return Ok(None); };
    faces.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    if faces.is_empty() {
        return Ok(None);
    }
    for operand in &matching[1..] {
        let Some(mut candidate) = resolved_face_operand(ctx, operand)? else { return Ok(None); };
        candidate.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        if candidate != faces {
            return Ok(None);
        }
    }
    let native = copy_face_text(ctx, &scope.id, "f3d direct face native id")?;
    Ok(Some(FaceSelection::Resolved {
        faces,
        native,
    }))
}

/// Resolve a face operand whose exact preceding topology proves one face.
///
/// This path is for single-face operands whose recipe carries the selected face
/// in its persistent-reference lane but whose active candidate lane is not a
/// current-face slot. The caller must still admit the operand's exact recipe
/// form; this helper only applies the unique historical-face proof.
pub(super) fn resolved_historical_face_operand(
    scope: &DesignParameterScope,
    operand: &DesignFaceOperand,
) -> Option<cadmpeg_ir::features::FaceSelection> {
    let previous_state_id = scope.previous_history_state_id()?;
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
pub(super) fn resolved_body_recipe_selection(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    operands: &[DesignBodyRecipeOperand],
) -> Option<cadmpeg_ir::features::FaceSelection> {
    if group.scope_record_index != scope.record_index
        || group.extrude_role().is_some()
        || group.members().is_empty()
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
    for (ordinal, record_index) in group
        .members()
        .iter()
        .map(|member| &member.value)
        .enumerate()
    {
        if !member_records.insert(*record_index) {
            return None;
        }
        let ordinal = u32::try_from(ordinal).ok()?;
        let mut matches = operands.iter().filter(|operand| {
            native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == group.scope_record_index
                && operand.owner.group() == Some((group.record_index, ordinal))
                && operand.record_index() == *record_index
        });
        let operand = matches.next()?;
        if matches.next().is_some()
            || operand.references().is_empty()
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
pub(super) fn resolved_body_recipe_shape(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    operands: &[DesignBodyRecipeOperand],
) -> Option<cadmpeg_ir::features::FaceSelection> {
    if crate::design::design_feature_family(&scope.kind())
        != Some(crate::design::DesignFeatureFamily::Extrude)
        || group.role() != DesignOperandRole::ROLE_0X5
    {
        return None;
    }
    resolved_body_recipe_selection(scope, group, operands)
}

pub(super) fn resolved_profile_face_group(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    operands: &[DesignFaceOperand],
) -> Option<cadmpeg_ir::features::ProfileRef> {
    use cadmpeg_ir::features::ProfileRef;

    let selection =
        resolved_historical_face_group(scope, scope.previous_history_state_id(), group, operands)?;
    let cadmpeg_ir::features::FaceSelection::Historical {
        state,
        faces,
        native,
    } = selection
    else {
        return None;
    };
    Some(ProfileRef::Planar(
        cadmpeg_ir::features::PlanarProfileRef::HistoricalFaces {
            state,
            faces,
            native: vec![native.as_str().to_owned()].try_into().ok()?,
        },
    ))
}

/// Return the top-level profile groups of one Extrude operand hierarchy.
///
/// A profile group named by another profile group's member table is a child
/// selection, not an additional profile consumed by the Extrude. The complete
/// hierarchy must be acyclic, and each child can have exactly one parent.
pub(crate) fn extrude_profile_group_roots<'a>(
    ctx: Option<&DecodeContext<'_>>,
    scope: &DesignParameterScope,
    groups: &'a [DesignConstructionOperandGroup],
) -> Result<Option<Vec<&'a DesignConstructionOperandGroup>>, CodecError> {
    use crate::records::topology::extrude_selection::DesignExtrudeOperandRole;

    let Some(stream) = native_stream(&scope.id) else { return Ok(None); };
    let mut profile_groups = Vec::new();
    for group in groups.iter().filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
                && group.extrude_role() == Some(DesignExtrudeOperandRole::Profile)
        }) {
        push_face_item(ctx, &mut profile_groups, group, "f3d Extrude profile group")?;
    }
    profile_groups.sort_by_key(|group| group.scope_reference_ordinal);
    if profile_groups.windows(2).any(|groups| {
        groups[0].scope_reference_ordinal == groups[1].scope_reference_ordinal
            || groups[0].record_index == groups[1].record_index
    }) {
        return Ok(None);
    }

    let mut groups_by_record = HashMap::new();
    for group in &profile_groups {
        // discarded-value: duplicate record indices are rejected by the length check below.
        let _ = insert_face_map(ctx, &mut groups_by_record, group.record_index, *group,
            "f3d Extrude profile group index")?;
    }
    if groups_by_record.len() != profile_groups.len() {
        return Ok(None);
    }
    let mut parent_by_child = HashMap::new();
    for parent in &profile_groups {
        for member in parent.members().iter().map(|member| &member.value) {
            let Some(child) = groups_by_record.get(member) else {
                continue;
            };
            if child.scope_reference_ordinal <= parent.scope_reference_ordinal {
                return Ok(None);
            }
            if insert_face_map(ctx, &mut parent_by_child, child.record_index,
                parent.record_index, "f3d Extrude profile parent index")?.is_some() {
                return Ok(None);
            }
        }
    }
    let mut roots = Vec::new();
    for group in profile_groups.iter().copied()
        .filter(|group| !parent_by_child.contains_key(&group.record_index)) {
        push_face_item(ctx, &mut roots, group, "f3d Extrude profile root")?;
    }
    if !profile_groups.is_empty() && roots.is_empty() {
        return Ok(None);
    }

    let mut visited = HashSet::new();
    for root in &roots {
        if !visit_extrude_profile_group(ctx, root, &groups_by_record, &mut visited)? {
            return Ok(None);
        }
    }
    if visited.len() != profile_groups.len() { return Ok(None); }
    Ok(Some(roots))
}

fn visit_extrude_profile_group(
    ctx: Option<&DecodeContext<'_>>,
    group: &DesignConstructionOperandGroup,
    groups_by_record: &HashMap<u32, &DesignConstructionOperandGroup>,
    visited: &mut HashSet<u32>,
) -> Result<bool, CodecError> {
    let _depth = ctx.map(|ctx| ctx.enter_nested("f3d Extrude profile hierarchy"))
        .transpose()?;
    if let Some(ctx) = ctx {
        ctx.charge_work(1, "f3d Extrude profile hierarchy")?;
    }
    if !insert_face_set(ctx, visited, group.record_index,
        "f3d Extrude profile visited group")? {
        return Ok(false);
    }
    for member in group.members().iter().map(|member| &member.value) {
        if let Some(child) = groups_by_record.get(member) {
            if !visit_extrude_profile_group(ctx, child, groups_by_record, visited)? {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// Resolve each member of an Extrude profile group to one exact leaf operand.
///
/// Direct members name face operands. A member can instead name a child
/// profile group when that complete child hierarchy contains exactly one leaf
/// operand. This preserves the parent member cardinality used by historical
/// selection proofs.
pub(crate) fn extrude_profile_group_operand_indices(
    ctx: Option<&DecodeContext<'_>>,
    root: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignFaceOperand],
) -> Result<Option<Vec<usize>>, CodecError> {
    use crate::records::topology::extrude_selection::DesignExtrudeOperandRole;

    let Some(stream) = native_stream(&root.id) else { return Ok(None); };
    let mut profile_groups = Vec::new();
    for group in groups.iter().filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == root.scope_record_index
                && group.extrude_role() == Some(DesignExtrudeOperandRole::Profile)
        }) {
        push_face_item(ctx, &mut profile_groups, group, "f3d Extrude leaf profile group")?;
    }
    let mut groups_by_record = HashMap::new();
    for group in &profile_groups {
        // discarded-value: duplicate record indices are rejected by the length check below.
        let _ = insert_face_map(ctx, &mut groups_by_record, group.record_index, *group,
            "f3d Extrude leaf group index")?;
    }
    if groups_by_record.len() != profile_groups.len()
        || groups_by_record.get(&root.record_index).copied() != Some(root)
    {
        return Ok(None);
    }

    let mut visited_groups = HashSet::new();
    collect_extrude_profile_group_operands(
        ctx,
        root,
        stream,
        &groups_by_record,
        operands,
        &mut visited_groups,
    )
}

fn collect_extrude_profile_group_operands(
    ctx: Option<&DecodeContext<'_>>,
    group: &DesignConstructionOperandGroup,
    stream: &str,
    groups_by_record: &HashMap<u32, &DesignConstructionOperandGroup>,
    operands: &[DesignFaceOperand],
    visited_groups: &mut HashSet<u32>,
) -> Result<Option<Vec<usize>>, CodecError> {
    let _depth = ctx.map(|ctx| ctx.enter_nested("f3d Extrude leaf hierarchy"))
        .transpose()?;
    if let Some(ctx) = ctx { ctx.charge_work(1, "f3d Extrude leaf hierarchy")?; }
    if group.members().is_empty() || !insert_face_set(ctx, visited_groups,
        group.record_index, "f3d Extrude leaf visited group")? {
        return Ok(None);
    }
    let mut indices = Vec::new();
    for (ordinal, record_index) in group
        .members()
        .iter()
        .map(|member| &member.value)
        .enumerate()
    {
        let Some(ordinal) = u32::try_from(ordinal).ok() else { return Ok(None); };
        let mut direct = operands
            .iter()
            .enumerate()
            .filter(|(_, operand)| {
                native_stream(&operand.id) == Some(stream)
                    && operand.scope_record_index == group.scope_record_index
                    && operand.group_record_index() == Some(group.record_index)
                    && operand.group_member_ordinal() == Some(ordinal)
                    && operand.record_index() == *record_index
            })
            .map(|(index, _)| index);
        let first = direct.next();
        let second = direct.next();
        let child = groups_by_record.get(record_index).copied();
        let index = match ((first, second), child) {
            ((Some(index), None), None) => index,
            ((None, None), Some(child)) if child.scope_reference_ordinal > group.scope_reference_ordinal => {
                let child_indices = collect_extrude_profile_group_operands(
                    ctx,
                    child,
                    stream,
                    groups_by_record,
                    operands,
                    visited_groups,
                )?;
                let Some(child_indices) = child_indices else { return Ok(None); };
                let [index] = child_indices.as_slice() else {
                    return Ok(None);
                };
                *index
            }
            _ => return Ok(None),
        };
        if indices.contains(&index) {
            return Ok(None);
        }
        push_face_item(ctx, &mut indices, index, "f3d Extrude leaf index")?;
    }
    Ok(Some(indices))
}

/// Whether every root member is one exact paired-reference face subgroup.
pub(crate) fn is_paired_extrude_profile_aggregate(
    root: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignFaceOperand],
) -> bool {
    use crate::records::{
        recipes::ConstructionRecipeKind, topology::extrude_selection::DesignExtrudeOperandRole,
    };

    let Some(stream) = native_stream(&root.id) else {
        return false;
    };
    !root.members().is_empty()
        && root
            .members()
            .iter()
            .map(|member| &member.value)
            .all(|record_index| {
                let mut children = groups.iter().filter(|group| {
                    native_stream(&group.id) == Some(stream)
                        && group.scope_record_index == root.scope_record_index
                        && group.record_index == *record_index
                        && group.scope_reference_ordinal > root.scope_reference_ordinal
                        && group.extrude_role() == Some(DesignExtrudeOperandRole::Profile)
                });
                let Some(child) = children.next() else {
                    return false;
                };
                if children.next().is_some() {
                    return false;
                }
                let [crate::records::identity::Located {
                    value: operand_record_index,
                    ..
                }] = child.members()
                else {
                    return false;
                };
                let mut leaves =
                    operands.iter().filter(|operand| {
                        native_stream(&operand.id) == Some(stream)
                    && operand.scope_record_index == root.scope_record_index
                    && operand.group_record_index() == Some(child.record_index)
                    && operand.group_member_ordinal() == Some(0)
                    && operand.record_index() == *operand_record_index
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
    ctx: Option<&DecodeContext<'_>>,
    scope: &DesignParameterScope,
    root: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignFaceOperand],
) -> Result<Option<cadmpeg_ir::features::ProfileRef>, CodecError> {
    use cadmpeg_ir::features::ProfileRef;

    let Some(indices) = extrude_profile_group_operand_indices(ctx, root, groups, operands)? else {
        return Ok(None);
    };
    if let Some(faces) = resolved_extrude_profile_active_faces(ctx, &indices, operands)? {
        return Ok(Some(ProfileRef::Planar(
            cadmpeg_ir::features::PlanarProfileRef::Faces(faces),
        )));
    }
    let mut faces = Vec::new();
    for index in indices {
        let Some(operand) = operands.get(index) else { return Ok(None); };
        let slots = &operand.resolved_face_slots;
        if slots.is_empty() {
            return Ok(None);
        }
        for slot in slots {
            if !faces.contains(slot) {
                push_face_item(ctx, &mut faces, *slot, "f3d Extrude historical face slot")?;
            }
        }
    }
    let Some(selection) = historical_face_selection(scope, root, faces) else { return Ok(None); };
    let cadmpeg_ir::features::FaceSelection::Historical {
        state,
        faces,
        native,
    } = selection
    else {
        return Ok(None);
    };
    Ok(Some(cadmpeg_ir::features::ProfileRef::Planar(
        cadmpeg_ir::features::PlanarProfileRef::HistoricalFaces {
            state,
            faces,
            native: vec![copy_face_text(ctx, native.as_str(), "f3d Extrude historical group id")?]
                .try_into().map_err(CodecError::malformed)?,
        },
    )))
}

fn resolved_extrude_profile_active_faces(
    ctx: Option<&DecodeContext<'_>>,
    indices: &[usize],
    operands: &[DesignFaceOperand],
) -> Result<Option<Vec<cadmpeg_ir::ids::FaceId>>, CodecError> {
    let mut faces = Vec::new();
    for index in indices {
        let Some(operand) = operands.get(*index) else { return Ok(None); };
        if operand.recipe_kind != crate::records::recipes::ConstructionRecipeKind::BoundedFace
            || operand.candidate_faces.is_empty()
            || !operand.unreferenced_candidate_faces.is_empty()
            || !operand.alternate_selector_candidate_faces.is_empty()
            || !operand.preceding_candidate_faces.is_empty()
            || !operand.changed_candidate_faces.is_empty()
            || !operand.historical_support_contexts.is_empty()
            || !operand.resolved_face_slots.is_empty()
            || operand.resolved_active_face.is_some()
        {
            return Ok(None);
        }
        let Some(crate::design::decode::operands::FaceRecipeProgramKind::Counted { header_value }) =
            crate::design::decode::operands::face_recipe_program_kind(&operand.recipe_program)
        else {
            return Ok(None);
        };
        if operand.recipe_nodes.len() != header_value {
            return Ok(None);
        }
        for face in &operand.candidate_faces {
            if !faces.contains(face) {
                let face = copy_face_id(ctx, face, "f3d Extrude active face id")?;
                push_face_item(ctx, &mut faces, face, "f3d Extrude active face")?;
            }
        }
    }
    Ok((!faces.is_empty()).then_some(faces))
}

/// Resolve a Loft section whose members use the edge-recipe envelope.
///
/// These members describe the selected face through a common persistent
/// selector/token clause. The clause must identify one direct preceding face
/// in every member, and its complete boundary must have exactly one edge per
/// group member. Any competing common clause or incomplete topology context
/// keeps the native group unresolved.
pub(super) fn resolved_loft_edge_profile_group(
    ctx: Option<&DecodeContext<'_>>,
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    operands: &[DesignEdgeOperand],
) -> Result<Option<cadmpeg_ir::features::ProfileRef>, CodecError> {
    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::Loft
        || !matches!(
            group.role(),
            DesignOperandRole::PROFILE | DesignOperandRole::ROLE_0X43
        )
        || group.members().is_empty()
        || !group.lost_edge_references.is_empty()
    {
        return Ok(None);
    }
    let Some(previous_state_id) = scope.previous_history_state_id() else { return Ok(None); };
    let Some(stream) = native_stream(&group.id) else { return Ok(None); };
    let Some(group_ordinal) = usize::try_from(group.scope_reference_ordinal).ok() else { return Ok(None); };
    let mut member_ids = HashSet::new();
    for member in group.members().iter().map(|member| member.value) {
        if !insert_face_set(ctx, &mut member_ids, member,
            "f3d Loft edge profile member index")? {
            return Ok(None);
        }
    }
    if scope.reference_members().values().nth(group_ordinal) != Some(&group.record_index) {
        return Ok(None);
    }
    let mut member_operands = Vec::new();
    for (ordinal, record_index) in group
        .members()
        .iter()
        .map(|member| &member.value)
        .enumerate()
    {
        let Some(operand) = (|| {
            let scope_ordinal = group
                .scope_reference_ordinal
                .checked_add(1)?
                .checked_add(u32::try_from(ordinal).ok()?)?;
            if scope
                .reference_members()
                .values()
                .nth(group_ordinal.checked_add(ordinal.checked_add(1)?)?)
                != Some(record_index)
            {
                return None;
            }
            let mut matches = operands.iter().filter(|operand| {
                native_stream(&operand.id) == Some(stream)
                    && operand.scope_record_index == group.scope_record_index
                    && operand.record_index() == *record_index
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
        })() else { return Ok(None); };
        push_face_item(ctx, &mut member_operands, operand,
            "f3d Loft edge profile member")?;
    }
    let Some(face_slot) = loft_edge_profile_face_slot(group.members().len(), &member_operands) else {
        return Ok(None);
    };
    let Some(selection) = historical_face_selection(scope, group, vec![face_slot]) else {
        return Ok(None);
    };
    let cadmpeg_ir::features::FaceSelection::Historical {
        state,
        faces,
        native,
    } = selection
    else {
        return Ok(None);
    };
    Ok(Some(cadmpeg_ir::features::ProfileRef::Planar(
        cadmpeg_ir::features::PlanarProfileRef::HistoricalFaces {
            state,
            faces,
            native: vec![copy_face_text(ctx, native.as_str(),
                "f3d Loft historical group id")?]
                .try_into().map_err(CodecError::malformed)?,
        },
    )))
}

fn loft_edge_profile_face_slot(
    member_count: usize,
    operands: &[&DesignEdgeOperand],
) -> Option<i64> {
    if member_count == 0 || operands.len() != member_count {
        return None;
    }
    let mut common_clause = None;
    for reference in &operands.first()?.recipe_references {
        if reference.candidate_faces.len() != 1 || !reference.alternate_selector_faces.is_empty() {
            continue;
        }
        if !operands.iter().all(|operand| {
            operand.recipe_references.iter().filter(|candidate| {
                candidate.selector == reference.selector
                    && candidate.token == reference.token
                    && candidate.design_reference == reference.design_reference
                    && candidate.candidate_faces.len() == 1
                    && candidate.alternate_selector_faces.is_empty()
            }).count() == 1
        }) {
            continue;
        }
        if let Some(prior) = common_clause {
            if prior != (reference.selector, &reference.token, reference.design_reference) {
                return None;
            }
        } else {
            common_clause = Some((reference.selector, &reference.token, reference.design_reference));
        }
    }
    let (selector, token, design_reference) = common_clause?;
    let mut evidence = None;
    for operand in operands {
        let (candidate, slot, count) = (|| {
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
                    reference.selector == selector
                        && reference.token == *token
                        && reference.design_reference == design_reference
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
                let slot = preceding_face
                    .as_str()
                    .rsplit_once('#')?
                    .1
                    .parse::<i64>()
                    .ok()?;
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
                    target = Some((candidate, slot, boundary_edge_count));
                }
            }
            target
        })()?;
        if count != member_count {
            return None;
        }
        if let Some((face, prior_slot, prior_count)) = evidence {
            if candidate != face || slot != prior_slot || count != prior_count {
                return None;
            }
        } else {
            evidence = Some((candidate, slot, count));
        }
    }
    Some(evidence?.1)
}

pub(super) fn resolved_historical_face_group(
    scope: &DesignParameterScope,
    previous_state_id: Option<i64>,
    group: &DesignConstructionOperandGroup,
    operands: &[DesignFaceOperand],
) -> Option<cadmpeg_ir::features::FaceSelection> {
    let faces = historical_face_group_slots(group, operands, FaceGroupMembers::Resolved)?;
    historical_face_selection_in_state(scope, group, previous_state_id?, faces)
}

fn historical_face_selection(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    faces: Vec<i64>,
) -> Option<cadmpeg_ir::features::FaceSelection> {
    let previous_state_id = scope.previous_history_state_id()?;
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
    let feature_key = feature.key();
    Some(
        FaceSelection::historical(
            feature_input_topology_id(&feature, previous_state_id),
            faces
                .into_iter()
                .map(|face| {
                    ids::history_input_face_id(
                        &ids::history_input_prefix(&feature_key, previous_state_id),
                        face,
                    )
                })
                .collect(),
            native.clone(),
        )
        .unwrap_or(FaceSelection::Native(native)),
    )
}

/// Resolve `SplitFace` target groups whose bounded-face member run can include
/// complete nested support recipes without their own active candidate lanes.
/// A nested support recipe contributes only when history proves its preceding
/// face slots. An unresolved support recipe remains a context member. Every
/// other member must prove its preceding face slots, and at least one member
/// must contribute a face.
pub(super) fn resolved_historical_split_face_target_group(
    scope: &DesignParameterScope,
    previous_state_id: Option<i64>,
    group: &DesignConstructionOperandGroup,
    operands: &[DesignFaceOperand],
) -> Option<cadmpeg_ir::features::FaceSelection> {
    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::SplitFace
        || group.role() != DesignOperandRole::ROLE_0X10
    {
        return None;
    }
    let faces = historical_face_group_slots(group, operands, FaceGroupMembers::SplitFaceContext)?;
    historical_face_selection_in_state(scope, group, previous_state_id?, faces)
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
    previous_state_id: Option<i64>,
    group: &DesignConstructionOperandGroup,
    operands: &[DesignFaceOperand],
    updated_face_slots: &[i64],
) -> Option<cadmpeg_ir::features::FaceSelection> {
    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::SplitFace
        || group.role() != DesignOperandRole::ROLE_0X10
    {
        return None;
    }
    resolved_historical_split_face_target_group(scope, previous_state_id, group, operands).or_else(
        || {
            let faces =
                split_face_updated_target_slots(scope, group, operands, updated_face_slots)?;
            historical_face_selection_in_state(scope, group, previous_state_id?, faces)
        },
    )
}

fn split_face_updated_target_slots(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    operands: &[DesignFaceOperand],
    updated_face_slots: &[i64],
) -> Option<Vec<i64>> {
    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::SplitFace
        || group.role() != DesignOperandRole::ROLE_0X10
        || updated_face_slots.is_empty()
        || updated_face_slots.len() != group.members().len()
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
    for (ordinal, record_index) in group
        .members()
        .iter()
        .map(|member| &member.value)
        .enumerate()
    {
        let ordinal = u32::try_from(ordinal).ok()?;
        let mut matches = operands.iter().filter(|operand| {
            native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == group.scope_record_index
                && operand.group_record_index() == Some(group.record_index)
                && operand.group_member_ordinal() == Some(ordinal)
                && operand.record_index() == *record_index
                && operand.recipe_kind
                    == crate::records::recipes::ConstructionRecipeKind::BoundedFace
        });
        let operand = matches.next()?;
        if matches.next().is_some() || operand.preceding_candidate_faces.is_empty() {
            return None;
        }
        for face in &operand.preceding_candidate_faces {
            let slot = face
                .as_str()
                .rsplit_once('#')
                .and_then(|(_, slot)| slot.parse().ok())?;
            if updated.contains(&slot) && represented.insert(slot) {
                faces.push(slot);
            }
        }
    }
    (represented == updated).then_some(faces)
}

#[derive(Clone, Copy)]
enum FaceGroupMembers {
    Resolved,
    SplitFaceContext,
}

fn historical_face_group_slots(
    group: &DesignConstructionOperandGroup,
    operands: &[DesignFaceOperand],
    members: FaceGroupMembers,
) -> Option<Vec<i64>> {
    let stream = native_stream(&group.id)?;
    let mut faces = Vec::with_capacity(group.members().len());
    let mut contributing_members = 0;
    for (ordinal, record_index) in group
        .members()
        .iter()
        .map(|member| &member.value)
        .enumerate()
    {
        let ordinal = u32::try_from(ordinal).ok()?;
        let mut matches = operands.iter().filter(|operand| {
            native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == group.scope_record_index
                && operand.group_record_index() == Some(group.record_index)
                && operand.group_member_ordinal() == Some(ordinal)
                && operand.record_index() == *record_index
        });
        let operand = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        let member_slots = if operand.resolved_face_slots.is_empty() {
            if matches!(members, FaceGroupMembers::SplitFaceContext) {
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
    let faces = if let Some(face) = &operand.resolved_active_face {
        std::slice::from_ref(face)
    } else {
        let candidates = face_operand_candidates(operand);
        if operand.alternate_selector_candidate_faces.is_empty()
            && operand.unreferenced_candidate_faces.is_empty()
            && candidates.len() != 1
        {
            return None;
        }
        candidates
    };
    let slots = faces
        .iter()
        .map(|face| face.as_str().rsplit_once('#')?.1.parse::<i64>().ok())
        .collect::<Option<Vec<_>>>()?;
    let preceding = operand
        .preceding_candidate_faces
        .iter()
        .filter_map(|face| face.as_str().rsplit_once('#')?.1.parse::<i64>().ok())
        .collect::<HashSet<_>>();
    (!slots.is_empty() && slots.iter().all(|slot| preceding.contains(slot))).then_some(slots)
}

fn is_split_face_context_member(operand: &DesignFaceOperand) -> bool {
    operand.recipe_kind == crate::records::recipes::ConstructionRecipeKind::BoundedFace
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

fn resolved_face_operand(
    ctx: Option<&DecodeContext<'_>>,
    operand: &DesignFaceOperand,
) -> Result<Option<Vec<cadmpeg_ir::ids::FaceId>>, CodecError> {
    if let Some(face) = &operand.resolved_active_face {
        let mut faces = Vec::new();
        let face = copy_face_id(ctx, face, "f3d resolved active face id")?;
        push_face_item(ctx, &mut faces, face, "f3d resolved active face")?;
        return Ok(Some(faces));
    }
    if !operand.resolved_face_slots.is_empty() {
        let active_candidates = || operand.candidate_faces.iter()
            .chain(&operand.unreferenced_candidate_faces)
            .chain(&operand.alternate_selector_candidate_faces);
        let mut faces = Vec::new();
        for slot in &operand.resolved_face_slots {
            let face = if active_candidates().next().is_none() {
                face_slot_id(ctx, *slot)?
            } else {
                let Some(face) = active_candidates().find(|face| {
                    face.as_str()
                        .rsplit_once('#')
                        .and_then(|(_, ordinal)| ordinal.parse::<i64>().ok())
                        == Some(*slot)
                }) else { return Ok(None); };
                copy_face_id(ctx, face, "f3d resolved face slot candidate id")?
            };
            push_face_item(ctx, &mut faces, face, "f3d resolved face slot")?;
        }
        return Ok(Some(faces));
    }
    let candidates = face_operand_candidates(operand);
    if !operand.alternate_selector_candidate_faces.is_empty() {
        let mut faces = Vec::new();
        for face in candidates {
            let face = copy_face_id(ctx, face, "f3d alternate face candidate id")?;
            push_face_item(ctx, &mut faces, face, "f3d alternate face candidate")?;
        }
        return Ok(Some(faces));
    }
    if operand.recipe_kind == crate::records::recipes::ConstructionRecipeKind::Face {
        let mut referenced = Vec::new();
        for reference in &operand.recipe_references {
            for face in &reference.candidate_faces {
                if !referenced.contains(face) {
                    let face = copy_face_id(ctx, face, "f3d referenced face id")?;
                    push_face_item(ctx, &mut referenced, face, "f3d referenced face")?;
                }
            }
        }
        if !referenced.is_empty() {
            return Ok(Some(referenced));
        }
    }
    if !operand.unreferenced_candidate_faces.is_empty()
        && complete_counted_face_recipe(operand).is_some()
    {
        let mut faces = Vec::new();
        for face in candidates {
            let face = copy_face_id(ctx, face, "f3d unreferenced face candidate id")?;
            push_face_item(ctx, &mut faces, face, "f3d unreferenced face candidate")?;
        }
        return Ok(Some(faces));
    }
    let [face] = candidates else { return Ok(None) };
    let mut faces = Vec::new();
    let face = copy_face_id(ctx, face, "f3d unique face candidate id")?;
    push_face_item(ctx, &mut faces, face, "f3d unique face candidate")?;
    Ok(Some(faces))
}

fn explicit_bounded_face_candidates(
    ctx: Option<&DecodeContext<'_>>,
    operand: &DesignFaceOperand,
) -> Result<Option<Vec<cadmpeg_ir::ids::FaceId>>, CodecError> {
    if operand.recipe_kind != crate::records::recipes::ConstructionRecipeKind::BoundedFace
        || !operand.resolved_face_slots.is_empty()
        || !operand.alternate_selector_candidate_faces.is_empty()
        || !operand.historical_support_contexts.is_empty()
        || complete_counted_face_recipe(operand).is_none()
    {
        return Ok(None);
    }
    if !operand.candidate_faces.is_empty() && operand.unreferenced_candidate_faces.is_empty() {
        let mut candidates = Vec::new();
        for face in &operand.candidate_faces {
            let face = copy_face_id(ctx, face, "f3d explicit bounded face candidate id")?;
            push_face_item(ctx, &mut candidates, face, "f3d explicit bounded face candidate")?;
        }
        return Ok(Some(candidates));
    }

    let mut lanes = HashMap::<i64, Vec<cadmpeg_ir::ids::FaceId>>::new();
    for reference in &operand.recipe_references {
        if operand.recipe_references.iter().any(|other| {
            other.design_reference == reference.design_reference
                && !other.alternate_selector_faces.is_empty()
        }) {
            continue;
        }
        for face in &reference.candidate_faces {
            if operand.candidate_faces.is_empty() || operand.candidate_faces.contains(face) {
                if !lanes.contains_key(&reference.design_reference) {
                    if let Some(ctx) = ctx {
                        ctx.charge_collection_items(1, "f3d explicit bounded face lane")?;
                        lanes.try_reserve(1).map_err(|_| {
                            ctx.refuse_codec_limit("f3d explicit bounded face lane", 0, 1)
                        })?;
                    }
                }
                let lane = lanes.entry(reference.design_reference).or_default();
                if !lane.contains(face) {
                    let face = copy_face_id(ctx, face, "f3d explicit bounded face lane id")?;
                    push_face_item(ctx, lane, face, "f3d explicit bounded face lane member")?;
                }
            }
        }
    }
    let mut ordered_lanes = Vec::new();
    for lane in lanes.into_values().filter(|lane| !lane.is_empty()) {
        push_face_item(ctx, &mut ordered_lanes, lane, "f3d explicit bounded face ordered lane")?;
    }
    for lane in &mut ordered_lanes {
        lane.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    }
    ordered_lanes.sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));
    let [lane, next @ ..] = ordered_lanes.as_slice() else {
        return Ok(None);
    };
    if next.first().is_some_and(|other| other.len() == lane.len()) {
        return Ok(None);
    }
    let mut selected = Vec::new();
    for face in lane {
        let face = copy_face_id(ctx, face, "f3d explicit bounded selected face id")?;
        push_face_item(ctx, &mut selected, face, "f3d explicit bounded selected face")?;
    }
    Ok(Some(selected))
}

fn complete_counted_face_recipe(operand: &DesignFaceOperand) -> Option<usize> {
    if operand.recipe_kind != crate::records::recipes::ConstructionRecipeKind::BoundedFace {
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
    if operand.recipe_kind != crate::records::recipes::ConstructionRecipeKind::BoundedFace
        || !matches!(
            crate::design::decode::operands::face_recipe_program_kind(&operand.recipe_program),
            Some(crate::design::decode::operands::FaceRecipeProgramKind::Counted { .. })
        )
        || operand.recipe_nodes.is_empty()
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
    candidates.sort_by(|left, right| left.as_str().cmp(right.as_str()));
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
    direct.as_str().rsplit_once('#')?.1.parse().ok()
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
        .then(|| direct.as_str().rsplit_once('#')?.1.parse().ok())
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
    if operand.recipe_kind != crate::records::recipes::ConstructionRecipeKind::BoundedFace {
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
        let slot = face.as_str().rsplit_once('#')?.1.parse::<i64>().ok()?;
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
        .map(|face| face.as_str().rsplit_once('#')?.1.parse::<i64>().ok())
        .collect::<Option<Vec<_>>>()?;
    if slots.iter().any(|slot| *slot < 0) {
        return None;
    }
    slots.sort_unstable();
    let unique = slots.windows(2).all(|pair| pair[0] != pair[1]);
    unique.then_some(slots)
}

fn counted_face_recipe_frame(operand: &DesignFaceOperand) -> Option<usize> {
    if operand.recipe_kind != crate::records::recipes::ConstructionRecipeKind::BoundedFace {
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
    contexts: &[crate::records::topology::historical_context::DesignHistoricalFaceSupportContext],
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
    contexts: &[crate::records::topology::historical_context::DesignHistoricalFaceSupportContext],
) -> Option<Vec<i64>> {
    let mut candidate_slots = candidates
        .iter()
        .map(|face| face.as_str().rsplit_once('#')?.1.parse::<i64>().ok())
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
    support_contexts: &[crate::records::topology::historical_context::DesignHistoricalFaceSupportContext],
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
    contexts: &[crate::records::topology::historical_context::DesignHistoricalFaceSupportContext],
) -> Option<Vec<i64>> {
    let same_faces = |first: &[&crate::records::topology::historical_context::DesignHistoricalFaceBoundaryContext],
                      second: &[&crate::records::topology::historical_context::DesignHistoricalFaceBoundaryContext]| {
        first.iter().map(|boundary| boundary.face_slot)
            .eq(second.iter().map(|boundary| boundary.face_slot))
    };
    let mut selected: Option<Vec<&crate::records::topology::historical_context::DesignHistoricalFaceBoundaryContext>> = None;
    for context in contexts {
        let Some(boundaries) = valid_preceding_face_boundaries(context) else { continue; };
        if boundary_edge_count(&boundaries) != Some(header_value) {
            continue;
        }
        if selected.as_ref().is_some_and(|first| !same_faces(first, &boundaries)) {
            return None;
        }
        selected.get_or_insert(boundaries);
    }
    if let Some(boundaries) = unique_preceding_face_boundaries(contexts) {
        if boundary_edge_count(&boundaries) == Some(header_value) {
            if selected.as_ref().is_some_and(|first| !same_faces(first, &boundaries)) {
                return None;
            }
            selected.get_or_insert(boundaries);
        }
    }
    Some(selected?.iter().map(|boundary| boundary.face_slot).collect())
}

fn valid_preceding_face_boundaries(
    context: &crate::records::topology::historical_context::DesignHistoricalFaceSupportContext,
) -> Option<Vec<&crate::records::topology::historical_context::DesignHistoricalFaceBoundaryContext>>
{
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
    contexts: &[crate::records::topology::historical_context::DesignHistoricalFaceSupportContext],
) -> Option<Vec<&crate::records::topology::historical_context::DesignHistoricalFaceBoundaryContext>>
{
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
    boundaries: &[&crate::records::topology::historical_context::DesignHistoricalFaceBoundaryContext],
) -> Option<usize> {
    boundaries.iter().try_fold(0usize, |total, boundary| {
        boundary.loops.iter().try_fold(total, |total, loop_| {
            let count = loop_.boundary.coedges().count();
            (count != 0).then(|| total.checked_add(count)).flatten()
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
        .filter_map(|face| face.as_str().rsplit_once('#')?.1.parse::<i64>().ok())
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
    if operand.recipe_kind == crate::records::recipes::ConstructionRecipeKind::Face {
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
        referenced.sort_by(|left, right| left.as_str().cmp(right.as_str()));
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
    candidates.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    candidates.dedup();
    (!candidates.is_empty()).then_some(candidates)
}

fn has_nested_bounded_face_history_candidates(operand: &DesignFaceOperand) -> bool {
    complete_counted_face_recipe(operand).is_some()
        && operand.candidate_faces.is_empty()
        && operand.unreferenced_candidate_faces.is_empty()
        && operand.alternate_selector_candidate_faces.is_empty()
        && operand.recipe_references.iter().any(|reference| {
            !reference.candidate_faces.is_empty()
                || !reference.alternate_selector_faces.is_empty()
        })
}

/// Return active B-rep faces for the legacy `FromFace` envelope whose counted
/// bounded recipe contains support references but no active face lane.
///
/// This is a candidate-generation proof, not a selection proof. The caller
/// must reduce the returned faces to one plane coincident with the profile
/// sketch before binding it to the operand.
fn extrude_start_plane_geometry_candidates(
    ctx: Option<&DecodeContext<'_>>,
    group: &DesignConstructionOperandGroup,
    operands: &[DesignFaceOperand],
    faces: &[cadmpeg_ir::topology::Face],
) -> Result<Option<Vec<cadmpeg_ir::ids::FaceId>>, CodecError> {
    let [crate::records::identity::Located {
        value: record_index,
        ..
    }] = group.members()
    else {
        return Ok(None);
    };
    let mut matching = operands.iter().filter(|operand| {
        native_stream(&operand.id) == native_stream(&group.id)
            && operand.scope_record_index == group.scope_record_index
            && operand.record_index() == *record_index
    });
    let Some(operand) = matching.next() else { return Ok(None); };
    if matching.next().is_some()
        || !face_operand_candidates(operand).is_empty()
        || !operand.resolved_face_slots.is_empty()
        || operand.resolved_active_face.is_some()
        || !has_nested_bounded_face_history_candidates(operand)
    {
        return Ok(None);
    }
    let mut candidates = Vec::new();
    for face in faces {
        let id = copy_face_id(ctx, &face.id, "f3d start plane candidate ID")?;
        push_face_item(ctx, &mut candidates, id, "f3d start plane candidate")?;
    }
    Ok(Some(candidates))
}

fn extrude_profile_sketch_id(
    profile: &cadmpeg_ir::features::ProfileRef,
) -> Option<&cadmpeg_ir::sketches::SketchId> {
    use cadmpeg_ir::features::ProfileRef;

    match profile {
        ProfileRef::Planar(
            cadmpeg_ir::features::PlanarProfileRef::Sketch(sketch)
            | cadmpeg_ir::features::PlanarProfileRef::SketchProfiles { sketch, .. }
            | cadmpeg_ir::features::PlanarProfileRef::SketchRegions { sketch, .. }
            | cadmpeg_ir::features::PlanarProfileRef::SketchEntities { sketch, .. }
            | cadmpeg_ir::features::PlanarProfileRef::SketchSelection { sketch, .. },
        ) => Some(sketch),
        ProfileRef::Planar(
            cadmpeg_ir::features::PlanarProfileRef::Native(_)
            | cadmpeg_ir::features::PlanarProfileRef::Unresolved(_)
            | cadmpeg_ir::features::PlanarProfileRef::Feature(_)
            | cadmpeg_ir::features::PlanarProfileRef::Generated { .. }
            | cadmpeg_ir::features::PlanarProfileRef::HistoricalFaces { .. }
            | cadmpeg_ir::features::PlanarProfileRef::Faces(_),
        )
        | ProfileRef::SpatialSketchProfiles { .. }
        | ProfileRef::SpatialSketchSelection { .. } => None,
    }
}

/// Geometry and native records used to resolve selected-face Extrude inputs.
pub(crate) struct ExtrudeFaceResolution<'a> {
    pub(crate) faces: &'a [cadmpeg_ir::topology::Face],
    pub(crate) surfaces: &'a [cadmpeg_ir::geometry::Surface],
    pub(crate) groups: &'a [DesignConstructionOperandGroup],
    pub(crate) operands: &'a mut [DesignFaceOperand],
    pub(crate) linear_tolerance: f64,
    pub(crate) angular_tolerance: f64,
}

pub(crate) fn bind_extrude_start_planes(
    ctx: Option<&DecodeContext<'_>>,
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    resolution: &mut ExtrudeFaceResolution<'_>,
) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{ExtrudeStart, FaceSelection, FeatureDefinition, FeatureOperation};

    for feature in features {
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| {
        edit_result = (|| -> Result<(), CodecError> {
        'feature_edit: {
            let FeatureDefinition::Operation(FeatureOperation::Extrude { profile, start, .. }) =
                definition
            else {
                break 'feature_edit;
            };
            let Some(sketch_id) = extrude_profile_sketch_id(profile) else {
                break 'feature_edit;
            };
            let Some(sketch) = sketches.iter().find(|sketch| sketch.id == *sketch_id) else {
                break 'feature_edit;
            };
            let ExtrudeStart::FromFace {
                face: FaceSelection::Native(native),
                offset,
            } = start
            else {
                break 'feature_edit;
            };
            let retained_offset = *offset;
            let mut matching_groups = resolution.groups.iter().filter(|group| group.id == *native);
            let Some(group) = matching_groups.next() else {
                break 'feature_edit;
            };
            if matching_groups.next().is_some()
                || group.extrude_face_role() != Some(DesignExtrudeFaceRole::Start)
            {
                break 'feature_edit;
            }
            let Some(stream) = native_stream(&group.id) else {
                break 'feature_edit;
            };
            let mut candidates = Vec::new();
            for record_index in group.members().iter().map(|member| &member.value) {
                let mut matching_operands = resolution.operands.iter().filter(|operand| {
                    native_stream(&operand.id) == Some(stream)
                        && operand.scope_record_index == group.scope_record_index
                        && operand.record_index() == *record_index
                });
                let Some(operand) = matching_operands.next() else {
                    candidates.clear();
                    break;
                };
                if matching_operands.next().is_some() {
                    candidates.clear();
                    break;
                }
                for face in face_operand_candidates(operand) {
                    let id = copy_face_id(ctx, face, "f3d start plane operand face ID")?;
                    push_face_item(ctx, &mut candidates, id,
                        "f3d start plane operand candidate")?;
                }
            }
            candidates.sort_by(|left, right| left.as_str().cmp(right.as_str()));
            candidates.dedup();
            if candidates.is_empty() {
                if let Some(geometry_candidates) = extrude_start_plane_geometry_candidates(
                    ctx,
                    group,
                    resolution.operands,
                    resolution.faces,
                )? {
                    candidates = geometry_candidates;
                }
            }
            let mut coincident = Vec::new();
            for candidate in candidates {
                if face_coincident_with_sketch(
                    &candidate,
                    sketch,
                    resolution.faces,
                    resolution.surfaces,
                    resolution.linear_tolerance,
                    resolution.angular_tolerance,
                ) {
                    push_face_item(ctx, &mut coincident, candidate,
                        "f3d coincident start plane face")?;
                }
            }
            if let [face] = coincident.as_slice() {
                let selected = copy_face_id(ctx, face, "f3d selected start plane face ID")?;
                let native = copy_face_text(ctx, native, "f3d selected start plane native ID")?;
                if retain_face_operand_resolution(ctx, group, resolution.operands, face)? {
                    *start = ExtrudeStart::FromFace {
                        face: FaceSelection::Resolved {
                            faces: vec![selected],
                            native,
                        },
                        offset: retained_offset,
                    };
                }
            }
        }
        Ok(())
        })();
        });
        edit_result?;
    }
    Ok(())
}

/// Resolve a legacy Extrude target face from a unique forward planar face.
///
/// Some bounded-face target operands retain only support references after
/// history projection. A target face is still exact when one referenced face
/// is planar, parallel to the sweep direction, and lies strictly ahead of
/// the profile plane. Ambiguous, nonplanar, and non-forward candidates remain
/// native.
pub(crate) fn bind_extrude_target_faces(
    ctx: Option<&DecodeContext<'_>>,
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    resolution: &mut ExtrudeFaceResolution<'_>,
) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{
        ExtrudeDirection, ExtrudeExtent, FeatureDefinition, FeatureOperation,
    };

    for feature in features {
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| {
        edit_result = (|| -> Result<(), CodecError> {
        'feature_edit: {
            let FeatureDefinition::Operation(FeatureOperation::Extrude {
                profile,
                direction,
                extent,
                ..
            }) = definition
            else {
                break 'feature_edit;
            };
            let Some(sketch_id) = extrude_profile_sketch_id(profile) else {
                break 'feature_edit;
            };
            let Some(sketch) = sketches.iter().find(|sketch| sketch.id == *sketch_id) else {
                break 'feature_edit;
            };
            let Some((sketch_origin, profile_normal, _)) = sketch.resolved_placement() else {
                break 'feature_edit;
            };
            let sweep_direction = match direction {
                ExtrudeDirection::ProfileNormal {} => profile_normal,
                ExtrudeDirection::ReversedProfileNormal {} => profile_normal.negated(),
                ExtrudeDirection::Explicit { vector, .. } => (*vector).into(),
                ExtrudeDirection::Unresolved {} => break 'feature_edit,
            };
            let (sketch_origin, sweep_direction) = (sketch_origin.get(), sweep_direction.get());
            if sweep_direction.norm() <= 0.0 {
                break 'feature_edit;
            }
            match extent {
                ExtrudeExtent::OneSided { side } => bind_extrude_target_face(
                    ctx,
                    &mut side.termination,
                    sketch_origin,
                    sweep_direction,
                    resolution,
                )?,
                ExtrudeExtent::TwoSided { first, second } => {
                    bind_extrude_target_face(
                        ctx,
                        &mut first.termination,
                        sketch_origin,
                        sweep_direction,
                        resolution,
                    )?;
                    bind_extrude_target_face(
                        ctx,
                        &mut second.termination,
                        sketch_origin,
                        sweep_direction.scale(-1.0),
                        resolution,
                    )?;
                }
                ExtrudeExtent::Symmetric { .. } => {}
            }
        }
        Ok(())
        })();
        });
        edit_result?;
    }
    Ok(())
}

fn bind_extrude_target_face(
    ctx: Option<&DecodeContext<'_>>,
    termination: &mut cadmpeg_ir::features::LinearTermination,
    sketch_origin: Point3,
    sweep_direction: Vector3,
    resolution: &mut ExtrudeFaceResolution<'_>,
) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{FaceSelection, LinearTermination};

    let LinearTermination::ToFace {
        face: FaceSelection::Native(native),
        offset: retained_offset,
    } = termination
    else {
        return Ok(());
    };
    let offset = *retained_offset;
    let mut matching_groups = resolution.groups.iter().filter(|group| {
        group.id == *native && group.extrude_face_role() == Some(DesignExtrudeFaceRole::Termination)
    });
    let Some(group) = matching_groups.next() else {
        return Ok(());
    };
    if matching_groups.next().is_some() {
        return Ok(());
    }
    let Some(face) = extrude_target_plane_candidate(
        ctx, group, resolution, sketch_origin, sweep_direction)?
    else {
        return Ok(());
    };
    let native = copy_face_text(ctx, native, "f3d target face native ID")?;
    if retain_face_operand_resolution(ctx, group, resolution.operands, &face)? {
        *termination = LinearTermination::ToFace {
            face: FaceSelection::Resolved {
                faces: vec![face],
                native,
            },
            offset,
        };
    }
    Ok(())
}

fn extrude_target_plane_candidate(
    ctx: Option<&DecodeContext<'_>>,
    group: &DesignConstructionOperandGroup,
    resolution: &ExtrudeFaceResolution<'_>,
    sketch_origin: Point3,
    sweep_direction: Vector3,
) -> Result<Option<cadmpeg_ir::ids::FaceId>, CodecError> {
    let [crate::records::identity::Located {
        value: record_index,
        ..
    }] = group.members()
    else {
        return Ok(None);
    };
    let Some(stream) = native_stream(&group.id) else { return Ok(None); };
    let mut matching_operands = resolution.operands.iter().filter(|operand| {
        native_stream(&operand.id) == Some(stream)
            && operand.scope_record_index == group.scope_record_index
            && operand.record_index() == *record_index
    });
    let Some(operand) = matching_operands.next() else { return Ok(None); };
    if matching_operands.next().is_some() {
        return Ok(None);
    }
    let direction_length = sweep_direction.norm();
    let mut found = None;
    let mut ambiguous = false;
    let mut consider = |candidate: &cadmpeg_ir::ids::FaceId| {
            let Some((index, face)) = resolution.faces.iter().enumerate()
                .find(|(_, face)| face.id == *candidate) else { return; };
            let Some(surface) = resolution.surfaces.iter()
                .find(|surface| surface.id == face.surface) else { return; };
            let Some(SolvedSurfaceGeometry::Plane(plane_surface)) = surface.geometry.solved()
            else {
                return;
            };
            let origin = plane_surface.origin().get();
            let normal = plane_surface.frame().axis().as_raw();
            if !parallel_vectors(*normal, sweep_direction, resolution.angular_tolerance) {
                return;
            }
            let distance =
                origin.vector_from(sketch_origin).dot(sweep_direction) / direction_length;
            if distance > resolution.linear_tolerance {
                if found.is_some_and(|previous| previous != index) {
                    ambiguous = true;
                } else {
                    found = Some(index);
                }
            }
    };
    let direct = face_operand_candidates(operand);
    if direct.is_empty() {
        if !has_nested_bounded_face_history_candidates(operand) {
            return Ok(None);
        }
        for reference in &operand.recipe_references {
            for candidate in reference.candidate_faces.iter()
                .chain(&reference.alternate_selector_faces) {
                consider(candidate);
            }
        }
    } else {
        for candidate in direct {
            consider(candidate);
        }
    }
    if ambiguous {
        return Ok(None);
    }
    let Some(index) = found else { return Ok(None); };
    Ok(Some(copy_face_id(ctx, &resolution.faces[index].id,
        "f3d target plane face ID")?))
}

pub(super) fn retain_face_operand_resolution(
    ctx: Option<&DecodeContext<'_>>,
    group: &DesignConstructionOperandGroup,
    operands: &mut [DesignFaceOperand],
    face: &cadmpeg_ir::ids::FaceId,
) -> Result<bool, CodecError> {
    let Some(stream) = native_stream(&group.id) else {
        return Ok(false);
    };
    let mut matches = operands.iter_mut().filter(|operand| {
        native_stream(&operand.id) == Some(stream)
            && operand.scope_record_index == group.scope_record_index
            && group
                .members()
                .iter()
                .any(|member| member.value == operand.record_index())
            && (face_operand_candidates(operand).contains(face)
                || (face_operand_candidates(operand).is_empty()
                    && operand.resolved_face_slots.is_empty()
                    && operand.resolved_active_face.is_none()
                    && has_nested_bounded_face_history_candidates(operand)))
    });
    let Some(operand) = matches.next() else {
        return Ok(false);
    };
    if matches.next().is_some() {
        return Ok(false);
    }
    let geometry_bound = face_operand_candidates(operand).is_empty()
        && operand.resolved_face_slots.is_empty()
        && operand.resolved_active_face.is_none()
        && has_nested_bounded_face_history_candidates(operand);
    if geometry_bound {
        operand.resolved_active_face = Some(copy_face_id(ctx, face,
            "f3d retained operand active face")?);
        return Ok(true);
    }
    let Some(slot) = face
        .as_str()
        .rsplit_once('#')
        .and_then(|(_, slot)| slot.parse::<i64>().ok())
    else {
        return Ok(false);
    };
    if !operand.resolved_face_slots.is_empty() && operand.resolved_face_slots != [slot] {
        return Ok(false);
    }
    operand.resolved_face_slots = vec![slot];
    Ok(true)
}

fn face_coincident_with_sketch(
    candidate: &cadmpeg_ir::ids::FaceId,
    sketch: &cadmpeg_ir::sketches::Sketch,
    faces: &[cadmpeg_ir::topology::Face],
    surfaces: &[cadmpeg_ir::geometry::Surface],
    linear_tolerance: f64,
    angular_tolerance: f64,
) -> bool {
    use cadmpeg_ir::geometry::SolvedSurfaceGeometry;

    let Some(face) = faces.iter().find(|face| face.id == *candidate) else {
        return false;
    };
    let Some(surface) = surfaces.iter().find(|surface| surface.id == face.surface) else {
        return false;
    };
    let Some(SolvedSurfaceGeometry::Plane(plane_surface)) = surface.geometry.solved() else {
        return false;
    };
    let origin = plane_surface.origin().get();
    let normal = plane_surface.frame().axis().as_raw();
    let Some((sketch_origin, sketch_normal, _)) = sketch.resolved_placement() else {
        return false;
    };
    parallel_vectors(*normal, sketch_normal.get(), angular_tolerance)
        && point_plane_distance(origin, sketch_origin.get(), sketch_normal.get())
            <= linear_tolerance
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

pub(super) fn design_angle(parameter: &DesignParameter) -> Option<cadmpeg_ir::scalar::Angle> {
    (parameter
        .unit()
        .map(|field| field.value.as_str())
        .is_some_and(design_angle_unit))
    .then_some(cadmpeg_ir::scalar::Angle::new(
        parameter.evaluated_value().get(),
    )?)
}

/// Length scale from a placement's stored origin to the neutral length unit.
/// The 201/329-byte frames store the origin in the neutral unit directly; the
/// `EntityGenesis`-flavor 213/341-byte frames and the member-run head record
/// of a feature-owned sketch store it in centimetres while their sketch point
/// and curve records carry values ten times the centimetre value, so the
/// origin scales by ten to stay commensurate with the entities.
pub(super) fn placement_origin_scale(placement: &DesignSketchPlacement) -> f64 {
    use crate::records::sketch_placement::DesignSketchFrameForm;
    match placement.frame.form() {
        DesignSketchFrameForm::ScopeGenesisCompact
        | DesignSketchFrameForm::ScopeGenesisExplicit(_)
        | DesignSketchFrameForm::MemberCompact { .. }
        | DesignSketchFrameForm::MemberExplicit { .. } => 10.0,
        DesignSketchFrameForm::ScopeCompact
        | DesignSketchFrameForm::ScopeLegacy305(_)
        | DesignSketchFrameForm::ScopeLegacy325(_)
        | DesignSketchFrameForm::ScopeExplicit(_) => 1.0,
    }
}

pub(super) fn sketch_curve_is_spatial(curve: &SketchCurveIdentity) -> bool {
    match curve.geometry.as_ref() {
        Some(SketchCurveGeometry::Line { start, end, .. }) => {
            !(planar_point(start.as_raw()) && planar_point(end.as_raw()))
        }
        Some(SketchCurveGeometry::Arc {
            center,
            normal,
            reference_direction,
            ..
        }) => {
            !(planar_point(center.as_raw())
                && reference_direction.as_raw().z.abs()
                    <= EPS_FACE_RESOLVE_SKETCH_CURVE_IS_SPATIAL_E9
                && sketch_normal_sign(normal.as_raw()).is_some())
        }
        Some(SketchCurveGeometry::Nurbs { geometry, .. }) => {
            geometry.poles().points().any(|point| !planar_point(point))
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        bounded_face_candidate_by_boundary_cardinality, convergent_face_support,
        effective_historical_face_slots, extrude_start_plane_geometry_candidates,
        extrude_target_plane_candidate, legacy_face_recipe_reference_candidates,
        loft_edge_profile_face_slot, resolve_surface_delete_face_history_set,
        resolved_explicit_bounded_face_group, resolved_extrude_profile_face_group,
        resolved_face_group, resolved_historical_split_face_target_group_with_updated_faces,
        retain_face_operand_resolution, stable_face_support_set, ExtrudeFaceResolution,
    };
    use crate::design::edge_resolve::feature_input_topology_id;
    use crate::ids::neutral_feature_id;
    use crate::records::topology::extrude_selection::DesignOperandRole;
    use crate::records::topology::face::DesignFaceOperand;
    use crate::records::{
        dimensions::DesignRecipeReference,
        feature::scope::DesignParameterScope,
        topology::{
            construction::DesignConstructionOperandGroup, edge_identity::DesignEdgeOperand,
            edge_recipe::DesignEdgeRecipeStructure, face::DesignFaceRecipeNode,
            historical_context::DesignEdgeRecipeReferenceContext,
            historical_context::DesignHistoricalFaceBoundaryContext,
            historical_context::DesignHistoricalFaceLoopContext,
            historical_context::DesignHistoricalFaceSupportContext,
        },
    };

    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
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
            "recipe_record_index": 203,
            "recipe_record_byte_offset": 341,
            "recipe_id": "f3d:test:recipe#201",
            "recipe_prefix_offset": 352,
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
            "next_byte_offset": 469
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
                "opaque_index_offset": 18,
                "opaque_scalar": 0.0,
                "opaque_scalar_offset": 22,
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
            resolved_explicit_bounded_face_group(None, &group, &[operand.clone()]).expect("projection resource budget"),
            Some(cadmpeg_ir::features::FaceSelection::Resolved {
                faces: vec![face(10), face(20)],
                native: group.id.clone(),
            })
        );
        group.operand_role =
            crate::records::topology::construction::DesignConstructionOperandRole::ExtrudeProfile;
        let scope: DesignParameterScope = serde_json::from_value(serde_json::json!({
            "id": "f3d:test:scope#100",
            "byte_offset": 0,
            "class_tag": "304",
            "record_index": 100,
            "frame_length": 300,
            "kind": "Extrude",
            "kind_offset": 32,
            "feature_ordinal": 1,
            "feature_ordinal_offset": 228,
            "history_state_id": 2,
            "history_state_id_offset": 24,
            "previous_history_state_id": 1,
            "previous_history_state_id_offset": 258,
            "reference_count_offset": 9,
            "reference_members": [150],
            "reference_member_offsets": [14],
            "paired_class_tag": "258",
            "paired_byte_offset": 300
        }))
        .expect("Extrude scope");
        assert_eq!(
            resolved_extrude_profile_face_group(
                None,
                &scope,
                &group,
                std::slice::from_ref(&group),
                &[operand.clone()],
            ).unwrap(),
            Some(cadmpeg_ir::features::ProfileRef::Planar(
                cadmpeg_ir::features::PlanarProfileRef::Faces(vec![face(10), face(20),])
            ))
        );
        operand.resolved_active_face =
            Some(FaceId::mint("f3d:brep/legacy/brep:entity#30").expect("identity grammar"));
        assert_eq!(
            resolved_face_group(None, &group, std::slice::from_ref(&operand)).expect("projection resource budget"),
            Some(cadmpeg_ir::features::FaceSelection::Resolved {
                faces: vec![
                    FaceId::mint("f3d:brep/legacy/brep:entity#30").expect("identity grammar")
                ],
                native: group.id.clone(),
            })
        );
        operand.resolved_active_face = None;
        operand.preceding_candidate_faces = vec![face(10), face(20)];
        assert_eq!(
            resolved_face_group(None, &group, std::slice::from_ref(&operand)).expect("projection resource budget"),
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
            resolved_explicit_bounded_face_group(None, &group, &[operand.clone()]).expect("projection resource budget"),
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
        assert!(resolved_explicit_bounded_face_group(None, &group, &[operand]).expect("projection resource budget").is_none());
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
                "recipe_record_byte_offset": 423,
                "recipe_id": format!("f3d:test:recipe#{record_index}"),
                "recipe_prefix_offset": 434,
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
                "next_byte_offset": 551
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
            "kind_offset": 65,
            "feature_ordinal": 1,
            "feature_ordinal_offset": 335,
            "history_state_id": 50,
            "history_state_id_offset": 57,
            "previous_history_state_id": 49,
            "previous_history_state_id_offset": 365,
            "reference_count_offset": 9,
            "reference_members": [150, 200, 201, 202],
            "reference_member_offsets": [14, 25, 36, 47],
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
            "member_offsets": [0, 11, 22],
            "frame": {
                "member_count_offset": 0,
                "opaque_index": 1,
                "opaque_index_offset": 18,
                "opaque_scalar": 0.0,
                "opaque_scalar_offset": 22,
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
            scope.previous_history_state_id(),
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
        assert_eq!(native.as_str(), group.id);
        assert_eq!(
            faces
                .iter()
                .map(|face| face.as_str().rsplit_once(':').unwrap().1)
                .collect::<Vec<_>>(),
            ["10", "20", "30"]
        );
        assert!(
            resolved_historical_split_face_target_group_with_updated_faces(
                &scope,
                scope.previous_history_state_id(),
                &group,
                &operands,
                &[10, 20]
            )
            .is_none()
        );
        assert!(
            resolved_historical_split_face_target_group_with_updated_faces(
                &scope,
                scope.previous_history_state_id(),
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
                boundary: crate::records::topology::historical_context::DesignHistoricalLoopBoundary::Coedges(
                    (0..edge_count)
                        .map(|ordinal| {
                            let coedge_slot = i64::try_from(ordinal).expect("test ordinal");
                            crate::records::topology::historical_context::DesignHistoricalLoopCoedge {
                                coedge_slot,
                                edge_slot: coedge_slot + 2_000,
                            }
                        })
                        .collect(),
                ),
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
        let edges = boundary.loops[0]
            .boundary
            .coedges()
            .map(|row| row.edge_slot)
            .collect::<Vec<_>>();
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
            "paired_byte_offset": 16,
            "paired_class_tag": "260",
            "recipe_record_index": record_index + 3,
            "recipe_record_byte_offset": 32,
            "recipe_id": "f3d:test:recipe",
            "recipe_prefix_offset": 43,
            "recipe_prefix_bytes": "",
            "recipe_references": [],
            "recipe_program_offset": 0,
            "recipe_program": [-1, -1, 2],
            "next_record_index": record_index + 4,
            "next_byte_offset": 160
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
            "frame_length": 200,
            "kind": "Loft",
            "kind_offset": 87,
            "feature_ordinal": 1,
            "feature_ordinal_offset": 128,
            "history_state_id": 7,
            "history_state_id_offset": 79,
            "previous_history_state_id": 6,
            "previous_history_state_id_offset": 158,
            "reference_count_offset": 9,
            "reference_members": [1000, 1001, 1819, 1, 2, 3],
            "reference_member_offsets": [14, 25, 36, 47, 58, 69],
            "paired_class_tag": "274",
            "paired_byte_offset": 200
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
            "member_offsets": [0, 11, 22],
            "frame": {
                "member_count_offset": 0,
                "opaque_index": 252,
                "opaque_index_offset": 18,
                "opaque_scalar": 0.0,
                "opaque_scalar_offset": 22,
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
                        boundary: crate::records::topology::historical_context::DesignHistoricalLoopBoundary::Coedges(
                            (0..*edge_count)
                                .map(|ordinal| {
                                    let coedge_slot = i64::try_from(ordinal).expect("test ordinal");
                                    crate::records::topology::historical_context::DesignHistoricalLoopCoedge {
                                        coedge_slot,
                                        edge_slot: coedge_slot + 10_000,
                                    }
                                })
                                .collect(),
                        ),
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
            "recipe_record_index": 24,
            "recipe_record_byte_offset": 24,
            "recipe_id": "f3d:test:recipe#22",
            "recipe_prefix_offset": 35,
            "recipe_prefix_bytes": "",
            "recipe_references": [],
            "recipe_kind": "bounded_face",
            "recipe_program_offset": 0,
            "recipe_program": [0, -1, 1],
            "recipe_node_offsets": [],
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
        let feature_key = feature.key();
        let expected_state = crate::design::edge_resolve::feature_input_topology_id(&feature, 6);
        let expected_face = crate::ids::history_input_face_id(
            &crate::ids::history_input_prefix(&feature_key, 6),
            100,
        );
        let operands = vec![
            edge_operand(1, 100, 0, 3, 200),
            edge_operand(2, 100, 1, 3, 201),
            edge_operand(3, 100, 0, 3, 202),
        ];
        assert!(matches!(
            super::resolved_loft_edge_profile_group(None, &scope, &group, &operands).unwrap(),
            Some(cadmpeg_ir::features::ProfileRef::Planar(cadmpeg_ir::features::PlanarProfileRef::HistoricalFaces {
                state,
                faces,
                native,
            })) if state == expected_state && faces.as_slice() == [expected_face] && native.as_slice() == [group.id]
        ));
    }

    fn assert_loft_edge_profile_collection_limit(operation: &'static str) {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let scope = loft_scope();
        let group = loft_group();
        let operands = vec![
            edge_operand(1, 100, 0, 3, 200),
            edge_operand(2, 100, 1, 3, 201),
            edge_operand(3, 100, 0, 3, 202),
        ];
        for limit in 0..16 {
            let mut policy = DecodePolicy::default();
            policy.limits.max_collection_items = limit;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            if matches!(super::resolved_loft_edge_profile_group(Some(&ctx), &scope,
                &group, &operands), Err(CodecError::ResourceLimit(failure))
                    if failure.dimension == ResourceDimension::CollectionItems
                        && failure.operation == operation) {
                return;
            }
        }
        panic!("no Loft edge profile refusal at {operation}");
    }

    #[test]
    fn loft_edge_member_index_refuses_collection_limit() {
        assert_loft_edge_profile_collection_limit("f3d Loft edge profile member index");
    }

    #[test]
    fn loft_edge_member_refuses_collection_limit() {
        assert_loft_edge_profile_collection_limit("f3d Loft edge profile member");
    }

    #[test]
    fn loft_historical_group_id_refuses_retained_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let scope = loft_scope();
        let group = loft_group();
        let operands = vec![
            edge_operand(1, 100, 0, 3, 200),
            edge_operand(2, 100, 1, 3, 201),
            edge_operand(3, 100, 0, 3, 202),
        ];
        for limit in 0..1000 {
            let mut policy = DecodePolicy::default();
            policy.limits.max_retained_bytes = limit;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            if matches!(super::resolved_loft_edge_profile_group(Some(&ctx), &scope,
                &group, &operands), Err(CodecError::ResourceLimit(failure))
                    if failure.dimension == ResourceDimension::RetainedBytes
                        && failure.operation == "f3d Loft historical group id") {
                return;
            }
        }
        panic!("no Loft historical group ID refusal");
    }

    fn start_geometry_fixture() -> (DesignFaceOperand, DesignConstructionOperandGroup, Vec<Face>) {
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
            "recipe_record_index": 203,
            "recipe_record_byte_offset": 341,
            "recipe_id": "f3d:test:recipe#201",
            "recipe_prefix_offset": 352,
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
            "next_byte_offset": 469
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
                "opaque_index": 1,
                "opaque_index_offset": 18,
                "opaque_scalar": 0.0,
                "opaque_scalar_offset": 22,
                "variant": false
            },
            "role_offset": 0,
            "paired_class_tag": "261",
            "paired_byte_offset": 0
        }))
        .expect("FromFace group");
        let faces = vec![Face {
            id: face(10),
            shell: ShellId::mint("test:model:shell#shell").expect("identity grammar"),
            surface: SurfaceId::mint("test:model:surface#surface").expect("identity grammar"),
            sense: Sense::Forward,
            loops: cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
            name: None,
            color: None,
            tolerance: None,
        }];
        (operand, group, faces)
    }

    fn assert_face_operand_retained_refusal(operand: &DesignFaceOperand, operation: &'static str) {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            super::resolved_face_operand(Some(&ctx), operand),
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == operation
                    && failure.dimension == ResourceDimension::RetainedBytes
        ));
    }

    #[test]
    fn active_face_operand_refuses_retained_limit() {
        let (mut operand, _, _) = start_geometry_fixture();
        operand.resolved_active_face = Some(face(10));
        assert_face_operand_retained_refusal(&operand, "f3d resolved active face id");
    }

    #[test]
    fn slot_face_operand_refuses_retained_limit() {
        let (mut operand, _, _) = start_geometry_fixture();
        operand.resolved_face_slots = vec![10];
        assert_face_operand_retained_refusal(&operand, "f3d resolved face slot id");
    }

    #[test]
    fn slot_candidate_face_operand_refuses_retained_limit() {
        let (mut operand, _, _) = start_geometry_fixture();
        operand.resolved_face_slots = vec![10];
        operand.candidate_faces = vec![face(10)];
        assert_face_operand_retained_refusal(&operand, "f3d resolved face slot candidate id");
    }

    #[test]
    fn alternate_face_operand_refuses_retained_limit() {
        let (mut operand, _, _) = start_geometry_fixture();
        operand.alternate_selector_candidate_faces = vec![face(10)];
        assert_face_operand_retained_refusal(&operand, "f3d alternate face candidate id");
    }

    #[test]
    fn referenced_face_operand_refuses_retained_limit() {
        let (mut operand, _, _) = start_geometry_fixture();
        operand.recipe_kind = crate::records::recipes::ConstructionRecipeKind::Face;
        assert_face_operand_retained_refusal(&operand, "f3d referenced face id");
    }

    #[test]
    fn unreferenced_face_operand_refuses_retained_limit() {
        let (mut operand, _, _) = start_geometry_fixture();
        operand.unreferenced_candidate_faces = vec![face(10)];
        assert_face_operand_retained_refusal(&operand, "f3d unreferenced face candidate id");
    }

    #[test]
    fn unique_face_operand_refuses_retained_limit() {
        let (mut operand, _, _) = start_geometry_fixture();
        operand.candidate_faces = vec![face(10)];
        assert_face_operand_retained_refusal(&operand, "f3d unique face candidate id");
    }

    #[test]
    fn resolved_face_group_refuses_member_collection_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let (mut operand, group, _) = start_geometry_fixture();
        operand.resolved_active_face = Some(face(10));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            resolved_face_group(Some(&ctx), &group, std::slice::from_ref(&operand)),
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == "f3d resolved face group member"
                    && failure.dimension == ResourceDimension::CollectionItems
        ));
    }

    #[test]
    fn explicit_bounded_face_group_refuses_candidate_retained_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let (mut operand, group, _) = start_geometry_fixture();
        operand.candidate_faces = vec![face(10)];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            resolved_explicit_bounded_face_group(Some(&ctx), &group, std::slice::from_ref(&operand)),
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == "f3d explicit bounded face candidate id"
                    && failure.dimension == ResourceDimension::RetainedBytes
        ));
    }

    #[test]
    fn explicit_bounded_face_group_refuses_lane_collection_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let (operand, group, _) = start_geometry_fixture();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            resolved_explicit_bounded_face_group(Some(&ctx), &group, std::slice::from_ref(&operand)),
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == "f3d explicit bounded face lane"
                    && failure.dimension == ResourceDimension::CollectionItems
        ));
    }

    #[test]
    fn resolved_face_group_refuses_native_id_retained_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let (mut operand, group, _) = start_geometry_fixture();
        operand.resolved_active_face = Some(face(10));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = u64::try_from(face(10).as_str().len()).unwrap();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            resolved_face_group(Some(&ctx), &group, std::slice::from_ref(&operand)),
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == "f3d resolved face group native id"
                    && failure.dimension == ResourceDimension::RetainedBytes
        ));
    }

    #[test]
    fn direct_face_selection_refuses_operand_collection_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let scope = loft_scope();
        let operand: DesignFaceOperand = serde_json::from_value(serde_json::json!({
            "id": "f3d:test:face-operand#1000",
            "scope_record_index": 1811,
            "scope_reference_ordinal": 0,
            "record_index": 1000,
            "byte_offset": 0,
            "class_tag": "271",
            "paired_byte_offset": 325,
            "paired_class_tag": "261",
            "recipe_record_index": 1003,
            "recipe_record_byte_offset": 341,
            "recipe_id": "f3d:test:recipe#1001",
            "recipe_prefix_offset": 352,
            "recipe_prefix_bytes": "",
            "recipe_references": [],
            "recipe_kind": "bounded_face",
            "recipe_program_offset": 0,
            "recipe_program": [],
            "recipe_node_offsets": [],
            "recipe_nodes": [],
            "candidate_faces": ["f3d:brep:entity#10"],
            "unreferenced_candidate_faces": [],
            "alternate_selector_candidate_faces": [],
            "preceding_candidate_faces": [],
            "changed_candidate_faces": [],
            "historical_support_contexts": [],
            "resolved_face_slots": [10],
            "next_record_index": 1002,
            "next_byte_offset": 469
        })).expect("direct face operand");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            super::resolved_direct_face_selection(Some(&ctx), &scope, &[operand]),
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == "f3d direct face operand"
                    && failure.dimension == ResourceDimension::CollectionItems
        ));
    }

    #[test]
    fn extrude_start_plane_geometry_fallback_requires_complete_nested_recipe() {
        let (operand, group, faces) = start_geometry_fixture();
        assert_eq!(
            extrude_start_plane_geometry_candidates(None, &group, std::slice::from_ref(&operand), &faces,).unwrap(),
            Some(vec![face(10)])
        );
        let mut bound = operand.clone();
        assert!(retain_face_operand_resolution(
            None,
            &group,
            std::slice::from_mut(&mut bound),
            &face(10)
        ).unwrap());
        assert_eq!(bound.resolved_active_face, Some(face(10)));

        let mut incomplete = operand;
        incomplete.recipe_nodes.clear();
        assert!(extrude_start_plane_geometry_candidates(None, &group, &[incomplete], &faces).unwrap().is_none());
    }

    #[test]
    fn start_plane_candidate_id_refuses_retained_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let (operand, group, faces) = start_geometry_fixture();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            extrude_start_plane_geometry_candidates(
                Some(&ctx), &group, std::slice::from_ref(&operand), &faces),
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == "f3d start plane candidate ID"
                    && failure.dimension == ResourceDimension::RetainedBytes
        ));
    }

    #[test]
    fn start_plane_candidate_refuses_collection_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let (operand, group, faces) = start_geometry_fixture();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            extrude_start_plane_geometry_candidates(
                Some(&ctx), &group, std::slice::from_ref(&operand), &faces),
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == "f3d start plane candidate"
                    && failure.dimension == ResourceDimension::CollectionItems
        ));
    }

    #[test]
    fn retained_start_face_refuses_retained_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let (mut operand, group, _) = start_geometry_fixture();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            retain_face_operand_resolution(
                Some(&ctx), &group, std::slice::from_mut(&mut operand), &face(10)),
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == "f3d retained operand active face"
                    && failure.dimension == ResourceDimension::RetainedBytes
        ));
        assert!(operand.resolved_active_face.is_none());
    }

    fn start_binder_fixture(
        nested: bool,
    ) -> (
        cadmpeg_ir::features::Feature,
        Sketch,
        DesignConstructionOperandGroup,
        DesignFaceOperand,
        Vec<Face>,
        Vec<Surface>,
    ) {
        use cadmpeg_ir::features::{
            BooleanOp, ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart,
            Feature, FeatureDefinition, FeatureEvaluation, FeatureOperation,
            LinearTermination, PlanarProfileRef, ProfileRef, FaceSelection,
        };
        let (nested_operand, mut group, faces) = start_geometry_fixture();
        group.operand_role = crate::records::topology::construction::DesignConstructionOperandRole::ExtrudeFaces {
            encoding: crate::records::topology::extrude_selection::DesignExtrudeFaceEncoding::SelectedStart,
            usage: crate::records::topology::extrude_selection::DesignExtrudeFaceRole::Start,
        };
        let operand = if nested {
            nested_operand
        } else {
            target_plane_operand(&[10])
        };
        let surface_id = faces[0].surface.clone();
        let surfaces = vec![Surface {
            id: surface_id,
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 2.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                ).unwrap(),
            )),
            source_object: None,
        }];
        let sketch = Sketch {
            id: SketchId::mint("synthetic:test:id#start-sketch").unwrap(),
            name: None,
            configuration: None,
            visible: None,
            placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
                Point3::new(0.0, 0.0, 2.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            ).unwrap(),
            profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
            native_ref: None,
        };
        let feature = Feature {
            id: cadmpeg_ir::features::FeatureId::mint("test:model:feature#start").unwrap(),
            ordinal: 0,
            name: None,
            suppressed: None,
            dependencies: Default::default(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Default::default(),
            evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
                FeatureOperation::Extrude {
                    profile: ProfileRef::Planar(PlanarProfileRef::Sketch(sketch.id.clone())),
                    direction: ExtrudeDirection::ProfileNormal {},
                    start: ExtrudeStart::FromFace {
                        face: FaceSelection::Native(group.id.clone()),
                        offset: None,
                    },
                    extent: ExtrudeExtent::OneSided {
                        side: ExtrudeSide {
                            termination: LinearTermination::ThroughAll {},
                            draft: None,
                        },
                    },
                    op: BooleanOp::NewBody,
                    solid: None,
                    face_maker: None,
                    inner_wire_taper: None,
                    length_along_profile_normal: None,
                    allow_multi_profile_faces: None,
                },
            )),
            native_ref: None,
        };
        (feature, sketch, group, operand, faces, surfaces)
    }

    fn assert_start_binder_refusal(
        operation: &'static str,
        dimension: cadmpeg_core::decode::ResourceDimension,
        nested: bool,
    ) {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;
        use cadmpeg_ir::features::{ExtrudeStart, FaceSelection, FeatureDefinition, FeatureOperation};

        let (feature, sketch, group, operand, faces, surfaces) = start_binder_fixture(nested);
        let run = |ctx: Option<&DecodeContext<'_>>| {
            let mut features = [feature.clone()];
            let mut operands = [operand.clone()];
            let mut resolution = ExtrudeFaceResolution {
                faces: &faces,
                surfaces: &surfaces,
                groups: std::slice::from_ref(&group),
                operands: &mut operands,
                linear_tolerance: super::EPS_FACE_TEST_TARGET_LINEAR_E9,
                angular_tolerance: super::EPS_FACE_TEST_TARGET_ANGULAR_E9,
            };
            let result = super::bind_extrude_start_planes(
                ctx, &mut features, std::slice::from_ref(&sketch), &mut resolution);
            (result, features)
        };
        let (result, features) = run(None);
        result.unwrap();
        assert!(matches!(
            features[0].evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                start: ExtrudeStart::FromFace {
                    face: FaceSelection::Resolved { .. }, ..
                }, ..
            })
        ));
        let mut found = false;
        for limit in 0..256 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            match dimension {
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
                ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
                _ => unreachable!(),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            if matches!(run(Some(&ctx)).0,
                Err(CodecError::ResourceLimit(failure))
                    if failure.operation == operation && failure.dimension == dimension) {
                found = true;
                break;
            }
        }
        assert!(found, "no resource refusal at {operation}");
    }

    #[test]
    fn start_operand_face_id_refuses_retained_limit() {
        assert_start_binder_refusal(
            "f3d start plane operand face ID",
            cadmpeg_core::decode::ResourceDimension::RetainedBytes, false);
    }

    #[test]
    fn start_operand_candidate_refuses_collection_limit() {
        assert_start_binder_refusal(
            "f3d start plane operand candidate",
            cadmpeg_core::decode::ResourceDimension::CollectionItems, false);
    }

    #[test]
    fn coincident_start_face_refuses_collection_limit() {
        assert_start_binder_refusal(
            "f3d coincident start plane face",
            cadmpeg_core::decode::ResourceDimension::CollectionItems, true);
    }

    #[test]
    fn selected_start_face_id_refuses_retained_limit() {
        assert_start_binder_refusal(
            "f3d selected start plane face ID",
            cadmpeg_core::decode::ResourceDimension::RetainedBytes, true);
    }

    #[test]
    fn selected_start_native_id_refuses_retained_limit() {
        assert_start_binder_refusal(
            "f3d selected start plane native ID",
            cadmpeg_core::decode::ResourceDimension::RetainedBytes, true);
    }

    #[test]
    fn target_native_id_refuses_retained_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;
        use cadmpeg_ir::features::{
            ExtrudeExtent, ExtrudeStart, FaceSelection, FeatureDefinition, FeatureOperation,
            LinearTermination,
        };

        let (mut feature, sketch, mut group, operand, faces, mut surfaces) =
            start_binder_fixture(false);
        group.operand_role =
            crate::records::topology::construction::DesignConstructionOperandRole::ExtrudeFaces {
                encoding: crate::records::topology::extrude_selection::DesignExtrudeFaceEncoding::Faces,
                usage: crate::records::topology::extrude_selection::DesignExtrudeFaceRole::Termination,
            };
        surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 3.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            ).unwrap(),
        ));
        feature.evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(FeatureOperation::Extrude {
                start,
                extent: ExtrudeExtent::OneSided { side },
                ..
            }) = definition {
                *start = ExtrudeStart::ProfilePlane {};
                side.termination = LinearTermination::ToFace {
                    face: FaceSelection::Native(group.id.clone()),
                    offset: None,
                };
            }
        });
        let run = |ctx: Option<&DecodeContext<'_>>| {
            let mut features = [feature.clone()];
            let mut operands = [operand.clone()];
            let mut resolution = ExtrudeFaceResolution {
                faces: &faces,
                surfaces: &surfaces,
                groups: std::slice::from_ref(&group),
                operands: &mut operands,
                linear_tolerance: super::EPS_FACE_TEST_TARGET_LINEAR_E9,
                angular_tolerance: super::EPS_FACE_TEST_TARGET_ANGULAR_E9,
            };
            let result = super::bind_extrude_target_faces(
                ctx, &mut features, std::slice::from_ref(&sketch), &mut resolution);
            (result, features)
        };
        let (result, features) = run(None);
        result.unwrap();
        assert!(matches!(
            features[0].evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                extent: ExtrudeExtent::OneSided {
                    side: cadmpeg_ir::features::ExtrudeSide {
                        termination: LinearTermination::ToFace {
                            face: FaceSelection::Resolved { .. }, ..
                        },
                        ..
                    },
                }, ..
            })
        ));
        let mut found = false;
        for limit in 0..256 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_retained_bytes = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            if matches!(run(Some(&ctx)).0,
                Err(CodecError::ResourceLimit(failure))
                    if failure.operation == "f3d target face native ID"
                        && failure.dimension == ResourceDimension::RetainedBytes) {
                found = true;
                break;
            }
        }
        assert!(found, "no retained-byte refusal at target native ID");
    }

    #[test]
    fn selected_face_start_requires_unique_sketch_plane_coincidence() {
        let sketch = Sketch {
            id: SketchId::mint("synthetic:test:id#sketch").unwrap(),
            name: None,
            configuration: None,
            visible: None,
            placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
                Point3::new(0.0, 0.0, 2.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
            profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
            native_ref: None,
        };
        let face = |id: &str, surface: &str| Face {
            id: FaceId::mint(format!("test:model:face#{id}")).expect("identity grammar"),
            shell: ShellId::mint("test:model:shell#shell").expect("identity grammar"),
            surface: SurfaceId::mint(format!("test:model:surface#{surface}"))
                .expect("identity grammar"),
            sense: Sense::Forward,
            loops: cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
            name: None,
            color: None,
            tolerance: None,
        };
        let plane = |id: &str, origin: Point3, normal: Vector3| Surface {
            id: SurfaceId::mint(format!("test:model:surface#{id}")).expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    origin,
                    normal.unit().unwrap(),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            )),
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
            .map(|slot| face(*slot).into_string())
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
            "recipe_record_index": 203,
            "recipe_record_byte_offset": 341,
            "recipe_id": "f3d:test:recipe#201",
            "recipe_prefix_offset": 352,
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
            "next_byte_offset": 469
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
                "opaque_index": 1,
                "opaque_index_offset": 18,
                "opaque_scalar": 0.0,
                "opaque_scalar_offset": 22,
                "variant": false
            },
            "role": DesignOperandRole::FACES.raw(),
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
            shell: ShellId::mint("test:model:shell#shell").expect("identity grammar"),
            surface: SurfaceId::mint(format!("test:model:surface#{surface}"))
                .expect("identity grammar"),
            sense: Sense::Forward,
            loops: cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
            name: None,
            color: None,
            tolerance: None,
        };
        let plane = |id: &str, origin: Point3, normal: Vector3| Surface {
            id: SurfaceId::mint(format!("test:model:surface#{id}")).expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    origin,
                    normal,
                    Vector3::new(0.0, 0.0, 1.0),
                )
                .unwrap(),
            )),
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
            extrude_target_plane_candidate(None, &group, &resolution, origin, sweep_direction).unwrap()
        };

        assert_eq!(candidate(&[1, 3, 4]), Some(face(1)));
        assert!(candidate(&[1, 2, 3, 4]).is_none());
        assert!(candidate(&[3, 4]).is_none());
    }

    #[test]
    fn target_plane_face_id_refuses_retained_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let surface_id = SurfaceId::mint("test:model:surface#forward").unwrap();
        let faces = [Face {
            id: face(1),
            shell: ShellId::mint("test:model:shell#shell").unwrap(),
            surface: surface_id.clone(),
            sense: Sense::Forward,
            loops: cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
            name: None,
            color: None,
            tolerance: None,
        }];
        let surfaces = [Surface {
            id: surface_id,
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(3.0, 0.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                ).unwrap(),
            )),
            source_object: None,
        }];
        let mut operands = [target_plane_operand(&[1])];
        let resolution = ExtrudeFaceResolution {
            faces: &faces,
            surfaces: &surfaces,
            groups: &[],
            operands: &mut operands,
            linear_tolerance: super::EPS_FACE_TEST_TARGET_LINEAR_E9,
            angular_tolerance: super::EPS_FACE_TEST_TARGET_ANGULAR_E9,
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            extrude_target_plane_candidate(
                Some(&ctx), &target_face_group(), &resolution,
                Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0)),
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == "f3d target plane face ID"
                    && failure.dimension == ResourceDimension::RetainedBytes
        ));
    }

    fn extrude_root_fixture() -> (DesignParameterScope, [DesignConstructionOperandGroup; 2]) {
        let scope = DesignParameterScope::empty(
            "f3d:test:scope#12",
            crate::records::feature::scope::DesignFeatureKind::Extrude,
            12,
        );
        let root: DesignConstructionOperandGroup = serde_json::from_value(serde_json::json!({
            "id": "f3d:test:group#100",
            "scope_record_index": 12,
            "scope_reference_ordinal": 0,
            "record_index": 100,
            "byte_offset": 1000,
            "class_tag": "332",
            "members": [101],
            "member_offsets": [1026],
            "frame": {
                "member_count_offset": 1021,
                "opaque_index": 1,
                "opaque_index_offset": 1072,
                "opaque_scalar": 0.0,
                "opaque_scalar_offset": 1076,
                "variant": false
            },
            "role": DesignOperandRole::PROFILE.raw(),
            "extrude_role": "profile",
            "role_offset": 1054,
            "paired_class_tag": "259",
            "paired_byte_offset": 1125
        })).unwrap();
        let mut child = root.clone();
        child.id = "f3d:test:group#101".into();
        child.record_index = 101;
        child.scope_reference_ordinal = 1;
        child.try_set_members(vec![crate::records::identity::Located {
            value: 200,
            offset: 1026,
        }]).unwrap();
        (scope, [root, child])
    }

    fn assert_extrude_root_collection_limit(operation: &'static str) {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let (scope, groups) = extrude_root_fixture();
        let roots = super::extrude_profile_group_roots(None, &scope, &groups)
            .unwrap().unwrap();
        assert_eq!(roots.iter().map(|group| group.record_index).collect::<Vec<_>>(), [100]);
        for limit in 0..16 {
            let mut policy = DecodePolicy::default();
            policy.limits.max_collection_items = limit;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            if matches!(super::extrude_profile_group_roots(Some(&ctx), &scope, &groups),
                Err(CodecError::ResourceLimit(failure))
                    if failure.dimension == ResourceDimension::CollectionItems
                        && failure.operation == operation
            ) {
                return;
            }
        }
        panic!("no Extrude profile hierarchy refusal at {operation}");
    }

    #[test]
    fn extrude_profile_group_refuses_collection_limit() {
        assert_extrude_root_collection_limit("f3d Extrude profile group");
    }

    #[test]
    fn extrude_profile_group_index_refuses_collection_limit() {
        assert_extrude_root_collection_limit("f3d Extrude profile group index");
    }

    #[test]
    fn extrude_profile_parent_index_refuses_collection_limit() {
        assert_extrude_root_collection_limit("f3d Extrude profile parent index");
    }

    #[test]
    fn extrude_profile_root_refuses_collection_limit() {
        assert_extrude_root_collection_limit("f3d Extrude profile root");
    }

    #[test]
    fn extrude_profile_visited_group_refuses_collection_limit() {
        assert_extrude_root_collection_limit("f3d Extrude profile visited group");
    }

    #[test]
    fn extrude_profile_hierarchy_refuses_depth_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let (scope, groups) = extrude_root_fixture();
        let mut policy = DecodePolicy::default();
        policy.limits.max_recursion_depth = 1;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(super::extrude_profile_group_roots(Some(&ctx), &scope, &groups),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::RecursionDepth
                    && failure.operation == "f3d Extrude profile hierarchy"));
    }

    #[test]
    fn extrude_profile_hierarchy_refuses_work_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let (scope, groups) = extrude_root_fixture();
        let mut policy = DecodePolicy::default();
        policy.limits.max_work_units = 1;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(super::extrude_profile_group_roots(Some(&ctx), &scope, &groups),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::WorkUnits
                    && failure.operation == "f3d Extrude profile hierarchy"));
    }

    fn extrude_leaf_fixture() -> (
        DesignConstructionOperandGroup,
        Vec<DesignConstructionOperandGroup>,
        Vec<DesignFaceOperand>,
    ) {
        let (_, groups) = extrude_root_fixture();
        let operand = serde_json::from_value(serde_json::json!({
            "id": "f3d:test:face-operand#200",
            "scope_record_index": 12,
            "scope_reference_ordinal": 1,
            "group_record_index": 101,
            "group_member_ordinal": 0,
            "record_index": 200,
            "byte_offset": 0,
            "class_tag": "297",
            "paired_byte_offset": 16,
            "paired_class_tag": "259",
            "recipe_record_index": 203,
            "recipe_record_byte_offset": 32,
            "recipe_id": "f3d:test:recipe#203",
            "recipe_prefix_offset": 43,
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
            "candidate_faces": ["f3d:brep:entity#10"],
            "next_record_index": 204,
            "next_byte_offset": 160
        })).unwrap();
        (groups[0].clone(), groups.to_vec(), vec![operand])
    }

    fn assert_extrude_leaf_collection_limit(operation: &'static str) {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let (root, groups, operands) = extrude_leaf_fixture();
        assert_eq!(super::extrude_profile_group_operand_indices(None, &root, &groups, &operands)
            .unwrap().unwrap(), [0]);
        for limit in 0..24 {
            let mut policy = DecodePolicy::default();
            policy.limits.max_collection_items = limit;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            if matches!(super::extrude_profile_group_operand_indices(Some(&ctx), &root,
                &groups, &operands), Err(CodecError::ResourceLimit(failure))
                    if failure.dimension == ResourceDimension::CollectionItems
                        && failure.operation == operation) {
                return;
            }
        }
        panic!("no Extrude leaf refusal at {operation}");
    }

    #[test]
    fn extrude_leaf_profile_group_refuses_collection_limit() {
        assert_extrude_leaf_collection_limit("f3d Extrude leaf profile group");
    }

    #[test]
    fn extrude_leaf_group_index_refuses_collection_limit() {
        assert_extrude_leaf_collection_limit("f3d Extrude leaf group index");
    }

    #[test]
    fn extrude_leaf_visited_group_refuses_collection_limit() {
        assert_extrude_leaf_collection_limit("f3d Extrude leaf visited group");
    }

    #[test]
    fn extrude_leaf_index_refuses_collection_limit() {
        assert_extrude_leaf_collection_limit("f3d Extrude leaf index");
    }

    #[test]
    fn extrude_leaf_hierarchy_refuses_depth_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let (root, groups, operands) = extrude_leaf_fixture();
        let mut policy = DecodePolicy::default();
        policy.limits.max_recursion_depth = 1;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(super::extrude_profile_group_operand_indices(Some(&ctx), &root,
            &groups, &operands), Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::RecursionDepth
                    && failure.operation == "f3d Extrude leaf hierarchy"));
    }

    #[test]
    fn extrude_leaf_hierarchy_refuses_work_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let (root, groups, operands) = extrude_leaf_fixture();
        let mut policy = DecodePolicy::default();
        policy.limits.max_work_units = 1;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(super::extrude_profile_group_operand_indices(Some(&ctx), &root,
            &groups, &operands), Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::WorkUnits
                    && failure.operation == "f3d Extrude leaf hierarchy"));
    }

    #[test]
    fn extrude_active_face_id_refuses_retained_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let (_, _, operands) = extrude_leaf_fixture();
        assert_eq!(super::resolved_extrude_profile_active_faces(None, &[0], &operands)
            .unwrap().unwrap(), [face(10)]);
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(super::resolved_extrude_profile_active_faces(Some(&ctx), &[0], &operands),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::RetainedBytes
                    && failure.operation == "f3d Extrude active face id"));
    }

    #[test]
    fn extrude_active_face_refuses_collection_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let (_, _, operands) = extrude_leaf_fixture();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(super::resolved_extrude_profile_active_faces(Some(&ctx), &[0], &operands),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::CollectionItems
                    && failure.operation == "f3d Extrude active face"));
    }
}
