// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

const ONE_COMMENT: &[u8] = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
    \xe0\x0aexpression\0\xf8\x01/*x*/\0";
const WITH_LOCAL_SYSTEM: &[u8] = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
    \xe0\x02local_sys\0\xf9\x04\x03\xe4\x0f\x0f\x0f\x0f\x0f\x18\xe5\x0f\x0f\x0f\
    \xe0\x0aexpression\0\xf8\x01/*x*/\0";
const AFFINE_HELIX: &[u8] = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
    \xe0\x0aexpression\0\xf8\x03r=5\0theta=t*360\0z=20*t\0";

fn parse(
    payload: &[u8],
    policy: DecodePolicy,
) -> Result<Vec<super::super::CurveExpressionRecord>, CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(payload, &arena, &policy).expect("root input is admitted");
    super::super::expression_records_with_model_name(&ctx, payload, None)
}

fn with_expression_policy<T>(
    policy: DecodePolicy,
    run: impl FnOnce(&DecodeContext<'_>) -> Result<T, CodecError>,
) -> Result<T, CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("test input is admitted");
    run(&ctx)
}

fn expression_lines(source: &[&str]) -> Vec<super::super::CurveExpressionLine> {
    source
        .iter()
        .enumerate()
        .map(|(offset, text)| super::super::CurveExpressionLine {
            text: (*text).to_owned(),
            offset,
        })
        .collect()
}

fn resource_error<T>(result: Result<T, CodecError>) -> CodecError {
    match result {
        Err(error) => error,
        Ok(_) => panic!("expected resource refusal"),
    }
}

fn target_limit_error(
    source: &str,
    dimension: ResourceDimension,
    operation: &'static str,
) -> CodecError {
    crate::test_support::last_refusal_at(&[], dimension, operation, |ctx| {
        super::super::expression_assignment_target(ctx, source)
    })
}

fn assert_target_limit(error: &CodecError, dimension: ResourceDimension, operation: &'static str) {
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == dimension && limit.operation == operation));
}

#[test]
fn expression_parsed_arguments_refuse_before_growth() {
    assert_target_limit(
        &target_limit_error(
            "foo(x,y)",
            ResourceDimension::CollectionItems,
            "creo expression parsed arguments",
        ),
        ResourceDimension::CollectionItems,
        "creo expression parsed arguments",
    );
}

#[test]
fn expression_target_arguments_refuse_before_growth() {
    assert_target_limit(
        &target_limit_error(
            "foo(x,y)",
            ResourceDimension::CollectionItems,
            "creo expression target arguments",
        ),
        ResourceDimension::CollectionItems,
        "creo expression target arguments",
    );
}

#[test]
fn expression_target_argument_text_refuses_before_copy() {
    assert_target_limit(
        &target_limit_error(
            "foo(x)",
            ResourceDimension::RetainedBytes,
            "creo expression target argument text",
        ),
        ResourceDimension::RetainedBytes,
        "creo expression target argument text",
    );
}

#[test]
fn expression_function_target_refuses_before_copy() {
    assert_target_limit(
        &target_limit_error(
            "foo()",
            ResourceDimension::RetainedBytes,
            "creo expression function target",
        ),
        ResourceDimension::RetainedBytes,
        "creo expression function target",
    );
}

#[test]
fn expression_table_column_refuses_before_copy() {
    assert_target_limit(
        &target_limit_error(
            "value(p,r,c)",
            ResourceDimension::RetainedBytes,
            "creo expression table column",
        ),
        ResourceDimension::RetainedBytes,
        "creo expression table column",
    );
}

#[test]
fn expression_table_parameter_refuses_before_copy() {
    assert_target_limit(
        &target_limit_error(
            "value(p,r,c)",
            ResourceDimension::RetainedBytes,
            "creo expression table parameter",
        ),
        ResourceDimension::RetainedBytes,
        "creo expression table parameter",
    );
}

#[test]
fn expression_table_row_refuses_before_copy() {
    assert_target_limit(
        &target_limit_error(
            "value(p,r,c)",
            ResourceDimension::RetainedBytes,
            "creo expression table row",
        ),
        ResourceDimension::RetainedBytes,
        "creo expression table row",
    );
}

#[test]
fn expression_scoped_target_refuses_before_copy() {
    assert_target_limit(
        &target_limit_error(
            "a:b",
            ResourceDimension::RetainedBytes,
            "creo expression scoped target",
        ),
        ResourceDimension::RetainedBytes,
        "creo expression scoped target",
    );
}

#[test]
fn expression_system_target_refuses_before_copy() {
    assert_target_limit(
        &target_limit_error(
            "D1",
            ResourceDimension::RetainedBytes,
            "creo expression system target",
        ),
        ResourceDimension::RetainedBytes,
        "creo expression system target",
    );
}

#[test]
fn expression_declared_unit_refuses_before_copy() {
    assert_target_limit(
        &target_limit_error(
            "a[mm]",
            ResourceDimension::RetainedBytes,
            "creo expression declared unit",
        ),
        ResourceDimension::RetainedBytes,
        "creo expression declared unit",
    );
}

#[test]
fn expression_parameter_target_refuses_before_copy() {
    assert_target_limit(
        &target_limit_error(
            "a",
            ResourceDimension::RetainedBytes,
            "creo expression parameter target",
        ),
        ResourceDimension::RetainedBytes,
        "creo expression parameter target",
    );
}

#[test]
fn expression_dependency_items_refuse_before_growth() {
    let line = expression_lines(&["a=b+c"]);
    assert!(with_expression_policy(DecodePolicy::service(), |ctx| {
        super::super::expression_assignment(ctx, &line[0])
    })
    .expect("service profile")
    .is_some());
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::CollectionItems,
        "creo expression dependency names",
        |ctx| super::super::expression_assignment(ctx, &line[0]),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo expression dependency names"));
}

#[test]
fn expression_dependency_text_refuses_before_copy() {
    let line = expression_lines(&["a=b"]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        Some("creo expression dependency text"),
        |cap| {
            let mut trial_policy = policy;
            trial_policy.limits.max_retained_bytes = cap;
            with_expression_policy(trial_policy, |ctx| {
                super::super::expression_assignment(ctx, &line[0])
            })
        },
    );
    let error = with_expression_policy(policy, |ctx| {
        super::super::expression_assignment(ctx, &line[0])
    })
    .expect_err("dependency text exceeds retained limit");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo expression dependency text"));
}

#[test]
fn expression_assignment_text_refuses_before_copy() {
    let line = expression_lines(&["a=1"]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
        ResourceDimension::RetainedBytes,
        Some("creo expression assignment text"),
        |cap| {
            let mut trial = policy;
            trial.limits.max_retained_bytes = cap;
            with_expression_policy(trial, |ctx| {
                super::super::expression_assignment(ctx, &line[0])
            })
        },
    );
    let error = with_expression_policy(policy, |ctx| {
        super::super::expression_assignment(ctx, &line[0])
    })
    .expect_err("assignment text exceeds retained limit");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo expression assignment text"));
}

#[test]
fn solve_line_index_nodes_refuse_before_insert() {
    let lines = expression_lines(&["SOLVE"]);
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::CollectionItems,
        "creo solve line index nodes",
        |ctx| crate::curve::tests::compile_solve_program(ctx, &lines),
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "creo solve line index nodes")
    );
}

#[test]
fn pending_solve_statements_refuse_before_growth() {
    let lines = expression_lines(&["SOLVE", "x=1", "FOR x"]);
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::CollectionItems,
        "creo pending solve statements",
        |ctx| crate::curve::tests::compile_solve_program(ctx, &lines),
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "creo pending solve statements")
    );
}

#[test]
fn solve_equations_refuse_before_growth() {
    let lines = expression_lines(&["SOLVE", "x=1", "FOR x"]);
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::CollectionItems,
        "creo solve equations",
        |ctx| crate::curve::tests::compile_solve_program(ctx, &lines),
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "creo solve equations")
    );
}

#[test]
fn solve_blocks_refuse_before_growth() {
    let lines = expression_lines(&["SOLVE", "x=1", "FOR x"]);
    assert_eq!(
        with_expression_policy(DecodePolicy::service(), |ctx| {
            crate::curve::tests::compile_solve_program(ctx, &lines)
        })
        .expect("service profile")
        .blocks
        .len(),
        1
    );
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::CollectionItems,
        "creo solve blocks",
        |ctx| crate::curve::tests::compile_solve_program(ctx, &lines),
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "creo solve blocks")
    );
}

#[test]
fn solve_assignments_refuse_before_growth() {
    let lines = expression_lines(&["SOLVE", "x=1", "y=2", "FOR x"]);
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::CollectionItems,
        "creo solve assignments",
        |ctx| crate::curve::tests::compile_solve_program(ctx, &lines),
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "creo solve assignments")
    );
}

#[test]
fn executable_solve_line_nodes_refuse_before_insert() {
    let lines = expression_lines(&["SOLVE", "x=1", "y=2", "FOR x"]);
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::CollectionItems,
        "creo executable solve line index nodes",
        |ctx| crate::curve::tests::compile_solve_program(ctx, &lines),
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "creo executable solve line index nodes")
    );
    let program = with_expression_policy(DecodePolicy::service(), |ctx| {
        crate::curve::tests::compile_solve_program(ctx, &lines)
    })
    .expect("service solve program");
    assert_eq!(
        program
            .executable_line_indices
            .into_iter()
            .collect::<Vec<_>>(),
        [2]
    );
}

#[test]
fn solve_equation_left_refuses_retained_limit() {
    let lines = expression_lines(&["SOLVE", "x=1", "FOR x"]);
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::RetainedBytes,
        "creo solve equation left",
        |ctx| crate::curve::tests::compile_solve_program(ctx, &lines),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo solve equation left"));
}

#[test]
fn solve_equation_right_refuses_retained_limit() {
    let lines = expression_lines(&["SOLVE", "x=1", "FOR x"]);
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::RetainedBytes,
        "creo solve equation right",
        |ctx| crate::curve::tests::compile_solve_program(ctx, &lines),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo solve equation right"));
}

#[test]
fn conditional_expression_assignments_refuse_before_growth() {
    let lines = expression_lines(&["else", "a=1"]);
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::CollectionItems,
        "creo conditional expression assignments",
        |ctx| {
            crate::curve::tests::evaluate_program_details(
                ctx,
                &lines,
                None,
                &super::super::ExternalRelationSymbols::default(),
            )
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "creo conditional expression assignments")
    );
}

#[test]
fn parsed_expression_assignment_slots_refuse_before_allocation() {
    let lines = expression_lines(&["a=1"]);
    let evaluation = with_expression_policy(DecodePolicy::service(), |ctx| {
        crate::curve::tests::evaluate_program_details(
            ctx,
            &lines,
            None,
            &super::super::ExternalRelationSymbols::default(),
        )
    })
    .expect("service profile");
    assert_eq!(evaluation.assignments.len(), 1);

    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::CollectionItems,
        "creo parsed expression assignment slots",
        |ctx| {
            crate::curve::tests::evaluate_program_details(
                ctx,
                &lines,
                None,
                &super::super::ExternalRelationSymbols::default(),
            )
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "creo parsed expression assignment slots")
    );
}

fn evaluation_limit_reaches(
    source: &[&str],
    external_symbols: &super::super::ExternalRelationSymbols,
    dimension: ResourceDimension,
    operation: &'static str,
) {
    let lines = expression_lines(source);
    with_expression_policy(DecodePolicy::service(), |ctx| {
        crate::curve::tests::evaluate_program_details(ctx, &lines, None, external_symbols)
    })
    .expect("service profile evaluates expression");

    let error = crate::test_support::last_refusal_at(&[], dimension, operation, |ctx| {
        crate::curve::tests::evaluate_program_details(ctx, &lines, None, external_symbols)
    });
    assert!(matches!(error, CodecError::ResourceLimit(refusal)
        if refusal.dimension == dimension && refusal.operation == operation));
}

fn external_symbol(
    value: Option<super::super::CurveExpressionValue>,
) -> super::super::ExternalRelationSymbols {
    named_external_symbol("external", value)
}

fn named_external_symbol(
    name: &str,
    value: Option<super::super::CurveExpressionValue>,
) -> super::super::ExternalRelationSymbols {
    super::super::ExternalRelationSymbols {
        values: BTreeMap::from([(name.to_owned(), value)]),
    }
}

macro_rules! evaluation_collection_test {
    ($name:ident, $source:expr, $external:expr, $operation:literal) => {
        #[test]
        fn $name() {
            evaluation_limit_reaches(
                $source,
                &$external,
                ResourceDimension::CollectionItems,
                $operation,
            );
        }
    };
}

macro_rules! evaluation_materialized_test {
    ($name:ident, $source:expr, $external:expr, $operation:literal) => {
        #[test]
        fn $name() {
            evaluation_limit_reaches(
                $source,
                &$external,
                ResourceDimension::MaterializedBytes,
                $operation,
            );
        }
    };
    ($name:ident, $source:expr, $operation:literal) => {
        #[test]
        fn $name() {
            evaluation_limit_reaches(
                $source,
                &super::super::ExternalRelationSymbols::default(),
                ResourceDimension::MaterializedBytes,
                $operation,
            );
        }
    };
}

fn solve_phase_inputs(
    source: &[&str],
    external_symbols: &super::super::ExternalRelationSymbols,
) -> (
    super::super::CurveExpressionSolveBlock,
    BTreeMap<String, super::super::CurveExpressionValue>,
    super::super::CurveExpressionEvaluation,
) {
    let lines = expression_lines(source);
    let evaluation = with_expression_policy(DecodePolicy::service(), |ctx| {
        crate::curve::tests::evaluate_program_details(ctx, &lines, None, external_symbols)
    })
    .expect("service profile evaluates the original expression");
    let block = with_expression_policy(DecodePolicy::service(), |ctx| {
        crate::curve::tests::compile_solve_program(ctx, &lines)
    })
    .expect("solve program")
    .blocks
    .pop()
    .expect("one solve block");
    let preceding: Vec<_> = lines
        .iter()
        .take_while(|line| line.offset < block.offset)
        .cloned()
        .collect();
    let preceding = with_expression_policy(DecodePolicy::service(), |ctx| {
        crate::curve::tests::evaluate_program_details(ctx, &preceding, None, external_symbols)
    })
    .expect("preceding assignments");
    let mut values = BTreeMap::new();
    for (name, value) in &external_symbols.values {
        if let Some(value) = value {
            values.insert(name.clone(), value.clone());
        }
    }
    for assignment in preceding.assignments {
        if let (Some((name, _)), Some(value)) =
            (assignment.scalar_target(), assignment.value.as_ref())
        {
            values.insert(name.to_ascii_lowercase(), value.clone());
        }
    }
    (block, values, evaluation)
}

fn affine_materialized_limit_reaches(
    source: &[&str],
    external_symbols: &super::super::ExternalRelationSymbols,
    operation: &'static str,
) {
    let (block, values, _) = solve_phase_inputs(source, external_symbols);
    let known: Vec<_> = block
        .unknowns
        .iter()
        .map(|unknown| {
            values
                .get(&unknown.name.to_ascii_lowercase())
                .and_then(super::super::quantity_parts_ref)
                .map(|(_, dimension)| dimension)
        })
        .collect();
    let dimensions = with_expression_policy(DecodePolicy::service(), |ctx| {
        crate::curve::solve::infer_solve_variable_dimensions(
            ctx,
            &block,
            &values,
            &known,
            super::super::RelationEvaluationContext::default(),
        )
    })
    .expect("dimension inference")
    .expect("valid dimensions");
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::MaterializedBytes,
        operation,
        |ctx| {
            crate::curve::solve::solve_affine_expression_block(
                ctx,
                &block,
                &values,
                &dimensions,
                super::super::RelationEvaluationContext::default(),
            )
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(refusal) if refusal.dimension == ResourceDimension::MaterializedBytes && refusal.operation == operation)
    );
}

macro_rules! affine_materialized_test {
    ($name:ident, $source:expr, $external:expr, $operation:literal) => {
        #[test]
        fn $name() {
            affine_materialized_limit_reaches($source, &$external, $operation);
        }
    };
}

macro_rules! installed_solution_materialized_test {
    ($name:ident, $source:expr, $external:expr, $operation:literal) => {
        #[test]
        fn $name() {
            let (block, values, evaluation) = solve_phase_inputs($source, &$external);
            let solution = evaluation.solve_solutions.get(&block.offset).expect("solved original block");
            let mut indices = std::collections::HashMap::<String, Vec<usize>>::new();
            for (index, assignment) in evaluation.assignments.iter().enumerate() { if let Some((name, _)) = assignment.scalar_target() { indices.entry(name.to_ascii_lowercase()).or_default().push(index); } }
            let error = crate::test_support::last_refusal_at(&[], ResourceDimension::MaterializedBytes, $operation, |ctx| {
                let mut scratch = ctx.reserve_scoped(0, "creo relation evaluation scratch")?;
                super::super::install_solve_solution(ctx, &block, solution, &mut evaluation.assignments.clone(), &indices, &mut values.clone(), &mut scratch)
            });
            assert!(matches!(error, CodecError::ResourceLimit(refusal) if refusal.dimension == ResourceDimension::MaterializedBytes && refusal.operation == $operation));
        }
    };
}

evaluation_collection_test!(
    existing_external_symbol_nodes_refuse,
    &[],
    external_symbol(None),
    "creo existing external symbol nodes"
);
evaluation_materialized_test!(
    existing_external_symbol_names_refuse,
    &[],
    external_symbol(None),
    "creo existing external symbol names"
);
evaluation_collection_test!(
    existing_assignment_symbol_nodes_refuse,
    &["a=1"],
    super::super::ExternalRelationSymbols::default(),
    "creo existing assignment symbol nodes"
);
evaluation_materialized_test!(
    existing_assignment_symbol_names_refuse,
    &["a=1"],
    super::super::ExternalRelationSymbols::default(),
    "creo existing assignment symbol names"
);
evaluation_collection_test!(
    defined_external_symbol_nodes_refuse,
    &[],
    external_symbol(None),
    "creo defined external symbol nodes"
);
evaluation_materialized_test!(
    defined_external_symbol_names_refuse,
    &[],
    external_symbol(None),
    "creo defined external symbol names"
);
evaluation_collection_test!(
    external_value_nodes_refuse,
    &[],
    external_symbol(Some(super::super::CurveExpressionValue::String(
        "text".into()
    ))),
    "creo external value nodes"
);
evaluation_materialized_test!(
    external_value_names_refuse,
    &[],
    external_symbol(Some(super::super::CurveExpressionValue::Number(
        cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite relation fixture")
    ))),
    "creo external value names"
);
evaluation_materialized_test!(
    external_string_values_refuse,
    &[],
    external_symbol(Some(super::super::CurveExpressionValue::String(
        "text".into()
    ))),
    "creo external string values"
);
evaluation_collection_test!(
    defined_assignment_symbol_nodes_refuse,
    &["a=1"],
    super::super::ExternalRelationSymbols::default(),
    "creo defined assignment symbol nodes"
);
evaluation_materialized_test!(
    defined_assignment_symbol_names_refuse,
    &["a=1"],
    super::super::ExternalRelationSymbols::default(),
    "creo defined assignment symbol names"
);
evaluation_collection_test!(
    evaluated_value_nodes_refuse,
    &["a=1"],
    super::super::ExternalRelationSymbols::default(),
    "creo evaluated value nodes"
);
evaluation_materialized_test!(
    evaluated_symbol_names_refuse,
    &["a=1"],
    super::super::ExternalRelationSymbols::default(),
    "creo evaluated symbol names"
);
evaluation_collection_test!(
    evaluated_assignments_refuse,
    &["a=1"],
    super::super::ExternalRelationSymbols::default(),
    "creo evaluated assignments"
);
evaluation_collection_test!(
    solve_dimension_snapshots_refuse,
    &["SOLVE", "x=1", "FOR x"],
    super::super::ExternalRelationSymbols::default(),
    "creo solve dimension snapshots"
);
evaluation_collection_test!(
    solve_initial_value_snapshots_refuse,
    &["SOLVE", "x=1", "FOR x"],
    super::super::ExternalRelationSymbols::default(),
    "creo solve initial value snapshots"
);
evaluation_collection_test!(
    solve_dimension_snapshot_nodes_refuse,
    &["SOLVE", "x=1", "FOR x"],
    super::super::ExternalRelationSymbols::default(),
    "creo solve dimension snapshot nodes"
);
evaluation_collection_test!(
    solve_initial_snapshot_nodes_refuse,
    &["SOLVE", "x=1", "FOR x"],
    super::super::ExternalRelationSymbols::default(),
    "creo solve initial snapshot nodes"
);
evaluation_collection_test!(
    solve_solution_nodes_refuse,
    &["SOLVE", "x=1", "FOR x"],
    super::super::ExternalRelationSymbols::default(),
    "creo solve solution nodes"
);
evaluation_collection_test!(
    existing_solve_symbol_nodes_refuse,
    &["SOLVE", "x=1", "FOR x"],
    super::super::ExternalRelationSymbols::default(),
    "creo existing solve symbol nodes"
);
#[test]
fn existing_solve_symbol_names_refuse() {
    let lines = expression_lines(&["SOLVE", "x=1", "FOR x"]);
    let external = super::super::ExternalRelationSymbols::default();
    with_expression_policy(DecodePolicy::service(), |ctx| {
        crate::curve::tests::evaluate_program_details(ctx, &lines, None, &external)
    })
    .expect("service evaluates the original expression");
    let program = with_expression_policy(DecodePolicy::service(), |ctx| {
        crate::curve::tests::compile_solve_program(ctx, &lines)
    })
    .expect("accepted solve program");
    let parsed = vec![None; lines.len()];
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::MaterializedBytes,
        "creo existing solve symbol names",
        |ctx| {
            let mut storage = ctx.reserve_scoped(0, "creo relation evaluation scratch")?;
            storage.with_storage(|| {
                super::super::expression_program_symbols(ctx, &external, &parsed, &program)
            })
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "creo existing solve symbol names")
    );
}

evaluation_collection_test!(
    defined_solve_symbol_nodes_refuse,
    &["SOLVE", "x=1", "FOR x"],
    super::super::ExternalRelationSymbols::default(),
    "creo defined solve symbol nodes"
);
evaluation_materialized_test!(
    defined_solve_symbol_names_refuse,
    &["SOLVE", "x=1", "FOR x"],
    super::super::ExternalRelationSymbols::default(),
    "creo defined solve symbol names"
);
evaluation_collection_test!(
    solved_value_nodes_refuse,
    &["SOLVE", "x=1", "FOR x"],
    super::super::ExternalRelationSymbols::default(),
    "creo solved value nodes"
);
installed_solution_materialized_test!(
    solved_value_names_refuse,
    &["SOLVE", "x=1", "FOR x"],
    super::super::ExternalRelationSymbols::default(),
    "creo solved value names"
);
evaluation_collection_test!(
    evaluated_function_assignments_refuse,
    &["foo(x)=1"],
    super::super::ExternalRelationSymbols::default(),
    "creo evaluated assignments"
);
evaluation_materialized_test!(
    evaluated_string_values_refuse,
    &["a=\"text\""],
    super::super::ExternalRelationSymbols::default(),
    "creo evaluated string values"
);
evaluation_materialized_test!(
    solve_initial_string_values_refuse,
    &["SOLVE", "x=1", "FOR x"],
    named_external_symbol(
        "x",
        Some(super::super::CurveExpressionValue::String("text".into()))
    ),
    "creo solve initial string values"
);

evaluation_materialized_test!(
    solve_snapshot_lookup_refuses_temporary_bytes,
    &["SOLVE", "x=1", "FOR x"],
    "creo solve snapshot lookup"
);

fn affine_helix_limit_reaches(dimension: ResourceDimension, operation: &'static str) {
    let record = super::super::expression_records(AFFINE_HELIX)
        .pop()
        .expect("complete helix expression");
    assert!(with_expression_policy(DecodePolicy::service(), |ctx| {
        super::super::expression_helix(ctx, &record)
    })
    .expect("service profile")
    .is_some());
    let error = crate::test_support::last_refusal_at(&[], dimension, operation, |ctx| {
        super::super::expression_helix(ctx, &record)
    });
    assert!(matches!(error, CodecError::ResourceLimit(refusal)
        if refusal.dimension == dimension && refusal.operation == operation));
}

macro_rules! affine_helix_collection_test {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            affine_helix_limit_reaches(ResourceDimension::CollectionItems, $operation);
        }
    };
}

macro_rules! affine_helix_materialized_test {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            affine_helix_limit_reaches(ResourceDimension::MaterializedBytes, $operation);
        }
    };
}

affine_helix_collection_test!(
    affine_time_value_node_refuses,
    "creo affine time value node"
);
affine_helix_materialized_test!(
    affine_time_value_name_refuses,
    "creo affine time value name"
);
affine_helix_collection_test!(
    affine_defined_time_node_refuses,
    "creo affine defined time node"
);
affine_helix_materialized_test!(
    affine_defined_time_name_refuses,
    "creo affine defined time name"
);
affine_helix_materialized_test!(
    affine_assignment_names_refuse,
    "creo affine assignment names"
);
affine_helix_collection_test!(
    affine_defined_symbol_nodes_refuse,
    "creo affine defined symbol nodes"
);
affine_helix_materialized_test!(
    affine_defined_symbol_names_refuse,
    "creo affine defined symbol names"
);
affine_helix_collection_test!(affine_value_nodes_refuse, "creo affine value nodes");

fn solve_storage_limit_reaches(source: &[&str], operation: &'static str) {
    let lines = expression_lines(source);
    with_expression_policy(DecodePolicy::service(), |ctx| {
        crate::curve::tests::evaluate_program_details(
            ctx,
            &lines,
            None,
            &super::super::ExternalRelationSymbols::default(),
        )
    })
    .expect("service profile solves expression");
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::CollectionItems,
        operation,
        |ctx| {
            crate::curve::tests::evaluate_program_details(
                ctx,
                &lines,
                None,
                &super::super::ExternalRelationSymbols::default(),
            )
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(refusal) if refusal.dimension == ResourceDimension::CollectionItems && refusal.operation == operation)
    );
}

macro_rules! solve_storage_test {
    ($name:ident, $source:expr, $operation:literal) => {
        #[test]
        fn $name() {
            solve_storage_limit_reaches($source, $operation);
        }
    };
}

solve_storage_test!(
    affine_variable_keys_refuse,
    &["SOLVE", "x=1", "FOR x"],
    "creo affine variable keys"
);
solve_storage_test!(
    affine_known_value_nodes_refuse,
    &["y=2", "SOLVE", "x+y=3", "FOR x"],
    "creo affine known value nodes"
);
solve_storage_test!(
    affine_coefficient_nodes_refuse,
    &["SOLVE", "x=1", "FOR x"],
    "creo affine coefficient nodes"
);
solve_storage_test!(
    affine_unknown_value_nodes_refuse,
    &["SOLVE", "x=1", "FOR x"],
    "creo affine unknown value nodes"
);
solve_storage_test!(
    affine_equation_coefficients_refuse,
    &["SOLVE", "x=1", "FOR x"],
    "creo affine equation coefficients"
);
solve_storage_test!(
    affine_equation_rows_refuse,
    &["SOLVE", "x=1", "FOR x"],
    "creo affine equation rows"
);
solve_storage_test!(
    affine_unique_solution_refuses,
    &["SOLVE", "x=1", "FOR x"],
    "creo affine unique solution"
);
solve_storage_test!(
    affine_solved_values_refuse,
    &["SOLVE", "x=1", "FOR x"],
    "creo affine solved values"
);
solve_storage_test!(
    nonlinear_initial_point_refuses,
    &["x=2", "SOLVE", "x*x*x=8", "FOR x"],
    "creo nonlinear initial point"
);
solve_storage_test!(
    nonlinear_line_search_point_refuses,
    &["x=1", "SOLVE", "x*x*x=8", "FOR x"],
    "creo nonlinear line-search point"
);
solve_storage_test!(
    nonlinear_solved_values_refuse,
    &["x=2", "SOLVE", "x*x*x=8", "FOR x"],
    "creo nonlinear solved values"
);
affine_materialized_test!(
    affine_variable_names_refuse,
    &["SOLVE", "x=1", "FOR x"],
    super::super::ExternalRelationSymbols::default(),
    "creo affine variable names"
);
affine_materialized_test!(
    affine_known_value_names_refuse,
    &["y=2", "SOLVE", "x+y=3", "FOR x"],
    super::super::ExternalRelationSymbols::default(),
    "creo affine known value names"
);
affine_materialized_test!(
    affine_coefficient_names_refuse,
    &["SOLVE", "x=1", "FOR x"],
    super::super::ExternalRelationSymbols::default(),
    "creo affine coefficient names"
);
affine_materialized_test!(
    affine_unknown_value_names_refuse,
    &["SOLVE", "x=1", "FOR x"],
    super::super::ExternalRelationSymbols::default(),
    "creo affine unknown value names"
);
solve_storage_test!(
    nonlinear_known_value_nodes_refuse,
    &["y=3", "x=2", "SOLVE", "x*x*x=8", "FOR x"],
    "creo nonlinear known value nodes"
);
solve_storage_test!(
    nonlinear_unknown_value_nodes_refuse,
    &["x=2", "SOLVE", "x*x*x=8", "FOR x"],
    "creo nonlinear unknown value nodes"
);
solve_storage_test!(
    nonlinear_residual_rows_refuse,
    &["x=2", "SOLVE", "x*x*x=8", "FOR x"],
    "creo nonlinear residual rows"
);
solve_storage_test!(
    nonlinear_jacobian_coefficients_refuse,
    &["x=2", "SOLVE", "x*x*x=8", "FOR x"],
    "creo nonlinear Jacobian coefficients"
);
solve_storage_test!(
    nonlinear_positive_probe_refuses,
    &["x=2", "SOLVE", "x*x*x=8", "FOR x"],
    "creo nonlinear positive probe"
);
solve_storage_test!(
    nonlinear_negative_probe_refuses,
    &["x=2", "SOLVE", "x*x*x=8", "FOR x"],
    "creo nonlinear negative probe"
);
solve_storage_test!(
    nonlinear_jacobian_rows_refuse,
    &["x=2", "SOLVE", "x*x*x=8", "FOR x"],
    "creo nonlinear Jacobian rows"
);
fn nonlinear_materialized_limit_reaches(
    source: &[&str],
    external_symbols: &super::super::ExternalRelationSymbols,
    operation: &'static str,
) {
    let lines = expression_lines(source);
    with_expression_policy(DecodePolicy::service(), |ctx| {
        crate::curve::tests::evaluate_program_details(ctx, &lines, None, external_symbols)
    })
    .expect("service profile solves the original expression");
    let block = with_expression_policy(DecodePolicy::service(), |ctx| {
        crate::curve::tests::compile_solve_program(ctx, &lines)
    })
    .expect("solve program")
    .blocks
    .pop()
    .expect("one solve block");
    let preceding: Vec<_> = lines
        .iter()
        .take_while(|line| line.offset < block.offset)
        .cloned()
        .collect();
    let preceding = with_expression_policy(DecodePolicy::service(), |ctx| {
        crate::curve::tests::evaluate_program_details(ctx, &preceding, None, external_symbols)
    })
    .expect("preceding assignments");
    let mut values = std::collections::BTreeMap::new();
    for assignment in preceding.assignments {
        if let (Some((name, _)), Some(value)) =
            (assignment.scalar_target(), assignment.value.as_ref())
        {
            values.insert(name.to_ascii_lowercase(), value.clone());
        }
    }
    let point: Vec<_> = block
        .unknowns
        .iter()
        .map(|unknown| {
            values
                .get(&unknown.name.to_ascii_lowercase())
                .and_then(super::super::quantity_parts_ref)
                .expect("initial numeric value")
                .0
        })
        .collect();
    let dimensions = vec![super::super::RelationDimension::default(); point.len()];
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::MaterializedBytes,
        operation,
        |ctx| {
            super::super::solve::evaluate_nonlinear_residuals(
                ctx,
                &block,
                &values,
                &dimensions,
                &point,
                super::super::RelationEvaluationContext::default(),
            )
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(refusal) if refusal.dimension == ResourceDimension::MaterializedBytes && refusal.operation == operation)
    );
}

macro_rules! nonlinear_materialized_test {
    ($name:ident, $source:expr, $external:expr, $operation:literal) => {
        #[test]
        fn $name() {
            nonlinear_materialized_limit_reaches($source, &$external, $operation);
        }
    };
}

nonlinear_materialized_test!(
    nonlinear_known_value_names_refuse,
    &["y=3", "x=2", "SOLVE", "x*x*x=8", "FOR x"],
    super::super::ExternalRelationSymbols::default(),
    "creo nonlinear known value names"
);
nonlinear_materialized_test!(
    nonlinear_unknown_value_names_refuse,
    &["x=2", "SOLVE", "x*x*x=8", "FOR x"],
    super::super::ExternalRelationSymbols::default(),
    "creo nonlinear unknown value names"
);
nonlinear_materialized_test!(
    nonlinear_known_string_values_refuse,
    &["y=\"text\"", "x=2", "SOLVE", "x*x*x=8", "FOR x"],
    super::super::ExternalRelationSymbols::default(),
    "creo nonlinear known string values"
);

#[test]
fn solve_unknowns_refuse_before_vector_growth() {
    let service = with_expression_policy(DecodePolicy::service(), |ctx| {
        super::super::curve_expression_solve_unknowns(ctx, "x, Y")
    })
    .expect("service profile")
    .expect("valid unknowns");
    assert_eq!(
        service
            .iter()
            .map(|unknown| unknown.name.as_str())
            .collect::<Vec<_>>(),
        ["x", "Y"]
    );
    assert!(with_expression_policy(DecodePolicy::service(), |ctx| {
        super::super::curve_expression_solve_unknowns(ctx, "x, X")
    })
    .expect("service profile")
    .is_none());

    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::CollectionItems,
        "creo solve unknowns",
        |ctx| super::super::curve_expression_solve_unknowns(ctx, "x"),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo solve unknowns"));
}

#[test]
fn solve_unknown_name_refuses_before_retained_copy() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        Some("creo solve unknown names"),
        |cap| {
            let mut trial_policy = policy;
            trial_policy.limits.max_retained_bytes = cap;
            with_expression_policy(trial_policy, |ctx| {
                super::super::curve_expression_solve_unknowns(ctx, "x")
            })
        },
    );
    let error = with_expression_policy(policy, |ctx| {
        super::super::curve_expression_solve_unknowns(ctx, "x")
    })
    .expect_err("unknown name exceeds retained limit");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo solve unknown names"));
}

#[test]
fn solve_unknown_duplicate_check_obeys_work_limit() {
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "creo solve unknown duplicate checks",
        |ctx| super::super::curve_expression_solve_unknowns(ctx, "x,y"),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "creo solve unknown duplicate checks"));
}

mod evaluation;
mod records;
mod trimming;
