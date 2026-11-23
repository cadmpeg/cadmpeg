// SPDX-License-Identifier: Apache-2.0
//! Validation of edge operands, face boundaries, and edge treatment groups.

use super::operand_groups::recipe_reference_frames_match;
use super::{
    design_stream, native_edge_recipe_faces, push_constant_finding, reload_native_arena, Ctx,
};
use crate::{design, history, records};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::check::{Check, Finding};
use cadmpeg_ir::report::Severity;
use std::collections::{HashMap, HashSet};

/// Validate edge operands and their recipe frames; returns their record set.
pub(super) fn validate_edge_operands<'a>(
    decode: &DecodeContext<'_>,
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> Result<HashSet<(&'a str, u32)>, CodecError> {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let recipes_by_id = &ctx.recipes_by_id;
    let scopes_by_index = &ctx.scopes_by_index;
    let historical_candidates_retained = history::projection_was_finalized(&native.asm_histories)
        || ctx.ir.fragment && native.asm_histories.is_empty();
    let mut edge_operand_slots = HashSet::new();
    let mut edge_operand_records = HashSet::new();
    let mut expected_edge_operands = reload_native_arena(decode, ctx.ir, "design_edge_operands")?;
    let scope_histories = history::scope_histories::bind_scope_histories(
        &native.design_parameter_scopes,
        &native.design_body_bindings,
        &native.design_body_recipe_operands,
        native_edge_recipe_faces(native),
        &native.asm_histories,
        &mut Vec::new(),
    );
    let expected_face_boundaries =
        history::edge_operand_history::bind_edge_operand_history_candidates(
            &mut expected_edge_operands,
            &native.design_parameter_scopes,
            &native.construction_recipes,
            history::binding_histories::BindingHistories::Shared(&native.asm_histories),
            &scope_histories,
        )?;
    validate_face_boundaries(
        ctx,
        findings,
        (!historical_candidates_retained).then_some(expected_face_boundaries.as_slice()),
    );
    let expected_edge_operands = expected_edge_operands
        .iter()
        .map(|operand| (operand.id.as_str(), operand))
        .collect::<std::collections::HashMap<_, _>>();
    for operand in &native.design_edge_operands {
        let native_stream = design_stream(&operand.id);
        let scope = scopes_by_index.get(&(native_stream, operand.scope_record_index));
        let header = records_by_index.get(&(native_stream, operand.record_index()));
        let recipe = recipes_by_id.get(operand.recipe_id.as_str());
        let design_reference = recipe
            .and_then(|recipe| recipe.record_index)
            .map(|record_index| i64::from(record_index.value))
            .filter(|value| *value >= 0);
        let expected_faces = match design_reference {
            Some(design_reference) => {
                design::decode::operands::operand_candidates::edge_operand_candidate_faces(
                    design_reference,
                    &native.persistent_subentity_tags,
                    Some(&operand.id),
                )
            }
            None => Vec::new(),
        };
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
        let expected_surface_patch_recipe_structure = scope
            .filter(|scope| {
                scope.kind() == crate::records::feature::scope::DesignFeatureKind::SurfacePatch
            })
            .and_then(|_| {
                design::decode::operands::recipe_structure::surface_patch_recipe_structure(
                    &operand.recipe_program,
                    operand.recipe_references.len(),
                )
            });
        let terminal_group_member = native
            .design_construction_operand_groups
            .iter()
            .any(|group| {
                design_stream(&group.id) == native_stream
                    && group.scope_record_index == operand.scope_record_index
                    && group.members().last().map(|member| &member.value)
                        == Some(&operand.record_index())
            });
        let valid = scope.is_some_and(|scope| {
            design::decode::operands::edge_operands::has_edge_recipe_operands(&scope.kind())
                && usize::try_from(operand.scope_reference_ordinal)
                    .ok()
                    .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                    == Some(&operand.record_index())
        }) && header.is_some_and(|header| {
            header.byte_offset == operand.byte_offset() && header.class_tag == operand.class_tag
        }) && (operand
            .record_index()
            .checked_add(scope.map_or(4, |scope| {
                design::decode::operands::edge_operands::edge_recipe_terminal_delta(&scope.kind())
            }))
            .is_some_and(|expected_offset| operand.next_record_index == expected_offset)
            || terminal_group_member)
            && operand
                .recipe_prefix_offset()
                .checked_add(u64_from_index(operand.recipe_prefix_bytes.len()))
                .zip(recipe.and_then(|recipe| recipe.byte_offset.checked_sub(4)))
                .is_some_and(|(prefix_end, recipe_prefix_end)| prefix_end == recipe_prefix_end)
            && recipe_reference_frames_match(
                &operand.recipe_references,
                &expected_references,
                historical_candidates_retained,
            )
            && recipe.is_some_and(|recipe| {
                design_stream(&recipe.id) == native_stream
                    && recipe.kind == crate::records::recipes::ConstructionRecipeKind::Edge
                    && recipe.byte_offset > operand.recipe_record_byte_offset()
                    && recipe.byte_offset < operand.next_byte_offset()
            })
            && design::decode::operands::recipe_structure::edge_recipe_structure(
                ctx.decode,
                &operand.recipe_program,
            )? == operand.recipe_structure
            && expected_surface_patch_recipe_structure == operand.surface_patch_recipe_structure
            && (historical_candidates_retained || expected_faces == operand.candidate_faces)
            && (ctx.ir.fragment && native.asm_histories.is_empty()
                || expected_edge_operands.get(operand.id.as_str()) == Some(&operand));
        let valid = valid
            && edge_operand_slots.insert((
                native_stream,
                operand.scope_record_index,
                operand.scope_reference_ordinal,
            ))
            && edge_operand_records.insert((native_stream, operand.record_index()));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design edge operand has an invalid scope or recipe frame",
                Some(operand.id.clone()),
            );
        }
    }
    Ok(edge_operand_records)
}

/// Validate `asm_face_boundaries` and every edge-operand reference to them.
///
/// Each boundary belongs to one delta state, carries the identity derived from
/// that state and its face slot, and is named by at least one reference
/// context. Each context names result-state boundaries of its result faces and
/// preceding-state boundaries of its preceding and support faces, in face
/// order. `expected` holds the boundaries re-derived from retained historical
/// topology, when the projection still retains it.
pub(super) fn validate_face_boundaries<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    expected: Option<&[records::topology::historical_context::AsmFaceBoundary]>,
) {
    use records::topology::historical_context::AsmFaceBoundary;
    let native = ctx.native;
    if ctx.ir.fragment && native.asm_histories.is_empty() && !native.design_edge_operands.is_empty()
    {
        findings.push(Finding {
            check: Check::FragmentSkipped,
            severity: Severity::Info,
            entity: None,
            related: Vec::new(),
            message:
                "skipped f3d.historical_face_boundaries: cited ASM states are outside this fragment"
                    .into(),
        });
        return;
    }
    let mut states = HashMap::new();
    for history in &native.asm_histories {
        for state in history.states() {
            states.insert(state.id.as_str(), state);
        }
    }
    let mut referenced = HashSet::new();
    for operand in &native.design_edge_operands {
        let stream = design_stream(&operand.id);
        let result_state_id = ctx
            .scopes_by_index
            .get(&(stream, operand.scope_record_index))
            .and_then(|scope| scope.history_state_id());
        let mut result_parent = None;
        let mut preceding_parent = None;
        let mut valid = true;
        let mut check =
            |ids: &'a [String], slots: &[i64], parent: &mut Option<&'a str>, state_id| {
                let mut remaining = slots.iter();
                for id in ids {
                    referenced.insert(id.as_str());
                    let boundary = AsmFaceBoundary::find(&native.asm_face_boundaries, id);
                    let state = boundary.and_then(|boundary| states.get(boundary.parent.as_str()));
                    valid &= boundary.zip(state).is_some_and(|(boundary, state)| {
                        Some(state.state_id) == state_id
                            && *parent.get_or_insert(boundary.parent.as_str()) == boundary.parent
                            && remaining.any(|slot| *slot == boundary.face_slot)
                    });
                }
            };
        for context in &operand.recipe_reference_contexts {
            let slots = |faces: &[cadmpeg_ir::ids::FaceId]| {
                faces
                    .iter()
                    .filter_map(|face| history::body_closures::stable_ref(face.as_str()))
                    .collect::<Vec<_>>()
            };
            check(
                &context.result_face_boundary_ids,
                &slots(&context.result_faces),
                &mut result_parent,
                result_state_id,
            );
            check(
                &context.preceding_face_boundary_ids,
                &slots(&context.preceding_faces),
                &mut preceding_parent,
                operand.recipe_state_id,
            );
            check(
                &context.preceding_support_face_boundary_ids,
                &context.preceding_support_face_slots,
                &mut preceding_parent,
                operand.recipe_state_id,
            );
        }
        valid &= result_parent
            .zip(preceding_parent)
            .is_none_or(|(result, preceding)| {
                states
                    .get(result)
                    .zip(states.get(preceding))
                    .is_some_and(|(result, preceding)| result.parent == preceding.parent)
            });
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design edge operand names an invalid historical face boundary",
                Some(operand.id.clone()),
            );
        }
    }
    let expected = expected.map(|expected| {
        expected
            .iter()
            .map(|boundary| (boundary.id.as_str(), boundary))
            .collect::<HashMap<_, _>>()
    });
    for boundary in &native.asm_face_boundaries {
        let valid = states.get(boundary.parent.as_str()).is_some_and(|state| {
            AsmFaceBoundary::identity(&state.id, state.byte_offset, boundary.face_slot).as_deref()
                == Some(boundary.id.as_str())
        }) && referenced.contains(boundary.id.as_str())
            && expected
                .as_ref()
                .is_none_or(|expected| expected.get(boundary.id.as_str()) == Some(&boundary));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion ASM face boundary has an invalid state, identity, or reference",
                Some(boundary.id.clone()),
            );
        }
    }
}

pub(super) fn validate_edge_treatment_vertex_operands<'a>(
    decode: &DecodeContext<'_>,
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> Result<HashSet<(&'a str, u32)>, CodecError> {
    let native = ctx.native;
    let mut expected = reload_native_arena::<
        records::feature::work_geometry::DesignEdgeTreatmentVertexOperand,
    >(decode, ctx.ir, "design_edge_treatment_vertex_operands")?;
    for operand in &mut expected {
        for reference in &mut operand.recipe.recipe_references {
            design::decode::dimension_frames::bind_recipe_reference_candidates(
                reference,
                &native.persistent_subentity_tags,
                Some(&operand.id),
            );
        }
    }
    let scope_histories = history::scope_histories::bind_scope_histories(
        &native.design_parameter_scopes,
        &native.design_body_bindings,
        &native.design_body_recipe_operands,
        native_edge_recipe_faces(native),
        &native.asm_histories,
        &mut Vec::new(),
    );
    history::vertex_recipes::bind_edge_treatment_vertex_history(
        &mut expected,
        &native.design_parameter_scopes,
        &native.asm_histories,
        &scope_histories,
    )?;
    let expected = expected
        .iter()
        .map(|operand| (operand.id.as_str(), operand))
        .collect::<std::collections::HashMap<_, _>>();
    let mut records = HashSet::new();
    for operand in &native.design_edge_treatment_vertex_operands {
        let stream = design_stream(&operand.id);
        let scope = ctx
            .scopes_by_index
            .get(&(stream, operand.scope_record_index));
        let mut groups = native
            .design_construction_operand_groups
            .iter()
            .filter(|group| {
                design_stream(&group.id) == stream
                    && group.scope_record_index == operand.scope_record_index
                    && group.record_index == operand.group_record_index
            });
        let group = groups.next();
        let expected_id = crate::ids::native_scoped_id(
            stream,
            "edge-treatment-vertex-operand",
            operand.recipe.byte_offset(),
        );
        let valid = operand.id == expected_id
            && scope.is_some_and(|scope| {
                design::decode::operands::edge_operands::has_edge_recipe_operands(&scope.kind())
                    && usize::try_from(operand.scope_reference_ordinal)
                        .ok()
                        .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                        == Some(&operand.recipe.record_index())
            })
            && group.is_some_and(|group| {
                usize::try_from(operand.group_member_ordinal)
                    .ok()
                    .and_then(|ordinal| group.members().get(ordinal).map(|member| &member.value))
                    == Some(&operand.recipe.record_index())
            })
            && groups.next().is_none()
            && expected.get(operand.id.as_str()) == Some(&operand);
        let valid = valid && records.insert((stream, operand.recipe.record_index()));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion edge-treatment vertex operand has an invalid group or recipe frame",
                Some(operand.id.clone()),
            );
        }
    }
    Ok(records)
}

/// Report Fillet/Chamfer edge groups with incomplete selection operands. The
/// groups are design-intent records, so a document without them has nothing
/// to check.
pub(super) fn validate_edge_treatment_groups<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    edge_operand_records: &HashSet<(&'a str, u32)>,
    edge_identity_records: &HashSet<(&'a str, u32)>,
    edge_treatment_vertex_records: &HashSet<(&'a str, u32)>,
) {
    if !ctx.holds_design_intent() {
        return;
    }
    let native = ctx.native;
    for scope in native.design_parameter_scopes.iter().filter(|scope| {
        matches!(
            scope.kind(),
            crate::records::feature::scope::DesignFeatureKind::Fillet
                | crate::records::feature::scope::DesignFeatureKind::Chamfer
        )
    }) {
        let native_stream = design_stream(&scope.id);
        let mut groups = native
            .design_construction_operand_groups
            .iter()
            .filter(|group| {
                design_stream(&group.id) == native_stream
                    && group.scope_record_index == scope.record_index
            })
            .peekable();
        let complete = groups.peek().is_some()
            && groups.all(|group| {
                let recipe_backed =
                    group
                        .members()
                        .iter()
                        .map(|member| &member.value)
                        .all(|member| {
                            edge_operand_records.contains(&(native_stream, *member))
                                || edge_treatment_vertex_records.contains(&(native_stream, *member))
                        });
                let identity_backed = group
                    .members()
                    .iter()
                    .map(|member| &member.value)
                    .all(|member| edge_identity_records.contains(&(native_stream, *member)));
                recipe_backed || identity_backed
            });
        if !complete {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design edge-treatment group has incomplete selection operands",
                Some(scope.id.clone()),
            );
        }
    }
}
