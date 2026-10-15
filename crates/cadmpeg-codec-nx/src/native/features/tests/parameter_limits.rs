// SPDX-License-Identifier: Apache-2.0

fn parameter_binding_input() -> (
    crate::native::features::FeatureInputBlock,
    crate::native::om::DataBlockReference,
    crate::native::om::ParameterFormula,
) {
    let input = crate::native::features::FeatureInputBlock {
        id: "input".into(),
        operation_label: "operation#1".into(),
        input_slot: crate::om::header_references::HeaderSlot::Zero,
        object: crate::om::reference_index::FeatureReferenceToken::from_wire(45, &[45])
            .expect("input token"),
        data_block: "block#45".into(),
        source_offset: 700,
    };
    let reference = crate::native::om::DataBlockReference {
        id: "reference".into(),
        data_block: input.data_block.clone(),
        ordinal: 0,
        object: crate::om::reference_index::FeatureReferenceToken::from_wire(201, &[0x80, 201])
            .expect("reference token"),
        target_record: None,
        target_expression_declaration: Some("declaration".into()),
        source_offset: 800,
    };
    let expression = crate::native::om::ParameterFormula {
        id: "expression#1".into(),
        owner: None,
        declaration: Some("declaration".into()),
        name: crate::om::parameter_name::ParameterName::new("p1".into()),
        unit: crate::native::om::ExpressionUnit::Millimeter,
        expression: "12".into(),
        value: Some(cadmpeg_ir::scalar::FiniteReal::try_from(12.0).expect("finite expression")),
        source_entry: "section".into(),
        source_table: cadmpeg_core::text::NonBlankString::new("table").expect("source table"),
        source_offset: 900,
    };
    (input, reference, expression)
}

#[test]
fn parameter_binding_admits_fixture() {
    let (input, reference, expression) = parameter_binding_input();
    let decode = || {
        crate::native::features::feature_parameter_bindings(
            std::slice::from_ref(&input),
            std::slice::from_ref(&reference),
            std::slice::from_ref(&expression),
        )
    };
    assert_eq!(decode().len(), 1);
}

#[test]
fn parameter_use_admits_fixture() {
    let (input, reference, expression) = parameter_binding_input();
    let mut bindings = {
        crate::native::features::feature_parameter_bindings(
            std::slice::from_ref(&input),
            std::slice::from_ref(&reference),
            std::slice::from_ref(&expression),
        )
    };
    let mut second = bindings[0].clone();
    second.id = "second-binding".into();
    second.source_offset = 801;
    bindings.push(second);
    let decode = || crate::native::features::feature_parameter_uses(&bindings);
    let admitted = decode();
    assert_eq!(admitted.len(), 1);
    assert_eq!(admitted[0].bindings.len(), 2);
}
