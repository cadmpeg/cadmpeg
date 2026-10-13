// SPDX-License-Identifier: Apache-2.0

use super::super::*;
use super::recipe_edge_operand;
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use std::cell::Cell;
use std::num::NonZeroU32;

fn selector() -> crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext {
    crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext {
        selector: 0,
        clauses: Vec::new(),
        incidence_matching_edge_slots: vec![17],
        boundary_count_matching_edge_slots: Vec::new(),
    }
}

#[test]
fn edge_reference_source_group_refuses_before_invalid_ordinal_visit() {
    let visited = Cell::new(0);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let source = (0..1024).map(|_| {
        visited.set(visited.get() + 1);
        None
    });
    let Err(CodecError::ResourceLimit(original)) =
        edge_group_assignment_candidates(&[], source, &ctx)
    else {
        panic!("first actual source advance must refuse");
    };
    assert_eq!(original.dimension, ResourceDimension::WorkUnits);
    assert_eq!(original.operation, "scan F3D assignment reference sources");
    assert_eq!(visited.get(), 0);
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn edge_reference_source_group_pays_each_invalid_prefix_step() {
    let visited = Cell::new(0);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let source = (0..1024).map(|_| {
        visited.set(visited.get() + 1);
        None
    });
    let Err(CodecError::ResourceLimit(original)) =
        edge_group_assignment_candidates(&[], source, &ctx)
    else {
        panic!("the fourth source step must refuse");
    };
    assert_eq!(original.operation, "scan F3D assignment reference sources");
    assert_eq!(visited.get(), 3);
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn edge_reference_source_group_stops_after_two_nonempty_sets() {
    crate::test_support::with_decode_context(|ctx| {
        let visited = Cell::new(0);
        let source = (0..1024).map(|position| {
            visited.set(visited.get() + 1);
            match position {
                0 => None,
                1 => Some(&[][..]),
                2 => Some(&[17, 18][..]),
                3 => Some(&[18, 19][..]),
                _ => panic!("unvisited source suffix"),
            }
        });
        assert_eq!(
            edge_group_assignment_candidates(&[], source, ctx).unwrap(),
            Some(EdgeAssignmentCandidates::Edges(vec![18]))
        );
        assert_eq!(visited.get(), 4);
    });
}

#[test]
fn edge_reference_source_direct_empty_and_invalid_meanings_are_distinct() {
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            edge_assignment_candidates(&[selector()], [None, Some(&[17][..])], ctx).unwrap(),
            Some(vec![17])
        );
        assert_eq!(
            edge_assignment_candidates(&[selector()], [Some(&[][..])], ctx).unwrap(),
            None
        );
        assert_eq!(
            edge_group_assignment_candidates(&[selector()], [Some(&[][..]), Some(&[17][..])], ctx)
                .unwrap(),
            Some(EdgeAssignmentCandidates::Edges(vec![17]))
        );
    });
}

#[test]
fn edge_reference_source_corroboration_refuses_before_invalid_visit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let visited = Cell::new(0);
        let source = (0..1024).map(|_| {
            visited.set(visited.get() + 1);
            None
        });
        let _probe = RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "scan F3D corroborated shared sets",
            None,
        );
        let Err(CodecError::ResourceLimit(original)) =
            edge_assignment_candidates(&[selector()], source, ctx)
        else {
            panic!("source must be admitted before advance");
        };
        assert_eq!(original.operation, "scan F3D corroborated shared sets");
        assert_eq!(visited.get(), 0);
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

#[test]
fn edge_reference_source_selector_gate_skips_all_source_steps() {
    crate::test_support::with_decode_context(|ctx| {
        let source = (0..1024).map(|_| -> Option<&[i64]> { panic!("source gate must stay lazy") });
        assert_eq!(edge_assignment_candidates(&[], source, ctx).unwrap(), None);
    });
}

#[test]
fn edge_reference_source_transition_consumer_refuses_invalid_selected_prefix() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let mut operand = recipe_edge_operand(10, &[17], &[]);
        operand.terminal_reference_edge_slots = vec![vec![17]];
        operand.local_topology_references = Some(vec![NonZeroU32::new(u32::MAX).unwrap(); 1024]);
        assert!(transition_chain_is_supported_by_recipe(&[17], 1, &[&operand], ctx).unwrap());
        let _probe = RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "scan F3D transition reference sources",
            None,
        );
        let Err(CodecError::ResourceLimit(original)) =
            transition_chain_is_supported_by_recipe(&[17], 1, &[&operand], ctx)
        else {
            panic!("invalid selected references must still admit their actual source steps");
        };
        assert_eq!(original.operation, "scan F3D transition reference sources");
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

#[test]
fn edge_reference_source_actual_selected_order_and_duplicates_reach_assignment() {
    crate::test_support::with_decode_context(|ctx| {
        let mut operand = recipe_edge_operand(10, &[], &[]);
        operand.terminal_reference_edge_slots = vec![vec![17, 18], vec![18, 19]];
        operand.local_topology_references = Some(
            [u32::MAX, 2, 1, 2]
                .map(|value| NonZeroU32::new(value).unwrap())
                .to_vec(),
        );
        assert_eq!(
            edge_group_assignment_candidates(&[], edge_operand_reference_edge_sets(&operand), ctx)
                .unwrap(),
            Some(EdgeAssignmentCandidates::Edges(vec![18]))
        );
    });
}
