// SPDX-License-Identifier: Apache-2.0

use super::{group, recipe_edge_operand};
use super::super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn run_case(owner: &str, ctx: &DecodeContext<'_>) -> Result<bool, CodecError> {
    match owner {
        "surface_patch" => surface_patch_grouped_recipe_edges(&[], ctx)
            .map(|value| value == SurfacePatchRecipeEdges::Absent),
        "hem_resolved" => {
            let mut operand = recipe_edge_operand(10, &[], &[]);
            operand.resolved_edge_slot = Some(17);
            resolved_hem_edge_slot(&operand, None, ctx).map(|value| value == Some(17))
        }
        "hem_without_previous" => resolved_hem_edge_slot(
            &recipe_edge_operand(10, &[], &[]), None, ctx,
        ).map(|value| value.is_none()),
        "hem_transition" => hem_transition_edge_slot(&recipe_edge_operand(10, &[], &[]), ctx)
            .map(|value| value.is_none()),
        "unique_group" => unique_edge_group_assignment(&[], ctx).map(|value| value.is_none()),
        "unique_context" => unique_edge_assignment_with_context(&[EdgeAssignmentCandidates::Context], ctx)
            .map(|value| value.is_none()),
        "assignment" => edge_assignment_candidates(&[], std::iter::empty::<Option<&[i64]>>(), ctx)
            .map(|value| value.is_none()),
        "partition" => {
            let mut target = group(2, 10);
            target.id = "unqualified".into();
            scope_partition_edge_group_candidates(&target, &[], &[], &[], ctx)
                .map(|value| value.is_none())
        }
        "terminal" => primary_terminal_reference_shared_edge(&recipe_edge_operand(10, &[], &[]), ctx)
            .map(|value| value.is_none()),
        "set_intersection" => unique_edge_set_intersection(ctx, &[])
            .map(|value| value == EdgeSetIntersection::Disjoint),
        "selector_intersection" => corroborated_edge_intersection(ctx, &[], &[], SelectorSlots::Incidence)
            .map(|value| value.is_none()),
        "selector_candidates" => corroborated_edge_candidates(
            &[], std::iter::empty::<Option<&[i64]>>(), SelectorSlots::Incidence, ctx,
        ).map(|value| value.is_none()),
        "selector_empty_candidates" => {
            let selector = crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext {
                selector: 0, clauses: Vec::new(), incidence_matching_edge_slots: Vec::new(),
                boundary_count_matching_edge_slots: Vec::new(),
            };
            corroborated_edge_candidates(&[selector], std::iter::empty::<Option<&[i64]>>(), SelectorSlots::Incidence, ctx)
                .map(|value| value.is_none())
        }
        "fixed_fillet" => {
            let scope = DesignParameterScope::empty("f3d:Design/BulkStream.dat:scope#42",
                crate::records::feature::scope::DesignFeatureKind::Hem, 42);
            project_fixed_fillet_with_corners(&scope, &[], &[], &[], &[], &[], ctx)
                .map(|value| value.is_none())
        }
        _ => panic!("unknown owning empty edge gate"),
    }
}

fn assert_free_and_original_refusal(owner: &str) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(run_case(owner, &ctx).unwrap());
    assert_eq!(ctx.resource_refusal(), None);
    let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "original empty edge refusal")
    else { panic!("zero work ceiling refuses one actual work unit"); };
    assert_eq!(original.dimension, ResourceDimension::WorkUnits);
    assert!(matches!(run_case(owner, &ctx), Err(CodecError::ResourceLimit(later)) if later == original));
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn edge_empty_surface_patch_is_free_and_preserves_original_refusal() {
    assert_free_and_original_refusal("surface_patch");
}

#[test]
fn edge_empty_hem_resolved_is_free_and_preserves_original_refusal() {
    assert_free_and_original_refusal("hem_resolved");
}

#[test]
fn edge_empty_hem_without_previous_is_free_and_preserves_original_refusal() {
    assert_free_and_original_refusal("hem_without_previous");
}

#[test]
fn edge_empty_hem_transition_is_free_and_preserves_original_refusal() {
    assert_free_and_original_refusal("hem_transition");
}

#[test]
fn edge_empty_unique_group_is_free_and_preserves_original_refusal() {
    assert_free_and_original_refusal("unique_group");
}

#[test]
fn edge_empty_unique_context_is_free_and_preserves_original_refusal() {
    assert_free_and_original_refusal("unique_context");
}

#[test]
fn edge_empty_assignment_is_free_and_preserves_original_refusal() {
    assert_free_and_original_refusal("assignment");
}

#[test]
fn edge_empty_partition_is_free_and_preserves_original_refusal() {
    assert_free_and_original_refusal("partition");
}

#[test]
fn edge_empty_terminal_is_free_and_preserves_original_refusal() {
    assert_free_and_original_refusal("terminal");
}

#[test]
fn edge_empty_set_intersection_is_free_and_preserves_original_refusal() {
    assert_free_and_original_refusal("set_intersection");
}

#[test]
fn edge_empty_selector_intersection_is_free_and_preserves_original_refusal() {
    assert_free_and_original_refusal("selector_intersection");
}

#[test]
fn edge_empty_selector_candidates_is_free_and_preserves_original_refusal() {
    assert_free_and_original_refusal("selector_candidates");
}

#[test]
fn edge_empty_selector_empty_candidates_is_free_and_preserves_original_refusal() {
    assert_free_and_original_refusal("selector_empty_candidates");
}

#[test]
fn edge_empty_fixed_fillet_is_free_and_preserves_original_refusal() {
    assert_free_and_original_refusal("fixed_fillet");
}

fn assert_public_original_refusal(owner: &str) {
    let selection_group = group(2, 10);
    let feature = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#empty-gate").unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "original public edge refusal")
    else { panic!("zero work ceiling refuses one actual work unit"); };
    let result = match owner {
        "surface_patch" => resolved_surface_patch_edge_group(
            &selection_group, std::slice::from_ref(&selection_group), &[], &[], None, &feature, &ctx,
        ),
        "treatment" => resolved_edge_treatment_group_with_corners(
            &selection_group, EdgeTreatmentInputs {
                groups: std::slice::from_ref(&selection_group), operands: &[], identity_operands: &[],
                vertex_operands: &[], histories: &[], previous_state_id: None,
                feature_id: &feature, treatment_radius: None,
            }, &ctx,
        ),
        _ => panic!("unknown public edge gate"),
    };
    assert!(matches!(result, Err(CodecError::ResourceLimit(later)) if later == original));
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn edge_empty_public_surface_patch_preserves_original_refusal() {
    assert_public_original_refusal("surface_patch");
}

#[test]
fn edge_empty_public_treatment_preserves_original_refusal() {
    assert_public_original_refusal("treatment");
}
