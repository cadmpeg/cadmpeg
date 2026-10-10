// SPDX-License-Identifier: Apache-2.0

use super::super::*;
use super::{group, identity, recipe_edge_operand};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn flange_operand() -> DesignEdgeOperand {
    let mut operand = recipe_edge_operand(10, &[], &[99]);
    operand.updated_boundary_edge_slots = vec![17];
    operand.preceding_boundary_edge_slots = vec![17];
    operand.changed_boundary_edge_slots = vec![17];
    operand.result_boundary_edge_slots = vec![17];
    operand
}

fn flange_group_operand() -> DesignEdgeOperand {
    let mut operand = recipe_edge_operand(10, &[], &[]);
    operand.preceding_boundary_edge_slots = vec![17, 18, 19];
    operand.changed_boundary_edge_slots = vec![17, 18];
    operand.updated_boundary_edge_slots = vec![17];
    operand.result_boundary_edge_slots = vec![17, 20];
    operand
}

fn run_case(owner: &str, ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
    match owner {
        "identity_uniform" => {
            let mut selection_group = group(2, 10);
            selection_group.try_set_members(vec![
                crate::records::identity::Located { value: 10, offset: 0 },
                crate::records::identity::Located { value: 11, offset: 11 },
            ]).unwrap();
            let operand = identity(10, &[(17, 3.0)]);
            let second = identity(11, &[(17, 3.0)]);
            let feature =
                cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#main-edge-group").unwrap();
            resolved_edge_treatment_group_with_corners(
                &selection_group,
                EdgeTreatmentInputs {
                    groups: std::slice::from_ref(&selection_group),
                    operands: &[],
                    identity_operands: &[operand, second],
                    vertex_operands: &[],
                    histories: &[],
                    previous_state_id: Some(7),
                    feature_id: &feature,
                    treatment_radius: None,
                },
                ctx,
            )
            .map(drop)
        }
        "flange" => edge_flange_updated_edge_candidate(&flange_operand(), ctx).map(drop),
        "flange_group" => {
            let selection_group = group(2, 10);
            let feature =
                cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#main-edge-group").unwrap();
            resolved_edge_flange_group(
                &selection_group,
                std::slice::from_ref(&selection_group),
                &[flange_group_operand()],
                &[],
                Some(7),
                &feature,
                ctx,
            )
            .map(drop)
        }
        "chain_resolved" => {
            let mut operand = recipe_edge_operand(10, &[17], &[17]);
            operand.resolved_edge_slot = Some(17);
            transition_chain_is_supported_by_recipe(&[17], 1, &[&operand], ctx).map(drop)
        }
        "partial" => partial_historical_edge_selection(
            [
                ("operand-a", Some(17)),
                ("operand-b", None),
                ("operand-c", Some(17)),
            ],
            41,
            "feature",
            cadmpeg_ir::ids::FeatureInputTopologyId::mint("f3d:history-input:state#feature")
                .unwrap(),
            "group",
            ctx,
        )
        .map(drop),
        "partition_target" => {
            let deleted = [17, 18, 19, 20];
            let member = |identity, resolved_edge| EdgeGroupMember {
                identity,
                resolved_edge,
                deleted_boundary_edges: &deleted,
            };
            let groups = [
                vec![member(10, Some(17)), member(11, Some(19))],
                vec![member(12, Some(18)), member(13, None)],
            ];
            partition_unique_incomplete_edge_group(1, &groups, ctx).map(drop)
        }
        "surface_patch" => {
            let mut selection_group = group(2, 10);
            selection_group
                .try_set_members(vec![
                    crate::records::identity::Located {
                        value: 10,
                        offset: 0,
                    },
                    crate::records::identity::Located {
                        value: 11,
                        offset: 11,
                    },
                ])
                .unwrap();
            let mut first = recipe_edge_operand(10, &[], &[]);
            first.resolved_edge_slot = Some(17);
            first.surface_patch_recipe_structure = Some(
                crate::records::topology::edge_recipe::DesignSurfacePatchRecipeStructure {
                    clauses: std::array::from_fn(|_| {
                        crate::records::topology::edge_recipe::DesignSurfacePatchRecipeClause {
                            fields: Vec::new(),
                            face_reference_ordinals: [0, 0],
                            edge_reference_ordinals: [0, 0],
                            entries: Vec::new(),
                        }
                    }),
                },
            );
            let mut second = recipe_edge_operand(11, &[], &[]);
            second.resolved_edge_slot = Some(18);
            second.surface_patch_recipe_structure = first.surface_patch_recipe_structure.clone();
            let feature =
                cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#main-edge-group").unwrap();
            resolved_edge_group(
                &selection_group,
                std::slice::from_ref(&selection_group),
                &[first, second],
                &[],
                Some(7),
                &feature,
                ctx,
            )
            .map(drop)
        }
        _ => super::scalar_edits::run_case(owner, ctx),
    }
}

fn assert_refusal(owner: &str, operation: &str) {
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| run_case(owner, ctx),
    );
    assert!(matches!(error, CodecError::ResourceLimit(original)
        if original.dimension == ResourceDimension::WorkUnits && original.operation == operation));
}

#[test]
fn edge_membership_refuses_identity_transition_equality_scan() {
    assert_refusal("identity_uniform", "scan F3D identity transition equality");
}

#[test]
fn edge_membership_refuses_identity_transition_equality_scalar() {
    assert_refusal("identity_uniform", "compare F3D identity transition edge");
}

#[test]
fn edge_membership_refuses_flange_preceding() {
    assert_refusal("flange", "find F3D flange preceding edge");
}

#[test]
fn edge_membership_refuses_flange_changed() {
    assert_refusal("flange", "find F3D flange changed edge");
}

#[test]
fn edge_membership_refuses_flange_deleted() {
    assert_refusal("flange", "find F3D flange deleted edge");
}

#[test]
fn edge_membership_refuses_flange_result() {
    assert_refusal("flange", "find F3D flange result edge");
}

#[test]
fn edge_membership_refuses_surface_patch_distinct() {
    assert_refusal("surface_patch", "find F3D surface patch resolved edge");
}

#[test]
fn edge_membership_refuses_chain_resolved_deleted() {
    assert_refusal("chain_resolved", "find F3D transition deleted recipe edge");
}

#[test]
fn edge_membership_refuses_chain_resolved_resolved() {
    assert_refusal("chain_resolved", "find F3D transition resolved recipe edge");
}

#[test]
fn edge_membership_refuses_transition_chain_is_supported_by_recipe_deleted_scan() {
    assert_refusal(
        "transition_chain_is_supported_by_recipe",
        "scan F3D transition deleted recipe edges",
    );
}

#[test]
fn edge_membership_refuses_transition_chain_is_supported_by_recipe_deleted_membership() {
    assert_refusal(
        "transition_chain_is_supported_by_recipe",
        "find F3D transition deleted chain edge",
    );
}

#[test]
fn edge_membership_refuses_transition_chain_is_supported_by_recipe_coverage_scan() {
    assert_refusal(
        "transition_chain_is_supported_by_recipe",
        "scan F3D transition chain coverage",
    );
}

#[test]
fn edge_membership_refuses_transition_chain_is_supported_by_recipe_coverage_membership() {
    assert_refusal(
        "transition_chain_is_supported_by_recipe",
        "find F3D transition covered edge",
    );
}

#[test]
fn edge_membership_refuses_unique_hem_transition_edge_candidate_support_scan() {
    assert_refusal(
        "unique_hem_transition_edge_candidate",
        "scan F3D Hem support membership",
    );
}

#[test]
fn edge_membership_refuses_unique_hem_transition_edge_candidate_support_membership() {
    assert_refusal(
        "unique_hem_transition_edge_candidate",
        "find F3D Hem changed support edge",
    );
}

#[test]
fn edge_membership_refuses_unique_hem_transition_edge_candidate_candidate() {
    assert_refusal(
        "unique_hem_transition_edge_candidate",
        "find F3D Hem support candidate",
    );
}

#[test]
fn edge_membership_refuses_partial_slot() {
    assert_refusal("partial", "find F3D partial edge slot");
}

#[test]
fn edge_membership_refuses_unique_deleted_reference_assignment_shared() {
    assert_refusal(
        "unique_deleted_reference_assignment",
        "find F3D deleted reference shared edge",
    );
}

#[test]
fn edge_membership_refuses_partition_unique_incomplete_edge_group_equality_scan() {
    assert_refusal(
        "partition_unique_incomplete_edge_group",
        "scan F3D partition universe equality",
    );
}

#[test]
fn edge_membership_refuses_partition_unique_incomplete_edge_group_equality_scalar() {
    assert_refusal(
        "partition_unique_incomplete_edge_group",
        "compare F3D partition universe edge",
    );
}

#[test]
fn edge_membership_refuses_partition_unique_incomplete_edge_group_universe() {
    assert_refusal(
        "partition_unique_incomplete_edge_group",
        "find F3D partition resolved universe edge",
    );
}

#[test]
fn edge_membership_refuses_partition_unique_incomplete_edge_group_reserved() {
    assert_refusal(
        "partition_unique_incomplete_edge_group",
        "find F3D partition reserved edge",
    );
}

#[test]
fn edge_membership_refuses_partition_unique_incomplete_edge_group_candidate() {
    assert_refusal(
        "partition_unique_incomplete_edge_group",
        "find F3D partition reserved candidate",
    );
}

#[test]
fn edge_membership_refuses_partition_target_proof_scan() {
    assert_refusal("partition_target", "scan F3D partition target proof");
}

#[test]
fn edge_membership_refuses_partition_target_proof_membership() {
    assert_refusal(
        "partition_target",
        "find F3D partition target resolved edge",
    );
}

#[test]
fn edge_membership_refuses_common_deleted_edge_group_candidates_equality_scan() {
    assert_refusal(
        "common_deleted_edge_group_candidates",
        "scan F3D common deleted equality",
    );
}

#[test]
fn edge_membership_refuses_common_deleted_edge_group_candidates_equality_scalar() {
    assert_refusal(
        "common_deleted_edge_group_candidates",
        "compare F3D common deleted edge",
    );
}

#[test]
fn edge_membership_refuses_deleted_boundary_edge_group_candidates_proof_scan() {
    assert_refusal(
        "deleted_boundary_edge_group_candidates",
        "scan F3D deleted boundary proof",
    );
}

#[test]
fn edge_membership_refuses_deleted_boundary_edge_group_candidates_predecessor() {
    assert_refusal(
        "deleted_boundary_edge_group_candidates",
        "find F3D deleted boundary predecessor edge",
    );
}

#[test]
fn edge_membership_refuses_deleted_boundary_edge_group_candidates_changed() {
    assert_refusal(
        "deleted_boundary_edge_group_candidates",
        "find F3D deleted boundary changed edge",
    );
}

#[test]
fn edge_membership_refuses_deleted_boundary_edge_group_candidates_contextual() {
    assert_refusal(
        "deleted_boundary_edge_group_candidates",
        "find F3D deleted boundary contextual edge",
    );
}

#[test]
fn edge_membership_refuses_deleted_boundary_edge_group_candidates_coverage_scan() {
    assert_refusal(
        "deleted_boundary_edge_group_candidates",
        "scan F3D deleted contextual coverage",
    );
}

#[test]
fn edge_membership_refuses_deleted_boundary_edge_group_candidates_sorted() {
    assert_refusal(
        "deleted_boundary_edge_group_candidates",
        "find F3D deleted contextual sorted edge",
    );
}

#[test]
fn edge_membership_refuses_contextual_deleted_edge_group_candidates_predecessor() {
    assert_refusal(
        "contextual_deleted_edge_group_candidates",
        "find F3D contextual deleted predecessor edge",
    );
}

#[test]
fn edge_membership_refuses_contextual_deleted_edge_group_candidates_changed() {
    assert_refusal(
        "contextual_deleted_edge_group_candidates",
        "find F3D contextual deleted changed edge",
    );
}

#[test]
fn edge_membership_refuses_contextual_deleted_edge_group_candidates_sorted() {
    assert_refusal(
        "contextual_deleted_edge_group_candidates",
        "find F3D contextual deleted sorted edge",
    );
}

#[test]
fn edge_membership_refuses_result_boundary_reference_edge_group_candidates_candidate() {
    assert_refusal(
        "result_boundary_reference_edge_group_candidates",
        "find F3D result boundary candidate",
    );
}

#[test]
fn edge_membership_refuses_result_boundary_reference_edge_group_candidates_predecessor() {
    assert_refusal(
        "result_boundary_reference_edge_group_candidates",
        "find F3D result candidate predecessor",
    );
}

#[test]
fn edge_membership_refuses_result_boundary_reference_edge_group_candidates_support_scan() {
    assert_refusal(
        "result_boundary_reference_edge_group_candidates",
        "scan F3D result candidate reference support",
    );
}

#[test]
fn edge_membership_refuses_result_boundary_reference_edge_group_candidates_support_membership() {
    assert_refusal(
        "result_boundary_reference_edge_group_candidates",
        "find F3D result candidate reference support",
    );
}

#[test]
fn edge_membership_flange_preserves_the_native_slot_and_unvisited_suffix_work() {
    fn decode(operand: &DesignEdgeOperand) -> (Option<i64>, u64) {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 1024;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let output = edge_flange_updated_edge_candidate(operand, &ctx).unwrap();
        let Err(CodecError::ResourceLimit(usage)) = ctx.charge_work(
            policy.limits.max_work_units + 1,
            "measure actual completed flange work",
        ) else {
            panic!("finite work control exposes cumulative usage");
        };
        assert_eq!(usage.dimension, ResourceDimension::WorkUnits);
        (output, usage.used)
    }
    let short = flange_operand();
    let mut long = short.clone();
    long.preceding_boundary_edge_slots.extend(0..1024);
    long.changed_boundary_edge_slots.extend(0..1024);
    long.result_boundary_edge_slots.extend(0..1024);
    let short = decode(&short);
    let long = decode(&long);
    assert_eq!(short.0, Some(17));
    assert_eq!(long.0, short.0);
    assert_eq!(long.1, short.1);
}

#[test]
fn edge_membership_flange_scalar_native_candidate_uses_no_storage_or_collection() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        edge_flange_updated_edge_candidate(&flange_operand(), &ctx).unwrap(),
        Some(17)
    );
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn edge_membership_common_deleted_equality_stops_at_the_first_mismatch() {
    fn work(second: &[i64]) -> u64 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 4096;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let output =
            common_deleted_edge_group_candidates([(true, &[17, 18, 19][..]), (true, second)], &ctx)
                .unwrap();
        assert_eq!(output, None);
        let Err(CodecError::ResourceLimit(usage)) = ctx.charge_work(
            policy.limits.max_work_units + 1,
            "measure actual completed common deleted work",
        ) else {
            panic!("finite work control exposes cumulative usage");
        };
        assert_eq!(usage.dimension, ResourceDimension::WorkUnits);
        usage.used
    }
    // Both inputs are sorted, unique, and equally long. The later mismatch
    // visits two more scalar pairs. Each costs one source step and two i64
    // comparisons of eight bytes; normalization work is the same.
    let early = work(&[16, 18, 19]);
    let late = work(&[17, 18, 20]);
    let pair_work = 1 + 2 * u64::try_from(std::mem::size_of::<i64>()).unwrap();
    assert_eq!(late, early + 2 * pair_work);
}

#[test]
fn edge_membership_refuses_flange_selected_native_slot_scan() {
    assert_refusal("flange_group", "scan F3D flange selected native slots");
}

#[test]
fn edge_membership_flange_candidate_array_refuses_collection_and_scoped_backing() {
    for dimension in [
        ResourceDimension::CollectionItems,
        ResourceDimension::MaterializedBytes,
    ] {
        let error = crate::test_support::resource_refusal_at(
            dimension,
            "collect F3D flange updated edge candidate",
            0,
            |ctx| run_case("flange_group", ctx),
        );
        assert!(matches!(error, CodecError::ResourceLimit(original)
            if original.dimension == dimension && original.operation == "collect F3D flange updated edge candidate"));
    }
}

#[test]
fn edge_membership_flange_native_output_survives_actual_temporary_backing_refund() {
    let selection_group = group(2, 10);
    let feature =
        cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#main-edge-group").unwrap();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let output = resolved_edge_flange_group(
        &selection_group,
        std::slice::from_ref(&selection_group),
        &[flange_group_operand()],
        &[],
        Some(7),
        &feature,
        &ctx,
    )
    .unwrap();
    let cadmpeg_ir::features::EdgeSelection::Historical { edges, .. } = output else {
        panic!("eligible native updated edge proves a historical selection");
    };
    let expected = crate::ids::history_input_edge_id(
        &crate::ids::history_input_prefix(&cadmpeg_ir::identity_key!("main-edge-group"), 7),
        17,
    );
    assert_eq!(edges.as_slice(), std::slice::from_ref(&expected));
    let Err(CodecError::ResourceLimit(usage)) = ctx.reserve_scoped(
        policy.limits.max_materialized_bytes + 1,
        "observe live flange temporary backing",
    ) else {
        panic!("finite materialized control exposes live usage");
    };
    assert_eq!(usage.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(usage.used, 0);
    assert_eq!(ctx.resource_refusal(), Some(usage));
    assert_eq!(edges[0].as_str(), expected.as_str());
}
