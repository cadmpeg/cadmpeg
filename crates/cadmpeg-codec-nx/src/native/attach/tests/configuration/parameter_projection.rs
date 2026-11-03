// SPDX-License-Identifier: Apache-2.0

use crate::native::attach::expressions::attach_expression_parameters;

#[test]
fn nx_block_dimension_parameters_list_source_content() {
    let expression = |key: u32| crate::native::om::ParameterFormula {
        id: format!("nx:test:expression#{key}"),
        owner: None,
        declaration: None,
        name: crate::om::parameter_name::ParameterName::new(format!("p{key}")),
        unit: crate::native::om::ExpressionUnit::Millimeter,
        expression: key.to_string(),
        value: Some(cadmpeg_ir::scalar::FiniteReal::try_from(f64::from(key)).unwrap()),
        source_entry: "part".into(),
        source_table: cadmpeg_core::text::NonBlankString::new("nx:test:expression-table#table")
            .unwrap(),
        source_offset: u64::from(key),
    };
    let expressions = [expression(20), expression(21), expression(22)];
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::test_support::with_decode_context(|ctx| {
        attach_expression_parameters(
            ctx,
            &mut ir,
            &expressions,
            &mut annotations,
            &mut Vec::new(),
        )
    })
    .expect("valid exactness fields");
    assert_eq!(
        (&*ir.model.features[0].source_content),
        ir.model
            .parameters
            .iter()
            .map(|parameter| {
                cadmpeg_ir::features::FeatureSourceContent::Parameter(parameter.id.clone())
            })
            .collect::<Vec<_>>()
    );
}

#[test]
fn nx_inch_expression_values_are_attached_in_millimeters() {
    let expression = |key: u32, name: &str, formula: &str, value: Option<f64>| {
        crate::native::om::ParameterFormula {
            id: format!("nx:test:expression#{key}"),
            owner: None,
            declaration: None,
            name: crate::om::parameter_name::ParameterName::new(name.to_string()),
            unit: crate::native::om::ExpressionUnit::Inch,
            expression: formula.into(),
            value: value.map(|value| cadmpeg_ir::scalar::FiniteReal::try_from(value).unwrap()),
            source_entry: "/Root/UG_PART/UG_PART".into(),
            source_table: cadmpeg_core::text::NonBlankString::new("nx:test:expression-table#table")
                .unwrap(),
            source_offset: u64::from(key),
        }
    };
    let expressions = [
        expression(1, "p1", "2", Some(2.0)),
        expression(2, "p2", "p1 * 3", Some(6.0)),
    ];
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();

    crate::test_support::with_decode_context(|ctx| {
        attach_expression_parameters(
            ctx,
            &mut ir,
            &expressions,
            &mut annotations,
            &mut Vec::new(),
        )
    })
    .expect("valid exactness fields");

    assert_eq!(
        ir.model.parameters[0].value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(2.0 * 25.4).unwrap()
        ))
    );
    assert_eq!(
        ir.model.parameters[1].value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(6.0 * 25.4).unwrap()
        ))
    );
    // A bare inch number no longer verifies against its millimetre cache.
    assert_eq!(
        crate::decode::feature_completeness::incomplete_expression_parameters(&ir),
        ir.model
            .parameters
            .iter()
            .map(|parameter| parameter.id.clone())
            .collect()
    );
}

#[test]
fn nx_native_expression_units_remain_outside_neutral_values() {
    let expression = crate::native::om::ParameterFormula {
        id: "nx:test:expression#native".into(),
        owner: None,
        declaration: None,
        name: crate::om::parameter_name::ParameterName::new("p1".to_string()),
        unit: crate::native::om::ExpressionUnit::Native("custom/unit".into()),
        expression: "4".into(),
        value: Some(cadmpeg_ir::scalar::FiniteReal::try_from(4.0).unwrap()),
        source_entry: "part".into(),
        source_table: cadmpeg_core::text::NonBlankString::new("nx:test:expression-table#table")
            .unwrap(),
        source_offset: 1,
    };
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();

    crate::test_support::with_decode_context(|ctx| {
        attach_expression_parameters(
            ctx,
            &mut ir,
            &[expression],
            &mut annotations,
            &mut Vec::new(),
        )
    })
    .expect("valid exactness fields");

    assert_eq!(ir.model.parameters[0].value, None);
    assert_eq!(
        crate::decode::feature_completeness::incomplete_expression_parameters(&ir),
        [ir.model.parameters[0].id.clone()].into()
    );
}
