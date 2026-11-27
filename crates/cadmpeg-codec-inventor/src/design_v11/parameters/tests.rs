// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::test_support::root_part_ir;
use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::scalar::Length;

fn parameter(name: &str, text: &str, number: Option<f64>) -> LegacyParameter {
    LegacyParameter {
        product_definition: None,
        id: ParameterId::mint(format!("inventor:design:parameter#{name}")).expect("id"),
        owner: None,
        ordinal: 0,
        name: name.into(),
        expression: text.into(),
        display: None,
        value: number.and_then(Length::new).map(LegacyValue::Length),
        dependencies: DistinctMembers::default(),
        properties: BTreeMap::default(),
        pmi: None,
        native_ref: Some(format!("inventor:record:parameter#{name}")),
    }
}

fn project_parameters(parameters: Vec<LegacyParameter>) -> CadIr {
    let mut ir = root_part_ir();
    let original = parameters.clone();
    ir.model.parameters = parameters;
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    project(&ctx, &mut ir, &[], &[]).expect("projection");
    assert_eq!(ir.model.parameters, original);
    ir
}

fn text(parameter: &DesignParameter) -> expr::NeutralWire<ParameterRef> {
    let ParameterDefinition::Expression(Expression::Neutral(Neutral(tree))) = &parameter.definition
    else {
        panic!("neutral expression");
    };
    expr::print(tree).expect("canonical expression")
}

#[test]
fn numeric_expression_keeps_authored_unit_and_canonical_cache() {
    let ir = project_parameters(vec![parameter("width", "(0.5 in) + (2 mm)", Some(14.7))]);
    let p = &ir.design.parameters[0];
    assert_eq!(text(p).text, "0.5[in] + 2[mm]");
    assert_eq!(p.kind, ValueKind::Quantity(Quantity::LENGTH));
    assert_eq!(
        p.value,
        Some(ParameterValue::Number(
            FiniteReal::new(14.7).expect("cache")
        ))
    );
    assert_eq!(p.scope.as_str(), "inventor:document:product#root");
    assert_eq!(p.owner, None);
    assert_eq!(p.unit, None);
    assert_eq!(p.tolerance, None);
}

#[test]
fn parameter_reference_and_feature_owner_keep_identity() {
    let width = parameter("width", "10 mm", Some(10.0));
    let mut half = parameter("half", "width / (2 ul)", Some(5.0));
    half.dependencies.extend([width.id.clone()]);
    half.owner = Some(
        cadmpeg_ir::features::FeatureId::mint("inventor:design:feature#sketch").expect("feature"),
    );
    let ir = project_parameters(vec![width.clone(), half.clone()]);
    let p = &ir.design.parameters[0];
    assert_eq!(p.id, half.id);
    assert_eq!(p.owner, half.owner.map(ParameterOwner::Feature));
    assert_eq!(text(p).text, "$0 / 2");
    assert_eq!(
        text(p).refs,
        vec![ParameterRef {
            target: RefTarget::Parameter(width.id),
            through: None
        }]
    );
}

#[test]
fn source_expression_retains_text_references_and_cache() {
    let width = parameter("width", "10 mm", Some(10.0));
    let mut p = parameter("custom", "isolate(width; mm; ul)", Some(4.0));
    p.dependencies.extend([width.id.clone()]);
    let ir = project_parameters(vec![width.clone(), p]);
    assert_eq!(
        ir.design.parameters[0].definition,
        ParameterDefinition::Expression(Expression::Source {
            dialect: ExpressionDialect::Inventor,
            tokens: vec![
                ExpressionToken::Text("isolate(".into()),
                ExpressionToken::Reference(ParameterRef {
                    target: RefTarget::Parameter(width.id),
                    through: None
                }),
                ExpressionToken::Text("; mm; ul)".into())
            ],
        })
    );
    assert_eq!(
        ir.design.parameters[0].value,
        Some(ParameterValue::Number(FiniteReal::new(4.0).expect("cache")))
    );
}

#[test]
fn ordering_and_ids_are_deterministic() {
    let a = parameter("a", "1 mm", Some(1.0));
    let z = parameter("z", "2 mm", Some(2.0));
    let forward = project_parameters(vec![a.clone(), z.clone()]);
    let reverse = project_parameters(vec![z, a]);
    assert_eq!(forward.design.parameters, reverse.design.parameters);
    assert!(forward.design.parameters[0].id < forward.design.parameters[1].id);
}

#[test]
fn unit_symbols_and_absent_cache_remain_explicit() {
    for symbol in ["mm", "cm", "in", "rad", "deg"] {
        assert!(unit(symbol).is_some());
    }
    assert_eq!(unit("ul"), Some(Unit::default()));
    assert_eq!(unit("unknown"), None);
    let ir = project_parameters(vec![parameter("unset", "unknown()", None)]);
    assert_eq!(ir.design.parameters[0].value, None);
    assert!(matches!(
        ir.design.parameters[0].definition,
        ParameterDefinition::Expression(Expression::Source { .. })
    ));
}

#[test]
fn trig_with_dimensionless_argument_stays_source() {
    let ir = project_parameters(vec![parameter("ratio", "sin(30 ul)", Some(0.5))]);
    assert!(matches!(
        ir.design.parameters[0].definition,
        ParameterDefinition::Expression(Expression::Source { .. })
    ));
}

#[test]
fn ambiguous_sign_and_modulo_stay_source() {
    for expression in ["sign(0)", "-3 % 2", "1 mm + 2 rad"] {
        let ir = project_parameters(vec![parameter("p", expression, Some(1.0))]);
        assert!(matches!(
            ir.design.parameters[0].definition,
            ParameterDefinition::Expression(Expression::Source {
                dialect: ExpressionDialect::Inventor,
                ..
            })
        ));
    }
}

#[test]
fn declared_unit_joins_native_parameter_and_unit_records() {
    use crate::design::PmDcUnitPayload;
    use crate::pmdc::PmDcReference;
    use crate::record_identity::{Located, RecordTypeId};

    let (mut raw, mut legacy) = crate::feature::fillet::tests::parameter(
        0,
        LegacyValue::Length(Length::new(25.4).expect("length")),
    );
    raw.edit(|payload| payload.unit = PmDcReference::from_packed(2));
    legacy.expression = "1 in".into();
    let make_unit = |ordinal, kind| {
        let payload: PmDcUnitPayload = json::from_value(cadmpeg_core::json!({
            "save_version_major":29, "header_value":0, "header_id":0, "kind":kind
        }))
        .expect("unit payload");
        Located::new(
            payload,
            RecordTypeId::from_bytes(crate::pmdc::inventor_id(1)),
            raw.identity.segment_token.clone(),
            ordinal,
        )
    };
    let units = [
        make_unit(
            1,
            cadmpeg_core::json!({
                "form":"definition", "numerators":[PmDcReference::from_packed(3)], "numerator_metadata":[0,0],
                "denominators":[], "denominator_metadata":null, "visible":true, "derived":PmDcReference::from_packed(0)
            }),
        ),
        make_unit(
            2,
            cadmpeg_core::json!({
                "form":"base", "dimension":"length", "symbol":"in", "scale_to_internal":2.54, "magnitude":1.0, "factor":1.0
            }),
        ),
    ];
    let mut ir = project_parameters(Vec::new());
    ir.model.parameters.push(legacy);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    project(&ctx, &mut ir, &[raw], &units).expect("projection");
    assert_eq!(ir.design.parameters[0].unit, unit("in"));
    assert_eq!(
        ir.design.parameters[0].kind,
        ValueKind::Quantity(Quantity::LENGTH)
    );
    assert_eq!(
        ir.design.parameters[0].value,
        Some(ParameterValue::Number(
            FiniteReal::new(25.4).expect("cache")
        ))
    );
    assert_eq!(text(&ir.design.parameters[0]).text, "1[in]");
}

#[test]
fn quantity_powers_keep_integral_exponents() {
    for (expression, expected, cached) in [
        ("(3 mm)^2 / (1 mm)", "3[mm]^2 / 1[mm]", 9.0),
        ("((3 mm)^2)^0.5", "(3[mm]^2)^0.5", 3.0),
    ] {
        let ir = project_parameters(vec![parameter("length", expression, Some(cached))]);
        assert_eq!(text(&ir.design.parameters[0]).text, expected);
    }
    let ir = project_parameters(vec![parameter("length", "(3 mm)^0.5", Some(3.0))]);
    assert!(matches!(
        ir.design.parameters[0].definition,
        ParameterDefinition::Expression(Expression::Source { .. })
    ));
}
