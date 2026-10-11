// SPDX-License-Identifier: Apache-2.0
use crate::entities::analytic_surfaces::{
    admit_analytic, form_reference_direction, required_direction, AnalyticDirectionError,
};
use crate::parameter::ParameterRecord;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::transform::Transform;

fn empty_record() -> ParameterRecord {
    ParameterRecord::from_test_tokens(1, 1..2, Vec::new(), 0, Vec::new(), Vec::new())
}

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
fn analytic_fixed_admission_has_no_losses_or_work() {
    let entry = crate::test_support::directory_target(1, 190);
    {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_entities = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let mut slots = ctx
            .reserve_scoped(0, "analytic test slots")
            .expect("empty owner");
        let mut losses = Vec::new();

        for _ in 0..64 {
            let result = admit_analytic(Ok(42_u32), &entry, &mut losses, &mut slots, &ctx);
            assert!(matches!(result, Ok(Some(42))));
            assert!(losses.is_empty());
        }
        drop(losses);
        drop(slots);
        ctx.finish_session().expect("fixed admission finishes");
    }
}

#[test]
fn analytic_missing_required_pointer_reports_missing_axis_without_work() {
    let record = empty_record();
    with_zero_context(|ctx| {
        let result = required_direction(&record, 1, "axis", &[], &[], ctx);
        assert!(matches!(
            result,
            Err(AnalyticDirectionError::MissingPointer("axis"))
        ));
    });
}

#[test]
fn analytic_optional_reference_direction_is_absent_without_work() {
    let record = empty_record();
    with_zero_context(|ctx| {
        let result = form_reference_direction(
            0,
            &record,
            1,
            "axis",
            Transform::identity(),
            (&[], &[]),
            ctx,
        );
        assert!(matches!(result, Ok(None)));
    });
}
