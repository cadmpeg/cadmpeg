// SPDX-License-Identifier: Apache-2.0
use crate::entities::{admit_with_scoped_loss_slots, non_resource_error};
use cadmpeg_core::decode::{
    DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, ResourceLimit,
};
use cadmpeg_core::CodecError;

fn with_zero_context(mut run: impl FnMut(&cadmpeg_core::decode::DecodeContext<'_>)) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_entities = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..64 {
        run(&ctx);
    }
    ctx.finish_session().unwrap();
}

#[test]
fn entity_foreign_resource_error_is_propagated_in_a_fresh_context() {
    let foreign = ResourceLimit::allocation_failed(
        ResourceDimension::Entities,
        17,
        19,
        "test foreign entity refusal",
    );
    with_zero_context(|ctx| {
        let expected = foreign;
        let result = non_resource_error(CodecError::ResourceLimit(foreign), ctx);
        assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == expected));
    });
}

#[test]
fn entity_scalar_admission_has_no_losses_or_work() {
    let entry = crate::test_support::directory_target(1, 104);
    {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_entities = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty caller input");
        let mut slots = ctx
            .reserve_scoped(0, "test entity loss slots")
            .expect("empty loss owner");
        let mut losses = Vec::new();

        for _ in 0..64 {
            let result =
                admit_with_scoped_loss_slots(Ok(42_u32), &entry, &mut slots, &mut losses, &ctx);
            assert!(matches!(result, Ok(Some(42))));
            assert!(losses.is_empty());
        }
        drop(losses);
        drop(slots);
        ctx.finish_session()
            .expect("fresh scalar admission finishes");
    }
}
