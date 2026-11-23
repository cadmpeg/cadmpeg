// SPDX-License-Identifier: Apache-2.0
//! Registered face operand frames and face operand decode.

use super::edge_operands::is_surface_untrim_face_group;
use super::face_operand_frames::parse_face_operand;
use crate::container::ContainerScan;
use crate::design::decode::meta::{unambiguous_design_primary_frames, DesignPrimaryFrame};
use crate::design::decode::sketch::{cached_borrowed_record_offsets, IndexedRecordOffsets};
use crate::design::decode::text::design_record_id;
use crate::design::{design_feature_family, DesignFeatureFamily};
use crate::ids::native_stream;
use crate::records::decal::DesignRecordHeader;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::recipes::{ConstructionRecipe, ConstructionRecipeKind};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::{DesignExtrudeOperandRole, DesignOperandRole};
use crate::records::topology::face::DesignFaceOperand;
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use std::collections::{HashMap, HashSet};

pub(crate) struct RegisteredFaceOperandFrame {
    pub(super) header: DesignRecordHeader,
    pub(super) offsets: [usize; 5],
    pub(super) next_record_index: u32,
    pub(super) recipe_frame: Option<crate::records::topology::face::RegisteredFaceRecipeFrame>,
}

pub(super) fn registered_face_operand_frame(
    stream: &str,
    frames: &[DesignPrimaryFrame<'_>],
    meta: &crate::metastream::MetaStream,
    ordinal: usize,
) -> Option<RegisteredFaceOperandFrame> {
    let chain = frames.get(ordinal..ordinal.checked_add(4)?)?;
    let [primary, first, second, recipe] = chain else {
        return None;
    };
    let record_index = u32::try_from(primary.entity_id).ok()?;
    if primary.member_end >= primary.end {
        return None;
    }
    for pair in chain.windows(2) {
        let previous = u32::try_from(pair[0].entity_id).ok()?;
        let next = u32::try_from(pair[1].entity_id).ok()?;
        if pair[0].end != pair[1].start || next != previous.checked_add(1)? {
            return None;
        }
    }
    let (end, next_record_index, recipe_frame) = if recipe.member_end < recipe.end {
        let record_index = u32::try_from(recipe.entity_id).ok()?;
        (
            recipe.member_end,
            record_index,
            Some(crate::records::topology::face::RegisteredFaceRecipeFrame {
                record_index,
                primary_offset: u64::try_from(recipe.start).ok()?,
                secondary_offset: u64::try_from(recipe.member_end).ok()?,
                end_offset: u64::try_from(recipe.end).ok()?,
                next_primary_index: meta
                    .records
                    .binary_search_by_key(&u64::try_from(recipe.end).ok()?, |record| {
                        record.bulk_offset
                    })
                    .ok()
                    .map(|index| meta.records[index].entity_id),
            }),
        )
    } else {
        let next = frames.get(ordinal.checked_add(chain.len())?)?;
        if recipe.end != next.start {
            return None;
        }
        (next.start, u32::try_from(next.entity_id).ok()?, None)
    };
    let byte_offset = u64::try_from(primary.start).ok()?;
    Some(RegisteredFaceOperandFrame {
        header: DesignRecordHeader {
            id: design_record_id(
                stream
                    .strip_prefix(crate::ids::SCHEME_PREFIX)
                    .unwrap_or(stream),
                ":design-record-header#",
                byte_offset,
            ),
            byte_offset,
            record_index,
            class_tag: primary.class_tag.clone(),
        },
        offsets: [
            primary.member_end,
            first.start,
            second.start,
            recipe.start,
            end,
        ],
        next_record_index,
        recipe_frame,
    })
}

/// Decode face-recipe operand frames named by grouped and direct feature references.
pub(crate) fn decode_face_operands(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
    recipes: &[ConstructionRecipe],
    types: &[crate::records::entity_header::SegmentType],
) -> Result<(Vec<DesignFaceOperand>, Vec<LossNote>), CodecError> {
    let mut header_index = HashMap::new();
    for header in headers {
        let Some(stream) = native_stream(&header.id) else {
            continue;
        };
        let key = (stream, header.record_index);
        header_index.insert(key, header);
    }
    let mut scope_index = HashMap::new();
    for scope in scopes {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let key = (stream, scope.record_index);
        scope_index.insert(key, scope);
    }
    let mut split_face_frames = HashMap::new();
    let mut indexed_split_face_streams = HashSet::new();
    for scope in scopes.iter().filter(|scope| {
        scope.kind() == crate::records::feature::scope::DesignFeatureKind::SplitFace
    }) {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        if !indexed_split_face_streams.insert(stream) {
            continue;
        }
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let Ok(Some(meta)) =
            crate::design::decode::meta::metadata_for_bulk_stream(scan, &entry.name)
        else {
            continue;
        };
        let Ok(frames) = unambiguous_design_primary_frames(bytes, &meta) else {
            continue;
        };
        for ordinal in 0..frames.len() {
            if let Some(frame) = registered_face_operand_frame(stream, &frames, &meta, ordinal) {
                split_face_frames.insert((stream, frame.header.record_index), frame);
            }
        }
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let grouped_members = groups
        .iter()
        .filter_map(|group| native_stream(&group.id).map(|stream| (stream, group)))
        .flat_map(|(stream, group)| {
            group
                .members()
                .iter()
                .map(move |member| (stream, group.scope_record_index, member.value))
        })
        .collect::<HashSet<_>>();
    let mut record_offset_index: HashMap<&str, IndexedRecordOffsets> = HashMap::new();
    for scope in scope_index.values().filter(|scope| {
        scope.kind() == crate::records::feature::scope::DesignFeatureKind::SplitFace
    }) {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(operation) = scope.split_face_operation() else {
            continue;
        };
        if let Some(reference) = operation.direction_reference.local_record_index() {
            seen.insert((stream, scope.record_index, reference));
        }
        for input in &operation.inputs {
            if let Some(reference) = input.value.local_record_index() {
                if !grouped_members.contains(&(stream, scope.record_index, reference)) {
                    seen.insert((stream, scope.record_index, reference));
                }
            }
        }
    }
    for group in groups {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        seen.insert((stream, group.scope_record_index, group.record_index));
        let Some(scope) = scope_index.get(&(stream, group.scope_record_index)) else {
            continue;
        };
        let is_extrude_operand = matches!(
            group.extrude_role(),
            Some(DesignExtrudeOperandRole::Profile | DesignExtrudeOperandRole::Faces(_))
        );
        let is_offset_faces_operand = design_feature_family(&scope.kind())
            == Some(DesignFeatureFamily::OffsetFaces)
            && group.role() == DesignOperandRole::ROLE_0X10;
        let is_shell_operand = design_feature_family(&scope.kind())
            == Some(DesignFeatureFamily::Shell)
            && group.role() == DesignOperandRole::ROLE_0X10;
        let is_loft_profile = design_feature_family(&scope.kind())
            == Some(DesignFeatureFamily::Loft)
            && matches!(
                group.role(),
                DesignOperandRole::PROFILE | DesignOperandRole::ROLE_0X43
            );
        let is_sweep_guide_surface = design_feature_family(&scope.kind())
            == Some(DesignFeatureFamily::Sweep)
            && group.role() == DesignOperandRole::FACES;
        let is_sweep_profile = design_feature_family(&scope.kind())
            == Some(DesignFeatureFamily::Sweep)
            && group.role() == DesignOperandRole::PROFILE;
        let is_revolve_axis = design_feature_family(&scope.kind())
            == Some(DesignFeatureFamily::Revolve)
            && matches!(
                group.role(),
                DesignOperandRole::ROLE_0X21 | DesignOperandRole::ROLE_0X5
            );
        let is_edge_treatment_support = matches!(
            design_feature_family(&scope.kind()),
            Some(DesignFeatureFamily::Fillet | DesignFeatureFamily::Chamfer)
        );
        let is_circular_pattern_seed = design_feature_family(&scope.kind())
            == Some(DesignFeatureFamily::CircularPattern)
            && group.role() == DesignOperandRole::BODIES_B;
        let is_mirror_seed = design_feature_family(&scope.kind())
            == Some(DesignFeatureFamily::Mirror)
            && group.role() == DesignOperandRole::BODIES_B;
        let is_mirror_plane = design_feature_family(&scope.kind())
            == Some(DesignFeatureFamily::Mirror)
            && group.role() == DesignOperandRole::ROLE_0X5;
        let is_pattern_face_seed = crate::design::is_pattern_face_seed_group(&scope.kind(), group);
        let is_bend_stationary = scope.kind()
            == crate::records::feature::scope::DesignFeatureKind::Bend
            && group.role() == DesignOperandRole::ROLE_0X10;
        let is_split_face_operand = scope.kind()
            == crate::records::feature::scope::DesignFeatureKind::SplitFace
            && matches!(
                group.role(),
                DesignOperandRole::ROLE_0X9 | DesignOperandRole::ROLE_0X10
            );
        let is_delete_face_operand = matches!(
            scope.kind(),
            crate::records::feature::scope::DesignFeatureKind::MoveFace
                | crate::records::feature::scope::DesignFeatureKind::DeleteFace
                | crate::records::feature::scope::DesignFeatureKind::SurfaceDeleteFace
                | crate::records::feature::scope::DesignFeatureKind::UnStitch
        );
        let is_emboss_face = scope.kind()
            == crate::records::feature::scope::DesignFeatureKind::Emboss
            && group.role() == DesignOperandRole::ROLE_0X10
            && scope
                .emboss_operand_construction()
                .is_some_and(|construction| {
                    group.record_index == construction.face_group.value
                        && group.scope_reference_ordinal == 2
                });
        let is_thread_face = scope.kind()
            == crate::records::feature::scope::DesignFeatureKind::Thread
            && group.role() == DesignOperandRole::ROLE_0X10;
        let is_hole_face = scope.kind() == crate::records::feature::scope::DesignFeatureKind::Hole
            && group.role() == DesignOperandRole::BODIES_A;
        let is_draft_operand =
            design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Draft);
        let is_replace_face_operand = design_feature_family(&scope.kind())
            == Some(DesignFeatureFamily::ReplaceFace)
            && matches!(
                group.role(),
                DesignOperandRole::ROLE_0X9 | DesignOperandRole::ROLE_0X10
            );
        let is_surface_offset_operand = design_feature_family(&scope.kind())
            == Some(DesignFeatureFamily::SurfaceOffset)
            && group.role() == DesignOperandRole::PROFILE;
        let is_surface_trim_tool = design_feature_family(&scope.kind())
            == Some(DesignFeatureFamily::SurfaceTrim)
            && group.role() == DesignOperandRole::ROLE_0X9
            && group.members().len() == 1;
        if scope.kind() == crate::records::feature::scope::DesignFeatureKind::SplitFace
            && !is_split_face_operand
        {
            for member in group.members() {
                seen.insert((stream, scope.record_index, member.value));
            }
            continue;
        }
        if !is_extrude_operand
            && !is_offset_faces_operand
            && !is_shell_operand
            && !is_loft_profile
            && !is_sweep_guide_surface
            && !is_sweep_profile
            && !is_revolve_axis
            && !is_edge_treatment_support
            && !is_circular_pattern_seed
            && !is_mirror_seed
            && !is_mirror_plane
            && !is_pattern_face_seed
            && !is_split_face_operand
            && !is_bend_stationary
            && !is_delete_face_operand
            && !is_surface_untrim_face_group(scope, group)
            && !is_emboss_face
            && !is_thread_face
            && !is_hole_face
            && !is_draft_operand
            && !is_replace_face_operand
            && !is_surface_offset_operand
            && !is_surface_trim_tool
        {
            continue;
        }
        if group.extrude_role() == Some(DesignExtrudeOperandRole::Profile)
            && scope.extrude_profile().is_some()
        {
            continue;
        }
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let records = cached_borrowed_record_offsets(&mut record_offset_index, stream, bytes);
        let stream_types = is_sweep_profile
            .then(|| crate::design::decode::meta::stream_types_by_entity(types, &entry.name));
        for (group_member_index, record_index) in group
            .members()
            .iter()
            .map(|member| &member.value)
            .enumerate()
        {
            if stream_types.as_ref().is_some_and(|types| {
                !types
                    .get(&u64::from(*record_index))
                    .is_some_and(|(guid, _)| {
                        guid.eq_ignore_ascii_case(
                            crate::records::feature::path_features::pipe::RECIPE_GUID,
                        )
                    })
            }) {
                continue;
            }
            if !seen.insert((stream, scope.record_index, *record_index)) {
                continue;
            }
            let Ok(group_member_ordinal) = u32::try_from(group_member_index) else {
                continue;
            };
            let registered_frame = is_split_face_operand
                .then(|| split_face_frames.get(&(stream, *record_index)))
                .flatten();
            let header = registered_frame
                .map(|frame| &frame.header)
                .or_else(|| header_index.get(&(stream, *record_index)).copied());
            let Some(header) = header else {
                continue;
            };
            let caller_next_byte_offset = group
                .members()
                .get(group_member_index + 1)
                .and_then(|record| header_index.get(&(stream, record.value)))
                .copied()
                .map(|header| header.byte_offset)
                .or_else(|| {
                    if !is_offset_faces_operand
                        && !is_shell_operand
                        && !is_extrude_operand
                        && group_member_index + 1 != group.members().len()
                    {
                        return None;
                    }
                    scope
                        .reference_members()
                        .values()
                        .position(|candidate| candidate == record_index)
                        .and_then(|ordinal| scope.reference_members().values().nth(ordinal + 1))
                        .and_then(|record_index| header_index.get(&(stream, *record_index)))
                        .map(|next| next.byte_offset)
                        .filter(|next| {
                            is_offset_faces_operand
                                || is_shell_operand
                                || *next > header.byte_offset
                        })
                });
            if let Some(operand) = parse_face_operand(
                ctx,
                bytes,
                records,
                crate::design::decode::operands::face_operand_frames::FaceOperandFrame {
                    scope,
                    scope_reference_ordinal: group.scope_reference_ordinal,
                    group_ownership: Some((group.record_index, group_member_ordinal)),
                    next_byte_offset: caller_next_byte_offset,
                    registered_frame,
                    header,
                },
                recipes,
            ) {
                let operand = operand?;
                if (!is_pattern_face_seed
                    || operand.recipe_kind == ConstructionRecipeKind::BoundedFace)
                    && (!is_surface_trim_tool || operand.is_terminal_face_recipe())
                {
                    out.push(operand);
                }
            }
        }
    }
    for scope in scope_index.values().filter(|scope| {
        let is_legacy_as_built_421 = scope.kind()
            == crate::records::feature::scope::DesignFeatureKind::AsBuilt
            && crate::design::assembly::legacy_as_built_421_generation(
                scope.frame_length(),
                scope.class_tag.as_str(),
                scope.paired_class_tag.as_str(),
            )
            .is_some();
        matches!(
            design_feature_family(&scope.kind()),
            Some(
                DesignFeatureFamily::OffsetFaces
                    | DesignFeatureFamily::Shell
                    | DesignFeatureFamily::Thicken
                    | DesignFeatureFamily::Split
                    | DesignFeatureFamily::ReplaceFace
            )
        ) || matches!(
            scope.kind(),
            crate::records::feature::scope::DesignFeatureKind::SplitFace
                | crate::records::feature::scope::DesignFeatureKind::Hole
        ) || is_legacy_as_built_421
    }) {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let records = cached_borrowed_record_offsets(&mut record_offset_index, stream, bytes);
        let legacy_as_built = scope.kind()
            == crate::records::feature::scope::DesignFeatureKind::AsBuilt
            && crate::design::assembly::legacy_as_built_421_generation(
                scope.frame_length(),
                scope.class_tag.as_str(),
                scope.paired_class_tag.as_str(),
            )
            .is_some();
        for ordinal in (0..scope.reference_members().len())
            .filter(|ordinal| !legacy_as_built || matches!(*ordinal, 1 | 3))
        {
            let Some(record_index) = scope.reference_members().values().nth(ordinal).copied()
            else {
                continue;
            };
            if scope.kind() == crate::records::feature::scope::DesignFeatureKind::ReplaceFace
                && groups.iter().any(|group| {
                    native_stream(&group.id) == Some(stream)
                        && group.scope_record_index == scope.record_index
                        && group
                            .members()
                            .iter()
                            .any(|member| member.value == record_index)
                })
            {
                continue;
            }
            if !seen.insert((stream, scope.record_index, record_index)) {
                continue;
            }
            let registered_frame = (scope.kind()
                == crate::records::feature::scope::DesignFeatureKind::SplitFace)
                .then(|| split_face_frames.get(&(stream, record_index)))
                .flatten();
            let header = registered_frame
                .map(|frame| &frame.header)
                .or_else(|| header_index.get(&(stream, record_index)).copied());
            let (Ok(scope_reference_ordinal), Some(header)) = (u32::try_from(ordinal), header)
            else {
                continue;
            };
            let caller_next_byte_offset = if scope.kind()
                == crate::records::feature::scope::DesignFeatureKind::AsBuilt
                && crate::design::assembly::legacy_as_built_421_generation(
                    scope.frame_length(),
                    scope.class_tag.as_str(),
                    scope.paired_class_tag.as_str(),
                )
                .is_some()
            {
                scope
                    .reference_members()
                    .values()
                    .nth(ordinal + 1)
                    .and_then(|record_index| header_index.get(&(stream, *record_index)))
                    .map(|header| header.byte_offset)
            } else {
                None
            };
            if let Some(operand) = parse_face_operand(
                ctx,
                bytes,
                records,
                crate::design::decode::operands::face_operand_frames::FaceOperandFrame {
                    scope,
                    scope_reference_ordinal,
                    group_ownership: None,
                    next_byte_offset: caller_next_byte_offset,
                    registered_frame,
                    header,
                },
                recipes,
            ) {
                out.push(operand?);
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    let losses = out
        .iter()
        .filter_map(|operand| {
            let stream = native_stream(&operand.id)?;
            let scope = scope_index.get(&(stream, operand.scope_record_index))?;
            let record_index = operand.record_index();
            (scope.kind() == crate::records::feature::scope::DesignFeatureKind::SplitFace
                && !split_face_frames.contains_key(&(stream, record_index)))
            .then(|| {
                crate::loss::F3dLossCode::FeatureSplitFaceIndexedOperandAssumed.note(format!(
                    "SplitFace scope {} assumes the indexed headers bound face operand {record_index}; its registered envelope is unavailable.",
                    scope.id,
                ))
            })
        })
        .collect();
    Ok((out, losses))
}
