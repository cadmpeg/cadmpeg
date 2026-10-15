// SPDX-License-Identifier: Apache-2.0
use crate::entities::copious::has_forbidden_form_63_duplicate;
use cadmpeg_ir::math::Point3;

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
fn copious_two_point_form63_sequence_is_invalid_without_work() {
    let points = [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)];
    with_zero_context(|ctx| {
        let result = has_forbidden_form_63_duplicate(&points, 0.0, ctx);
        assert!(matches!(result, Ok(true)));
    });
}
