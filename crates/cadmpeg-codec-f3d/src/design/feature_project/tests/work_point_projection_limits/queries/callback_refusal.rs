// SPDX-License-Identifier: Apache-2.0
//! `WorkPoint` visitors preserve state order and unread inputs.

use super::*;
use crate::design::feature_project::visit_work_point_history_states;

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
