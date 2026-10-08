// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
const EPS_NONLINEAR_VALUE: f64 = 1.0e-9;

use crate::curve::tests::compile_solve_program as curve_expression_solve_program;
use crate::curve::expression_records;
use crate::curve::quantity_value;
use crate::curve::solve_unique_affine_system;
use crate::curve::tests::evaluate_expression_program;
use crate::curve::AffineEquationRow;
use crate::curve::CurveExpressionEquation;
use crate::curve::CurveExpressionLine;
use crate::curve::CurveExpressionSolveBlock;
use crate::curve::CurveExpressionValue;
use crate::curve::ExternalRelationSymbols;
use crate::curve::RelationDimension;
use crate::curve::RelationEvaluationContext;
use crate::curve::SolveUnknown;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn affine_math_arguments_fit_fixed_three_slot_frame() {
    use crate::curve::{AffineValue, CreoMathFunction, ExpressionValue};

    let values = [
        AffineValue {
            constant: 1.0,
            linear: 0.0,
        },
        AffineValue {
            constant: 2.0,
            linear: 0.0,
        },
        AffineValue {
            constant: 3.0,
            linear: 0.0,
        },
    ];
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    assert_eq!(
        AffineValue::function_checked(
            CreoMathFunction::If,
            None,
            &values,
            RelationEvaluationContext::default(),
            &ctx,
        )
        .expect("fixed frame needs no collection"),
        Some(values[1]),
    );
    assert_eq!(
        AffineValue::function_checked(
            CreoMathFunction::If,
            None,
            &[values[0], values[1], values[2], values[0]],
            RelationEvaluationContext::default(),
            &ctx,
        )
        .expect("unsupported arity needs no collection"),
        None,
    );
}

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
        crate::curve::infer_solve_variable_dimensions(
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
    dimension_inference_refuses_equation_coefficient_work,
    DimensionLimitCase::Basic,
    ResourceDimension::WorkUnits,
    "creo dimension equation coefficient work"
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
            crate::curve::infer_solve_variable_dimensions(
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

fn evaluate_expression_program_details(
    lines: &[CurveExpressionLine],
    model_name: Option<&str>,
    external_symbols: &ExternalRelationSymbols,
) -> crate::curve::CurveExpressionEvaluation {
    crate::decode::with_test_decode_ctx(|ctx| {
        crate::curve::tests::evaluate_program_details(ctx, lines, model_name, external_symbols)
    })
    .expect("test curve expression evaluation")
}

fn infer_solve_variable_dimensions(
    block: &CurveExpressionSolveBlock,
    values: &BTreeMap<String, CurveExpressionValue>,
    known_dimensions: &[Option<RelationDimension>],
    context: RelationEvaluationContext<'_>,
) -> Option<Vec<RelationDimension>> {
    crate::decode::with_test_decode_ctx(|ctx| {
        crate::curve::infer_solve_variable_dimensions(ctx, block, values, known_dimensions, context)
    })
    .expect("test dimension inference")
}

#[test]
fn decodes_counted_curve_expression_source_lines() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x89\x4c\
        \xe0\x0aexpression\0\xf8\x04r=5\0theta=t*360\0z=71*t\0q=r+2*(3)\0\
        \xe0\x00backup_ents(crv_fr_eqn)\0\xe3\xe0\x01id\0\0\
        \xe0\x0aexpression\0\xf8\x01r=5\0";
    let records = expression_records(payload);
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].entity_id, 0x094c);
    assert!(!records[0].backup);
    assert_eq!(
        records[0]
            .lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        ["r=5", "theta=t*360", "z=71*t", "q=r+2*(3)"]
    );
    assert!(records[1].backup);
    assert_eq!(records[1].lines[0].text, "r=5");
    assert!(records[0].lines[0].offset < records[0].lines[1].offset);
    assert_eq!(records[0].assignments.len(), 4);
    assert_eq!(
        records[0].assignments[0].parameter_target(),
        Some(("r", None))
    );
    assert_eq!(records[0].assignments[0].expression, "5");
    assert!(records[0].assignments[0].dependencies.is_empty());
    assert_eq!(
        records[0].assignments[0].value,
        Some(CurveExpressionValue::Number(
            cadmpeg_ir::scalar::FiniteReal::new(5.0).expect("finite relation fixture")
        ))
    );
    assert_eq!(
        records[0].assignments[1].parameter_target(),
        Some(("theta", None))
    );
    assert_eq!(records[0].assignments[1].expression, "t*360");
    assert_eq!(records[0].assignments[1].dependencies, ["t"]);
    assert_eq!(records[0].assignments[1].value, None);
    assert_eq!(records[0].assignments[2].value, None);
    assert_eq!(records[0].assignments[3].dependencies, ["r"]);
    assert_eq!(
        records[0].assignments[3].value,
        Some(CurveExpressionValue::Number(
            cadmpeg_ir::scalar::FiniteReal::new(11.0).expect("finite relation fixture")
        ))
    );
}

#[test]
fn standalone_equality_does_not_create_an_assignment() {
    let lines = ["ghost==missing", "seen=exists('ghost')", "flag=1==1"]
        .into_iter()
        .enumerate()
        .map(|(offset, text)| CurveExpressionLine {
            text: text.to_owned(),
            offset,
        })
        .collect::<Vec<_>>();

    let assignments =
        evaluate_expression_program(&lines, None, &ExternalRelationSymbols::default());

    assert_eq!(assignments.len(), 2);
    assert_eq!(assignments[0].parameter_target(), Some(("seen", None)));
    assert_eq!(assignments[0].value, None);
    assert_eq!(assignments[1].parameter_target(), Some(("flag", None)));
    assert_eq!(
        assignments[1].value,
        Some(CurveExpressionValue::Number(
            cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite relation fixture")
        ))
    );
}

#[test]
fn retains_simultaneous_equations_without_sequential_assignments() {
    let lines = [
        "area=100",
        "base=10",
        "width=99",
        "SOLVE",
        "width=height+1",
        "offset=base+1",
        "width*height=area",
        "FOR width, height",
        "present=exists('width')",
        "after_width=width",
        "result=area+1",
    ]
    .into_iter()
    .enumerate()
    .map(|(offset, text)| CurveExpressionLine {
        text: text.to_owned(),
        offset,
    })
    .collect::<Vec<_>>();

    let program =
        crate::decode::with_test_decode_ctx(|ctx| curve_expression_solve_program(ctx, &lines))
            .expect("solve program");
    assert!(!program.unresolved_control);
    let [block] = program.blocks.as_slice() else {
        panic!("one solve block");
    };
    assert_eq!(
        block
            .unknowns
            .iter()
            .map(|unknown| unknown.name.as_str())
            .collect::<Vec<_>>(),
        ["width", "height"]
    );
    assert_eq!(block.offset, 3);
    assert_eq!(block.for_offset, 7);
    assert_eq!(block.equations.len(), 2);
    assert_eq!(block.equations[0].left, "width");
    assert_eq!(block.equations[0].right, "height+1");
    assert_eq!(block.equations[0].dependencies, ["width", "height"]);
    assert_eq!(block.equations[1].left, "width*height");
    assert_eq!(block.equations[1].right, "area");
    assert_eq!(block.equations[1].dependencies, ["width", "height", "area"]);
    assert_eq!(block.assignments.len(), 1);
    assert_eq!(
        block.assignments[0].parameter_target(),
        Some(("offset", None))
    );
    assert_eq!(block.assignments[0].expression, "base+1");
    assert_eq!(block.assignments[0].dependencies, ["base"]);

    let assignments =
        evaluate_expression_program(&lines, None, &ExternalRelationSymbols::default());
    assert_eq!(assignments.len(), 7);
    assert_eq!(assignments[0].parameter_target(), Some(("area", None)));
    assert_eq!(assignments[1].parameter_target(), Some(("base", None)));
    assert_eq!(assignments[2].parameter_target(), Some(("width", None)));
    assert_eq!(assignments[2].value, None);
    assert_eq!(assignments[3].parameter_target(), Some(("offset", None)));
    assert_eq!(
        assignments[3].value,
        Some(CurveExpressionValue::Number(
            cadmpeg_ir::scalar::FiniteReal::new(11.0).expect("finite relation fixture")
        ))
    );
    assert_eq!(assignments[4].parameter_target(), Some(("present", None)));
    assert_eq!(
        assignments[4].value,
        Some(CurveExpressionValue::Number(
            cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite relation fixture")
        ))
    );
    assert_eq!(
        assignments[5].parameter_target(),
        Some(("after_width", None))
    );
    assert_eq!(assignments[5].value, None);
    assert_eq!(assignments[6].parameter_target(), Some(("result", None)));
    assert_eq!(
        assignments[6].value,
        Some(CurveExpressionValue::Number(
            cadmpeg_ir::scalar::FiniteReal::new(101.0).expect("finite relation fixture")
        ))
    );
}

#[test]
fn solves_complete_affine_simultaneous_equations() {
    let lines = [
        "x=0",
        "y=0",
        "sum=10",
        "SOLVE",
        "x+y=sum",
        "x-y=2",
        "FOR x,y",
        "product=x*y",
    ]
    .into_iter()
    .enumerate()
    .map(|(offset, text)| CurveExpressionLine {
        text: text.to_owned(),
        offset,
    })
    .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert_eq!(
        evaluation.solve_solutions[&3],
        [
            CurveExpressionValue::Number(
                cadmpeg_ir::scalar::FiniteReal::new(6.0).expect("finite relation fixture")
            ),
            CurveExpressionValue::Number(
                cadmpeg_ir::scalar::FiniteReal::new(4.0).expect("finite relation fixture")
            ),
        ]
    );
    assert_eq!(evaluation.assignments.len(), 4);
    assert_eq!(
        evaluation
            .assignments
            .iter()
            .map(|assignment| assignment.value.clone())
            .collect::<Vec<_>>(),
        [
            Some(CurveExpressionValue::Number(
                cadmpeg_ir::scalar::FiniteReal::new(6.0).expect("finite relation fixture")
            )),
            Some(CurveExpressionValue::Number(
                cadmpeg_ir::scalar::FiniteReal::new(4.0).expect("finite relation fixture")
            )),
            Some(CurveExpressionValue::Number(
                cadmpeg_ir::scalar::FiniteReal::new(10.0).expect("finite relation fixture")
            )),
            Some(CurveExpressionValue::Number(
                cadmpeg_ir::scalar::FiniteReal::new(24.0).expect("finite relation fixture")
            )),
        ]
    );
}

#[test]
fn solves_affine_systems_without_previous_numeric_values() {
    let lines = ["SOLVE", "x+y=10[mm]", "x-y=2[mm]", "FOR x,y", "sum=x+y"]
        .into_iter()
        .enumerate()
        .map(|(offset, text)| CurveExpressionLine {
            text: text.to_owned(),
            offset,
        })
        .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert_eq!(
        evaluation.solve_solutions[&0],
        [
            CurveExpressionValue::Length(
                cadmpeg_ir::scalar::FiniteReal::new(6.0).expect("finite relation fixture")
            ),
            CurveExpressionValue::Length(
                cadmpeg_ir::scalar::FiniteReal::new(4.0).expect("finite relation fixture")
            ),
        ]
    );
    assert_eq!(
        evaluation.assignments[0].value,
        Some(CurveExpressionValue::Length(
            cadmpeg_ir::scalar::FiniteReal::new(10.0).expect("finite relation fixture")
        ))
    );
}

#[test]
fn infers_missing_solve_dimensions_through_known_quantities() {
    let lines = [
        "speed=2[mm/s]",
        "total=10[mm]",
        "SOLVE",
        "distance+speed*duration=total",
        "duration=2[s]",
        "FOR distance,duration",
    ]
    .into_iter()
    .enumerate()
    .map(|(offset, text)| CurveExpressionLine {
        text: text.to_owned(),
        offset,
    })
    .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert_eq!(
        evaluation.solve_solutions[&2],
        [
            CurveExpressionValue::Length(
                cadmpeg_ir::scalar::FiniteReal::new(6.0).expect("finite relation fixture")
            ),
            quantity_value(2.0, RelationDimension::TIME).expect("finite relation fixture"),
        ]
    );
}

#[test]
fn leaves_free_solve_dimensions_unresolved() {
    let lines = ["SOLVE", "x+y=y", "x-y=x", "FOR x,y"]
        .into_iter()
        .enumerate()
        .map(|(offset, text)| CurveExpressionLine {
            text: text.to_owned(),
            offset,
        })
        .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert!(evaluation.solve_solutions.is_empty());
    assert!(evaluation.assignments.is_empty());
}

#[test]
fn rejects_missing_solve_dimensions_with_conflicting_units() {
    let lines = ["SOLVE", "x=1[mm]", "x=1[s]", "FOR x"]
        .into_iter()
        .enumerate()
        .map(|(offset, text)| CurveExpressionLine {
            text: text.to_owned(),
            offset,
        })
        .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert!(evaluation.solve_solutions.is_empty());
    assert!(evaluation.assignments.is_empty());
}

#[test]
fn leaves_underdetermined_affine_systems_unsolved() {
    let lines = ["x=0", "y=0", "SOLVE", "x+y=10", "FOR x,y"]
        .into_iter()
        .enumerate()
        .map(|(offset, text)| CurveExpressionLine {
            text: text.to_owned(),
            offset,
        })
        .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert!(evaluation.solve_solutions.is_empty());
    assert_eq!(evaluation.assignments[0].value, None);
    assert_eq!(evaluation.assignments[1].value, None);
}

#[test]
fn rejects_overflow_when_reducing_affine_comparison() {
    let overflow = [
        "x=0",
        "limit=1e308",
        "SOLVE",
        "if(limit-(-limit)>0,x,0)=1",
        "FOR x",
    ]
    .into_iter()
    .enumerate()
    .map(|(offset, text)| CurveExpressionLine {
        text: text.to_owned(),
        offset,
    })
    .collect::<Vec<_>>();
    let evaluation =
        evaluate_expression_program_details(&overflow, None, &ExternalRelationSymbols::default());
    assert!(evaluation.solve_solutions.is_empty());
    assert_eq!(evaluation.assignments[0].value, None);
}

#[test]
fn solves_affine_systems_with_fixed_boolean_annihilators() {
    let lines = ["x=0", "y=0", "SOLVE", "x=3", "y+(0&x)+(x&0)=4", "FOR x,y"]
        .into_iter()
        .enumerate()
        .map(|(offset, text)| CurveExpressionLine {
            text: text.to_owned(),
            offset,
        })
        .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert_eq!(
        evaluation.solve_solutions[&2],
        [
            CurveExpressionValue::Number(
                cadmpeg_ir::scalar::FiniteReal::new(3.0).expect("finite relation fixture")
            ),
            CurveExpressionValue::Number(
                cadmpeg_ir::scalar::FiniteReal::new(4.0).expect("finite relation fixture")
            ),
        ]
    );

    let lines = ["x=0", "y=0", "SOLVE", "x=3", "y+(1|x)+(x|1)=6", "FOR x,y"]
        .into_iter()
        .enumerate()
        .map(|(offset, text)| CurveExpressionLine {
            text: text.to_owned(),
            offset,
        })
        .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert_eq!(
        evaluation.solve_solutions[&2],
        [
            CurveExpressionValue::Number(
                cadmpeg_ir::scalar::FiniteReal::new(3.0).expect("finite relation fixture")
            ),
            CurveExpressionValue::Number(
                cadmpeg_ir::scalar::FiniteReal::new(4.0).expect("finite relation fixture")
            ),
        ]
    );
}

#[test]
fn solves_affine_systems_with_fixed_function_powers() {
    let lines = [
        "x=0[mm]",
        "y=0",
        "SOLVE",
        "pow(x,1)=3[mm]",
        "y+pow(x,0)=5",
        "FOR x,y",
    ]
    .into_iter()
    .enumerate()
    .map(|(offset, text)| CurveExpressionLine {
        text: text.to_owned(),
        offset,
    })
    .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert_eq!(
        evaluation.solve_solutions[&2],
        [
            CurveExpressionValue::Length(
                cadmpeg_ir::scalar::FiniteReal::new(3.0).expect("finite relation fixture")
            ),
            CurveExpressionValue::Number(
                cadmpeg_ir::scalar::FiniteReal::new(4.0).expect("finite relation fixture")
            ),
        ]
    );
}

#[test]
fn solves_affine_systems_with_branch_and_sign_invariants() {
    let lines = [
        "x=0",
        "y=0[mm]",
        "SOLVE",
        "x=3",
        "if(x,y,y)+sign(0[mm],x)=4[mm]",
        "FOR x,y",
    ]
    .into_iter()
    .enumerate()
    .map(|(offset, text)| CurveExpressionLine {
        text: text.to_owned(),
        offset,
    })
    .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert_eq!(
        evaluation.solve_solutions[&2],
        [
            CurveExpressionValue::Number(
                cadmpeg_ir::scalar::FiniteReal::new(3.0).expect("finite relation fixture")
            ),
            CurveExpressionValue::Length(
                cadmpeg_ir::scalar::FiniteReal::new(4.0).expect("finite relation fixture")
            ),
        ]
    );
}

#[test]
fn leaves_inconsistent_affine_systems_unsolved() {
    let lines = [
        "x=0", "y=0", "SOLVE", "x+y=10", "x-y=2", "x+y=11", "FOR x,y",
    ]
    .into_iter()
    .enumerate()
    .map(|(offset, text)| CurveExpressionLine {
        text: text.to_owned(),
        offset,
    })
    .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert!(evaluation.solve_solutions.is_empty());
    assert_eq!(evaluation.assignments[0].value, None);
    assert_eq!(evaluation.assignments[1].value, None);
}

#[test]
fn solves_unique_nonlinear_simultaneous_equations() {
    let lines = ["x=2", "SOLVE", "x*x*x=8", "FOR x", "after=x+1"]
        .into_iter()
        .enumerate()
        .map(|(offset, text)| CurveExpressionLine {
            text: text.to_owned(),
            offset,
        })
        .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    let [CurveExpressionValue::Number(solution)] = evaluation.solve_solutions[&1].as_slice() else {
        panic!("expected one numeric nonlinear solution");
    };
    assert!((solution.get() - 2.0).abs() <= EPS_NONLINEAR_VALUE);
    let Some(CurveExpressionValue::Number(after)) = &evaluation.assignments[1].value else {
        panic!("expected evaluated assignment after nonlinear solve");
    };
    assert!((after.get() - 3.0).abs() <= EPS_NONLINEAR_VALUE);
}

#[test]
fn rejects_nonlinear_roots_with_rank_deficient_jacobian() {
    let lines = ["x=0", "SOLVE", "x*x*x=0", "FOR x"]
        .into_iter()
        .enumerate()
        .map(|(offset, text)| CurveExpressionLine {
            text: text.to_owned(),
            offset,
        })
        .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert!(evaluation.solve_solutions.is_empty());
    assert_eq!(evaluation.assignments.len(), 1);
    assert_eq!(evaluation.assignments[0].value, None);
}

#[test]
fn leaves_nonlinear_systems_without_previous_values_unsolved() {
    let lines = ["SOLVE", "x*x*x=8", "FOR x"]
        .into_iter()
        .enumerate()
        .map(|(offset, text)| CurveExpressionLine {
            text: text.to_owned(),
            offset,
        })
        .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert!(evaluation.solve_solutions.is_empty());
    assert!(evaluation.assignments.is_empty());
}

#[test]
fn rejects_nonlinear_systems_with_multiple_roots() {
    let lines = ["x=1", "SOLVE", "x*x=4", "FOR x"]
        .into_iter()
        .enumerate()
        .map(|(offset, text)| CurveExpressionLine {
            text: text.to_owned(),
            offset,
        })
        .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert!(evaluation.solve_solutions.is_empty());
    assert_eq!(evaluation.assignments.len(), 1);
    assert_eq!(evaluation.assignments[0].value, None);
}

#[test]
fn leaves_discrete_unary_not_forms_unsolved() {
    let lines = ["x=1", "SOLVE", "~x=0", "x=1", "FOR x"]
        .into_iter()
        .enumerate()
        .map(|(offset, text)| CurveExpressionLine {
            text: text.to_owned(),
            offset,
        })
        .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert!(evaluation.solve_solutions.is_empty());
    assert_eq!(evaluation.assignments[0].value, None);
}

#[test]
fn affine_solver_is_invariant_under_independent_equation_scaling() {
    let mut rows = [
        AffineEquationRow {
            coefficients: vec![1e-15, 0.0],
            rhs: 6e-15,
        },
        AffineEquationRow {
            coefficients: vec![0.0, 1e15],
            rhs: 4e15,
        },
    ];

    let solution =
        crate::decode::with_test_decode_ctx(|ctx| solve_unique_affine_system(ctx, &mut rows, 2))
            .expect("service profile")
            .expect("independently scaled unique system");
    assert!((solution[0] - 6.0).abs() <= 1.0e-12);
    assert!((solution[1] - 4.0).abs() <= 1.0e-12);
}

#[test]
fn solves_dimensioned_affine_simultaneous_equations() {
    let lines = [
        "x=0[mm]",
        "y=0[mm]",
        "SOLVE",
        "x+y=10[mm]",
        "x-y=2[mm]",
        "FOR x,y",
        "area=x*y",
    ]
    .into_iter()
    .enumerate()
    .map(|(offset, text)| CurveExpressionLine {
        text: text.to_owned(),
        offset,
    })
    .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert_eq!(
        evaluation.solve_solutions[&2],
        [
            CurveExpressionValue::Length(
                cadmpeg_ir::scalar::FiniteReal::new(6.0).expect("finite relation fixture")
            ),
            CurveExpressionValue::Length(
                cadmpeg_ir::scalar::FiniteReal::new(4.0).expect("finite relation fixture")
            ),
        ]
    );
    assert_eq!(
        evaluation.assignments[0].value,
        Some(CurveExpressionValue::Length(
            cadmpeg_ir::scalar::FiniteReal::new(6.0).expect("finite relation fixture")
        ))
    );
    assert_eq!(
        evaluation.assignments[1].value,
        Some(CurveExpressionValue::Length(
            cadmpeg_ir::scalar::FiniteReal::new(4.0).expect("finite relation fixture")
        ))
    );
    assert_eq!(
        evaluation.assignments[2].value,
        Some(
            quantity_value(
                24.0,
                RelationDimension::LENGTH
                    .scale(2)
                    .expect("squared length dimension")
            )
            .expect("finite relation fixture")
        )
    );
}

#[test]
fn solves_affine_piecewise_expressions_with_unknown_independent_branches() {
    let lines = [
        "x=0[mm]",
        "y=0[mm]",
        "SOLVE",
        "min(x+2[mm],x+5[mm])+y=10[mm]",
        "max(x-1[mm],x-3[mm])-y=3[mm]",
        "if(x+1[mm]>x,x,x+100[mm])-y=4[mm]",
        "FOR x,y",
    ]
    .into_iter()
    .enumerate()
    .map(|(offset, text)| CurveExpressionLine {
        text: text.to_owned(),
        offset,
    })
    .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert_eq!(
        evaluation.solve_solutions[&2],
        [
            CurveExpressionValue::Length(
                cadmpeg_ir::scalar::FiniteReal::new(6.0).expect("finite relation fixture")
            ),
            CurveExpressionValue::Length(
                cadmpeg_ir::scalar::FiniteReal::new(2.0).expect("finite relation fixture")
            ),
        ]
    );
}

#[test]
fn leaves_unknown_dependent_piecewise_systems_unsolved() {
    let lines = [
        "x=0[mm]",
        "y=0[mm]",
        "SOLVE",
        "min(x,-x)+y=10[mm]",
        "x-y=2[mm]",
        "FOR x,y",
    ]
    .into_iter()
    .enumerate()
    .map(|(offset, text)| CurveExpressionLine {
        text: text.to_owned(),
        offset,
    })
    .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert!(evaluation.solve_solutions.is_empty());
    assert_eq!(evaluation.assignments[0].value, None);
    assert_eq!(evaluation.assignments[1].value, None);
}

#[test]
fn solves_parallel_affine_clamps_deadbands_and_tolerance_tests() {
    let lines = [
        "x=0[mm]",
        "y=0[mm]",
        "SOLVE",
        "bound(x+2[mm],x+1[mm],x+3[mm])+y=10[mm]",
        "dead(x+4[mm],x+1[mm],x+3[mm])-y=1[mm]",
        "near(x+1[mm],x+2[mm],1[mm])=1",
        "dbl_in_tol(x+2[mm],x+1[mm],1[mm])=1",
        "FOR x,y",
    ]
    .into_iter()
    .enumerate()
    .map(|(offset, text)| CurveExpressionLine {
        text: text.to_owned(),
        offset,
    })
    .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert_eq!(
        evaluation.solve_solutions[&2],
        [
            CurveExpressionValue::Length(
                cadmpeg_ir::scalar::FiniteReal::new(8.0).expect("finite relation fixture")
            ),
            CurveExpressionValue::Length(
                cadmpeg_ir::scalar::FiniteReal::new(0.0).expect("finite relation fixture")
            ),
        ]
    );
}

#[test]
fn leaves_unknown_dependent_clamps_unsolved() {
    let lines = [
        "x=0[mm]",
        "y=0[mm]",
        "SOLVE",
        "bound(x,0[mm],10[mm])+y=10[mm]",
        "x-y=2[mm]",
        "FOR x,y",
    ]
    .into_iter()
    .enumerate()
    .map(|(offset, text)| CurveExpressionLine {
        text: text.to_owned(),
        offset,
    })
    .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert!(evaluation.solve_solutions.is_empty());
    assert_eq!(evaluation.assignments[0].value, None);
    assert_eq!(evaluation.assignments[1].value, None);
}

#[test]
fn solves_affine_systems_with_different_unknown_dimensions() {
    let lines = [
        "distance=0[mm]",
        "duration=0[s]",
        "speed=2[mm/s]",
        "total=10[mm]",
        "SOLVE",
        "distance+speed*duration=total",
        "duration=2[s]",
        "FOR distance,duration",
    ]
    .into_iter()
    .enumerate()
    .map(|(offset, text)| CurveExpressionLine {
        text: text.to_owned(),
        offset,
    })
    .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert_eq!(
        evaluation.solve_solutions[&4],
        [
            CurveExpressionValue::Length(
                cadmpeg_ir::scalar::FiniteReal::new(6.0).expect("finite relation fixture")
            ),
            quantity_value(2.0, RelationDimension::TIME).expect("finite relation fixture"),
        ]
    );
}

#[test]
fn infers_independent_dimensions_for_untyped_solve_variables() {
    let lines = [
        "speed=2[mm/s]",
        "total=10[mm]",
        "SOLVE",
        "distance+speed*duration=total",
        "duration=2[s]",
        "FOR distance,duration",
    ]
    .into_iter()
    .enumerate()
    .map(|(offset, text)| CurveExpressionLine {
        text: text.to_owned(),
        offset,
    })
    .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert_eq!(
        evaluation.solve_solutions[&2],
        [
            CurveExpressionValue::Length(
                cadmpeg_ir::scalar::FiniteReal::new(6.0).expect("finite relation fixture")
            ),
            quantity_value(2.0, RelationDimension::TIME).expect("finite relation fixture"),
        ]
    );
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
    let error = crate::test_support::last_refusal_at(&[], ResourceDimension::CollectionItems, "creo_solve_dimension_components", |ctx| {
        crate::curve::infer_solve_variable_dimensions(ctx, &block, &BTreeMap::new(), &[Some(RelationDimension::LENGTH)], RelationEvaluationContext::default())
    });
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo_solve_dimension_components"
                && limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
    ));
}

#[test]
fn dimension_axis_refuses_collection_limit() {
    let error = crate::test_support::last_refusal_at(&[], ResourceDimension::CollectionItems, "creo_solve_dimension_axis", |ctx| {
        let mut rows = vec![AffineEquationRow { coefficients: vec![1.0], rhs: 2.0 }];
        crate::curve::solve_dimension_axis(ctx, &mut rows, 1, &BTreeSet::from([0]))
    });
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo_solve_dimension_axis"
                && limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
    ));
}

#[test]
fn dimension_pivot_rows_refuse_collection_limit() {
    let error = crate::test_support::last_refusal_at(&[], ResourceDimension::CollectionItems, "creo solve dimension pivot rows", |ctx| {
        let mut rows = vec![AffineEquationRow { coefficients: vec![1.0], rhs: 2.0 }];
        crate::curve::solve_dimension_axis(ctx, &mut rows, 1, &BTreeSet::from([0]))
    });
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo solve dimension pivot rows"
                && limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
    ));
}

fn nonlinear_seed_error(operation: &'static str) -> cadmpeg_core::CodecError {
    let cap = crate::test_support::allocation_limit_at(ResourceDimension::CollectionItems, Some(operation), |cap| with_collection_limit(cap, |ctx| {
        crate::curve::nonlinear_initial_guesses(
            ctx,
            &[Some(CurveExpressionValue::Number(
                cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite relation fixture"),
            ))],
            &[RelationDimension::default()],
        )
    }));
    with_collection_limit(cap, |ctx| {
        crate::curve::nonlinear_initial_guesses(
            ctx,
            &[Some(CurveExpressionValue::Number(
                cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite relation fixture"),
            ))],
            &[RelationDimension::default()],
        )
    }).expect_err("nonlinear seed allocation exceeds the limit")
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

#[test]
fn preserves_reserved_quantity_dimensions_in_affine_systems() {
    let lines = [
        "acceleration=0[mm/s^2]",
        "SOLVE",
        "acceleration=G",
        "FOR acceleration",
    ]
    .into_iter()
    .enumerate()
    .map(|(offset, text)| CurveExpressionLine {
        text: text.to_owned(),
        offset,
    })
    .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert_eq!(
        evaluation.solve_solutions[&1],
        [quantity_value(9_800.0, RelationDimension::ACCELERATION)
            .expect("finite relation fixture")]
    );
}

#[test]
fn leaves_dimensionally_inconsistent_affine_systems_unsolved() {
    let lines = ["x=0[mm]", "SOLVE", "x=1[s]", "FOR x"]
        .into_iter()
        .enumerate()
        .map(|(offset, text)| CurveExpressionLine {
            text: text.to_owned(),
            offset,
        })
        .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert!(evaluation.solve_solutions.is_empty());
    assert_eq!(evaluation.assignments[0].value, None);
}

#[test]
fn unterminated_solve_block_cannot_create_assignments() {
    let lines = ["before=1", "SOLVE", "false_parameter=2", "after=3"]
        .into_iter()
        .enumerate()
        .map(|(offset, text)| CurveExpressionLine {
            text: text.to_owned(),
            offset,
        })
        .collect::<Vec<_>>();

    let program =
        crate::decode::with_test_decode_ctx(|ctx| curve_expression_solve_program(ctx, &lines))
            .expect("solve program");
    assert!(program.unresolved_control);
    assert!(program.blocks.is_empty());
    let assignments =
        evaluate_expression_program(&lines, None, &ExternalRelationSymbols::default());
    assert_eq!(assignments.len(), 1);
    assert_eq!(assignments[0].parameter_target(), Some(("before", None)));
}
