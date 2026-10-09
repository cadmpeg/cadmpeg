// SPDX-License-Identifier: Apache-2.0

use crate::curve::{
    apply_declared_relation_unit, expression_helix, expression_identifier_end,
    format_relation_real_admitted, synchronize_solve_blocks, valid_expression_identifier,
    ConditionalFrame, ConditionalStack, CurveExpressionActivation, CurveExpressionRecord,
    CurveExpressionSolveBlock, CurveExpressionValue,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

fn active_and_refused<T>(mut run: impl FnMut(&DecodeContext<'_>) -> Result<T, CodecError>) {
    for refused in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_entities = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        if refused {
            let original = ctx
                .charge_work_limit(1, "before curve recovery")
                .expect_err("zero work");
            assert!(matches!(run(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == original));
            assert!(matches!(run(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == original));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
        } else {
            let _value = run(&ctx).expect("constant recovery is free");
            assert!(ctx.resource_refusal().is_none());
            ctx.finish_session().expect("active session");
        }
    }
}

#[test]
fn empty_solve_synchronization_is_free_and_keeps_original_refusal() {
    active_and_refused(|ctx| {
        synchronize_solve_blocks(ctx, &mut [], &[], &BTreeMap::new())
    });
}

#[test]
fn empty_relation_identifier_is_free_and_keeps_original_refusal() {
    active_and_refused(|ctx| {
        let value = valid_expression_identifier(ctx, "")?;
        assert!(!value);
        Ok(value)
    });
}

#[test]
fn missing_relation_identifier_start_is_free_and_keeps_original_refusal() {
    for (source, start) in [(b"".as_slice(), 0), (b"a".as_slice(), 1)] {
        active_and_refused(|ctx| {
            let value = expression_identifier_end(ctx, source, start)?;
            assert_eq!(value, None);
            Ok(value)
        });
    }
}

#[test]
fn invalid_relation_identifier_start_is_free_and_keeps_original_refusal() {
    for source in [b"1".as_slice(), b":".as_slice(), b" ".as_slice()] {
        active_and_refused(|ctx| {
            let value = expression_identifier_end(ctx, source, 0)?;
            assert_eq!(value, None);
            Ok(value)
        });
    }
}

#[test]
fn first_conditional_frame_is_free_and_keeps_original_refusal() {
    active_and_refused(|ctx| {
        let mut stack = ConditionalStack::default();
        let result = stack.push(ctx, ConditionalFrame {
            parent: CurveExpressionActivation::Active,
            condition: Some(true),
        });
        if result.is_err() {
            assert!(matches!(stack, ConditionalStack::Empty));
        } else {
            assert!(matches!(stack, ConditionalStack::Open { .. }));
            assert_eq!(stack.end(), CurveExpressionActivation::Active);
            assert!(matches!(stack, ConditionalStack::Empty));
        }
        result
    });
}

#[test]
fn nonfinite_relation_formatting_is_free_and_keeps_original_refusal() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        active_and_refused(|ctx| {
            let result = format_relation_real_admitted(ctx, value, None, false)?;
            assert_eq!(result, None);
            Ok(result)
        });
    }
}

#[test]
fn zero_relation_formatting_is_free_and_keeps_original_refusal() {
    for value in [0.0, -0.0] {
        for (decimals, scientific) in [(None, false), (Some(2), false), (Some(2), true)] {
            active_and_refused(|ctx| {
                let result = format_relation_real_admitted(ctx, value, decimals, scientific)?;
                assert_eq!(result.as_deref(), Some(""));
                assert_eq!(result.as_ref().map(String::capacity), Some(0));
                Ok(result)
            });
        }
    }
}

#[test]
fn absent_declared_relation_unit_is_free_and_keeps_original_refusal() {
    active_and_refused(|ctx| {
        let value = CurveExpressionValue::Number(
            cadmpeg_ir::scalar::FiniteReal::new(7.0).expect("finite"),
        );
        let result = apply_declared_relation_unit(ctx, value.clone(), None)?;
        assert_eq!(result, Some(value));
        Ok(result)
    });
}

#[test]
fn unsupported_helix_control_is_free_and_keeps_original_refusal() {
    for route in 0..3 {
        let record = CurveExpressionRecord {
            entity_id: 7,
            backup: false,
            local_system: None,
            lines: Vec::new(),
            assignments: Vec::new(),
            solve_blocks: if route == 1 { vec![CurveExpressionSolveBlock {
                equations: Vec::new(), assignments: Vec::new(), unknowns: Vec::new(),
                offset: 0, for_offset: 1,
            }] } else { Vec::new() },
            unresolved_solve_control: route == 2,
            prohibited_constructs: if route == 0 { vec!["abs".into()] } else { Vec::new() },
            offset: 0,
            expression_offset: 1,
        };
        active_and_refused(|ctx| {
            let result = expression_helix(ctx, &record)?;
            assert_eq!(result, None);
            Ok(result)
        });
    }
}
