// SPDX-License-Identifier: Apache-2.0
//! Derived controls retain nonliteral formulas and their source driver identities.

use super::*;
use cadmpeg_ir::{features::ParameterValue, scalar::PositiveLength};

#[test]
fn magnitude_keeps_the_authored_source_formula_and_cache() {
    let owner = FeatureId::mint("test:feature:thickness#0").unwrap();
    let driver = ParameterId::mint("test:parameter:driver#0").unwrap();
    let raw = ParameterId::mint("test:parameter:value#0").unwrap();
    let source = DesignParameter {
        id: raw.clone(),
        owner: Some(owner.clone()),
        ordinal: 0,
        name: "Value".into(),
        expression: Expression {
            tokens: vec![
                ExpressionToken::Text("-2 * ".into()),
                ExpressionToken::Parameter(driver),
            ],
        },
        driving: true,
        display: None,
        value: Some(ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(-2.0).unwrap(),
        )),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some("native:Value".into()),
    };
    let mut parameters = vec![source.clone()];
    let mut dimension = Dimension::Literal(PositiveLength::new(2.0).unwrap());
    computed_control(
        &mut dimension,
        &owner,
        &mut parameters,
        "Value",
        "magnitude",
        "abs(",
    )
    .unwrap();
    assert_eq!(parameters[0], source);
    assert_eq!(
        parameters[1].expression.tokens,
        vec![
            ExpressionToken::Text("abs(".into()),
            ExpressionToken::Parameter(raw),
            ExpressionToken::Text(")".into()),
        ]
    );
    assert_eq!(dimension.parameter(), Some(&parameters[1].id));
    assert_eq!(dimension.value_from(&parameters).unwrap().get(), 2.0);
    assert!(!parameters[1].driving);
}

#[test]
fn registered_angular_controls_keep_their_quantity_with_generic_float_carriers() {
    use cadmpeg_ir::Codec;
    let decoded = crate::FcstdCodec
        .decode(
            &mut std::io::Cursor::new(crate::test_support::test_archive::CORE_OPERATIONS),
            &cadmpeg_ir::DecodeOptions::default(),
        )
        .unwrap();
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Revolution"))
        .unwrap();
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| {
            parameter.owner.as_ref() == Some(&feature.id) && parameter.name == "Angle"
        })
        .unwrap();
    let ParameterValue::Angle(value) = parameter.value.as_ref().unwrap() else {
        panic!("registered revolution angle must remain angular")
    };
    assert_eq!(value.get(), 225_f64.to_radians());
    assert_eq!(
        parameter
            .expression
            .literal_value_as(parameter.value.as_ref().unwrap()),
        parameter.value
    );
    let dimension = Dimension::<cadmpeg_ir::scalar::PositiveAngle>::Parameter(parameter.id.clone());
    assert_eq!(
        decoded.ir().dimension_value(&dimension).unwrap().get(),
        225_f64.to_radians()
    );
    let properties = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let source = properties
        .iter()
        .find(|property| property.id == parameter.native_ref.as_deref().unwrap())
        .unwrap();
    assert_eq!(source.type_name, "App::PropertyFloatConstraint");
    assert_eq!(
        source.values()[0].attributes["value"],
        "225.0000000000000000"
    );
    crate::test_support::test_archive::assert_valid_document(decoded.ir());
}
