// SPDX-License-Identifier: Apache-2.0
use crate::features::{
    DesignParameter, Dimension, Expression, ExpressionToken, ParameterId, ParameterValue,
};
use crate::pmi::{DimensionKind, PmiAnnotation, PmiDefinition, PmiDimension, PmiTarget};
use crate::CadIr;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

fn document() -> CadIr {
    let input = ParameterId::mint("test:model:parameter#input").unwrap();
    let consumer = ParameterId::mint("test:model:parameter#consumer").unwrap();
    let mut ir = CadIr::empty();
    let parameter = |id, expression| DesignParameter {
        product_definition: None,
        id,
        owner: None,
        ordinal: 0,
        name: "test:model:parameter#plain-text".into(),
        expression,
        driving: true,
        display: None,
        value: Some(ParameterValue::Length(
            crate::scalar::Length::new(2.0).unwrap(),
        )),
        properties: std::collections::BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };
    ir.model
        .parameters
        .push(parameter(input.clone(), "2 mm".into()));
    ir.model.parameters.push(parameter(
        consumer,
        Expression {
            tokens: vec![
                ExpressionToken::Parameter(input.clone()),
                ExpressionToken::Text(" + ".into()),
                ExpressionToken::Parameter(input.clone()),
            ],
        },
    ));
    ir.model.pmi.push(PmiAnnotation {
        id: crate::ids::PmiId::mint("test:model:pmi#dimension").unwrap(),
        native_ref: None,
        name: Some("test:model:parameter#plain-text".into()),
        visible: None,
        targets: vec![
            PmiTarget::Body {
                body: crate::ids::BodyId::mint("test:model:body#one").unwrap(),
            },
            PmiTarget::Body {
                body: crate::ids::BodyId::mint("test:model:body#one").unwrap(),
            },
        ],
        definition: PmiDefinition::Dimension(
            PmiDimension::new(DimensionKind::Size, Some(Dimension::Parameter(input)), None)
                .unwrap(),
        ),
    });
    ir
}

#[test]
fn registry_reference_visit_keeps_real_owners_expression_multiplicity_optional_dimensions_and_lists(
) {
    let ir = document();
    let before = ir.model.clone();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut edges = Vec::new();
    ir.visit_neutral_references(&ctx, &mut |owner, target| {
        edges.push((owner.to_owned(), target.to_owned()));
        Ok(())
    })
    .unwrap();
    assert_eq!(
        edges
            .iter()
            .filter(|(owner, target)| owner == "test:model:parameter#consumer"
                && target == "test:model:parameter#input")
            .count(),
        2
    );
    assert_eq!(
        edges
            .iter()
            .filter(|(owner, target)| owner == "test:model:pmi#dimension"
                && target == "test:model:parameter#input")
            .count(),
        1
    );
    assert_eq!(
        edges
            .iter()
            .filter(|(owner, target)| owner == "test:model:pmi#dimension"
                && target == "test:model:body#one")
            .count(),
        2
    );
    assert!(edges
        .iter()
        .all(|(owner, target)| owner != target && target != "test:model:parameter#plain-text"));
    assert_eq!(ir.model, before);
}

#[test]
fn registry_reference_visit_uses_the_actual_context_recursion_budget() {
    let ir = document();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        ir.visit_neutral_references(&ctx, &mut |_, _| Ok(())),
        Err(CodecError::ResourceLimit(_))
    ));
}

#[test]
fn registry_reference_visit_propagates_the_actual_callback_error() {
    let ir = document();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut calls = 0;
    let result = ir.visit_neutral_references(&ctx, &mut |_, _| {
        calls += 1;
        Err(CodecError::malformed("stop visiting references"))
    });
    assert!(
        matches!(result, Err(CodecError::Malformed(message)) if message == "stop visiting references")
    );
    assert_eq!(calls, 1);
}
