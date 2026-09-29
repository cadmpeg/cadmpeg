// SPDX-License-Identifier: Apache-2.0
use crate::decode::feature_completeness;

use std::collections::BTreeMap;

#[test]
fn nx_expression_parameter_references_preserve_formula_order() {
    assert_eq!(
        crate::native::om::expression_parameter_names(
            "max(p12, p3) + p12 + exp2 + p7_radius + p7_radius + p4bad + p5_"
        )
        .collect::<Vec<_>>(),
        vec!["p12", "p3", "p12", "p7_radius", "p7_radius"]
    );
}

#[test]
fn nx_expression_graph_rejects_noncanonical_parameter_tokens() {
    let expression = |name: &str, formula: &str, value: Option<f64>| crate::native::om::Expression {
        id: format!("nx:test:expression#{name}"),
        owner: None,
        declaration: None,
        name: crate::om::parameter_name::ParameterName::new(name.to_string()),
        unit: crate::native::om::ExpressionUnit::Millimeter,
        expression: formula.into(),
        value: value.map(|value| cadmpeg_ir::scalar::FiniteReal::try_from(value).unwrap()),
        source_entry: "part".into(),
        source_table: cadmpeg_core::text::NonBlankString::new("nx:test:expression-table#table")
            .unwrap(),
        source_offset: 0,
    };
    let mut expressions = vec![
        expression("p4", "3", Some(3.0)),
        expression("p5", "p4bad + 2", None),
        expression("p6", "p4_ + 2", None),
    ];

    crate::test_support::with_decode_context(|ctx| {
        crate::native::om::evaluate_expression_graphs(ctx, &mut expressions)
    })
    .unwrap();

    assert_eq!(
        expressions[1]
            .value
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        None
    );
    assert_eq!(
        expressions[2]
            .value
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        None
    );
}

#[test]
fn nx_expression_graph_evaluates_exact_qualified_dependencies() {
    let expression = |name: &str, formula: &str, value: Option<f64>| crate::native::om::Expression {
        id: format!("nx:test:expression#{name}"),
        owner: None,
        declaration: None,
        name: crate::om::parameter_name::ParameterName::new(name.to_string()),
        unit: crate::native::om::ExpressionUnit::Millimeter,
        expression: formula.into(),
        value: value.map(|value| cadmpeg_ir::scalar::FiniteReal::try_from(value).unwrap()),
        source_entry: "part".into(),
        source_table: cadmpeg_core::text::NonBlankString::new("nx:test:expression-table#table")
            .unwrap(),
        source_offset: 0,
    };
    let mut expressions = vec![
        expression("p7", "3", Some(3.0)),
        expression("p7_radius", "5", Some(5.0)),
        expression("p8", "p7_radius * 2", None),
        expression("p9", "p8 + p7", None),
    ];

    crate::test_support::with_decode_context(|ctx| {
        crate::native::om::evaluate_expression_graphs(ctx, &mut expressions)
    })
    .unwrap();

    assert_eq!(
        expressions[2]
            .value
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        Some(10.0)
    );
    assert_eq!(
        expressions[3]
            .value
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        Some(13.0)
    );
}

#[test]
fn nx_expression_graph_substitutes_dependencies_as_atomic_operands() {
    let expression = |name: &str, formula: &str, value: Option<f64>| crate::native::om::Expression {
        id: format!("nx:test:expression#{name}"),
        owner: None,
        declaration: None,
        name: crate::om::parameter_name::ParameterName::new(name.to_string()),
        unit: crate::native::om::ExpressionUnit::Millimeter,
        expression: formula.into(),
        value: value.map(|value| cadmpeg_ir::scalar::FiniteReal::try_from(value).unwrap()),
        source_entry: "part".into(),
        source_table: cadmpeg_core::text::NonBlankString::new("nx:test:expression-table#table")
            .unwrap(),
        source_offset: 0,
    };
    let mut expressions = vec![
        expression("p1", "-2", Some(-2.0)),
        expression("p2", "p1^2", None),
        expression("p3", "-p1^2", None),
    ];

    crate::test_support::with_decode_context(|ctx| {
        crate::native::om::evaluate_expression_graphs(ctx, &mut expressions)
    })
    .unwrap();

    assert_eq!(
        expressions[1]
            .value
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        Some(4.0)
    );
    assert_eq!(
        expressions[2]
            .value
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        Some(-4.0)
    );
}

#[test]
fn nx_expression_graph_scopes_names_to_their_expression_table() {
    let expression = |id: &str, table: &str, name: &str, formula: &str, value: Option<f64>| {
        crate::native::om::Expression {
            id: id.into(),
            owner: None,
            declaration: None,
            name: crate::om::parameter_name::ParameterName::new(name.to_string()),
            unit: crate::native::om::ExpressionUnit::Millimeter,
            expression: formula.into(),
            value: value.map(|value| cadmpeg_ir::scalar::FiniteReal::try_from(value).unwrap()),
            source_entry: "part".into(),
            source_table: cadmpeg_core::text::NonBlankString::new(table).unwrap(),
            source_offset: 0,
        }
    };
    let mut expressions = vec![
        expression(
            "a-p2",
            "nx:test:expression-table#table-a",
            "p2",
            "5",
            Some(5.0),
        ),
        expression(
            "a-p3",
            "nx:test:expression-table#table-a",
            "p3",
            "p2 * 2",
            None,
        ),
        expression(
            "b-p2",
            "nx:test:expression-table#table-b",
            "p2",
            "7",
            Some(7.0),
        ),
        expression(
            "b-p3",
            "nx:test:expression-table#table-b",
            "p3",
            "p2 * 2",
            None,
        ),
    ];

    crate::test_support::with_decode_context(|ctx| {
        crate::native::om::evaluate_expression_graphs(ctx, &mut expressions)
    })
    .unwrap();

    assert_eq!(
        expressions[1]
            .value
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        Some(10.0)
    );
    assert_eq!(
        expressions[3]
            .value
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        Some(14.0)
    );
}

#[test]
fn nx_expression_graph_rejects_every_duplicate_name_in_one_table() {
    let expression = |id: &str, table: &str, name: &str, formula: &str, value: Option<f64>| {
        crate::native::om::Expression {
            id: id.into(),
            owner: None,
            declaration: None,
            name: crate::om::parameter_name::ParameterName::new(name.to_string()),
            unit: crate::native::om::ExpressionUnit::Millimeter,
            expression: formula.into(),
            value: value.map(|value| cadmpeg_ir::scalar::FiniteReal::try_from(value).unwrap()),
            source_entry: "part".into(),
            source_table: cadmpeg_core::text::NonBlankString::new(table).unwrap(),
            source_offset: 0,
        }
    };
    let mut expressions = vec![
        expression(
            "a-p1-first",
            "nx:test:expression-table#table-a",
            "p1",
            "3",
            Some(3.0),
        ),
        expression(
            "a-p1-second",
            "nx:test:expression-table#table-a",
            "p1",
            "5",
            Some(5.0),
        ),
        expression(
            "a-p2",
            "nx:test:expression-table#table-a",
            "p2",
            "p1 * 2",
            None,
        ),
        expression(
            "b-p1",
            "nx:test:expression-table#table-b",
            "p1",
            "7",
            Some(7.0),
        ),
        expression(
            "b-p2",
            "nx:test:expression-table#table-b",
            "p2",
            "p1 * 2",
            None,
        ),
    ];

    crate::test_support::with_decode_context(|ctx| {
        crate::native::om::evaluate_expression_graphs(ctx, &mut expressions)
    })
    .unwrap();

    assert_eq!(
        expressions[0]
            .value
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        None
    );
    assert_eq!(
        expressions[1]
            .value
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        None
    );
    assert_eq!(
        expressions[2]
            .value
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        None
    );
    assert_eq!(
        expressions[3]
            .value
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        Some(7.0)
    );
    assert_eq!(
        expressions[4]
            .value
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        Some(14.0)
    );
}

#[test]
fn nx_expression_graph_scopes_equal_names_by_declared_unit() {
    let expression = |id: &str,
                      name: &str,
                      unit: crate::native::om::ExpressionUnit,
                      formula: &str,
                      value: Option<f64>| {
        crate::native::om::Expression {
            id: id.into(),
            owner: None,
            declaration: None,
            name: crate::om::parameter_name::ParameterName::new(name.to_string()),
            unit,
            expression: formula.into(),
            value: value.map(|value| cadmpeg_ir::scalar::FiniteReal::try_from(value).unwrap()),
            source_entry: "part".into(),
            source_table: cadmpeg_core::text::NonBlankString::new(
                "nx:test:expression-table#table",
            )
            .unwrap(),
            source_offset: 0,
        }
    };
    let mut expressions = vec![
        expression(
            "length-p1",
            "p1",
            crate::native::om::ExpressionUnit::Millimeter,
            "5",
            Some(5.0),
        ),
        expression(
            "angle-p1",
            "p1",
            crate::native::om::ExpressionUnit::Degree,
            "45",
            Some(45.0),
        ),
        expression(
            "length-p2",
            "p2",
            crate::native::om::ExpressionUnit::Millimeter,
            "p1 * 2",
            None,
        ),
        expression(
            "angle-p2",
            "p2",
            crate::native::om::ExpressionUnit::Degree,
            "p1 / 3",
            None,
        ),
    ];

    crate::test_support::with_decode_context(|ctx| {
        crate::native::om::evaluate_expression_graphs(ctx, &mut expressions)
    })
    .unwrap();

    assert_eq!(
        expressions[0]
            .value
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        Some(5.0)
    );
    assert_eq!(
        expressions[1]
            .value
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        Some(45.0)
    );
    assert_eq!(
        expressions[2]
            .value
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        Some(10.0)
    );
    assert_eq!(
        expressions[3]
            .value
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        Some(15.0)
    );
}

#[test]
fn nx_formula_dependencies_resolve_to_section_parameters() {
    let expression = |key: u32, name: &str, text: &str, value: Option<f64>| crate::native::om::Expression {
        id: format!("nx:test:expression#{key}"),
        owner: None,
        declaration: None,
        name: crate::om::parameter_name::ParameterName::new(name.to_string()),
        unit: crate::native::om::ExpressionUnit::Millimeter,
        expression: text.into(),
        value: value.map(|value| cadmpeg_ir::scalar::FiniteReal::try_from(value).unwrap()),
        source_entry: "/Root/UG_PART/UG_PART".into(),
        source_table: cadmpeg_core::text::NonBlankString::new("nx:test:expression-table#table")
            .unwrap(),
        source_offset: u64::from(key),
    };
    let expressions = [
        expression(20, "p2", "5", Some(5.0)),
        expression(21, "p2_radius", "7", Some(7.0)),
        expression(90, "p9", "p2_radius * 2 + p2_radius", None),
    ];
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::test_support::with_decode_context(|ctx| {
        crate::native::attach::expressions::attach_expression_parameters(
            ctx,
            &mut ir,
            &expressions,
            &[],
            &[],
            &mut annotations,
        )
    })
    .expect("valid exactness fields");

    assert_eq!(ir.model.parameters[2].value, None);
    assert_eq!(
        ir.model.parameters[2].dependencies.as_slice(),
        vec![ir.model.parameters[1].id.clone()]
    );
}

#[test]
fn nx_formula_dependencies_reject_ambiguous_parameter_names() {
    let expression = |key: u32, name: &str, text: &str| crate::native::om::Expression {
        id: format!("nx:test:expression#{key}"),
        owner: None,
        declaration: None,
        name: crate::om::parameter_name::ParameterName::new(name.to_string()),
        unit: crate::native::om::ExpressionUnit::Millimeter,
        expression: text.into(),
        value: None,
        source_entry: "/Root/UG_PART/UG_PART".into(),
        source_table: cadmpeg_core::text::NonBlankString::new("nx:test:expression-table#table")
            .unwrap(),
        source_offset: u64::from(key),
    };
    let expressions = [
        expression(20, "p2", "5"),
        expression(21, "p2", "7"),
        expression(90, "p9", "p2 * 2"),
    ];
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::test_support::with_decode_context(|ctx| {
        crate::native::attach::expressions::attach_expression_parameters(
            ctx,
            &mut ir,
            &expressions,
            &[],
            &[],
            &mut annotations,
        )
    })
    .expect("valid exactness fields");

    assert!(ir.model.parameters[2].dependencies.is_empty());
}

#[test]
fn nx_formula_dependencies_bind_equal_names_within_declared_unit() {
    let expression =
        |key: u32, name: &str, unit: crate::native::om::ExpressionUnit, text: &str, value: Option<f64>| {
            crate::native::om::Expression {
                id: format!("nx:test:expression#{key}"),
                owner: None,
                declaration: None,
                name: crate::om::parameter_name::ParameterName::new(name.to_string()),
                unit,
                expression: text.into(),
                value: value
                    .map(|value| cadmpeg_ir::scalar::FiniteReal::try_from(value).unwrap()),
                source_entry: "/Root/UG_PART/UG_PART".into(),
                source_table: cadmpeg_core::text::NonBlankString::new(
                    "nx:test:expression-table#table",
                )
                .unwrap(),
                source_offset: u64::from(key),
            }
        };
    let expressions = [
        expression(10, "p1", crate::native::om::ExpressionUnit::Millimeter, "5", Some(5.0)),
        expression(11, "p1", crate::native::om::ExpressionUnit::Degree, "45", Some(45.0)),
        expression(
            20,
            "p2",
            crate::native::om::ExpressionUnit::Millimeter,
            "p1 * 2",
            Some(10.0),
        ),
        expression(
            21,
            "p2",
            crate::native::om::ExpressionUnit::Degree,
            "p1 / 3",
            Some(15.0),
        ),
    ];
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();

    crate::test_support::with_decode_context(|ctx| {
        crate::native::attach::expressions::attach_expression_parameters(
            ctx,
            &mut ir,
            &expressions,
            &[],
            &[],
            &mut annotations,
        )
    })
    .expect("valid exactness fields");

    assert_eq!(
        ir.model.parameters[2].dependencies.as_slice(),
        [ir.model.parameters[0].id.clone()]
    );
    assert_eq!(
        ir.model.parameters[3].dependencies.as_slice(),
        [ir.model.parameters[1].id.clone()]
    );
    assert_eq!(
        ir.model.parameters[0]
            .properties
            .get("unit")
            .map(String::as_str),
        Some("millimeter")
    );
    assert_eq!(
        ir.model.parameters[1]
            .properties
            .get("unit")
            .map(String::as_str),
        Some("degree")
    );
    assert!(crate::test_support::with_decode_context(|ctx| {
        feature_completeness::incomplete_expression_parameters(ctx, &ir)
    })
    .unwrap()
    .is_empty());

    ir.model.parameters[0]
        .properties
        .insert(cadmpeg_core::nonblank_literal!("unit"), "native".into());
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| {
            feature_completeness::incomplete_expression_parameters(ctx, &ir)
        })
        .unwrap(),
        [
            ir.model.parameters[0].id.clone(),
            ir.model.parameters[2].id.clone(),
        ]
        .into()
    );
}

#[test]
fn nx_formula_dependencies_resolve_within_the_expression_table() {
    let expression =
        |id: &str, table: &str, name: &str, text: &str, source_offset: u64| crate::native::om::Expression {
            id: format!("nx:test:expression#{id}"),
            owner: None,
            declaration: None,
            name: crate::om::parameter_name::ParameterName::new(name.to_string()),
            unit: crate::native::om::ExpressionUnit::Millimeter,
            expression: text.into(),
            value: None,
            source_entry: "/Root/UG_PART/UG_PART".into(),
            source_table: cadmpeg_core::text::NonBlankString::new(table).unwrap(),
            source_offset,
        };
    let expressions = [
        expression(
            "a-p3",
            "nx:test:expression-table#table-a",
            "p3",
            "p2 * 2",
            40,
        ),
        expression(
            "b-p3",
            "nx:test:expression-table#table-b",
            "p3",
            "p2 * 2",
            10,
        ),
        expression("a-p2", "nx:test:expression-table#table-a", "p2", "5", 30),
        expression("b-p2", "nx:test:expression-table#table-b", "p2", "7", 20),
    ];
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();

    crate::test_support::with_decode_context(|ctx| {
        crate::native::attach::expressions::attach_expression_parameters(
            ctx,
            &mut ir,
            &expressions,
            &[],
            &[],
            &mut annotations,
        )
    })
    .expect("valid exactness fields");

    assert_eq!(ir.model.features.len(), 2);
    assert_eq!(
        ir.model.features[0].id.as_str(),
        "nx:test:feature#equations-table-b"
    );
    assert_eq!(ir.model.features[0].ordinal, 0);
    assert_eq!(
        ir.model.features[1].id.as_str(),
        "nx:test:feature#equations-table-a"
    );
    assert_eq!(ir.model.features[1].ordinal, 1);
    assert_eq!(
        ir.model
            .parameters
            .iter()
            .map(|parameter| (parameter.name.as_str(), parameter.ordinal))
            .collect::<Vec<_>>(),
        [("p2", 0), ("p3", 1), ("p2", 0), ("p3", 1)]
    );
    assert_eq!(ir.model.parameters[1].owner, ir.model.parameters[0].owner);
    assert_eq!(
        ir.model.parameters[1].dependencies.as_slice(),
        [ir.model.parameters[0].id.clone()]
    );
    assert_eq!(ir.model.parameters[3].owner, ir.model.parameters[2].owner);
    assert_eq!(
        ir.model.parameters[3].dependencies.as_slice(),
        [ir.model.parameters[2].id.clone()]
    );
    assert_ne!(ir.model.parameters[1].owner, ir.model.parameters[3].owner);
    for (parameter, value) in ir.model.parameters.iter_mut().zip([7.0, 14.0, 5.0, 10.0]) {
        parameter.value = Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(value).unwrap(),
        ));
    }
    assert!(crate::test_support::with_decode_context(|ctx| {
        feature_completeness::incomplete_expression_parameters(ctx, &ir)
    })
    .unwrap()
    .is_empty());

    let mut inconsistent = ir.clone();
    inconsistent.model.parameters[1].value =
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(1.0).unwrap(),
        ));
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| {
            feature_completeness::incomplete_expression_parameters(ctx, &inconsistent)
        })
        .unwrap(),
        [inconsistent.model.parameters[1].id.clone()].into()
    );

    let mut duplicate_name = ir.clone();
    duplicate_name.model.parameters[1].name = duplicate_name.model.parameters[0].name.clone();
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| {
            feature_completeness::incomplete_expression_parameters(ctx, &duplicate_name)
        })
        .unwrap(),
        duplicate_name.model.parameters[..2]
            .iter()
            .map(|parameter| parameter.id.clone())
            .collect()
    );

    let mut unevaluated = ir.clone();
    unevaluated.model.parameters[1].value = None;
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| {
            feature_completeness::incomplete_expression_parameters(ctx, &unevaluated)
        })
        .unwrap(),
        [unevaluated.model.parameters[1].id.clone()].into()
    );

    let mut operation_owned = unevaluated;
    operation_owned.model.features[0].evaluation.set_definition(
        cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Native {
                kind: "TEST_OPERATION".into(),
                parameters: BTreeMap::default(),
            },
        ),
    );
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| {
            feature_completeness::incomplete_expression_parameters(ctx, &operation_owned)
        })
        .unwrap(),
        [operation_owned.model.parameters[1].id.clone()].into()
    );
}

#[test]
fn nx_cyclic_formula_table_omits_invalid_neutral_dependency_edges() {
    let expression = |id: &str, name: &str, text: &str, source_offset| crate::native::om::Expression {
        id: format!("nx:test:expression#{id}"),
        owner: None,
        declaration: None,
        name: crate::om::parameter_name::ParameterName::new(name.to_string()),
        unit: crate::native::om::ExpressionUnit::Millimeter,
        expression: text.to_string(),
        value: None,
        source_entry: "part".to_string(),
        source_table: cadmpeg_core::text::NonBlankString::new("nx:test:expression-table#table")
            .unwrap(),
        source_offset,
    };
    let expressions = [
        expression("p2", "p2", "p3 + 1", 10),
        expression("p3", "p3", "p2 + 1", 20),
    ];
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::test_support::with_decode_context(|ctx| {
        crate::native::attach::expressions::attach_expression_parameters(
            ctx,
            &mut ir,
            &expressions,
            &[],
            &[],
            &mut annotations,
        )
    })
    .expect("valid exactness fields");

    assert_eq!(ir.model.parameters[0].expression, "p3 + 1");
    assert_eq!(ir.model.parameters[1].expression, "p2 + 1");
    assert!(ir
        .model
        .parameters
        .iter()
        .all(|parameter| parameter.dependencies.is_empty()));
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| {
            feature_completeness::incomplete_expression_parameters(ctx, &ir)
        })
        .unwrap(),
        ir.model
            .parameters
            .iter()
            .map(|parameter| parameter.id.clone())
            .collect()
    );
    let mut losses = Vec::new();
    crate::test_support::with_decode_context(|ctx| {
        crate::decode::report::append_design_intent_losses(ctx, &ir, &mut losses)
    })
    .unwrap();
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("2 NX expression parameter(s)"));
}

#[test]
fn nx_cyclic_formula_table_retains_independent_acyclic_dependencies() {
    let expression = |id: &str, name: &str, text: &str, source_offset| crate::native::om::Expression {
        id: format!("nx:test:expression#{id}"),
        owner: None,
        declaration: None,
        name: crate::om::parameter_name::ParameterName::new(name.to_string()),
        unit: crate::native::om::ExpressionUnit::Millimeter,
        expression: text.to_string(),
        value: None,
        source_entry: "part".to_string(),
        source_table: cadmpeg_core::text::NonBlankString::new("nx:test:expression-table#table")
            .unwrap(),
        source_offset,
    };
    let expressions = [
        expression("p2", "p2", "p3 + 1", 10),
        expression("p3", "p3", "p2 + 1", 20),
        expression("p5", "p5", "p4 * 2", 40),
        expression("p4", "p4", "7", 30),
    ];
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();

    crate::test_support::with_decode_context(|ctx| {
        crate::native::attach::expressions::attach_expression_parameters(
            ctx,
            &mut ir,
            &expressions,
            &[],
            &[],
            &mut annotations,
        )
    })
    .expect("valid exactness fields");

    assert_eq!(
        ir.model
            .parameters
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect::<Vec<_>>(),
        ["p4", "p5", "p2", "p3"]
    );
    assert_eq!(
        ir.model.parameters[1].dependencies.as_slice(),
        [ir.model.parameters[0].id.clone()]
    );
    assert!(ir.model.parameters[2].dependencies.is_empty());
    assert!(ir.model.parameters[3].dependencies.is_empty());
    for (parameter, value) in ir.model.parameters.iter_mut().zip([7.0, 14.0, 1.0, 1.0]) {
        parameter.value = Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(value).unwrap(),
        ));
    }
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| {
            feature_completeness::incomplete_expression_parameters(ctx, &ir)
        })
        .unwrap(),
        ir.model.parameters[2..]
            .iter()
            .map(|parameter| parameter.id.clone())
            .collect()
    );
}

#[test]
fn nx_parameter_uses_group_binding_witnesses_and_project_consumers() {
    use crate::native::features::{feature_parameter_uses, FeatureParameterBinding};

    let binding = |id: &str, operation: &str, slot: u8, offset: u64| FeatureParameterBinding {
        id: id.to_string(),
        operation_label: operation.to_string(),
        input_slot: crate::om::header_references::HeaderSlot::try_from(slot).unwrap(),
        input_block: format!("block-{slot}"),
        reference_ordinal: 0,
        expression_declaration: "declaration".to_string(),
        expression: Some("nx:test:expression#20".to_string()),
        object_id: 20,
        source_offset: offset,
    };
    let uses = crate::test_support::with_decode_context(|ctx| {
        feature_parameter_uses(
            ctx,
            &[
                binding("late", "nx:feature-history:operation-label#1-2", 1, 30),
                binding("early", "nx:feature-history:operation-label#1-2", 0, 20),
                binding("other", "nx:feature-history:operation-label#1-3", 0, 40),
            ],
        )
    })
    .expect("admitted parameter uses");
    assert_eq!(uses.len(), 2);
    assert_eq!(
        uses[0]
            .bindings
            .iter()
            .map(|binding| binding.binding.as_str())
            .collect::<Vec<_>>(),
        ["early", "late"]
    );
    assert_eq!(
        uses[0]
            .bindings
            .iter()
            .map(|binding| binding.source_offset)
            .collect::<Vec<_>>(),
        [20, 30]
    );

    let expression = crate::native::om::Expression {
        id: "nx:test:expression#20".to_string(),
        owner: None,
        declaration: None,
        name: crate::om::parameter_name::ParameterName::new("p20".to_string()),
        unit: crate::native::om::ExpressionUnit::Millimeter,
        expression: "5".to_string(),
        value: Some(cadmpeg_ir::scalar::FiniteReal::try_from(5.0).unwrap()),
        source_entry: "part".to_string(),
        source_table: cadmpeg_core::text::NonBlankString::new("nx:test:expression-table#table")
            .unwrap(),
        source_offset: 20,
    };
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::test_support::with_decode_context(|ctx| {
        crate::native::attach::expressions::attach_expression_parameters(
            ctx,
            &mut ir,
            &[expression],
            &[],
            &uses,
            &mut annotations,
        )
    })
    .expect("valid exactness fields");
    assert_eq!(
        ir.model.parameters[0].properties["consumer.0"],
        "nx:feature-history:feature#1-2"
    );
    assert_eq!(
        ir.model.parameters[0].properties["consumer.1"],
        "nx:feature-history:feature#1-3"
    );
}

#[test]
fn nx_parameter_consumers_follow_physical_use_order() {
    let expression = crate::native::om::Expression {
        id: "nx:test:expression#20".to_string(),
        owner: None,
        declaration: None,
        name: crate::om::parameter_name::ParameterName::new("p20".to_string()),
        unit: crate::native::om::ExpressionUnit::Millimeter,
        expression: "5".to_string(),
        value: Some(cadmpeg_ir::scalar::FiniteReal::try_from(5.0).unwrap()),
        source_entry: "part".to_string(),
        source_table: cadmpeg_core::text::NonBlankString::new("nx:test:expression-table#table")
            .unwrap(),
        source_offset: 10,
    };
    let parameter_use = |id: &str, operation: &str, source_offset| {
        crate::native::features::FeatureParameterUse {
            id: id.to_string(),
            operation_label: operation.to_string(),
            expression: expression.id.clone(),
            bindings: vec![crate::native::features::FeatureParameterUseBinding {
                binding: format!("binding-{id}"),
                source_offset,
            }],
        }
    };
    let uses = [
        parameter_use("later", "nx:feature-history:operation-label#0-1", 40),
        parameter_use("earlier", "nx:feature-history:operation-label#9-8", 30),
    ];
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::test_support::with_decode_context(|ctx| {
        crate::native::attach::expressions::attach_expression_parameters(
            ctx,
            &mut ir,
            &[expression],
            &[],
            &uses,
            &mut annotations,
        )
    })
    .expect("valid exactness fields");

    assert_eq!(
        ir.model.parameters[0].properties["parameter_use.0"],
        "earlier"
    );
    assert_eq!(
        ir.model.parameters[0].properties["parameter_use.1"],
        "later"
    );
}

#[test]
fn nx_parameter_consumers_depend_on_preceding_expression_owner() {
    let expression = crate::native::om::Expression {
        id: "nx:test:expression#20".to_string(),
        owner: None,
        declaration: None,
        name: crate::om::parameter_name::ParameterName::new("p20".to_string()),
        unit: crate::native::om::ExpressionUnit::Millimeter,
        expression: "5".to_string(),
        value: Some(cadmpeg_ir::scalar::FiniteReal::try_from(5.0).unwrap()),
        source_entry: "part".to_string(),
        source_table: cadmpeg_core::text::NonBlankString::new("nx:test:expression-table#table")
            .unwrap(),
        source_offset: 20,
    };
    let parameter_use = crate::native::features::FeatureParameterUse {
        id: "use".to_string(),
        operation_label: "nx:feature-history:operation-label#1-2".to_string(),
        expression: expression.id.clone(),
        bindings: vec![crate::native::features::FeatureParameterUseBinding {
            binding: "binding".to_string(),
            source_offset: 30,
        }],
    };
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::test_support::with_decode_context(|ctx| {
        crate::native::attach::expressions::attach_expression_parameters(
            ctx,
            &mut ir,
            &[expression],
            &[],
            std::slice::from_ref(&parameter_use),
            &mut annotations,
        )
    })
    .expect("valid exactness fields");
    let parameter_owners = ir
        .model
        .parameters
        .iter()
        .map(|parameter| (parameter.id.clone(), parameter.owner.clone()))
        .collect();
    let dependencies = crate::test_support::with_decode_context(|ctx| {
        crate::native::attach::parameter_owner_dependencies(
            ctx,
            &parameter_owners,
            &[
                cadmpeg_ir::features::ParameterId::mint("nx:test:parameter#20")
                    .expect("identity grammar"),
                cadmpeg_ir::features::ParameterId::mint("nx:test:parameter#20")
                    .expect("identity grammar"),
            ],
        )
    })
    .unwrap();

    assert_eq!(ir.model.features[0].ordinal, 0);
    assert_eq!(
        dependencies,
        [ir.model.parameters[0].owner.clone().unwrap()]
    );
}

#[test]
fn nx_feature_parameter_binding_joins_only_resolved_input_references() {
    use crate::native::om::DataBlockReference;
    use crate::native::features::FeatureInputBlock;

    let input = FeatureInputBlock {
        id: "nx:feature-history:input-block#0-7-0".to_string(),
        operation_label: "nx:feature-history:operation-label#0-7".to_string(),
        input_slot: crate::om::header_references::HeaderSlot::Zero,
        object: crate::om::reference_index::FeatureReferenceToken::from_wire(45, &[45])
            .unwrap(),
        data_block: "nx:om-data-blocks-2:block#45".to_string(),
        source_offset: 700,
    };
    let reference = |ordinal: u32, declaration: Option<&str>| DataBlockReference {
        id: format!("nx:om-data-block-references-2-45:reference#{ordinal}"),
        data_block: input.data_block.clone(),
        ordinal,
        object: crate::om::reference_index::FeatureReferenceToken::from_wire(
            201 + ordinal,
            &[0x80, (201 + ordinal) as u8],
        )
        .unwrap(),
        target_record: Some(format!("nx:om-record-directory-0:entry#{ordinal}")),
        target_expression_declaration: declaration.map(str::to_string),
        source_offset: 800 + u64::from(ordinal),
    };
    let references = [
        reference(0, Some("nx:om-expression-declarations-0:declaration#3")),
        reference(1, None),
    ];

    let expression = crate::native::om::Expression {
        id: "nx:om-entry-9:expression#3".to_string(),
        owner: None,
        declaration: Some("nx:om-expression-declarations-0:declaration#3".to_string()),
        name: crate::om::parameter_name::ParameterName::new("p3".to_string()),
        unit: crate::native::om::ExpressionUnit::Millimeter,
        expression: "12".to_string(),
        value: Some(cadmpeg_ir::scalar::FiniteReal::try_from(12.0).unwrap()),
        source_entry: "/Root/UG_PART/UG_PART".to_string(),
        source_table: cadmpeg_core::text::NonBlankString::new("nx:test:expression-table#table")
            .unwrap(),
        source_offset: 900,
    };
    let bindings = crate::test_support::with_decode_context(|ctx| {
        crate::native::features::feature_parameter_bindings(
            ctx,
            std::slice::from_ref(&input),
            &references,
            std::slice::from_ref(&expression),
        )
    })
    .expect("admitted parameter bindings");
    assert_eq!(bindings.len(), 1);
    assert_eq!(
        bindings[0].id,
        "nx:feature-history:parameter-binding#0-7-0-0"
    );
    assert_eq!(bindings[0].input_slot.number(), 0);
    assert_eq!(bindings[0].reference_ordinal, 0);
    assert_eq!(bindings[0].object_id, 201);
    assert_eq!(
        bindings[0].expression_declaration,
        "nx:om-expression-declarations-0:declaration#3"
    );
    assert_eq!(
        bindings[0].expression.as_deref(),
        Some("nx:om-entry-9:expression#3")
    );

    let mut duplicate = expression.clone();
    duplicate.id = "nx:om-entry-9:expression#30".to_string();
    let ambiguous = crate::test_support::with_decode_context(|ctx| {
        crate::native::features::feature_parameter_bindings(
            ctx,
            &[input],
            &references,
            &[expression, duplicate],
        )
    })
    .expect("ambiguous parameter binding");
    assert_eq!(ambiguous.len(), 1);
    assert_eq!(ambiguous[0].expression, None);
}

