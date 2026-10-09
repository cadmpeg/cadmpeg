// SPDX-License-Identifier: Apache-2.0

use super::{group, identity, recipe_edge_operand, recipe_reference};
use super::super::{resolved_edge_group, scope_partition_edge_group_candidates, surface_patch_grouped_recipe_edges, SurfacePatchRecipeEdges};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{EdgeSelection, FeatureId};
use cadmpeg_ir::ids::EdgeId;

fn measure<T>(decode: impl FnOnce(&DecodeContext<'_>) -> T) -> (T, u64) {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = decode(&ctx);
    let Err(CodecError::ResourceLimit(usage)) = ctx.charge_work(
        policy.limits.max_work_units + 1, "measure completed edge text work",
    ) else { panic!("finite ceiling exposes actual cumulative work"); };
    assert_eq!(usage.dimension, ResourceDimension::WorkUnits);
    (result, usage.used)
}

fn exact_operand(left: &str, right: &str) -> super::DesignEdgeOperand {
    let mut operand = recipe_edge_operand(10, &[], &[]);
    let mut first = recipe_reference(&[]);
    first.candidate_edges.push(EdgeId::mint(left).unwrap());
    let mut second = recipe_reference(&[]);
    second.candidate_edges.push(EdgeId::mint(right).unwrap());
    operand.recipe_references = vec![first, second];
    operand
}

fn assert_recipe_refusal(operation: &str) {
    let operand = exact_operand("f3d:test:edge#17", "f3d:test:edge#17");
    crate::test_support::resource_refusal_at(ResourceDimension::WorkUnits, operation, 0,
        |ctx| surface_patch_grouped_recipe_edges(&[&operand], ctx));
}

#[test]
fn edge_text_recipe_operand_step_refuses_before_visit() {
    assert_recipe_refusal("scan F3D surface patch recipe operands");
}

#[test]
fn edge_text_recipe_reference_step_refuses_before_visit() {
    assert_recipe_refusal("scan F3D surface patch recipe references");
}

#[test]
fn edge_text_recipe_comparison_refuses_before_byte_pair() {
    assert_recipe_refusal("compare F3D surface patch recipe edge identity");
}

#[test]
fn edge_text_recipe_first_mismatch_skips_reference_and_operand_suffixes() {
    let first = "f3d:test:edge#1000";
    let short = exact_operand(first, "f3d:test:edge#2000");
    let mut long = short.clone();
    long.recipe_references.extend((0..1024).map(|_| recipe_reference(&[17])));
    let unvisited = recipe_edge_operand(11, &[], &[]);
    let short = measure(|ctx| surface_patch_grouped_recipe_edges(&[&short], ctx).unwrap());
    let long = measure(|ctx| surface_patch_grouped_recipe_edges(&[&long, &unvisited], ctx).unwrap());
    assert_eq!(short.0, SurfacePatchRecipeEdges::Inconclusive);
    assert_eq!(long, short);
    // One operand, two references, then the shared prefix plus its first mismatch.
    assert_eq!(short.1, 3 + u64::try_from("f3d:test:edge#".len()).unwrap() + 1);
}

#[test]
fn edge_text_recipe_late_mismatch_pays_only_reached_pairs() {
    let early = exact_operand("f3d:test:edge#1000", "f3d:test:edge#2000");
    let late = exact_operand("f3d:test:edge#1000", "f3d:test:edge#1001");
    let early = measure(|ctx| surface_patch_grouped_recipe_edges(&[&early], ctx).unwrap());
    let late = measure(|ctx| surface_patch_grouped_recipe_edges(&[&late], ctx).unwrap());
    assert_eq!(early.0, SurfacePatchRecipeEdges::Inconclusive);
    assert_eq!(late.0, early.0);
    assert_eq!(late.1 - early.1, 3);
}

#[test]
fn edge_text_recipe_unequal_lengths_use_the_fixed_length_gate() {
    let operand = exact_operand("f3d:test:edge#1", "f3d:test:edge#2000");
    let result = measure(|ctx| surface_patch_grouped_recipe_edges(&[&operand], ctx).unwrap());
    assert_eq!(result, (SurfacePatchRecipeEdges::Inconclusive, 3));
}

#[test]
fn edge_text_recipe_empty_reference_prefix_is_visited_before_exact_proof() {
    let short = exact_operand("f3d:test:edge#1000", "f3d:test:edge#2000");
    let mut prefixed = short.clone();
    prefixed.recipe_references.insert(0, recipe_reference(&[]));
    let short = measure(|ctx| surface_patch_grouped_recipe_edges(&[&short], ctx).unwrap());
    let prefixed = measure(|ctx| surface_patch_grouped_recipe_edges(&[&prefixed], ctx).unwrap());
    assert_eq!(prefixed.0, short.0);
    assert_eq!(prefixed.1, short.1 + 1);
}

fn historical(combined: bool, ctx: &DecodeContext<'_>) -> Result<EdgeSelection, CodecError> {
    let mut selection_group = group(2, 10);
    selection_group.try_set_members(vec![
        crate::records::identity::Located { value: 10, offset: 0 },
        crate::records::identity::Located { value: 11, offset: 11 },
    ]).unwrap();
    let mut first = recipe_edge_operand(10, &[], &[]);
    first.resolved_edge_slot = Some(17);
    let mut second = recipe_edge_operand(11, &[18], &[]);
    let identities = if combined {
        second.terminal_reference_edge_slots = vec![vec![18], vec![19]];
        let first_identity = identity(10, &[]);
        let mut second_identity = identity(11, &[]);
        second_identity.resolved_edge_slot = Some(18);
        vec![first_identity, second_identity]
    } else {
        second.resolved_edge_slot = Some(18);
        Vec::new()
    };
    let feature = FeatureId::mint("f3d:model:feature#text-historical").unwrap();
    resolved_edge_group(&selection_group, std::slice::from_ref(&selection_group),
        &[first, second], &identities, Some(7), &feature, ctx)
}

fn assert_historical_refusal(combined: bool, operation: &str) {
    crate::test_support::resource_refusal_at(ResourceDimension::WorkUnits, operation, 0,
        |ctx| historical(combined, ctx));
    crate::test_support::with_decode_context(|ctx| {
        let EdgeSelection::Historical { edges, .. } = historical(combined, ctx).unwrap()
        else { panic!("complete native proof produces historical edges"); };
        let prefix = crate::ids::history_input_prefix(&cadmpeg_ir::identity_key!("text-historical"), 7);
        assert_eq!(edges.as_slice(), [
            crate::ids::history_input_edge_id(&prefix, 17),
            crate::ids::history_input_edge_id(&prefix, 18),
        ]);
    });
}

#[test]
fn edge_text_combined_historical_membership_step_refuses_before_visit() {
    assert_historical_refusal(true, "scan F3D combined historical edge identities");
}

#[test]
fn edge_text_combined_historical_equality_refuses_before_pair() {
    assert_historical_refusal(true, "compare F3D combined historical edge identity");
}

#[test]
fn edge_text_resolved_historical_membership_step_refuses_before_visit() {
    assert_historical_refusal(false, "scan F3D resolved historical edge identities");
}

#[test]
fn edge_text_resolved_historical_equality_refuses_before_pair() {
    assert_historical_refusal(false, "compare F3D resolved historical edge identity");
}

fn partition_ids(target: &str, candidate: &str, ctx: &DecodeContext<'_>) -> Result<Option<Vec<i64>>, CodecError> {
    let mut target_group = group(2, 10);
    target_group.id = target.into();
    let mut candidate_group = group(3, 11);
    candidate_group.id = candidate.into();
    scope_partition_edge_group_candidates(&target_group, &[candidate_group], &[], target_group.members(), ctx)
}

#[test]
fn edge_text_partition_group_step_refuses_before_visit() {
    crate::test_support::resource_refusal_at(ResourceDimension::WorkUnits,
        "scan F3D partition scope groups", 0,
        |ctx| partition_ids("f3d:test:group-2", "f3d:test:group-3", ctx));
}

#[test]
fn edge_text_partition_identity_refuses_before_pair() {
    crate::test_support::resource_refusal_at(ResourceDimension::WorkUnits,
        "compare F3D partition group identity", 0,
        |ctx| partition_ids("f3d:test:group-2", "f3d:test:group-3", ctx));
}

#[test]
fn edge_text_partition_identity_stops_at_first_mismatch() {
    let early = measure(|ctx| partition_ids("f3d:test:group-1000", "f3d:test:group-2000", ctx).unwrap());
    let late = measure(|ctx| partition_ids("f3d:test:group-1000", "f3d:test:group-1001", ctx).unwrap());
    assert_eq!(early.0, None);
    assert_eq!(late.0, early.0);
    assert_eq!(early.1, 1 + u64::try_from("f3d:test:group-".len()).unwrap() + 1);
    assert_eq!(late.1 - early.1, 3);
}

#[test]
fn edge_text_partition_target_identity_is_compared_once_per_group() {
    let id = "f3d:test:group-1000";
    let result = measure(|ctx| partition_ids(id, id, ctx).unwrap());
    assert_eq!(result, (None, 1 + u64::try_from(id.len()).unwrap()));
}

#[test]
fn edge_text_public_recipe_caller_refuses_at_progressive_equality() {
    let selection_group = group(2, 10);
    let operand = exact_operand("f3d:test:edge#1000", "f3d:test:edge#2000");
    let feature = FeatureId::mint("f3d:model:feature#text-recipe").unwrap();
    crate::test_support::resource_refusal_at(ResourceDimension::WorkUnits,
        "compare F3D surface patch recipe edge identity", 0, |ctx| {
            super::super::resolved_surface_patch_edge_group(&selection_group,
                std::slice::from_ref(&selection_group), std::slice::from_ref(&operand), &[],
                Some(7), &feature, ctx)
        });
    crate::test_support::with_decode_context(|ctx| {
        let selection = super::super::resolved_surface_patch_edge_group(&selection_group,
            std::slice::from_ref(&selection_group), &[operand], &[], Some(7), &feature, ctx).unwrap();
        assert!(matches!(selection, EdgeSelection::Native(id) if id == selection_group.id));
    });
}

#[test]
fn edge_text_all_owners_preserve_the_original_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "original edge text refusal")
    else { panic!("zero ceiling refuses the first actual unit"); };
    let operand = exact_operand("f3d:test:edge#17", "f3d:test:edge#17");
    assert!(matches!(surface_patch_grouped_recipe_edges(&[&operand], &ctx),
        Err(CodecError::ResourceLimit(later)) if later == original));
    assert!(matches!(partition_ids("f3d:test:group-2", "f3d:test:group-3", &ctx),
        Err(CodecError::ResourceLimit(later)) if later == original));
    for combined in [false, true] {
        assert!(matches!(historical(combined, &ctx),
            Err(CodecError::ResourceLimit(later)) if later == original));
    }
    assert_eq!(ctx.resource_refusal(), Some(original));
}
