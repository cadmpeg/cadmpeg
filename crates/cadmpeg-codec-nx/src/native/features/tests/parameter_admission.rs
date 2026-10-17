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

#[test]
fn parameter_uses_group_many_bindings_like_the_pairwise_rule() {
    use crate::native::features::FeatureParameterBinding;

    let (input, reference, expression) = parameter_binding_input();
    let base = crate::native::features::feature_parameter_bindings(
        std::slice::from_ref(&input),
        std::slice::from_ref(&reference),
        std::slice::from_ref(&expression),
    )
    .pop()
    .expect("one binding");
    let bindings = (0..2048_u64)
        .map(|index| {
            let mut binding = base.clone();
            binding.id = format!("binding#{index}");
            binding.operation_label = format!("operation#{}", index % 64);
            binding.expression =
                (!index.is_multiple_of(5)).then(|| format!("expression#{}", index % 48));
            binding.source_offset = 10_000 - index % 700;
            binding
        })
        .collect::<Vec<_>>();

    let uses = crate::native::features::feature_parameter_uses(&bindings);

    let mut expected = Vec::new();
    for (ordinal, binding) in bindings.iter().enumerate() {
        let Some(expression) = binding.expression.as_deref() else {
            continue;
        };
        let same = |candidate: &&FeatureParameterBinding| {
            candidate.operation_label == binding.operation_label
                && candidate.expression.as_deref() == Some(expression)
        };
        if bindings[..ordinal].iter().any(|candidate| same(&candidate)) {
            continue;
        }
        let mut occurrences = bindings
            .iter()
            .filter(same)
            .map(|candidate| (candidate.source_offset, candidate.id.clone()))
            .collect::<Vec<_>>();
        occurrences.sort_by_key(|(offset, _)| *offset);
        expected.push((
            binding.operation_label.clone(),
            expression.to_owned(),
            occurrences,
        ));
    }
    expected.sort_by(|left, right| {
        left.2[0]
            .0
            .cmp(&right.2[0].0)
            .then_with(|| left.0.cmp(&right.0))
            .then_with(|| left.1.cmp(&right.1))
    });
    let actual = uses
        .iter()
        .map(|parameter_use| {
            (
                parameter_use.operation_label.clone(),
                parameter_use.expression.clone(),
                parameter_use
                    .bindings
                    .iter()
                    .map(|binding| (binding.source_offset, binding.binding.clone()))
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
    assert_eq!(uses.len(), 192);
}
