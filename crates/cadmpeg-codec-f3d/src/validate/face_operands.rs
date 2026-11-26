// SPDX-License-Identifier: Apache-2.0
//! Validation of face operands and face source groups.

use super::fillets::{
    mirror_operand_recipe_admitted, revolve_axis_recipe_kind_admitted,
    revolve_face_group_role_admitted,
};
use super::operand_groups::{recipe_program_operand_length, recipe_reference_frames_match};
use super::{design_stream, push_constant_finding, Ctx};
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::{design, history, ids, records};
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::check::{Check, Finding};
use std::collections::{BTreeSet, HashSet};

/// Validate face operands and their recipe frames; returns their record set.
pub(super) fn validate_face_operands<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    expected_face_operands: &[records::topology::face::DesignFaceOperand],
) -> Result<HashSet<(&'a str, u32, u32)>, CodecError> {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let recipes_by_id = &ctx.recipes_by_id;
    let scopes_by_index = &ctx.scopes_by_index;
    let historical_candidates_retained = history::projection_was_finalized(&native.asm_histories)
        || ctx.ir.fragment && native.asm_histories.is_empty();
    let face_groups_by_index = native
        .design_construction_operand_groups
        .iter()
        .map(|group| ((design_stream(&group.id), group.record_index), group))
        .collect::<std::collections::HashMap<_, _>>();
    let expected_face_operands = expected_face_operands
        .iter()
        .map(|operand| (operand.id.as_str(), operand))
        .collect::<std::collections::HashMap<_, _>>();
    let mut face_operand_records = HashSet::new();
    let header_indices_by_offset = records_by_index
        .values()
        .map(|header| {
            (
                (design_stream(&header.id), header.byte_offset),
                header.record_index,
            )
        })
        .collect::<std::collections::HashMap<_, _>>();
    let mut header_offsets_by_stream = std::collections::HashMap::<&str, Vec<u64>>::new();
    let mut header_offset_claims = std::collections::HashMap::<(&str, u32), (u64, u64)>::new();
    let mut header_index_claims = std::collections::HashMap::<(&str, u64), (u32, u32)>::new();
    if native
        .design_face_operands
        .iter()
        .any(|operand| operand.registered_recipe_frame().is_some())
    {
        for header in &native.design_record_headers {
            let stream = design_stream(&header.id);
            header_offsets_by_stream
                .entry(stream)
                .or_default()
                .push(header.byte_offset);
            header_offset_claims
                .entry((stream, header.record_index))
                .and_modify(|range| {
                    range.0 = range.0.min(header.byte_offset);
                    range.1 = range.1.max(header.byte_offset);
                })
                .or_insert((header.byte_offset, header.byte_offset));
            header_index_claims
                .entry((stream, header.byte_offset))
                .and_modify(|range| {
                    range.0 = range.0.min(header.record_index);
                    range.1 = range.1.max(header.record_index);
                })
                .or_insert((header.record_index, header.record_index));
        }
        for offsets in header_offsets_by_stream.values_mut() {
            offsets.sort_unstable();
        }
    }
    let mut stream_types_by_stream = std::collections::HashMap::new();
    for operand in &native.design_face_operands {
        let native_stream = design_stream(&operand.id);
        let stream_types = stream_types_by_stream
            .entry(native_stream)
            .or_insert_with(|| {
                native_stream
                    .strip_prefix("f3d:")
                    .and_then(ids::decode_identity_key_component)
                    .map(|entry| {
                        design::decode::meta::stream_types_by_entity(&native.design_types, &entry)
                    })
                    .unwrap_or_default()
            });
        let topology_recipe_member = stream_types
            .get(&u64::from(operand.record_index()))
            .is_some_and(|(guid, _)| {
                guid.eq_ignore_ascii_case(records::feature::path_features::pipe::RECIPE_GUID)
            });
        let scope = scopes_by_index.get(&(native_stream, operand.scope_record_index));
        let header = records_by_index.get(&(native_stream, operand.record_index()));
        let recipe = recipes_by_id.get(operand.recipe_id.as_str());
        let mut expected_faces = recipe
            .and_then(|recipe| recipe.record_index)
            .map(|record_index| i64::from(record_index.value))
            .filter(|value| *value >= 0)
            .into_iter()
            .flat_map(|design_reference| {
                native
                    .persistent_subentity_tags
                    .iter()
                    .filter(move |tag| {
                        crate::ids::same_native_occurrence(&tag.id, &operand.id)
                            && tag.design_references.contains(&design_reference)
                    })
                    .filter_map(|tag| match &tag.target {
                        cadmpeg_ir::attributes::AttributeTarget::Face(id) => Some(id),
                        _ => None,
                    })
            })
            .collect::<Vec<_>>();
        expected_faces.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        expected_faces.dedup();
        let mut expected_references = design::decode::dimension_frames::decode_recipe_references(
            &operand.recipe_prefix_bytes,
            operand.recipe_prefix_offset(),
        );
        if !historical_candidates_retained {
            for reference in &mut expected_references {
                design::decode::dimension_frames::bind_recipe_reference_candidates(
                    reference,
                    &native.persistent_subentity_tags,
                    Some(&operand.id),
                );
            }
        }
        let recipe_design_reference = recipe
            .and_then(|recipe| recipe.record_index)
            .map(|record_index| i64::from(record_index.value))
            .filter(|value| *value >= 0);
        let referenced_faces = expected_references
            .iter()
            .filter(|reference| Some(reference.design_reference) == recipe_design_reference)
            .flat_map(|reference| &reference.candidate_faces)
            .collect::<std::collections::HashSet<_>>();
        let expected_unreferenced_faces = expected_faces
            .iter()
            .copied()
            .filter(|face| !referenced_faces.contains(face))
            .collect::<Vec<_>>();
        let mut expected_alternate_selector_faces = expected_references
            .iter()
            .filter(|reference| Some(reference.design_reference) == recipe_design_reference)
            .flat_map(|reference| &reference.alternate_selector_faces)
            .collect::<Vec<_>>();
        expected_alternate_selector_faces.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        expected_alternate_selector_faces.dedup();
        let next_header_matches = match operand.registered_recipe_frame() {
            Some(frame) => {
                scope.is_some_and(|scope| {
                    scope.kind() == records::feature::scope::DesignFeatureKind::SplitFace
                }) && frame.record_index == operand.recipe_record_index()
                    && frame.primary_offset == operand.recipe_record_byte_offset()
                    && frame.record_index == operand.next_record_index
                    && frame.secondary_offset == operand.next_byte_offset()
                    && frame.primary_offset < frame.secondary_offset
                    && frame
                        .secondary_offset
                        .checked_add(u64_from_index(
                            crate::layout::indexed_design_record_header::LEN,
                        ))
                        .is_some_and(|end| end <= frame.end_offset)
                    && header_offset_claims
                        .get(&(native_stream, frame.record_index))
                        .is_none_or(|range| {
                            range.0 >= frame.primary_offset && range.1 < frame.end_offset
                        })
                    && header_index_claims
                        .get(&(native_stream, frame.primary_offset))
                        .is_none_or(|range| *range == (frame.record_index, frame.record_index))
                    && header_index_claims
                        .get(&(native_stream, frame.secondary_offset))
                        .is_none_or(|range| *range == (frame.record_index, frame.record_index))
                    && match frame.next_primary_index {
                        Some(index) => {
                            index != u64::from(frame.record_index)
                                && u32::try_from(index)
                                    .ok()
                                    .and_then(|index| {
                                        header_offset_claims.get(&(native_stream, index))
                                    })
                                    .is_none_or(|range| range.0 == frame.end_offset)
                                && header_index_claims
                                    .get(&(native_stream, frame.end_offset))
                                    .is_none_or(|range| {
                                        u64::from(range.0) == index && u64::from(range.1) == index
                                    })
                        }
                        None => header_offsets_by_stream
                            .get(native_stream)
                            .is_none_or(|offsets| {
                                offsets
                                    .last()
                                    .is_none_or(|offset| *offset < frame.end_offset)
                            }),
                    }
                    && header_offsets_by_stream
                        .get(native_stream)
                        .is_none_or(|offsets| {
                            let first =
                                offsets.partition_point(|offset| *offset <= frame.primary_offset);
                            let end = offsets.partition_point(|offset| *offset < frame.end_offset);
                            offsets.get(first..end).is_some_and(|inside| {
                                inside
                                    .iter()
                                    .all(|offset| *offset == frame.secondary_offset)
                            })
                        })
            }
            None => face_operand_next_header_matches(
                header_indices_by_offset
                    .get(&(native_stream, operand.next_byte_offset()))
                    .copied(),
                operand.next_record_index,
                records_by_index.contains_key(&(native_stream, operand.next_record_index)),
            ),
        };
        let expected_node_offsets = operand
            .recipe_program
            .windows(3)
            .enumerate()
            .filter(|(_, values)| *values == [-1, -1, 2])
            .map(|(index, _)| {
                u64_from_index(index)
                    .checked_mul(4)
                    .and_then(|delta| operand.recipe_program_offset.checked_add(delta))
                    .ok_or_else(|| {
                        CodecError::Malformed("F3D face recipe node offset overflows".into())
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let expected_nodes = expected_node_offsets
            .iter()
            .copied()
            .zip(
                expected_node_offsets
                    .iter()
                    .copied()
                    .skip(1)
                    .chain(std::iter::once(operand.next_byte_offset())),
            )
            .collect::<Vec<_>>();
        let valid_program =
            match design::decode::operands::face_recipe_program_kind(&operand.recipe_program) {
                Some(design::decode::operands::FaceRecipeProgramKind::Terminal) => {
                    operand.recipe_nodes.is_empty()
                }
                Some(design::decode::operands::FaceRecipeProgramKind::Counted { .. }) => {
                    operand.recipe_nodes.len() == expected_nodes.len()
                        && operand.recipe_nodes.iter().zip(expected_nodes).try_fold(
                            true,
                            |valid, (node, (start, end))| -> Result<bool, CodecError> {
                                if !valid {
                                    return Ok(false);
                                }
                                Ok(node.byte_offset == start
                                    && node.end_byte_offset == end
                                    && node.program.get(0..3) == Some(&[-1, -1, 2])
                                    && node.recipe_structure
                                        == node
                                            .program
                                            .get(3..)
                                            .map(|program| {
                                                design::decode::operands::recipe_structure::face_recipe_structure(
                                                    ctx.decode, program,
                                                )
                                            })
                                            .transpose()?
                                            .flatten()
                                    && u64_from_index(node.program.len())
                                        .checked_mul(4)
                                        .and_then(|delta| start.checked_add(delta))
                                        .is_some_and(|expected_offset| expected_offset == end))
                            },
                        )?
                        && operand.recipe_nodes.first().is_none_or(|first_node| {
                            first_node
                                .byte_offset
                                .checked_sub(operand.recipe_program_offset)
                                .and_then(|byte_offset| usize::try_from(byte_offset / 4).ok())
                                .is_some_and(|first_node_index| {
                                    operand
                                        .recipe_nodes
                                        .iter()
                                        .flat_map(|node| node.program.iter().copied())
                                        .eq(operand
                                            .recipe_program
                                            .iter()
                                            .copied()
                                            .skip(first_node_index))
                                })
                        })
                }
                None => false,
            };
        let expected_history = expected_face_operands.get(operand.id.as_str());
        let valid = scope.is_some_and(|scope| {
            let family = design::design_feature_family(&scope.kind());
            match (operand.group_record_index(), operand.group_member_ordinal()) {
                (Some(group_record_index), Some(group_member_ordinal)) => {
                    let group = face_groups_by_index
                        .get(&(native_stream, group_record_index))
                        .copied();
                    let exact_group_member = group.is_some_and(|group| {
                        group.scope_record_index == operand.scope_record_index
                            && usize::try_from(operand.scope_reference_ordinal)
                                .ok()
                                .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                                == Some(&group_record_index)
                            && usize::try_from(group_member_ordinal)
                                .ok()
                                .and_then(|ordinal| {
                                    group.members().get(ordinal).map(|member| &member.value)
                                })
                                == Some(&operand.record_index())
                    });
                    let next_member = group.and_then(|group| {
                        usize::try_from(group_member_ordinal)
                            .ok()
                            .and_then(|ordinal| ordinal.checked_add(1))
                            .and_then(|ordinal| group.members().get(ordinal))
                            .map(|member| member.value)
                    });
                    let frame_end = next_member
                        .or_else(|| {
                            scope
                                .reference_members()
                                .values()
                                .position(|candidate| *candidate == operand.record_index())
                                .and_then(|ordinal| {
                                    scope.reference_members().values().nth(ordinal + 1)
                                })
                                .copied()
                        })
                        .and_then(|record_index| records_by_index.get(&(native_stream, record_index)))
                        .map(|next| next.byte_offset)
                        .filter(|next| *next > operand.byte_offset());
                    exact_group_member
                        && frame_end.is_none_or(|end| operand.next_byte_offset() <= end)
                        && match family {
                            Some(
                                design::DesignFeatureFamily::Extrude
                                | design::DesignFeatureFamily::OffsetFaces
                                | design::DesignFeatureFamily::Shell
                                | design::DesignFeatureFamily::Thicken
                                | design::DesignFeatureFamily::Split,
                            ) => true,
                            Some(design::DesignFeatureFamily::ReplaceFace) => {
                                group.is_some_and(|group| {
                                    matches!(
                                        group.role(),
                                        DesignOperandRole::ROLE_0X9 | DesignOperandRole::ROLE_0X10
                                    )
                                }) && matches!(
                                    operand.recipe_kind,
                                    records::recipes::ConstructionRecipeKind::Face
                                        | records::recipes::ConstructionRecipeKind::BoundedFace
                                )
                            }
                            Some(design::DesignFeatureFamily::Loft) => {
                                group.is_some_and(|group| {
                                    matches!(
                                        group.role(),
                                        DesignOperandRole::PROFILE | DesignOperandRole::ROLE_0X43
                                    )
                                }) && operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::BoundedFace
                            }
                            Some(design::DesignFeatureFamily::Sweep) => {
                                group.is_some_and(|group| match group.role() {
                                    DesignOperandRole::FACES => true,
                                    DesignOperandRole::PROFILE => topology_recipe_member,
                                    _ => false,
                                }) && operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::BoundedFace
                            }
                            Some(design::DesignFeatureFamily::SurfaceOffset) => {
                                group
                                    .is_some_and(|group| group.role() == DesignOperandRole::PROFILE)
                                    && operand.recipe_kind
                                        == records::recipes::ConstructionRecipeKind::BoundedFace
                            }
                            Some(design::DesignFeatureFamily::SheetMetalBend) => {
                                group.is_some_and(|group| {
                                    group.role() == DesignOperandRole::ROLE_0X10
                                })
                            }
                            Some(design::DesignFeatureFamily::SurfaceTrim) => {
                                group.is_some_and(|group| {
                                    group.role() == DesignOperandRole::ROLE_0X9
                                        && group.members().len() == 1
                                }) && operand.is_terminal_face_recipe()
                            }
                            Some(design::DesignFeatureFamily::Draft) => {
                                group.is_some_and(|group| match group.role() {
                                    DesignOperandRole::ROLE_0X10 => {
                                        operand.recipe_kind
                                            == records::recipes::ConstructionRecipeKind::BoundedFace
                                    }
                                    DesignOperandRole::ROLE_0X21 => {
                                        operand.recipe_kind
                                            == records::recipes::ConstructionRecipeKind::Face
                                    }
                                    _ => false,
                                })
                            }
                            Some(design::DesignFeatureFamily::Revolve) => {
                                group.is_some_and(|group| {
                                    revolve_face_group_role_admitted(group.role())
                                }) && revolve_axis_recipe_kind_admitted(operand.recipe_kind)
                            }
                            Some(
                                design::DesignFeatureFamily::RectangularPattern
                                | design::DesignFeatureFamily::CircularPattern
                                | design::DesignFeatureFamily::Mirror,
                            ) if group.is_some_and(|group| {
                                design::is_pattern_face_seed_group(&scope.kind(), group)
                            }) =>
                            {
                                operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::BoundedFace
                            }
                            Some(design::DesignFeatureFamily::CircularPattern) => {
                                group.is_some_and(|group| {
                                    group.role() == DesignOperandRole::BODIES_B
                                }) && operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::Face
                            }
                            Some(design::DesignFeatureFamily::Mirror) => {
                                group.is_some_and(|group| {
                                    mirror_operand_recipe_admitted(group.role(), operand.recipe_kind)
                                })
                            }
                            Some(design::DesignFeatureFamily::Thread) => {
                                group.is_some_and(|group| {
                                    group.role() == DesignOperandRole::ROLE_0X10
                                        && scope.thread_construction().is_none_or(|construction| {
                                            construction
                                                .face_group_record_indices
                                                .contains(&group.record_index)
                                        })
                                }) && operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::BoundedFace
                            }
                            Some(
                                design::DesignFeatureFamily::Fillet
                                | design::DesignFeatureFamily::Chamfer,
                            ) => {
                                operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::BoundedFace
                                    && native.design_edge_identity_operands.iter().any(|identity| {
                                        design_stream(&identity.id) == native_stream
                                            && identity.scope_record_index
                                                == operand.scope_record_index
                                            && identity.group_record_index == group_record_index
                                            && identity.group_member_ordinal == group_member_ordinal
                                            && identity.record_index() == operand.record_index()
                                            && identity.class_tag == operand.class_tag
                                    })
                            }
                            Some(design::DesignFeatureFamily::Emboss) =>
                            {
                                group.is_some_and(|group| {
                                    group.role() == DesignOperandRole::ROLE_0X10
                                        && scope.emboss_operand_construction().is_some_and(|construction| {
                                            group.record_index == construction.face_group.value
                                        })
                                }) && operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::BoundedFace
                            }
                            None if scope.kind()
                                == crate::records::feature::scope::DesignFeatureKind::SplitFace =>
                            {
                                group.is_some_and(|group| {
                                    matches!(group.role(), DesignOperandRole::ROLE_0X9 | DesignOperandRole::ROLE_0X10)
                                }) && matches!(operand.recipe_kind,
                                    records::recipes::ConstructionRecipeKind::BoundedFace | records::recipes::ConstructionRecipeKind::Face)
                            }
                            None if group.is_some_and(|group| {
                                design::decode::operands::edge_operands::is_surface_untrim_face_group(scope, group)
                            }) =>
                            {
                                matches!(
                                    operand.recipe_kind,
                                    records::recipes::ConstructionRecipeKind::Face
                                        | records::recipes::ConstructionRecipeKind::BoundedFace
                                )
                            }
                            None if matches!(
                                scope.kind(),
                                crate::records::feature::scope::DesignFeatureKind::MoveFace
                                    | crate::records::feature::scope::DesignFeatureKind::DeleteFace
                                    | crate::records::feature::scope::DesignFeatureKind::SurfaceDeleteFace
                                    | crate::records::feature::scope::DesignFeatureKind::UnStitch
                            ) =>
                            {
                                group.is_some_and(|group| {
                                    group.role() == DesignOperandRole::ROLE_0X10
                                }) && operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::BoundedFace
                            }
                            _ => false,
                        }
                }
                (None, None) => {
                    let direct_member = usize::try_from(operand.scope_reference_ordinal)
                        .ok()
                        .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                        == Some(&operand.record_index());
                    direct_member
                        && match family {
                            Some(
                                design::DesignFeatureFamily::OffsetFaces
                                | design::DesignFeatureFamily::Shell
                                | design::DesignFeatureFamily::Thicken,
                            ) => true,
                            Some(design::DesignFeatureFamily::Split) => split_direct_face_admitted(
                                operand.scope_reference_ordinal,
                                operand.recipe_kind,
                            ),
                            Some(design::DesignFeatureFamily::Hole) => matches!(
                                operand.recipe_kind,
                                records::recipes::ConstructionRecipeKind::BoundedFace
                                    | records::recipes::ConstructionRecipeKind::Face
                            ),
                            None if scope.kind() == crate::records::feature::scope::DesignFeatureKind::SplitFace => matches!(
                                operand.recipe_kind,
                                records::recipes::ConstructionRecipeKind::BoundedFace | records::recipes::ConstructionRecipeKind::Face
                            ),
                            Some(design::DesignFeatureFamily::Assemble)
                                if scope.kind()
                                    == crate::records::feature::scope::DesignFeatureKind::AsBuilt
                                    && design::assembly::legacy_as_built_421_generation(
                                        scope.frame_length(),
                                        scope.class_tag.as_str(),
                                        scope.paired_class_tag.as_str(),
                                    )
                                    .is_some() =>
                            {
                                matches!(
                                    (operand.scope_reference_ordinal, operand.recipe_kind),
                                    (1, records::recipes::ConstructionRecipeKind::BoundedFace)
                                        | (3, records::recipes::ConstructionRecipeKind::Face)
                                )
                            }
                            _ => false,
                        }
                }
                _ => false,
            }
        }) && header.is_some_and(|header| {
            header.byte_offset == operand.byte_offset() && header.class_tag == operand.class_tag
        }) && operand
            .recipe_prefix_offset().checked_add(u64_from_index(operand.recipe_prefix_bytes.len())).zip(recipe.and_then(|recipe| recipe.byte_offset.checked_sub(4))).is_some_and(|(prefix_end, recipe_prefix_end)| prefix_end == recipe_prefix_end)
            && recipe_reference_frames_match(
                &operand.recipe_references,
                &expected_references,
                historical_candidates_retained,
            )
            && valid_program
            && recipe_program_operand_length(operand.recipe_kind).is_some_and(|operand_length| {
                recipe.is_some_and(|recipe| {
                    recipe.byte_offset.checked_add(operand_length).is_some_and(|expected_offset| operand.recipe_program_offset == expected_offset)
                })
            })
            && u64_from_index(operand.recipe_program.len()).checked_mul(4).and_then(|delta| operand
                    .recipe_program_offset.checked_add(delta)).is_some_and(|expected_offset| operand.next_byte_offset() == expected_offset)
            && next_header_matches
            && (historical_candidates_retained
                || face_ids_match_refs(&operand.candidate_faces, &expected_faces))
            && (historical_candidates_retained
                || face_ids_match_refs(&operand.unreferenced_candidate_faces, &expected_unreferenced_faces))
            && (historical_candidates_retained
                || face_ids_match_refs(&operand.alternate_selector_candidate_faces, &expected_alternate_selector_faces))
            && (ctx.ir.fragment && native.asm_histories.is_empty() || expected_history.is_some_and(|expected| {
                operand.preceding_candidate_faces == expected.preceding_candidate_faces
                    && operand.changed_candidate_faces == expected.changed_candidate_faces
                    && operand.historical_support_contexts == expected.historical_support_contexts
                    && operand.first_reference_support_contexts
                        == expected.first_reference_support_contexts
            }))
            && recipe.is_some_and(|recipe| {
                design_stream(&recipe.id) == native_stream
                    && recipe.kind == operand.recipe_kind
                    && recipe.byte_offset > operand.recipe_record_byte_offset()
                    && recipe.byte_offset < operand.next_byte_offset()
            });
        let valid = valid
            && face_operand_records.insert((
                native_stream,
                operand.scope_record_index,
                operand.record_index(),
            ));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design face operand has an invalid scope or recipe frame",
                Some(operand.id.clone()),
            );
        }
    }
    Ok(face_operand_records)
}

/// Whether a face operand's next record index agrees with the header arena.
///
/// `header_at_next` is the index of the arena header at the operand's next
/// byte offset. The arena omits records that no entity reference names, and
/// the stream end carries next index `0`.
pub(super) fn face_operand_next_header_matches(
    header_at_next: Option<u32>,
    next_record_index: u32,
    next_index_in_arena: bool,
) -> bool {
    match header_at_next {
        Some(record_index) => record_index == next_record_index,
        None => next_record_index == 0 || !next_index_in_arena,
    }
}

/// A `Split` direct face operand is the reference-one member, or a
/// reference-zero or reference-two tool face with a `face_recipe_data` recipe.
pub(super) fn split_direct_face_admitted(
    scope_reference_ordinal: u32,
    recipe_kind: records::recipes::ConstructionRecipeKind,
) -> bool {
    scope_reference_ordinal == 1
        || (matches!(scope_reference_ordinal, 0 | 2)
            && recipe_kind == records::recipes::ConstructionRecipeKind::Face)
}

pub(super) fn face_ids_match_refs(
    actual: &[cadmpeg_ir::ids::FaceId],
    expected: &[&cadmpeg_ir::ids::FaceId],
) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| actual == *expected)
}

/// Report face-group members with no resolved recipe operand.
pub(super) fn validate_face_group_member_resolution(
    findings: &mut Vec<Finding>,
    face_group_members: BTreeSet<(&str, u32, u32)>,
    face_operand_records: &HashSet<(&str, u32, u32)>,
    identity_group_members: &HashSet<(&str, u32, u32)>,
    entity_selection_operands: &[records::topology::entity_selection::DesignEntitySelectionOperand],
    scopes: &[records::feature::scope::DesignParameterScope],
) {
    let entity_selection_records = entity_selection_operands
        .iter()
        .map(|operand| {
            (
                design_stream(&operand.id),
                operand.scope_record_index,
                operand.record_index(),
            )
        })
        .collect::<std::collections::HashSet<_>>();
    let native_profiles = scopes
        .iter()
        .flat_map(|scope| {
            let stream = design_stream(&scope.id);
            scope
                .extrude_native_profiles()
                .into_iter()
                .flatten()
                .map(move |profile| (stream, scope.record_index, profile.record_index))
        })
        .collect::<HashSet<_>>();
    for member in face_group_members {
        if !native_profiles.contains(&member)
            && !face_operand_records.contains(&member)
            && !identity_group_members.contains(&member)
            && !entity_selection_records.contains(&member)
        {
            let entity = {
                format!(
                    "{}:design-face-group-member#{}:{}",
                    member.0, member.1, member.2
                )
            };
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design Extrude face group has an unresolved recipe operand",
                Some(entity),
            );
        }
    }
}

/// Validate retained Face source carriers and their persistent identities.
pub(super) fn validate_face_source_groups(ctx: &Ctx, findings: &mut Vec<Finding>) {
    use design::decode::operands::face_sources::{
        face_source_carrier_spec, face_source_profile_pair_count, face_source_record_is_registered,
        FaceSourceType,
    };
    let native = ctx.native;
    let mut carrier_records = HashSet::new();
    for group in &native.design_face_source_groups {
        let native_stream = design_stream(&group.id);
        let scope = ctx
            .scopes_by_index
            .get(&(native_stream, group.scope_record_index));
        let carrier_header = ctx
            .records_by_index
            .get(&(native_stream, group.carrier_record_index));
        let paired_header = ctx
            .records_by_index
            .get(&(native_stream, group.paired_record_index));
        let carrier_ordinal = usize::try_from(group.carrier_reference_ordinal).ok();
        let source_spec = face_source_carrier_spec(group.source_members.len());
        let registered = |record, class: &str, role| {
            face_source_record_is_registered(&native.design_types, &group.id, record, class, role)
        };
        let scope_links_valid = scope.is_some_and(|scope| {
            scope.kind() == crate::records::feature::scope::DesignFeatureKind::Face
                && registered(
                    scope.record_index,
                    scope.class_tag.as_str(),
                    FaceSourceType::Scope,
                )
                && face_source_profile_pair_count(&native.design_types, scope, |record| {
                    ctx.records_by_index
                        .get(&(native_stream, record))
                        .map(|header| header.class_tag.as_str())
                }) == Some(group.source_members.len())
                && carrier_ordinal
                    .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                    == Some(&group.carrier_record_index)
                && carrier_ordinal
                    .and_then(|ordinal| ordinal.checked_add(1))
                    .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                    == Some(&group.paired_record_index)
        });
        let headers_valid = carrier_header.is_some_and(|header| {
            header.byte_offset == group.carrier_span.start()
                && header.class_tag == group.carrier_class_tag
                && registered(
                    header.record_index,
                    header.class_tag.as_str(),
                    FaceSourceType::Carrier,
                )
        }) && paired_header.is_some_and(|header| {
            header.byte_offset == group.carrier_span.end()
                && header.class_tag == group.paired_class_tag
                && registered(
                    header.record_index,
                    header.class_tag.as_str(),
                    FaceSourceType::Settings,
                )
        });
        let source_offsets_valid = source_spec.is_some_and(|layout| {
            let Ok(source_reference_offset) = u64::try_from(layout.source_reference_offset) else {
                return false;
            };
            if layout
                .scalar_offset
                .checked_add(crate::layout::sheet_metal_region_carrier_scalar_lane::LEN)
                .is_none_or(|length| {
                    u64::try_from(length).map_or(true, |length| {
                        group
                            .carrier_span
                            .start()
                            .checked_add(length)
                            .is_none_or(|end| end > group.carrier_span.end())
                    })
                })
            {
                return false;
            }
            group
                .source_members
                .iter()
                .enumerate()
                .all(|(ordinal, member)| {
                    let Ok(ordinal) = u64::try_from(ordinal) else {
                        return false;
                    };
                    group
                        .carrier_span
                        .start()
                        .checked_add(source_reference_offset)
                        .and_then(|offset| {
                            offset.checked_add(
                                ordinal.checked_mul(
                                    u64::try_from(
                                        crate::layout::sheet_metal_region_identity_reference::LEN,
                                    )
                                    .ok()?,
                                )?,
                            )
                        })
                        == Some(member.offset)
                })
        });
        let mut source_records = HashSet::new();
        let source_members_valid = if let Some(layout) = source_spec {
            if group.source_members.len() == layout.source_count {
                let mut valid = true;
                for member in group.source_members.iter().map(|member| &member.value) {
                    let unique_record = source_records.insert(member.record_index);
                    let persistent = &member.persistent_identity;
                    let local_id_offset = member.byte_offset.checked_add(21);
                    let asset_id_offset = member.byte_offset.checked_add(33);
                    let member_valid = unique_record
                        && registered(
                            member.record_index,
                            member.class_tag.as_str(),
                            FaceSourceType::Identity,
                        )
                        && member.byte_offset > group.carrier_span.start()
                        && local_id_offset == Some(persistent.local_id_offset())
                        && asset_id_offset == Some(persistent.asset_id_offset())
                        && persistent.context_id_offset() > persistent.asset_id_offset()
                        && persistent.tail_slot_offset() > persistent.context_id_offset()
                        && persistent.next_byte_offset() > member.byte_offset;
                    if !member_valid {
                        valid = false;
                        break;
                    }
                }
                valid
            } else {
                false
            }
        } else {
            false
        };
        let valid = carrier_records.insert((native_stream, group.carrier_record_index))
            && scope_links_valid
            && headers_valid
            && source_offsets_valid
            && source_members_valid;
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design Face source carrier has invalid links or offsets",
                Some(group.id.clone()),
            );
        }
    }
}
