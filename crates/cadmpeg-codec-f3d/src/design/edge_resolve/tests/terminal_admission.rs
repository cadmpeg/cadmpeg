// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::CodecError;

fn probe_policy() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy
}

fn terminal_operand() -> DesignEdgeOperand {
    let side = |scalars: [i32; 2]| {
        serde_json::from_value(serde_json::json!({
            "field_count": 3, "header_value": 0, "scalars": scalars,
            "payload_prefix": [0], "payload_entry_count": 0, "entries": []
        })).unwrap()
    };
    let mut operand = recipe_edge_operand(10, &[], &[]);
    operand.recipe_structure = Some(crate::records::topology::edge_recipe::DesignEdgeRecipeStructure {
        root: 2, sides: vec![side([2, 1]), side([1, 3])],
    });
    operand.terminal_reference_edge_slots = vec![vec![25, 38, 39, 62], vec![25, 35, 55, 69], vec![35, 39, 57, 83]];
    operand.terminal_boundary_edge_slots = vec![25, 35, 38, 39, 55, 57, 62, 69, 83];
    operand
}

#[test]
fn terminal_edge_admits_reached_sources_and_membership_with_sticky_refusal() {
    for operation in ["scan F3D terminal reference edges", "find F3D terminal shared reference edge", "find F3D terminal boundary edge"] {
        crate::test_support::with_decode_policy(&probe_policy(), |ctx| {
            let mut operand = terminal_operand();
            assert_eq!(resolved_edge_operand(&operand, ctx).unwrap(), Some(25));
            let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
            let CodecError::ResourceLimit(original) = resolved_edge_operand(&operand, ctx).unwrap_err() else { panic!("work refusal"); };
            assert_eq!(original.operation, operation);
            operand.resolved_edge_slot = Some(99);
            let CodecError::ResourceLimit(sticky) = resolved_edge_operand(&operand, ctx).unwrap_err() else { panic!("sticky refusal"); };
            assert_eq!(sticky, original);
        });
    }
}

#[test]
fn terminal_edge_fast_paths_skip_reference_work_and_storage() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let mut operand = terminal_operand();
        operand.resolved_edge_slot = Some(99);
        assert_eq!(resolved_edge_operand(&operand, ctx).unwrap(), Some(99));
        operand.recipe_program = vec![1];
        operand.recipe_structure = None;
        assert_eq!(resolved_edge_operand(&operand, ctx).unwrap(), None);
    });
    crate::test_support::with_decode_policy(&probe_policy(), |ctx| {
        let mut operand = terminal_operand();
        operand.resolved_edge_slot = Some(99);
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "scan F3D terminal reference edges", None);
        assert_eq!(resolved_edge_operand(&operand, ctx).unwrap(), Some(99));
        operand.resolved_edge_slot = None;
        assert!(matches!(resolved_edge_operand(&operand, ctx), Err(CodecError::ResourceLimit(limit)) if limit.operation == "scan F3D terminal reference edges"));
    });
}

#[test]
fn terminal_edge_duplicate_candidate_skips_repeated_membership() {
    let mut operand = terminal_operand();
    operand.terminal_reference_edge_slots = vec![vec![25], vec![25, 25]];
    operand.terminal_boundary_edge_slots = vec![25];
    // Two source items and the end probe; two memberships, each with one
    // visited slot and the pair of fixed-width i64 keys. The repeated
    // selected candidate requires no second membership search.
    let required = 3 + 2 * (1 + 2 * u64::try_from(std::mem::size_of::<i64>()).unwrap());
    for (cap, completes) in [(required, true), (required - 1, false)] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let result = resolved_edge_operand(&operand, ctx);
            if completes {
                assert_eq!(result.unwrap(), Some(25));
                assert!(matches!(ctx.charge_work(1, "after terminal proof"), Err(CodecError::ResourceLimit(limit)) if limit.used == required));
            } else {
                assert!(matches!(result, Err(CodecError::ResourceLimit(limit)) if limit.operation == "find F3D terminal boundary edge"));
            }
        });
    }
}

#[test]
fn ambiguous_terminal_edge_skips_boundary_query() {
    crate::test_support::with_decode_policy(&probe_policy(), |ctx| {
        let mut operand = terminal_operand();
        operand.terminal_reference_edge_slots[0].push(69);
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "find F3D terminal boundary edge", None);
        assert_eq!(resolved_edge_operand(&operand, ctx).unwrap(), None);
        operand.terminal_reference_edge_slots[0].pop();
        assert!(matches!(resolved_edge_operand(&operand, ctx), Err(CodecError::ResourceLimit(limit)) if limit.operation == "find F3D terminal boundary edge"));
    });
}

#[test]
fn radius_edge_queries_terminal_proof_once_per_operand() {
    let operand = terminal_operand();
    // One operand visit; four candidate items and their end probe;
    // thirteen reached support memberships and one boundary membership.
    // Each scalar membership visits one slot and compares two i64 keys.
    let required = 1 + 5 + 14 * (1 + 2 * u64::try_from(std::mem::size_of::<i64>()).unwrap());
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = required;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(super::super::radius_edge_group_candidates(&[&operand], 3.0, ctx).unwrap(), Some(vec![25]));
        assert!(matches!(ctx.charge_work(1, "after radius proof"), Err(CodecError::ResourceLimit(limit)) if limit.used == required));
    });
}

#[test]
fn radius_edge_rejected_chain_releases_scratch() {
    let mut proven = terminal_operand();
    proven.resolved_edge_slot = Some(25);
    let mut unsupported = recipe_edge_operand(11, &[38], &[]);
    unsupported.recipe_structure = None;
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4 * u64::try_from(std::mem::size_of::<i64>()).unwrap();
    crate::test_support::with_decode_policy(&policy, |ctx| {
        for _ in 0..2 {
            assert_eq!(super::super::radius_edge_group_candidates(&[&proven, &unsupported], 3.0, ctx).unwrap(), None);
        }
        assert!(matches!(super::super::radius_edge_group_candidates(&[&proven], 3.0, ctx), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes));
    });
}

#[test]
fn context_only_edge_members_preserve_a_terminal_query_refusal() {
    crate::test_support::with_decode_policy(&probe_policy(), |ctx| {
        let operand = terminal_operand();
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "find F3D terminal shared reference edge", None);
        let members = [&operand].into_iter().map(|operand| Ok((resolved_edge_operand(operand, ctx)?, operand.changed_boundary_edge_slots.as_slice())));
        assert!(matches!(super::super::context_only_edge_group_candidates(members, ctx), Err(CodecError::ResourceLimit(limit)) if limit.operation == "find F3D terminal shared reference edge"));
    });
}

#[test]
fn context_only_edge_rejection_releases_candidates_before_the_next_query() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4 * u64::try_from(std::mem::size_of::<i64>()).unwrap();
    crate::test_support::with_decode_policy(&policy, |ctx| {
        for _ in 0..2 {
            let members = [Ok((Some(25), &[][..])), Ok((None, &[38][..]))];
            assert_eq!(super::super::context_only_edge_group_candidates(members, ctx).unwrap(), None);
        }
        let members = [Ok((Some(25), &[][..]))];
        assert!(matches!(super::super::context_only_edge_group_candidates(members, ctx), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes));
    });
}

#[test]
fn radius_edge_admits_each_candidate_and_dedup_before_execution() {
    let mut operand = terminal_operand();
    operand.resolved_edge_slot = Some(25);
    operand.treatment_radius_candidates = vec![crate::records::topology::edge_identity::DesignEdgeTreatmentRadiusCandidate {
        edge_slot: 25, radius: cadmpeg_ir::scalar::PositiveReal::new(3.0).unwrap(),
    }];
    for operation in ["scan F3D radius edge operands", "scan F3D treatment radius candidates", "deduplicate F3D radius edge candidates"] {
        crate::test_support::with_decode_policy(&probe_policy(), |ctx| {
            assert_eq!(super::super::radius_edge_group_candidates(&[&operand], 3.0, ctx).unwrap(), Some(vec![25]));
            let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
            let CodecError::ResourceLimit(original) = super::super::radius_edge_group_candidates(&[&operand], 3.0, ctx).unwrap_err() else { panic!("radius work refusal"); };
            assert_eq!(original.operation, operation);
            let CodecError::ResourceLimit(sticky) = super::super::radius_edge_group_candidates(&[], f64::NAN, ctx).unwrap_err() else { panic!("sticky radius refusal"); };
            assert_eq!(sticky, original);
        });
    }
}
