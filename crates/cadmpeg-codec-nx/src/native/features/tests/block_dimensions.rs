// SPDX-License-Identifier: Apache-2.0

use crate::native::features::feature_block_dimensions;
use crate::native::features::FeatureConstructionMember;

#[test]
fn nx_block_dimensions_do_not_cross_expression_sections() {
    use crate::native::features::FeatureBlockConstruction;
    use crate::native::features::FeatureParameterBinding;
    use crate::native::om::{Expression, ExpressionDeclaration, ExpressionUnit};

    let operation = "nx:feature-history:operation-label#0-1";
    let construction = FeatureBlockConstruction {
        id: "nx:feature-history:block-construction#0-1".into(),
        operation_label: operation.into(),
        control: 0,
        members: std::array::from_fn(|ordinal| FeatureConstructionMember {
            reference: format!("reference#{ordinal}"),
            data_block: format!("block#{ordinal}"),
        }),
        terminal_reference: "terminal-reference".into(),
        terminal_data_block: "terminal-block".into(),
    };
    let binding = FeatureParameterBinding {
        id: "binding".into(),
        operation_label: operation.into(),
        input_slot: crate::om::header_references::HeaderSlot::Zero,
        input_block: "input".into(),
        reference_ordinal: 0,
        expression_declaration: "declaration-20".into(),
        expression: Some("expression-20".into()),
        object_id: 20,
        source_offset: 1,
    };
    let declaration = |index: u32, source_entry: &str| ExpressionDeclaration {
        id: format!("declaration-{index}"),
        object_id: index,
        record: format!("{source_entry}:entry#{index}"),
        name: crate::om::parameter_name::ParameterName::<_, u32>::parse(format!("p{index}"))
            .unwrap(),
        literal: None,
        source_entry: source_entry.into(),
        source_offset: u64::from(index),
    };
    let expression = |index: u32, source_entry: &str, source_table: &str| Expression {
        id: format!("expression-{index}"),
        owner: Some(crate::native::om::ExpressionOwner {
            object_id: index,
            record: format!("{source_entry}:entry#{index}"),
        }),
        declaration: Some(format!("declaration-{index}")),
        name: crate::om::parameter_name::ParameterName::new(format!("p{index}")),
        unit: ExpressionUnit::Millimeter,
        expression: index.to_string(),
        value: Some(cadmpeg_ir::scalar::FiniteReal::try_from(f64::from(index)).unwrap()),
        source_entry: source_entry.into(),
        source_table: cadmpeg_core::text::NonBlankString::new(source_table).unwrap(),
        source_offset: u64::from(index),
    };
    let mut expressions = [
        expression(20, "section-a", "table-a"),
        expression(21, "section-a", "table-a"),
        expression(22, "section-b", "table-b"),
    ];
    let mut declarations = [
        declaration(20, "section-a"),
        declaration(21, "section-a"),
        declaration(22, "section-b"),
    ];

    assert!(feature_block_dimensions(
        std::slice::from_ref(&construction),
        std::slice::from_ref(&binding),
        &declarations,
        &expressions,
    )
    .is_empty());

    declarations[2].source_entry = "section-a".into();
    declarations[2].record = "section-a:entry#22".into();
    assert!(feature_block_dimensions(
        std::slice::from_ref(&construction),
        std::slice::from_ref(&binding),
        &declarations,
        &expressions,
    )
    .is_empty());

    expressions[2].source_entry = "section-a".into();
    expressions[2].source_table = cadmpeg_core::text::NonBlankString::new("table-a").unwrap();
    assert_eq!(
        feature_block_dimensions(
            std::slice::from_ref(&construction),
            std::slice::from_ref(&binding),
            &declarations,
            &expressions,
        )
        .len(),
        1
    );

    for expression in &mut expressions {
        expression.unit = ExpressionUnit::Inch;
    }
    let dimensions = feature_block_dimensions(
        std::slice::from_ref(&construction),
        std::slice::from_ref(&binding),
        &declarations,
        &expressions,
    );
    assert_eq!(
        dimensions[0]
            .dimensions
            .each_ref()
            .map(|dimension| dimension.value.get()),
        [508.0, 533.4, 558.8]
    );
}

#[test]
fn nx_block_dimensions_refuse_an_inch_length_that_overflows_millimeters() {
    use crate::native::features::FeatureBlockConstruction;
    use crate::native::features::FeatureParameterBinding;
    use crate::native::om::{Expression, ExpressionDeclaration, ExpressionUnit};

    let operation = "nx:feature-history:operation-label#0-1";
    let construction = FeatureBlockConstruction {
        id: "nx:feature-history:block-construction#0-1".into(),
        operation_label: operation.into(),
        control: 0,
        members: std::array::from_fn(|ordinal| FeatureConstructionMember {
            reference: format!("reference#{ordinal}"),
            data_block: format!("block#{ordinal}"),
        }),
        terminal_reference: "terminal-reference".into(),
        terminal_data_block: "terminal-block".into(),
    };
    let binding = FeatureParameterBinding {
        id: "binding".into(),
        operation_label: operation.into(),
        input_slot: crate::om::header_references::HeaderSlot::Zero,
        input_block: "input".into(),
        reference_ordinal: 0,
        expression_declaration: "declaration-20".into(),
        expression: Some("expression-20".into()),
        object_id: 20,
        source_offset: 1,
    };
    let declaration = |index: u32, source_entry: &str| ExpressionDeclaration {
        id: format!("declaration-{index}"),
        object_id: index,
        record: format!("{source_entry}:entry#{index}"),
        name: crate::om::parameter_name::ParameterName::<_, u32>::parse(format!("p{index}"))
            .unwrap(),
        literal: None,
        source_entry: source_entry.into(),
        source_offset: u64::from(index),
    };
    let expression = |index: u32, source_entry: &str, source_table: &str| Expression {
        id: format!("expression-{index}"),
        owner: Some(crate::native::om::ExpressionOwner {
            object_id: index,
            record: format!("{source_entry}:entry#{index}"),
        }),
        declaration: Some(format!("declaration-{index}")),
        name: crate::om::parameter_name::ParameterName::new(format!("p{index}")),
        unit: ExpressionUnit::Millimeter,
        expression: index.to_string(),
        value: Some(cadmpeg_ir::scalar::FiniteReal::try_from(f64::from(index)).unwrap()),
        source_entry: source_entry.into(),
        source_table: cadmpeg_core::text::NonBlankString::new(source_table).unwrap(),
        source_offset: u64::from(index),
    };
    let mut expressions = [
        expression(20, "section-a", "table-a"),
        expression(21, "section-a", "table-a"),
        expression(22, "section-a", "table-a"),
    ];
    let declarations = [
        declaration(20, "section-a"),
        declaration(21, "section-a"),
        declaration(22, "section-a"),
    ];

    for expression in &mut expressions {
        expression.unit = ExpressionUnit::Inch;
    }
    assert_eq!(
        feature_block_dimensions(
            std::slice::from_ref(&construction),
            std::slice::from_ref(&binding),
            &declarations,
            &expressions,
        )
        .len(),
        1
    );

    expressions[2].value = Some(cadmpeg_ir::scalar::FiniteReal::try_from(f64::MAX).unwrap());
    assert!(feature_block_dimensions(
        std::slice::from_ref(&construction),
        std::slice::from_ref(&binding),
        &declarations,
        &expressions,
    )
    .is_empty());
}

