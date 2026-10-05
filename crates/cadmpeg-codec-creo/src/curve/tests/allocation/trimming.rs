// SPDX-License-Identifier: Apache-2.0
use super::expression_lines;

#[test]
fn evaluated_relation_trim_refuses_work() {
    let lines = expression_lines(&["x=1"]);
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo evaluated relation line trim",
        |ctx| {
            crate::curve::evaluate_expression_program_details(
                ctx,
                &lines,
                None,
                &crate::curve::ExternalRelationSymbols::default(),
            )
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo evaluated relation line trim")
    );
}

#[test]
fn relation_control_trim_refuses_work() {
    let lines = expression_lines(&["if 1", "endif"]);
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo relation control line trim",
        |ctx| crate::curve::expression_program_control_is_valid(ctx, &lines),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo relation control line trim")
    );
}

#[test]
fn relation_keyword_trim_refuses_before_false() {
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo relation keyword trim",
        |ctx| crate::curve::starts_relation_keyword(ctx, " OTHER ", "if"),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo relation keyword trim")
    );
}

#[test]
fn relation_condition_trim_refuses_before_missing_expression() {
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo relation condition trim",
        |ctx| crate::curve::conditional_keyword_expression(ctx, " IF ", "if"),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo relation condition trim")
    );
}

#[test]
fn final_target_argument_trim_refuses_work() {
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo final target argument trim",
        |ctx| crate::curve::split_assignment_target_arguments(ctx, " x , y "),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo final target argument trim")
    );
}

#[test]
fn separated_target_argument_trim_refuses_work() {
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo separated target argument trim",
        |ctx| crate::curve::split_assignment_target_arguments(ctx, " x , y "),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo separated target argument trim")
    );
}

#[test]
fn function_target_body_trim_refuses_before_empty_arguments() {
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo function target body trim",
        |ctx| crate::curve::expression_target_function_call(ctx, "foo( )"),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo function target body trim")
    );
}

#[test]
fn relation_declared_unit_trim_refuses_work() {
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo relation declared unit trim",
        |ctx| crate::curve::expression_assignment_target(ctx, "x[ mm ]"),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo relation declared unit trim")
    );
}

#[test]
fn solve_right_trim_refuses_work() {
    let lines = expression_lines(&["SOLVE", "x = 1", "FOR x"]);
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo solve right operand trim",
        |ctx| crate::curve::curve_expression_solve_program(ctx, &lines),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo solve right operand trim")
    );
}

#[test]
fn solve_left_trim_refuses_work() {
    let lines = expression_lines(&["SOLVE", "x = 1", "FOR x"]);
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo solve left operand trim",
        |ctx| crate::curve::curve_expression_solve_program(ctx, &lines),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo solve left operand trim")
    );
}

#[test]
fn solve_source_trim_refuses_before_comment_skip() {
    let lines = expression_lines(&[" /* comment */ "]);
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo solve source line trim",
        |ctx| crate::curve::curve_expression_solve_program(ctx, &lines),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo solve source line trim")
    );
}

#[test]
fn assignment_expression_trim_refuses_work() {
    let lines = expression_lines(&[" x = 1 "]);
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo assignment expression trim",
        |ctx| crate::curve::expression_assignment(ctx, &lines[0]),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo assignment expression trim")
    );
}

#[test]
fn assignment_name_trim_refuses_work() {
    let lines = expression_lines(&[" x = 1 "]);
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo assignment name trim",
        |ctx| crate::curve::expression_assignment(ctx, &lines[0]),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo assignment name trim")
    );
}

#[test]
fn assignment_source_trim_refuses_before_comment_skip() {
    let lines = expression_lines(&[" /* comment */ "]);
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo assignment source line trim",
        |ctx| crate::curve::expression_assignment(ctx, &lines[0]),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo assignment source line trim")
    );
}
