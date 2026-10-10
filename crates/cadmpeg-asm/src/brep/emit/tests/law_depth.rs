// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::LawExpression;

use super::super::{map_law_expression, LawExpressionScope};
use crate::brep::AsmBrep;
use crate::nurbs::proc_surface::EmbeddedLawExpression;

fn chain(descents: usize) -> EmbeddedLawExpression {
    (0..descents).fold(EmbeddedLawExpression::Integer(7), |operand, _| {
        EmbeddedLawExpression::Algebraic {
            operator: "COS".into(),
            operands: vec![operand],
        }
    })
}

fn expected_chain(descents: usize) -> LawExpression {
    (0..descents).fold(LawExpression::Integer { value: 7 }, |operand, _| {
        LawExpression::Algebraic {
            operator: "COS".into(),
            operands: vec![operand],
        }
    })
}

fn map(
    ctx: &DecodeContext<'_>,
    out: &mut AsmBrep,
    expression: EmbeddedLawExpression,
) -> Result<LawExpression, CodecError> {
    let prefix = cadmpeg_ir::identity_key!("7");
    map_law_expression(ctx, out, crate::asm_format!("sat"),
        LawExpressionScope::Surface(&prefix), cadmpeg_ir::identity_key!("primary"), expression)
}

#[test]
fn law_expression_accepts_exact_child_depth() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Three COS nodes each descend to one operand. The root is not a descent.
    policy.limits.max_recursion_depth = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut out = AsmBrep::default();
    assert_eq!(map(&ctx, &mut out, chain(3)).unwrap(), expected_chain(3));
    assert!(out.curves.is_empty() && out.surfaces.is_empty());
    ctx.finish_session().unwrap();
}

#[test]
fn law_expression_refuses_first_and_last_child_depth() {
    for cap in [0, 2] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut out = AsmBrep::default();
        let Err(CodecError::ResourceLimit(first)) = map(&ctx, &mut out, chain(3)) else {
            panic!("the next operand exceeds the active child depth");
        };
        assert_eq!(first.dimension, ResourceDimension::RecursionDepth);
        assert_eq!(first.operation, "ASM law expression operand");
        assert_eq!((first.limit, first.used, first.additional), (cap, cap, 1));
        for _ in 0..64 {
            assert!(matches!(map(&ctx, &mut out, chain(3)),
                Err(CodecError::ResourceLimit(last)) if last == first));
            assert!(matches!(map(&ctx, &mut out, EmbeddedLawExpression::Null),
                Err(CodecError::ResourceLimit(last)) if last == first));
        }
        assert!(out.curves.is_empty() && out.surfaces.is_empty());
        assert!(matches!(ctx.finish_session(),
            Err(CodecError::ResourceLimit(last)) if last == first));
    }
}

#[test]
fn law_expression_releases_child_depth_for_siblings() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // VEC has three COS operands. Each branch has two descents, not six.
    policy.limits.max_recursion_depth = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut out = AsmBrep::default();
    let expression = EmbeddedLawExpression::Algebraic {
        operator: "VEC".into(), operands: vec![chain(1), chain(1), chain(1)],
    };
    let expected = LawExpression::Algebraic {
        operator: "VEC".into(), operands: vec![expected_chain(1), expected_chain(1), expected_chain(1)],
    };
    assert_eq!(map(&ctx, &mut out, expression).unwrap(), expected);
    assert_eq!(map(&ctx, &mut out, chain(2)).unwrap(), expected_chain(2));
    assert!(out.curves.is_empty() && out.surfaces.is_empty());
    ctx.finish_session().unwrap();
}

#[test]
fn law_expression_uses_actual_caller_depth() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let caller_depth = ctx.enter_nested("test actual law caller").unwrap();
    let mut out = AsmBrep::default();
    assert_eq!(map(&ctx, &mut out, chain(1)).unwrap(), expected_chain(1));
    let Err(CodecError::ResourceLimit(first)) = map(&ctx, &mut out, chain(2)) else {
        panic!("the second child shares the live caller depth");
    };
    assert_eq!(first.dimension, ResourceDimension::RecursionDepth);
    assert_eq!(first.operation, "ASM law expression operand");
    assert_eq!((first.limit, first.used, first.additional), (2, 2, 1));
    drop(caller_depth);
    assert!(matches!(map(&ctx, &mut out, EmbeddedLawExpression::Null),
        Err(CodecError::ResourceLimit(last)) if last == first));
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn law_expression_empty_and_scalar_execute_no_child_descent() {
    crate::test_support::with_entry_context(|ctx, original| {
        for expression in [EmbeddedLawExpression::Integer(7),
            EmbeddedLawExpression::Algebraic { operator: "COS".into(), operands: Vec::new() }] {
            let mut out = AsmBrep::default();
            let result = map(ctx, &mut out, expression);
            match original {
                Some(first) => assert!(matches!(result,
                    Err(CodecError::ResourceLimit(last)) if last == first)),
                None => match result.expect("no child work is free") {
                    LawExpression::Integer { value } => assert_eq!(value, 7),
                    LawExpression::Algebraic { operator, operands } => {
                        assert_eq!(operator, "COS");
                        assert!(operands.is_empty());
                    }
                    _ => panic!("preserve scalar or empty recovery"),
                },
            }
            assert!(out.curves.is_empty() && out.surfaces.is_empty());
        }
    });
}

#[test]
fn law_expression_children_preserve_every_original_refusal() {
    crate::test_support::with_entry_context(|ctx, original| {
        let Some(first) = original else { return; };
        let mut out = AsmBrep::default();
        assert!(matches!(map(ctx, &mut out, chain(3)),
            Err(CodecError::ResourceLimit(last)) if last == first));
        assert!(out.curves.is_empty() && out.surfaces.is_empty());
    });
}
