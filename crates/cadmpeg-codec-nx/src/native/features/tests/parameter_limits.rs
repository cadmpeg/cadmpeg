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

fn parameter_binding_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let (input, reference, expression) = parameter_binding_input();
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        crate::native::features::feature_parameter_bindings(
            ctx,
            std::slice::from_ref(&input),
            std::slice::from_ref(&reference),
            std::slice::from_ref(&expression),
        )
    };
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| decode(ctx))
            .expect("admitted parameter binding")
            .len(),
        1
    );
    
    
    
    crate::test_support::with_decode_context_over(&[], |policy| { configure(policy); }, |ctx| {

    decode(ctx).expect_err("parameter binding resource limit")

})
}

#[test]
fn parameter_binding_refuses_collection_limit() {
    let error = parameter_binding_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn parameter_binding_refuses_retained_limit() {
    let error = parameter_binding_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn parameter_binding_refuses_work_limit() {
    let error = parameter_binding_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

fn parameter_use_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let (input, reference, expression) = parameter_binding_input();
    let mut bindings = crate::test_support::with_decode_context(|ctx| {
        crate::native::features::feature_parameter_bindings(
            ctx,
            std::slice::from_ref(&input),
            std::slice::from_ref(&reference),
            std::slice::from_ref(&expression),
        )
    })
    .expect("admitted parameter binding input");
    let mut second = bindings[0].clone();
    second.id = "second-binding".into();
    second.source_offset = 801;
    bindings.push(second);
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        crate::native::features::feature_parameter_uses(ctx, &bindings)
    };
    let admitted = crate::test_support::with_decode_context(|ctx| decode(ctx))
        .expect("admitted parameter use");
    assert_eq!(admitted.len(), 1);
    assert_eq!(admitted[0].bindings.len(), 2);
    
    
    
    crate::test_support::with_decode_context_over(&[], |policy| { configure(policy); }, |ctx| {

    decode(ctx).expect_err("parameter use resource limit")

})
}

#[test]
fn parameter_use_refuses_collection_limit() {
    let error = parameter_use_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn parameter_use_refuses_retained_limit() {
    let error = parameter_use_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn parameter_use_refuses_work_limit() {
    let error = parameter_use_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}
