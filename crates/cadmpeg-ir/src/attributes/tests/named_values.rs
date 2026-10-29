// SPDX-License-Identifier: Apache-2.0
//! Named structured attribute contracts.

use super::super::{AttributeExpression, AttributeMapEntry, AttributeValue};
use crate::features::{Expression, ExpressionToken, ParameterId};
use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
use cadmpeg_core::decode::{DecodeArena, DecodeContext};
use serde_json::json;

#[test]
fn map_preserves_decoded_text_duplicate_keys_and_source_order() {
    let wire = json!({"kind":"map", "value":[
        {"key":"a", "value":"1"},
        {"key":"b", "value":"2"},
        {"key":"a", "value":"A&B"}
    ]});
    let value: AttributeValue = serde_json::from_value(wire.clone()).expect("named map");
    assert_eq!(
        value,
        AttributeValue::Map(vec![
            AttributeMapEntry {
                key: "a".into(),
                value: "1".into()
            },
            AttributeMapEntry {
                key: "b".into(),
                value: "2".into()
            },
            AttributeMapEntry {
                key: "a".into(),
                value: "A&B".into()
            },
        ])
    );
    assert_eq!(serde_json::to_value(value).expect("map wire"), wire);
}

#[test]
fn expression_preserves_path_tokens_and_absent_comment() {
    let parameter = ParameterId::mint("test:model:parameter#Width").expect("parameter identity");
    let value = AttributeValue::Expression(AttributeExpression {
        path: "Placement.Base.x".into(),
        expression: Expression {
            tokens: vec![
                ExpressionToken::Parameter(parameter),
                ExpressionToken::Text(" + 2 mm".into()),
            ],
        },
        comment: None,
    });
    let wire = json!({"kind":"expression","value":{
        "path":"Placement.Base.x", "expression":{"tokens":[
            {"kind":"parameter","value":"test:model:parameter#Width"},
            {"kind":"text","value":" + 2 mm"}
        ]}, "comment":null
    }});
    assert_eq!(serde_json::to_value(&value).expect("expression wire"), wire);
    assert_eq!(
        serde_json::from_value::<AttributeValue>(wire).expect("expression"),
        value
    );
}

#[test]
fn local_and_external_references_keep_their_distinct_named_fields() {
    for wire in [
        json!({"kind":"sub_reference","value":{
            "object":"test:native:object#Box", "subelements":["Face1","Edge2"]
        }}),
        json!({"kind":"external_reference","value":{
            "document":"other.FCStd", "object":"Box", "subelements":["Face1"]
        }}),
    ] {
        let value: AttributeValue = serde_json::from_value(wire.clone()).expect("named reference");
        assert_eq!(serde_json::to_value(value).expect("reference wire"), wire);
    }
}

#[test]
fn named_forms_reject_unknown_fields_and_missing_required_reference_fields() {
    for wire in [
        json!({"kind":"map","value":[{"key":"a","value":"1","extra":1}]}),
        json!({"kind":"expression","value":{
            "path":"Placement.Base.x","expression":{"tokens":[]},"comment":null,"extra":1
        }}),
        json!({"kind":"sub_reference","value":{"object":"test:native:object#Box"}}),
        json!({"kind":"external_reference","value":{"object":"Box","subelements":[]}}),
    ] {
        assert!(serde_json::from_value::<AttributeValue>(wire).is_err());
    }
}

#[test]
fn identity_walk_and_rewrite_distinguish_local_targets_and_external_names() {
    let arena = DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let ctx = DecodeContext::new(&arena, &policy, cadmpeg_core::decode::DecodeDepth::Full);
    let local = AttributeValue::SubReference {
        object: Some("test:native:object#Box".into()),
        subelements: vec!["Face1".into()],
    };
    let external = AttributeValue::ExternalReference {
        document: "other.FCStd".into(),
        object: Some("Box".into()),
        subelements: vec!["Face1".into()],
    };
    let expression = AttributeValue::Expression(AttributeExpression {
        path: "Placement.Base.x".into(),
        expression: Expression {
            tokens: vec![ExpressionToken::Parameter(
                ParameterId::mint("test:model:parameter#Width").expect("parameter identity"),
            )],
        },
        comment: Some("source comment".into()),
    });
    let mut visited = Vec::new();
    for value in [&local, &external, &expression] {
        value
            .visit_identity_references(&ctx, &mut |id| {
                visited.push(id.to_owned());
                Ok(())
            })
            .expect("walk actual references");
    }
    assert_eq!(
        visited,
        ["test:native:object#Box", "test:model:parameter#Width"]
    );
    let mut map = IdentityMap::new(|id: &str| {
        Ok(id
            .replace("#Box", "#EditedBox")
            .replace("#Width", "#EditedWidth"))
    });
    assert_eq!(
        local
            .rewrite_identities(&ctx, &mut map)
            .expect("local rewrite"),
        AttributeValue::SubReference {
            object: Some("test:native:object#EditedBox".into()),
            subelements: vec!["Face1".into()]
        }
    );
    assert_eq!(
        external
            .clone()
            .rewrite_identities(&ctx, &mut map)
            .expect("external literal"),
        external
    );
    let AttributeValue::Expression(rewritten) = expression
        .rewrite_identities(&ctx, &mut map)
        .expect("parameter rewrite")
    else {
        panic!("expression variant")
    };
    assert_eq!(rewritten.path, "Placement.Base.x");
    assert_eq!(rewritten.comment.as_deref(), Some("source comment"));
    assert_eq!(
        rewritten.expression.tokens,
        [ExpressionToken::Parameter(
            ParameterId::mint("test:model:parameter#EditedWidth").expect("rewritten parameter")
        )]
    );
}

#[test]
fn deleting_a_named_reference_requires_dropping_its_value() {
    use crate::schema::rewrite::typed::prune::{analyze, ReferencePruneEffect};
    use std::collections::BTreeSet;
    let arena = DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let ctx = DecodeContext::new(&arena, &policy, cadmpeg_core::decode::DecodeDepth::Full);
    let local = AttributeValue::SubReference {
        object: Some("test:native:object#Box".into()),
        subelements: vec!["Face1".into()],
    };
    assert_eq!(
        analyze(
            &local,
            &ctx,
            &BTreeSet::from(["test:native:object#Box".into()])
        )
        .expect("analyze required local reference"),
        ReferencePruneEffect::DropRequired
    );
    let expression = AttributeValue::Expression(AttributeExpression {
        path: "Placement.Base.x".into(),
        comment: None,
        expression: Expression {
            tokens: vec![ExpressionToken::Parameter(
                ParameterId::mint("test:model:parameter#Width").expect("parameter identity"),
            )],
        },
    });
    assert_eq!(
        analyze(
            &expression,
            &ctx,
            &BTreeSet::from(["test:model:parameter#Width".into()])
        )
        .expect("analyze required expression reference"),
        ReferencePruneEffect::DropRequired
    );
}

#[cfg(feature = "schema")]
#[test]
fn schema_contains_all_four_named_forms_and_structured_definitions() {
    use std::collections::BTreeSet;
    let schema = serde_json::to_value(schemars::schema_for!(AttributeValue)).expect("schema JSON");
    let branches = schema["oneOf"].as_array().expect("tagged value schema");
    let tags = branches
        .iter()
        .map(|branch| {
            let tag = &branch["properties"]["kind"];
            tag.get("const")
                .and_then(serde_json::Value::as_str)
                .or_else(|| {
                    tag.get("enum")
                        .and_then(serde_json::Value::as_array)
                        .filter(|values| values.len() == 1)
                        .and_then(|values| values[0].as_str())
                })
                .expect("one explicit kind per branch")
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        tags,
        BTreeSet::from([
            "integer",
            "float",
            "string",
            "boolean",
            "reference",
            "vector",
            "map",
            "expression",
            "sub_reference",
            "external_reference",
            "empty_reference",
            "json",
        ])
    );
    for (name, expected_fields) in [
        ("AttributeMapEntry", BTreeSet::from(["key", "value"])),
        (
            "AttributeExpression",
            BTreeSet::from(["path", "expression", "comment"]),
        ),
    ] {
        let definition = &schema["$defs"][name];
        assert_eq!(definition["additionalProperties"], false);
        let fields = definition["properties"]
            .as_object()
            .expect("named structured fields")
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        assert_eq!(fields, expected_fields);
    }
}
