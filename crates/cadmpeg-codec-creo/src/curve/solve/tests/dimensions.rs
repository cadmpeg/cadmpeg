// SPDX-License-Identifier: Apache-2.0

use crate::curve::solve::AffineEquationRow;
use crate::curve::{
    CurveExpressionEquation, CurveExpressionSolveBlock, CurveExpressionValue, RelationDimension,
    RelationEvaluationContext, SolveUnknown,
};
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy)]
enum DimensionLimitCase {
    Basic,
    KnownNumber,
    KnownText,
}

fn dimension_inference_limit_reaches(
    case: DimensionLimitCase,
    dimension: ResourceDimension,
    operation: &'static str,
) -> bool {
    let block = CurveExpressionSolveBlock {
        equations: vec![CurveExpressionEquation {
            left: "1".to_owned(),
            right: "x".to_owned(),
            dependencies: vec!["x".to_owned()],
            offset: 0,
        }],
        assignments: Vec::new(),
        unknowns: vec![SolveUnknown {
            name: "x".to_owned(),
            solution: None,
        }],
        offset: 0,
        for_offset: 1,
    };
    let values = match case {
        DimensionLimitCase::Basic => BTreeMap::new(),
        DimensionLimitCase::KnownNumber => BTreeMap::from([(
            "driver".to_owned(),
            CurveExpressionValue::Number(
                cadmpeg_ir::scalar::FiniteReal::new(2.0).expect("finite relation fixture"),
            ),
        )]),
        DimensionLimitCase::KnownText => BTreeMap::from([(
            "driver".to_owned(),
            CurveExpressionValue::String("abc".to_owned()),
        )]),
    };
    let error = crate::test_support::last_refusal_at(&[], dimension, operation, |ctx| {
        crate::curve::solve::infer_solve_variable_dimensions(
            ctx,
            &block,
            &values,
            &[None],
            RelationEvaluationContext::default(),
        )
    });
    matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == dimension && resource.operation == operation)
}

macro_rules! dimension_limit_test {
    ($name:ident, $case:expr, $dimension:expr, $operation:literal) => {
        #[test]
        fn $name() {
            assert!(dimension_inference_limit_reaches(
                $case, $dimension, $operation
            ));
        }
    };
}

dimension_limit_test!(
    dimension_inference_refuses_variable_key_vector,
    DimensionLimitCase::Basic,
    ResourceDimension::CollectionItems,
    "creo dimension variable keys"
);
dimension_limit_test!(
    dimension_inference_refuses_variable_key_text,
    DimensionLimitCase::Basic,
    ResourceDimension::MaterializedBytes,
    "creo dimension variable key text"
);
dimension_limit_test!(
    dimension_inference_refuses_symbolic_variable_nodes,
    DimensionLimitCase::Basic,
    ResourceDimension::CollectionItems,
    "creo dimension variable nodes"
);
dimension_limit_test!(
    dimension_inference_refuses_symbolic_variable_names,
    DimensionLimitCase::Basic,
    ResourceDimension::MaterializedBytes,
    "creo dimension variable names"
);
dimension_limit_test!(
    dimension_inference_refuses_unknown_value_nodes,
    DimensionLimitCase::Basic,
    ResourceDimension::CollectionItems,
    "creo dimension unknown value nodes"
);
dimension_limit_test!(
    dimension_inference_refuses_unknown_value_names,
    DimensionLimitCase::Basic,
    ResourceDimension::MaterializedBytes,
    "creo dimension unknown value names"
);
dimension_limit_test!(
    dimension_inference_refuses_known_value_nodes,
    DimensionLimitCase::KnownNumber,
    ResourceDimension::CollectionItems,
    "creo dimension known value nodes"
);
dimension_limit_test!(
    dimension_inference_refuses_known_value_names,
    DimensionLimitCase::KnownNumber,
    ResourceDimension::MaterializedBytes,
    "creo dimension known value names"
);
dimension_limit_test!(
    dimension_inference_refuses_known_text,
    DimensionLimitCase::KnownText,
    ResourceDimension::MaterializedBytes,
    "creo dimension known text"
);
dimension_limit_test!(
    dimension_inference_refuses_constraint_rows,
    DimensionLimitCase::Basic,
    ResourceDimension::CollectionItems,
    "creo dimension constraint rows"
);
dimension_limit_test!(
    dimension_inference_refuses_axis_variable_keys,
    DimensionLimitCase::Basic,
    ResourceDimension::CollectionItems,
    "creo dimension axis variable keys"
);
dimension_limit_test!(
    dimension_inference_refuses_axis_variable_names,
    DimensionLimitCase::Basic,
    ResourceDimension::MaterializedBytes,
    "creo dimension axis variable names"
);
dimension_limit_test!(
    dimension_inference_refuses_equation_coefficients,
    DimensionLimitCase::Basic,
    ResourceDimension::CollectionItems,
    "creo dimension equation coefficients"
);
dimension_limit_test!(
    dimension_inference_refuses_equation_rows,
    DimensionLimitCase::Basic,
    ResourceDimension::CollectionItems,
    "creo dimension equation rows"
);
dimension_limit_test!(
    dimension_inference_refuses_difference_variable_nodes,
    DimensionLimitCase::Basic,
    ResourceDimension::CollectionItems,
    "creo dimension difference variable nodes"
);
dimension_limit_test!(
    dimension_inference_refuses_required_column_nodes,
    DimensionLimitCase::Basic,
    ResourceDimension::CollectionItems,
    "creo dimension required column nodes"
);
dimension_limit_test!(
    dimension_inference_refuses_inferred_dimensions,
    DimensionLimitCase::Basic,
    ResourceDimension::CollectionItems,
    "creo inferred variable dimensions"
);

#[test]
fn dimension_inference_refuses_duplicate_comparison_work() {
    let block = CurveExpressionSolveBlock {
        equations: Vec::new(),
        assignments: Vec::new(),
        unknowns: vec![
            SolveUnknown {
                name: "x".to_owned(),
                solution: None,
            },
            SolveUnknown {
                name: "X".to_owned(),
                solution: None,
            },
        ],
        offset: 0,
        for_offset: 1,
    };
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "creo dimension duplicate checks",
        |ctx| {
            crate::curve::solve::infer_solve_variable_dimensions(
                ctx,
                &block,
                &BTreeMap::new(),
                &[None, None],
                RelationEvaluationContext::default(),
            )
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo dimension duplicate checks"));
}

fn with_collection_limit<T>(
    limit: u64,
    run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("test decode context");
    run(&ctx)
}

fn infer_solve_variable_dimensions(
    block: &CurveExpressionSolveBlock,
    values: &BTreeMap<String, CurveExpressionValue>,
    known_dimensions: &[Option<RelationDimension>],
    context: RelationEvaluationContext<'_>,
) -> Option<Vec<RelationDimension>> {
    crate::decode::with_test_decode_ctx(|ctx| {
        crate::curve::solve::infer_solve_variable_dimensions(
            ctx,
            block,
            values,
            known_dimensions,
            context,
        )
    })
    .expect("test dimension inference")
}

#[test]
fn infers_integral_dimensions_through_sqrt_for_untyped_variables() {
    let block = CurveExpressionSolveBlock {
        equations: vec![CurveExpressionEquation {
            left: "sqrt(area)".to_owned(),
            right: "length".to_owned(),
            dependencies: vec!["area".to_owned(), "length".to_owned()],
            offset: 0,
        }],
        assignments: Vec::new(),
        unknowns: vec![SolveUnknown {
            name: "area".to_owned(),
            solution: None,
        }],
        offset: 0,
        for_offset: 1,
    };
    let values = BTreeMap::from([(
        "length".to_owned(),
        CurveExpressionValue::Length(
            cadmpeg_ir::scalar::FiniteReal::new(2.0).expect("finite relation fixture"),
        ),
    )]);

    assert_eq!(
        infer_solve_variable_dimensions(
            &block,
            &values,
            &[None],
            RelationEvaluationContext::default(),
        ),
        Some(vec![RelationDimension {
            length: 2,
            mass: 0,
            time: 0,
            angle: 0,
            temperature: 0,
        }]),
    );

    let mut non_integral = block;
    non_integral.equations[0].left = "area".to_owned();
    non_integral.equations[0].right = "sqrt(length)".to_owned();
    assert_eq!(
        infer_solve_variable_dimensions(
            &non_integral,
            &values,
            &[None],
            RelationEvaluationContext::default(),
        ),
        None,
    );
}

#[test]
fn dimension_components_refuse_collection_limit() {
    let block = CurveExpressionSolveBlock {
        equations: Vec::new(),
        assignments: Vec::new(),
        unknowns: vec![SolveUnknown {
            name: "length".to_owned(),
            solution: None,
        }],
        offset: 0,
        for_offset: 1,
    };
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::CollectionItems,
        "creo_solve_dimension_components",
        |ctx| {
            crate::curve::solve::infer_solve_variable_dimensions(
                ctx,
                &block,
                &BTreeMap::new(),
                &[Some(RelationDimension::LENGTH)],
                RelationEvaluationContext::default(),
            )
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo_solve_dimension_components"
                && limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
    ));
}

#[test]
fn dimension_axis_refuses_collection_limit() {
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::CollectionItems,
        "creo_solve_dimension_axis",
        |ctx| {
            let mut rows = vec![AffineEquationRow {
                coefficients: vec![1.0],
                rhs: 2.0,
            }];
            crate::curve::solve::solve_dimension_axis(ctx, &mut rows, 1, &BTreeSet::from([0]))
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo_solve_dimension_axis"
                && limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
    ));
}

#[test]
fn dimension_pivot_rows_refuse_collection_limit() {
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::CollectionItems,
        "creo solve dimension pivot rows",
        |ctx| {
            let mut rows = vec![AffineEquationRow {
                coefficients: vec![1.0],
                rhs: 2.0,
            }];
            crate::curve::solve::solve_dimension_axis(ctx, &mut rows, 1, &BTreeSet::from([0]))
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo solve dimension pivot rows"
                && limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
    ));
}

fn nonlinear_seed_error(operation: &'static str) -> cadmpeg_core::CodecError {
    let cap = crate::test_support::allocation_limit_at(
        ResourceDimension::CollectionItems,
        Some(operation),
        |cap| {
            with_collection_limit(cap, |ctx| {
                crate::curve::solve::nonlinear_initial_guesses(
                    ctx,
                    &[Some(CurveExpressionValue::Number(
                        cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite relation fixture"),
                    ))],
                    &[RelationDimension::default()],
                )
            })
        },
    );
    with_collection_limit(cap, |ctx| {
        crate::curve::solve::nonlinear_initial_guesses(
            ctx,
            &[Some(CurveExpressionValue::Number(
                cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite relation fixture"),
            ))],
            &[RelationDimension::default()],
        )
    })
    .expect_err("nonlinear seed allocation exceeds the limit")
}

#[test]
fn nonlinear_zero_seed_refuses_collection_limit() {
    assert!(matches!(
        nonlinear_seed_error("creo_solve_seed_zero"),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo_solve_seed_zero"
                && limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
    ));
}

#[test]
fn nonlinear_magnitude_seed_refuses_collection_limit() {
    assert!(matches!(
        nonlinear_seed_error("creo_solve_seed_magnitude"),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo_solve_seed_magnitude"
                && limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
    ));
}

#[test]
fn nonlinear_axis_seed_refuses_collection_limit() {
    assert!(matches!(
        nonlinear_seed_error("creo_solve_seed_axis"),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo_solve_seed_axis"
                && limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
    ));
}

#[test]
fn nonlinear_initial_seed_refuses_collection_limit() {
    assert!(matches!(
        nonlinear_seed_error("creo solve initial seed"),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo solve initial seed"
                && limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
    ));
}

#[test]
fn nonlinear_seed_rows_refuse_collection_limit() {
    assert!(matches!(
        nonlinear_seed_error("creo solve seed rows"),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo solve seed rows"
                && limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
    ));
}

fn dimension_conversion_result(
    name: &str,
) -> Result<Option<Vec<crate::curve::RelationDimension>>, CodecError> {
    use crate::curve::{
        CurveExpressionEquation, CurveExpressionSolveBlock, CurveExpressionValue, SolveUnknown,
    };
    let mut expression = name.to_owned();
    for _ in 0..8 {
        expression = format!("({expression}^127)");
    }
    let block = CurveExpressionSolveBlock {
        equations: vec![CurveExpressionEquation {
            left: expression,
            right: "1".to_owned(),
            dependencies: vec![name.to_owned()],
            offset: 0,
        }],
        assignments: Vec::new(),
        unknowns: vec![SolveUnknown {
            name: "x".to_owned(),
            solution: None,
        }],
        offset: 0,
        for_offset: 1,
    };
    let values = BTreeMap::from([(
        "length".to_owned(),
        CurveExpressionValue::Length(
            cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite relation fixture"),
        ),
    )]);
    crate::decode::with_test_decode_ctx(|ctx| {
        crate::curve::solve::infer_solve_variable_dimensions(
            ctx,
            &block,
            &values,
            &[None],
            RelationEvaluationContext::default(),
        )
    })
}

#[test]
fn dimension_inference_refuses_inexact_integer_coefficient() {
    let error =
        dimension_conversion_result("x").expect_err("127 to the eighth power is not exact in f64");
    assert!(matches!(error, CodecError::Malformed(_)));
}

#[test]
fn dimension_inference_refuses_inexact_integer_constant() {
    let error = dimension_conversion_result("length")
        .expect_err("127 to the eighth power is not exact in f64");
    assert!(matches!(error, CodecError::Malformed(_)));
}
