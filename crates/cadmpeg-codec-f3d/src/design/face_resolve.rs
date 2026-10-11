// SPDX-License-Identifier: Apache-2.0
//! Resolve face-selection operands and extrude start planes.

use crate::design::feature_project::{design_angle_unit, unique_feature_match_for_decode};
use crate::ids::{self, native_stream_charged};
use crate::records::{
    feature::{
        extrude::{DesignExtrudeExtent, DesignExtrudePrologue},
        scope::DesignParameterScope,
    },
    parameters::DesignParameter,
    sketch_placement::DesignSketchPlacement,
    topology::{
        body_recipe::DesignBodyRecipeOperand, construction::DesignConstructionOperandGroup,
        edge_identity::DesignEdgeOperand, extrude_selection::DesignExtrudeFaceRole,
        extrude_selection::DesignOperandRole, face::DesignFaceOperand,
    },
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use self::membership::{contains_face, contains_slot};
use self::operand_queries::unique_face_operand;
use self::planes::{face_coincident_with_sketch, parallel_vectors, plane_for_face};

mod membership;
mod operand_queries;
mod planes;

const MAX_FREE_I64_TEXT_BYTES: usize = 20;
const FACE_SLOT_PREFIX: &str = "f3d:brep:entity#";

fn face_slot_id(ctx: &DecodeContext<'_>, slot: i64) -> Result<cadmpeg_ir::ids::FaceId, CodecError> {
    use std::fmt::Write;

    let mut magnitude = slot.unsigned_abs();
    let mut digits = 1usize;
    while magnitude >= 10 {
        magnitude /= 10;
        digits += 1;
    }
    let len = FACE_SLOT_PREFIX.len() + digits + usize::from(slot.is_negative());
    let operation = "f3d resolved face slot id";

    let mut text = ctx.retained_string(len, operation)?;
    text.push_str(FACE_SLOT_PREFIX);

    write!(&mut text, "{slot}")
        .map_err(|_| CodecError::malformed("face slot formatting failed"))?;
    cadmpeg_ir::ids::FaceId::try_from(text).map_err(CodecError::malformed)
}

fn historical_face_id(
    ctx: &DecodeContext<'_>,
    prefix: &cadmpeg_ir::ids::IdentityKey,
    slot: i64,
) -> Result<cadmpeg_ir::ids::HistoricalFaceId, CodecError> {
    const NAMESPACE: &str = "f3d:history-input:face#";

    use std::fmt::Write;

    let mut magnitude = slot.unsigned_abs();
    let mut digits = 1usize;
    while magnitude >= 10 {
        magnitude /= 10;
        digits += 1;
    }
    let operation = "f3d historical face id";
    let len = NAMESPACE
        .len()
        .checked_add(prefix.as_str().len())
        .and_then(|len| len.checked_add(1 + digits + usize::from(slot.is_negative())))
        .ok_or_else(|| ctx.refuse_codec_limit(operation, 0, 1))?;

    let mut text = ctx.retained_string(len, operation)?;
    text.push_str(NAMESPACE);
    ctx.append_retained(&mut text, prefix.as_str(), operation)?;
    text.push(':');

    write!(&mut text, "{slot}")
        .map_err(|_| CodecError::malformed("historical face ID formatting failed"))?;
    cadmpeg_ir::ids::HistoricalFaceId::try_from(text).map_err(CodecError::malformed)
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
    ctx: &DecodeContext<'_>,
    group: &DesignConstructionOperandGroup,
    operands: &[DesignFaceOperand],
) -> Result<Option<cadmpeg_ir::features::FaceSelection>, CodecError> {
    if group.members().is_empty() || operands.is_empty() {
        return Ok(None);
    }
    let Some(stream) =
        crate::ids::native_stream_charged(ctx, &group.id, "resolve F3D face group stream")?
    else {
        return Ok(None);
    };
    let mut selected = Vec::new();
    let mut seen = HashSet::new();
    let mut selection_storage = ctx.reserve_scoped(0, "f3d face group proof storage")?;
    let mut members = group.members().iter();
    while let Some(member) = ctx.next_charged(&mut members, "visit F3D face group member")? {
        let Some(operand) = unique_face_operand(
            ctx,
            operands,
            Some(stream),
            group.scope_record_index,
            member.value,
        )?
        else {
            return Ok(None);
        };
        if operand.resolved_active_face.is_none()
            && operand.resolved_face_slots.is_empty()
            && operand.alternate_selector_candidate_faces.is_empty()
            && (!operand.preceding_candidate_faces.is_empty()
                || !operand.changed_candidate_faces.is_empty()
                || !operand.historical_support_contexts.is_empty())
        {
            return Ok(None);
        }
        let (operand_faces, _operand_storage) = ctx
            .with_scoped_storage("f3d face operand proof storage", || {
                resolved_face_operand(ctx, operand)
            })?;
        let Some(operand_faces) = operand_faces else {
            return Ok(None);
        };
        for face in ctx.admit_iter(operand_faces, "f3d resolved group selected face source")? {
            selection_storage.with_storage(|| -> Result<(), CodecError> {
                if ctx.insert_hash_set(
                    &mut seen,
                    face.key(),
                    "find F3D resolved face group member",
                )? {
                    ctx.push_vec(&mut selected, face, "f3d resolved face group proof")?;
                }
                Ok(())
            })?;
        }
    }
    if selected.is_empty() {
        return Ok(None);
    }
    let mut faces = Vec::new();
    for candidate in ctx.admit_iter(&selected, "f3d resolved face group output source")? {
        let face = candidate.materialize(ctx)?;
        ctx.push_vec(&mut faces, face, "f3d resolved face group member")?;
    }
    let native = ctx.copy_retained_text(&group.id, "f3d resolved face group native id")?;
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
    ctx: &DecodeContext<'_>,
    group: &DesignConstructionOperandGroup,
    operands: &[DesignFaceOperand],
) -> Result<Option<cadmpeg_ir::features::FaceSelection>, CodecError> {
    let Some(stream) = native_stream_charged(ctx, &group.id, "f3d explicit bounded group stream")?
    else {
        return Ok(None);
    };
    let mut selected = Vec::new();
    let mut seen = HashSet::new();
    let mut selection_storage = ctx.reserve_scoped(0, "f3d face group proof storage")?;
    let mut members = group.members().iter();
    while let Some(member) =
        ctx.next_charged(&mut members, "f3d explicit bounded group member source")?
    {
        let Some(operand) = unique_face_operand(
            ctx,
            operands,
            Some(stream),
            group.scope_record_index,
            member.value,
        )?
        else {
            return Ok(None);
        };
        let (candidate_faces, _operand_storage) =
            ctx.with_scoped_storage("f3d explicit face operand proof storage", || {
                if operand.resolved_face_slots.is_empty() {
                    explicit_bounded_face_candidates(ctx, operand)
                } else {
                    resolved_face_operand(ctx, operand)
                }
            })?;
        let Some(candidate_faces) = candidate_faces else {
            return Ok(None);
        };
        for face in ctx.admit_iter(
            candidate_faces,
            "f3d explicit bounded group selected face source",
        )? {
            selection_storage.with_storage(|| -> Result<(), CodecError> {
                if ctx.insert_hash_set(
                    &mut seen,
                    face.key(),
                    "find F3D explicit bounded face group member",
                )? {
                    ctx.push_vec(&mut selected, face, "f3d explicit bounded face group proof")?;
                }
                Ok(())
            })?;
        }
    }
    if selected.is_empty() {
        return Ok(None);
    }
    let mut faces = Vec::new();
    for candidate in ctx.admit_iter(&selected, "f3d explicit bounded face group output source")? {
        let face = candidate.materialize(ctx)?;
        ctx.push_vec(&mut faces, face, "f3d explicit bounded face group member")?;
    }
    let native = ctx.copy_retained_text(&group.id, "f3d explicit bounded face group native id")?;
    Ok(Some(cadmpeg_ir::features::FaceSelection::Resolved {
        faces,
        native,
    }))
}

/// Resolve a direct scope face selection when every direct operand proves the
/// same current face set through stable input-topology slots.
pub(super) fn resolved_direct_face_selection(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    operands: &[DesignFaceOperand],
) -> Result<Option<cadmpeg_ir::features::FaceSelection>, CodecError> {
    use cadmpeg_ir::features::FaceSelection;

    let Some(stream) = native_stream_charged(ctx, &scope.id, "f3d direct face scope stream")?
    else {
        return Ok(None);
    };
    let mut matching_storage = ctx.reserve_scoped(0, "f3d direct face match storage")?;
    let mut matching = Vec::new();
    for operand in ctx.admit_iter(operands, "f3d direct face operand source")? {
        if crate::design::feature_project::feature_stream_matches(
            ctx,
            &operand.id,
            Some(stream),
            "f3d direct face operand stream",
        )? && operand.scope_record_index == scope.record_index
            && operand.group_record_index().is_none()
            && operand.group_member_ordinal().is_none()
            && operand.recipe_kind == crate::records::recipes::ConstructionRecipeKind::BoundedFace
            && usize::try_from(operand.scope_reference_ordinal)
                .ok()
                .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                == Some(&operand.record_index())
        {
            matching_storage
                .with_storage(|| ctx.push_vec(&mut matching, operand, "f3d direct face operand"))?;
        }
    }
    ctx.stable_sort_by_key(
        &mut matching[..],
        |value| {
            let operand = value;
            {
                operand.scope_reference_ordinal
            }
        },
        Ord::cmp,
        "sort f3d design face_resolve 1",
    )?;
    if matching.is_empty()
        || ctx.any_by(
            &matching,
            |operand| Ok(operand.resolved_face_slots.is_empty()),
            "f3d direct face completeness source",
        )?
    {
        return Ok(None);
    }
    let (first, _first_storage) = ctx
        .with_scoped_storage("f3d direct face proof storage", || {
            resolved_face_operand(ctx, matching[0])
        })?;
    let Some(first) = first.filter(|faces| !faces.is_empty()) else {
        return Ok(None);
    };
    let mut index_storage = ctx.reserve_scoped(0, "f3d direct face comparison index storage")?;
    let (positions, counts) = index_storage.with_storage(|| -> Result<_, CodecError> {
        if matching.len() == 1 {
            return Ok((HashMap::new(), Vec::<usize>::new()));
        }
        let mut positions = HashMap::new();
        let mut counts = Vec::<usize>::new();
        for face in ctx.admit_iter(&first, "f3d direct face comparison index source")? {
            let key = face.key();
            if let Some(index) = ctx
                .get_hash_map(&positions, &key, "f3d direct face comparison lookup")?
                .copied()
            {
                counts[index] += 1;
            } else {
                ctx.insert_hash_map(
                    &mut positions,
                    key,
                    counts.len(),
                    "f3d direct face comparison index",
                )?;
                ctx.push_vec(&mut counts, 1, "f3d direct face comparison count")?;
            }
        }
        Ok((positions, counts))
    })?;
    let mut remaining = matching[1..].iter();
    while let Some(operand) =
        ctx.next_charged(&mut remaining, "f3d direct face comparison operand source")?
    {
        let (candidate, _candidate_storage) = ctx
            .with_scoped_storage("f3d direct face comparison storage", || {
                resolved_face_operand(ctx, operand)
            })?;
        let Some(candidate) = candidate else {
            return Ok(None);
        };
        if candidate.len() != first.len() {
            return Ok(None);
        }
        let (mut unmatched, _count_storage) = ctx.with_scoped_storage(
            "f3d direct face comparison count storage",
            || -> Result<_, CodecError> {
                let mut copied = Vec::new();
                for count in ctx.admit_iter(&counts, "f3d direct face comparison count source")? {
                    ctx.push_vec(&mut copied, *count, "f3d direct face comparison count copy")?;
                }
                Ok(copied)
            },
        )?;
        if !ctx.all_by(
            &candidate,
            |face| {
                let Some(index) = ctx
                    .get_hash_map(
                        &positions,
                        &face.key(),
                        "f3d direct face candidate comparison",
                    )?
                    .copied()
                else {
                    return Ok(false);
                };
                if unmatched[index] == 0 {
                    return Ok(false);
                }
                unmatched[index] -= 1;
                Ok(true)
            },
            "f3d direct face candidate source",
        )? {
            return Ok(None);
        }
    }
    let (mut faces, face_storage) = ctx.with_scoped_storage(
        "f3d direct face selection storage",
        || -> Result<_, CodecError> {
            let mut faces = Vec::new();
            for candidate in ctx.admit_iter(&first, "f3d direct face selected output source")? {
                let face = candidate.materialize(ctx)?;
                ctx.push_vec(&mut faces, face, "f3d direct face selected output")?;
            }
            Ok(faces)
        },
    )?;
    ctx.stable_sort_by(
        &mut faces[..],
        |value| value.as_str(),
        Ord::cmp,
        "sort f3d design face_resolve 2",
    )?;
    let faces = face_storage.commit_value(faces)?;
    let native = ctx.copy_retained_text(&scope.id, "f3d direct face native id")?;
    Ok(Some(FaceSelection::Resolved { faces, native }))
}

/// Resolve a face operand whose exact preceding topology proves one face.
///
/// This path is for single-face operands whose recipe carries the selected face
/// in its persistent-reference lane but whose active candidate lane is not a
/// current-face slot. The caller must still admit the operand's exact recipe
/// form; this helper only applies the unique historical-face proof.
pub(super) fn resolved_historical_face_operand(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    operand: &DesignFaceOperand,
) -> Result<Option<cadmpeg_ir::features::FaceSelection>, CodecError> {
    let Some(previous_state_id) = scope.previous_history_state_id() else {
        return Ok(None);
    };
    let Some(face_slot) = resolve_face_operand_history_candidates(ctx, operand)? else {
        return Ok(None);
    };
    historical_face_selection_with_native(
        ctx,
        scope,
        previous_state_id,
        &[face_slot],
        &operand.id,
        "f3d historical face operand id",
    )
}

/// Resolve the complete input-state body boundaries selected by a body-recipe
/// group. Persistent-reference candidate faces identify each body; they do
/// not define a partial target boundary.
pub(super) fn resolved_body_recipe_selection(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    operands: &[DesignBodyRecipeOperand],
) -> Result<Option<cadmpeg_ir::features::FaceSelection>, CodecError> {
    if group.scope_record_index != scope.record_index
        || group.extrude_role().is_some()
        || group.members().is_empty()
    {
        return Ok(None);
    }
    let Some(stream) = native_stream_charged(ctx, &group.id, "f3d face group native stream")?
    else {
        return Ok(None);
    };
    if !crate::design::feature_project::feature_stream_matches(
        ctx,
        &scope.id,
        Some(stream),
        "f3d body recipe scope stream",
    )? {
        return Ok(None);
    }
    let mut faces = Vec::new();
    let mut state_id = None;
    let mut member_records = HashSet::new();
    let mut proof_storage = ctx.reserve_scoped(0, "f3d body recipe proof storage")?;
    let mut source = group.members().iter().enumerate();
    while let Some((ordinal, member)) =
        ctx.next_charged(&mut source, "f3d face group member source")?
    {
        let record_index = &member.value;
        if !proof_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut member_records,
                *record_index,
                "f3d body recipe member index",
            )
        })? {
            return Ok(None);
        }
        let Some(ordinal) = u32::try_from(ordinal).ok() else {
            return Ok(None);
        };
        let operand = unique_feature_match_for_decode(
            ctx,
            operands,
            |operand| {
                Ok(crate::design::feature_project::feature_stream_matches(
                    ctx,
                    &operand.id,
                    Some(stream),
                    "f3d face member operand stream",
                )? && operand.scope_record_index == group.scope_record_index
                    && operand.owner.group() == Some((group.record_index, ordinal))
                    && operand.record_index() == *record_index)
            },
            "f3d face group operand source",
        )?;
        let Some(operand) = operand else {
            return Ok(None);
        };
        if operand.references().is_empty()
            || operand.resolved_body_slot.is_none()
            || operand.resolved_body_face_slots.is_empty()
        {
            return Ok(None);
        }
        let Some(operand_state_id) = operand.resolved_body_state_id else {
            return Ok(None);
        };
        match state_id {
            None => state_id = Some(operand_state_id),
            Some(expected) if expected == operand_state_id => {}
            Some(_) => return Ok(None),
        }
        for face in ctx.admit_iter(
            &operand.resolved_body_face_slots,
            "f3d body recipe selected slot source",
        )? {
            if !contains_slot(ctx, &faces, *face, "find F3D body recipe face slot")? {
                proof_storage.with_storage(|| {
                    ctx.push_vec(&mut faces, *face, "f3d body recipe face slot")
                })?;
            }
        }
    }
    let Some(state_id) = state_id else {
        return Ok(None);
    };
    historical_face_selection_in_state(ctx, scope, group, state_id, &faces)
}

/// Resolve the complete input-state body boundaries selected by an Extrude
/// target-shape group. Persistent-reference candidate faces identify each
/// body; they do not define a partial target boundary.
pub(super) fn resolved_body_recipe_shape(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    operands: &[DesignBodyRecipeOperand],
) -> Result<Option<cadmpeg_ir::features::FaceSelection>, CodecError> {
    if crate::design::design_feature_family(&scope.kind())
        != Some(crate::design::DesignFeatureFamily::Extrude)
        || group.role() != DesignOperandRole::ROLE_0X5
    {
        return Ok(None);
    }
    resolved_body_recipe_selection(ctx, scope, group, operands)
}

fn historical_profile_native_group(
    ctx: &DecodeContext<'_>,
    native: cadmpeg_core::text::NonBlankString,
) -> Result<cadmpeg_ir::features::NativeSelections, CodecError> {
    let native = native.into_string(ctx, "f3d historical profile native id")?;
    let mut members = ctx.collection_vec(1, "f3d historical profile native group")?;
    // The singleton slot is already admitted and cannot grow the allocation.
    members.push(native);
    cadmpeg_ir::features::NativeSelections::new(
        members,
        ctx,
        "f3d historical profile native validation",
    )?
    .map_err(CodecError::malformed)
}

pub(super) fn resolved_profile_face_group(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    operands: &[DesignFaceOperand],
) -> Result<Option<cadmpeg_ir::features::ProfileRef>, CodecError> {
    use cadmpeg_ir::features::ProfileRef;

    let Some(selection) = resolved_historical_face_group(
        ctx,
        scope,
        scope.previous_history_state_id(),
        group,
        operands,
    )?
    else {
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
    Ok(Some(ProfileRef::Planar(
        cadmpeg_ir::features::PlanarProfileRef::HistoricalFaces {
            state,
            faces,
            native: historical_profile_native_group(ctx, native)?,
        },
    )))
}

/// Return the top-level profile groups of one Extrude operand hierarchy.
///
/// A profile group named by another profile group's member table is a child
/// selection, not an additional profile consumed by the Extrude. The complete
/// hierarchy must be acyclic, and each child can have exactly one parent.
pub(crate) fn extrude_profile_group_roots<'a>(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    groups: &'a [DesignConstructionOperandGroup],
) -> Result<Option<Vec<&'a DesignConstructionOperandGroup>>, CodecError> {
    use crate::records::topology::extrude_selection::DesignExtrudeOperandRole;

    let Some(stream) = native_stream_charged(ctx, &scope.id, "f3d Extrude profile native stream")?
    else {
        return Ok(None);
    };
    let mut proof_storage = ctx.reserve_scoped(0, "f3d Extrude profile proof storage")?;
    let mut profile_groups = Vec::new();
    let mut group_source = groups.iter();
    while let Some(group) =
        ctx.next_charged(&mut group_source, "f3d Extrude profile group source")?
    {
        if !(crate::design::feature_project::feature_stream_matches(
            ctx,
            &group.id,
            Some(stream),
            "f3d Extrude profile group stream",
        )? && group.scope_record_index == scope.record_index
            && group.extrude_role() == Some(DesignExtrudeOperandRole::Profile))
        {
            continue;
        }
        proof_storage.with_storage(|| {
            ctx.push_vec(&mut profile_groups, group, "f3d Extrude profile group")
        })?;
    }
    ctx.stable_sort_by_key(
        &mut profile_groups[..],
        |value| {
            let group = value;
            {
                group.scope_reference_ordinal
            }
        },
        Ord::cmp,
        "sort f3d design face_resolve 4",
    )?;
    if ctx.any_by(
        profile_groups.windows(2),
        |groups| {
            Ok(
                groups[0].scope_reference_ordinal == groups[1].scope_reference_ordinal
                    || groups[0].record_index == groups[1].record_index,
            )
        },
        "f3d Extrude profile ordinal uniqueness",
    )? {
        return Ok(None);
    }

    let mut groups_by_record = HashMap::new();
    for group in ctx.admit_iter(&profile_groups, "f3d Extrude profile index source")? {
        // discarded-value: duplicate record indices are rejected by the length check below.
        let _ = proof_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut groups_by_record,
                group.record_index,
                *group,
                "f3d Extrude profile group index",
            )
        })?;
    }
    if groups_by_record.len() != profile_groups.len() {
        return Ok(None);
    }
    let mut parent_by_child = HashMap::new();
    let mut parent_source = profile_groups.iter();
    while let Some(parent) =
        ctx.next_charged(&mut parent_source, "f3d Extrude profile parent source")?
    {
        let mut members = parent.members().iter();
        while let Some(member) =
            ctx.next_charged(&mut members, "f3d Extrude profile parent member source")?
        {
            let member = &member.value;
            let Some(child) = ctx.get_hash_map(
                &groups_by_record,
                member,
                "f3d Extrude profile child lookup",
            )?
            else {
                continue;
            };
            if child.scope_reference_ordinal <= parent.scope_reference_ordinal {
                return Ok(None);
            }
            if proof_storage
                .with_storage(|| {
                    ctx.insert_hash_map(
                        &mut parent_by_child,
                        child.record_index,
                        parent.record_index,
                        "f3d Extrude profile parent index",
                    )
                })?
                .is_some()
            {
                return Ok(None);
            }
        }
    }
    let mut roots = Vec::new();
    for group in ctx.admit_iter(&profile_groups, "f3d Extrude profile root source")? {
        let group = *group;
        if ctx.contains_key_hash_map(
            &parent_by_child,
            &group.record_index,
            "f3d Extrude profile parent lookup",
        )? {
            continue;
        }
        ctx.push_vec(&mut roots, group, "f3d Extrude profile root")?;
    }
    if !profile_groups.is_empty() && roots.is_empty() {
        return Ok(None);
    }

    let mut visited = HashSet::new();
    let mut root_source = roots.iter();
    while let Some(root) =
        ctx.next_charged(&mut root_source, "f3d Extrude profile validation source")?
    {
        if !proof_storage.with_storage(|| {
            visit_extrude_profile_group(ctx, root, &groups_by_record, &mut visited)
        })? {
            return Ok(None);
        }
    }
    if visited.len() != profile_groups.len() {
        return Ok(None);
    }
    Ok(Some(roots))
}

fn visit_extrude_profile_group(
    ctx: &DecodeContext<'_>,
    group: &DesignConstructionOperandGroup,
    groups_by_record: &HashMap<u32, &DesignConstructionOperandGroup>,
    visited: &mut HashSet<u32>,
) -> Result<bool, CodecError> {
    let _depth = Some((ctx.enter_nested("f3d Extrude profile hierarchy"))?);
    if !ctx.insert_hash_set(
        visited,
        group.record_index,
        "f3d Extrude profile visited group",
    )? {
        return Ok(false);
    }
    let mut members = group.members().iter();
    while let Some(member) =
        ctx.next_charged(&mut members, "f3d Extrude profile hierarchy member source")?
    {
        let member = &member.value;
        if let Some(child) = ctx.get_hash_map(
            groups_by_record,
            member,
            "f3d Extrude profile hierarchy child lookup",
        )? {
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
    ctx: &DecodeContext<'_>,
    root: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignFaceOperand],
) -> Result<Option<Vec<usize>>, CodecError> {
    use crate::records::topology::extrude_selection::DesignExtrudeOperandRole;

    let Some(stream) = native_stream_charged(ctx, &root.id, "f3d Extrude profile native stream")?
    else {
        return Ok(None);
    };
    let mut proof_storage = ctx.reserve_scoped(0, "f3d Extrude profile proof storage")?;
    let mut profile_groups = Vec::new();
    let mut group_source = groups.iter();
    while let Some(group) = ctx.next_charged(&mut group_source, "f3d Extrude leaf group source")? {
        if !(crate::design::feature_project::feature_stream_matches(
            ctx,
            &group.id,
            Some(stream),
            "f3d Extrude leaf group stream",
        )? && group.scope_record_index == root.scope_record_index
            && group.extrude_role() == Some(DesignExtrudeOperandRole::Profile))
        {
            continue;
        }
        proof_storage.with_storage(|| {
            ctx.push_vec(&mut profile_groups, group, "f3d Extrude leaf profile group")
        })?;
    }
    let mut groups_by_record = HashMap::new();
    for group in ctx.admit_iter(&profile_groups, "f3d Extrude profile index source")? {
        // discarded-value: duplicate record indices are rejected by the length check below.
        let _ = proof_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut groups_by_record,
                group.record_index,
                *group,
                "f3d Extrude leaf group index",
            )
        })?;
    }
    if groups_by_record.len() != profile_groups.len() {
        return Ok(None);
    }
    let Some(found) = ctx.get_hash_map(
        &groups_by_record,
        &root.record_index,
        "f3d Extrude leaf root lookup",
    )?
    else {
        return Ok(None);
    };
    if !std::ptr::eq(*found, root) && !ctx.equal(*found, root, "f3d Extrude leaf root equality")? {
        return Ok(None);
    }

    let mut visited_groups = HashSet::new();
    let mut visited_storage = ctx.reserve_scoped(0, "f3d Extrude leaf visited storage")?;
    collect_extrude_profile_group_operands(
        ctx,
        root,
        stream,
        &groups_by_record,
        operands,
        &mut visited_groups,
        &mut visited_storage,
    )
}

fn collect_extrude_profile_group_operands(
    ctx: &DecodeContext<'_>,
    group: &DesignConstructionOperandGroup,
    stream: &str,
    groups_by_record: &HashMap<u32, &DesignConstructionOperandGroup>,
    operands: &[DesignFaceOperand],
    visited_groups: &mut HashSet<u32>,
    visited_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
) -> Result<Option<Vec<usize>>, CodecError> {
    let _depth = Some((ctx.enter_nested("f3d Extrude leaf hierarchy"))?);
    if group.members().is_empty()
        || !visited_storage.with_storage(|| {
            ctx.insert_hash_set(
                visited_groups,
                group.record_index,
                "f3d Extrude leaf visited group",
            )
        })?
    {
        return Ok(None);
    }
    let mut indices = Vec::new();
    let mut members = group.members().iter().enumerate();
    while let Some((ordinal, member)) =
        ctx.next_charged(&mut members, "f3d Extrude leaf member source")?
    {
        let record_index = &member.value;
        let Some(ordinal) = u32::try_from(ordinal).ok() else {
            return Ok(None);
        };
        let mut direct = operands.iter().enumerate();
        let mut matches =
            |(index, operand): (usize, &DesignFaceOperand)| -> Result<Option<usize>, CodecError> {
                Ok((crate::design::feature_project::feature_stream_matches(
                    ctx,
                    &operand.id,
                    Some(stream),
                    "f3d Extrude leaf operand stream",
                )? && operand.scope_record_index == group.scope_record_index
                    && operand.group_record_index() == Some(group.record_index)
                    && operand.group_member_ordinal() == Some(ordinal)
                    && operand.record_index() == *record_index)
                    .then_some(index))
            };
        let first = ctx.find_map(&mut direct, &mut matches, "f3d Extrude leaf operand source")?;
        let second = ctx.find_map(direct, matches, "f3d Extrude leaf operand source")?;
        let child = ctx
            .get_hash_map(
                groups_by_record,
                record_index,
                "f3d Extrude leaf child lookup",
            )?
            .copied();
        let index = match ((first, second), child) {
            ((Some(index), None), None) => index,
            ((None, None), Some(child))
                if child.scope_reference_ordinal > group.scope_reference_ordinal =>
            {
                let child_indices = collect_extrude_profile_group_operands(
                    ctx,
                    child,
                    stream,
                    groups_by_record,
                    operands,
                    visited_groups,
                    visited_storage,
                )?;
                let Some(child_indices) = child_indices else {
                    return Ok(None);
                };
                let [index] = child_indices.as_slice() else {
                    return Ok(None);
                };
                *index
            }
            _ => return Ok(None),
        };

        if !indices.is_empty()
            && ctx.any_by(
                &indices,
                |candidate| Ok(*candidate == index),
                "find F3D Extrude leaf index",
            )?
        {
            return Ok(None);
        }
        ctx.push_vec(&mut indices, index, "f3d Extrude leaf index")?;
    }
    Ok(Some(indices))
}

/// Whether every root member is one exact paired-reference face subgroup.
pub(crate) fn is_paired_extrude_profile_aggregate(
    ctx: &DecodeContext<'_>,
    root: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignFaceOperand],
) -> Result<bool, CodecError> {
    use crate::records::{
        recipes::ConstructionRecipeKind, topology::extrude_selection::DesignExtrudeOperandRole,
    };

    let Some(stream) =
        native_stream_charged(ctx, &root.id, "f3d paired Extrude profile native stream")?
    else {
        return Ok(false);
    };
    if root.members().is_empty() {
        return Ok(false);
    }
    let mut members = root.members().iter();
    while let Some(member) = ctx.next_charged(&mut members, "f3d paired Extrude member source")? {
        let record_index = &member.value;
        let child = unique_feature_match_for_decode(
            ctx,
            groups,
            |group| {
                Ok(crate::design::feature_project::feature_stream_matches(
                    ctx,
                    &group.id,
                    Some(stream),
                    "f3d paired Extrude child stream",
                )? && group.scope_record_index == root.scope_record_index
                    && group.record_index == *record_index
                    && group.scope_reference_ordinal > root.scope_reference_ordinal
                    && group.extrude_role() == Some(DesignExtrudeOperandRole::Profile))
            },
            "f3d paired Extrude child source",
        )?;
        let Some(child) = child else {
            return Ok(false);
        };
        let [crate::records::identity::Located {
            value: operand_record_index,
            ..
        }] = child.members()
        else {
            return Ok(false);
        };
        let leaf = unique_feature_match_for_decode(
            ctx,
            operands,
            |operand| {
                Ok(crate::design::feature_project::feature_stream_matches(
                    ctx,
                    &operand.id,
                    Some(stream),
                    "f3d paired Extrude leaf stream",
                )? && operand.scope_record_index == root.scope_record_index
                    && operand.group_record_index() == Some(child.record_index)
                    && operand.group_member_ordinal() == Some(0)
                    && operand.record_index() == *operand_record_index
                    && operand.recipe_kind == ConstructionRecipeKind::BoundedFace
                    && crate::design::decode::dimension_frames::is_paired_recipe_reference_frame(
                        ctx,
                        &operand.recipe_prefix_bytes,
                    )?)
            },
            "f3d paired Extrude leaf source",
        )?;
        if leaf.is_none() {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Resolve one top-level Extrude profile group through its exact leaf operands.
///
/// A complete active bounded-face lane is represented directly when the
/// operand has no historical lane. Otherwise the selected faces must be
/// proven in the consuming feature's preceding topology.
pub(crate) fn resolved_extrude_profile_face_group(
    ctx: &DecodeContext<'_>,
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
    for index in ctx.admit_iter(&indices, "f3d Extrude historical operand source")? {
        let Some(operand) = operands.get(*index) else {
            return Ok(None);
        };
        let slots = &operand.resolved_face_slots;
        if slots.is_empty() {
            return Ok(None);
        }
        for slot in ctx.admit_iter(slots, "f3d Extrude historical slot source")? {
            if !contains_slot(ctx, &faces, *slot, "find F3D Extrude historical face slot")? {
                ctx.push_vec(&mut faces, *slot, "f3d Extrude historical face slot")?;
            }
        }
    }
    let Some(selection) = historical_face_selection(ctx, scope, root, &faces)? else {
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
            native: historical_profile_native_group(ctx, native)?,
        },
    )))
}

fn resolved_extrude_profile_active_faces(
    ctx: &DecodeContext<'_>,
    indices: &[usize],
    operands: &[DesignFaceOperand],
) -> Result<Option<Vec<cadmpeg_ir::ids::FaceId>>, CodecError> {
    if indices.is_empty() {
        return Ok(None);
    }
    let mut storage = ctx.reserve_scoped(0, "f3d Extrude active face proof storage")?;
    let mut seen = HashSet::new();
    let mut selected = Vec::new();
    let mut source = indices.iter();
    while let Some(index) = ctx.next_charged(&mut source, "f3d Extrude active operand source")? {
        let Some(operand) = operands.get(*index) else {
            return Ok(None);
        };
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
        for face in ctx.admit_iter(
            &operand.candidate_faces,
            "f3d Extrude active candidate source",
        )? {
            storage.with_storage(|| -> Result<(), CodecError> {
                if ctx.insert_hash_set(&mut seen, face, "f3d Extrude active face membership")? {
                    ctx.push_vec(&mut selected, face, "f3d Extrude active face selection")?;
                }
                Ok(())
            })?;
        }
    }
    if selected.is_empty() {
        return Ok(None);
    }
    let mut faces = Vec::new();
    for face in ctx.admit_iter(&selected, "f3d Extrude active selected source")? {
        let face = (*face).try_clone_for_decode(ctx, "f3d Extrude active face id")?;
        ctx.push_vec(&mut faces, face, "f3d Extrude active face")?;
    }
    Ok(Some(faces))
}

/// Resolve a Loft section whose members use the edge-recipe envelope.
///
/// These members describe the selected face through a common persistent
/// selector/token clause. The clause must identify one direct preceding face
/// in every member, and its complete boundary must have exactly one edge per
/// group member. Any competing common clause or incomplete topology context
/// keeps the native group unresolved.
pub(super) fn resolved_loft_edge_profile_group(
    ctx: &DecodeContext<'_>,
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
    let Some(previous_state_id) = scope.previous_history_state_id() else {
        return Ok(None);
    };
    let Some(stream) = native_stream_charged(ctx, &group.id, "f3d Loft profile native stream")?
    else {
        return Ok(None);
    };
    let Some(group_ordinal) = usize::try_from(group.scope_reference_ordinal).ok() else {
        return Ok(None);
    };
    let mut proof_storage = ctx.reserve_scoped(0, "f3d Loft edge profile proof storage")?;
    let mut member_ids = HashSet::new();
    let mut members = group.members().iter();
    while let Some(member) =
        ctx.next_charged(&mut members, "f3d Loft edge member uniqueness source")?
    {
        let member = member.value;
        if !proof_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut member_ids,
                member,
                "f3d Loft edge profile member index",
            )
        })? {
            return Ok(None);
        }
    }
    if scope.reference_members().values().nth(group_ordinal) != Some(&group.record_index) {
        return Ok(None);
    }
    let mut member_operands = Vec::new();
    let mut members = group.members().iter().enumerate();
    while let Some((ordinal, member)) =
        ctx.next_charged(&mut members, "f3d Loft edge profile member source")?
    {
        let record_index = &member.value;
        let Some(operand) = (|| -> Result<Option<&DesignEdgeOperand>, CodecError> {
            let Some(scope_ordinal) = u32::try_from(ordinal).ok().and_then(|ordinal| {
                group
                    .scope_reference_ordinal
                    .checked_add(1)?
                    .checked_add(ordinal)
            }) else {
                return Ok(None);
            };
            let Some(member_ordinal) = group_ordinal
                .checked_add(ordinal)
                .and_then(|index| index.checked_add(1))
            else {
                return Ok(None);
            };
            if scope.reference_members().values().nth(member_ordinal) != Some(record_index) {
                return Ok(None);
            }
            let operand = unique_feature_match_for_decode(
                ctx,
                operands,
                |operand| {
                    Ok(crate::design::feature_project::feature_stream_matches(
                        ctx,
                        &operand.id,
                        Some(stream),
                        "f3d Loft edge profile operand stream",
                    )? && operand.scope_record_index == group.scope_record_index
                        && operand.record_index() == *record_index)
                },
                "f3d Loft edge profile operand source",
            )?;
            let Some(operand) = operand else {
                return Ok(None);
            };
            if operand.scope_reference_ordinal != scope_ordinal
                || operand.recipe_state_id != Some(previous_state_id)
                || operand.recipe_structure.is_none()
                || operand.surface_patch_recipe_structure.is_some()
            {
                return Ok(None);
            }
            Ok(Some(operand))
        })()?
        else {
            return Ok(None);
        };
        proof_storage.with_storage(|| {
            ctx.push_vec(
                &mut member_operands,
                operand,
                "f3d Loft edge profile member",
            )
        })?;
    }
    let Some(face_slot) =
        loft_edge_profile_face_slot(group.members().len(), &member_operands, ctx)?
    else {
        return Ok(None);
    };
    let Some(selection) = historical_face_selection(ctx, scope, group, &[face_slot])? else {
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
            native: historical_profile_native_group(ctx, native)?,
        },
    )))
}

fn loft_edge_profile_face_slot(
    member_count: usize,
    operands: &[&DesignEdgeOperand],
    ctx: &DecodeContext<'_>,
) -> Result<Option<i64>, CodecError> {
    if member_count == 0 || operands.len() != member_count {
        return Ok(None);
    }
    let matches_clause =
        |candidate: &crate::records::dimensions::DesignRecipeReference,
         reference: &crate::records::dimensions::DesignRecipeReference| {
            Ok(candidate.selector == reference.selector
                && ctx.equal(
                    candidate.token.as_str(),
                    reference.token.as_str(),
                    "f3d Loft face clause token",
                )?
                && candidate.design_reference == reference.design_reference
                && candidate.candidate_faces.len() == 1
                && candidate.alternate_selector_faces.is_empty())
        };
    let mut common_clause: Option<(i64, &str, i64)> = None;
    let mut references = operands[0].recipe_references.iter();
    while let Some(reference) = ctx.next_charged(&mut references, "f3d Loft face clause scan")? {
        if reference.candidate_faces.len() != 1 || !reference.alternate_selector_faces.is_empty() {
            continue;
        }
        if !ctx.all_by(
            operands,
            |operand| {
                let mut clauses = operand.recipe_references.iter();
                let first = ctx.find_by(
                    clauses.by_ref(),
                    |candidate| matches_clause(candidate, reference),
                    "f3d Loft face clause uniqueness",
                )?;
                Ok(first.is_some()
                    && !ctx.any_by(
                        clauses,
                        |candidate| matches_clause(candidate, reference),
                        "f3d Loft face clause uniqueness",
                    )?)
            },
            "f3d Loft face common clause operands",
        )? {
            continue;
        }
        if let Some((selector, token, design_reference)) = common_clause {
            if selector != reference.selector
                || !ctx.equal(
                    token,
                    reference.token.as_str(),
                    "f3d Loft face common token",
                )?
                || design_reference != reference.design_reference
            {
                return Ok(None);
            }
        } else {
            common_clause = Some((
                reference.selector,
                reference.token.as_str(),
                reference.design_reference,
            ));
        }
    }
    let Some((selector, token, design_reference)) = common_clause else {
        return Ok(None);
    };
    let mut evidence = None;
    let mut members = operands.iter();
    while let Some(operand) = ctx.next_charged(&mut members, "f3d Loft face operand scan")? {
        if operand.recipe_references.len() != operand.recipe_reference_contexts.len()
            || operand.candidate_faces.is_empty()
            || operand.preceding_candidate_faces.is_empty()
            || operand.result_candidate_faces.is_empty()
        {
            return Ok(None);
        }
        let Some(target_ordinal) = ctx.position_by(
            &operand.recipe_references,
            |reference| {
                Ok(reference.selector == selector
                    && ctx.equal(
                        reference.token.as_str(),
                        token,
                        "f3d Loft face target token",
                    )?
                    && reference.design_reference == design_reference
                    && reference.candidate_faces.len() == 1
                    && reference.alternate_selector_faces.is_empty())
            },
            "f3d Loft face target clause",
        )?
        else {
            return Ok(None);
        };
        let mut target = None;
        let mut contexts = operand
            .recipe_references
            .iter()
            .zip(&operand.recipe_reference_contexts)
            .enumerate();
        while let Some((ordinal, (reference, context))) =
            ctx.next_charged(&mut contexts, "f3d Loft face context scan")?
        {
            let Ok(reference_ordinal) = u32::try_from(ordinal) else {
                return Ok(None);
            };
            if context.reference_ordinal != reference_ordinal {
                return Ok(None);
            }
            let [candidate] = reference.candidate_faces.as_slice() else {
                return Ok(None);
            };
            let [preceding_face] = context.preceding_faces.as_slice() else {
                return Ok(None);
            };
            if !ctx.equal(
                preceding_face,
                candidate,
                "f3d Loft preceding face identity",
            )? {
                return Ok(None);
            }
            let Some((_, slot)) =
                ctx.rsplit_once(preceding_face.as_str(), "#", "f3d Loft face slot suffix")?
            else {
                return Ok(None);
            };
            let parsed_slot = if slot.len() <= MAX_FREE_I64_TEXT_BYTES {
                slot.parse::<i64>()
            } else {
                ctx.parse_text::<i64>(slot, "f3d Loft face slot value")?
            };
            let Ok(slot) = parsed_slot else {
                return Ok(None);
            };
            if !ctx.contains(
                &operand.preceding_candidate_faces,
                candidate,
                "f3d Loft preceding face membership",
            )? || !ctx.contains(
                &operand.result_candidate_faces,
                candidate,
                "f3d Loft result face membership",
            )? {
                return Ok(None);
            }
            let [preceding_boundary] = context.preceding_face_boundaries.as_slice() else {
                return Ok(None);
            };
            let mut boundary_edge_count = 0usize;
            let mut loops = preceding_boundary.loops.iter();
            while let Some(loop_) =
                ctx.next_charged(&mut loops, "f3d Loft face boundary loop scan")?
            {
                let count = loop_.boundary.coedge_count();
                if count == 0 {
                    return Ok(None);
                }
                let Some(total) = boundary_edge_count.checked_add(count) else {
                    return Ok(None);
                };
                boundary_edge_count = total;
            }
            if preceding_boundary.face_slot != slot {
                return Ok(None);
            }
            let [result_face] = context.result_faces.as_slice() else {
                return Ok(None);
            };
            let [result_boundary] = context.result_face_boundaries.as_slice() else {
                return Ok(None);
            };
            if !ctx.equal(result_face, candidate, "f3d Loft result face identity")?
                || result_boundary.face_slot != slot
            {
                return Ok(None);
            }
            let [support_slot] = context.preceding_support_face_slots.as_slice() else {
                return Ok(None);
            };
            let [support_boundary] = context.preceding_support_face_boundaries.as_slice() else {
                return Ok(None);
            };
            if *support_slot != slot || support_boundary.face_slot != slot {
                return Ok(None);
            }
            if ordinal == target_ordinal {
                target = Some((candidate, slot, boundary_edge_count));
            }
        }
        let Some((candidate, slot, count)) = target else {
            return Ok(None);
        };
        if count != member_count {
            return Ok(None);
        }
        if let Some((face, prior_slot, prior_count)) = evidence {
            if !ctx.equal(candidate, face, "f3d Loft common face identity")?
                || slot != prior_slot
                || count != prior_count
            {
                return Ok(None);
            }
        } else {
            evidence = Some((candidate, slot, count));
        }
    }
    Ok(evidence.map(|(_, slot, _)| slot))
}

pub(super) fn resolved_historical_face_group(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    previous_state_id: Option<i64>,
    group: &DesignConstructionOperandGroup,
    operands: &[DesignFaceOperand],
) -> Result<Option<cadmpeg_ir::features::FaceSelection>, CodecError> {
    let Some(faces) =
        historical_face_group_slots(ctx, group, operands, FaceGroupMembers::Resolved)?
    else {
        return Ok(None);
    };
    let Some(previous_state_id) = previous_state_id else {
        return Ok(None);
    };
    historical_face_selection_in_state(ctx, scope, group, previous_state_id, &faces)
}

fn historical_face_selection(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    faces: &[i64],
) -> Result<Option<cadmpeg_ir::features::FaceSelection>, CodecError> {
    let Some(previous_state_id) = scope.previous_history_state_id() else {
        return Ok(None);
    };
    historical_face_selection_in_state(ctx, scope, group, previous_state_id, faces)
}

fn historical_face_selection_in_state(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    previous_state_id: i64,
    faces: &[i64],
) -> Result<Option<cadmpeg_ir::features::FaceSelection>, CodecError> {
    historical_face_selection_with_native(
        ctx,
        scope,
        previous_state_id,
        faces,
        &group.id,
        "f3d historical face group id",
    )
}

fn historical_face_selection_with_native(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    previous_state_id: i64,
    faces: &[i64],
    native: &str,
    native_operation: &'static str,
) -> Result<Option<cadmpeg_ir::features::FaceSelection>, CodecError> {
    use cadmpeg_ir::features::FaceSelection;
    if faces.is_empty() {
        return Ok(None);
    }
    let ((feature, prefix), _identity_storage) =
        ctx.with_scoped_storage("f3d historical face identity storage", || {
            let feature = crate::design::identity::neutral_feature_id(ctx, scope)?;
            let feature_key = crate::design::identity::identity_key(ctx, feature.as_str())?;
            let prefix =
                crate::design::identity::history_input_prefix(ctx, feature_key, previous_state_id)?;
            Ok::<_, CodecError>((feature, prefix))
        })?;
    let (selection, selection_storage) =
        ctx.with_scoped_storage("f3d historical face selection storage", || {
            let historical_native = ctx.copy_retained_text(native, native_operation)?;
            let mut historical_faces = Vec::new();
            for face in ctx.admit_iter(faces, "f3d historical face slot source")? {
                let id = historical_face_id(ctx, &prefix, *face)?;
                ctx.push_vec(&mut historical_faces, id, "f3d historical face member")?;
            }
            let selection = FaceSelection::historical(
                crate::design::identity::feature_input_topology_id(
                    ctx,
                    &feature,
                    previous_state_id,
                )?,
                historical_faces,
                historical_native,
                ctx,
            )?;
            Ok::<_, CodecError>(selection)
        })?;
    match selection {
        Ok(selection) => selection_storage.commit_value(Some(selection)),
        Err(_) => {
            drop(selection_storage);
            Ok(Some(FaceSelection::Native(ctx.copy_retained_text(
                native,
                "f3d historical face fallback id",
            )?)))
        }
    }
}

/// Resolve `SplitFace` target groups whose bounded-face member run can include
/// complete nested support recipes without their own active candidate lanes.
/// A nested support recipe contributes only when history proves its preceding
/// face slots. An unresolved support recipe remains a context member. Every
/// other member must prove its preceding face slots, and at least one member
/// must contribute a face.
pub(super) fn resolved_historical_split_face_target_group(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    previous_state_id: Option<i64>,
    group: &DesignConstructionOperandGroup,
    operands: &[DesignFaceOperand],
) -> Result<Option<cadmpeg_ir::features::FaceSelection>, CodecError> {
    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::SplitFace
        || group.role() != DesignOperandRole::ROLE_0X10
    {
        return Ok(None);
    }
    let Some(faces) =
        historical_face_group_slots(ctx, group, operands, FaceGroupMembers::SplitFaceContext)?
    else {
        return Ok(None);
    };
    let Some(previous_state_id) = previous_state_id else {
        return Ok(None);
    };
    historical_face_selection_in_state(ctx, scope, group, previous_state_id, &faces)
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
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    previous_state_id: Option<i64>,
    group: &DesignConstructionOperandGroup,
    operands: &[DesignFaceOperand],
    updated_face_slots: &[i64],
) -> Result<Option<cadmpeg_ir::features::FaceSelection>, CodecError> {
    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::SplitFace
        || group.role() != DesignOperandRole::ROLE_0X10
    {
        return Ok(None);
    }
    if let Some(selection) =
        resolved_historical_split_face_target_group(ctx, scope, previous_state_id, group, operands)?
    {
        return Ok(Some(selection));
    }
    let Some(faces) =
        split_face_updated_target_slots(ctx, scope, group, operands, updated_face_slots)?
    else {
        return Ok(None);
    };
    let Some(previous_state_id) = previous_state_id else {
        return Ok(None);
    };
    historical_face_selection_in_state(ctx, scope, group, previous_state_id, &faces)
}

fn split_face_updated_target_slots(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    operands: &[DesignFaceOperand],
    updated_face_slots: &[i64],
) -> Result<Option<Vec<i64>>, CodecError> {
    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::SplitFace
        || group.role() != DesignOperandRole::ROLE_0X10
        || updated_face_slots.is_empty()
        || updated_face_slots.len() != group.members().len()
    {
        return Ok(None);
    }
    let mut proof_storage = ctx.reserve_scoped(0, "f3d SplitFace updated proof storage")?;
    let mut updated = HashSet::new();
    for slot in ctx.admit_iter(updated_face_slots, "f3d SplitFace updated slot source")? {
        proof_storage.with_storage(|| {
            ctx.insert_hash_set(&mut updated, *slot, "f3d SplitFace updated face index")
        })?;
    }
    if updated.len() != updated_face_slots.len() {
        return Ok(None);
    }
    let Some(stream) = native_stream_charged(ctx, &group.id, "f3d face group native stream")?
    else {
        return Ok(None);
    };
    let mut represented = HashSet::new();
    let mut faces = Vec::new();
    let mut source = group.members().iter().enumerate();
    while let Some((ordinal, member)) =
        ctx.next_charged(&mut source, "f3d face group member source")?
    {
        let record_index = &member.value;
        let Some(ordinal) = u32::try_from(ordinal).ok() else {
            return Ok(None);
        };
        let operand = unique_feature_match_for_decode(
            ctx,
            operands,
            |operand| {
                Ok(crate::design::feature_project::feature_stream_matches(
                    ctx,
                    &operand.id,
                    Some(stream),
                    "f3d face member operand stream",
                )? && operand.scope_record_index == group.scope_record_index
                    && operand.group_record_index() == Some(group.record_index)
                    && operand.group_member_ordinal() == Some(ordinal)
                    && operand.record_index() == *record_index
                    && operand.recipe_kind
                        == crate::records::recipes::ConstructionRecipeKind::BoundedFace)
            },
            "f3d face group operand source",
        )?;
        let Some(operand) = operand else {
            return Ok(None);
        };
        if operand.preceding_candidate_faces.is_empty() {
            return Ok(None);
        }
        let mut candidates = operand.preceding_candidate_faces.iter();
        while let Some(face) =
            ctx.next_charged(&mut candidates, "f3d SplitFace preceding target source")?
        {
            let Some(slot) = stable_face_slot(ctx, face)? else {
                return Ok(None);
            };
            if ctx.contains_hash_set(&updated, &slot, "f3d SplitFace updated slot membership")?
                && proof_storage.with_storage(|| {
                    ctx.insert_hash_set(
                        &mut represented,
                        slot,
                        "f3d SplitFace represented face index",
                    )
                })?
            {
                ctx.push_vec(&mut faces, slot, "f3d SplitFace updated face")?;
            }
        }
    }
    Ok((represented.len() == updated.len()).then_some(faces))
}

#[derive(Clone, Copy)]
enum FaceGroupMembers {
    Resolved,
    SplitFaceContext,
}

fn historical_face_group_slots(
    ctx: &DecodeContext<'_>,
    group: &DesignConstructionOperandGroup,
    operands: &[DesignFaceOperand],
    members: FaceGroupMembers,
) -> Result<Option<Vec<i64>>, CodecError> {
    let Some(stream) = native_stream_charged(ctx, &group.id, "f3d face group native stream")?
    else {
        return Ok(None);
    };
    let mut faces = Vec::new();
    let mut contributing_members = 0;
    let mut source = group.members().iter().enumerate();
    while let Some((ordinal, member)) =
        ctx.next_charged(&mut source, "f3d face group member source")?
    {
        let record_index = &member.value;
        let Some(ordinal) = u32::try_from(ordinal).ok() else {
            return Ok(None);
        };
        let operand = unique_feature_match_for_decode(
            ctx,
            operands,
            |operand| {
                Ok(crate::design::feature_project::feature_stream_matches(
                    ctx,
                    &operand.id,
                    Some(stream),
                    "f3d face member operand stream",
                )? && operand.scope_record_index == group.scope_record_index
                    && operand.group_record_index() == Some(group.record_index)
                    && operand.group_member_ordinal() == Some(ordinal)
                    && operand.record_index() == *record_index)
            },
            "f3d face group operand source",
        )?;
        let Some(operand) = operand else {
            return Ok(None);
        };
        let mut candidate_slots = None;
        if operand.resolved_face_slots.is_empty() {
            if matches!(members, FaceGroupMembers::SplitFaceContext) {
                if let Some(slots) = split_face_complete_candidate_slots(ctx, operand)? {
                    candidate_slots = Some(slots);
                } else if is_split_face_context_member(ctx, operand)? {
                    continue;
                } else {
                    return Ok(None);
                }
            } else {
                return Ok(None);
            }
        }
        let member_slots = candidate_slots
            .as_deref()
            .unwrap_or(&operand.resolved_face_slots);
        contributing_members += 1;
        for face in ctx.admit_iter(member_slots, "f3d historical member slot source")? {
            if !contains_slot(ctx, &faces, *face, "find F3D historical face group slot")? {
                ctx.push_vec(&mut faces, *face, "f3d historical face group slot")?;
            }
        }
    }
    Ok((contributing_members > 0 && !faces.is_empty()).then_some(faces))
}

fn split_face_complete_candidate_slots(
    ctx: &DecodeContext<'_>,
    operand: &DesignFaceOperand,
) -> Result<Option<Vec<i64>>, CodecError> {
    if complete_counted_face_recipe(ctx, operand)?.is_none() {
        return Ok(None);
    }
    let faces = if let Some(face) = &operand.resolved_active_face {
        std::slice::from_ref(face)
    } else {
        let candidates = face_operand_candidates(operand);
        if operand.alternate_selector_candidate_faces.is_empty()
            && operand.unreferenced_candidate_faces.is_empty()
            && candidates.len() != 1
        {
            return Ok(None);
        }
        candidates
    };
    let mut storage = ctx.reserve_scoped(0, "f3d SplitFace complete candidate storage")?;
    let mut slots = Vec::new();
    let mut source = faces.iter();
    while let Some(face) =
        ctx.next_charged(&mut source, "f3d SplitFace complete candidate source")?
    {
        let Some(slot) = stable_face_slot(ctx, face)? else {
            return Ok(None);
        };
        storage.with_storage(|| {
            ctx.push_vec(&mut slots, slot, "f3d SplitFace complete candidate slot")
        })?;
    }
    let mut preceding = HashSet::new();
    for face in ctx.admit_iter(
        &operand.preceding_candidate_faces,
        "f3d SplitFace preceding candidate source",
    )? {
        if let Some(slot) = stable_face_slot(ctx, face)? {
            storage.with_storage(|| {
                ctx.insert_hash_set(
                    &mut preceding,
                    slot,
                    "f3d SplitFace preceding candidate index",
                )
            })?;
        }
    }
    if slots.is_empty()
        || !ctx.all_by(
            &slots,
            |slot| ctx.contains_hash_set(&preceding, slot, "f3d SplitFace preceding membership"),
            "f3d SplitFace complete containment",
        )?
    {
        return Ok(None);
    }
    Ok(Some(ctx.collect_vec(
        slots.iter().copied(),
        "f3d SplitFace accepted candidate slots",
    )?))
}

fn is_split_face_context_member(
    ctx: &DecodeContext<'_>,
    operand: &DesignFaceOperand,
) -> Result<bool, CodecError> {
    Ok(
        operand.recipe_kind == crate::records::recipes::ConstructionRecipeKind::BoundedFace
            && matches!(
                crate::design::decode::operands::face_recipe_program_kind(&operand.recipe_program),
                Some(crate::design::decode::operands::FaceRecipeProgramKind::Counted { .. })
            )
            && !operand.recipe_nodes.is_empty()
            && ctx.all_by(
                &operand.recipe_nodes,
                |node| Ok(node.recipe_structure.is_some()),
                "f3d SplitFace context node source",
            )?
            && face_operand_candidates(operand).is_empty()
            && operand.alternate_selector_candidate_faces.is_empty()
            && operand.preceding_candidate_faces.is_empty()
            && operand.changed_candidate_faces.is_empty()
            && ctx.any_by(
                &operand.recipe_references,
                |reference| {
                    Ok(!reference.candidate_faces.is_empty()
                        || !reference.alternate_selector_faces.is_empty())
                },
                "f3d SplitFace context reference source",
            )?,
    )
}

fn stable_face_slot(
    ctx: &DecodeContext<'_>,
    face: &cadmpeg_ir::ids::FaceId,
) -> Result<Option<i64>, CodecError> {
    let Some((_, slot)) = ctx.rsplit_once(face.as_str(), "#", "f3d stable face slot suffix")?
    else {
        return Ok(None);
    };
    Ok(if slot.len() <= MAX_FREE_I64_TEXT_BYTES {
        slot.parse::<i64>().ok()
    } else {
        ctx.parse_text::<i64>(slot, "f3d stable face slot value")?
            .ok()
    })
}

// Source identities and scalar slots remain borrowed until every operand agrees.
#[derive(Clone, Copy)]
enum FaceCandidate<'source> {
    Existing {
        face: &'source cadmpeg_ir::ids::FaceId,
        operation: &'static str,
    },
    Generated(i64),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum FaceKey<'source> {
    Slot(i64),
    Text(&'source cadmpeg_ir::ids::FaceId),
}

impl cadmpeg_core::decode::cost::DecodeCost for FaceKey<'_> {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        match self {
            Self::Slot(_) => Ok(cadmpeg_core::decode::u64_from_index(
                std::mem::size_of::<i64>(),
            )),
            Self::Text(face) => {
                cadmpeg_core::decode::cost::DecodeCost::decode_cost(*face, ctx, operation)
            }
        }
    }
}

impl<'source> FaceCandidate<'source> {
    fn key(self) -> FaceKey<'source> {
        let face = match self {
            Self::Existing { face, .. } => face,
            Self::Generated(slot) => return FaceKey::Slot(slot),
        };
        let text = face.as_str();
        // A canonical generated identity has a literal prefix and at most 20
        // decimal bytes. These fixed bounds admit no input-sized text scan.
        if text.len() > FACE_SLOT_PREFIX.len() + MAX_FREE_I64_TEXT_BYTES {
            return FaceKey::Text(face);
        }
        let Some(tail) = text.strip_prefix(FACE_SLOT_PREFIX) else {
            return FaceKey::Text(face);
        };
        let minimal =
            tail == "0" || matches!(tail.as_bytes(), [b'1'..=b'9', ..] | [b'-', b'1'..=b'9', ..]);
        if minimal {
            if let Ok(slot) = tail.parse::<i64>() {
                return FaceKey::Slot(slot);
            }
        }
        FaceKey::Text(face)
    }

    fn materialize(self, ctx: &DecodeContext<'_>) -> Result<cadmpeg_ir::ids::FaceId, CodecError> {
        match self {
            Self::Existing { face, operation } => face.try_clone_for_decode(ctx, operation),
            Self::Generated(slot) => face_slot_id(ctx, slot),
        }
    }
}

fn resolved_face_operand<'source>(
    ctx: &DecodeContext<'_>,
    operand: &'source DesignFaceOperand,
) -> Result<Option<Vec<FaceCandidate<'source>>>, CodecError> {
    if let Some(face) = &operand.resolved_active_face {
        let mut faces = Vec::new();
        let face = FaceCandidate::Existing {
            face,
            operation: "f3d resolved active face id",
        };
        ctx.push_vec(&mut faces, face, "f3d resolved active face")?;
        return Ok(Some(faces));
    }
    if !operand.resolved_face_slots.is_empty() {
        if operand.candidate_faces.is_empty()
            && operand.unreferenced_candidate_faces.is_empty()
            && operand.alternate_selector_candidate_faces.is_empty()
        {
            let mut faces = Vec::new();
            for slot in ctx.admit_iter(
                &operand.resolved_face_slots,
                "f3d generated face slot source",
            )? {
                let face = FaceCandidate::Generated(*slot);
                ctx.push_vec(&mut faces, face, "f3d resolved face slot")?;
            }
            return Ok(Some(faces));
        }
        let mut storage = ctx.reserve_scoped(0, "f3d resolved slot proof storage")?;
        let mut by_slot = HashMap::new();
        let mut selected = Vec::new();
        let mut candidates = operand
            .candidate_faces
            .iter()
            .chain(&operand.unreferenced_candidate_faces)
            .chain(&operand.alternate_selector_candidate_faces);
        let mut slots = operand.resolved_face_slots.iter();
        while let Some(slot) = ctx.next_charged(&mut slots, "f3d resolved face slot source")? {
            let cached = ctx
                .get_hash_map(&by_slot, slot, "f3d resolved face slot lookup")?
                .copied();
            let face = if let Some(face) = cached {
                Some(face)
            } else {
                ctx.find_map(
                    &mut candidates,
                    |face| {
                        let Some(candidate_slot) = stable_face_slot(ctx, face)? else {
                            return Ok(None);
                        };
                        let first = storage.with_storage(|| -> Result<_, CodecError> {
                            Ok(*ctx
                                .entry_hash_map(
                                    &mut by_slot,
                                    candidate_slot,
                                    "f3d resolved face slot index",
                                )?
                                .or_insert(face))
                        })?;
                        Ok((candidate_slot == *slot).then_some(first))
                    },
                    "f3d resolved slot candidate source",
                )?
            };
            let Some(face) = face else { return Ok(None) };
            ctx.push_vec(
                &mut selected,
                FaceCandidate::Existing {
                    face,
                    operation: "f3d resolved face slot candidate id",
                },
                "f3d resolved face slot selection",
            )?;
        }
        return Ok(Some(selected));
    }
    let candidates = face_operand_candidates(operand);
    if !operand.alternate_selector_candidate_faces.is_empty() {
        let mut faces = Vec::new();
        for face in ctx.admit_iter(candidates, "f3d resolved candidate source")? {
            let face = FaceCandidate::Existing {
                face,
                operation: "f3d alternate face candidate id",
            };
            ctx.push_vec(&mut faces, face, "f3d alternate face candidate")?;
        }
        return Ok(Some(faces));
    }
    if operand.recipe_kind == crate::records::recipes::ConstructionRecipeKind::Face {
        let mut storage = ctx.reserve_scoped(0, "f3d referenced face proof storage")?;
        let mut seen = HashSet::new();
        let mut selected = Vec::new();
        for reference in ctx.admit_iter(
            &operand.recipe_references,
            "f3d referenced face reference source",
        )? {
            for face in ctx.admit_iter(
                &reference.candidate_faces,
                "f3d referenced face candidate source",
            )? {
                if storage.with_storage(|| {
                    ctx.insert_hash_set(&mut seen, face, "f3d referenced face membership")
                })? {
                    ctx.push_vec(
                        &mut selected,
                        FaceCandidate::Existing {
                            face,
                            operation: "f3d referenced face id",
                        },
                        "f3d referenced face selection",
                    )?;
                }
            }
        }
        if !selected.is_empty() {
            return Ok(Some(selected));
        }
    }
    if !operand.unreferenced_candidate_faces.is_empty()
        && complete_counted_face_recipe(ctx, operand)?.is_some()
    {
        let mut faces = Vec::new();
        for face in ctx.admit_iter(candidates, "f3d resolved candidate source")? {
            let face = FaceCandidate::Existing {
                face,
                operation: "f3d unreferenced face candidate id",
            };
            ctx.push_vec(&mut faces, face, "f3d unreferenced face candidate")?;
        }
        return Ok(Some(faces));
    }
    let [face] = candidates else { return Ok(None) };
    let mut faces = Vec::new();
    let face = FaceCandidate::Existing {
        face,
        operation: "f3d unique face candidate id",
    };
    ctx.push_vec(&mut faces, face, "f3d unique face candidate")?;
    Ok(Some(faces))
}

fn explicit_bounded_face_candidates<'source>(
    ctx: &DecodeContext<'_>,
    operand: &'source DesignFaceOperand,
) -> Result<Option<Vec<FaceCandidate<'source>>>, CodecError> {
    if operand.recipe_kind != crate::records::recipes::ConstructionRecipeKind::BoundedFace
        || !operand.resolved_face_slots.is_empty()
        || !operand.alternate_selector_candidate_faces.is_empty()
        || !operand.historical_support_contexts.is_empty()
        || complete_counted_face_recipe(ctx, operand)?.is_none()
    {
        return Ok(None);
    }
    if !operand.candidate_faces.is_empty() && operand.unreferenced_candidate_faces.is_empty() {
        let mut candidates = Vec::new();
        for face in ctx.admit_iter(
            &operand.candidate_faces,
            "f3d explicit bounded owned candidate source",
        )? {
            let face = FaceCandidate::Existing {
                face,
                operation: "f3d explicit bounded face candidate id",
            };
            ctx.push_vec(&mut candidates, face, "f3d explicit bounded face candidate")?;
        }
        return Ok(Some(candidates));
    }

    let mut storage = ctx.reserve_scoped(0, "f3d explicit bounded lane storage")?;
    let lanes = storage.with_storage(|| -> Result<_, CodecError> {
        let mut lookup_storage = ctx.reserve_scoped(0, "f3d explicit bounded lookup storage")?;
        let (alternate_references, candidates) =
            lookup_storage.with_storage(|| -> Result<_, CodecError> {
                let mut candidates = HashSet::new();
                for face in ctx.admit_iter(
                    &operand.candidate_faces,
                    "f3d explicit bounded candidate index scan",
                )? {
                    ctx.insert_hash_set(
                        &mut candidates,
                        face,
                        "f3d explicit bounded candidate index",
                    )?;
                }
                let mut alternate_references = HashSet::new();
                for reference in ctx.admit_iter(
                    &operand.recipe_references,
                    "f3d explicit bounded alternate scan",
                )? {
                    if !reference.alternate_selector_faces.is_empty() {
                        ctx.insert_hash_set(
                            &mut alternate_references,
                            reference.design_reference,
                            "f3d explicit bounded alternate reference",
                        )?;
                    }
                }
                Ok((alternate_references, candidates))
            })?;
        let mut lanes = BTreeMap::<i64, BTreeSet<&cadmpeg_ir::ids::FaceId>>::new();
        for reference in ctx.admit_iter(
            &operand.recipe_references,
            "f3d explicit bounded reference scan",
        )? {
            if ctx.contains_hash_set(
                &alternate_references,
                &reference.design_reference,
                "f3d explicit bounded alternate lookup",
            )? {
                continue;
            }
            for face in ctx.admit_iter(
                &reference.candidate_faces,
                "f3d explicit bounded candidate scan",
            )? {
                if operand.candidate_faces.is_empty()
                    || ctx.contains_hash_set(
                        &candidates,
                        face,
                        "f3d explicit bounded candidate membership",
                    )?
                {
                    ctx.insert_btree_group_set(
                        &mut lanes,
                        reference.design_reference,
                        face,
                        "f3d explicit bounded face lane",
                        "f3d explicit bounded face lane member",
                    )?;
                }
            }
        }
        Ok(lanes)
    })?;
    let mut largest = None::<&BTreeSet<&cadmpeg_ir::ids::FaceId>>;
    let mut tied = false;
    for (_, lane) in ctx.admit_iter(&lanes, "f3d explicit bounded lane selection")? {
        match largest {
            Some(previous) if BTreeSet::len(previous) > lane.len() => {}
            Some(previous) if previous.len() == lane.len() => tied = true,
            _ => {
                largest = Some(lane);
                tied = false;
            }
        }
    }
    let Some(lane) = largest.filter(|_| !tied) else {
        return Ok(None);
    };
    let mut selected = Vec::new();
    for face in ctx.admit_iter(lane, "f3d explicit bounded selected scan")? {
        let face = FaceCandidate::Existing {
            face,
            operation: "f3d explicit bounded selected face id",
        };
        ctx.push_vec(&mut selected, face, "f3d explicit bounded selected face")?;
    }
    Ok(Some(selected))
}

fn complete_counted_face_recipe(
    ctx: &DecodeContext<'_>,
    operand: &DesignFaceOperand,
) -> Result<Option<usize>, CodecError> {
    if operand.recipe_kind != crate::records::recipes::ConstructionRecipeKind::BoundedFace {
        return Ok(None);
    }
    let Some(crate::design::decode::operands::FaceRecipeProgramKind::Counted { header_value }) =
        crate::design::decode::operands::face_recipe_program_kind(&operand.recipe_program)
    else {
        return Ok(None);
    };
    if operand.recipe_nodes.is_empty()
        || !ctx.all_by(
            &operand.recipe_nodes,
            |node| Ok(node.recipe_structure.is_some()),
            "f3d counted face structure source",
        )?
    {
        return Ok(None);
    }
    if operand.recipe_nodes.len() == header_value {
        return Ok(Some(header_value));
    }
    let Some(boundaries) =
        unique_preceding_face_boundaries(ctx, &operand.historical_support_contexts)?
    else {
        return Ok(None);
    };
    Ok(
        (boundary_edge_count(ctx, boundaries.boundaries.iter().copied())? == Some(header_value))
            .then_some(header_value),
    )
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
    ctx: &DecodeContext<'_>,
    operand: &DesignFaceOperand,
    recipe_record_index: i32,
) -> Result<Option<Vec<cadmpeg_ir::ids::FaceId>>, CodecError> {
    if operand.recipe_kind != crate::records::recipes::ConstructionRecipeKind::BoundedFace
        || !matches!(
            crate::design::decode::operands::face_recipe_program_kind(&operand.recipe_program),
            Some(crate::design::decode::operands::FaceRecipeProgramKind::Counted { .. })
        )
        || operand.recipe_nodes.is_empty()
        || !operand.unreferenced_candidate_faces.is_empty()
        || !operand.alternate_selector_candidate_faces.is_empty()
    {
        return Ok(None);
    }
    let mut storage = ctx.reserve_scoped(0, "f3d legacy face proof storage")?;
    let mut selected = BTreeSet::new();
    for reference in ctx.admit_iter(
        &operand.recipe_references,
        "f3d legacy face reference source",
    )? {
        if reference.design_reference != i64::from(recipe_record_index) {
            continue;
        }
        let candidates = if reference.candidate_faces.is_empty() {
            &reference.alternate_selector_faces
        } else {
            &reference.candidate_faces
        };
        for face in ctx.admit_iter(candidates, "f3d legacy selected candidate source")? {
            storage.with_storage(|| {
                ctx.insert_btree_set(&mut selected, face, "f3d legacy selected face index")
            })?;
        }
    }
    if selected.is_empty() {
        return Ok(None);
    }
    if !operand.candidate_faces.is_empty() {
        let mut active_storage = ctx.reserve_scoped(0, "f3d legacy active face storage")?;
        let mut active = BTreeSet::new();
        for face in ctx.admit_iter(
            &operand.candidate_faces,
            "f3d legacy active candidate source",
        )? {
            active_storage.with_storage(|| {
                ctx.insert_btree_set(&mut active, face, "f3d legacy active face index")
            })?;
        }
        if active.len() != selected.len()
            || !ctx.all_by(
                active.iter().zip(&selected),
                |(first, second)| ctx.equal(*first, *second, "f3d legacy face id comparison"),
                "f3d legacy face set comparison",
            )?
        {
            return Ok(None);
        }
    }
    let mut candidates = Vec::new();
    for face in ctx.admit_iter(&selected, "f3d legacy accepted face source")? {
        let face = (*face).try_clone_for_decode(ctx, "f3d legacy face candidate id")?;
        ctx.push_vec(&mut candidates, face, "f3d legacy face candidate")?;
    }
    Ok(Some(candidates))
}

pub(crate) fn resolve_face_operand_history_candidates(
    ctx: &DecodeContext<'_>,
    operand: &DesignFaceOperand,
) -> Result<Option<i64>, CodecError> {
    let Some(direct) = unique_face_operand_history_candidate(operand) else {
        return resolve_face_operand_support_candidate(ctx, operand);
    };
    let mut referenced_lane = false;
    let mut matched = false;
    if operand.recipe_kind == crate::records::recipes::ConstructionRecipeKind::Face {
        matched = ctx.any_by(
            &operand.recipe_references,
            |reference| {
                referenced_lane |= !reference.candidate_faces.is_empty()
                    || !reference.alternate_selector_faces.is_empty();
                ctx.any_by(
                    reference
                        .candidate_faces
                        .iter()
                        .chain(&reference.alternate_selector_faces),
                    |candidate| {
                        ctx.equal(candidate, direct, "f3d direct referenced face membership")
                    },
                    "f3d direct referenced candidate source",
                )
            },
            "f3d direct face reference source",
        )?;
    }
    if !matched && !referenced_lane {
        matched = ctx.contains(
            face_operand_candidates(operand),
            direct,
            "f3d direct face candidate membership",
        )?;
    }
    if !matched
        && complete_counted_face_recipe(ctx, operand)?.is_some()
        && operand.candidate_faces.is_empty()
        && operand.unreferenced_candidate_faces.is_empty()
        && operand.alternate_selector_candidate_faces.is_empty()
    {
        matched = ctx.any_by(
            &operand.recipe_references,
            |reference| {
                ctx.any_by(
                    reference
                        .candidate_faces
                        .iter()
                        .chain(&reference.alternate_selector_faces),
                    |candidate| ctx.equal(candidate, direct, "f3d direct nested face membership"),
                    "f3d direct nested candidate source",
                )
            },
            "f3d direct nested reference source",
        )?;
    }
    if !matched {
        return Ok(None);
    }
    stable_face_slot(ctx, direct)
}

pub(crate) fn resolve_face_operand_history_candidate_from(
    ctx: &DecodeContext<'_>,
    operand: &DesignFaceOperand,
    candidates: &[cadmpeg_ir::ids::FaceId],
) -> Result<Option<i64>, CodecError> {
    let Some(direct) = unique_face_operand_history_candidate(operand) else {
        return resolve_face_operand_support_candidate(ctx, operand);
    };
    if !ctx.contains(candidates, direct, "f3d direct supplied face membership")? {
        return Ok(None);
    }
    stable_face_slot(ctx, direct)
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
    ctx: &DecodeContext<'_>,
    operand: &DesignFaceOperand,
) -> Result<Option<Vec<i64>>, CodecError> {
    if operand.recipe_kind != crate::records::recipes::ConstructionRecipeKind::BoundedFace {
        return Ok(None);
    }
    if let Some(candidate) = convergent_effective_face_support(ctx, operand)? {
        return Ok(Some(candidate));
    }
    let Some(header_value) = complete_counted_face_recipe(ctx, operand)? else {
        return Ok(None);
    };
    bounded_face_candidate_by_boundary_cardinality(
        ctx,
        header_value,
        &operand.historical_support_contexts,
    )
}

pub(crate) fn resolve_stable_bounded_face_history_set(
    ctx: &DecodeContext<'_>,
    operand: &DesignFaceOperand,
) -> Result<Option<Vec<i64>>, CodecError> {
    if complete_counted_face_recipe(ctx, operand)?.is_none() {
        return Ok(None);
    }
    let mut active_storage = ctx.reserve_scoped(0, "f3d stable bounded candidate storage")?;
    let mut active_faces = Vec::new();
    let mut faces = operand.preceding_candidate_faces.iter();
    while let Some(face) = ctx.next_charged(&mut faces, "f3d stable bounded candidate source")? {
        let Some(slot) = stable_face_slot(ctx, face)? else {
            return Ok(None);
        };
        if ctx.contains(
            &active_faces,
            &slot,
            "f3d stable bounded candidate membership",
        )? {
            return Ok(None);
        }
        active_storage.with_storage(|| {
            ctx.push_vec(&mut active_faces, slot, "f3d stable bounded active face")
        })?;
    }
    stable_face_support_set(ctx, &active_faces, &operand.historical_support_contexts)
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
    ctx: &DecodeContext<'_>,
    operand: &DesignFaceOperand,
) -> Result<Option<Vec<i64>>, CodecError> {
    if counted_face_recipe_frame(ctx, operand)?.is_none() {
        return Ok(None);
    }
    if operand.preceding_candidate_faces.is_empty() || operand.changed_candidate_faces.is_empty() {
        return Ok(None);
    }
    let Some(active) = unique_stable_face_slots(ctx, &operand.preceding_candidate_faces)? else {
        return Ok(None);
    };
    let Some(changed) = unique_stable_face_slots(ctx, &operand.changed_candidate_faces)? else {
        return Ok(None);
    };
    let active_faces = &active.0;
    if active_faces.is_empty()
        || changed.0.len() != active_faces.len()
        || !ctx.equal(
            &changed.0,
            active_faces,
            "f3d SurfaceDeleteFace changed equality",
        )?
        || operand.historical_support_contexts.len() != active_faces.len()
    {
        return Ok(None);
    }
    let mut storage = ctx.reserve_scoped(0, "f3d SurfaceDeleteFace coverage storage")?;
    let mut covered = HashSet::new();
    let mut source = operand.historical_support_contexts.iter();
    while let Some(context) =
        ctx.next_charged(&mut source, "f3d SurfaceDeleteFace context source")?
    {
        if ctx
            .binary_search(
                active_faces,
                &context.active_face_slot,
                "f3d SurfaceDeleteFace active membership",
            )?
            .is_err()
            || !matches!(context.preceding_face_slots.as_slice(), [face] if *face == context.active_face_slot)
            || !matches!(context.changed_preceding_face_slots.as_slice(), [face] if *face == context.active_face_slot)
        {
            return Ok(None);
        }
        if !storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut covered,
                context.active_face_slot,
                "f3d SurfaceDeleteFace covered face index",
            )
        })? {
            return Ok(None);
        }
        let Some(boundaries) = valid_preceding_face_boundaries(ctx, context)? else {
            return Ok(None);
        };
        let [boundary] = boundaries else {
            return Ok(None);
        };
        if boundary.face_slot != context.active_face_slot {
            return Ok(None);
        }
    }
    if covered.len() != active_faces.len() {
        return Ok(None);
    }
    Ok(Some(ctx.collect_vec(
        active_faces.iter().copied(),
        "f3d SurfaceDeleteFace accepted slots",
    )?))
}

fn unique_stable_face_slots<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    faces: &[cadmpeg_ir::ids::FaceId],
) -> Result<Option<(Vec<i64>, cadmpeg_core::decode::ScopedReservation<'ctx>)>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "f3d stable face slot storage")?;
    let mut slots = Vec::new();
    let mut source = faces.iter();
    while let Some(face) = ctx.next_charged(&mut source, "f3d stable face slot source")? {
        let Some(slot) = stable_face_slot(ctx, face)? else {
            return Ok(None);
        };
        if slot < 0 {
            return Ok(None);
        }
        storage.with_storage(|| ctx.push_vec(&mut slots, slot, "f3d stable face slot"))?;
    }
    ctx.sort_unstable_by(
        &mut slots,
        |value| value,
        Ord::cmp,
        "f3d stable face slot sort",
    )?;
    let unique = ctx.all_by(
        slots.windows(2),
        |pair| Ok(pair[0] != pair[1]),
        "f3d stable face slot uniqueness",
    )?;
    Ok(unique.then_some((slots, storage)))
}

fn counted_face_recipe_frame(
    ctx: &DecodeContext<'_>,
    operand: &DesignFaceOperand,
) -> Result<Option<usize>, CodecError> {
    if operand.recipe_kind != crate::records::recipes::ConstructionRecipeKind::BoundedFace {
        return Ok(None);
    }
    let Some(crate::design::decode::operands::FaceRecipeProgramKind::Counted { header_value }) =
        crate::design::decode::operands::face_recipe_program_kind(&operand.recipe_program)
    else {
        return Ok(None);
    };
    if operand.recipe_nodes.len() != header_value
        || operand.recipe_nodes.is_empty()
        || ctx.any_by(
            &operand.recipe_nodes,
            |node| Ok(!matches!(node.program.as_slice(), [-1, -1, 2, ..])),
            "f3d counted face frame source",
        )?
    {
        return Ok(None);
    }
    Ok(Some(header_value))
}

fn stable_face_support_set(
    ctx: &DecodeContext<'_>,
    active_faces: &[i64],
    contexts: &[crate::records::topology::historical_context::DesignHistoricalFaceSupportContext],
) -> Result<Option<Vec<i64>>, CodecError> {
    if active_faces.is_empty() || contexts.len() != active_faces.len() {
        return Ok(None);
    }
    let mut active_storage = ctx.reserve_scoped(0, "f3d stable bounded active storage")?;
    let mut unique_active = HashSet::new();
    let mut faces = active_faces.iter();
    while let Some(face) = ctx.next_charged(&mut faces, "f3d stable bounded active source")? {
        if !active_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut unique_active,
                *face,
                "f3d stable bounded active face index",
            )
        })? {
            return Ok(None);
        }
    }
    let mut covered_storage = ctx.reserve_scoped(0, "f3d stable bounded covered storage")?;
    let mut covered = HashSet::new();
    let mut contexts = contexts.iter();
    while let Some(context) =
        ctx.next_charged(&mut contexts, "f3d stable bounded context source")?
    {
        if !ctx.contains_hash_set(
            &unique_active,
            &context.active_face_slot,
            "f3d stable bounded active membership",
        )? || !matches!(context.preceding_face_slots.as_slice(), [slot] if *slot == context.active_face_slot)
            || !context.changed_preceding_face_slots.is_empty()
        {
            return Ok(None);
        }
        if !covered_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut covered,
                context.active_face_slot,
                "f3d stable bounded covered face index",
            )
        })? {
            return Ok(None);
        }
    }
    Ok(Some(ctx.collect_vec(
        active_faces.iter().copied(),
        "f3d stable bounded support face",
    )?))
}

fn convergent_effective_face_support(
    ctx: &DecodeContext<'_>,
    operand: &DesignFaceOperand,
) -> Result<Option<Vec<i64>>, CodecError> {
    let Some(active_faces) = effective_historical_face_slots(
        ctx,
        face_operand_candidates(operand),
        &operand.historical_support_contexts,
    )?
    else {
        return Ok(None);
    };
    convergent_face_support(ctx, &active_faces.0, &operand.historical_support_contexts)
}

/// Return candidate slots that have a complete historical support context.
///
/// A persistent-reference lane can contain revisions that are absent from the
/// retained history topology. Those revisions are not effective candidates for
/// the common-support rule. The history binder omits them when it builds the
/// support contexts, so the context-active set is the effective subset. Keep
/// the subset admission explicit and reject a context that is not in the
/// operand's candidate lane.
fn effective_historical_face_slots<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    candidates: &[cadmpeg_ir::ids::FaceId],
    contexts: &[crate::records::topology::historical_context::DesignHistoricalFaceSupportContext],
) -> Result<Option<(Vec<i64>, cadmpeg_core::decode::ScopedReservation<'ctx>)>, CodecError> {
    let mut candidate_storage = ctx.reserve_scoped(0, "f3d effective candidate storage")?;
    let mut candidate_slots = Vec::new();
    let mut candidates = candidates.iter();
    while let Some(face) = ctx.next_charged(&mut candidates, "f3d effective candidate source")? {
        let Some(slot) = stable_face_slot(ctx, face)? else {
            return Ok(None);
        };
        candidate_storage.with_storage(|| {
            ctx.push_vec(
                &mut candidate_slots,
                slot,
                "f3d effective candidate face slot",
            )
        })?;
    }
    ctx.sort_unstable_by(
        &mut candidate_slots,
        |value| value,
        Ord::cmp,
        "f3d effective candidate face sort",
    )?;
    ctx.dedup_vec(
        &mut candidate_slots,
        "f3d face resolve candidate slots dedup",
    )?;
    let mut active_storage = ctx.reserve_scoped(0, "f3d effective active storage")?;
    let mut active_faces = Vec::new();
    active_storage.with_storage(|| -> Result<(), CodecError> {
        for context in ctx.admit_iter(contexts, "f3d effective active source")? {
            ctx.push_vec(
                &mut active_faces,
                context.active_face_slot,
                "f3d effective active face slot",
            )?;
        }
        Ok(())
    })?;
    ctx.sort_unstable_by(
        &mut active_faces,
        |value| value,
        Ord::cmp,
        "f3d effective active face sort",
    )?;
    ctx.dedup_vec(&mut active_faces, "f3d face resolve active faces dedup")?;
    if active_faces.is_empty()
        || !ctx.all_by(
            &active_faces,
            |slot| {
                Ok(ctx
                    .binary_search(&candidate_slots, slot, "f3d effective candidate membership")?
                    .is_ok())
            },
            "f3d effective active containment",
        )?
    {
        return Ok(None);
    }
    Ok(Some((active_faces, active_storage)))
}

fn convergent_face_support(
    ctx: &DecodeContext<'_>,
    active_faces: &[i64],
    support_contexts: &[crate::records::topology::historical_context::DesignHistoricalFaceSupportContext],
) -> Result<Option<Vec<i64>>, CodecError> {
    if active_faces.is_empty() {
        return Ok(None);
    }
    let mut contexts = support_contexts.iter();
    let Some(first) = ctx.next_charged(&mut contexts, "f3d convergent context source")? else {
        return Ok(None);
    };
    let mut support_storage = ctx.reserve_scoped(0, "f3d convergent support storage")?;
    let mut support = Vec::new();
    support_storage.with_storage(|| -> Result<(), CodecError> {
        for slot in ctx.admit_iter(&first.preceding_face_slots, "f3d convergent support source")? {
            ctx.push_vec(&mut support, *slot, "f3d convergent support face")?;
        }
        Ok(())
    })?;
    ctx.sort_unstable_by(
        &mut support,
        |value| value,
        Ord::cmp,
        "f3d convergent support face sort",
    )?;
    ctx.dedup_vec(&mut support, "f3d face resolve support dedup")?;
    if support.is_empty() {
        return Ok(None);
    }
    let mut covered_storage = ctx.reserve_scoped(0, "f3d convergent covered storage")?;
    let mut covered = Vec::new();
    covered_storage.with_storage(|| {
        ctx.push_vec(
            &mut covered,
            first.active_face_slot,
            "f3d convergent covered face",
        )
    })?;
    while let Some(context) = ctx.next_charged(&mut contexts, "f3d convergent context source")? {
        let mut candidate_storage = ctx.reserve_scoped(0, "f3d convergent candidate storage")?;
        let mut candidate = Vec::new();
        candidate_storage.with_storage(|| -> Result<(), CodecError> {
            for slot in ctx.admit_iter(
                &context.preceding_face_slots,
                "f3d convergent candidate source",
            )? {
                ctx.push_vec(
                    &mut candidate,
                    *slot,
                    "f3d convergent candidate support face",
                )?;
            }
            Ok(())
        })?;
        ctx.sort_unstable_by(
            &mut candidate,
            |value| value,
            Ord::cmp,
            "f3d convergent candidate face sort",
        )?;
        ctx.dedup_vec(&mut candidate, "f3d face resolve candidate dedup")?;
        if candidate.len() != support.len()
            || !ctx.equal(&candidate, &support, "f3d convergent support equality")?
        {
            return Ok(None);
        }
        covered_storage.with_storage(|| {
            ctx.push_vec(
                &mut covered,
                context.active_face_slot,
                "f3d convergent covered face",
            )
        })?;
    }
    ctx.sort_unstable_by(
        &mut covered,
        |value| value,
        Ord::cmp,
        "f3d convergent covered face sort",
    )?;
    ctx.dedup_vec(&mut covered, "f3d face resolve covered dedup")?;
    if covered.len() != active_faces.len()
        || !ctx.equal(
            covered.as_slice(),
            active_faces,
            "f3d convergent active coverage",
        )?
    {
        return Ok(None);
    }
    Ok(Some(ctx.collect_vec(
        support.iter().copied(),
        "f3d convergent accepted support faces",
    )?))
}

fn bounded_face_candidate_by_boundary_cardinality(
    ctx: &DecodeContext<'_>,
    header_value: usize,
    contexts: &[crate::records::topology::historical_context::DesignHistoricalFaceSupportContext],
) -> Result<Option<Vec<i64>>, CodecError> {
    if contexts.is_empty() {
        return Ok(None);
    }
    let mut selected_storage = ctx.reserve_scoped(0, "f3d bounded selected face storage")?;
    let mut selected: Option<Vec<i64>> = None;
    let mut source = contexts.iter();
    while let Some(context) =
        ctx.next_charged(&mut source, "f3d bounded cardinality context source")?
    {
        let Some(boundaries) = valid_preceding_face_boundaries(ctx, context)? else {
            continue;
        };
        if boundary_edge_count(ctx, boundaries.iter())? != Some(header_value) {
            continue;
        }
        let mut storage = ctx.reserve_scoped(0, "f3d bounded candidate face storage")?;
        let mut slots = storage.with_storage(|| {
            ctx.collect_vec(
                boundaries.iter().map(|boundary| boundary.face_slot),
                "f3d bounded face cardinality candidate",
            )
        })?;
        ctx.sort_unstable_by(
            &mut slots,
            |value| value,
            Ord::cmp,
            "f3d bounded face candidate sort",
        )?;
        if let Some(first) = &selected {
            if first.len() != slots.len()
                || !ctx.equal(first, &slots, "f3d bounded face cardinality equality")?
            {
                return Ok(None);
            }
        } else {
            // The candidate allocation remains scratch until every competing set agrees.
            selected_storage = storage;
            selected = Some(slots);
        }
    }
    if let Some(boundaries) = unique_preceding_face_boundaries(ctx, contexts)? {
        if boundary_edge_count(ctx, boundaries.boundaries.iter().copied())? == Some(header_value) {
            let mut storage = ctx.reserve_scoped(0, "f3d bounded union face storage")?;
            let mut slots = storage.with_storage(|| {
                ctx.collect_vec(
                    boundaries
                        .boundaries
                        .iter()
                        .map(|boundary| boundary.face_slot),
                    "f3d bounded face cardinality union",
                )
            })?;
            ctx.sort_unstable_by(
                &mut slots,
                |value| value,
                Ord::cmp,
                "f3d bounded face union sort",
            )?;
            if let Some(first) = &selected {
                if first.len() != slots.len()
                    || !ctx.equal(first, &slots, "f3d bounded face cardinality equality")?
                {
                    return Ok(None);
                }
            } else {
                selected_storage = storage;
                selected = Some(slots);
            }
        }
    }
    let Some(selected) = selected else {
        return Ok(None);
    };
    let result = ctx.collect_vec(selected.iter().copied(), "f3d bounded accepted face slots")?;
    drop(selected);
    drop(selected_storage);
    Ok(Some(result))
}

fn valid_preceding_face_boundaries<'a>(
    ctx: &DecodeContext<'_>,
    context: &'a crate::records::topology::historical_context::DesignHistoricalFaceSupportContext,
) -> Result<
    Option<&'a [crate::records::topology::historical_context::DesignHistoricalFaceBoundaryContext]>,
    CodecError,
> {
    let expected_faces = &context.preceding_face_slots;
    let boundaries = &context.preceding_face_boundaries;
    if expected_faces.is_empty() || boundaries.is_empty() {
        return Ok(None);
    }
    let mut storage = ctx.reserve_scoped(0, "f3d preceding boundary membership storage")?;
    let mut expected = HashSet::new();
    let mut seen = HashSet::new();
    let mut source = boundaries.iter();
    while let Some(boundary) = ctx.next_charged(&mut source, "f3d preceding boundary source")? {
        if boundary.loops.is_empty() {
            return Ok(None);
        }
        if expected.is_empty() {
            storage.with_storage(|| -> Result<(), CodecError> {
                for face in ctx.admit_iter(expected_faces, "f3d preceding expected face source")? {
                    ctx.insert_hash_set(&mut expected, *face, "f3d preceding expected face index")?;
                }
                Ok(())
            })?;
        }
        if !ctx.contains_hash_set(
            &expected,
            &boundary.face_slot,
            "f3d preceding expected face lookup",
        )? || !storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut seen,
                boundary.face_slot,
                "f3d preceding boundary face index",
            )
        })? {
            return Ok(None);
        }
    }
    Ok((expected.len() == seen.len()).then_some(boundaries.as_slice()))
}

struct FaceBoundarySet<'a, 'ctx> {
    boundaries:
        Vec<&'a crate::records::topology::historical_context::DesignHistoricalFaceBoundaryContext>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

fn unique_preceding_face_boundaries<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    contexts: &'a [crate::records::topology::historical_context::DesignHistoricalFaceSupportContext],
) -> Result<Option<FaceBoundarySet<'a, 'ctx>>, CodecError> {
    if contexts.is_empty() {
        return Ok(None);
    }
    let mut index_storage = ctx.reserve_scoped(0, "f3d unique preceding boundary indexes")?;
    let mut active = HashSet::new();
    let mut by_face = BTreeMap::new();
    let mut output_storage = ctx.reserve_scoped(0, "f3d unique preceding boundary storage")?;
    let mut boundaries = Vec::new();
    let mut source = contexts.iter();
    while let Some(context) =
        ctx.next_charged(&mut source, "f3d unique preceding context source")?
    {
        if !index_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut active,
                context.active_face_slot,
                "f3d unique preceding active index",
            )
        })? {
            return Ok(None);
        }
        let Some(context_boundaries) = valid_preceding_face_boundaries(ctx, context)? else {
            return Ok(None);
        };
        let mut source = context_boundaries.iter();
        while let Some(boundary) =
            ctx.next_charged(&mut source, "f3d unique preceding boundary source")?
        {
            let first = index_storage.with_storage(|| -> Result<Option<bool>, CodecError> {
                match ctx.entry_btree_map(
                    &mut by_face,
                    boundary.face_slot,
                    "f3d unique preceding boundary index",
                )? {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(boundary);
                        Ok(Some(true))
                    }
                    std::collections::btree_map::Entry::Occupied(entry) => Ok(ctx
                        .equal(
                            *entry.get(),
                            boundary,
                            "f3d unique preceding boundary equality",
                        )?
                        .then_some(false)),
                }
            })?;
            let Some(first) = first else { return Ok(None) };
            if !first {
                continue;
            }
            output_storage.with_storage(|| {
                ctx.push_vec(
                    &mut boundaries,
                    boundary,
                    "f3d unique preceding boundary output",
                )
            })?;
        }
    }
    Ok(Some(FaceBoundarySet {
        boundaries,
        _storage: output_storage,
    }))
}

fn boundary_edge_count<'a>(
    ctx: &DecodeContext<'_>,
    boundaries: impl IntoIterator<Item = &'a crate::records::topology::historical_context::DesignHistoricalFaceBoundaryContext>,
) -> Result<Option<usize>, CodecError> {
    let mut total = 0usize;
    let mut source = boundaries.into_iter();
    while let Some(boundary) = ctx.next_charged(&mut source, "f3d boundary count face source")? {
        let mut loops = boundary.loops.iter();
        while let Some(loop_) = ctx.next_charged(&mut loops, "f3d boundary count loop source")? {
            // One boundary variant supplies one exact-length slice; the other iterator lanes are empty.
            let count = loop_.boundary.coedges().size_hint().0;
            if count == 0 {
                return Ok(None);
            }
            let Some(sum) = total.checked_add(count) else {
                return Ok(None);
            };
            total = sum;
        }
    }
    Ok(Some(total))
}

fn resolve_face_operand_support_candidate(
    ctx: &DecodeContext<'_>,
    operand: &DesignFaceOperand,
) -> Result<Option<i64>, CodecError> {
    let Some(reference) = operand.recipe_references.first() else {
        return Ok(None);
    };
    let active_faces = if reference.candidate_faces.is_empty() {
        &reference.alternate_selector_faces
    } else {
        &reference.candidate_faces
    };
    if active_faces.is_empty() || operand.historical_support_contexts.is_empty() {
        return Ok(None);
    }
    let mut faces = active_faces.iter();
    let Some(first) = ctx.find_map(
        &mut faces,
        |face| stable_face_slot(ctx, face),
        "f3d support initial face source",
    )?
    else {
        return Ok(None);
    };
    let mut storage = ctx.reserve_scoped(0, "f3d support face index storage")?;
    let mut active = HashSet::new();
    storage.with_storage(|| {
        ctx.insert_hash_set(&mut active, first, "f3d support active face index")
    })?;
    let mut candidate = None;
    let mut contexts = operand.historical_support_contexts.iter();
    while let Some(context) = ctx.next_charged(&mut contexts, "f3d support context source")? {
        let mut matched = ctx.contains_hash_set(
            &active,
            &context.active_face_slot,
            "f3d support active face lookup",
        )?;
        if !matched && faces.len() != 0 {
            matched = ctx
                .find_map(
                    &mut faces,
                    |face| {
                        let Some(slot) = stable_face_slot(ctx, face)? else {
                            return Ok(None);
                        };
                        storage.with_storage(|| {
                            ctx.insert_hash_set(&mut active, slot, "f3d support active face index")
                        })?;
                        Ok((slot == context.active_face_slot).then_some(()))
                    },
                    "f3d support active face source",
                )?
                .is_some();
        }
        if !matched {
            continue;
        }
        let slots = if context.changed_preceding_face_slots.is_empty() {
            &context.preceding_face_slots
        } else {
            &context.changed_preceding_face_slots
        };
        let mut slots = slots.iter();
        while let Some(slot) = ctx.next_charged(&mut slots, "f3d support predecessor source")? {
            if candidate.is_some_and(|first| first != *slot) {
                return Ok(None);
            }
            candidate = Some(*slot);
        }
    }
    Ok(candidate)
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
    ctx: &DecodeContext<'_>,
    operand: &DesignFaceOperand,
) -> Result<Vec<cadmpeg_ir::ids::FaceId>, CodecError> {
    if operand.recipe_kind == crate::records::recipes::ConstructionRecipeKind::Face {
        let referenced = historical_reference_face_candidates(
            ctx,
            operand,
            "f3d historical face candidate id",
            "f3d historical face candidate",
        )?;
        if !referenced.is_empty() {
            return Ok(referenced);
        }
    }
    let mut candidates = Vec::new();
    for candidate in ctx.admit_iter(
        face_operand_candidates(operand),
        "f3d historical fallback candidate source",
    )? {
        let candidate = candidate.try_clone_for_decode(ctx, "f3d historical fallback face id")?;
        ctx.push_vec(&mut candidates, candidate, "f3d historical fallback face")?;
    }
    Ok(candidates)
}

/// Return nested persistent-reference faces for a complete bounded-face
/// recipe whose own active candidate lanes are empty. The nested faces are
/// topology supports, not selected faces; callers must map them through the
/// historical support graph and prove a unique preceding target.
pub(crate) fn nested_bounded_face_history_candidates(
    ctx: &DecodeContext<'_>,
    operand: &DesignFaceOperand,
) -> Result<Option<Vec<cadmpeg_ir::ids::FaceId>>, CodecError> {
    if !operand.candidate_faces.is_empty()
        || !operand.unreferenced_candidate_faces.is_empty()
        || !operand.alternate_selector_candidate_faces.is_empty()
        || complete_counted_face_recipe(ctx, operand)?.is_none()
    {
        return Ok(None);
    }
    let candidates = historical_reference_face_candidates(
        ctx,
        operand,
        "f3d nested bounded face candidate id",
        "f3d nested bounded face candidate",
    )?;
    Ok((!candidates.is_empty()).then_some(candidates))
}

fn historical_reference_face_candidates(
    ctx: &DecodeContext<'_>,
    operand: &DesignFaceOperand,
    id_operation: &'static str,
    output_operation: &'static str,
) -> Result<Vec<cadmpeg_ir::ids::FaceId>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "f3d historical referenced candidate storage")?;
    let mut selected = BTreeSet::new();
    for reference in ctx.admit_iter(
        &operand.recipe_references,
        "f3d historical reference source",
    )? {
        for candidate in ctx
            .admit_iter(
                &reference.candidate_faces,
                "f3d historical referenced candidate source",
            )?
            .chain(ctx.admit_iter(
                &reference.alternate_selector_faces,
                "f3d historical referenced candidate source",
            )?)
        {
            storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut selected,
                    candidate,
                    "f3d historical referenced candidate index",
                )
            })?;
        }
    }
    let mut candidates = Vec::new();
    for candidate in ctx.admit_iter(&selected, "f3d historical accepted candidate source")? {
        let candidate = (*candidate).try_clone_for_decode(ctx, id_operation)?;
        ctx.push_vec(&mut candidates, candidate, output_operation)?;
    }
    Ok(candidates)
}

fn has_nested_bounded_face_history_candidates(
    ctx: &DecodeContext<'_>,
    operand: &DesignFaceOperand,
) -> Result<bool, CodecError> {
    Ok(complete_counted_face_recipe(ctx, operand)?.is_some()
        && operand.candidate_faces.is_empty()
        && operand.unreferenced_candidate_faces.is_empty()
        && operand.alternate_selector_candidate_faces.is_empty()
        && ctx.any_by(
            &operand.recipe_references,
            |reference| {
                Ok(!reference.candidate_faces.is_empty()
                    || !reference.alternate_selector_faces.is_empty())
            },
            "f3d nested bounded reference source",
        )?)
}

/// Return active B-rep faces for the legacy `FromFace` envelope whose counted
/// bounded recipe contains support references but no active face lane.
///
/// This is a candidate-generation proof, not a selection proof. The caller
/// must reduce the returned faces to one plane coincident with the profile
/// sketch before binding it to the operand.
fn extrude_start_plane_geometry_candidates<'faces>(
    ctx: &DecodeContext<'_>,
    group: &DesignConstructionOperandGroup,
    operands: &[DesignFaceOperand],
    faces: &'faces [cadmpeg_ir::topology::Face],
) -> Result<Option<&'faces [cadmpeg_ir::topology::Face]>, CodecError> {
    let [crate::records::identity::Located {
        value: record_index,
        ..
    }] = group.members()
    else {
        return Ok(None);
    };
    let stream = native_stream_charged(ctx, &group.id, "f3d start geometry group stream")?;
    let Some(operand) = unique_face_operand(
        ctx,
        operands,
        stream,
        group.scope_record_index,
        *record_index,
    )?
    else {
        return Ok(None);
    };
    if !face_operand_candidates(operand).is_empty()
        || !operand.resolved_face_slots.is_empty()
        || operand.resolved_active_face.is_some()
        || !has_nested_bounded_face_history_candidates(ctx, operand)?
    {
        return Ok(None);
    }
    Ok(Some(faces))
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
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    resolution: &mut ExtrudeFaceResolution<'_>,
) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{ExtrudeStart, FaceSelection, FeatureDefinition, FeatureOperation};

    for feature in ctx.admit_iter(features, "f3d Extrude start feature source")? {
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| {
            edit_result = (|| -> Result<(), CodecError> {
                'feature_edit: {
                    let FeatureDefinition::Operation(FeatureOperation::Extrude {
                        profile,
                        start,
                        ..
                    }) = definition
                    else {
                        break 'feature_edit;
                    };
                    let Some(sketch_id) = extrude_profile_sketch_id(profile) else {
                        break 'feature_edit;
                    };
                    let Some(sketch) = ctx.find_by(
                        sketches,
                        |sketch| {
                            ctx.equal(&sketch.id, sketch_id, "f3d Extrude start sketch comparison")
                        },
                        "f3d Extrude start sketch source",
                    )?
                    else {
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
                    let Some(group) =
                        crate::design::feature_project::unique_feature_match_for_decode(
                            ctx,
                            resolution.groups.iter(),
                            |group| {
                                ctx.equal(&group.id, native, "f3d Extrude start group comparison")
                            },
                            "f3d Extrude start group source",
                        )?
                    else {
                        break 'feature_edit;
                    };
                    if group.extrude_face_role() != Some(DesignExtrudeFaceRole::Start) {
                        break 'feature_edit;
                    }
                    let Some(stream) = ids::native_stream_charged(
                        ctx,
                        &group.id,
                        "f3d Extrude start group stream",
                    )?
                    else {
                        break 'feature_edit;
                    };
                    let mut candidate_storage =
                        ctx.reserve_scoped(0, "f3d Extrude start candidate storage")?;
                    let mut candidates = Vec::new();
                    let mut members = group.members().iter();
                    while let Some(member) =
                        ctx.next_charged(&mut members, "f3d Extrude start member source")?
                    {
                        let Some(operand) = unique_face_operand(
                            ctx,
                            resolution.operands,
                            Some(stream),
                            group.scope_record_index,
                            member.value,
                        )?
                        else {
                            candidates.clear();
                            break;
                        };
                        for face in ctx.admit_iter(
                            face_operand_candidates(operand),
                            "f3d Extrude start candidate source",
                        )? {
                            ctx.push_scoped_vec(
                                &mut candidate_storage,
                                &mut candidates,
                                face,
                                "f3d start plane operand candidate",
                            )?;
                        }
                    }
                    ctx.stable_sort_by(
                        &mut candidates[..],
                        |value| value.as_str(),
                        Ord::cmp,
                        "sort f3d design face_resolve 10",
                    )?;
                    ctx.dedup_vec(&mut candidates, "f3d face resolve candidates dedup")?;
                    if candidates.is_empty() {
                        if let Some(found) = extrude_start_plane_geometry_candidates(
                            ctx,
                            group,
                            resolution.operands,
                            resolution.faces,
                        )? {
                            for face in ctx
                                .admit_iter(found, "f3d Extrude start geometry candidate source")?
                            {
                                ctx.push_scoped_vec(
                                    &mut candidate_storage,
                                    &mut candidates,
                                    &face.id,
                                    "f3d start plane operand candidate",
                                )?;
                            }
                        }
                    }
                    let coincident =
                        crate::design::feature_project::unique_feature_match_for_decode(
                            ctx,
                            candidates.iter().copied(),
                            |candidate| {
                                face_coincident_with_sketch(
                                    ctx,
                                    candidate,
                                    sketch,
                                    resolution.faces,
                                    resolution.surfaces,
                                    resolution.linear_tolerance,
                                    resolution.angular_tolerance,
                                )
                            },
                            "f3d Extrude start coincidence source",
                        )?;
                    if let Some(face) = coincident {
                        let (selected, selected_storage) = ctx
                            .with_scoped_storage("f3d selected start plane storage", || {
                                face.try_clone_for_decode(ctx, "f3d selected start plane face ID")
                            })?;
                        drop(candidates);
                        drop(candidate_storage);
                        if retain_face_operand_resolution(
                            ctx,
                            group,
                            resolution.operands,
                            &selected,
                        )? {
                            let (face, output_storage) = ctx.with_scoped_storage(
                                "f3d selected start plane output storage",
                                || {
                                    let faces = ctx
                                        .collect_vec([selected], "f3d selected start plane face")?;
                                    let native = ctx.copy_retained_text(
                                        native,
                                        "f3d selected start plane native ID",
                                    )?;
                                    Ok::<_, CodecError>(FaceSelection::Resolved { faces, native })
                                },
                            )?;
                            selected_storage.commit()?;
                            let face = output_storage.commit_value(face)?;
                            *start = ExtrudeStart::FromFace {
                                face,
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
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    resolution: &mut ExtrudeFaceResolution<'_>,
) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{
        ExtrudeDirection, ExtrudeExtent, FeatureDefinition, FeatureOperation,
    };

    for feature in ctx.admit_iter(features, "f3d Extrude target feature source")? {
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
                    let Some(sketch) = ctx.find_by(
                        sketches,
                        |sketch| {
                            ctx.equal(
                                &sketch.id,
                                sketch_id,
                                "f3d Extrude target sketch comparison",
                            )
                        },
                        "f3d Extrude target sketch source",
                    )?
                    else {
                        break 'feature_edit;
                    };
                    let Some((sketch_origin, profile_normal, _)) = sketch.resolved_placement()
                    else {
                        break 'feature_edit;
                    };
                    let sweep_direction = match direction {
                        ExtrudeDirection::ProfileNormal {} => profile_normal,
                        ExtrudeDirection::ReversedProfileNormal {} => profile_normal.negated(),
                        ExtrudeDirection::Explicit { vector, .. } => (*vector).into(),
                        ExtrudeDirection::Unresolved {} => break 'feature_edit,
                    };
                    let (sketch_origin, sweep_direction) =
                        (sketch_origin.get(), sweep_direction.get());
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
    ctx: &DecodeContext<'_>,
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
    let Some(group) = unique_feature_match_for_decode(
        ctx,
        resolution.groups.iter(),
        |group| {
            Ok(
                ctx.equal(&group.id, native, "f3d Extrude target group comparison")?
                    && group.extrude_face_role() == Some(DesignExtrudeFaceRole::Termination),
            )
        },
        "f3d Extrude target group source",
    )?
    else {
        return Ok(());
    };
    let Some(face) =
        extrude_target_plane_candidate(ctx, group, resolution, sketch_origin, sweep_direction)?
    else {
        return Ok(());
    };
    let face = &resolution.faces[face].id;
    if retain_face_operand_resolution(ctx, group, resolution.operands, face)? {
        let face = face.try_clone_for_decode(ctx, "f3d target plane face ID")?;
        let faces = ctx.collect_vec([face], "f3d target face output")?;
        let native = ctx.copy_retained_text(native, "f3d target face native ID")?;
        *termination = LinearTermination::ToFace {
            face: FaceSelection::Resolved { faces, native },
            offset,
        };
    }
    Ok(())
}

fn extrude_target_plane_candidate(
    ctx: &DecodeContext<'_>,
    group: &DesignConstructionOperandGroup,
    resolution: &ExtrudeFaceResolution<'_>,
    sketch_origin: Point3,
    sweep_direction: Vector3,
) -> Result<Option<usize>, CodecError> {
    let [crate::records::identity::Located {
        value: record_index,
        ..
    }] = group.members()
    else {
        return Ok(None);
    };
    let Some(stream) = native_stream_charged(ctx, &group.id, "f3d target plane group stream")?
    else {
        return Ok(None);
    };
    let Some(operand) = unique_face_operand(
        ctx,
        resolution.operands,
        Some(stream),
        group.scope_record_index,
        *record_index,
    )?
    else {
        return Ok(None);
    };
    let direction_length = sweep_direction.norm();
    let mut found = None;
    let mut ambiguous = false;
    let mut consider = |candidate: &cadmpeg_ir::ids::FaceId| -> Result<(), CodecError> {
        let Some((index, plane_surface)) =
            plane_for_face(ctx, candidate, resolution.faces, resolution.surfaces)?
        else {
            return Ok(());
        };
        let origin = plane_surface.origin().get();
        let normal = plane_surface.frame().axis().as_raw();
        if !parallel_vectors(*normal, sweep_direction, resolution.angular_tolerance) {
            return Ok(());
        }
        let distance = origin.vector_from(sketch_origin).dot(sweep_direction) / direction_length;
        if distance > resolution.linear_tolerance {
            if found.is_some_and(|previous| previous != index) {
                ambiguous = true;
            } else {
                found = Some(index);
            }
        }
        Ok(())
    };
    let direct = face_operand_candidates(operand);
    if direct.is_empty() {
        if !has_nested_bounded_face_history_candidates(ctx, operand)? {
            return Ok(None);
        }
        let mut references = operand.recipe_references.iter();
        while let Some(reference) =
            ctx.next_charged(&mut references, "visit F3D target plane reference")?
        {
            if reference.candidate_faces.is_empty() && reference.alternate_selector_faces.is_empty()
            {
                continue;
            }
            let mut candidates = reference
                .candidate_faces
                .iter()
                .chain(&reference.alternate_selector_faces);
            while let Some(candidate) =
                ctx.next_charged(&mut candidates, "visit F3D target plane nested candidate")?
            {
                consider(candidate)?;
            }
        }
    } else {
        let mut candidates = direct.iter();
        while let Some(candidate) =
            ctx.next_charged(&mut candidates, "visit F3D target plane candidate")?
        {
            consider(candidate)?;
        }
    }
    if ambiguous {
        return Ok(None);
    }
    Ok(found)
}

pub(super) fn retain_face_operand_resolution(
    ctx: &DecodeContext<'_>,
    group: &DesignConstructionOperandGroup,
    operands: &mut [DesignFaceOperand],
    face: &cadmpeg_ir::ids::FaceId,
) -> Result<bool, CodecError> {
    let Some(stream) = crate::ids::native_stream_charged(
        ctx,
        &group.id,
        "resolve F3D retained face group stream",
    )?
    else {
        return Ok(false);
    };
    let mut selected = None;
    let mut input = operands.iter_mut();
    while let Some(operand) = ctx.next_charged(&mut input, "visit F3D retained face operand")? {
        let same_stream = match crate::ids::native_stream_charged(
            ctx,
            &operand.id,
            "resolve F3D retained face operand stream",
        )? {
            Some(candidate) => ctx.equal_bytes(
                candidate.as_bytes(),
                stream.as_bytes(),
                "compare F3D retained face operand stream",
            )?,
            None => false,
        };
        if !same_stream || operand.scope_record_index != group.scope_record_index {
            continue;
        }
        if group.members().is_empty()
            || !ctx.any_by(
                group.members(),
                |member| Ok(member.value == operand.record_index()),
                "find F3D retained face group member",
            )?
        {
            continue;
        }
        if !(contains_face(
            ctx,
            face_operand_candidates(operand),
            face,
            "find F3D retained face operand candidate",
        )? || (face_operand_candidates(operand).is_empty()
            && operand.resolved_face_slots.is_empty()
            && operand.resolved_active_face.is_none()
            && has_nested_bounded_face_history_candidates(ctx, operand)?))
        {
            continue;
        }
        if selected.is_some() {
            return Ok(false);
        }
        selected = Some(operand);
    }
    let Some(operand) = selected else {
        return Ok(false);
    };
    let geometry_bound = face_operand_candidates(operand).is_empty()
        && operand.resolved_face_slots.is_empty()
        && operand.resolved_active_face.is_none()
        && has_nested_bounded_face_history_candidates(ctx, operand)?;
    if geometry_bound {
        operand.resolved_active_face =
            Some((face).try_clone_for_decode(ctx, "f3d retained operand active face")?);
        return Ok(true);
    }
    let Some(slot) = stable_face_slot(ctx, face)? else {
        return Ok(false);
    };
    if !operand.resolved_face_slots.is_empty() && operand.resolved_face_slots != [slot] {
        return Ok(false);
    }
    operand.resolved_face_slots = ctx.collect_vec([slot], "f3d retained operand face slot")?;
    Ok(true)
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

#[cfg(test)]
mod tests {
    mod borrowed_candidates;
    mod selection_proofs;

    mod boundary_admission;
    mod candidate_admission;
    mod candidate_limits;
    mod explicit_lanes;
    mod extrude;
    mod membership_queries;
    mod profile_native_transfer;
    mod support_admission;

    use super::{
        bounded_face_candidate_by_boundary_cardinality, convergent_face_support,
        effective_historical_face_slots, legacy_face_recipe_reference_candidates,
        loft_edge_profile_face_slot, resolve_stable_bounded_face_history_set,
        resolve_surface_delete_face_history_set, resolved_explicit_bounded_face_group,
        resolved_face_group, resolved_historical_face_group, resolved_historical_face_operand,
        resolved_historical_split_face_target_group_with_updated_faces,
        resolved_profile_face_group, stable_face_support_set,
    };
    use crate::ids::feature_input_topology_id;
    use crate::ids::neutral_feature_id;
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

    use cadmpeg_ir::ids::FaceId;
    use cadmpeg_ir::ids::{ShellId, SurfaceId};
    use cadmpeg_ir::topology::{Face, Sense};

    fn face(slot: i64) -> FaceId {
        FaceId::mint(format!("f3d:brep:entity#{slot}")).expect("identity grammar")
    }

    fn legacy_face_candidate_limit_fixture() -> DesignFaceOperand {
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
            "candidate_faces": ["f3d:brep:entity#10"],
            "next_record_index": 202,
            "next_byte_offset": 469
        })).unwrap();
        operand.recipe_references = vec![reference(10, "selected", 201)];
        operand
    }

    fn assert_legacy_face_candidate_collection_refusal(operation: &'static str) {
        use cadmpeg_core::decode::ResourceDimension;
        let operand = legacy_face_candidate_limit_fixture();
        crate::test_support::with_decode_context(|ctx| {
            assert_eq!(
                legacy_face_recipe_reference_candidates(ctx, &operand, 201).unwrap(),
                Some(vec![face(10)])
            );
        });
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::CollectionItems,
            operation,
            0,
            |ctx| legacy_face_recipe_reference_candidates(ctx, &operand, 201),
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(ref failure)
            if failure.operation == operation
                && failure.dimension == ResourceDimension::CollectionItems),
            "expected {operation} refusal, got {error:?}"
        );
    }

    #[test]
    fn legacy_face_candidate_id_refuses_retained_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;
        let operand = legacy_face_candidate_limit_fixture();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = u64::try_from(face(10).as_str().len() - 1).unwrap();

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = legacy_face_recipe_reference_candidates(&ctx, &operand, 201);
        assert!(
            matches!(result, Err(CodecError::ResourceLimit(ref failure))
            if failure.operation == "f3d legacy face candidate id"
                && failure.dimension == ResourceDimension::RetainedBytes),
            "expected legacy face ID refusal, got {result:?}"
        );
    }

    #[test]
    fn legacy_face_candidate_refuses_collection_limit() {
        assert_legacy_face_candidate_collection_refusal("f3d legacy face candidate");
    }

    #[test]
    fn legacy_active_face_index_refuses_collection_limit() {
        assert_legacy_face_candidate_collection_refusal("f3d legacy active face index");
    }

    #[test]
    fn legacy_selected_face_index_refuses_collection_limit() {
        assert_legacy_face_candidate_collection_refusal("f3d legacy selected face index");
    }

    #[test]
    fn split_face_updated_transition_resolves_legacy_target_group() {
        crate::test_support::with_decode_context(|decode_ctx| {
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
                decode_ctx,
                &scope,
                scope.previous_history_state_id(),
                &group,
                &operands,
                &[10, 20, 30],
            )
            .expect("projection resource budget")
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
                    decode_ctx,
                    &scope,
                    scope.previous_history_state_id(),
                    &group,
                    &operands,
                    &[10, 20]
                )
                .expect("projection resource budget")
                .is_none()
            );
            assert!(
                resolved_historical_split_face_target_group_with_updated_faces(
                    decode_ctx,
                    &scope,
                    scope.previous_history_state_id(),
                    &group,
                    &operands,
                    &[10, 20, 40]
                )
                .expect("projection resource budget")
                .is_none()
            );
        });
    }

    fn assert_split_face_updated_slot_refusal(operation: &'static str, limit: u64) {
        crate::test_support::with_decode_context(|decode_ctx| {
            use cadmpeg_core::decode::{
                DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
            };
            use cadmpeg_core::CodecError;

            let scope = DesignParameterScope::empty(
                "f3d:test:scope#100",
                crate::records::feature::scope::DesignFeatureKind::SplitFace,
                100,
            );
            let group: DesignConstructionOperandGroup = serde_json::from_value(serde_json::json!({
                "id": "f3d:test:construction-group#150",
                "scope_record_index": 100,
                "scope_reference_ordinal": 1,
                "record_index": 150,
                "byte_offset": 0,
                "class_tag": "262",
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
                "role": 0x0000_0010_0000_0000_u64,
                "role_offset": 0,
                "paired_class_tag": "258",
                "paired_byte_offset": 0
            }))
            .unwrap();
            let operand: DesignFaceOperand = serde_json::from_value(serde_json::json!({
                "id": "f3d:test:face-operand#200",
                "scope_record_index": 100,
                "scope_reference_ordinal": 1,
                "group_record_index": 150,
                "group_member_ordinal": 0,
                "record_index": 200,
                "byte_offset": 0,
                "class_tag": "277",
                "paired_byte_offset": 407,
                "paired_class_tag": "258",
                "recipe_record_index": 203,
                "recipe_record_byte_offset": 423,
                "recipe_id": "f3d:test:recipe#200",
                "recipe_prefix_offset": 434,
                "recipe_prefix_bytes": "",
                "recipe_references": [],
                "recipe_kind": "bounded_face",
                "recipe_program_offset": 0,
                "recipe_program": [0, -1, 2],
                "recipe_node_offsets": [],
                "recipe_nodes": [],
                "candidate_faces": ["f3d:brep:entity#10"],
                "preceding_candidate_faces": ["f3d:brep:entity#10"],
                "next_record_index": 204,
                "next_byte_offset": 551
            }))
            .unwrap();
            assert_eq!(
                super::split_face_updated_target_slots(
                    decode_ctx,
                    &scope,
                    &group,
                    std::slice::from_ref(&operand),
                    &[10]
                )
                .unwrap(),
                Some(vec![10])
            );
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_collection_items = limit;

            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = super::split_face_updated_target_slots(
                &ctx,
                &scope,
                &group,
                std::slice::from_ref(&operand),
                &[10],
            );
            assert!(
                matches!(result, Err(CodecError::ResourceLimit(ref failure))
            if failure.operation == operation
                && failure.dimension == ResourceDimension::CollectionItems),
                "expected {operation} refusal, got {result:?}"
            );
        });
    }

    #[test]
    fn split_face_updated_index_refuses_collection_limit() {
        assert_split_face_updated_slot_refusal("f3d SplitFace updated face index", 0);
    }

    #[test]
    fn split_face_represented_index_refuses_collection_limit() {
        assert_split_face_updated_slot_refusal("f3d SplitFace represented face index", 1);
    }

    #[test]
    fn split_face_updated_face_refuses_collection_limit() {
        assert_split_face_updated_slot_refusal("f3d SplitFace updated face", 2);
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
        crate::test_support::with_decode_context(|decode_ctx| {
            let contexts = [
                support(10, &[(100, 12)]),
                support(11, &[(101, 4), (102, 4)]),
                support(12, &[(101, 4), (102, 4)]),
            ];
            assert_eq!(
                bounded_face_candidate_by_boundary_cardinality(decode_ctx, 8, &contexts).unwrap(),
                Some(vec![101, 102])
            );
            assert_eq!(
                bounded_face_candidate_by_boundary_cardinality(decode_ctx, 12, &contexts).unwrap(),
                Some(vec![100])
            );
            assert_eq!(
                bounded_face_candidate_by_boundary_cardinality(decode_ctx, 20, &contexts).unwrap(),
                Some(vec![100, 101, 102])
            );
        });
    }

    #[test]
    fn bounded_face_cardinality_rejects_conflicting_complete_sets() {
        crate::test_support::with_decode_context(|decode_ctx| {
            let contexts = [support(10, &[(100, 4)]), support(11, &[(101, 4)])];
            assert_eq!(
                bounded_face_candidate_by_boundary_cardinality(decode_ctx, 4, &contexts).unwrap(),
                None
            );
        });
    }

    #[test]
    fn bounded_face_cardinality_candidate_refuses_collection_limit() {
        use cadmpeg_core::decode::ResourceDimension;
        use cadmpeg_core::CodecError;
        let contexts = [support(10, &[(100, 4)])];
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::CollectionItems,
            "f3d bounded face cardinality candidate",
            0,
            |ctx| bounded_face_candidate_by_boundary_cardinality(ctx, 4, &contexts),
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d bounded face cardinality candidate"));
    }

    #[test]
    fn bounded_face_cardinality_union_refuses_collection_limit() {
        use cadmpeg_core::decode::ResourceDimension;
        use cadmpeg_core::CodecError;
        let contexts = [support(10, &[(100, 4)]), support(11, &[(101, 4)])];
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::CollectionItems,
            "f3d bounded face cardinality union",
            0,
            |ctx| bounded_face_candidate_by_boundary_cardinality(ctx, 8, &contexts),
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d bounded face cardinality union"));
    }

    #[test]
    fn effective_faces_resolve_only_with_complete_convergent_support() {
        crate::test_support::with_decode_context(|decode_ctx| {
            let contexts = [
                support(10, &[(100, 4)]),
                support(11, &[(100, 4)]),
                support(12, &[(100, 4)]),
            ];
            assert_eq!(
                convergent_face_support(decode_ctx, &[10, 11, 12], &contexts).unwrap(),
                Some(vec![100])
            );
            assert_eq!(
                convergent_face_support(decode_ctx, &[10, 11, 12, 13], &contexts).unwrap(),
                None
            );

            let conflicting = [support(10, &[(100, 4)]), support(11, &[(101, 4)])];
            assert_eq!(
                convergent_face_support(decode_ctx, &[10, 11], &conflicting).unwrap(),
                None
            );
        });
    }

    #[test]
    fn effective_historical_faces_exclude_unmapped_revisions() {
        crate::test_support::with_decode_context(|decode_ctx| {
            let contexts = [
                support(10, &[(100, 4)]),
                support(11, &[(100, 4)]),
                support(12, &[(100, 4)]),
            ];
            let candidates = [face(10), face(11), face(12), face(13)];
            assert_eq!(
                effective_historical_face_slots(decode_ctx, &candidates, &contexts)
                    .unwrap()
                    .map(|value| value.0),
                Some(vec![10, 11, 12])
            );
            assert_eq!(
                effective_historical_face_slots(decode_ctx, &[face(10), face(11)], &contexts)
                    .unwrap()
                    .map(|value| value.0),
                None
            );
        });
    }

    fn assert_effective_face_slot_refusal(limit: u64, operation: &'static str) {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let contexts = [
            support(10, &[(100, 4)]),
            support(11, &[(100, 4)]),
            support(12, &[(100, 4)]),
        ];
        let candidates = [face(10), face(11), face(12), face(13)];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = effective_historical_face_slots(&ctx, &candidates, &contexts);
        assert!(
            matches!(result, Err(CodecError::ResourceLimit(ref failure))
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == operation),
            "expected {operation} refusal, got {result:?}"
        );
    }

    #[test]
    fn effective_candidate_face_slot_refuses_collection_limit() {
        assert_effective_face_slot_refusal(0, "f3d effective candidate face slot");
    }

    #[test]
    fn effective_active_face_slot_refuses_collection_limit() {
        assert_effective_face_slot_refusal(4, "f3d effective active face slot");
    }

    fn assert_convergent_face_refusal(limit: u64, operation: &'static str) {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let contexts = [
            support(10, &[(100, 4)]),
            support(11, &[(100, 4)]),
            support(12, &[(100, 4)]),
        ];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = convergent_face_support(&ctx, &[10, 11, 12], &contexts);
        assert!(
            matches!(result, Err(CodecError::ResourceLimit(ref failure))
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == operation),
            "expected {operation} refusal, got {result:?}"
        );
    }

    #[test]
    fn convergent_support_face_refuses_collection_limit() {
        assert_convergent_face_refusal(0, "f3d convergent support face");
    }

    #[test]
    fn convergent_covered_face_refuses_collection_limit() {
        assert_convergent_face_refusal(1, "f3d convergent covered face");
    }

    #[test]
    fn convergent_candidate_support_refuses_collection_limit() {
        assert_convergent_face_refusal(2, "f3d convergent candidate support face");
    }

    #[test]
    fn stable_bounded_face_set_preserves_each_proven_predecessor() {
        crate::test_support::with_decode_context(|decode_ctx| {
            let active_faces = [10, 11];
            let mut contexts = vec![support(10, &[(10, 4)]), support(11, &[(11, 4)])];

            assert_eq!(
                stable_face_support_set(decode_ctx, &active_faces, &contexts).unwrap(),
                Some(vec![10, 11])
            );
            contexts[1].preceding_face_slots = vec![12];
            assert_eq!(
                stable_face_support_set(decode_ctx, &active_faces, &contexts).unwrap(),
                None
            );
        });
    }

    fn surface_delete_face_operand() -> DesignFaceOperand {
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
        operand
    }

    fn stable_bounded_face_operand() -> DesignFaceOperand {
        let mut operand = surface_delete_face_operand();
        for context in &mut operand.historical_support_contexts {
            context.changed_preceding_face_slots.clear();
        }
        let side = crate::records::topology::edge_recipe::DesignTopologyRecipeSide {
            header_value: 0,
            scalars: Vec::new(),
            payload_prefix: Vec::new(),
            entries: Vec::new(),
        };
        operand.recipe_nodes[0].recipe_structure =
            Some(crate::records::topology::face::DesignFaceRecipeStructure {
                root: 0,
                prelude: [0, 0],
                sides: [side.clone(), side],
                postlude_value: None,
            });
        operand
    }

    #[test]
    fn complete_counted_face_node_run_does_not_require_support_boundary_proof() {
        let mut operand = stable_bounded_face_operand();
        let mut contexts = [support(10, &[(100, 4_096)]), support(11, &[(100, 4_096)])];
        let crate::records::topology::historical_context::DesignHistoricalLoopBoundary::Coedges(
            rows,
        ) = &mut contexts[1].preceding_face_boundaries[0].loops[0].boundary
        else {
            panic!("coedge fixture");
        };
        rows.last_mut().expect("last coedge").edge_slot += 1;
        assert!(crate::test_support::with_decode_context(|ctx| {
            super::unique_preceding_face_boundaries(ctx, &contexts)
                .unwrap()
                .is_none()
        }));
        operand.historical_support_contexts = Vec::from(contexts);
        let native_before = serde_json::to_value(&operand).unwrap();

        assert_eq!(
            crate::test_support::with_decode_context(|ctx| super::complete_counted_face_recipe(
                ctx, &operand
            ))
            .unwrap(),
            Some(1)
        );
        assert_eq!(serde_json::to_value(&operand).unwrap(), native_before);
    }

    #[test]
    fn face_support_candidate_requires_one_matching_predecessor() {
        let mut operand = stable_bounded_face_operand();
        operand.recipe_references = vec![reference(10, "selected", 1)];
        assert_eq!(
            crate::test_support::with_decode_context(|ctx| {
                super::resolve_face_operand_support_candidate(ctx, &operand)
            })
            .unwrap(),
            Some(10)
        );
        operand.recipe_references[0].candidate_faces.push(face(11));
        assert_eq!(
            crate::test_support::with_decode_context(|ctx| {
                super::resolve_face_operand_support_candidate(ctx, &operand)
            })
            .unwrap(),
            None
        );
        operand.recipe_references[0].candidate_faces.truncate(1);
        operand.historical_support_contexts[0].changed_preceding_face_slots = vec![12];
        assert_eq!(
            crate::test_support::with_decode_context(|ctx| {
                super::resolve_face_operand_support_candidate(ctx, &operand)
            })
            .unwrap(),
            Some(12)
        );
    }

    fn assert_historical_face_group_collection_refusal(limit: u64, operation: &'static str) {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let (mut operand, group, _) = start_geometry_fixture();
        operand.resolved_face_slots = vec![10];
        let scope = loft_scope();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = resolved_historical_face_group(
            &ctx,
            &scope,
            scope.previous_history_state_id(),
            &group,
            std::slice::from_ref(&operand),
        );
        assert!(
            matches!(result, Err(CodecError::ResourceLimit(ref failure))
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == operation),
            "expected {operation} refusal, got {result:?}"
        );
    }

    fn assert_historical_face_group_materialized_refusal(operation: &'static str) {
        use cadmpeg_core::decode::ResourceDimension;
        use cadmpeg_core::CodecError;
        let (mut operand, group, _) = start_geometry_fixture();
        operand.resolved_face_slots = vec![10];
        let scope = loft_scope();
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::MaterializedBytes,
            operation,
            0,
            |ctx| {
                resolved_historical_face_group(
                    ctx,
                    &scope,
                    scope.previous_history_state_id(),
                    &group,
                    std::slice::from_ref(&operand),
                )
            },
        );
        assert!(
            matches!(error, CodecError::ResourceLimit(failure) if failure.dimension == ResourceDimension::MaterializedBytes && failure.operation == operation)
        );
    }

    #[test]
    fn historical_face_group_slot_refuses_collection_limit() {
        assert_historical_face_group_collection_refusal(0, "f3d historical face group slot");
    }

    #[test]
    fn historical_face_member_refuses_collection_limit() {
        assert_historical_face_group_collection_refusal(1, "f3d historical face member");
    }

    #[test]
    fn historical_face_group_id_refuses_materialized_limit() {
        assert_historical_face_group_materialized_refusal("f3d historical face group id");
    }

    #[test]
    fn historical_face_id_refuses_materialized_limit() {
        assert_historical_face_group_materialized_refusal("f3d historical face id");
    }

    #[test]
    fn historical_face_operand_id_refuses_retained_limit() {
        crate::test_support::with_decode_context(|decode_ctx| {
            use cadmpeg_core::decode::{
                DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
            };
            use cadmpeg_core::CodecError;

            let (mut operand, _, _) = start_geometry_fixture();
            operand.candidate_faces = vec![face(10)];
            operand.preceding_candidate_faces = vec![face(10)];
            let scope = loft_scope();
            assert!(
                resolved_historical_face_operand(decode_ctx, &scope, &operand)
                    .unwrap()
                    .is_some()
            );
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_retained_bytes = 0;

            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = resolved_historical_face_operand(&ctx, &scope, &operand);
            assert!(
                matches!(result, Err(CodecError::ResourceLimit(ref failure))
            if failure.dimension == ResourceDimension::RetainedBytes
                && failure.operation == "f3d historical face selection storage"),
                "expected historical operand ID refusal, got {result:?}"
            );
        });
    }

    #[test]
    fn split_face_complete_candidate_slot_refuses_collection_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let mut operand = stable_bounded_face_operand();
        operand.resolved_active_face = Some(face(10));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = super::split_face_complete_candidate_slots(&ctx, &operand);
        assert!(
            matches!(result, Err(CodecError::ResourceLimit(ref failure))
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d SplitFace complete candidate slot"),
            "expected SplitFace slot refusal, got {result:?}"
        );
    }

    #[test]
    fn split_face_preceding_candidate_index_refuses_collection_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let mut operand = stable_bounded_face_operand();
        operand.resolved_active_face = Some(face(10));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 1;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = super::split_face_complete_candidate_slots(&ctx, &operand);
        assert!(
            matches!(result, Err(CodecError::ResourceLimit(ref failure))
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d SplitFace preceding candidate index"),
            "expected SplitFace preceding index refusal, got {result:?}"
        );
    }

    #[test]
    fn stable_bounded_active_face_refuses_collection_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let operand = stable_bounded_face_operand();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = resolve_stable_bounded_face_history_set(&ctx, &operand).unwrap_err();
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d stable bounded active face"));
    }

    #[test]
    fn stable_bounded_support_face_refuses_collection_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let operand = stable_bounded_face_operand();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 6;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = resolve_stable_bounded_face_history_set(&ctx, &operand).unwrap_err();
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d stable bounded support face"));
    }

    #[test]
    fn stable_bounded_active_index_refuses_collection_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let operand = stable_bounded_face_operand();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = resolve_stable_bounded_face_history_set(&ctx, &operand).unwrap_err();
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d stable bounded active face index"));
    }

    #[test]
    fn stable_bounded_covered_index_refuses_collection_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let operand = stable_bounded_face_operand();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 4;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = resolve_stable_bounded_face_history_set(&ctx, &operand).unwrap_err();
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d stable bounded covered face index"));
    }

    #[test]
    fn surface_delete_face_history_set_requires_complete_changed_one_to_one_support() {
        crate::test_support::with_decode_context(|decode_ctx| {
            let mut operand = surface_delete_face_operand();
            assert_eq!(
                resolve_surface_delete_face_history_set(decode_ctx, &operand).unwrap(),
                Some(vec![10, 11])
            );

            operand.historical_support_contexts[1]
                .changed_preceding_face_slots
                .clear();
            assert_eq!(
                resolve_surface_delete_face_history_set(decode_ctx, &operand).unwrap(),
                None
            );

            operand.historical_support_contexts[1].changed_preceding_face_slots = vec![11];
            operand.historical_support_contexts[1].preceding_face_slots = vec![10, 11];
            assert_eq!(
                resolve_surface_delete_face_history_set(decode_ctx, &operand).unwrap(),
                None
            );
        });
    }

    #[test]
    fn surface_delete_face_slots_refuse_collection_limit() {
        use cadmpeg_core::decode::ResourceDimension;
        use cadmpeg_core::CodecError;
        let operand = surface_delete_face_operand();
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::CollectionItems,
            "f3d stable face slot",
            0,
            |ctx| resolve_surface_delete_face_history_set(ctx, &operand),
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d stable face slot"));
    }

    #[test]
    fn surface_delete_face_covered_index_refuses_collection_limit() {
        use cadmpeg_core::decode::ResourceDimension;
        use cadmpeg_core::CodecError;
        let operand = surface_delete_face_operand();
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::CollectionItems,
            "f3d SurfaceDeleteFace covered face index",
            0,
            |ctx| resolve_surface_delete_face_history_set(ctx, &operand),
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d SurfaceDeleteFace covered face index"));
    }

    #[test]
    fn loft_edge_profile_requires_one_common_face_clause() {
        let first = edge_operand(1, 100, 0, 3, 200);
        let second = edge_operand(2, 100, 1, 3, 201);
        let third = edge_operand(3, 100, 0, 3, 202);
        let members = [&first, &second, &third];
        crate::test_support::with_decode_context(|ctx| {
            assert_eq!(
                loft_edge_profile_face_slot(3, &members, ctx).unwrap(),
                Some(100)
            );
        });

        let different_target = edge_operand(4, 101, 1, 3, 203);
        let mismatched = [&first, &different_target, &third];
        crate::test_support::with_decode_context(|ctx| {
            assert_eq!(
                loft_edge_profile_face_slot(3, &mismatched, ctx).unwrap(),
                None
            );
        });

        let mut ambiguous = first.clone();
        append_reference(&mut ambiguous, 2, 300, "alternate", 309, 2);
        let mut ambiguous_second = second.clone();
        append_reference(&mut ambiguous_second, 2, 300, "alternate", 309, 2);
        let mut ambiguous_third = third.clone();
        append_reference(&mut ambiguous_third, 2, 300, "alternate", 309, 2);
        let ambiguous_members = [&ambiguous, &ambiguous_second, &ambiguous_third];
        crate::test_support::with_decode_context(|ctx| {
            assert_eq!(
                loft_edge_profile_face_slot(3, &ambiguous_members, ctx).unwrap(),
                None
            );
        });
    }

    #[test]
    fn loft_edge_profile_projects_the_proven_face_into_history() {
        let scope = loft_scope();
        let group = loft_group();
        let feature = crate::ids::neutral_feature_id(&scope);
        let feature_key = feature.key();
        let expected_state = crate::ids::feature_input_topology_id(&feature, 6);
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
            crate::test_support::with_decode_context(|decode_ctx| super::resolved_loft_edge_profile_group(decode_ctx, &scope, &group, &operands)).unwrap(),
            Some(cadmpeg_ir::features::ProfileRef::Planar(cadmpeg_ir::features::PlanarProfileRef::HistoricalFaces {
                state,
                faces,
                native,
            })) if state == expected_state && faces.as_slice() == [expected_face] && native.as_slice() == [group.id]
        ));
    }

    fn assert_loft_edge_profile_collection_limit(operation: &'static str) {
        use cadmpeg_core::decode::ResourceDimension;
        use cadmpeg_core::CodecError;

        let scope = loft_scope();
        let group = loft_group();
        let operands = vec![
            edge_operand(1, 100, 0, 3, 200),
            edge_operand(2, 100, 1, 3, 201),
            edge_operand(3, 100, 0, 3, 202),
        ];
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::CollectionItems,
            operation,
            0,
            |ctx| super::resolved_loft_edge_profile_group(ctx, &scope, &group, &operands),
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == operation));
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
    fn loft_edge_profile_native_group_refuses_collection_limit() {
        assert_loft_edge_profile_collection_limit("f3d historical profile native group");
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

    #[test]
    fn loft_face_profile_moves_owned_native_selection() {
        crate::test_support::with_decode_context(|decode_ctx| {
            let (mut operand, group, _) = start_geometry_fixture();
            operand.resolved_face_slots = vec![10];
            let scope = loft_scope();
            assert!(resolved_profile_face_group(
                decode_ctx,
                &scope,
                &group,
                std::slice::from_ref(&operand)
            )
            .unwrap()
            .is_some());
        });
    }

    fn assert_face_operand_retained_refusal(operand: &DesignFaceOperand, operation: &'static str) {
        use cadmpeg_core::decode::ResourceDimension;
        let (_, group, _) = start_geometry_fixture();
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::RetainedBytes,
            operation,
            0,
            |ctx| super::resolved_face_group(ctx, &group, std::slice::from_ref(operand)),
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(failure)
            if failure.operation == operation && failure.dimension == ResourceDimension::RetainedBytes)
        );
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
        use cadmpeg_core::decode::ResourceDimension;
        use cadmpeg_core::CodecError;
        let (mut operand, group, _) = start_geometry_fixture();
        operand.resolved_active_face = Some(face(10));
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::CollectionItems,
            "f3d resolved face group member",
            0,
            |ctx| resolved_face_group(ctx, &group, std::slice::from_ref(&operand)),
        );
        assert!(matches!(
            error,
            CodecError::ResourceLimit(failure)
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
            resolved_explicit_bounded_face_group(&ctx, &group, std::slice::from_ref(&operand)),
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
            resolved_explicit_bounded_face_group(&ctx, &group, std::slice::from_ref(&operand)),
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
        let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            "f3d resolved face group native id",
            |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                match ResourceDimension::RetainedBytes {
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                        policy.limits.max_retained_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        policy.limits.max_collection_items = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                        policy.limits.max_materialized_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        policy.limits.max_work_units = cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                (resolved_face_group(&ctx, &group, std::slice::from_ref(&operand))).map(|_| ())
            },
        ) {
            cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
            error => panic!("unexpected refusal: {error:?}"),
        };
        policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match ResourceDimension::RetainedBytes {
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                policy.limits.max_retained_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                policy.limits.max_collection_items = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                policy.limits.max_materialized_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                policy.limits.max_work_units = refusal_cap;
            }
            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
        }
        let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            "f3d resolved face group native id",
            |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                match ResourceDimension::RetainedBytes {
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                        policy.limits.max_retained_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        policy.limits.max_collection_items = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                        policy.limits.max_materialized_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        policy.limits.max_work_units = cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                (resolved_face_group(&ctx, &group, std::slice::from_ref(&operand))).map(|_| ())
            },
        ) {
            cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
            error => panic!("unexpected refusal: {error:?}"),
        };
        policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match ResourceDimension::RetainedBytes {
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                policy.limits.max_retained_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                policy.limits.max_collection_items = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                policy.limits.max_materialized_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                policy.limits.max_work_units = refusal_cap;
            }
            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            resolved_face_group(&ctx, &group, std::slice::from_ref(&operand)),
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
        }))
        .expect("direct face operand");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            super::resolved_direct_face_selection(&ctx, &scope, &[operand]),
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == "f3d direct face operand"
                    && failure.dimension == ResourceDimension::CollectionItems
        ));
    }
}
