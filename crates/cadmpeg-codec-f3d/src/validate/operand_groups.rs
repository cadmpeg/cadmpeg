// SPDX-License-Identifier: Apache-2.0
//! Validation of edge identity, body recipe, and operand group carriers.

use super::assembly::body_recipe_reference_table_is_admitted;
use super::{
    design_stream, native_edge_recipe_faces, push_constant_finding, reload_native_arena, Ctx,
};
use crate::{design, history, records};
use cadmpeg_core::decode::{id_from_index, u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::check::{Check, Finding};
use std::collections::HashSet;

/// Validate edge identity operands; returns their backing record set.
pub(super) fn validate_edge_identity_operands<'a>(
    decode: &DecodeContext<'_>,
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    expected_face_operands: &[records::topology::face::DesignFaceOperand],
) -> Result<HashSet<(&'a str, u32)>, CodecError> {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let operand_groups_by_index = &ctx.operand_groups_by_index;
    let mut expected_edge_identity_operands =
        reload_native_arena(decode, ctx.ir, "design_edge_identity_operands")?;
    let scope_histories = history::scope_histories::bind_scope_histories(
        &native.design_parameter_scopes,
        &native.design_body_bindings,
        &native.design_body_recipe_operands,
        native_edge_recipe_faces(native),
        &native.asm_histories,
        &mut Vec::new(),
    );
    history::selection::bind_edge_identity_history(
        &mut expected_edge_identity_operands,
        &native.design_construction_operand_identities,
        &native.design_parameter_scopes,
        history::binding_histories::BindingHistories::Shared(&native.asm_histories),
        &scope_histories,
        &HashSet::new(),
    );
    history::selection::bind_edge_identity_bounded_face_rules(
        &mut expected_edge_identity_operands,
        expected_face_operands,
    );
    let expected_edge_identity_operands = expected_edge_identity_operands
        .iter()
        .map(|operand| (operand.id.as_str(), operand))
        .collect::<std::collections::HashMap<_, _>>();
    let mut edge_identity_slots = HashSet::new();
    let mut edge_identity_records = HashSet::new();
    for operand in &native.design_edge_identity_operands {
        let native_stream = design_stream(&operand.id);
        let scope = scopes_by_index.get(&(native_stream, operand.scope_record_index));
        let group = operand_groups_by_index.get(&(native_stream, operand.group_record_index));
        let header = records_by_index.get(&(native_stream, operand.record_index()));
        let valid = scope.is_some_and(|scope| {
            matches!(
                design::design_feature_family(&scope.kind()),
                Some(design::DesignFeatureFamily::Fillet | design::DesignFeatureFamily::Chamfer)
            )
        }) && group.is_some_and(|group| {
            group.scope_record_index == operand.scope_record_index
                && usize::try_from(operand.group_member_ordinal)
                    .ok()
                    .and_then(|ordinal| group.members().get(ordinal).map(|member| &member.value))
                    == Some(&operand.record_index())
        }) && header.is_some_and(|header| {
            header.byte_offset == operand.byte_offset() && header.class_tag == operand.class_tag
        }) && expected_edge_identity_operands.get(operand.id.as_str())
            == Some(&operand)
            && edge_identity_slots.insert((
                native_stream,
                operand.group_record_index,
                operand.group_member_ordinal,
            ))
            && edge_identity_records.insert((native_stream, operand.record_index()));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design edge identity operand has an invalid fixed frame",
                Some(operand.id.clone()),
            );
        }
    }
    Ok(edge_identity_records)
}

/// Validate whole-body recipe operands; returns their backing record set.
pub(super) fn validate_body_recipe_operands<'a>(
    decode: &DecodeContext<'_>,
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> Result<HashSet<(&'a str, u32)>, CodecError> {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let operand_groups_by_index = &ctx.operand_groups_by_index;
    let recipes_by_id = &ctx.recipes_by_id;
    let mut expected_operands = reload_native_arena(decode, ctx.ir, "design_body_recipe_operands")?;
    design::decode::operands::body_recipes::bind_body_recipe_operand_candidates(
        &mut expected_operands,
        &native.construction_recipes,
        &native.persistent_subentity_tags,
        &native.design_parameter_scopes,
    );
    history::body_recipe_operands::bind_body_recipe_operand_history_candidates(
        &mut expected_operands,
        &native.construction_recipes,
        &native.design_parameter_scopes,
        history::binding_histories::BindingHistories::Shared(&native.asm_histories),
    )?;
    let expected_operands = expected_operands
        .iter()
        .map(|operand| (operand.id.as_str(), operand))
        .collect::<std::collections::HashMap<_, _>>();
    let mut member_slots = HashSet::new();
    let mut operand_records = HashSet::new();
    for operand in &native.design_body_recipe_operands {
        let native_stream = design_stream(&operand.id);
        let scope = scopes_by_index.get(&(native_stream, operand.scope_record_index));
        let header = records_by_index.get(&(native_stream, operand.record_index()));
        let recipe = recipes_by_id.get(operand.recipe_id.as_str());
        let valid_owner = scope.is_some_and(|scope| match operand.owner {
            records::topology::body_recipe::DesignOperandOwner::Group {
                group_record_index,
                group_member_ordinal,
            } => operand_groups_by_index
                .get(&(native_stream, group_record_index))
                .is_some_and(|group| {
                    group.scope_record_index == operand.scope_record_index
                        && usize::try_from(group_member_ordinal)
                            .ok()
                            .and_then(|ordinal| {
                                group.members().get(ordinal).map(|member| &member.value)
                            })
                            == Some(&operand.record_index())
                }),
            records::topology::body_recipe::DesignOperandOwner::ScopeReference {
                scope_reference_ordinal,
            } => {
                (scope.kind() == crate::records::feature::scope::DesignFeatureKind::Hole
                    || (!scope_reference_ordinal.is_multiple_of(2)
                        && scope.combine_operation().is_some_and(|operation| {
                            operation.target_record_index == operand.record_index()
                                || operation
                                    .tools
                                    .iter()
                                    .any(|tool| tool.record_index == operand.record_index())
                        })))
                    && usize::try_from(scope_reference_ordinal)
                        .ok()
                        .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                        == Some(&operand.record_index())
            }
        });
        let valid = valid_owner
            && header.is_some_and(|header| {
                header.byte_offset == operand.byte_offset() && header.class_tag == operand.class_tag
            })
            && body_recipe_reference_table_is_admitted(scope.copied(), operand)
            && recipe.is_some_and(|recipe| {
                let selector_is_valid = recipe.design.as_ref().is_some_and(|design| {
                    let design_id = &design.id;
                    let design_id_offset = design_id.offset;
                    let Some(selector) = design.selector else {
                        return false;
                    };
                    u64::try_from(design_id.value.len())
                        .ok()
                        .is_some_and(|length| {
                            let selector_follows_id =
                                design_id_offset.checked_add(length) == Some(selector.byte_offset);
                            let prefix_frame =
                                selector.byte_offset.checked_add(20) == Some(recipe.byte_offset);
                            let body_suffix_frame = recipe
                                .byte_offset
                                .checked_add(u64_from_index(b"body_recipe_data".len()))
                                .and_then(|offset| offset.checked_add(12))
                                == Some(design_id_offset)
                                && selector.value == operand.next_record_index();
                            selector_follows_id
                                && selector.value != 0
                                && (prefix_frame || body_suffix_frame)
                        })
                });
                design_stream(&recipe.id) == native_stream
                    && recipe.kind == records::recipes::ConstructionRecipeKind::Body
                    && recipe.byte_offset > operand.context_id_offset()
                    && recipe.byte_offset < operand.next_byte_offset()
                    && selector_is_valid
            })
            && expected_operands.get(operand.id.as_str()) == Some(&operand)
            && member_slots.insert((native_stream, operand.scope_record_index, operand.owner))
            && operand_records.insert((native_stream, operand.record_index()));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design body recipe operand has an invalid nested frame",
                Some(operand.id.clone()),
            );
        }
    }
    Ok(operand_records)
}

/// Report operand groups lacking a typed member carrier.
pub(super) fn validate_operand_group_carriers<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    operand_identity_groups: &HashSet<(&'a str, u32)>,
    edge_identity_records: &HashSet<(&'a str, u32)>,
    body_recipe_operand_records: &HashSet<(&'a str, u32)>,
    edge_operand_records: &HashSet<(&'a str, u32)>,
    edge_treatment_vertex_records: &HashSet<(&'a str, u32)>,
) {
    let native = ctx.native;
    for group in &native.design_construction_operand_groups {
        let native_stream = design_stream(&group.id);
        let mut identity_members = native
            .design_edge_identity_operands
            .iter()
            .filter(|operand| {
                design_stream(&operand.id) == native_stream
                    && operand.scope_record_index == group.scope_record_index
                    && operand.group_record_index == group.record_index
            })
            .collect::<Vec<_>>();
        identity_members.sort_by_key(|left| left.group_member_ordinal);
        let has_exact_identity_members = !group.members().is_empty()
            && identity_members.len() == group.members().len()
            && identity_members
                .iter()
                .enumerate()
                .all(|(ordinal, operand)| {
                    usize::try_from(operand.group_member_ordinal) == Ok(ordinal)
                        && group.members().get(ordinal).map(|member| &member.value)
                            == Some(&operand.record_index())
                        && edge_identity_records.contains(&(native_stream, operand.record_index()))
                });
        let has_exact_entity_selection_members = !group.members().is_empty()
            && group
                .members()
                .iter()
                .map(|member| &member.value)
                .enumerate()
                .all(|(ordinal, record_index)| {
                    id_from_index(ordinal).is_some_and(|ordinal| {
                        native
                            .design_entity_selection_operands
                            .iter()
                            .any(|operand| {
                                design_stream(&operand.id) == native_stream
                                    && operand.scope_record_index == group.scope_record_index
                                    && operand.group_record_index == group.record_index
                                    && operand.group_member_ordinal == ordinal
                                    && operand.record_index() == *record_index
                            })
                    })
                });
        let has_exact_face_members = !group.members().is_empty()
            && group
                .members()
                .iter()
                .map(|member| &member.value)
                .enumerate()
                .all(|(ordinal, record_index)| {
                    id_from_index(ordinal).is_some_and(|ordinal| {
                        native.design_face_operands.iter().any(|operand| {
                            design_stream(&operand.id) == native_stream
                                && operand.scope_record_index == group.scope_record_index
                                && operand.group_record_index() == Some(group.record_index)
                                && operand.group_member_ordinal() == Some(ordinal)
                                && operand.record_index() == *record_index
                        })
                    })
                });
        let has_exact_body_recipe_members = !group.members().is_empty()
            && group
                .members()
                .iter()
                .map(|member| &member.value)
                .enumerate()
                .all(|(ordinal, record_index)| {
                    id_from_index(ordinal).is_some_and(|ordinal| {
                        native.design_body_recipe_operands.iter().any(|operand| {
                            design_stream(&operand.id) == native_stream
                                && operand.scope_record_index == group.scope_record_index
                                && operand.owner.group() == Some((group.record_index, ordinal))
                                && operand.record_index() == *record_index
                                && body_recipe_operand_records
                                    .contains(&(native_stream, operand.record_index()))
                        })
                    })
                });
        let has_exact_topology_recipe_members = !group.members().is_empty()
            && group
                .members()
                .iter()
                .map(|member| &member.value)
                .all(|record_index| {
                    edge_operand_records.contains(&(native_stream, *record_index))
                        || edge_treatment_vertex_records.contains(&(native_stream, *record_index))
                });
        let has_exact_sketch_profile_member = group.members().len() == 1
            && ctx
                .scopes_by_index
                .get(&(native_stream, group.scope_record_index))
                .is_some_and(|scope| {
                    scope
                        .extrude_profile()
                        .or(scope.sweep_profile())
                        .or(scope.base_flange_profile())
                        .is_some_and(|profile| {
                            group
                                .members()
                                .iter()
                                .map(|member| member.value)
                                .eq([profile.record_index])
                        })
                });
        // A profile group of several sketch-profile operands: each member is an
        // operand that the scope retains as a native profile.
        let has_exact_native_profile_members = group.extrude_role()
            == Some(records::topology::extrude_selection::DesignExtrudeOperandRole::Profile)
            && ctx
                .scopes_by_index
                .get(&(native_stream, group.scope_record_index))
                .and_then(|scope| scope.extrude_native_profiles())
                .is_some_and(|profiles| {
                    group
                        .members()
                        .iter()
                        .map(|member| member.value)
                        .eq(profiles.iter().map(|profile| profile.record_index))
                });
        let has_exact_group_members = !group.members().is_empty()
            && group
                .members()
                .iter()
                .map(|member| &member.value)
                .all(|record_index| {
                    native
                        .design_construction_operand_groups
                        .iter()
                        .any(|member| {
                            design_stream(&member.id) == native_stream
                                && member.scope_record_index == group.scope_record_index
                                && member.scope_reference_ordinal > group.scope_reference_ordinal
                                && member.record_index == *record_index
                        })
                });
        let has_exact_trailing_carrier = group.frame.trailing_records().is_empty()
            || operand_identity_groups.contains(&(native_stream, group.record_index))
            || (group.frame.trailing_transforms().len()
                + group.frame.trailing_dual_transforms().len()
                + group.frame.trailing_flags().len()
                == group.frame.trailing_records().len()
                && group
                    .frame
                    .trailing_records()
                    .iter()
                    .map(|record| &record.value)
                    .all(|record_index| {
                        group
                            .frame
                            .trailing_transforms()
                            .iter()
                            .any(|transform| transform.record_index() == *record_index)
                            || group
                                .frame
                                .trailing_dual_transforms()
                                .iter()
                                .any(|transform| transform.record_index == *record_index)
                            || group
                                .frame
                                .trailing_flags()
                                .iter()
                                .any(|flag| flag.record_index == *record_index)
                    }));
        let has_exact_member_carrier = operand_identity_groups
            .contains(&(native_stream, group.record_index))
            || has_exact_identity_members
            || has_exact_entity_selection_members
            || has_exact_face_members
            || has_exact_body_recipe_members
            || has_exact_topology_recipe_members
            || has_exact_sketch_profile_member
            || has_exact_native_profile_members
            || has_exact_group_members;
        if !has_exact_member_carrier {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design construction operand group has no exact typed member",
                Some(group.id.clone()),
            );
        }
        if !has_exact_trailing_carrier {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design construction operand group has no exact trailing carrier",
                Some(group.id.clone()),
            );
        }
    }
}

/// Bytes from a face recipe header to the recipe program, by recipe kind.
///
/// `None` states a recipe kind that carries no face operand: it states no
/// program offset at all.
pub(super) fn recipe_program_operand_length(
    kind: records::recipes::ConstructionRecipeKind,
) -> Option<u64> {
    match kind {
        records::recipes::ConstructionRecipeKind::Face => Some(16),
        records::recipes::ConstructionRecipeKind::BoundedFace => Some(24),
        records::recipes::ConstructionRecipeKind::Body
        | records::recipes::ConstructionRecipeKind::Edge
        | records::recipes::ConstructionRecipeKind::Vertex => None,
    }
}

pub(super) fn recipe_reference_frames_match(
    actual: &[records::dimensions::DesignRecipeReference],
    expected: &[records::dimensions::DesignRecipeReference],
    ignore_derived_candidates: bool,
) -> bool {
    if !ignore_derived_candidates {
        return actual == expected;
    }
    actual.len() == expected.len()
        && actual.iter().zip(expected).all(|(actual, expected)| {
            actual.selector == expected.selector
                && actual.selector_offset == expected.selector_offset
                && actual.token == expected.token
                && actual.token_integer == expected.token_integer
                && actual.token_offset == expected.token_offset
                && actual.design_reference == expected.design_reference
                && actual.design_reference_offset == expected.design_reference_offset
        })
}
