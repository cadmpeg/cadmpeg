// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::DecodePolicy;

const ADMISSION_SCRATCH_LIMIT: u64 = 1 << 12;

fn many_placements(admitted: bool) {
    let (placement, point, _, _, entity) = super::projected_constraint_fixture();
    let placements = vec![placement; 128];
    let entities = if admitted { vec![entity] } else { Vec::new() };
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = ADMISSION_SCRATCH_LIMIT;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let output = crate::design::constraints::project_sketch_constraints(
            ctx,
            &placements,
            &[],
            (&[point], &[], &[]),
            &[],
            &entities,
        )
        .unwrap();
        assert!(output.is_empty());
        drop(output);
        let _released = ctx
            .reserve_scoped(ADMISSION_SCRATCH_LIMIT, "released planar admission storage")
            .unwrap();
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn planar_admission_rejected_ids_release_each_iteration() {
    many_placements(false);
}

#[test]
fn planar_admission_duplicate_ids_borrow_input_storage() {
    many_placements(true);
}
