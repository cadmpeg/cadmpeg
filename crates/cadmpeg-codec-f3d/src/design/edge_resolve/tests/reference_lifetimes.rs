// SPDX-License-Identifier: Apache-2.0

use super::super::*;
use super::recipe_edge_operand;
use cadmpeg_core::decode::{refusal_probe::RefusalProbe, DecodePolicy, ResourceDimension};
use std::cell::Cell;

fn reference(
    edges: &[i64],
) -> crate::records::topology::historical_context::DesignEdgeRecipeReferenceContext {
    serde_json::from_value(serde_json::json!({
        "reference_ordinal": 0, "result_faces": [], "result_shared_edge_slots": [],
        "preceding_faces": [], "preceding_support_face_slots": [], "shared_edge_slots": [],
        "changed_shared_edge_slots": [], "changed_reference_edge_slots": edges,
    }))
    .unwrap()
}

fn probe_policy() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_materialized_bytes = u64::MAX;
    policy
}

#[test]
fn edge_reference_lifetime_hem_scalar_releases_all_query_backing() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    // Four borrowed references, three support edges and one candidate use
    // three minimum four-slot vectors. All three buffers overlap.
    let required =
        4 * u64::try_from(std::mem::size_of::<&[i64]>() + 2 * std::mem::size_of::<i64>()).unwrap();
    policy.limits.max_materialized_bytes = required;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        for _ in 0..2 {
            assert_eq!(
                unique_hem_transition_edge_candidate(
                    &[63, 106, 110, 167],
                    [&[][..], &[167][..], &[106][..], &[110][..]],
                    ctx
                )
                .unwrap(),
                Some(63)
            );
        }
    });
    policy.limits.max_materialized_bytes = required - 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) = unique_hem_transition_edge_candidate(
            &[63, 106, 110, 167],
            [&[][..], &[167][..], &[106][..], &[110][..]],
            ctx,
        ) else {
            panic!("live query overlap must refuse one byte below");
        };
        assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(original.operation, "f3d hem candidate edge");
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

#[test]
fn edge_reference_lifetime_hem_rejected_proof_releases_query_backing() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes =
        4 * u64::try_from(std::mem::size_of::<&[i64]>() + std::mem::size_of::<i64>()).unwrap();
    crate::test_support::with_decode_policy(&policy, |ctx| {
        for _ in 0..2 {
            assert_eq!(
                unique_hem_transition_edge_candidate(&[63], [&[][..], &[167][..]], ctx).unwrap(),
                None
            );
        }
    });
}

#[test]
fn edge_reference_lifetime_hem_source_refuses_before_adapter_visit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let visited = Cell::new(0);
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let source = (0..1024).map(|_| {
            visited.set(visited.get() + 1);
            &[][..]
        });
        let Err(CodecError::ResourceLimit(original)) =
            unique_hem_transition_edge_candidate(&[63], source, ctx)
        else {
            panic!("source step must refuse before its adapter");
        };
        assert_eq!(original.operation, "scan F3D Hem reference set sources");
        assert_eq!(visited.get(), 0);
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

#[test]
fn edge_reference_lifetime_hem_each_reached_scan_preserves_the_original_refusal() {
    for operation in [
        "scan F3D Hem reference support sets",
        "scan F3D Hem reference support edges",
        "scan F3D Hem changed candidate edges",
    ] {
        crate::test_support::with_decode_policy(&probe_policy(), |ctx| {
            assert_eq!(
                unique_hem_transition_edge_candidate(&[63, 167], [&[][..], &[167][..]], ctx)
                    .unwrap(),
                Some(63)
            );
            let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
            let Err(CodecError::ResourceLimit(original)) =
                unique_hem_transition_edge_candidate(&[63, 167], [&[][..], &[167][..]], ctx)
            else {
                panic!("reached Hem step must refuse");
            };
            assert_eq!(original.operation, operation);
        });
    }
}

#[test]
fn edge_reference_lifetime_hem_eligibility_stops_at_second_empty_context() {
    let mut operand = recipe_edge_operand(10, &[63], &[]);
    operand.recipe_reference_contexts = vec![reference(&[]); 1024];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 2;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(hem_transition_edge_slot(&operand, ctx).unwrap(), None);
        assert_eq!(ctx.resource_refusal(), None);
    });
    policy.limits.max_work_units = 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) = hem_transition_edge_slot(&operand, ctx)
        else {
            panic!("second eligibility step must refuse");
        };
        assert_eq!(original.operation, "scan F3D Hem reference eligibility");
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

#[test]
fn edge_reference_lifetime_changed_rejected_candidate_arrays_are_temporary() {
    let mut operand = recipe_edge_operand(10, &[], &[]);
    operand.recipe_reference_contexts = vec![reference(&[17]), reference(&[18])];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4 * u64::try_from(std::mem::size_of::<i64>()).unwrap();
    crate::test_support::with_decode_policy(&policy, |ctx| {
        for _ in 0..2 {
            assert_eq!(
                changed_reference_edge_group_candidates(&[&operand], ctx).unwrap(),
                None
            );
        }
    });
}

#[test]
fn edge_reference_lifetime_changed_success_retains_only_actual_assignment() {
    let mut operand = recipe_edge_operand(10, &[], &[]);
    operand.recipe_reference_contexts = vec![reference(&[17, 18]), reference(&[18])];
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            changed_reference_edge_group_candidates(&[&operand], ctx).unwrap(),
            Some(vec![18])
        );
        let Err(CodecError::ResourceLimit(original)) =
            ctx.charge_retained(u64::MAX, "measure actual reference assignment retention")
        else {
            panic!("retention measurement");
        };
        assert_eq!(
            original.used,
            u64::try_from(std::mem::size_of::<i64>()).unwrap()
        );
    });
}

#[test]
fn edge_reference_lifetime_changed_empty_context_prefix_is_paid() {
    let mut operand = recipe_edge_operand(10, &[], &[]);
    operand.recipe_reference_contexts = vec![reference(&[]); 1024];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 4;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) =
            changed_reference_edge_group_candidates(&[&operand], ctx)
        else {
            panic!("fourth raw context step must refuse after the operand step");
        };
        assert_eq!(original.operation, "scan F3D changed reference contexts");
        assert_eq!(original.used, 4);
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

#[test]
fn edge_reference_lifetime_changed_each_reached_scan_preserves_original_refusal() {
    for operation in [
        "scan F3D changed reference operands",
        "scan F3D changed reference contexts",
        "scan F3D first changed reference edges",
    ] {
        let mut operand = recipe_edge_operand(10, &[], &[]);
        operand.recipe_reference_contexts = vec![reference(&[]), reference(&[17])];
        crate::test_support::with_decode_policy(&probe_policy(), |ctx| {
            assert_eq!(
                changed_reference_edge_group_candidates(&[&operand], ctx).unwrap(),
                Some(vec![17])
            );
            let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
            let Err(CodecError::ResourceLimit(original)) =
                changed_reference_edge_group_candidates(&[&operand], ctx)
            else {
                panic!("reached changed-reference step must refuse");
            };
            assert_eq!(original.operation, operation);
        });
    }
}
