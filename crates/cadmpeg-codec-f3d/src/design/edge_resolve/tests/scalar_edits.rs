// SPDX-License-Identifier: Apache-2.0

use super::{identity, recipe_edge_operand, recipe_reference};
use super::super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn reference(edges: &[i64]) -> crate::records::topology::historical_context::DesignEdgeRecipeReferenceContext {
    serde_json::from_value(serde_json::json!({
        "reference_ordinal": 0, "result_faces": [], "result_shared_edge_slots": [],
        "preceding_faces": [], "shared_edge_slots": [], "changed_shared_edge_slots": [],
        "changed_reference_edge_slots": edges,
    })).unwrap()
}

fn boundary_operand(record: u32, deleted: &[i64], references: &[&[i64]]) -> DesignEdgeOperand {
    let mut operand = recipe_edge_operand(record, deleted, deleted);
    operand.preceding_boundary_edge_slots = deleted.to_vec();
    operand.recipe_structure = Some(crate::records::topology::edge_recipe::DesignEdgeRecipeStructure {
        root: 2, sides: Vec::new(),
    });
    operand.recipe_references = references.iter().map(|_| recipe_reference(&[])).collect();
    operand.recipe_reference_contexts = references.iter().map(|edges| reference(edges)).collect();
    operand
}

pub(super) fn run_case(owner: &str, ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
    match owner {
        "sorted_transition_slots" => sorted_transition_slots(&[18, 17, 17], ctx, "scalar transition fixture").map(drop),
        "transition_chain_is_supported_by_recipe" => {
            let first = recipe_edge_operand(10, &[17, 17], &[17]);
            let second = recipe_edge_operand(11, &[18, 18], &[18]);
            transition_chain_is_supported_by_recipe(&[17, 18], 2, &[&first, &second], ctx).map(drop)
        }
        "unique_hem_transition_edge_candidate" => unique_hem_transition_edge_candidate(
            &[63, 106, 110, 167, 63], [&[][..], &[106][..], &[110][..], &[167][..]], ctx,
        ).map(drop),
        "changed_reference_edge_group_candidates" => {
            let mut first = recipe_edge_operand(10, &[], &[]);
            first.recipe_reference_contexts = vec![reference(&[17, 17, 19]), reference(&[17, 18])];
            let mut second = recipe_edge_operand(11, &[], &[]);
            second.recipe_reference_contexts = vec![reference(&[18, 18]), reference(&[18, 20])];
            changed_reference_edge_group_candidates(&[&first, &second], ctx).map(drop)
        }
        "deleted_reference_edge_group_candidates" => {
            let first = boundary_operand(10, &[17], &[&[17, 17]]);
            let second = boundary_operand(11, &[18], &[&[18, 18]]);
            deleted_reference_edge_group_candidates(&[&first, &second], ctx).map(drop)
        }
        "unique_deleted_reference_assignment" => unique_deleted_reference_assignment(
            &[vec![17, 17], vec![18, 18]], &[vec![17], vec![18]], ctx,
        ).map(drop),
        "edge_group_assignment_candidates" => edge_group_assignment_candidates(
            &[], [&[18, 17, 17, 19][..], &[17, 18][..]].into_iter().map(Some), ctx,
        ).map(drop),
        "radius_edge_identity_group_candidates" => {
            let operand = identity(10, &[(17, 3.0), (17, 3.0), (18, 3.0)]);
            radius_edge_identity_group_candidates(&[&operand], 3.0, ctx).map(drop)
        }
        "unique_bipartite_assignment" => unique_bipartite_assignment(&[vec![17, 17], vec![18]], ctx).map(drop),
        "partition_unique_incomplete_edge_group" => {
            let deleted = [17, 18, 19, 20, 17];
            let member = |identity, resolved_edge| EdgeGroupMember { identity, resolved_edge, deleted_boundary_edges: &deleted };
            let groups = [vec![member(10, Some(17)), member(11, Some(19))], vec![member(12, None), member(13, None)]];
            partition_unique_incomplete_edge_group(1, &groups, ctx).map(drop)
        }
        "common_deleted_edge_group_candidates" => common_deleted_edge_group_candidates(
            [(true, &[18, 17, 17][..]), (true, &[17, 18, 18][..])], ctx,
        ).map(drop),
        "deleted_boundary_edge_group_candidates" => {
            let first = boundary_operand(10, &[17, 18, 17], &[&[17, 18, 17]]);
            let second = boundary_operand(11, &[17, 18, 18], &[&[17, 18, 18]]);
            deleted_boundary_edge_group_candidates(&[&first, &second], ctx).map(drop)
        }
        "contextual_deleted_edge_group_candidates" => {
            let first = boundary_operand(10, &[], &[&[17, 17]]);
            let second = boundary_operand(11, &[17, 18, 17], &[&[17], &[18]]);
            contextual_deleted_edge_group_candidates(&[&first, &second], ctx).map(drop)
        }
        "result_boundary_reference_edge_group_candidates" => {
            let mut operand = boundary_operand(10, &[], &[&[19, 19], &[19]]);
            operand.preceding_boundary_edge_slots = vec![17, 18];
            operand.result_boundary_edge_slots = vec![19, 20];
            operand.recipe_structure = Some(crate::records::topology::edge_recipe::DesignEdgeRecipeStructure {
                root: 2,
                sides: (0..2).map(|_| crate::records::topology::edge_recipe::DesignTopologyRecipeSide {
                    header_value: 2, scalars: vec![1, 0], payload_prefix: vec![0], entries: Vec::new(),
                }).collect(),
            });
            result_boundary_reference_edge_group_candidates(&[&operand], ctx).map(drop)
        }
        "changed_boundary_count_edge_group_candidates" => {
            let selector = |edges| crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext {
                selector: 0, clauses: vec![None, None], incidence_matching_edge_slots: Vec::new(), boundary_count_matching_edge_slots: edges,
            };
            let first = [selector(vec![18, 17, 17])];
            let second = [selector(vec![17, 18, 18])];
            changed_boundary_count_edge_group_candidates([first.as_slice(), second.as_slice()], ctx).map(drop)
        }
        _ => panic!("unknown owning scalar case"),
    }
}

fn assert_refusal(owner: &str, operation: &str) {
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits, operation, 0, |ctx| run_case(owner, ctx),
    );
    assert!(matches!(error, CodecError::ResourceLimit(original)
        if original.dimension == ResourceDimension::WorkUnits && original.operation == operation));
}

#[test]
fn edge_scalar_edits_refuse_sorted_transition_slots_sorted() {
    assert_refusal("sorted_transition_slots", "deduplicate F3D transition slots");
}

#[test]
fn edge_scalar_edits_refuse_transition_chain_is_supported_by_recipe_all_recipe_edges() {
    assert_refusal("transition_chain_is_supported_by_recipe", "deduplicate F3D transition recipe edges");
}

#[test]
fn edge_scalar_edits_refuse_unique_hem_transition_edge_candidate_support_edges() {
    assert_refusal("unique_hem_transition_edge_candidate", "deduplicate F3D Hem support edges");
}

#[test]
fn edge_scalar_edits_refuse_unique_hem_transition_edge_candidate_candidates() {
    assert_refusal("unique_hem_transition_edge_candidate", "deduplicate F3D Hem candidate edges");
}

#[test]
fn edge_scalar_edits_refuse_changed_reference_edge_group_candidates_retain() {
    assert_refusal("changed_reference_edge_group_candidates", "retain F3D changed reference candidates");
}

#[test]
fn edge_scalar_edits_refuse_changed_reference_edge_group_candidates_candidates() {
    assert_refusal("changed_reference_edge_group_candidates", "deduplicate F3D changed reference edges");
}

#[test]
fn edge_scalar_edits_refuse_deleted_reference_edge_group_candidates_candidates() {
    assert_refusal("deleted_reference_edge_group_candidates", "deduplicate F3D deleted reference edges");
}

#[test]
fn edge_scalar_edits_refuse_unique_deleted_reference_assignment_candidates() {
    assert_refusal("unique_deleted_reference_assignment", "deduplicate F3D deleted reference shared edges");
}

#[test]
fn edge_scalar_edits_refuse_edge_group_assignment_candidates_retain() {
    assert_refusal("edge_group_assignment_candidates", "retain F3D second assignment reference candidates");
}

#[test]
fn edge_scalar_edits_refuse_edge_group_assignment_candidates_candidates() {
    assert_refusal("edge_group_assignment_candidates", "deduplicate F3D assignment reference edges");
}

#[test]
fn edge_scalar_edits_refuse_radius_edge_identity_group_candidates_chain() {
    assert_refusal("radius_edge_identity_group_candidates", "deduplicate F3D radius identity edges");
}

#[test]
fn edge_scalar_edits_refuse_unique_bipartite_assignment_candidates() {
    assert_refusal("unique_bipartite_assignment", "deduplicate F3D normalized assignment edges");
}

#[test]
fn edge_scalar_edits_refuse_partition_unique_incomplete_edge_group_deleted() {
    assert_refusal("partition_unique_incomplete_edge_group", "deduplicate F3D partition deleted edges");
}

#[test]
fn edge_scalar_edits_refuse_common_deleted_edge_group_candidates_candidates() {
    assert_refusal("common_deleted_edge_group_candidates", "deduplicate F3D common deleted candidates");
}

#[test]
fn edge_scalar_edits_refuse_common_deleted_edge_group_candidates_normalized() {
    assert_refusal("common_deleted_edge_group_candidates", "deduplicate F3D common deleted normalized edges");
}

#[test]
fn edge_scalar_edits_refuse_deleted_boundary_edge_group_candidates_deleted() {
    assert_refusal("deleted_boundary_edge_group_candidates", "deduplicate F3D deleted boundary edges");
}

#[test]
fn edge_scalar_edits_refuse_deleted_boundary_edge_group_candidates_contextual() {
    assert_refusal("deleted_boundary_edge_group_candidates", "deduplicate F3D deleted boundary contextual edges");
}

#[test]
fn edge_scalar_edits_refuse_contextual_deleted_edge_group_candidates_deleted() {
    assert_refusal("contextual_deleted_edge_group_candidates", "deduplicate F3D contextual deleted edges");
}

#[test]
fn edge_scalar_edits_refuse_contextual_deleted_edge_group_candidates_candidates() {
    assert_refusal("contextual_deleted_edge_group_candidates", "deduplicate F3D contextual deleted candidates");
}

#[test]
fn edge_scalar_edits_refuse_result_boundary_reference_edge_group_candidates_candidates() {
    assert_refusal("result_boundary_reference_edge_group_candidates", "deduplicate F3D result boundary edges");
}

#[test]
fn edge_scalar_edits_refuse_changed_boundary_count_edge_group_candidates_candidates() {
    assert_refusal("changed_boundary_count_edge_group_candidates", "deduplicate F3D boundary count edges");
}

#[test]
fn edge_scalar_edits_original_refusal_precedes_each_owner() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(
        policy.limits.max_work_units + 1, "original edge scalar edit refusal")
    else { panic!("creating session refuses excess work"); };
    assert!(matches!(run_case("changed_boundary_count_edge_group_candidates", &ctx), Err(CodecError::ResourceLimit(later)) if later == original));
    assert!(matches!(run_case("changed_reference_edge_group_candidates", &ctx), Err(CodecError::ResourceLimit(later)) if later == original));
    assert!(matches!(run_case("common_deleted_edge_group_candidates", &ctx), Err(CodecError::ResourceLimit(later)) if later == original));
    assert!(matches!(run_case("contextual_deleted_edge_group_candidates", &ctx), Err(CodecError::ResourceLimit(later)) if later == original));
    assert!(matches!(run_case("deleted_boundary_edge_group_candidates", &ctx), Err(CodecError::ResourceLimit(later)) if later == original));
    assert!(matches!(run_case("deleted_reference_edge_group_candidates", &ctx), Err(CodecError::ResourceLimit(later)) if later == original));
    assert!(matches!(run_case("edge_group_assignment_candidates", &ctx), Err(CodecError::ResourceLimit(later)) if later == original));
    assert!(matches!(run_case("partition_unique_incomplete_edge_group", &ctx), Err(CodecError::ResourceLimit(later)) if later == original));
    assert!(matches!(run_case("radius_edge_identity_group_candidates", &ctx), Err(CodecError::ResourceLimit(later)) if later == original));
    assert!(matches!(run_case("result_boundary_reference_edge_group_candidates", &ctx), Err(CodecError::ResourceLimit(later)) if later == original));
    assert!(matches!(run_case("sorted_transition_slots", &ctx), Err(CodecError::ResourceLimit(later)) if later == original));
    assert!(matches!(run_case("transition_chain_is_supported_by_recipe", &ctx), Err(CodecError::ResourceLimit(later)) if later == original));
    assert!(matches!(run_case("unique_bipartite_assignment", &ctx), Err(CodecError::ResourceLimit(later)) if later == original));
    assert!(matches!(run_case("unique_deleted_reference_assignment", &ctx), Err(CodecError::ResourceLimit(later)) if later == original));
    assert!(matches!(run_case("unique_hem_transition_edge_candidate", &ctx), Err(CodecError::ResourceLimit(later)) if later == original));
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn edge_scalar_edits_native_candidate_order_and_unvisited_membership_suffix_are_preserved() {
    let short = [17, 18];
    let mut long = short.to_vec();
    long.extend(std::iter::repeat_n(i64::MAX, 1024));
    let marker = "after edge scalar membership";
    let CodecError::ResourceLimit(expected) = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits, marker, 0, |ctx| {
            assert_eq!(edge_group_assignment_candidates(&[], [&[18, 17, 17][..], &short[..]].into_iter().map(Some), ctx)?,
                Some(EdgeAssignmentCandidates::Edges(vec![17, 18])));
            ctx.charge_work(1, marker)
        },
    ) else { panic!("final scalar operation has a real work boundary"); };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = expected.limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(edge_group_assignment_candidates(&[], [&[18, 17, 17][..], &long[..]].into_iter().map(Some), &ctx).unwrap(),
        Some(EdgeAssignmentCandidates::Edges(vec![17, 18])));
    let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, marker)
    else { panic!("untouched suffix does not use extra work"); };
    assert_eq!(original, expected);
    assert_eq!(ctx.resource_refusal(), Some(original));
}
