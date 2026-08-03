// SPDX-License-Identifier: Apache-2.0
//! Parse edge, face, and body operand frames and recipe structure.

use crate::bytes::{is_guid_relaxed, lp_ascii_filtered, lp_utf16_bounded, take_reference};
use crate::container::{role, ContainerScan};
use crate::design::decode::dimension_frames::{
    bind_recipe_reference_candidates, decode_recipe_references, recipe_record_prefix,
};
use crate::design::decode::scopes::payload_prologue;
use crate::design::decode::sketch::{
    indexed_record_index, next_indexed_record_offset, next_indexed_record_offset_with_index,
};
use crate::design::{design_feature_family, DesignFeatureFamily};
use crate::ids::{self, native_stream};
use crate::records::{
    ConstructionRecipe, ConstructionRecipeKind, DesignBodyRecipeOperand,
    DesignBodyRecipeOperandOwner, DesignBodyRecipeReference, DesignConstructionOperandGroup,
    DesignConstructionOperandGroupFrame, DesignConstructionOperandIdentity,
    DesignConstructionPersistentIdentity, DesignConstructionTrackingPath,
    DesignEdgeIdentityOperand, DesignEdgeOperand, DesignEntityHeader, DesignEntitySelectionOperand,
    DesignExtrudeFaceRole, DesignExtrudeOperandRole, DesignExtrudePrologue,
    DesignExtrudeSelectionGroup, DesignExtrudeSelectionMember, DesignExtrudeStart,
    DesignFaceOperand, DesignFilletRadiusGroup, DesignFilletRadiusLaw, DesignParameter,
    DesignParameterOwner, DesignParameterScope, DesignRecordHeader, DesignSketchProfileOperand,
    DesignTopologyRecipeEntry, DesignTopologyRecipeSide, DesignTopologyRecipeTriplet,
    LostEdgeReference, PersistentSubentityTag, SketchCurveIdentity, SketchPoint,
    SketchRelationOperand,
};
use cadmpeg_codec_core::le::{f64_at, i32_at, u32_at, u64_at as read_u64};
use cadmpeg_codec_core::CodecError;
use std::collections::{HashMap, HashSet};

/// Decode edge-recipe operand frames named by edge-selecting feature scopes.
pub fn decode_edge_operands(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
    recipes: &[ConstructionRecipe],
) -> Result<Vec<DesignEdgeOperand>, CodecError> {
    let headers = headers
        .iter()
        .filter_map(|header| Some(((native_stream(&header.id)?, header.record_index), header)))
        .collect::<HashMap<_, _>>();
    let mut out = Vec::new();
    for scope in scopes
        .iter()
        .filter(|scope| has_edge_recipe_operands(&scope.kind))
    {
        let mut member_indices = groups
            .iter()
            .filter(|group| {
                native_stream(&group.id) == native_stream(&scope.id)
                    && group.scope_record_index == scope.record_index
            })
            .flat_map(|group| group.members.iter().copied())
            .collect::<HashSet<_>>();
        if let Some(operation) = &scope.surface_extend_operation {
            member_indices.extend(operation.edge_record_indices.iter().copied());
        }
        if let Some(operation) = &scope.surface_offset_operation {
            member_indices.extend(operation.edge_record_indices.iter().copied());
        }
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && stream == ids::native_scope(&entry.name)
        }) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for (ordinal, record_index) in scope.reference_members.iter().copied().enumerate() {
            if !member_indices.contains(&record_index) {
                continue;
            }
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let Some(header) = headers.get(&(stream, record_index)) else {
                continue;
            };
            let Some(operand) = parse_edge_operand(bytes, scope, ordinal, header, recipes) else {
                continue;
            };
            out.push(operand);
        }
    }
    out.sort_by_key(|operand| operand.id.clone());
    Ok(out)
}

/// Whether a feature family owns edge-recipe operands directly or through a
/// counted construction-operand group.
pub(crate) fn has_edge_recipe_operands(kind: &str) -> bool {
    matches!(
        design_feature_family(kind),
        Some(
            DesignFeatureFamily::Fillet
                | DesignFeatureFamily::Chamfer
                | DesignFeatureFamily::Revolve
                | DesignFeatureFamily::Loft
                | DesignFeatureFamily::Sweep
                | DesignFeatureFamily::Pipe
                | DesignFeatureFamily::SurfacePatch
                | DesignFeatureFamily::SurfaceExtend
                | DesignFeatureFamily::SurfaceOffset
                | DesignFeatureFamily::SurfaceRuled
        )
    ) || matches!(kind, "EdgeFlange" | "Hem")
}

/// Decode persistent selection identities named by Fillet and Chamfer groups.
pub fn decode_edge_identity_operands(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignEdgeIdentityOperand>, CodecError> {
    let headers = headers
        .iter()
        .filter_map(|header| Some(((native_stream(&header.id)?, header.record_index), header)))
        .collect::<HashMap<_, _>>();
    let mut out = Vec::new();
    for group in groups {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(scope) = scopes.iter().find(|scope| {
            native_stream(&scope.id) == Some(stream)
                && scope.record_index == group.scope_record_index
                && matches!(
                    design_feature_family(&scope.kind),
                    Some(DesignFeatureFamily::Fillet | DesignFeatureFamily::Chamfer)
                )
        }) else {
            continue;
        };
        let Some(entry) = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && stream == ids::native_scope(&entry.name)
        }) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for (ordinal, record_index) in group.members.iter().copied().enumerate() {
            let Some(header) = headers.get(&(stream, record_index)) else {
                continue;
            };
            let Ok(start) = usize::try_from(header.byte_offset) else {
                continue;
            };
            let Some(parsed) = parse_edge_identity_member(bytes, start) else {
                continue;
            };
            let Ok(group_member_ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            out.push(DesignEdgeIdentityOperand {
                id: ids::native_design_edge_identity_operand_id(&entry.name, header.byte_offset),
                scope_record_index: scope.record_index,
                group_record_index: group.record_index,
                group_member_ordinal,
                record_index,
                byte_offset: header.byte_offset,
                class_tag: header.class_tag.clone(),
                compact_layout: parsed.compact_layout,
                local_id: parsed.local_id,
                local_id_offset: parsed.local_id_offset,
                asset_id: parsed.asset_id,
                asset_id_offset: parsed.asset_id_offset,
                context_id: parsed.context_id,
                context_id_offset: parsed.context_id_offset,
                historical_entity_kind: None,
                historical_entity_ref: None,
                historical_state_ids: Vec::new(),
                treatment_radius_candidates: Vec::new(),
                transition_edge_candidates: Vec::new(),
                resolved_edge_slots: Vec::new(),
                resolved_edge_slot: None,
                resolution_identity_id: None,
            });
        }
    }
    out.sort_by_key(|operand| operand.id.clone());
    Ok(out)
}

/// Decode face-recipe operand frames named by grouped and direct feature references.
pub fn decode_face_operands(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
    recipes: &[ConstructionRecipe],
) -> Result<Vec<DesignFaceOperand>, CodecError> {
    let headers = headers
        .iter()
        .filter_map(|header| Some(((native_stream(&header.id)?, header.record_index), header)))
        .collect::<HashMap<_, _>>();
    let scopes = scopes
        .iter()
        .filter_map(|scope| Some(((native_stream(&scope.id)?, scope.record_index), scope)))
        .collect::<HashMap<_, _>>();
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for group in groups {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(scope) = scopes.get(&(stream, group.scope_record_index)) else {
            continue;
        };
        let is_extrude_operand = matches!(
            group.extrude_role,
            Some(DesignExtrudeOperandRole::Profile | DesignExtrudeOperandRole::Faces)
        );
        let is_offset_faces_operand = design_feature_family(&scope.kind)
            == Some(DesignFeatureFamily::OffsetFaces)
            && group.role == 0x0000_0010_0000_0000;
        let is_shell_operand = design_feature_family(&scope.kind)
            == Some(DesignFeatureFamily::Shell)
            && group.role == 0x0000_0010_0000_0000;
        let is_loft_profile = design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Loft)
            && matches!(group.role, 0x0000_0041_0000_0000 | 0x0000_0043_0000_0000);
        let is_sweep_guide_surface = design_feature_family(&scope.kind)
            == Some(DesignFeatureFamily::Sweep)
            && group.role == 0x0000_0011_0000_0000;
        let is_edge_treatment_support = matches!(
            design_feature_family(&scope.kind),
            Some(DesignFeatureFamily::Fillet | DesignFeatureFamily::Chamfer)
        );
        let is_circular_pattern_seed = design_feature_family(&scope.kind)
            == Some(DesignFeatureFamily::CircularPattern)
            && group.role == 0x0000_0008_0000_0000;
        let is_mirror_seed = design_feature_family(&scope.kind)
            == Some(DesignFeatureFamily::Mirror)
            && group.role == 0x0000_0008_0000_0000;
        let is_split_face_operand = scope.kind == "SplitFace";
        let is_delete_face_operand = scope.kind == "DeleteFace";
        let is_thread_face = scope.kind == "Thread" && group.role == 0x0000_0010_0000_0000;
        let is_draft_operand =
            design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Draft);
        if !is_extrude_operand
            && !is_offset_faces_operand
            && !is_shell_operand
            && !is_loft_profile
            && !is_sweep_guide_surface
            && !is_edge_treatment_support
            && !is_circular_pattern_seed
            && !is_mirror_seed
            && !is_split_face_operand
            && !is_delete_face_operand
            && !is_thread_face
            && !is_draft_operand
        {
            continue;
        }
        if group.extrude_role == Some(DesignExtrudeOperandRole::Profile)
            && scope.extrude_profile.is_some()
        {
            continue;
        }
        let Some(entry) = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && stream == ids::native_scope(&entry.name)
        }) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for (group_member_index, record_index) in group.members.iter().enumerate() {
            if !seen.insert((stream, scope.record_index, *record_index)) {
                continue;
            }
            let Ok(group_member_ordinal) = u32::try_from(group_member_index) else {
                continue;
            };
            let Some(header) = headers.get(&(stream, *record_index)) else {
                continue;
            };
            let next_byte_offset = group
                .members
                .get(group_member_index + 1)
                .and_then(|record_index| headers.get(&(stream, *record_index)))
                .copied()
                .map(|header| header.byte_offset)
                .or_else(|| {
                    if !is_offset_faces_operand && !is_shell_operand {
                        return None;
                    }
                    scope
                        .reference_members
                        .iter()
                        .position(|candidate| candidate == record_index)
                        .and_then(|ordinal| scope.reference_members.get(ordinal + 1))
                        .and_then(|record_index| headers.get(&(stream, *record_index)))
                        .map(|header| header.byte_offset)
                });
            if let Some(operand) = parse_face_operand(
                bytes,
                scope,
                group.scope_reference_ordinal,
                Some((group.record_index, group_member_ordinal)),
                next_byte_offset,
                header,
                recipes,
            ) {
                out.push(operand);
            }
        }
    }
    for scope in scopes.values().filter(|scope| {
        matches!(
            design_feature_family(&scope.kind),
            Some(
                DesignFeatureFamily::OffsetFaces
                    | DesignFeatureFamily::Shell
                    | DesignFeatureFamily::Thicken
                    | DesignFeatureFamily::Split
            )
        ) || scope.kind == "SplitFace"
    }) {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && stream == ids::native_scope(&entry.name)
        }) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for (ordinal, record_index) in scope.reference_members.iter().copied().enumerate() {
            if !seen.insert((stream, scope.record_index, record_index)) {
                continue;
            }
            let (Ok(scope_reference_ordinal), Some(header)) =
                (u32::try_from(ordinal), headers.get(&(stream, record_index)))
            else {
                continue;
            };
            if let Some(operand) = parse_face_operand(
                bytes,
                scope,
                scope_reference_ordinal,
                None,
                None,
                header,
                recipes,
            ) {
                out.push(operand);
            }
        }
    }
    out.sort_by_key(|operand| operand.id.clone());
    Ok(out)
}

/// Join each face recipe's persistent Design reference to active solved faces.
pub fn bind_face_operand_candidates(
    operands: &mut [DesignFaceOperand],
    recipes: &[ConstructionRecipe],
    tags: &[PersistentSubentityTag],
) {
    use cadmpeg_ir::attributes::AttributeTarget;

    let recipes = recipes
        .iter()
        .map(|recipe| (recipe.id.as_str(), recipe))
        .collect::<HashMap<_, _>>();
    for operand in operands {
        operand.alternate_selector_candidate_faces.clear();
        for reference in &mut operand.recipe_references {
            bind_recipe_reference_candidates(reference, tags, Some(&operand.id));
        }
        let Some(design_reference) = recipes
            .get(operand.recipe_id.as_str())
            .map(|recipe| i64::from(recipe.record_index))
            .filter(|value| *value >= 0)
        else {
            continue;
        };
        operand.candidate_faces = tags
            .iter()
            .filter(|tag| {
                crate::ids::same_native_occurrence(&tag.id, &operand.id)
                    && tag.design_references.contains(&design_reference)
            })
            .filter_map(|tag| match &tag.target {
                AttributeTarget::Face(id) => Some(id.clone()),
                _ => None,
            })
            .collect();
        operand
            .candidate_faces
            .sort_by(|left, right| left.0.cmp(&right.0));
        operand.candidate_faces.dedup();
        let referenced = operand
            .recipe_references
            .iter()
            .filter(|reference| reference.design_reference == design_reference)
            .flat_map(|reference| &reference.candidate_faces)
            .collect::<HashSet<_>>();
        operand.unreferenced_candidate_faces = operand
            .candidate_faces
            .iter()
            .filter(|face| !referenced.contains(face))
            .cloned()
            .collect();
        operand.alternate_selector_candidate_faces = operand
            .recipe_references
            .iter()
            .filter(|reference| reference.design_reference == design_reference)
            .flat_map(|reference| &reference.alternate_selector_faces)
            .cloned()
            .collect();
        operand
            .alternate_selector_candidate_faces
            .sort_by(|left, right| left.0.cmp(&right.0));
        operand.alternate_selector_candidate_faces.dedup();
    }
}

/// Join each edge recipe's persistent Design reference to active solved faces.
pub fn bind_edge_operand_candidates(
    operands: &mut [DesignEdgeOperand],
    recipes: &[ConstructionRecipe],
    tags: &[PersistentSubentityTag],
) {
    let recipes = recipes
        .iter()
        .map(|recipe| (recipe.id.as_str(), recipe))
        .collect::<HashMap<_, _>>();
    for operand in operands {
        operand.candidate_faces.clear();
        for reference in &mut operand.recipe_references {
            bind_recipe_reference_candidates(reference, tags, Some(&operand.id));
        }
        let Some(design_reference) = recipes
            .get(operand.recipe_id.as_str())
            .map(|recipe| i64::from(recipe.record_index))
            .filter(|value| *value >= 0)
        else {
            continue;
        };
        operand.candidate_faces =
            edge_operand_candidate_faces(design_reference, tags, Some(&operand.id));
    }
}

pub(crate) fn edge_operand_candidate_faces(
    design_reference: i64,
    tags: &[PersistentSubentityTag],
    owner_id: Option<&str>,
) -> Vec<cadmpeg_ir::ids::FaceId> {
    use cadmpeg_ir::attributes::AttributeTarget;

    let mut faces = tags
        .iter()
        .filter(|tag| {
            owner_id.is_none_or(|owner_id| crate::ids::same_native_occurrence(&tag.id, owner_id))
                && tag.design_references.contains(&design_reference)
        })
        .filter_map(|tag| match &tag.target {
            AttributeTarget::Face(id) => Some(id.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    faces.sort_by(|left, right| left.0.cmp(&right.0));
    faces.dedup();
    faces
}

/// Resolve the unique sketch-profile frame named by profile-based scopes.
pub fn bind_sketch_profiles(
    scan: &ContainerScan,
    scopes: &mut [DesignParameterScope],
    headers: &[DesignRecordHeader],
    entities: &[DesignEntityHeader],
) -> Result<(), CodecError> {
    let headers = headers
        .iter()
        .filter_map(|header| Some(((native_stream(&header.id)?, header.record_index), header)))
        .collect::<HashMap<_, _>>();
    for scope in scopes.iter_mut().filter(|scope| {
        design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Extrude)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Sweep)
            || scope.kind == "BaseFlange"
    }) {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && stream == ids::native_scope(&entry.name)
        }) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let candidates = scope
            .reference_members
            .iter()
            .copied()
            .enumerate()
            .filter_map(|(ordinal, record_index)| {
                let ordinal = u32::try_from(ordinal).ok()?;
                let header = headers.get(&(stream, record_index))?;
                parse_sketch_profile(bytes, stream, ordinal, header, entities)
            })
            .collect::<Vec<_>>();
        if let [profile] = candidates.as_slice() {
            if scope.kind == "BaseFlange" {
                scope.base_flange_profile = Some(profile.clone());
            } else if design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Sweep) {
                scope.sweep_profile = Some(profile.clone());
            } else {
                scope.extrude_profile = Some(profile.clone());
            }
        }
    }
    Ok(())
}

/// Decode the counted selection group named by each Extrude scope.
pub fn decode_extrude_selection_groups(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignExtrudeSelectionGroup>, CodecError> {
    let headers = headers
        .iter()
        .filter_map(|header| Some(((native_stream(&header.id)?, header.record_index), header)))
        .collect::<HashMap<_, _>>();
    let mut out = Vec::new();
    for scope in scopes
        .iter()
        .filter(|scope| design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Extrude))
    {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && stream == ids::native_scope(&entry.name)
        }) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for (ordinal, record_index) in scope.reference_members.iter().copied().enumerate() {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let Some(header) = headers.get(&(stream, record_index)) else {
                continue;
            };
            if let Some(mut group) = parse_extrude_selection_group(bytes, scope, ordinal, header) {
                group.id =
                    ids::native_design_extrude_selection_group_id(&entry.name, header.byte_offset);
                out.push(group);
            }
        }
    }
    out.sort_by_key(|group| group.id.clone());
    Ok(out)
}

/// Decode counted construction-operand groups named by feature scopes.
///
/// A scope reference member whose record opens the group grammar but does not
/// close it is recorded on the owning scope, so a group the grammar cannot read
/// is distinguishable from a reference member that is not a group at all.
pub fn decode_construction_operand_groups(
    scan: &ContainerScan,
    scopes: &mut [DesignParameterScope],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignConstructionOperandGroup>, CodecError> {
    let headers = headers
        .iter()
        .filter_map(|h| Some(((native_stream(&h.id)?, h.record_index), h)))
        .collect::<HashMap<_, _>>();
    let mut out = Vec::new();
    for scope in scopes.iter_mut().filter(|scope| {
        design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Extrude)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Coil)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Loft)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Sweep)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Pipe)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::OffsetFaces)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Revolve)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Shell)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Thicken)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Move)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::SurfacePatch)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::SurfaceRuled)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::BoundaryFill)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Split)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Draft)
            || scope.kind == "SplitFace"
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Scale)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::CircularPattern)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Mirror)
            || scope.kind == "RemoveBody"
            || scope.kind == "SurfaceStitch"
            || scope.kind == "DeleteFace"
            || scope.kind == "Decal"
            || scope.kind == "Thread"
            || matches!(scope.kind.as_str(), "BaseFlange" | "EdgeFlange" | "Hem")
            || has_typed_edge_treatment_group(&scope.kind)
    }) {
        let scope_group_start = out.len();
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && stream == ids::native_scope(&entry.name)
        }) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut unclosed = Vec::new();
        for (ordinal, record_index) in scope.reference_members.iter().copied().enumerate() {
            let (Ok(ordinal), Some(header)) =
                (u32::try_from(ordinal), headers.get(&(stream, record_index)))
            else {
                continue;
            };
            match parse_construction_operand_group(bytes, scope, ordinal, header) {
                ConstructionOperandGroupParse::Complete(mut group) => {
                    group.id = ids::native_design_construction_operand_group_id(
                        &entry.name,
                        header.byte_offset,
                    );
                    out.push(*group);
                }
                ConstructionOperandGroupParse::Unclosed => unclosed.push(record_index),
                ConstructionOperandGroupParse::NotAGroup => {}
            }
        }
        scope.unclosed_construction_operand_groups = unclosed;
        if design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Extrude) {
            assign_extrude_face_roles(scope, &mut out[scope_group_start..]);
        }
    }
    out.sort_by_key(|group| group.id.clone());
    Ok(out)
}

pub(crate) fn assign_extrude_face_roles(
    scope: &DesignParameterScope,
    groups: &mut [DesignConstructionOperandGroup],
) {
    let mut face_groups = groups
        .iter_mut()
        .filter(|group| group.extrude_role == Some(DesignExtrudeOperandRole::Faces));
    if scope.extrude_prologue.map(DesignExtrudePrologue::start)
        == Some(DesignExtrudeStart::FromFace)
    {
        if let Some(group) = face_groups.next() {
            group.extrude_face_role = Some(DesignExtrudeFaceRole::Start);
        }
    }
    for group in face_groups {
        group.extrude_face_role = Some(DesignExtrudeFaceRole::Termination);
    }
}

/// Pair Fillet construction-operand groups with their radius inputs.
pub fn decode_fillet_radius_groups(
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    owners: &[DesignParameterOwner],
    parameters: &[DesignParameter],
) -> Vec<DesignFilletRadiusGroup> {
    let parameters = parameters
        .iter()
        .filter_map(|parameter| {
            Some((
                (native_stream(&parameter.id)?, parameter.record_index),
                parameter,
            ))
        })
        .collect::<HashMap<_, _>>();
    let mut out = Vec::new();
    for scope in scopes
        .iter()
        .filter(|scope| design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Fillet))
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
        scope_groups.sort_by_key(|group| group.scope_reference_ordinal);
        let mut owned_parameters = owners
            .iter()
            .filter(|owner| {
                native_stream(&owner.id) == Some(stream)
                    && owner.scope_record_index == scope.record_index
            })
            .filter_map(|owner| {
                Some((
                    owner.local_ordinal,
                    *parameters.get(&(stream, owner.parameter_record_index))?,
                ))
            })
            .collect::<Vec<_>>();
        owned_parameters.sort_by_key(|(ordinal, _)| *ordinal);
        let radii = owned_parameters
            .iter()
            .filter_map(|(_, parameter)| (parameter.source_kind == "Radius").then_some(*parameter))
            .collect::<Vec<_>>();
        let weights = owned_parameters
            .iter()
            .filter_map(|(_, parameter)| {
                (parameter.source_kind == "TangencyWeight").then_some(*parameter)
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
                out.push(DesignFilletRadiusGroup {
                    id: format!("{stream}:design-fillet-radius-group#{}", group.record_index),
                    scope_record_index: scope.record_index,
                    group_ordinal,
                    group_record_index: group.record_index,
                    edge_operand_record_indices: group.members.clone(),
                    law: DesignFilletRadiusLaw::Constant {
                        radius_parameter_record_index: radius.record_index,
                    },
                    tangency_weight_parameter_record_index: weights
                        .get(ordinal)
                        .map(|parameter| parameter.record_index),
                });
            }
            continue;
        }
        let [group] = scope_groups.as_slice() else {
            continue;
        };
        let chord_lengths = owned_parameters
            .iter()
            .filter_map(|(_, parameter)| {
                (parameter.source_kind == "ChordLen").then_some(parameter.record_index)
            })
            .collect::<Vec<_>>();
        if weights.len() == 1 && owned_parameters.len() == 2 {
            let [chord_length] = chord_lengths.as_slice() else {
                continue;
            };
            out.push(DesignFilletRadiusGroup {
                id: format!("{stream}:design-fillet-radius-group#{}", group.record_index),
                scope_record_index: scope.record_index,
                group_ordinal: 0,
                group_record_index: group.record_index,
                edge_operand_record_indices: group.members.clone(),
                law: DesignFilletRadiusLaw::Chordal {
                    chord_length_parameter_record_index: *chord_length,
                },
                tangency_weight_parameter_record_index: Some(weights[0].record_index),
            });
            continue;
        }
        let asymmetric_offsets = |kind: &str| {
            owned_parameters
                .iter()
                .filter_map(|(_, parameter)| {
                    (parameter.source_kind == kind).then_some(parameter.record_index)
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
                out.push(DesignFilletRadiusGroup {
                    id: format!("{stream}:design-fillet-radius-group#{}", group.record_index),
                    scope_record_index: scope.record_index,
                    group_ordinal: 0,
                    group_record_index: group.record_index,
                    edge_operand_record_indices: group.members.clone(),
                    law: DesignFilletRadiusLaw::Asymmetric {
                        offset_one_parameter_record_index: *offset_one,
                        offset_two_parameter_record_index: *offset_two,
                    },
                    tangency_weight_parameter_record_index: Some(weight.record_index),
                });
                continue;
            }
        }
        let records = |kind: &str| {
            owned_parameters
                .iter()
                .filter_map(|(_, parameter)| {
                    (parameter.source_kind == kind).then_some(parameter.record_index)
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
        let variable_parameter_count = 2 + middle_radii.len() + middle_parameters.len() + 1;
        if middle_radii.len() != middle_parameters.len()
            || weights.len() != 1
            || owned_parameters.len() != variable_parameter_count
        {
            continue;
        }
        out.push(DesignFilletRadiusGroup {
            id: format!("{stream}:design-fillet-radius-group#{}", group.record_index),
            scope_record_index: scope.record_index,
            group_ordinal: 0,
            group_record_index: group.record_index,
            edge_operand_record_indices: group.members.clone(),
            law: DesignFilletRadiusLaw::Variable {
                start_radius_parameter_record_index: *start,
                end_radius_parameter_record_index: *end,
                middle_radius_parameter_record_indices: middle_radii,
                middle_parameter_record_indices: middle_parameters,
            },
            tangency_weight_parameter_record_index: Some(weights[0].record_index),
        });
    }
    out.sort_by_key(|group| group.id.clone());
    out
}

/// Remove fixed Fillet interpretations of frames that are indexed parameter owners.
pub fn disambiguate_fixed_fillet_parameters(
    scopes: &mut [DesignParameterScope],
    owners: &[DesignParameterOwner],
) {
    let indexed_scopes = owners
        .iter()
        .filter_map(|owner| {
            Some((
                native_stream(&owner.id)?.to_owned(),
                owner.scope_record_index,
            ))
        })
        .collect::<HashSet<_>>();
    for scope in scopes {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        if indexed_scopes.contains(&(stream.to_owned(), scope.record_index)) {
            scope.fixed_fillet_parameters = None;
        }
    }
}

/// Outcome of reading a scope reference member as a construction-operand group.
pub(crate) enum ConstructionOperandGroupParse {
    /// The record does not open a construction-operand group.
    NotAGroup,
    /// The record opens a group the grammar does not close.
    Unclosed,
    /// A complete group.
    Complete(Box<DesignConstructionOperandGroup>),
}

impl ConstructionOperandGroupParse {
    /// The group, where the record carried a complete one.
    #[cfg(test)]
    pub(crate) fn complete(self) -> Option<DesignConstructionOperandGroup> {
        match self {
            Self::Complete(group) => Some(*group),
            Self::NotAGroup | Self::Unclosed => None,
        }
    }
}

/// Read the construction-operand group at `header`.
///
/// The record's members are a leading-block presence byte, the property block
/// its presence byte gates, the counted member run, two optional references,
/// the counted trailing-reference run, a zero u32 and the u32 role, ten zero bytes, an
/// ordinal, a duration, and a repeat of the ordinal that one container
/// generation omits. The tail is a reference to record `N + 2`, a flag block,
/// a reference to record `N + 1`, a zero byte, the owning scope's reference,
/// and the same-index paired header. The flag block's last byte is zero; one
/// container generation prefixes it with a further byte. Neither the repeated
/// ordinal nor the prefix byte is announced, so the tail settles both: exactly
/// one of the four readings reaches a paired header carrying this record's own
/// index.
pub(crate) fn parse_construction_operand_group(
    bytes: &[u8],
    scope: &DesignParameterScope,
    scope_reference_ordinal: u32,
    header: &DesignRecordHeader,
) -> ConstructionOperandGroupParse {
    use ConstructionOperandGroupParse::{Complete, NotAGroup, Unclosed};

    let Ok(start) = usize::try_from(header.byte_offset) else {
        return NotAGroup;
    };
    // The indexed header is the three-digit class tag, the u64 entity id whose
    // low word is the record index, and the record's own empty name.
    if bytes.get(start + 11..start + 19) != Some(&[0; 8]) {
        return NotAGroup;
    }
    let Some(mut cursor) = payload_prologue(bytes, start + 19, bytes.len()) else {
        return NotAGroup;
    };
    let member_count_at = cursor;
    let Some(member_count) = u32_at(bytes, cursor) else {
        return NotAGroup;
    };
    cursor += 4;
    // A reference is at least one byte, so a count the remaining bytes cannot
    // supply is corrupt and must not reach the allocator.
    if usize::try_from(member_count).unwrap_or(usize::MAX) > bytes.len().saturating_sub(cursor) {
        return NotAGroup;
    }
    let mut members = Vec::new();
    let mut member_offsets = Vec::new();
    for _ in 0..member_count {
        let Some((record_index, offset)) = take_record_reference(bytes, &mut cursor) else {
            return NotAGroup;
        };
        members.push(record_index);
        member_offsets.push(offset);
    }
    let mut auxiliary_record_indices = Vec::new();
    let mut auxiliary_record_offsets = Vec::new();
    for _ in 0..2 {
        if bytes.get(cursor) == Some(&0) {
            cursor += 1;
            continue;
        }
        let Some((record_index, offset)) = take_record_reference(bytes, &mut cursor) else {
            return NotAGroup;
        };
        auxiliary_record_indices.push(record_index);
        auxiliary_record_offsets.push(offset);
    }
    let Some(trailing_count) = u32_at(bytes, cursor) else {
        return NotAGroup;
    };
    cursor += 4;
    if usize::try_from(trailing_count).unwrap_or(usize::MAX) > bytes.len().saturating_sub(cursor) {
        return NotAGroup;
    }
    let mut trailing_record_indices = Vec::new();
    let mut trailing_record_offsets = Vec::new();
    for _ in 0..trailing_count {
        let Some((record_index, offset)) = take_record_reference(bytes, &mut cursor) else {
            return NotAGroup;
        };
        trailing_record_indices.push(record_index);
        trailing_record_offsets.push(offset);
    }
    // The role occupies the high word of a u64 whose low word is zero.
    let role_at = cursor;
    let (Some(0), Some(role)) = (u32_at(bytes, role_at), read_u64(bytes, role_at)) else {
        return NotAGroup;
    };
    cursor += 8;
    if bytes.get(cursor..cursor + 10) != Some(&[0; 10]) {
        return NotAGroup;
    }
    cursor += 10;
    let opaque_index_at = cursor;
    let (Some(opaque_index), Some(opaque_scalar)) =
        (u32_at(bytes, cursor), f64_at(bytes, cursor + 4))
    else {
        return NotAGroup;
    };
    if opaque_index == 0 || !opaque_scalar.is_finite() {
        return NotAGroup;
    }
    cursor += 12;

    // Past this point the record has opened the group grammar, so a tail that
    // does not close is a group this reader cannot name.
    let mut closed = None;
    for repeats_ordinal in [true, false] {
        let mut tail = cursor;
        if repeats_ordinal {
            if u32_at(bytes, tail) != Some(opaque_index) {
                continue;
            }
            tail += 4;
        }
        if take_record_reference(bytes, &mut tail).map(|(index, _)| index)
            != header.record_index.checked_add(2)
        {
            continue;
        }
        for flag_bytes in [2usize, 3] {
            let Some(flags) = bytes.get(tail..tail + flag_bytes) else {
                continue;
            };
            if flags.last() != Some(&0) {
                continue;
            }
            // The wider block prefixes the narrower one, so the variant flag is
            // always the byte before the terminating zero.
            let variant = flags[flag_bytes - 2] != 0;
            let mut after = tail + flag_bytes;
            if take_record_reference(bytes, &mut after).map(|(index, _)| index)
                != header.record_index.checked_add(1)
            {
                continue;
            }
            if bytes.get(after) != Some(&0) {
                continue;
            }
            after += 1;
            if take_record_reference(bytes, &mut after).map(|(index, _)| index)
                != Some(scope.record_index)
            {
                continue;
            }
            let Some((paired_class_tag, after_tag)) =
                lp_ascii_filtered(bytes, after, 3..=3, u8::is_ascii_digit)
            else {
                continue;
            };
            if u32_at(bytes, after_tag) != Some(header.record_index) {
                continue;
            }
            if closed.replace((variant, after, paired_class_tag)).is_some() {
                return Unclosed;
            }
        }
    }
    let Some((variant, paired_at, paired_class_tag)) = closed else {
        return Unclosed;
    };

    let extrude_role = if design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Extrude) {
        match role {
            0x0000_0004_0000_0000 | 0x0000_0008_0000_0000 => Some(DesignExtrudeOperandRole::Bodies),
            0x0000_0041_0000_0000 => Some(DesignExtrudeOperandRole::Profile),
            0x0000_0011_0000_0000 => Some(DesignExtrudeOperandRole::Faces),
            0x0000_0005_0000_0000
                if scope.extrude_prologue.map(DesignExtrudePrologue::start)
                    == Some(DesignExtrudeStart::FromFace) =>
            {
                Some(DesignExtrudeOperandRole::Faces)
            }
            _ => None,
        }
    } else {
        None
    };
    let (Ok(member_count_offset), Ok(role_offset), Ok(opaque_index_offset), Ok(paired_byte_offset)) = (
        u64::try_from(member_count_at),
        u64::try_from(role_at),
        u64::try_from(opaque_index_at),
        u64::try_from(paired_at),
    ) else {
        return Unclosed;
    };
    Complete(Box::new(DesignConstructionOperandGroup {
        id: String::new(),
        scope_record_index: scope.record_index,
        scope_reference_ordinal,
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        members,
        lost_edge_references: Vec::new(),
        member_offsets,
        frame: DesignConstructionOperandGroupFrame {
            member_count_offset,
            auxiliary_record_indices,
            auxiliary_record_offsets,
            auxiliary_paths: Vec::new(),
            trailing_record_indices,
            trailing_record_offsets,
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index,
            opaque_index_offset,
            opaque_scalar,
            opaque_scalar_offset: opaque_index_offset + 4,
            variant,
        },
        role,
        extrude_role,
        extrude_face_role: None,
        role_offset,
        paired_class_tag,
        paired_byte_offset,
    }))
}

/// Bind exact typed records selected by construction-group trailing runs.
pub fn bind_construction_operand_trailing_records(
    scan: &ContainerScan,
    groups: &mut [DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
) -> Result<(), CodecError> {
    let headers = headers
        .iter()
        .filter_map(|header| Some(((native_stream(&header.id)?, header.record_index), header)))
        .collect::<HashMap<_, _>>();
    for group in groups {
        group.frame.trailing_transforms.clear();
        group.frame.trailing_dual_transforms.clear();
        group.frame.trailing_flags.clear();
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(entry) = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && stream == ids::native_scope(&entry.name)
        }) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for record_index in &group.frame.trailing_record_indices {
            let Some(header) = headers.get(&(stream, *record_index)) else {
                continue;
            };
            if let Some(transform) = parse_construction_operand_transform(bytes, header) {
                group.frame.trailing_transforms.push(transform);
            } else if let Some(transform) = parse_construction_operand_dual_transform(bytes, header)
            {
                group.frame.trailing_dual_transforms.push(transform);
            } else if let Some(flag) = parse_construction_operand_flag(bytes, header) {
                group.frame.trailing_flags.push(flag);
            }
        }
    }
    Ok(())
}

pub(crate) fn parse_construction_operand_flag(
    bytes: &[u8],
    header: &DesignRecordHeader,
) -> Option<crate::records::DesignConstructionOperandFlag> {
    let start = usize::try_from(header.byte_offset).ok()?;
    if bytes.get(start + 11..start + 21)? != [0; 10]
        || bytes.get(start + 21) != Some(&1)
        || bytes.get(start + 23) != Some(&0)
    {
        return None;
    }
    let value = match bytes.get(start + 22)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    Some(crate::records::DesignConstructionOperandFlag {
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        value,
        value_offset: u64::try_from(start + 22).ok()?,
    })
}

/// Bind exact persistent-entity path records selected by construction groups.
pub fn bind_construction_operand_paths(
    scan: &ContainerScan,
    groups: &mut [DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
) -> Result<(), CodecError> {
    let headers = headers
        .iter()
        .filter_map(|header| Some(((native_stream(&header.id)?, header.record_index), header)))
        .collect::<HashMap<_, _>>();
    for group in groups {
        group.frame.auxiliary_paths.clear();
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(entry) = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && stream == ids::native_scope(&entry.name)
        }) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for record_index in &group.frame.auxiliary_record_indices {
            let Some(header) = headers.get(&(stream, *record_index)) else {
                continue;
            };
            if let Some(path) =
                parse_construction_operand_path(bytes, group.scope_record_index, header)
            {
                group.frame.auxiliary_paths.push(path);
            }
        }
    }
    Ok(())
}

pub(crate) fn parse_construction_operand_path(
    bytes: &[u8],
    expected_scope_record_index: u32,
    header: &DesignRecordHeader,
) -> Option<crate::records::DesignConstructionOperandPath> {
    use crate::design::decode::sketch::valid_sketch_transform;

    let start = usize::try_from(header.byte_offset).ok()?;
    if bytes.get(start + 11..start + 21)? != [0; 10] || bytes.get(start + 21) != Some(&1) {
        return None;
    }
    let entity_ref = read_u64(bytes, start + 22)?;
    let entity_ref_offset = u64::try_from(start + 22).ok()?;
    let (transform, transform_offset, compact_variant, mut cursor) =
        if bytes.get(start + 30..start + 33)? == [0; 3] {
            let values = cadmpeg_codec_core::le::f64s_at(bytes, start + 33, 16)?;
            let mut transform = [[0.0; 4]; 4];
            for (ordinal, value) in values.iter().copied().enumerate() {
                transform[ordinal / 4][ordinal % 4] = value;
            }
            if !valid_sketch_transform(&transform) || bytes.get(start + 161) != Some(&0) {
                return None;
            }
            (
                Some(transform),
                Some(u64::try_from(start + 33).ok()?),
                None,
                start + 162,
            )
        } else {
            let variant = match bytes.get(start + 30..start + 34)? {
                [0, 0, variant @ (0 | 1), 0] => *variant != 0,
                _ => return None,
            };
            (None, None, Some(variant), start + 34)
        };
    let (scope_record_index, scope_record_index_offset) =
        take_record_reference(bytes, &mut cursor)?;
    if scope_record_index != expected_scope_record_index {
        return None;
    }
    let (nested_record_index, nested_record_index_offset) =
        take_record_reference(bytes, &mut cursor)?;
    if nested_record_index != header.record_index.checked_add(2)?
        || bytes.get(cursor..cursor + 6)? != [0; 6]
    {
        return None;
    }
    let following_at = cursor.checked_add(6)?;
    let (following_class_tag, after_tag) =
        lp_ascii_filtered(bytes, following_at, 3..=3, u8::is_ascii_digit)?;
    let following_record_index = u32_at(bytes, after_tag)?;
    if following_record_index != header.record_index.checked_add(1)? {
        return None;
    }
    Some(crate::records::DesignConstructionOperandPath {
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        entity_ref,
        entity_ref_offset,
        transform,
        transform_offset,
        compact_variant,
        scope_record_index,
        scope_record_index_offset,
        nested_record_index,
        nested_record_index_offset,
        following_record_index,
        following_byte_offset: u64::try_from(following_at).ok()?,
        following_class_tag,
    })
}

pub(crate) fn parse_construction_operand_transform(
    bytes: &[u8],
    header: &DesignRecordHeader,
) -> Option<crate::records::DesignConstructionOperandTransform> {
    let start = usize::try_from(header.byte_offset).ok()?;
    if bytes.get(start + 11..start + 22)? != [0; 11]
        || bytes.get(start + 150..start + 152)? != [1, 0]
    {
        return None;
    }
    let transform_at = start.checked_add(22)?;
    let transform = rigid_transform_at(bytes, transform_at)?;
    let following_at = start.checked_add(152)?;
    let (following_class_tag, after_tag) =
        lp_ascii_filtered(bytes, following_at, 3..=3, u8::is_ascii_digit)?;
    let following_record_index = u32_at(bytes, after_tag)?;
    if following_record_index != header.record_index.checked_add(1)? {
        return None;
    }
    Some(crate::records::DesignConstructionOperandTransform {
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        transform,
        transform_offset: u64::try_from(transform_at).ok()?,
        following_record_index,
        following_byte_offset: u64::try_from(following_at).ok()?,
        following_class_tag,
    })
}

pub(crate) fn parse_construction_operand_dual_transform(
    bytes: &[u8],
    header: &DesignRecordHeader,
) -> Option<crate::records::DesignConstructionOperandDualTransform> {
    let start = usize::try_from(header.byte_offset).ok()?;
    if bytes.get(start + 11..start + 21)? != [0; 10] || bytes.get(start + 277) != Some(&0) {
        return None;
    }
    let first_at = start.checked_add(21)?;
    let second_at = start.checked_add(149)?;
    Some(crate::records::DesignConstructionOperandDualTransform {
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        first_transform: rigid_transform_at(bytes, first_at)?,
        first_transform_offset: u64::try_from(first_at).ok()?,
        second_transform: rigid_transform_at(bytes, second_at)?,
        second_transform_offset: u64::try_from(second_at).ok()?,
    })
}

fn rigid_transform_at(bytes: &[u8], at: usize) -> Option<[[f64; 4]; 4]> {
    let values = cadmpeg_codec_core::le::f64s_at(bytes, at, 16)?;
    let mut transform = [[0.0; 4]; 4];
    for (ordinal, value) in values.iter().copied().enumerate() {
        transform[ordinal / 4][ordinal % 4] = value;
    }
    crate::design::decode::sketch::valid_sketch_transform(&transform).then_some(transform)
}

/// Take one reference naming a record of the same segment, advancing `at` past
/// every byte it owns. Returns the record index and the byte offset of the low
/// word of the target entity id.
fn take_record_reference(bytes: &[u8], at: &mut usize) -> Option<(u32, u64)> {
    let target_at = at.checked_add(1)?;
    let reference = take_reference(bytes, at)?;
    if reference.segment.is_some() || reference.link_name.is_some() {
        return None;
    }
    Some((
        u32::try_from(reference.target?).ok()?,
        u64::try_from(target_at).ok()?,
    ))
}

/// Decode the persistent identity frame named by each construction-operand group.
pub fn decode_construction_operand_identities(
    scan: &ContainerScan,
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignConstructionOperandIdentity>, CodecError> {
    let headers = headers
        .iter()
        .filter_map(|header| Some(((native_stream(&header.id)?, header.record_index), header)))
        .collect::<HashMap<_, _>>();
    let mut out = Vec::new();
    for group in groups {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(entry) = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && stream == ids::native_scope(&entry.name)
        }) else {
            continue;
        };
        let Some(trailing_record_index) = group.frame.trailing_record_indices.first() else {
            continue;
        };
        let Some(wrapper_header) = headers.get(&(stream, *trailing_record_index)) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        if let Some(mut identity) =
            parse_construction_operand_identity(bytes, group, wrapper_header)
        {
            identity.id = ids::native_design_construction_operand_identity_id(
                &entry.name,
                wrapper_header.byte_offset,
            );
            out.push(identity);
        }
    }
    out.sort_by_key(|identity| identity.id.clone());
    Ok(out)
}

/// Bind a contiguous unresolved-edge run to the construction group whose
/// first identity wrapper terminates that run.
pub fn bind_lost_edge_groups(
    groups: &mut [DesignConstructionOperandGroup],
    identities: &[DesignConstructionOperandIdentity],
    lost_edges: &[LostEdgeReference],
) -> Result<(), CodecError> {
    for group in groups {
        group.lost_edge_references.clear();
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let mut identity_matches = identities.iter().filter(|identity| {
            native_stream(&identity.id) == Some(stream)
                && identity.group_record_index == group.record_index
        });
        let Some(identity) = identity_matches.next() else {
            continue;
        };
        if identity_matches.next().is_some() {
            return Err(CodecError::Malformed(format!(
                "Fusion construction group {} has multiple identity chains",
                group.record_index
            )));
        }
        let Some((wrapper_record_index, wrapper_byte_offset, wrapper_class_tag)) = identity
            .wrapper_record_indices
            .first()
            .zip(identity.wrapper_byte_offsets.first())
            .zip(identity.wrapper_class_tags.first())
            .map(|((record_index, byte_offset), class_tag)| {
                (*record_index, *byte_offset, class_tag.as_str())
            })
        else {
            continue;
        };
        let mut stream_edges = lost_edges
            .iter()
            .filter(|edge| native_stream(&edge.id) == Some(stream))
            .collect::<Vec<_>>();
        stream_edges.sort_by_key(|edge| edge.record_byte_offset);
        let terminals = stream_edges
            .iter()
            .enumerate()
            .filter(|(_, edge)| {
                edge.next_record_index == wrapper_record_index
                    && edge.next_byte_offset == wrapper_byte_offset
                    && edge.next_class_tag == wrapper_class_tag
            })
            .map(|(ordinal, _)| ordinal)
            .collect::<Vec<_>>();
        let [terminal] = terminals.as_slice() else {
            if terminals.is_empty() {
                continue;
            }
            return Err(CodecError::Malformed(format!(
                "Fusion construction group {} has multiple terminating lost-edge runs",
                group.record_index
            )));
        };
        let mut start = *terminal;
        while start > 0 {
            let previous = stream_edges[start - 1];
            let current = stream_edges[start];
            if previous.next_byte_offset != current.record_byte_offset
                || previous.next_record_index != current.record_index
                || previous.next_class_tag != current.class_tag
            {
                break;
            }
            start -= 1;
        }
        let run = &stream_edges[start..=*terminal];
        if run.len() != group.members.len() {
            return Err(CodecError::Malformed(format!(
                "Fusion construction group {} has {} operands but its lost-edge run has {} records",
                group.record_index,
                group.members.len(),
                run.len()
            )));
        }
        group.lost_edge_references = run.iter().map(|edge| edge.id.clone()).collect();
    }
    Ok(())
}

pub(crate) fn parse_construction_operand_identity(
    bytes: &[u8],
    group: &DesignConstructionOperandGroup,
    wrapper_header: &DesignRecordHeader,
) -> Option<DesignConstructionOperandIdentity> {
    let mut current_at = usize::try_from(wrapper_header.byte_offset).ok()?;
    let mut current_record_index = wrapper_header.record_index;
    let mut current_class_tag = wrapper_header.class_tag.clone();
    let mut chain_started = false;
    if let Some(transform) = parse_construction_operand_transform(bytes, wrapper_header) {
        current_at = usize::try_from(transform.following_byte_offset).ok()?;
        current_record_index = transform.following_record_index;
        current_class_tag = transform.following_class_tag;
        chain_started = true;
    }
    let mut wrapper_record_indices = Vec::new();
    let mut wrapper_byte_offsets = Vec::new();
    let mut wrapper_class_tags = Vec::new();
    let mut seen = HashSet::new();
    loop {
        if parse_construction_tracking_path(
            bytes,
            current_at,
            current_record_index,
            &current_class_tag,
        )
        .is_some()
            || bytes.get(current_at + 11..current_at + 21)? != [0; 10]
            || bytes.get(current_at + 21..current_at + 24)? != [1, 1, 0]
        {
            break;
        }
        if !seen.insert((current_record_index, current_at)) {
            return None;
        }
        wrapper_record_indices.push(current_record_index);
        wrapper_byte_offsets.push(u64::try_from(current_at).ok()?);
        wrapper_class_tags.push(current_class_tag);
        current_at = current_at.checked_add(24)?;
        let (next_class_tag, after_next_tag) =
            lp_ascii_filtered(bytes, current_at, 0..=2000, u8::is_ascii_graphic)?;
        current_record_index = u32_at(bytes, after_next_tag)?;
        current_class_tag = next_class_tag;
        chain_started = true;
    }
    let tracking_path = parse_construction_tracking_path(
        bytes,
        current_at,
        current_record_index,
        &current_class_tag,
    );
    if let Some(path) = &tracking_path {
        current_at = usize::try_from(path.following_byte_offset).ok()?;
        current_record_index = path.following_record_index;
        current_class_tag.clone_from(&path.following_class_tag);
        chain_started = true;
    }
    if !chain_started {
        return None;
    }
    let persistent_identity = parse_extrude_identity_member(bytes, current_at).map(|member| {
        DesignConstructionPersistentIdentity {
            local_id: member.local_id,
            local_id_offset: member.local_id_offset,
            asset_id: member.asset_id,
            asset_id_offset: member.asset_id_offset,
            context_id: member.context_id,
            context_id_offset: member.context_id_offset,
            tail_slot_present: member.tail_slot_present,
            tail_slot_offset: member.tail_slot_offset,
            next_record_index: member.next_record_index,
            next_byte_offset: member.next_byte_offset,
        }
    });
    Some(DesignConstructionOperandIdentity {
        id: String::new(),
        group_record_index: group.record_index,
        wrapper_record_indices,
        wrapper_byte_offsets,
        wrapper_class_tags,
        following_record_index: current_record_index,
        following_byte_offset: u64::try_from(current_at).ok()?,
        following_class_tag: current_class_tag,
        tracking_path,
        persistent_identity,
    })
}

pub(crate) fn parse_construction_tracking_path(
    bytes: &[u8],
    wrapper_at: usize,
    wrapper_record_index: u32,
    wrapper_class_tag: &str,
) -> Option<DesignConstructionTrackingPath> {
    if bytes.get(wrapper_at + 11..wrapper_at + 21)? != [0; 10]
        || bytes.get(wrapper_at + 21) != Some(&1)
        || read_u64(bytes, wrapper_at + 22)? != u64::from(wrapper_record_index.checked_add(1)?)
        || bytes.get(wrapper_at + 30..wrapper_at + 33)? != [0; 3]
    {
        return None;
    }
    let carrier_at = wrapper_at.checked_add(33)?;
    let (carrier_class_tag, after_carrier_tag) =
        lp_ascii_filtered(bytes, carrier_at, 3..=3, u8::is_ascii_digit)?;
    let carrier_record_index = u32_at(bytes, after_carrier_tag)?;
    if carrier_record_index != wrapper_record_index.checked_add(1)?
        || bytes.get(carrier_at + 11..carrier_at + 21)? != [0; 10]
        || u32_at(bytes, carrier_at + 21)? != 1
        || u32_at(bytes, carrier_at + 25)? != 0
        || u32_at(bytes, carrier_at + 29)? != 1
        || u32_at(bytes, carrier_at + 33)? != 2
        || read_u64(bytes, carrier_at + 45)? != 0
        || u32_at(bytes, carrier_at + 53)? != 1
        || read_u64(bytes, carrier_at + 65)? != 0
    {
        return None;
    }
    let primary_identity = read_u64(bytes, carrier_at + 37)?;
    let selector = i32_at(bytes, carrier_at + 57)?;
    let kind = u32_at(bytes, carrier_at + 61)?;
    let mut cursor = carrier_at.checked_add(73)?;
    let (first_related_identity, first_related_identity_offset) =
        take_optional_tracking_identity(bytes, &mut cursor)?;
    let (second_related_identity, second_related_identity_offset) =
        take_optional_tracking_identity(bytes, &mut cursor)?;
    let following_at = cursor;
    let (following_class_tag, after_following_tag) =
        lp_ascii_filtered(bytes, following_at, 3..=3, u8::is_ascii_digit)?;
    let following_record_index = u32_at(bytes, after_following_tag)?;
    if following_record_index != carrier_record_index.checked_add(1)? {
        return None;
    }
    Some(DesignConstructionTrackingPath {
        wrapper_record_index,
        wrapper_byte_offset: u64::try_from(wrapper_at).ok()?,
        wrapper_class_tag: wrapper_class_tag.to_owned(),
        carrier_record_index,
        carrier_byte_offset: u64::try_from(carrier_at).ok()?,
        carrier_class_tag,
        primary_identity,
        primary_identity_offset: u64::try_from(carrier_at + 37).ok()?,
        selector,
        selector_offset: u64::try_from(carrier_at + 57).ok()?,
        kind,
        kind_offset: u64::try_from(carrier_at + 61).ok()?,
        first_related_identity,
        first_related_identity_offset,
        second_related_identity,
        second_related_identity_offset,
        following_record_index,
        following_byte_offset: u64::try_from(following_at).ok()?,
        following_class_tag,
    })
}

fn take_optional_tracking_identity(
    bytes: &[u8],
    cursor: &mut usize,
) -> Option<(Option<u64>, Option<u64>)> {
    match u32_at(bytes, *cursor)? {
        0 => {
            *cursor = (*cursor).checked_add(4)?;
            Some((None, None))
        }
        1 => {
            let value_at = (*cursor).checked_add(4)?;
            let value = read_u64(bytes, value_at)?;
            *cursor = value_at.checked_add(8)?;
            Some((Some(value), Some(u64::try_from(value_at).ok()?)))
        }
        _ => None,
    }
}

pub(crate) fn parse_extrude_selection_group(
    bytes: &[u8],
    scope: &DesignParameterScope,
    scope_reference_ordinal: u32,
    header: &DesignRecordHeader,
) -> Option<DesignExtrudeSelectionGroup> {
    let start = usize::try_from(header.byte_offset).ok()?;
    if bytes.get(start + 11..start + 21)? != [0; 10]
        || bytes.get(start + 21) != Some(&1)
        || u32_at(bytes, start + 22)? != scope.record_index
        || bytes.get(start + 26..start + 32)? != [0; 6]
    {
        return None;
    }
    let member_count = usize::try_from(u32_at(bytes, start + 32)?).ok()?;
    let mut position = start.checked_add(36)?;
    // Each member consumes 11 bytes; a count the remaining bytes cannot
    // supply is corrupt and must not reach the allocator.
    if member_count == 0 || member_count > bytes.len().saturating_sub(position) / 11 {
        return None;
    }
    let mut members = Vec::with_capacity(member_count);
    let mut member_offsets = Vec::with_capacity(member_count);
    for _ in 0..member_count {
        if bytes.get(position) != Some(&1) || bytes.get(position + 5..position + 11)? != [0; 6] {
            return None;
        }
        members.push(u32_at(bytes, position + 1)?);
        member_offsets.push(u64::try_from(position + 1).ok()?);
        position = position.checked_add(11)?;
    }
    let opaque_index = u32_at(bytes, position)?;
    let opaque_scalar = f64_at(bytes, position + 4)?;
    if opaque_index == 0
        || !opaque_scalar.is_finite()
        || u32_at(bytes, position + 12)? != opaque_index
        || bytes.get(position + 16) != Some(&1)
        || u32_at(bytes, position + 17)? != header.record_index.checked_add(2)?
        || bytes.get(position + 21..position + 27)? != [0; 6]
        || bytes.get(position + 27) != Some(&1)
        || !matches!(bytes.get(position + 28), Some(0 | 1))
        || bytes.get(position + 29) != Some(&0)
        || bytes.get(position + 30) != Some(&1)
        || u32_at(bytes, position + 31)? != header.record_index.checked_add(1)?
        || bytes.get(position + 35..position + 42)? != [0; 7]
        || bytes.get(position + 42) != Some(&1)
        || u32_at(bytes, position + 43)? != scope.record_index
        || bytes.get(position + 47..position + 53)? != [0; 6]
    {
        return None;
    }
    let paired_at = position.checked_add(53)?;
    let (paired_class_tag, after_paired_tag) =
        lp_ascii_filtered(bytes, paired_at, 0..=2000, u8::is_ascii_graphic)?;
    if u32_at(bytes, after_paired_tag)? != header.record_index {
        return None;
    }
    Some(DesignExtrudeSelectionGroup {
        id: String::new(),
        scope_record_index: scope.record_index,
        scope_reference_ordinal,
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        member_count_offset: u64::try_from(start + 32).ok()?,
        members,
        member_offsets,
        opaque_index,
        opaque_index_offset: u64::try_from(position).ok()?,
        opaque_scalar,
        opaque_scalar_offset: u64::try_from(position + 4).ok()?,
        variant: bytes[position + 28] != 0,
        paired_class_tag,
        paired_byte_offset: u64::try_from(paired_at).ok()?,
    })
}

/// Decode the fixed-width records named by Extrude selection groups.
pub fn decode_extrude_selection_members(
    scan: &ContainerScan,
    groups: &[DesignExtrudeSelectionGroup],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignExtrudeSelectionMember>, CodecError> {
    let headers = headers
        .iter()
        .filter_map(|header| Some(((native_stream(&header.id)?, header.record_index), header)))
        .collect::<HashMap<_, _>>();
    let mut out = Vec::new();
    for group in groups {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(entry) = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && stream == ids::native_scope(&entry.name)
        }) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for (ordinal, record_index) in group.members.iter().copied().enumerate() {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let Some(header) = headers.get(&(stream, record_index)) else {
                continue;
            };
            if let Some(mut member) = parse_extrude_selection_member(bytes, group, ordinal, header)
            {
                member.id =
                    ids::native_design_extrude_selection_member_id(&entry.name, header.byte_offset);
                out.push(member);
            }
        }
    }
    out.sort_by_key(|member| member.id.clone());
    Ok(out)
}

/// Decode nested persistent-entity frames named by construction groups.
pub fn decode_entity_selection_operands(
    scan: &ContainerScan,
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignEntitySelectionOperand>, CodecError> {
    let headers = headers
        .iter()
        .filter_map(|header| Some(((native_stream(&header.id)?, header.record_index), header)))
        .collect::<HashMap<_, _>>();
    let mut out = Vec::new();
    for group in groups {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(entry) = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && stream == ids::native_scope(&entry.name)
        }) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for (ordinal, record_index) in group.members.iter().copied().enumerate() {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let Some(header) = headers.get(&(stream, record_index)) else {
                continue;
            };
            if let Some(mut operand) = parse_entity_selection_operand(bytes, group, ordinal, header)
            {
                operand.id =
                    ids::native_design_entity_selection_operand_id(&entry.name, header.byte_offset);
                out.push(operand);
            }
        }
    }
    out.sort_by_key(|operand| operand.id.clone());
    Ok(out)
}

pub(crate) fn parse_entity_selection_operand(
    bytes: &[u8],
    group: &DesignConstructionOperandGroup,
    group_member_ordinal: u32,
    header: &DesignRecordHeader,
) -> Option<DesignEntitySelectionOperand> {
    let start = usize::try_from(header.byte_offset).ok()?;
    if bytes.get(start + 11..start + 21)? != [0; 10]
        || bytes.get(start + 21) != Some(&1)
        || u32_at(bytes, start + 22)? != header.record_index.checked_add(3)?
        || bytes.get(start + 26..start + 32)? != [0; 6]
        || u32_at(bytes, start + 32)? != 1
    {
        return None;
    }
    let (asset_id, after_asset_id) = lp_utf16_bounded(bytes, start + 36, 1..=256)?;
    let (context_id, after_context_id) = lp_utf16_bounded(bytes, after_asset_id, 1..=256)?;
    if !is_guid_relaxed(&asset_id)
        || !is_guid_relaxed(&context_id)
        || u32_at(bytes, after_context_id)? != 2
        || bytes.get(after_context_id + 4..after_context_id + 8)? != [0; 4]
    {
        return None;
    }
    let paired_at = next_indexed_record_offset(bytes, after_context_id + 8)?;
    let nested_one_at = next_indexed_record_offset(bytes, paired_at + 11)?;
    let nested_two_at = next_indexed_record_offset(bytes, nested_one_at + 11)?;
    let identity_at = next_indexed_record_offset(bytes, nested_two_at + 11)?;
    let next_at = next_indexed_record_offset(bytes, identity_at + 11)?;
    let expected = [
        header.record_index,
        header.record_index.checked_add(1)?,
        header.record_index.checked_add(2)?,
        header.record_index.checked_add(3)?,
    ];
    for (offset, expected) in [paired_at, nested_one_at, nested_two_at, identity_at]
        .into_iter()
        .zip(expected)
    {
        let (_, after_tag) = lp_ascii_filtered(bytes, offset, 0..=2000, u8::is_ascii_graphic)?;
        if u32_at(bytes, after_tag)? != expected {
            return None;
        }
    }
    let (_, after_next_tag) = lp_ascii_filtered(bytes, next_at, 0..=2000, u8::is_ascii_graphic)?;
    let next_record_index = u32_at(bytes, after_next_tag)?;
    let (primary_identity_offset, secondary_identity_offset) =
        if bytes.get(identity_at + 11..identity_at + 29)? == [0; 18]
            && identity_at.checked_add(45)? == next_at
            && next_record_index == header.record_index.checked_add(4)?
        {
            (
                identity_at.checked_add(29)?,
                Some(identity_at.checked_add(37)?),
            )
        } else if bytes.get(identity_at + 11..identity_at + 21)? == [0; 10]
            && identity_at.checked_add(29)? == next_at
        {
            (identity_at.checked_add(21)?, None)
        } else {
            return None;
        };
    Some(DesignEntitySelectionOperand {
        id: String::new(),
        scope_record_index: group.scope_record_index,
        group_record_index: group.record_index,
        group_member_ordinal,
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        asset_id,
        asset_id_offset: u64::try_from(start + 40).ok()?,
        context_id,
        context_id_offset: u64::try_from(after_asset_id + 4).ok()?,
        identity_record_index: header.record_index.checked_add(3)?,
        identity_record_offset: u64::try_from(identity_at).ok()?,
        primary_identity: read_u64(bytes, primary_identity_offset)?,
        primary_identity_offset: u64::try_from(primary_identity_offset).ok()?,
        secondary_identity: secondary_identity_offset.and_then(|offset| read_u64(bytes, offset)),
        secondary_identity_offset: secondary_identity_offset
            .and_then(|offset| u64::try_from(offset).ok()),
        historical_edge_candidates: Vec::new(),
        historical_face_candidates: Vec::new(),
        resolved_edge_slot: None,
        next_record_index,
        next_byte_offset: u64::try_from(next_at).ok()?,
    })
}

/// Decode whole-body construction operands that contain one persistent body recipe.
pub fn decode_body_recipe_operands(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
    recipes: &[ConstructionRecipe],
) -> Result<Vec<DesignBodyRecipeOperand>, CodecError> {
    let mut headers_by_identity = HashMap::<_, Option<&DesignRecordHeader>>::new();
    for header in headers {
        let Some(stream) = native_stream(&header.id) else {
            continue;
        };
        headers_by_identity
            .entry((stream, header.record_index))
            .and_modify(|header| *header = None)
            .or_insert(Some(header));
    }
    let mut body_recipes_by_stream = HashMap::<_, Vec<&ConstructionRecipe>>::new();
    for recipe in recipes
        .iter()
        .filter(|recipe| recipe.kind == ConstructionRecipeKind::Body)
    {
        let Some(stream) = native_stream(&recipe.id) else {
            continue;
        };
        body_recipes_by_stream
            .entry(stream)
            .or_default()
            .push(recipe);
    }
    for stream_recipes in body_recipes_by_stream.values_mut() {
        stream_recipes.sort_by_key(|recipe| recipe.byte_offset);
    }
    let mut out = Vec::new();
    for group in groups {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(entry) = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && stream == ids::native_scope(&entry.name)
        }) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for (ordinal, record_index) in group.members.iter().copied().enumerate() {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let Some(Some(header)) = headers_by_identity.get(&(stream, record_index)) else {
                continue;
            };
            let Some(recipe) = unique_body_recipe(
                bytes,
                header,
                body_recipes_by_stream
                    .get(stream)
                    .map_or(&[], Vec::as_slice),
            ) else {
                continue;
            };
            if let Some(mut operand) =
                parse_body_recipe_operand(bytes, group, ordinal, header, recipe)
            {
                operand.id =
                    ids::native_design_body_recipe_operand_id(&entry.name, header.byte_offset);
                out.push(operand);
            }
        }
    }
    for scope in scopes
        .iter()
        .filter(|scope| scope.combine_operation.is_some())
    {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && stream == ids::native_scope(&entry.name)
        }) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(operation) = &scope.combine_operation else {
            continue;
        };
        for record_index in &operation.body_selection_record_indexes {
            let mut ordinals = scope
                .reference_members
                .iter()
                .enumerate()
                .filter(|(_, member)| *member == record_index)
                .filter_map(|(ordinal, _)| u32::try_from(ordinal).ok());
            let Some(scope_reference_ordinal) = ordinals.next() else {
                continue;
            };
            if ordinals.next().is_some() || scope_reference_ordinal.is_multiple_of(2) {
                continue;
            }
            let Some(Some(header)) = headers_by_identity.get(&(stream, *record_index)) else {
                continue;
            };
            let Some(recipe) = unique_body_recipe(
                bytes,
                header,
                body_recipes_by_stream
                    .get(stream)
                    .map_or(&[], Vec::as_slice),
            ) else {
                continue;
            };
            let owner = DesignBodyRecipeOperandOwner::ScopeReference {
                scope_reference_ordinal,
            };
            if let Some(mut operand) =
                parse_body_recipe_operand_frame(bytes, scope.record_index, owner, header, recipe)
            {
                operand.id =
                    ids::native_design_body_recipe_operand_id(&entry.name, header.byte_offset);
                out.push(operand);
            }
        }
    }
    out.sort_by_key(|operand| operand.id.clone());
    let mut owner_counts = HashMap::new();
    for operand in &out {
        *owner_counts.entry(operand.id.clone()).or_insert(0_u32) += 1;
    }
    out.retain(|operand| owner_counts.get(&operand.id) == Some(&1));
    Ok(out)
}

fn unique_body_recipe<'a>(
    bytes: &[u8],
    header: &DesignRecordHeader,
    recipes: &'a [&'a ConstructionRecipe],
) -> Option<&'a ConstructionRecipe> {
    let start = usize::try_from(header.byte_offset).ok()?;
    let prologue_end = body_recipe_prologue_end(bytes, start, header.record_index)?;
    let next_at = next_indexed_record_offset_with_index(
        bytes,
        prologue_end,
        header.record_index.checked_add(4)?,
    )?;
    let lower = u64::try_from(prologue_end.max(next_at.saturating_sub(1 << 16))).ok()?;
    let upper = u64::try_from(next_at).ok()?;
    let matching = &recipes[recipes.partition_point(|recipe| recipe.byte_offset < lower)
        ..recipes.partition_point(|recipe| recipe.byte_offset < upper)];
    let [recipe] = matching else {
        return None;
    };
    Some(recipe)
}

/// Offset past the four consecutively indexed records that open a body-recipe
/// operand, or `None` when the records after `start` do not carry
/// `record_index` through `record_index + 3` in order.
///
/// The prologue depends only on the operand header, so a caller weighing many
/// candidate recipes against one header resolves it once.
fn body_recipe_prologue_end(bytes: &[u8], start: usize, record_index: u32) -> Option<usize> {
    let mut search = start.checked_add(11)?;
    for expected in [
        record_index,
        record_index.checked_add(1)?,
        record_index.checked_add(2)?,
        record_index.checked_add(3)?,
    ] {
        let at = next_indexed_record_offset(bytes, search)?;
        if indexed_record_index(bytes, at)? != expected {
            return None;
        }
        search = at.checked_add(11)?;
    }
    Some(search)
}

/// Offset of the record carrying `record_index + 4` that closes a body-recipe
/// operand whose recipe sits at `recipe_at`, searching the 64 KiB after the
/// recipe. The recipe must follow the prologue that ends at `prologue_end`.
fn body_recipe_operand_end(
    bytes: &[u8],
    prologue_end: usize,
    record_index: u32,
    recipe_at: usize,
) -> Option<usize> {
    if recipe_at < prologue_end {
        return None;
    }
    let expected = record_index.checked_add(4)?;
    let search_limit = recipe_at.checked_add(1 << 16)?.min(bytes.len());
    let window = bytes.get(..search_limit)?;
    let mut search = recipe_at;
    while let Some(at) = next_indexed_record_offset(window, search) {
        if indexed_record_index(window, at) == Some(expected) {
            return Some(at);
        }
        search = at.checked_add(1)?;
    }
    None
}

pub(crate) fn parse_body_recipe_operand(
    bytes: &[u8],
    group: &DesignConstructionOperandGroup,
    group_member_ordinal: u32,
    header: &DesignRecordHeader,
    recipe: &ConstructionRecipe,
) -> Option<DesignBodyRecipeOperand> {
    parse_body_recipe_operand_frame(
        bytes,
        group.scope_record_index,
        DesignBodyRecipeOperandOwner::Group {
            group_record_index: group.record_index,
            group_member_ordinal,
        },
        header,
        recipe,
    )
}

fn parse_body_recipe_operand_frame(
    bytes: &[u8],
    scope_record_index: u32,
    owner: DesignBodyRecipeOperandOwner,
    header: &DesignRecordHeader,
    recipe: &ConstructionRecipe,
) -> Option<DesignBodyRecipeOperand> {
    let start = usize::try_from(header.byte_offset).ok()?;
    let recipe_at = usize::try_from(recipe.byte_offset).ok()?;
    let prologue_end = body_recipe_prologue_end(bytes, start, header.record_index)?;
    let next_at = body_recipe_operand_end(bytes, prologue_end, header.record_index, recipe_at)?;
    let reference_count = usize::try_from(u32_at(bytes, start + 21)?).ok()?;
    if start >= recipe_at
        || recipe_at >= next_at
        || bytes.get(start + 11..start + 21)? != [0; 10]
        || reference_count == 0
    {
        return None;
    }
    let mut cursor = start.checked_add(25)?;
    // Each reference consumes 12 bytes; a count the remaining bytes cannot
    // supply is corrupt and must not reach the allocator.
    if reference_count > bytes.len().saturating_sub(cursor) / 12 {
        return None;
    }
    let mut references = Vec::with_capacity(reference_count);
    for _ in 0..reference_count {
        references.push(DesignBodyRecipeReference {
            design_reference: read_u64(bytes, cursor)?,
            design_reference_offset: u64::try_from(cursor).ok()?,
            form: u32_at(bytes, cursor + 8)?,
            form_offset: u64::try_from(cursor + 8).ok()?,
            candidate_faces: Vec::new(),
            preceding_candidate_faces: Vec::new(),
            preceding_body_slots: Vec::new(),
        });
        cursor = cursor.checked_add(12)?;
    }
    if bytes.get(cursor) != Some(&1)
        || bytes.get(cursor + 9..cursor + 11)? != [0; 2]
        || u32_at(bytes, cursor + 11)? != 1
    {
        return None;
    }
    let nested_record_index = read_u64(bytes, cursor + 1)?;
    let asset_id_at = cursor.checked_add(15)?;
    let (asset_id, after_asset_id) = lp_utf16_bounded(bytes, asset_id_at, 1..=256)?;
    let (context_id, after_context_id) = lp_utf16_bounded(bytes, after_asset_id, 1..=256)?;
    if !is_guid_relaxed(&asset_id)
        || !is_guid_relaxed(&context_id)
        || u32_at(bytes, after_context_id)? != 2
        || bytes.get(after_context_id + 4..after_context_id + 8)? != [0; 4]
        || nested_record_index != u64::from(header.record_index.checked_add(3)?)
    {
        return None;
    }
    Some(DesignBodyRecipeOperand {
        id: String::new(),
        scope_record_index,
        owner,
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        asset_id,
        asset_id_offset: u64::try_from(asset_id_at + 4).ok()?,
        context_id,
        context_id_offset: u64::try_from(after_asset_id + 4).ok()?,
        references,
        nested_record_index,
        nested_record_index_offset: u64::try_from(cursor + 1).ok()?,
        recipe_id: recipe.id.clone(),
        resolved_face_slot: None,
        resolved_body_slot: None,
        next_record_index: header.record_index.checked_add(4)?,
        next_byte_offset: u64::try_from(next_at).ok()?,
    })
}

/// Join body-recipe Design references to solved persistent face tags.
pub fn bind_body_recipe_operand_candidates(
    operands: &mut [DesignBodyRecipeOperand],
    recipes: &[ConstructionRecipe],
    tags: &[PersistentSubentityTag],
) {
    use cadmpeg_ir::attributes::AttributeTarget;

    let mut recipes_by_id = HashMap::<_, Option<&ConstructionRecipe>>::new();
    for recipe in recipes {
        recipes_by_id
            .entry(recipe.id.as_str())
            .and_modify(|recipe| *recipe = None)
            .or_insert(Some(recipe));
    }
    for operand in operands {
        let tag_selector = recipes_by_id
            .get(operand.recipe_id.as_str())
            .and_then(|recipe| *recipe)
            .and_then(|recipe| recipe.design_selector)
            .map(|selector| i64::from(selector.value));
        for reference in &mut operand.references {
            reference.candidate_faces.clear();
            let Ok(design_reference) = i64::try_from(reference.design_reference) else {
                continue;
            };
            reference.candidate_faces = tags
                .iter()
                .filter(|tag| {
                    crate::ids::same_native_occurrence(&tag.id, &operand.id)
                        && tag.design_references.contains(&design_reference)
                        && (reference.form != 3 || tag_selector == Some(tag.selector))
                })
                .filter_map(|tag| match &tag.target {
                    AttributeTarget::Face(face) => Some(face.clone()),
                    _ => None,
                })
                .collect();
            reference
                .candidate_faces
                .sort_by(|left, right| left.0.cmp(&right.0));
            reference.candidate_faces.dedup();
        }
    }
}

/// Resolve selection-member local identities against persistent point and
/// curve identities owned by the Extrude scope's selected Sketch.
pub fn bind_extrude_selection_geometry(
    members: &mut [DesignExtrudeSelectionMember],
    groups: &[DesignExtrudeSelectionGroup],
    scopes: &[DesignParameterScope],
    points: &[SketchPoint],
    curves: &[SketchCurveIdentity],
) {
    let selected_sketches = groups
        .iter()
        .filter_map(|group| {
            let stream = native_stream(&group.id)?;
            let scope = scopes.iter().find(|scope| {
                native_stream(&scope.id) == Some(stream)
                    && scope.record_index == group.scope_record_index
            })?;
            Some((
                (stream, group.record_index),
                scope.extrude_profile.as_ref()?.entity_suffix,
            ))
        })
        .collect::<HashMap<_, _>>();
    for member in members {
        let Some(stream) = native_stream(&member.id) else {
            continue;
        };
        let Some(entity_suffix) = selected_sketches.get(&(stream, member.group_record_index))
        else {
            continue;
        };
        let Ok(entity_suffix) = u32::try_from(*entity_suffix) else {
            continue;
        };
        let point_operands = points.iter().filter_map(|point| {
            (native_stream(&point.id) == Some(stream)
                && point.owner_reference == Some(entity_suffix)
                && point.persistent_id == member.local_id)
                .then_some(SketchRelationOperand::Point {
                    record_index: point.record_index,
                    persistent_id: point.persistent_id,
                })
        });
        let curve_operands = curves.iter().filter_map(|curve| {
            (native_stream(&curve.id) == Some(stream)
                && curve.owner_reference == Some(entity_suffix)
                && (curve.primary_id == member.local_id
                    || curve.secondary_id != 0 && curve.secondary_id == member.local_id))
                .then_some(SketchRelationOperand::Curve {
                    record_index: curve.record_index,
                    primary_id: curve.primary_id,
                    secondary_id: curve.secondary_id,
                })
        });
        let matches = point_operands.chain(curve_operands).collect::<Vec<_>>();
        if let [resolved] = matches.as_slice() {
            member.resolved_geometry = Some(resolved.clone());
        }
    }
}

/// Bind selection members to construction-operand identity chains that
/// terminate at the same fixed persistent identity record.
pub fn bind_extrude_selection_identities(
    members: &mut [DesignExtrudeSelectionMember],
    identities: &[DesignConstructionOperandIdentity],
) {
    for member in members {
        let Some(stream) = native_stream(&member.id) else {
            continue;
        };
        let mut matches = identities
            .iter()
            .filter(|identity| {
                native_stream(&identity.id) == Some(stream)
                    && identity.following_record_index == member.record_index
                    && identity.following_byte_offset == member.byte_offset
                    && identity
                        .persistent_identity
                        .as_ref()
                        .is_some_and(|persistent| {
                            persistent.local_id == member.local_id
                                && persistent.asset_id == member.asset_id
                                && persistent.context_id == member.context_id
                        })
            })
            .collect::<Vec<_>>();
        matches.sort_by_key(|identity| identity.wrapper_byte_offsets.first().copied());
        member.operand_identity_ids = matches
            .into_iter()
            .map(|identity| identity.id.clone())
            .collect();
    }
}

pub(crate) fn parse_extrude_selection_member(
    bytes: &[u8],
    group: &DesignExtrudeSelectionGroup,
    group_member_ordinal: u32,
    header: &DesignRecordHeader,
) -> Option<DesignExtrudeSelectionMember> {
    let start = usize::try_from(header.byte_offset).ok()?;
    let member = parse_extrude_identity_member(bytes, start)?;
    Some(DesignExtrudeSelectionMember {
        id: String::new(),
        group_record_index: group.record_index,
        group_member_ordinal,
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        local_id: member.local_id,
        local_id_offset: member.local_id_offset,
        asset_id: member.asset_id,
        asset_id_offset: member.asset_id_offset,
        context_id: member.context_id,
        context_id_offset: member.context_id_offset,
        tail_slot_present: member.tail_slot_present,
        tail_slot_offset: member.tail_slot_offset,
        resolved_geometry: None,
        operand_identity_ids: Vec::new(),
        historical_entity_kind: None,
        historical_entity_ref: None,
        historical_state_ids: Vec::new(),
        next_record_index: member.next_record_index,
        next_byte_offset: member.next_byte_offset,
    })
}

struct ParsedExtrudeIdentityMember {
    local_id: u64,
    local_id_offset: u64,
    asset_id: String,
    asset_id_offset: u64,
    context_id: String,
    context_id_offset: u64,
    tail_slot_present: bool,
    tail_slot_offset: u64,
    next_record_index: u32,
    next_byte_offset: u64,
}

fn parse_extrude_identity_member(
    bytes: &[u8],
    start: usize,
) -> Option<ParsedExtrudeIdentityMember> {
    if bytes.get(start + 11..start + 21)? != [0; 10] {
        return None;
    }
    let local_id = read_u64(bytes, start + 21)?;
    let (asset_id, after_asset_id) = lp_utf16_bounded(bytes, start + 29, 1..=256)?;
    let (context_id, after_context_id) = lp_utf16_bounded(bytes, after_asset_id, 1..=256)?;
    let tail_slot_offset = after_context_id.checked_add(4)?;
    let tail_slot_present = match bytes.get(tail_slot_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    if !is_guid_relaxed(&asset_id)
        || !is_guid_relaxed(&context_id)
        || u32_at(bytes, after_context_id)? != 2
    {
        return None;
    }
    let next_at = if u32_at(bytes, tail_slot_offset + 1) == Some(0)
        && after_context_id.checked_add(9)? == start.checked_add(190)?
    {
        start.checked_add(190)?
    } else if bytes.get(tail_slot_offset + 1..tail_slot_offset + 4)? == [0; 3] {
        let mut cursor = tail_slot_offset.checked_add(4)?;
        let (next_record_index, _) = take_record_reference(bytes, &mut cursor)?;
        let next_at = cursor;
        let (_, after_next_tag) =
            lp_ascii_filtered(bytes, next_at, 0..=2000, u8::is_ascii_graphic)?;
        if u32_at(bytes, after_next_tag)? != next_record_index {
            return None;
        }
        next_at
    } else {
        return None;
    };
    let (_, after_next_tag) = lp_ascii_filtered(bytes, next_at, 0..=2000, u8::is_ascii_graphic)?;
    Some(ParsedExtrudeIdentityMember {
        local_id,
        local_id_offset: u64::try_from(start + 21).ok()?,
        asset_id,
        asset_id_offset: u64::try_from(start + 33).ok()?,
        context_id,
        context_id_offset: u64::try_from(after_asset_id + 4).ok()?,
        tail_slot_present,
        tail_slot_offset: u64::try_from(tail_slot_offset).ok()?,
        next_record_index: u32_at(bytes, after_next_tag)?,
        next_byte_offset: u64::try_from(next_at).ok()?,
    })
}

pub(crate) struct ParsedEdgeIdentityMember {
    pub(crate) compact_layout: bool,
    pub(crate) local_id: u64,
    pub(crate) local_id_offset: u64,
    pub(crate) asset_id: String,
    pub(crate) asset_id_offset: u64,
    pub(crate) context_id: String,
    pub(crate) context_id_offset: u64,
}

pub(crate) fn parse_edge_identity_member(
    bytes: &[u8],
    start: usize,
) -> Option<ParsedEdgeIdentityMember> {
    let (compact_layout, marker_offset) = if bytes.get(start + 11..start + 23) == Some(&[0; 12]) {
        (false, 23)
    } else if bytes.get(start + 11..start + 22) == Some(&[0; 11]) {
        (true, 22)
    } else {
        return None;
    };
    let local_id_offset = marker_offset + 1;
    let asset_offset = marker_offset + 15;
    if bytes.get(start + marker_offset) != Some(&1)
        || bytes.get(start + marker_offset + 5..start + marker_offset + 11)? != [0; 6]
        || u32_at(bytes, start + marker_offset + 11)? != 1
    {
        return None;
    }
    let local_id = u64::from(u32_at(bytes, start + local_id_offset)?);
    let (asset_id, after_asset_id) = lp_utf16_bounded(bytes, start + asset_offset, 1..=256)?;
    let (context_id, _after_context_id) = lp_utf16_bounded(bytes, after_asset_id, 1..=256)?;
    if !is_guid_relaxed(&asset_id) || !is_guid_relaxed(&context_id) {
        return None;
    }
    Some(ParsedEdgeIdentityMember {
        compact_layout,
        local_id,
        local_id_offset: u64::try_from(start + local_id_offset).ok()?,
        asset_id,
        asset_id_offset: u64::try_from(start + asset_offset + 4).ok()?,
        context_id,
        context_id_offset: u64::try_from(after_asset_id + 4).ok()?,
    })
}

pub(crate) fn parse_sketch_profile(
    bytes: &[u8],
    stream: &str,
    scope_reference_ordinal: u32,
    header: &DesignRecordHeader,
    entities: &[DesignEntityHeader],
) -> Option<DesignSketchProfileOperand> {
    let start = usize::try_from(header.byte_offset).ok()?;
    if bytes.get(start + 11..start + 21)? != [0; 10]
        || bytes.get(start + 21) != Some(&1)
        || u32_at(bytes, start + 22)? != header.record_index.checked_add(3)?
        || bytes.get(start + 26..start + 32)? != [0; 6]
        || u32_at(bytes, start + 32)? != 1
    {
        return None;
    }
    let (asset_id, after_asset_id) = lp_utf16_bounded(bytes, start + 36, 1..=256)?;
    if !is_guid_relaxed(&asset_id) {
        return None;
    }
    let (entity_suffix_text, after_entity_suffix) =
        lp_utf16_bounded(bytes, after_asset_id, 1..=256)?;
    let entity_suffix = entity_suffix_text.parse::<u64>().ok()?;
    let paired_at = next_indexed_record_offset(bytes, start + 11)?;
    let (paired_class_tag, after_paired_tag) =
        lp_ascii_filtered(bytes, paired_at, 0..=2000, u8::is_ascii_graphic)?;
    let tail_length = paired_at.checked_sub(after_entity_suffix)?;
    if u32_at(bytes, after_paired_tag)? != header.record_index
        || !matches!(tail_length, 89 | 93 | 94)
    {
        return None;
    }
    if matches!(tail_length, 89 | 93) {
        let tail = after_entity_suffix;
        let (nested_two_at, nested_one_at, scope_at) = if tail_length == 89 {
            (53, 66, 78)
        } else {
            (57, 70, 82)
        };
        if bytes.get(tail..tail + 8) != Some(&[1, 0, 0, 0, 0, 0, 0, 0])
            || u32_at(bytes, tail + 8) != Some(1)
            || marked_record_reference(bytes, tail + nested_two_at)
                != header.record_index.checked_add(2)
            || bytes.get(tail + nested_one_at - 2..tail + nested_one_at) != Some(&[0; 2])
            || marked_record_reference(bytes, tail + nested_one_at)
                != header.record_index.checked_add(1)
            || bytes.get(tail + scope_at - 1) != Some(&0)
            || marked_record_reference(bytes, tail + scope_at).is_none()
            || u32_at(bytes, tail + 41) == Some(0)
            || (tail_length == 93 && u32_at(bytes, tail + 41) != u32_at(bytes, tail + 53))
        {
            return None;
        }
    }
    let matches = entities
        .iter()
        .filter(|entity| {
            native_stream(&entity.id) == Some(stream)
                && entity.in_sketch_module()
                && entity.entity_suffix == entity_suffix
        })
        .collect::<Vec<_>>();
    let [entity] = matches.as_slice() else {
        return None;
    };
    Some(DesignSketchProfileOperand {
        scope_reference_ordinal,
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        asset_id,
        asset_id_offset: u64::try_from(start + 40).ok()?,
        entity_id: entity.entity_id.clone(),
        entity_suffix,
        entity_reference_offset: u64::try_from(after_asset_id + 4).ok()?,
        paired_class_tag,
        paired_byte_offset: u64::try_from(paired_at).ok()?,
    })
}

fn marked_record_reference(bytes: &[u8], at: usize) -> Option<u32> {
    if bytes.get(at) != Some(&1) || bytes.get(at + 5..at + 11)? != [0; 6] {
        return None;
    }
    u32_at(bytes, at + 1)
}

pub(crate) fn parse_edge_operand(
    bytes: &[u8],
    scope: &DesignParameterScope,
    scope_reference_ordinal: u32,
    header: &DesignRecordHeader,
    recipes: &[ConstructionRecipe],
) -> Option<DesignEdgeOperand> {
    let start = usize::try_from(header.byte_offset).ok()?;
    let mut offsets = Vec::with_capacity(5);
    let mut position = start.checked_add(11)?;
    for record_index in (0..4).map(|delta| header.record_index.checked_add(delta)) {
        let offset = next_indexed_record_offset_with_index(bytes, position, record_index?)?;
        offsets.push(offset);
        position = offset.checked_add(11)?;
    }
    offsets.push(next_indexed_record_offset_with_index(
        bytes,
        position,
        header.record_index.checked_add(4)?,
    )?);
    let mut indexed = Vec::with_capacity(offsets.len());
    for offset in &offsets {
        let (class_tag, after_tag) =
            lp_ascii_filtered(bytes, *offset, 0..=2000, u8::is_ascii_graphic)?;
        indexed.push((class_tag, u32_at(bytes, after_tag)?));
    }
    let next_one = header.record_index.checked_add(1)?;
    let next_two = header.record_index.checked_add(2)?;
    let recipe_record_index = header.record_index.checked_add(3)?;
    let next_record_index = header.record_index.checked_add(4)?;
    if indexed[0].1 != header.record_index
        || indexed[1].1 != next_one
        || indexed[2].1 != next_two
        || indexed[3].1 != recipe_record_index
        || indexed[4].1 != next_record_index
    {
        return None;
    }
    let stream = native_stream(&scope.id)?;
    let recipe_start = u64::try_from(offsets[3]).ok()?;
    let next_byte_offset = u64::try_from(offsets[4]).ok()?;
    let matches = recipes
        .iter()
        .filter(|recipe| {
            native_stream(&recipe.id) == Some(stream)
                && recipe.kind == ConstructionRecipeKind::Edge
                && recipe.byte_offset > recipe_start
                && recipe.byte_offset < next_byte_offset
        })
        .collect::<Vec<_>>();
    let [recipe] = matches.as_slice() else {
        return None;
    };
    let (recipe_prefix_at, recipe_prefix_bytes) = recipe_record_prefix(
        bytes,
        offsets[3],
        usize::try_from(recipe.byte_offset).ok()?,
        b"edge_recipe_data".len(),
    )?;
    let recipe_references =
        decode_recipe_references(&recipe_prefix_bytes, u64::try_from(recipe_prefix_at).ok()?);
    let recipe_program_at = usize::try_from(recipe.byte_offset)
        .ok()?
        .checked_add(b"edge_recipe_data".len())?;
    let recipe_program_bytes =
        bytes.get(recipe_program_at..usize::try_from(next_byte_offset).ok()?)?;
    if recipe_program_bytes.is_empty()
        || recipe_program_bytes.len() % 4 != 0
        || recipe_program_bytes.len() > 64 * 1024
    {
        return None;
    }
    let recipe_program = recipe_program_bytes
        .chunks_exact(4)
        .map(|raw| {
            i32::from_le_bytes(
                raw.try_into()
                    .expect("invariant: chunks_exact(4) yields four-byte slices"),
            )
        })
        .collect::<Vec<_>>();
    let recipe_structure = edge_recipe_structure(&recipe_program);
    let local_topology_references = recipe_structure.as_ref().and_then(|structure| {
        edge_recipe_local_topology_references(structure, recipe_references.len())
    });
    Some(DesignEdgeOperand {
        id: ids::native_design_edge_operand_id(
            stream.strip_prefix(ids::SCHEME_PREFIX).unwrap_or(stream),
            header.byte_offset,
        ),
        scope_record_index: scope.record_index,
        scope_reference_ordinal,
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        paired_byte_offset: u64::try_from(offsets[0]).ok()?,
        paired_class_tag: indexed[0].0.clone(),
        recipe_record_index,
        recipe_record_byte_offset: recipe_start,
        recipe_id: recipe.id.clone(),
        recipe_prefix_offset: u64::try_from(recipe_prefix_at).ok()?,
        recipe_prefix_bytes,
        recipe_references,
        recipe_program_offset: u64::try_from(recipe_program_at).ok()?,
        recipe_program,
        recipe_structure,
        local_topology_references,
        candidate_faces: Vec::new(),
        result_candidate_faces: Vec::new(),
        result_boundary_edge_slots: Vec::new(),
        preceding_candidate_faces: Vec::new(),
        terminal_candidate_faces: Vec::new(),
        changed_candidate_faces: Vec::new(),
        preceding_boundary_edge_slots: Vec::new(),
        terminal_boundary_edge_slots: Vec::new(),
        changed_boundary_edge_slots: Vec::new(),
        deleted_boundary_edge_slots: Vec::new(),
        updated_boundary_edge_slots: Vec::new(),
        treatment_radius_candidates: Vec::new(),
        changed_boundary_edge_contexts: Vec::new(),
        terminal_boundary_edge_contexts: Vec::new(),
        terminal_reference_edge_slots: Vec::new(),
        recipe_reference_contexts: Vec::new(),
        recipe_selectors: Vec::new(),
        recipe_state_id: None,
        resolved_edge_slot: None,
        resolved_axis_origin: None,
        resolved_axis_direction: None,
        next_record_index: indexed[4].1,
        next_byte_offset,
    })
}

pub(crate) fn edge_recipe_structure(
    program: &[i32],
) -> Option<crate::records::DesignEdgeRecipeStructure> {
    edge_recipe_structure_tail(program.get(7..)?)
}

pub(crate) fn edge_recipe_local_topology_references(
    structure: &crate::records::DesignEdgeRecipeStructure,
    reference_count: usize,
) -> Option<Vec<std::num::NonZeroU32>> {
    topology_recipe_references(
        structure.sides.iter().flat_map(|side| {
            std::iter::once(side.header_value).chain(side.scalars.iter().copied())
        }),
        reference_count,
    )
}

fn edge_recipe_structure_tail(
    program: &[i32],
) -> Option<crate::records::DesignEdgeRecipeStructure> {
    let (&root, mut remaining) = program.split_first()?;
    let side_count = usize::try_from(root).ok()?;
    if side_count == 0 {
        return None;
    }
    remaining = recipe_delimiter(remaining)?;
    let structures = edge_recipe_side_sequences(remaining, side_count)
        .into_iter()
        .filter_map(|(sides, tail)| {
            matches!(tail, [] | [-1 | 0])
                .then_some(crate::records::DesignEdgeRecipeStructure { root, sides })
        })
        .collect::<Vec<_>>();
    let [structure] = structures.as_slice() else {
        return None;
    };
    Some(structure.clone())
}

fn edge_recipe_side_sequences(
    words: &[i32],
    side_count: usize,
) -> Vec<(Vec<DesignTopologyRecipeSide>, &[i32])> {
    if side_count == 0 {
        return vec![(Vec::new(), words)];
    }
    let mut out = Vec::new();
    for (side, tail) in edge_recipe_counted_side_candidates(words) {
        let remaining = if side_count == 1 {
            tail
        } else if let Some(remaining) = recipe_delimiter(tail) {
            remaining
        } else {
            continue;
        };
        for (mut following, tail) in edge_recipe_side_sequences(remaining, side_count - 1) {
            following.insert(0, side.clone());
            out.push((following, tail));
        }
    }
    out
}

fn recipe_delimiter(words: &[i32]) -> Option<&[i32]> {
    matches!(words.first(), Some(-1 | 0)).then(|| &words[1..])
}

fn edge_recipe_counted_side_candidates(words: &[i32]) -> Vec<(DesignTopologyRecipeSide, &[i32])> {
    let Some(field_count) = words
        .first()
        .and_then(|word| u32::try_from(*word).ok())
        .and_then(std::num::NonZeroU32::new)
    else {
        return Vec::new();
    };
    if field_count.get() < 2 {
        return Vec::new();
    }
    let Some(scalar_count) = usize::try_from(field_count.get())
        .ok()
        .and_then(|count| count.checked_sub(1))
    else {
        return Vec::new();
    };
    let Some(&header_value) = words.get(1) else {
        return Vec::new();
    };
    let Some(mut remaining) = words.get(2..).and_then(recipe_delimiter) else {
        return Vec::new();
    };
    // Each scalar consumes at least one remaining word; a larger count is
    // corrupt and must not reach the allocator.
    if scalar_count > remaining.len() {
        return Vec::new();
    }
    let mut scalars = Vec::with_capacity(scalar_count);
    for _ in 0..scalar_count {
        let Some((&scalar, tail)) = remaining.split_first() else {
            return Vec::new();
        };
        scalars.push(scalar);
        let Some(tail) = recipe_delimiter(tail) else {
            return Vec::new();
        };
        remaining = tail;
    }
    (0..remaining.len())
        .filter(|entry_count_at| {
            *entry_count_at == 1 && remaining.first() == Some(&0)
                || *entry_count_at > 0 && remaining.get(entry_count_at - 1) == Some(&-1)
        })
        .filter_map(|entry_count_at| {
            let payload_entry_count = u32::try_from(*remaining.get(entry_count_at)?).ok()?;
            if entry_count_at != 1 && payload_entry_count == 0 {
                return None;
            }
            let payload_len = usize::try_from(payload_entry_count).ok()?.checked_mul(8)?;
            let entries_at = entry_count_at.checked_add(1)?;
            let entries_end = entries_at.checked_add(payload_len)?;
            let entries = edge_recipe_entries(remaining.get(entries_at..entries_end)?)?;
            Some((
                DesignTopologyRecipeSide {
                    field_count,
                    header_value,
                    scalars: scalars.clone(),
                    payload_prefix: remaining[..entry_count_at].to_vec(),
                    payload_entry_count,
                    entries,
                },
                remaining.get(entries_end..)?,
            ))
        })
        .collect()
}

pub(crate) fn face_recipe_structure(
    program: &[i32],
) -> Option<crate::records::DesignFaceRecipeStructure> {
    let (&root, remaining) = program.split_first()?;
    let (&first_prelude, remaining) = recipe_delimiter(remaining)?.split_first()?;
    let (&second_prelude, remaining) = recipe_delimiter(remaining)?.split_first()?;
    let remaining = recipe_delimiter(remaining)?;
    let structures = edge_recipe_side_sequences(remaining, 2)
        .into_iter()
        .filter_map(|(sides, tail)| matches!(tail, [] | [-1 | 0]).then(|| sides.try_into().ok())?)
        .collect::<Vec<[_; 2]>>();
    let [sides] = structures.as_slice() else {
        return None;
    };
    Some(crate::records::DesignFaceRecipeStructure {
        root,
        prelude: [first_prelude, second_prelude],
        sides: sides.clone(),
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FaceRecipeProgramKind {
    Terminal,
    Counted { header_value: usize },
}

pub(crate) fn face_recipe_program_kind(program: &[i32]) -> Option<FaceRecipeProgramKind> {
    if program == [0, -1] {
        return Some(FaceRecipeProgramKind::Terminal);
    }
    if !matches!(program.get(0..2), Some([0, -1 | 0])) {
        return None;
    }
    let header_value = usize::try_from(*program.get(2)?).ok()?;
    (header_value > 0 && header_value <= 100_000)
        .then_some(FaceRecipeProgramKind::Counted { header_value })
}

fn topology_recipe_references(
    words: impl IntoIterator<Item = i32>,
    reference_count: usize,
) -> Option<Vec<std::num::NonZeroU32>> {
    words
        .into_iter()
        .filter(|word| *word != 0)
        .map(|word| {
            let ordinal = std::num::NonZeroU32::new(u32::try_from(word).ok()?)?;
            (usize::try_from(ordinal.get()).ok()? <= reference_count).then_some(ordinal)
        })
        .collect()
}

pub(crate) fn edge_recipe_entries(words: &[i32]) -> Option<Vec<DesignTopologyRecipeEntry>> {
    let entries = words
        .chunks_exact(8)
        .map(|entry| {
            let selector = entry[0];
            if selector < 0 {
                return None;
            }
            let boundary_edge_count = std::num::NonZeroU32::new(u32::try_from(entry[1]).ok()?)?;
            let topology_triplets = [
                edge_recipe_topology_triplet(&entry[2..5], boundary_edge_count)?,
                edge_recipe_topology_triplet(&entry[5..8], boundary_edge_count)?,
            ];
            topology_triplets
                .iter()
                .all(|triplet| triplet.outer.get() <= boundary_edge_count.get())
                .then_some(DesignTopologyRecipeEntry {
                    selector,
                    boundary_edge_count,
                    common_incident_edge_ordinal: topology_triplets[0]
                        .incident_edge_ordinal
                        .filter(|ordinal| {
                            topology_triplets[1].incident_edge_ordinal == Some(*ordinal)
                        }),
                    topology_triplets,
                })
        })
        .collect::<Option<Vec<_>>>()?;
    entries
        .windows(2)
        .all(|pair| pair[0].selector < pair[1].selector)
        .then_some(entries)
}

fn edge_recipe_topology_triplet(
    words: &[i32],
    boundary_edge_count: std::num::NonZeroU32,
) -> Option<DesignTopologyRecipeTriplet> {
    let [outer, middle, repeated_outer] = words else {
        return None;
    };
    if outer != repeated_outer {
        return None;
    }
    let outer = std::num::NonZeroU32::new(u32::try_from(*outer).ok()?)?;
    let middle = u32::try_from(*middle).ok()?;
    let vertex_ordinal = outer.get().checked_sub(1)?;
    let incident = if middle == outer.get() {
        Some((
            crate::records::DesignTopologyIncidentSide::Following,
            vertex_ordinal,
        ))
    } else if middle.checked_add(1) == Some(outer.get()) {
        Some((
            crate::records::DesignTopologyIncidentSide::Preceding,
            vertex_ordinal
                .checked_add(boundary_edge_count.get())?
                .checked_sub(1)?
                % boundary_edge_count.get(),
        ))
    } else {
        None
    };
    Some(DesignTopologyRecipeTriplet {
        outer,
        middle,
        vertex_ordinal,
        incident_edge_ordinal: incident.map(|(_, ordinal)| ordinal),
        incident_side: incident.map(|(side, _)| side),
    })
}

pub(crate) fn parse_face_operand(
    bytes: &[u8],
    scope: &DesignParameterScope,
    scope_reference_ordinal: u32,
    group_ownership: Option<(u32, u32)>,
    next_byte_offset: Option<u64>,
    header: &DesignRecordHeader,
    recipes: &[ConstructionRecipe],
) -> Option<DesignFaceOperand> {
    let start = usize::try_from(header.byte_offset).ok()?;
    let mut offsets = Vec::with_capacity(5);
    let mut position = start.checked_add(11)?;
    for record_index in (0..4).map(|delta| header.record_index.checked_add(delta)) {
        let offset = next_indexed_record_offset_with_index(bytes, position, record_index?)?;
        offsets.push(offset);
        position = offset.checked_add(11)?;
    }
    let immediate_next = next_indexed_record_offset(bytes, position)?;
    if let Some(limit) = next_byte_offset {
        if immediate_next > usize::try_from(limit).ok()? {
            return None;
        }
    }
    offsets.push(immediate_next);
    let mut indexed = Vec::with_capacity(offsets.len());
    for offset in &offsets {
        let (class_tag, after_tag) =
            lp_ascii_filtered(bytes, *offset, 0..=2000, u8::is_ascii_graphic)?;
        indexed.push((class_tag, u32_at(bytes, after_tag)?));
    }
    let recipe_record_index = header.record_index.checked_add(3)?;
    if indexed[0].1 != header.record_index
        || indexed[1].1 != header.record_index.checked_add(1)?
        || indexed[2].1 != header.record_index.checked_add(2)?
        || indexed[3].1 != recipe_record_index
    {
        return None;
    }
    let stream = native_stream(&scope.id)?;
    let recipe_start = u64::try_from(offsets[3]).ok()?;
    let next_byte_offset = u64::try_from(offsets[4]).ok()?;
    let matches = recipes
        .iter()
        .filter(|recipe| {
            native_stream(&recipe.id) == Some(stream)
                && matches!(
                    recipe.kind,
                    ConstructionRecipeKind::Face | ConstructionRecipeKind::BoundedFace
                )
                && recipe.byte_offset > recipe_start
                && recipe.byte_offset < next_byte_offset
        })
        .collect::<Vec<_>>();
    let [recipe] = matches.as_slice() else {
        return None;
    };
    let family_name_len = match recipe.kind {
        ConstructionRecipeKind::Face => b"face_recipe_data".len(),
        ConstructionRecipeKind::BoundedFace => b"bounded_face_recipe_data".len(),
        _ => return None,
    };
    let (recipe_prefix_at, recipe_prefix_bytes) = recipe_record_prefix(
        bytes,
        offsets[3],
        usize::try_from(recipe.byte_offset).ok()?,
        family_name_len,
    )?;
    let recipe_references =
        decode_recipe_references(&recipe_prefix_bytes, u64::try_from(recipe_prefix_at).ok()?);
    let recipe_program_at = usize::try_from(recipe.byte_offset)
        .ok()?
        .checked_add(family_name_len)?;
    let recipe_program_bytes =
        bytes.get(recipe_program_at..usize::try_from(next_byte_offset).ok()?)?;
    if recipe_program_bytes.is_empty()
        || recipe_program_bytes.len() % 4 != 0
        || recipe_program_bytes.len() > 64 * 1024
    {
        return None;
    }
    let recipe_program = recipe_program_bytes
        .chunks_exact(4)
        .map(|raw| {
            i32::from_le_bytes(
                raw.try_into()
                    .expect("invariant: chunks_exact(4) yields four-byte slices"),
            )
        })
        .collect::<Vec<_>>();
    let program_kind = face_recipe_program_kind(&recipe_program)?;
    let recipe_program_offset = u64::try_from(recipe_program_at).ok()?;
    let recipe_node_indices = recipe_program
        .windows(3)
        .enumerate()
        .filter(|(_, values)| *values == [-1, -1, 2])
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if !recipe_node_indices.is_empty() && recipe_node_indices.first() != Some(&3) {
        return None;
    }
    if program_kind == FaceRecipeProgramKind::Terminal && !recipe_node_indices.is_empty() {
        return None;
    }
    let recipe_node_offsets = recipe_node_indices
        .iter()
        .map(|index| u64::try_from(recipe_program_at.checked_add(index.checked_mul(4)?)?).ok())
        .collect::<Option<Vec<_>>>()?;
    let recipe_nodes = recipe_node_indices
        .iter()
        .copied()
        .zip(
            recipe_node_indices
                .iter()
                .copied()
                .skip(1)
                .chain(std::iter::once(recipe_program.len())),
        )
        .map(|(start, end)| {
            let program = recipe_program.get(start..end)?.to_vec();
            let recipe_structure = program.get(3..).and_then(face_recipe_structure);
            Some(crate::records::DesignFaceRecipeNode {
                byte_offset: u64::try_from(recipe_program_at.checked_add(start.checked_mul(4)?)?)
                    .ok()?,
                end_byte_offset: u64::try_from(recipe_program_at.checked_add(end.checked_mul(4)?)?)
                    .ok()?,
                recipe_structure,
                program,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(DesignFaceOperand {
        id: ids::native_design_face_operand_id(
            stream.strip_prefix(ids::SCHEME_PREFIX).unwrap_or(stream),
            header.byte_offset,
        ),
        scope_record_index: scope.record_index,
        scope_reference_ordinal,
        group_record_index: group_ownership.map(|ownership| ownership.0),
        group_member_ordinal: group_ownership.map(|ownership| ownership.1),
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        paired_byte_offset: u64::try_from(offsets[0]).ok()?,
        paired_class_tag: indexed[0].0.clone(),
        recipe_record_index,
        recipe_record_byte_offset: recipe_start,
        recipe_id: recipe.id.clone(),
        recipe_prefix_offset: u64::try_from(recipe_prefix_at).ok()?,
        recipe_prefix_bytes,
        recipe_references,
        recipe_kind: recipe.kind,
        recipe_program_offset,
        recipe_program,
        recipe_node_offsets,
        recipe_nodes,
        candidate_faces: Vec::new(),
        unreferenced_candidate_faces: Vec::new(),
        alternate_selector_candidate_faces: Vec::new(),
        preceding_candidate_faces: Vec::new(),
        changed_candidate_faces: Vec::new(),
        historical_support_contexts: Vec::new(),
        resolved_face_slots: Vec::new(),
        next_record_index: indexed[4].1,
        next_byte_offset,
    })
}

pub(crate) fn has_typed_edge_treatment_group(kind: &str) -> bool {
    matches!(
        design_feature_family(kind),
        Some(DesignFeatureFamily::Fillet | DesignFeatureFamily::Chamfer)
    )
}
