// SPDX-License-Identifier: Apache-2.0

use crate::curve::test_support::{expression_lines, solve_phase_inputs, with_expression_policy};
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn affine_materialized_limit_reaches(
    source: &[&str],
    external_symbols: &crate::curve::ExternalRelationSymbols,
    operation: &'static str,
) {
    let (block, values, _) = solve_phase_inputs(source, external_symbols);
    let known: Vec<_> = block
        .unknowns
        .iter()
        .map(|unknown| {
            values
                .get(&unknown.name.to_ascii_lowercase())
                .and_then(crate::curve::quantity_parts_ref)
                .map(|(_, dimension)| dimension)
        })
        .collect();
    let dimensions = with_expression_policy(DecodePolicy::service(), |ctx| {
        crate::curve::solve::infer_solve_variable_dimensions(
            ctx,
            &block,
            &values,
            &known,
            crate::curve::RelationEvaluationContext::default(),
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
                crate::curve::RelationEvaluationContext::default(),
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

affine_materialized_test!(
    affine_variable_names_refuse,
    &["SOLVE", "x=1", "FOR x"],
    crate::curve::ExternalRelationSymbols::default(),
    "creo affine variable names"
);
affine_materialized_test!(
    affine_known_value_names_refuse,
    &["y=2", "SOLVE", "x+y=3", "FOR x"],
    crate::curve::ExternalRelationSymbols::default(),
    "creo affine known value names"
);
affine_materialized_test!(
    affine_coefficient_names_refuse,
    &["SOLVE", "x=1", "FOR x"],
    crate::curve::ExternalRelationSymbols::default(),
    "creo affine coefficient names"
);
affine_materialized_test!(
    affine_unknown_value_names_refuse,
    &["SOLVE", "x=1", "FOR x"],
    crate::curve::ExternalRelationSymbols::default(),
    "creo affine unknown value names"
);
fn nonlinear_materialized_limit_reaches(
    source: &[&str],
    external_symbols: &crate::curve::ExternalRelationSymbols,
    operation: &'static str,
) {
    let lines = expression_lines(source);
    with_expression_policy(DecodePolicy::service(), |ctx| {
        crate::curve::test_support::evaluate_program_details(ctx, &lines, None, external_symbols)
    })
    .expect("service profile solves the original expression");
    let block = with_expression_policy(DecodePolicy::service(), |ctx| {
        crate::curve::test_support::compile_solve_program(ctx, &lines)
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
        crate::curve::test_support::evaluate_program_details(
            ctx,
            &preceding,
            None,
            external_symbols,
        )
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
                .and_then(crate::curve::quantity_parts_ref)
                .expect("initial numeric value")
                .0
        })
        .collect();
    let dimensions = vec![crate::curve::RelationDimension::default(); point.len()];
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::MaterializedBytes,
        operation,
        |ctx| {
            crate::curve::solve::evaluate_nonlinear_residuals(
                ctx,
                &block,
                &values,
                &dimensions,
                &point,
                crate::curve::RelationEvaluationContext::default(),
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
    crate::curve::ExternalRelationSymbols::default(),
    "creo nonlinear known value names"
);
nonlinear_materialized_test!(
    nonlinear_unknown_value_names_refuse,
    &["x=2", "SOLVE", "x*x*x=8", "FOR x"],
    crate::curve::ExternalRelationSymbols::default(),
    "creo nonlinear unknown value names"
);
nonlinear_materialized_test!(
    nonlinear_known_string_values_refuse,
    &["y=\"text\"", "x=2", "SOLVE", "x*x*x=8", "FOR x"],
    crate::curve::ExternalRelationSymbols::default(),
    "creo nonlinear known string values"
);
