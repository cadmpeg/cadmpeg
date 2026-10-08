// SPDX-License-Identifier: Apache-2.0

use super::{assert_target_limit, parse, resource_error, ONE_COMMENT};
use crate::curve::test_support::{expression_lines, with_expression_policy};
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

#[test]
fn solve_synchronization_refuses_each_retained_value() {
    use crate::curve::{
        CurveExpressionActivation, CurveExpressionAssignment, CurveExpressionSolveBlock,
        CurveExpressionTarget, CurveExpressionValue, SolveUnknown,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let assignment = CurveExpressionAssignment {
        target: CurveExpressionTarget::Parameter {
            name: "a".into(),
            declared_unit: None,
        },
        expression: "\"ab\"".into(),
        dependencies: Vec::new(),
        value: Some(CurveExpressionValue::String("ab".into())),
        activation: CurveExpressionActivation::Active,
        offset: 1,
    };
    for (assignment_value, solution, operation) in [
        (
            Some(CurveExpressionValue::String("ab".into())),
            None,
            "creo synchronized assignment values",
        ),
        (
            None,
            Some(CurveExpressionValue::String("ab".into())),
            "creo synchronized solve values",
        ),
    ] {
        let mut evaluated = assignment.clone();
        evaluated.value = assignment_value;
        let solutions = solution.into_iter().map(|value| (0, vec![value])).collect();
        let mut blocks = vec![CurveExpressionSolveBlock {
            equations: Vec::new(),
            assignments: vec![assignment.clone()],
            unknowns: vec![SolveUnknown {
                name: "x".into(),
                solution: None,
            }],
            offset: 0,
            for_offset: 2,
        }];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
            ResourceDimension::RetainedBytes,
            Some(operation),
            |cap| {
                let mut trial = policy;
                trial.limits.max_retained_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &trial).expect("root");
                crate::curve::synchronize_solve_blocks(
                    &ctx,
                    &mut blocks.clone(),
                    &[evaluated.clone()],
                    &solutions,
                )
            },
        );
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert!(
            matches!(crate::curve::synchronize_solve_blocks(&ctx, &mut blocks, &[evaluated.clone()], &solutions),
            Err(cadmpeg_core::CodecError::ResourceLimit(resource))
            if resource.dimension == ResourceDimension::RetainedBytes && resource.operation == operation)
        );
        crate::decode::with_test_decode_ctx(|ctx| {
            crate::curve::synchronize_solve_blocks(
                ctx,
                &mut blocks,
                &[evaluated.clone()],
                &solutions,
            )
        })
        .expect("service synchronization");
        assert_eq!(blocks[0].assignments[0], evaluated);
        assert_eq!(
            blocks[0].unknowns[0].solution.as_ref(),
            solutions.get(&0).and_then(|values| values.first())
        );
    }
}

#[test]
fn expression_helix_required_outputs_refuse_scan_work() {
    let record = crate::curve::expression_records(
        b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\xe0\x0aexpression\0\xf8\x01a=1\0",
    )
    .pop()
    .expect("record");
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| crate::curve::expression_helix(ctx, &record))
            .expect("service")
            .is_none()
    );
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = crate::test_support::allocation_limit_at(
        ResourceDimension::WorkUnits,
        Some("creo helix output scan work"),
        |cap| {
            let mut trial = policy;
            trial.limits.max_work_units = cap;
            with_expression_policy(trial, |ctx| crate::curve::expression_helix(ctx, &record))
        },
    );
    let error = with_expression_policy(policy, |ctx| crate::curve::expression_helix(ctx, &record))
        .expect_err("output scan needs work");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "creo helix output scan work")
    );
}

#[test]
fn affine_equation_merge_propagates_coefficient_node_refusal() {
    use crate::curve::{RelationDimension, SimultaneousAffineValue};
    let left = SimultaneousAffineValue::constant(0.0, RelationDimension::default());
    let right = SimultaneousAffineValue {
        dimension: RelationDimension::default(),
        constant: 0.0,
        coefficients: BTreeMap::from([("y".to_owned(), 1.0)]),
    };
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
        ResourceDimension::CollectionItems,
        Some("creo affine combined coefficient nodes"),
        |cap| {
            let mut trial = policy;
            trial.limits.max_collection_items = cap;
            with_expression_policy(trial, |ctx| {
                left.clone().combine_admitted(right.clone(), true, ctx)
            })
        },
    );
    let error = resource_error(with_expression_policy(policy, |ctx| {
        let result = left.combine_admitted(right, true, ctx);
        assert_eq!(
            ctx.resource_refusal(),
            match &result {
                Err(CodecError::ResourceLimit(limit)) => Some(*limit),
                _ => None,
            }
        );
        result
    }));
    assert_target_limit(
        &error,
        ResourceDimension::CollectionItems,
        "creo affine combined coefficient nodes",
    );
    crate::decode::with_test_decode_ctx(|ctx| {
        let left = SimultaneousAffineValue::constant(2.0, RelationDimension::default());
        let right = SimultaneousAffineValue {
            dimension: RelationDimension::default(),
            constant: 1.0,
            coefficients: BTreeMap::from([("y".to_owned(), 1.0)]),
        };
        let difference = left
            .combine_admitted(right, true, ctx)
            .expect("service merge")
            .expect("equal dimensions");
        assert_eq!(difference.constant, 1.0);
        assert_eq!(
            difference.coefficients,
            BTreeMap::from([("y".to_owned(), -1.0)])
        );
    });
}

#[test]
fn relation_record_line_utf8_refuses_before_comment_skip() {
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo UTF-8 validation",
        |ctx| crate::curve::expression_records_with_model_name(ctx, ONE_COMMENT, None),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo UTF-8 validation")
    );
}

#[test]
fn prohibited_construct_trim_refuses_before_comment_skip() {
    let lines = expression_lines(&["  /* comment */  "]);
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "creo prohibited construct whitespace trim",
        |ctx| crate::curve::curve_equation_prohibited_constructs(ctx, &lines),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "creo prohibited construct whitespace trim"));
}

#[test]
fn unterminated_solve_program_needs_no_retained_storage() {
    let lines = expression_lines(&["SOLVE", "x=1"]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let program = with_expression_policy(policy, |ctx| {
        crate::curve::test_support::compile_solve_program(ctx, &lines)
    })
    .expect("no native block is retained");
    assert!(program.unresolved_control);
    assert!(program.blocks.is_empty());
}

#[test]
fn incomplete_expression_record_needs_no_retained_storage() {
    let payload =
        b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\xe0\x0aexpression\0\xf8\x02a=1\0b=2";
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    assert!(parse(payload, policy)
        .expect("incomplete lines are temporary")
        .is_empty());
}

#[test]
fn duplicate_solve_unknowns_need_no_retained_storage() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    assert!(
        with_expression_policy(policy, |ctx| crate::curve::curve_expression_solve_unknowns(
            ctx, "x, X"
        ))
        .expect("rejected names are temporary")
        .is_none()
    );
}

#[test]
fn zero_affine_addend_needs_no_new_coefficient_node() {
    let right = crate::curve::SimultaneousAffineValue {
        dimension: crate::curve::RelationDimension::default(),
        constant: 0.0,
        coefficients: BTreeMap::from([("x".to_owned(), 0.0)]),
    };
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let result = with_expression_policy(policy, |ctx| {
        crate::curve::SimultaneousAffineValue::constant(
            0.0,
            crate::curve::RelationDimension::default(),
        )
        .combine_admitted(right, false, ctx)
    })
    .expect("zero addend reserves no node")
    .expect("matching dimensions");
    assert!(result.coefficients.is_empty());
}

#[test]
fn zero_dimension_addend_needs_no_new_variable_node() {
    let right = crate::curve::DimensionForm {
        constant: crate::curve::DimensionRational::default(),
        variables: BTreeMap::from([("x".to_owned(), crate::curve::DimensionRational::default())]),
    };
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let result = with_expression_policy(policy, |ctx| {
        crate::curve::DimensionForm::default().combine_admitted(ctx, right, false)
    })
    .expect("zero addend reserves no node")
    .expect("valid rationals");
    assert!(result.variables.is_empty());
}

#[test]
fn external_relation_string_comparison_refuses_work() {
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "creo external relation value comparison",
        |ctx| {
            let mut symbols = crate::curve::ExternalRelationSymbols::default();
            symbols.observe(
                ctx,
                "source".to_owned(),
                Some(crate::curve::CurveExpressionValue::String(
                    "same".to_owned(),
                )),
            )?;
            symbols.observe(
                ctx,
                "source".to_owned(),
                Some(crate::curve::CurveExpressionValue::String(
                    "same".to_owned(),
                )),
            )?;
            Ok(())
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "creo external relation value comparison")
    );
}

#[test]
fn disabled_expression_values_use_temporary_storage() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\xe0\x0aexpression\0\xf8\x01message=itos(2)\0";
    let records =
        crate::test_support::assert_refusal_order(ResourceDimension::RetainedBytes, &[], |limit| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = limit;
            let result = parse(payload, policy);
            if let Err(CodecError::ResourceLimit(ref refusal)) = result {
                assert_ne!(refusal.operation, "creo evaluated assignment string");
            }
            result
        });
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].assignments.len(), 1);
    assert_eq!(records[0].assignments[0].value, None);
    assert_eq!(records[0].prohibited_constructs, ["itos"]);
}
