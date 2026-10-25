// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::features::{Expression, ExpressionToken, ParameterId};
use crate::schema::rewrite::{identities, typed::RewriteIdentities};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn structured_attribute_wire_preserves_order_duplicates_and_nullable_numeric_slots() {
    let entries = vec![
        AttributeMapEntry {
            key: "b".into(),
            value: "2".into(),
        },
        AttributeMapEntry {
            key: "a".into(),
            value: "1".into(),
        },
        AttributeMapEntry {
            key: "b".into(),
            value: "3".into(),
        },
    ];
    let value = AttributeValue::Map(entries);
    let wire = serde_json::to_value(&value).unwrap();
    assert_eq!(
        wire,
        serde_json::json!({"kind":"map","value":[{"key":"b","value":"2"},{"key":"a","value":"1"},{"key":"b","value":"3"}]})
    );
    assert_eq!(
        serde_json::from_value::<AttributeValue>(wire).unwrap(),
        value
    );
    for values in [
        None,
        Some(Vec::new()),
        Some(vec![Some(FiniteReal::new(7.0).unwrap()), None]),
    ] {
        let value = AttributeValue::NumericList(AttributeNumericList::Float(values.clone()));
        let wire = serde_json::to_value(&value).unwrap();
        assert_eq!(
            wire["value"]["values"],
            serde_json::to_value(values).unwrap()
        );
        assert_eq!(
            serde_json::from_value::<AttributeValue>(wire).unwrap(),
            value
        );
    }
}

#[test]
fn attribute_references_rewrite_only_local_objects_and_parameter_tokens() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let object = "source:native:object#box";
    let parameter = ParameterId::mint("source:model:parameter#width").unwrap();
    let map = |id: &str| {
        Ok(match id {
            "source:native:object#box" => "edited:graph:node#17".to_owned(),
            "source:model:parameter#width" => "edited:control:value#9".to_owned(),
            _ => panic!("unexpected identity {id}"),
        })
    };
    let local = AttributeValue::SubReference {
        object: object.into(),
        subelements: vec!["Face1".into()],
    };
    assert_eq!(
        identities(&ctx, local, map).unwrap(),
        AttributeValue::SubReference {
            object: "edited:graph:node#17".into(),
            subelements: vec!["Face1".into()]
        }
    );
    let external = AttributeValue::ExternalReference {
        document: object.into(),
        object: object.into(),
        subelements: vec!["Face1".into()],
    };
    assert_eq!(identities(&ctx, external.clone(), map).unwrap(), external);
    let literal = AttributeValue::Map(vec![AttributeMapEntry {
        key: object.into(),
        value: object.into(),
    }]);
    assert_eq!(identities(&ctx, literal.clone(), map).unwrap(), literal);
    let expression = AttributeValue::Expression(AttributeExpression {
        path: "Placement.Base.x".into(),
        expression: Expression {
            tokens: vec![
                ExpressionToken::Text(format!("{object} + ")),
                ExpressionToken::Parameter(parameter),
            ],
        },
        comment: Some(object.into()),
    });
    let rewritten = identities(&ctx, expression, map).unwrap();
    let AttributeValue::Expression(rewritten) = &rewritten else {
        panic!("expression stays typed")
    };
    assert_eq!(rewritten.path, "Placement.Base.x");
    assert_eq!(rewritten.comment.as_deref(), Some(object));
    assert_eq!(
        rewritten.expression.tokens,
        [
            ExpressionToken::Text(format!("{object} + ")),
            ExpressionToken::Parameter(ParameterId::mint("edited:control:value#9").unwrap())
        ]
    );
    let mut references = Vec::new();
    rewritten
        .visit_identity_references(&ctx, &mut |id| {
            references.push(id.to_owned());
            Ok(())
        })
        .unwrap();
    assert_eq!(references, ["edited:control:value#9"]);
}

fn attribute(id: &str, values: Vec<AttributeValue>) -> SourceAttribute {
    SourceAttribute {
        id: AttributeId::mint(id).unwrap(),
        native_ref: None,
        target: AttributeTarget::Document,
        name: "metadata".into(),
        values,
    }
}

#[test]
fn structured_attribute_validation_checks_local_and_parameter_targets() {
    let mut ir = crate::CadIr::empty();
    let target = "test:model:attribute#held";
    ir.model
        .attributes
        .push(attribute(target, vec![AttributeValue::Integer(7)]));
    ir.model.attributes.push(attribute(
        "test:model:attribute#links",
        vec![
            AttributeValue::SubReference {
                object: target.into(),
                subelements: vec!["Face1".into()],
            },
            AttributeValue::ExternalReference {
                document: "other.FCStd".into(),
                object: "NotLocal".into(),
                subelements: Vec::new(),
            },
        ],
    ));
    assert!(crate::validate::validate_neutral(&ir, Vec::new())
        .unwrap()
        .is_ok());
    ir.model.attributes[1]
        .values
        .push(AttributeValue::SubReference {
            object: "test:native:object#missing".into(),
            subelements: Vec::new(),
        });
    ir.model.attributes[1]
        .values
        .push(AttributeValue::Expression(AttributeExpression {
            path: "Placement.Base.x".into(),
            expression: Expression {
                tokens: vec![ExpressionToken::Parameter(
                    ParameterId::mint("test:model:parameter#missing").unwrap(),
                )],
            },
            comment: None,
        }));
    let report = crate::validate::validate_neutral(&ir, Vec::new()).unwrap();
    for missing in ["test:native:object#missing", "test:model:parameter#missing"] {
        assert!(report
            .findings
            .iter()
            .any(
                |finding| finding.entity.as_deref() == Some("test:model:attribute#links")
                    && finding.message.contains(missing)
            ));
    }
}

#[test]
fn numeric_placement_validation_preserves_nonunit_quaternions_and_rejects_zero() {
    let mut ir = crate::CadIr::empty();
    let placement = |quaternion| AttributePlacement {
        position: FiniteVector::new([1.0, 2.0, 3.0]).unwrap(),
        quaternion: FiniteVector::new(quaternion).unwrap(),
    };
    ir.model.attributes.push(attribute(
        "test:model:attribute#frames",
        vec![AttributeValue::NumericList(
            AttributeNumericList::Placement(Some(vec![
                Some(placement([0.0, 0.0, 0.0, 2.0])),
                None,
            ])),
        )],
    ));
    assert!(crate::validate::validate_neutral(&ir, Vec::new())
        .unwrap()
        .is_ok());
    ir.model.attributes[0].values = vec![AttributeValue::NumericList(
        AttributeNumericList::Placement(Some(vec![Some(placement([0.0; 4]))])),
    )];
    let report = crate::validate::validate_neutral(&ir, Vec::new()).unwrap();
    assert!(report
        .findings
        .iter()
        .any(
            |finding| finding.entity.as_deref() == Some("test:model:attribute#frames")
                && finding.message.contains("zero quaternion")
        ));
}

#[test]
fn curvature_numeric_slots_have_exactly_eight_components_and_keep_missing_positions() {
    let record = FiniteVector::new([1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]).unwrap();
    let value = AttributeValue::NumericList(AttributeNumericList::Curvature(Some(vec![
        Some(record),
        None,
    ])));
    let wire = serde_json::to_value(&value).unwrap();
    assert_eq!(
        wire,
        serde_json::json!({"kind":"numeric_list","value":{"kind":"curvature","values":[[1.0,2.0,3.0,4.0,5.0,6.0,7.0,8.0],null]}})
    );
    assert_eq!(
        serde_json::from_value::<AttributeValue>(wire).unwrap(),
        value
    );
    assert!(serde_json::from_value::<AttributeValue>(serde_json::json!({"kind":"numeric_list","value":{"kind":"curvature","values":[[1,2,3,4,5,6,7]]}})).is_err());
}
