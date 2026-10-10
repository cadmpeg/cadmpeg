// SPDX-License-Identifier: Apache-2.0
//! WorkPoint visitor results cannot replace the original caller refusal.

use super::*;
use crate::design::feature_project::visit_work_point_history_states;

const ORIGINAL: &str = "original F3D WorkPoint visitor refusal";

fn assert_callback_refusal(returned: Option<bool>) {
    for counted in [false, true] {
        let scope = scope();
        let construction = if counted {
            construction(DesignWorkPointRuleForm::Native {
                reference_type: 99,
                inputs: vec![vertex_input(4), vertex_input(5)],
            })
        } else {
            vertex_construction(true)
        };
        let original_construction = construction.clone();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, ORIGINAL, Some(1));
            let mut visits = 0;
            let result =
                visit_work_point_history_states(ctx, &scope, &construction, &[], |state| {
                    visits += 1;
                    assert_eq!(state, 4);
                    assert!(matches!(
                        ctx.charge_work(1, ORIGINAL),
                        Err(CodecError::ResourceLimit(_))
                    ));
                    returned
                        .ok_or_else(|| CodecError::malformed("ordinary WorkPoint visitor error"))
                });
            let original = ctx.resource_refusal().expect("visitor stored its refusal");
            assert_eq!(original.operation, ORIGINAL);
            assert_eq!(original.dimension, ResourceDimension::WorkUnits);
            assert_eq!(original.additional, 1);
            assert!(matches!(result, Err(CodecError::ResourceLimit(later)) if later == original));
            assert_eq!(visits, 1);
            assert!(
                matches!(visit_work_point_history_states(ctx, &scope, &construction, &[], |_| {
                visits += 1;
                Ok(true)
            }), Err(CodecError::ResourceLimit(later)) if later == original)
            );
            assert_eq!(visits, 1);
        });
        assert_eq!(construction, original_construction);
    }
}

#[test]
fn work_point_visitor_false_keeps_original_refusal() {
    assert_callback_refusal(Some(false));
}

#[test]
fn work_point_visitor_true_keeps_original_refusal() {
    assert_callback_refusal(Some(true));
}

#[test]
fn work_point_visitor_ordinary_error_keeps_original_refusal() {
    assert_callback_refusal(None);
}

#[test]
fn work_point_visitor_ordinary_error_preserves_unvisited_native_input() {
    let scope = scope();
    let construction = construction(DesignWorkPointRuleForm::Native {
        reference_type: 99,
        inputs: vec![vertex_input(4), vertex_input(5)],
    });
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let mut visits = 0;
        let result = visit_work_point_history_states(ctx, &scope, &construction, &[], |state| {
            visits += 1;
            assert_eq!(state, 4);
            Err(CodecError::malformed("ordinary WorkPoint visitor error"))
        });
        assert!(
            matches!(result, Err(CodecError::Malformed(message)) if message == "ordinary WorkPoint visitor error")
        );
        assert_eq!(visits, 1);
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn work_point_visitor_fixed_false_uses_zero_positive_work() {
    let scope = scope();
    let construction = vertex_construction(true);
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let mut visits = 0;
        assert!(
            !visit_work_point_history_states(ctx, &scope, &construction, &[], |state| {
                visits += 1;
                assert_eq!(state, 4);
                Ok(false)
            })
            .unwrap()
        );
        assert_eq!(visits, 1);
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn work_point_visitor_keeps_native_state_order_and_absent_input() {
    let scope = scope();
    let construction = construction(DesignWorkPointRuleForm::Native {
        reference_type: 99,
        inputs: vec![
            vertex_input(4),
            DesignWorkPointInput::try_new(23, 0, None).unwrap(),
            vertex_input(5),
        ],
    });
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 4;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let mut states = Vec::new();
        assert!(
            visit_work_point_history_states(ctx, &scope, &construction, &[], |state| {
                states.push(state);
                Ok(true)
            })
            .unwrap()
        );
        assert_eq!(states, [4, 5]);
        assert_eq!(ctx.resource_refusal(), None);
    });
}
