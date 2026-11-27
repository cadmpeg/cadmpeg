// SPDX-License-Identifier: Apache-2.0
//! Validation of extrude selection groups and entity selection operands.

use super::{design_stream, push_constant_finding, Ctx};
use crate::{design, history, records};
use cadmpeg_ir::report::check::{Check, Finding};
use std::collections::HashSet;

/// Validate compact selection groups and their counted member frames.
pub(super) fn validate_extrude_selection_groups(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let mut group_slots = HashSet::new();
    for group in &native.design_extrude_selection_groups {
        let native_stream = design_stream(&group.id);
        let scope = scopes_by_index.get(&(native_stream, group.scope_record_index));
        let header = records_by_index.get(&(native_stream, group.record_index));
        let frame_valid = scope.is_some_and(|scope| {
            (design::design_feature_family(&scope.kind())
                == Some(design::DesignFeatureFamily::Extrude)
                || scope
                    .emboss_operand_construction()
                    .is_some_and(|construction| {
                        group.record_index == construction.auxiliary_group.value
                            && group.scope_reference_ordinal == 8
                            && !group.typed_references
                    }))
                && usize::try_from(group.scope_reference_ordinal)
                    .ok()
                    .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                    == Some(&group.record_index)
        }) && header.is_some_and(|header| {
            header.byte_offset == group.byte_offset() && header.class_tag == group.class_tag
        }) && group
            .members()
            .iter()
            .all(|member| records_by_index.contains_key(&(native_stream, member.value)));
        let valid = if frame_valid {
            group_slots.insert((
                native_stream,
                group.scope_record_index,
                group.scope_reference_ordinal,
            ))
        } else {
            false
        };
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design selection group has an invalid counted frame",
                Some(group.id.clone()),
            );
        }
    }
}

/// Validate Extrude selection members against their resolved sketch geometry.
pub(super) fn validate_extrude_selection_members(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let groups_by_index = &ctx.groups_by_index;
    let mut member_slots = HashSet::new();
    let mut member_records = HashSet::new();
    let finalized_history = history::projection_was_finalized(&native.asm_histories);
    let expected_histories = (!finalized_history).then(|| {
        history::selection::extrude_selection_identity_kinds(
            &native.design_extrude_selection_members,
            &native.design_component_naming_spaces,
            &native.design_body_bindings,
            &native.asm_histories,
        )
    });
    for (member_ordinal, member) in native.design_extrude_selection_members.iter().enumerate() {
        let native_stream = design_stream(&member.id);
        let group = groups_by_index.get(&(native_stream, member.group_record_index));
        let header = records_by_index.get(&(native_stream, member.record_index()));
        let selected_profile = group
            .and_then(|group| scopes_by_index.get(&(native_stream, group.scope_record_index)))
            .and_then(|scope| scope.extrude_profile());
        let selected_sketch =
            selected_profile.and_then(|profile| u32::try_from(profile.entity_id.suffix()).ok());
        let point_targets = native.sketch_points.iter().filter_map(|point| {
            (selected_sketch.is_some()
                && design_stream(&point.id) == native_stream
                && (point.owner_reference.is_none() || point.owner_reference == selected_sketch)
                && point.persistent_id() == Some(member.local_id))
            .then_some(records::sketch_relations::SketchRelationOperand::Point {
                record_index: point.record_index,
                persistent_id: point.persistent_id(),
            })
        });
        let curve_targets = native.sketch_curve_identities.iter().filter_map(|curve| {
            (selected_sketch.is_some()
                && design_stream(&curve.id) == native_stream
                && (curve.owner_reference.is_none() || curve.owner_reference == selected_sketch)
                && (curve.primary_id.get() == member.local_id
                    || curve.secondary_id != 0 && curve.secondary_id == member.local_id))
                .then_some(records::sketch_relations::SketchRelationOperand::Curve {
                    record_index: curve.record_index,
                    primary_id: curve.primary_id.get(),
                    secondary_id: curve.secondary_id,
                })
        });
        let mut targets = point_targets.chain(curve_targets);
        let first_target = targets.next();
        let expected_target = if targets.next().is_none() {
            first_target
        } else {
            None
        };
        let mut expected_identities = native
            .design_construction_operand_identities
            .iter()
            .filter(|identity| {
                design_stream(&identity.id) == native_stream
                    && identity.following_record_index() == member.record_index()
                    && identity.following_byte_offset() == member.byte_offset()
                    && identity.persistent_identity().is_some_and(|persistent| {
                        persistent.local_id == member.local_id
                            && persistent.asset_id == member.asset_id
                            && persistent.context_id == member.context_id
                    })
            })
            .collect::<Vec<_>>();
        expected_identities.sort_by(|left, right| {
            left.wrappers()
                .first()
                .map(|wrapper| wrapper.byte_offset)
                .cmp(&right.wrappers().first().map(|wrapper| wrapper.byte_offset))
        });
        let history_matches = if finalized_history {
            if let Some(binding) = member.historical.as_ref() {
                binding
                    .state_ids
                    .iter()
                    .copied()
                    .collect::<std::collections::HashSet<_>>()
                    .len()
                    == binding.state_ids.len()
                    && binding.state_ids.iter().all(|state_id| {
                        native
                            .asm_histories
                            .iter()
                            .flat_map(crate::history_records::AsmHistory::states)
                            .any(|state| state.state_id == *state_id)
                    })
            } else {
                true
            }
        } else {
            expected_histories
                .as_ref()
                .and_then(|expected| expected.get(member_ordinal))
                .and_then(Option::as_ref)
                .map(|(kind, entity_ref, states)| (*kind, *entity_ref, states.as_slice()))
                == member.historical.as_ref().map(|binding| {
                    (
                        binding.kind,
                        binding.entity_ref,
                        binding.state_ids.as_slice(),
                    )
                })
        };
        let terminal_next = member.next_record_index == 0
            && !records_by_index.values().any(|header| {
                design_stream(&header.id) == native_stream
                    && header.byte_offset == member.next_byte_offset()
            });
        let valid = group.is_some_and(|group| {
            usize::try_from(member.group_member_ordinal)
                .ok()
                .and_then(|ordinal| group.members().get(ordinal))
                .map(|reference| reference.value)
                == Some(member.record_index())
        }) && header.is_some_and(|header| {
            header.byte_offset == member.byte_offset() && header.class_tag == member.class_tag
        }) && member.scope_tail.is_none_or(|tail| {
            group.is_some_and(|group| tail.scope_record_index == group.scope_record_index)
        }) && selected_profile
            .is_none_or(|profile| profile.asset_id == member.asset_id)
            && member.resolved_geometry == expected_target
            && member
                .operand_identity_ids
                .iter()
                .map(String::as_str)
                .eq(expected_identities
                    .iter()
                    .map(|identity| identity.id.as_str()))
            && history_matches
            && (member.next_record_index != 0 || terminal_next)
            && member_slots.insert((
                native_stream,
                member.group_record_index,
                member.group_member_ordinal,
            ))
            && member_records.insert((native_stream, member.record_index()));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design Extrude selection member has an invalid fixed frame",
                Some(member.id.clone()),
            );
        }
    }
}

/// Validate entity-selection operand nested frames.
pub(super) fn validate_entity_selection_operands(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let operand_groups_by_index = &ctx.operand_groups_by_index;
    let mut entity_selection_slots = HashSet::new();
    for operand in &native.design_entity_selection_operands {
        let native_stream = design_stream(&operand.id);
        let group = operand_groups_by_index.get(&(native_stream, operand.group_record_index));
        let header = records_by_index.get(&(native_stream, operand.record_index()));
        let valid = group.is_some_and(|group| {
            group.scope_record_index == operand.scope_record_index
                && usize::try_from(operand.group_member_ordinal)
                    .ok()
                    .and_then(|ordinal| group.members().get(ordinal).map(|member| &member.value))
                    == Some(&operand.record_index())
        }) && header.is_some_and(|header| {
            header.byte_offset == operand.byte_offset() && header.class_tag == *operand.class_tag()
        }) && (operand.sketch_point().is_none()
            || group.is_some_and(|group| {
                crate::design::decode::operands::entity_selections::is_loft_point_group(
                    group,
                    &native.design_parameter_scopes,
                )
            }))
            && entity_selection_slots.insert((
                native_stream,
                operand.group_record_index,
                operand.group_member_ordinal,
            ));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design entity-selection operand has an invalid nested frame",
                Some(operand.id.clone()),
            );
        }
    }
}

/// Report Extrude selection groups with missing or inconsistent members.
pub(super) fn validate_extrude_selection_group_members(
    ctx: &Ctx<'_, '_>,
    findings: &mut Vec<Finding>,
) {
    let native = ctx.native;
    let members_by_slot = &ctx.members_by_slot;
    for group in &native.design_extrude_selection_groups {
        let native_stream = design_stream(&group.id);
        if ctx
            .scopes_by_index
            .get(&(native_stream, group.scope_record_index))
            .is_some_and(|scope| scope.emboss_operand_construction().is_some())
        {
            continue;
        }
        let complete = (0..group.members().len()).all(|ordinal| {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                return false;
            };
            let Some(member) = members_by_slot.get(&(native_stream, group.record_index, ordinal))
            else {
                return false;
            };
            let next = usize::try_from(ordinal)
                .ok()
                .and_then(|ordinal| group.members().get(ordinal + 1));
            next.is_none_or(|next_record_index| {
                let Some(next_ordinal) = ordinal.checked_add(1) else {
                    return false;
                };
                let next_member =
                    members_by_slot.get(&(native_stream, group.record_index, next_ordinal));
                member.next_record_index == next_record_index.value
                    && next_member.is_some_and(|next_member| {
                        member.next_byte_offset() == next_member.byte_offset()
                    })
            })
        });
        let context_id = members_by_slot
            .get(&(native_stream, group.record_index, 0))
            .map(|member| member.context_id.as_str());
        let context_consistent = context_id.is_some_and(|context_id| {
            (0..group.members().len()).all(|ordinal| {
                u32::try_from(ordinal)
                    .ok()
                    .and_then(|ordinal| {
                        members_by_slot.get(&(native_stream, group.record_index, ordinal))
                    })
                    .is_some_and(|member| member.context_id.as_str() == context_id)
            })
        });
        if !(complete && context_consistent) {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design Extrude selection group has missing members",
                Some(group.id.clone()),
            );
        }
    }
}
