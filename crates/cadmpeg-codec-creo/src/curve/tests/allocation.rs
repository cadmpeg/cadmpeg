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
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("test input is admitted");
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

fn target_limit_error(source: &str, collection_limit: u64, retained_limit: u64) -> CodecError {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    resource_error(with_expression_policy(policy, |ctx| {
        super::super::expression_assignment_target(ctx, source)
    }))
}

fn assert_target_limit(error: CodecError, dimension: ResourceDimension, operation: &'static str) {
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == dimension && limit.operation == operation));
}

#[test]
fn expression_parsed_arguments_refuse_before_growth() {
    assert_target_limit(
        target_limit_error("foo(x,y)", 0, u64::MAX),
        ResourceDimension::CollectionItems,
        "creo expression parsed arguments",
    );
}

#[test]
fn expression_target_arguments_refuse_before_growth() {
    assert_target_limit(
        target_limit_error("foo(x,y)", 2, u64::MAX),
        ResourceDimension::CollectionItems,
        "creo expression target arguments",
    );
}

#[test]
fn expression_target_argument_text_refuses_before_copy() {
    assert_target_limit(
        target_limit_error("foo(x)", u64::MAX, 0),
        ResourceDimension::RetainedBytes,
        "creo expression target argument text",
    );
}

#[test]
fn expression_function_target_refuses_before_copy() {
    assert_target_limit(
        target_limit_error("foo()", u64::MAX, 0),
        ResourceDimension::RetainedBytes,
        "creo expression function target",
    );
}

#[test]
fn expression_table_column_refuses_before_copy() {
    assert_target_limit(
        target_limit_error("value(p,r,c)", u64::MAX, 0),
        ResourceDimension::RetainedBytes,
        "creo expression table column",
    );
}

#[test]
fn expression_table_parameter_refuses_before_copy() {
    assert_target_limit(
        target_limit_error("value(p,r,c)", u64::MAX, 1),
        ResourceDimension::RetainedBytes,
        "creo expression table parameter",
    );
}

#[test]
fn expression_table_row_refuses_before_copy() {
    assert_target_limit(
        target_limit_error("value(p,r,c)", u64::MAX, 2),
        ResourceDimension::RetainedBytes,
        "creo expression table row",
    );
}

#[test]
fn expression_scoped_target_refuses_before_copy() {
    assert_target_limit(
        target_limit_error("a:b", u64::MAX, 0),
        ResourceDimension::RetainedBytes,
        "creo expression scoped target",
    );
}

#[test]
fn expression_system_target_refuses_before_copy() {
    assert_target_limit(
        target_limit_error("D1", u64::MAX, 0),
        ResourceDimension::RetainedBytes,
        "creo expression system target",
    );
}

#[test]
fn expression_declared_unit_refuses_before_copy() {
    assert_target_limit(
        target_limit_error("a[mm]", u64::MAX, 0),
        ResourceDimension::RetainedBytes,
        "creo expression declared unit",
    );
}

#[test]
fn expression_parameter_target_refuses_before_copy() {
    assert_target_limit(
        target_limit_error("a", u64::MAX, 0),
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
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let error = with_expression_policy(policy, |ctx| {
        super::super::expression_assignment(ctx, &line[0])
    })
    .expect_err("dependency vector exceeds collection limit");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo expression dependency names"));
}

#[test]
fn expression_dependency_text_refuses_before_copy() {
    let line = expression_lines(&["a=b"]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
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
    policy.limits.max_retained_bytes = 0;
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
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let error = resource_error(with_expression_policy(policy, |ctx| {
        super::super::curve_expression_solve_program(ctx, &lines)
    }));
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo solve line index nodes"));
}

#[test]
fn pending_solve_statements_refuse_before_growth() {
    let lines = expression_lines(&["SOLVE", "x=1", "FOR x"]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 3;
    let error = resource_error(with_expression_policy(policy, |ctx| {
        super::super::curve_expression_solve_program(ctx, &lines)
    }));
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo pending solve statements"));
}

#[test]
fn solve_equations_refuse_before_growth() {
    let lines = expression_lines(&["SOLVE", "x=1", "FOR x"]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 5;
    let error = resource_error(with_expression_policy(policy, |ctx| {
        super::super::curve_expression_solve_program(ctx, &lines)
    }));
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo solve equations"));
}

#[test]
fn solve_blocks_refuse_before_growth() {
    let lines = expression_lines(&["SOLVE", "x=1", "FOR x"]);
    assert_eq!(with_expression_policy(DecodePolicy::service(), |ctx| {
        super::super::curve_expression_solve_program(ctx, &lines)
    })
    .expect("service profile")
    .blocks
    .len(), 1);
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 6;
    let error = resource_error(with_expression_policy(policy, |ctx| {
        super::super::curve_expression_solve_program(ctx, &lines)
    }));
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo solve blocks"));
}

#[test]
fn solve_assignment_indices_refuse_before_growth() {
    let lines = expression_lines(&["SOLVE", "x=1", "y=2", "FOR x"]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 9;
    let error = resource_error(with_expression_policy(policy, |ctx| {
        super::super::curve_expression_solve_program(ctx, &lines)
    }));
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo solve assignment indices"));
}

#[test]
fn solve_assignments_refuse_before_growth() {
    let lines = expression_lines(&["SOLVE", "x=1", "y=2", "FOR x"]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 10;
    let error = resource_error(with_expression_policy(policy, |ctx| {
        super::super::curve_expression_solve_program(ctx, &lines)
    }));
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo solve assignments"));
}

#[test]
fn executable_solve_line_nodes_refuse_before_insert() {
    let lines = expression_lines(&["SOLVE", "x=1", "y=2", "FOR x"]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 12;
    let error = resource_error(with_expression_policy(policy, |ctx| {
        super::super::curve_expression_solve_program(ctx, &lines)
    }));
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo executable solve line index nodes"));
    let program = with_expression_policy(DecodePolicy::service(), |ctx| {
        super::super::curve_expression_solve_program(ctx, &lines)
    }).expect("service solve program");
    assert_eq!(program.executable_line_indices.into_iter().collect::<Vec<_>>(), [2]);
}

#[test]
fn solve_equation_left_refuses_retained_limit() {
    let lines = expression_lines(&["SOLVE", "x=1", "FOR x"]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 1;
    let error = resource_error(with_expression_policy(policy, |ctx| {
        super::super::curve_expression_solve_program(ctx, &lines)
    }));
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo solve equation left"));
}

#[test]
fn solve_equation_right_refuses_retained_limit() {
    let lines = expression_lines(&["SOLVE", "x=1", "FOR x"]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 2;
    let error = resource_error(with_expression_policy(policy, |ctx| {
        super::super::curve_expression_solve_program(ctx, &lines)
    }));
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo solve equation right"));
}

#[test]
fn conditional_expression_assignments_refuse_before_growth() {
    let lines = expression_lines(&["else", "a=1"]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let error = resource_error(with_expression_policy(policy, |ctx| {
        super::super::evaluate_expression_program_details(
            ctx,
            &lines,
            None,
            &super::super::ExternalRelationSymbols::default(),
        )
    }));
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo conditional expression assignments"));
}

#[test]
fn parsed_expression_assignment_slots_refuse_before_allocation() {
    let lines = expression_lines(&["a=1"]);
    let evaluation = with_expression_policy(DecodePolicy::service(), |ctx| {
        super::super::evaluate_expression_program_details(
            ctx,
            &lines,
            None,
            &super::super::ExternalRelationSymbols::default(),
        )
    })
    .expect("service profile");
    assert_eq!(evaluation.assignments.len(), 1);

    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let error = resource_error(with_expression_policy(policy, |ctx| {
        super::super::evaluate_expression_program_details(
            ctx,
            &lines,
            None,
            &super::super::ExternalRelationSymbols::default(),
        )
    }));
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo parsed expression assignment slots"));
}

fn evaluation_limit_reaches(
    source: &[&str],
    external_symbols: &super::super::ExternalRelationSymbols,
    dimension: ResourceDimension,
    operation: &'static str,
) {
    let lines = expression_lines(source);
    with_expression_policy(DecodePolicy::service(), |ctx| {
        super::super::evaluate_expression_program_details(ctx, &lines, None, external_symbols)
    })
    .expect("service profile evaluates expression");

    for limit in 0..256 {
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = limit,
            _ => panic!("unsupported test limit"),
        }
        let result = with_expression_policy(policy, |ctx| {
            super::super::evaluate_expression_program_details(ctx, &lines, None, external_symbols)
        });
        if matches!(result, Err(CodecError::ResourceLimit(ref refusal))
            if refusal.dimension == dimension && refusal.operation == operation)
        {
            return;
        }
    }
    panic!("no limit reaches {operation}");
}

fn external_symbol(value: Option<super::super::CurveExpressionValue>) -> super::super::ExternalRelationSymbols {
    named_external_symbol("external", value)
}

fn named_external_symbol(name: &str, value: Option<super::super::CurveExpressionValue>) -> super::super::ExternalRelationSymbols {
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

macro_rules! evaluation_retained_test {
    ($name:ident, $source:expr, $external:expr, $operation:literal) => {
        #[test]
        fn $name() {
            evaluation_limit_reaches(
                $source,
                &$external,
                ResourceDimension::RetainedBytes,
                $operation,
            );
        }
    };
}

evaluation_collection_test!(existing_external_symbol_nodes_refuse, &[], external_symbol(None), "creo existing external symbol nodes");
evaluation_retained_test!(existing_external_symbol_names_refuse, &[], external_symbol(None), "creo existing external symbol names");
evaluation_collection_test!(existing_assignment_symbol_nodes_refuse, &["a=1"], super::super::ExternalRelationSymbols::default(), "creo existing assignment symbol nodes");
evaluation_retained_test!(existing_assignment_symbol_names_refuse, &["a=1"], super::super::ExternalRelationSymbols::default(), "creo existing assignment symbol names");
evaluation_collection_test!(defined_external_symbol_nodes_refuse, &[], external_symbol(None), "creo defined external symbol nodes");
evaluation_retained_test!(defined_external_symbol_names_refuse, &[], external_symbol(None), "creo defined external symbol names");
evaluation_collection_test!(external_value_nodes_refuse, &[], external_symbol(Some(super::super::CurveExpressionValue::String("text".into()))), "creo external value nodes");
evaluation_retained_test!(external_value_names_refuse, &[], external_symbol(Some(super::super::CurveExpressionValue::Number(1.0))), "creo external value names");
evaluation_retained_test!(external_string_values_refuse, &[], external_symbol(Some(super::super::CurveExpressionValue::String("text".into()))), "creo external string values");
evaluation_collection_test!(defined_assignment_symbol_nodes_refuse, &["a=1"], super::super::ExternalRelationSymbols::default(), "creo defined assignment symbol nodes");
evaluation_retained_test!(defined_assignment_symbol_names_refuse, &["a=1"], super::super::ExternalRelationSymbols::default(), "creo defined assignment symbol names");
evaluation_collection_test!(evaluated_value_nodes_refuse, &["a=1"], super::super::ExternalRelationSymbols::default(), "creo evaluated value nodes");
evaluation_retained_test!(evaluated_symbol_names_refuse, &["a=1"], super::super::ExternalRelationSymbols::default(), "creo evaluated symbol names");
evaluation_collection_test!(evaluated_assignments_refuse, &["a=1"], super::super::ExternalRelationSymbols::default(), "creo evaluated assignments");
evaluation_collection_test!(solve_dimension_snapshots_refuse, &["SOLVE", "x=1", "FOR x"], super::super::ExternalRelationSymbols::default(), "creo solve dimension snapshots");
evaluation_collection_test!(solve_initial_value_snapshots_refuse, &["SOLVE", "x=1", "FOR x"], super::super::ExternalRelationSymbols::default(), "creo solve initial value snapshots");
evaluation_collection_test!(solve_dimension_snapshot_nodes_refuse, &["SOLVE", "x=1", "FOR x"], super::super::ExternalRelationSymbols::default(), "creo solve dimension snapshot nodes");
evaluation_collection_test!(solve_initial_snapshot_nodes_refuse, &["SOLVE", "x=1", "FOR x"], super::super::ExternalRelationSymbols::default(), "creo solve initial snapshot nodes");
evaluation_collection_test!(solve_solution_nodes_refuse, &["SOLVE", "x=1", "FOR x"], super::super::ExternalRelationSymbols::default(), "creo solve solution nodes");
evaluation_collection_test!(existing_solve_symbol_nodes_refuse, &["SOLVE", "x=1", "FOR x"], super::super::ExternalRelationSymbols::default(), "creo existing solve symbol nodes");
evaluation_retained_test!(existing_solve_symbol_names_refuse, &["SOLVE", "x=1", "FOR x"], super::super::ExternalRelationSymbols::default(), "creo existing solve symbol names");
evaluation_collection_test!(defined_solve_symbol_nodes_refuse, &["SOLVE", "x=1", "FOR x"], super::super::ExternalRelationSymbols::default(), "creo defined solve symbol nodes");
evaluation_retained_test!(defined_solve_symbol_names_refuse, &["SOLVE", "x=1", "FOR x"], super::super::ExternalRelationSymbols::default(), "creo defined solve symbol names");
evaluation_collection_test!(solved_value_nodes_refuse, &["SOLVE", "x=1", "FOR x"], super::super::ExternalRelationSymbols::default(), "creo solved value nodes");
evaluation_retained_test!(solved_value_names_refuse, &["SOLVE", "x=1", "FOR x"], super::super::ExternalRelationSymbols::default(), "creo solved value names");
evaluation_collection_test!(evaluated_function_assignments_refuse, &["foo(x)=1"], super::super::ExternalRelationSymbols::default(), "creo evaluated assignments");
evaluation_retained_test!(evaluated_string_values_refuse, &["a=\"text\""], super::super::ExternalRelationSymbols::default(), "creo evaluated string values");
evaluation_retained_test!(solve_initial_string_values_refuse, &["SOLVE", "x=1", "FOR x"], named_external_symbol("x", Some(super::super::CurveExpressionValue::String("text".into()))), "creo solve initial string values");

macro_rules! evaluation_materialized_test {
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

evaluation_materialized_test!(solve_snapshot_lookup_refuses_temporary_bytes, &["SOLVE", "x=1", "FOR x"], "creo solve snapshot lookup");

fn affine_helix_limit_reaches(dimension: ResourceDimension, operation: &'static str) {
    let record = super::super::expression_records(AFFINE_HELIX)
        .pop()
        .expect("complete helix expression");
    assert!(with_expression_policy(DecodePolicy::service(), |ctx| {
        super::super::expression_helix(ctx, &record)
    })
    .expect("service profile")
    .is_some());
    for limit in 0..256 {
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
            _ => panic!("unsupported affine test limit"),
        }
        let result = with_expression_policy(policy, |ctx| {
            super::super::expression_helix(ctx, &record)
        });
        if matches!(result, Err(CodecError::ResourceLimit(ref refusal))
            if refusal.dimension == dimension && refusal.operation == operation)
        {
            return;
        }
    }
    panic!("no limit reaches {operation}");
}

macro_rules! affine_helix_collection_test {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            affine_helix_limit_reaches(ResourceDimension::CollectionItems, $operation);
        }
    };
}

macro_rules! affine_helix_retained_test {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            affine_helix_limit_reaches(ResourceDimension::RetainedBytes, $operation);
        }
    };
}

affine_helix_collection_test!(affine_time_value_node_refuses, "creo affine time value node");
affine_helix_retained_test!(affine_time_value_name_refuses, "creo affine time value name");
affine_helix_collection_test!(affine_defined_time_node_refuses, "creo affine defined time node");
affine_helix_retained_test!(affine_defined_time_name_refuses, "creo affine defined time name");
affine_helix_retained_test!(affine_assignment_names_refuse, "creo affine assignment names");
affine_helix_collection_test!(affine_defined_symbol_nodes_refuse, "creo affine defined symbol nodes");
affine_helix_retained_test!(affine_defined_symbol_names_refuse, "creo affine defined symbol names");
affine_helix_collection_test!(affine_value_nodes_refuse, "creo affine value nodes");

fn solve_storage_limit_reaches(source: &[&str], operation: &'static str) {
    let lines = expression_lines(source);
    with_expression_policy(DecodePolicy::service(), |ctx| {
        super::super::evaluate_expression_program_details(
            ctx,
            &lines,
            None,
            &super::super::ExternalRelationSymbols::default(),
        )
    })
    .expect("service profile solves expression");
    for limit in 0..8192 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let result = with_expression_policy(policy, |ctx| {
            super::super::evaluate_expression_program_details(
                ctx,
                &lines,
                None,
                &super::super::ExternalRelationSymbols::default(),
            )
        });
        if matches!(result, Err(CodecError::ResourceLimit(ref refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == operation)
        {
            return;
        }
    }
    panic!("no limit reaches {operation}");
}

macro_rules! solve_storage_test {
    ($name:ident, $source:expr, $operation:literal) => {
        #[test]
        fn $name() {
            solve_storage_limit_reaches($source, $operation);
        }
    };
}

solve_storage_test!(affine_variable_keys_refuse, &["SOLVE", "x=1", "FOR x"], "creo affine variable keys");
solve_storage_test!(affine_known_value_nodes_refuse, &["y=2", "SOLVE", "x+y=3", "FOR x"], "creo affine known value nodes");
solve_storage_test!(affine_coefficient_nodes_refuse, &["SOLVE", "x=1", "FOR x"], "creo affine coefficient nodes");
solve_storage_test!(affine_unknown_value_nodes_refuse, &["SOLVE", "x=1", "FOR x"], "creo affine unknown value nodes");
solve_storage_test!(affine_equation_coefficients_refuse, &["SOLVE", "x=1", "FOR x"], "creo affine equation coefficients");
solve_storage_test!(affine_equation_rows_refuse, &["SOLVE", "x=1", "FOR x"], "creo affine equation rows");
solve_storage_test!(affine_unique_solution_refuses, &["SOLVE", "x=1", "FOR x"], "creo affine unique solution");
solve_storage_test!(affine_solved_values_refuse, &["SOLVE", "x=1", "FOR x"], "creo affine solved values");
solve_storage_test!(nonlinear_initial_point_refuses, &["x=2", "SOLVE", "x*x*x=8", "FOR x"], "creo nonlinear initial point");
solve_storage_test!(nonlinear_line_search_point_refuses, &["x=1", "SOLVE", "x*x*x=8", "FOR x"], "creo nonlinear line-search point");
solve_storage_test!(nonlinear_solved_values_refuse, &["x=2", "SOLVE", "x*x*x=8", "FOR x"], "creo nonlinear solved values");
evaluation_retained_test!(affine_variable_names_refuse, &["SOLVE", "x=1", "FOR x"], super::super::ExternalRelationSymbols::default(), "creo affine variable names");
evaluation_retained_test!(affine_known_value_names_refuse, &["y=2", "SOLVE", "x+y=3", "FOR x"], super::super::ExternalRelationSymbols::default(), "creo affine known value names");
evaluation_retained_test!(affine_coefficient_names_refuse, &["SOLVE", "x=1", "FOR x"], super::super::ExternalRelationSymbols::default(), "creo affine coefficient names");
evaluation_retained_test!(affine_unknown_value_names_refuse, &["SOLVE", "x=1", "FOR x"], super::super::ExternalRelationSymbols::default(), "creo affine unknown value names");
solve_storage_test!(nonlinear_known_value_nodes_refuse, &["y=3", "x=2", "SOLVE", "x*x*x=8", "FOR x"], "creo nonlinear known value nodes");
solve_storage_test!(nonlinear_unknown_value_nodes_refuse, &["x=2", "SOLVE", "x*x*x=8", "FOR x"], "creo nonlinear unknown value nodes");
solve_storage_test!(nonlinear_residual_rows_refuse, &["x=2", "SOLVE", "x*x*x=8", "FOR x"], "creo nonlinear residual rows");
solve_storage_test!(nonlinear_jacobian_coefficients_refuse, &["x=2", "SOLVE", "x*x*x=8", "FOR x"], "creo nonlinear Jacobian coefficients");
solve_storage_test!(nonlinear_positive_probe_refuses, &["x=2", "SOLVE", "x*x*x=8", "FOR x"], "creo nonlinear positive probe");
solve_storage_test!(nonlinear_negative_probe_refuses, &["x=2", "SOLVE", "x*x*x=8", "FOR x"], "creo nonlinear negative probe");
solve_storage_test!(nonlinear_jacobian_rows_refuse, &["x=2", "SOLVE", "x*x*x=8", "FOR x"], "creo nonlinear Jacobian rows");
evaluation_retained_test!(nonlinear_known_value_names_refuse, &["y=3", "x=2", "SOLVE", "x*x*x=8", "FOR x"], super::super::ExternalRelationSymbols::default(), "creo nonlinear known value names");
evaluation_retained_test!(nonlinear_unknown_value_names_refuse, &["x=2", "SOLVE", "x*x*x=8", "FOR x"], super::super::ExternalRelationSymbols::default(), "creo nonlinear unknown value names");
evaluation_retained_test!(nonlinear_known_string_values_refuse, &["y=\"text\"", "x=2", "SOLVE", "x*x*x=8", "FOR x"], super::super::ExternalRelationSymbols::default(), "creo nonlinear known string values");

#[test]
fn solve_unknowns_refuse_before_vector_growth() {
    let service = with_expression_policy(DecodePolicy::service(), |ctx| {
        super::super::curve_expression_solve_unknowns(ctx, "x, Y")
    })
    .expect("service profile")
    .expect("valid unknowns");
    assert_eq!(service.iter().map(|unknown| unknown.name.as_str()).collect::<Vec<_>>(), ["x", "Y"]);
    assert!(with_expression_policy(DecodePolicy::service(), |ctx| {
        super::super::curve_expression_solve_unknowns(ctx, "x, X")
    })
    .expect("service profile")
    .is_none());

    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let error = with_expression_policy(policy, |ctx| {
        super::super::curve_expression_solve_unknowns(ctx, "x")
    })
    .expect_err("one unknown exceeds collection limit");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo solve unknowns"));
}

#[test]
fn solve_unknown_name_refuses_before_retained_copy() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
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
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let error = with_expression_policy(policy, |ctx| {
        super::super::curve_expression_solve_unknowns(ctx, "x,y")
    })
    .expect_err("second unknown needs one comparison");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "creo solve unknown duplicate checks"));
}

#[test]
fn curve_expression_labels_refuse_before_vector_growth() {
    assert_eq!(
        parse(ONE_COMMENT, DecodePolicy::service())
            .expect("service profile admits expression")
            .len(),
        1
    );
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let error = parse(ONE_COMMENT, policy).expect_err("one label needs one collection item");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo expression record labels"
    ));
}

#[test]
fn curve_expression_local_system_body_refuses_before_copy() {
    assert_eq!(
        parse(WITH_LOCAL_SYSTEM, DecodePolicy::service())
            .expect("service profile admits local system")
            .len(),
        1
    );
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let error =
        parse(WITH_LOCAL_SYSTEM, policy).expect_err("the local-system body needs retained bytes");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "creo expression local-system body"
    ));
}

#[test]
fn curve_expression_lines_refuse_before_vector_growth() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let error = parse(ONE_COMMENT, policy).expect_err("the line follows one label");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo expression record lines"
    ));
}

#[test]
fn curve_expression_line_text_refuses_before_copy() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let error = parse(ONE_COMMENT, policy).expect_err("the source line needs retained bytes");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "creo expression record line text"
    ));
}

#[test]
fn curve_expression_records_refuse_before_vector_growth() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let error = parse(ONE_COMMENT, policy).expect_err("the record follows its label and line");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo expression records"
    ));
}

#[test]
fn curve_parameter_scalar_cache_refuses_before_unique_image_growth() {
    let payload = [0x46, 0x08, 0, 0, 0, 0, 0, 0];
    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy)
            .expect("root input is admitted");
        super::super::parameter_records_with_face_ids(&ctx, &payload, None)
    };
    assert!(run(3).expect("service admits scalar image").is_empty());
    let error = run(0).expect_err("scalar image requires a set node");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo scalar cache unique images"
    ));
}

#[test]
fn depdb_curve_scalar_cache_refuses_before_unique_image_growth() {
    let payload = b"crv_array\0\xf2\xf8\x02crv_id\0\x06type\0\x08feat_id\0\x04topol_ref_data\0\x46\x08\0\0\0\0\0\0";
    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy)
            .expect("root input is admitted");
        super::super::depdb_cross_section_rows(&ctx, payload)
    };
    assert!(run(100).expect("service admits scalar image").is_empty());
    let error = run(0).expect_err("scalar image requires a set node");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo scalar cache unique images"
    ));
}

fn scalar_lane_with_limits(
    body: &[u8],
    type_byte: u8,
    max_collection_items: u64,
    max_retained_bytes: u64,
) -> Result<super::super::CurveScalarLane, CodecError> {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_collection_items;
    policy.limits.max_retained_bytes = max_retained_bytes;
    let (ctx, _) =
        DecodeContext::from_root_bytes(body, &arena, &policy).expect("root input is admitted");
    super::super::curve_scalar_lane(&ctx, body, type_byte, &crate::scalar::ScalarCache::default())
}

#[test]
fn curve_parameter_references_refuse_before_vector_growth() {
    let body = [0xf7, 1];
    assert_eq!(
        scalar_lane_with_limits(&body, 0, 3, 100)
            .expect("service limits admit reference")
            .references.len(),
        1
    );
    let error = scalar_lane_with_limits(&body, 0, 2, 100)
        .expect_err("reference follows two claim slots");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo curve parameter references"));
}

#[test]
fn curve_parameter_scalars_refuse_before_vector_growth() {
    assert_eq!(
        scalar_lane_with_limits(&[0x0e], 8, 2, 100)
            .expect("service limits admit scalar")
            .scalar_tokens.len(),
        1
    );
    let error = scalar_lane_with_limits(&[0x0e], 8, 1, 100)
        .expect_err("scalar follows one claim slot");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo curve parameter scalars"));
}

#[test]
fn curve_scalar_raw_token_refuses_before_copy() {
    assert_eq!(
        scalar_lane_with_limits(&[0x0e], 8, 2, 1)
            .expect("one raw byte is admitted")
            .scalar_tokens[0].raw,
        [0x0e]
    );
    let error = scalar_lane_with_limits(&[0x0e], 8, 2, 0)
        .expect_err("one raw byte needs retained admission");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo curve scalar raw token"));
}

#[test]
fn curve_zero_raw_token_refuses_before_copy() {
    assert_eq!(
        scalar_lane_with_limits(&[0x18], 0, 2, 1)
            .expect("one zero byte is admitted")
            .scalar_tokens[0].raw,
        [0x18]
    );
    let error = scalar_lane_with_limits(&[0x18], 0, 2, 0)
        .expect_err("one zero byte needs retained admission");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo curve zero raw token"));
}

#[test]
fn curve_opaque_spans_refuse_before_vector_growth() {
    assert_eq!(
        scalar_lane_with_limits(&[0xff], 0, 2, 1)
            .expect("one opaque span is admitted")
            .opaque_spans.len(),
        1
    );
    let error = scalar_lane_with_limits(&[0xff], 0, 1, 1)
        .expect_err("opaque span follows one claim slot");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo curve opaque spans"));
}

#[test]
fn curve_opaque_raw_span_refuses_before_copy() {
    assert_eq!(
        scalar_lane_with_limits(&[0xff], 0, 2, 1)
            .expect("one opaque byte is admitted")
            .opaque_spans[0].raw,
        [0xff]
    );
    let error = scalar_lane_with_limits(&[0xff], 0, 2, 0)
        .expect_err("one opaque byte needs retained admission");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo curve opaque raw span"));
}

#[test]
fn curve_parameter_records_refuse_before_vector_growth() {
    let payload = b"topol_ref_data\0\x07\x08\x04\x01\xf6\xff\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3";
    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy)
            .expect("root input is admitted");
        super::super::parameter_records_with_face_ids(&ctx, payload, None)
    };
    let admitted = (1..100)
        .find(|&limit| run(limit).is_ok())
        .expect("service profile admits one row");
    assert_eq!(run(admitted).expect("row is admitted").len(), 1);
    let error = run(admitted - 1).expect_err("last collection item is the record");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo curve parameter records"));
}

#[test]
fn expression_conditional_validation_refuses_before_stack_growth() {
    let lines = [
        super::super::CurveExpressionLine {
            text: "if 1".to_string(),
            offset: 0,
        },
        super::super::CurveExpressionLine {
            text: "endif".to_string(),
            offset: 5,
        },
    ];
    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is admitted");
        super::super::expression_program_control_is_valid(&ctx, &lines)
    };
    assert_eq!(run(1).expect("one conditional is admitted"), true);
    let error = run(0).expect_err("one conditional requires a validation slot");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo expression conditional validation"));
}

#[test]
fn expression_conditional_parent_refuses_before_stack_growth() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let frame = || super::super::ConditionalFrame {
        parent: super::super::CurveExpressionActivation::Active,
        condition: Some(true),
    };
    let mut stack = super::super::ConditionalStack::default();
    stack.push(&ctx, frame()).expect("first frame is held inline");
    let error = stack.push(&ctx, frame()).expect_err("a nested frame needs a parent slot");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo expression conditional parents"));
    assert_eq!(
        stack.end(),
        super::super::CurveExpressionActivation::Active
    );
}

const ONE_DEPDB_CURVE_ROW: &[u8] = b"crv_array\0\xf2\xf8\x02crv_id\0\x06type\0\x08feat_id\0\x04topol_ref_data\0\x07\x08\x04\x01\xf6\xe4\xff\0\x09\x0a\0\xe1\xe0next_record\0";

fn depdb_rows_with_limit(limit: u64) -> Result<Vec<super::super::DepdbCurveRow>, CodecError> {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(ONE_DEPDB_CURVE_ROW, &arena, &policy)
        .expect("root input is admitted");
    super::super::depdb_cross_section_rows(&ctx, ONE_DEPDB_CURVE_ROW)
}

#[test]
fn depdb_curve_rows_refuse_before_fallible_reservation() {
    assert_eq!(
        depdb_rows_with_limit(100)
            .expect("service admits one complete curve row")
            .len(),
        1
    );
    let error = depdb_rows_with_limit(0).expect_err("one row requires one collection item");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo cross-section curve rows"
    ));
}

#[test]
fn depdb_curve_boundaries_refuse_before_vec_growth() {
    let error = depdb_rows_with_limit(1).expect_err("boundary follows row reservation");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo cross-section row boundaries"
    ));
}

#[test]
fn depdb_segment_unique_candidates_need_no_collection_items() {
    let segment = [7, 8, 4, 1, 0xf6, 0, 9, 10, 0];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&segment, &arena, &policy).expect("root input is admitted");
    let row = super::super::parse_depdb_curve_segment(
        &ctx,
        &segment,
        0,
        &crate::scalar::ScalarCache::default(),
    )
    .expect("candidate scan has no collection")
    .expect("one prefix and one suffix");
    assert_eq!(row.id, 7);
    assert_eq!(row.suffix.x1, 9);
}

#[test]
fn depdb_segment_distinct_prefixes_remain_ambiguous() {
    let segment = [7, 8, 4, 1, 0xf6, 8, 8, 4, 1, 0xf6, 0, 9, 10, 0];
    assert!(crate::decode::with_test_decode_ctx(|ctx| {
        super::super::parse_depdb_curve_segment(
            ctx,
            &segment,
            0,
            &crate::scalar::ScalarCache::default(),
        )
    })
    .expect("ambiguous segment is parsed")
    .is_none());
}

#[test]
fn curve_prototype_vec_refuses_before_growth() {
    let payload = b"crv_array\0crv_id\0\x07type\0\x08feat_id\0\x04";
    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy)
            .expect("root input is admitted");
        super::super::prototypes(&ctx, payload)
    };
    assert_eq!(run(1).expect("service admits one prototype").len(), 1);
    let error = run(0).expect_err("one prototype requires a vector item");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo curve prototypes"
    ));
}

#[test]
fn solve_synchronization_refuses_each_retained_value() {
    use super::super::{CurveExpressionActivation, CurveExpressionAssignment,
        CurveExpressionSolveBlock, CurveExpressionTarget, CurveExpressionValue, SolveUnknown};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let assignment = CurveExpressionAssignment {
        target: CurveExpressionTarget::Parameter { name: "a".into(), declared_unit: None },
        expression: "\"ab\"".into(), dependencies: Vec::new(),
        value: Some(CurveExpressionValue::String("ab".into())),
        activation: CurveExpressionActivation::Active, offset: 1,
    };
    for (assignment_value, solution, operation) in [
        (Some(CurveExpressionValue::String("ab".into())), None, "creo synchronized assignment values"),
        (None, Some(CurveExpressionValue::String("ab".into())), "creo synchronized solve values"),
    ] {
        let mut evaluated = assignment.clone();
        evaluated.value = assignment_value;
        let solutions = solution.into_iter().map(|value| (0, vec![value])).collect();
        let mut blocks = vec![CurveExpressionSolveBlock {
            equations: Vec::new(), assignments: vec![assignment.clone()],
            unknowns: vec![SolveUnknown { name: "x".into(), solution: None }],
            offset: 0, for_offset: 2,
        }];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert!(matches!(super::super::synchronize_solve_blocks(&ctx, &mut blocks, &[evaluated.clone()], &solutions),
            Err(cadmpeg_core::CodecError::ResourceLimit(resource))
            if resource.dimension == ResourceDimension::RetainedBytes && resource.operation == operation));
        crate::decode::with_test_decode_ctx(|ctx| {
            super::super::synchronize_solve_blocks(ctx, &mut blocks, &[evaluated.clone()], &solutions)
        }).expect("service synchronization");
        assert_eq!(blocks[0].assignments[0], evaluated);
        assert_eq!(blocks[0].unknowns[0].solution.as_ref(), solutions.get(&0).and_then(|values| values.first()));
    }
}
