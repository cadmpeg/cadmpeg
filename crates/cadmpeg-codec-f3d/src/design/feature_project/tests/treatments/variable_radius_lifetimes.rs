// SPDX-License-Identifier: Apache-2.0
//! Rejected authored radius laws release only their actual typed candidates.

use super::*;
use cadmpeg_core::decode::{refusal_probe::RefusalProbe, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const RADIUS_QUERY_SCRATCH: u64 = 4096;

fn endpoint(record: u32, kind: &str, value: f64) -> DesignParameter {
    let mut parameter = parse_design_parameter_record(&parameter_record(
        Some(record + 100),
        "value",
        kind,
        Some("mm"),
        "d1",
        value,
    ))
    .unwrap();
    parameter.record_index = record;
    parameter
}

fn typed_radius_bytes() -> u64 {
    use cadmpeg_ir::features::edge_treatments::VariableRadius;
    use cadmpeg_ir::scalar::{Fraction, NonNegativeLength};
    // Exact two-element IntoIter hint reserves two slots in the distinct typed
    // output vector. Raw samples have their own materialized lease.
    u64::try_from(2 * std::mem::size_of::<VariableRadius<Fraction, NonNegativeLength>>()).unwrap()
}

#[test]
fn authored_variable_radius_rejected_zero_law_refunds_actual_typed_backing() {
    let start = endpoint(1, "StartRadius", 0.0);
    let end = endpoint(2, "EndRadius", 0.0);
    let controls = [(0, &start), (1, &end)];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = typed_radius_bytes();
    policy.limits.max_materialized_bytes = RADIUS_QUERY_SCRATCH;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        for _ in 0..7 {
            assert!(
                crate::design::feature_project::variable_fillet_law(ctx, &controls)
                    .unwrap()
                    .is_none()
            );
            let all = ctx
                .reserve_scoped(
                    RADIUS_QUERY_SCRATCH,
                    "reuse rejected authored radius scratch",
                )
                .unwrap();
            drop(all);
        }
        ctx.charge_retained(
            typed_radius_bytes(),
            "reuse rejected authored typed radius retention",
        )
        .unwrap();
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn authored_variable_radius_typed_allocation_refuses_before_conversion() {
    let start = endpoint(1, "StartRadius", 0.2);
    let end = endpoint(2, "EndRadius", 0.4);
    let controls = [(0, &start), (1, &end)];
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::MaterializedBytes, "IR variable radius admitted samples", 0,
        |ctx| crate::design::feature_project::variable_fillet_law(ctx, &controls),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes
            && limit.operation == "IR variable radius admitted samples"
            && limit.additional == typed_radius_bytes()));
}

#[test]
fn authored_variable_radius_positivity_work_refusal_remains_original_after_candidate_drop() {
    let start = endpoint(1, "StartRadius", 0.2);
    let end = endpoint(2, "EndRadius", 0.4);
    let controls = [(0, &start), (1, &end)];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_materialized_bytes = RADIUS_QUERY_SCRATCH;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let _probe = RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "IR variable radius positivity",
            None,
        );
        let Err(CodecError::ResourceLimit(original)) =
            crate::design::feature_project::variable_fillet_law(ctx, &controls)
        else {
            panic!("typed radius positivity must refuse before execution");
        };
        assert_eq!(original.operation, "IR variable radius positivity");
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(original));
        assert!(
            matches!(crate::design::feature_project::variable_fillet_law(ctx, &[]), Err(CodecError::ResourceLimit(later)) if later == original)
        );
    });
}

#[test]
fn authored_variable_radius_allowed_law_retains_exact_typed_samples() {
    let start = endpoint(1, "StartRadius", 0.2);
    let end = endpoint(2, "EndRadius", 0.4);
    let controls = [(0, &start), (1, &end)];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = typed_radius_bytes();
    policy.limits.max_materialized_bytes = RADIUS_QUERY_SCRATCH;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let (points, tangency) =
            crate::design::feature_project::variable_fillet_law(ctx, &controls)
                .unwrap()
                .unwrap();
        assert_eq!(tangency, None);
        assert_eq!(
            points
                .as_slice()
                .iter()
                .map(|point| (point.parameter.get(), point.radius.get()))
                .collect::<Vec<_>>(),
            [(0.0, 2.0), (1.0, 4.0)]
        );
        let all = ctx
            .reserve_scoped(
                RADIUS_QUERY_SCRATCH,
                "reuse allowed authored radius scratch",
            )
            .unwrap();
        drop(all);
        // The accepted law remains live, so its entire typed backing remains
        // retained. One further byte must refuse without losing that refusal.
        let Err(CodecError::ResourceLimit(original)) =
            ctx.charge_retained(1, "live accepted typed radius backing")
        else {
            panic!("accepted typed radius backing must remain retained");
        };
        assert_eq!(
            (original.used, original.additional),
            (typed_radius_bytes(), 1)
        );
        assert_eq!(original.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(original.operation, "live accepted typed radius backing");
        assert_eq!(ctx.resource_refusal(), Some(original));
        assert!(
            matches!(crate::design::feature_project::variable_fillet_law(ctx, &[]), Err(CodecError::ResourceLimit(later)) if later == original)
        );
        drop(points);
    });
}
