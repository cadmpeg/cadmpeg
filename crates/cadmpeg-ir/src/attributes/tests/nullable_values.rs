// SPDX-License-Identifier: Apache-2.0
//! Readable empty values and given-up slots have distinct wire states.

use super::super::{AttributeJson, AttributeTarget, AttributeValue, SourceAttribute};
use crate::ids::AttributeId;
use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodePolicy};
use serde_json::json;
use std::collections::BTreeMap;

fn attribute(ctx: &DecodeContext<'_>, values: Vec<AttributeValue>) -> SourceAttribute {
    SourceAttribute::new(
        ctx,
        AttributeId::mint("test:model:attribute#value").unwrap(),
        None,
        AttributeTarget::Document,
        "value".into(),
        values,
    )
    .unwrap()
}

#[test]
fn known_values_keep_their_exact_old_wire_and_source_positions() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let known = attribute(
        &ctx,
        vec![
            AttributeValue::Integer(7),
            AttributeValue::String("held".into()),
        ],
    );
    let wire = json!({"id":"test:model:attribute#value","target":{"kind":"document"},
        "name":"value","values":[{"kind":"integer","value":7},{"kind":"string","value":"held"}]});
    assert_eq!(serde_json::to_value(&known).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<SourceAttribute>(wire).unwrap(),
        known
    );
    let mut damaged = known;
    damaged.values.insert(1, None);
    assert_eq!(
        serde_json::to_value(&damaged).unwrap()["values"],
        json!([
            {"kind":"integer","value":7}, null, {"kind":"string","value":"held"}
        ])
    );
    assert!(damaged.values[1].is_none());
    assert_eq!(
        damaged.values[2],
        Some(AttributeValue::String("held".into()))
    );
}

#[test]
fn given_up_slot_empty_link_and_json_null_are_different_values() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut value = attribute(
        &ctx,
        vec![
            AttributeValue::EmptyReference,
            AttributeValue::Json(AttributeJson::Null),
        ],
    );
    value.values.insert(0, None);
    let wire = serde_json::to_value(&value).unwrap();
    assert_eq!(
        wire["values"],
        json!([
            null, {"kind":"empty_reference"}, {"kind":"json","value":{"kind":"null"}}
        ])
    );
    assert_eq!(
        serde_json::from_value::<SourceAttribute>(wire).unwrap(),
        value
    );
    let mut ir = crate::CadIr::empty();
    ir.model.attributes.push(value);
    assert!(!crate::validate::validate_neutral(&ir, Vec::new())
        .unwrap()
        .findings
        .iter()
        .any(|finding| finding.severity == crate::report::Severity::Error));
}

#[test]
fn readable_absent_link_objects_remain_none_without_losing_selectors() {
    let local = AttributeValue::SubReference {
        object: None,
        subelements: vec!["Face1".into()],
    };
    let external = AttributeValue::ExternalReference {
        document: "other.FCStd".into(),
        object: None,
        subelements: Vec::new(),
    };
    assert_eq!(
        serde_json::to_value(&local).unwrap(),
        json!({"kind":"sub_reference","value":{"object":null,"subelements":["Face1"]}})
    );
    assert_eq!(
        serde_json::to_value(&external).unwrap(),
        json!({"kind":"external_reference","value":{"document":"other.FCStd","object":null,"subelements":[]}})
    );
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut visited = Vec::new();
    local
        .visit_identity_references(&ctx, &mut |id| {
            visited.push(id.to_owned());
            Ok(())
        })
        .unwrap();
    assert!(visited.is_empty());
    let mut map = IdentityMap::new(|_: &str| {
        Err(cadmpeg_core::CodecError::malformed(
            "absent or external text is not a local reference",
        ))
    });
    assert_eq!(
        local.clone().rewrite_identities(&ctx, &mut map).unwrap(),
        local
    );
    assert_eq!(
        external.clone().rewrite_identities(&ctx, &mut map).unwrap(),
        external
    );
}

#[test]
fn empty_object_strings_cannot_alias_absent_link_objects() {
    for wire in [
        json!({"kind":"reference","value":""}),
        json!({"kind":"sub_reference","value":{"object":"","subelements":["Face1"]}}),
        json!({"kind":"external_reference","value":{"document":"other.FCStd","object":"","subelements":[]}}),
    ] {
        assert!(serde_json::from_value::<AttributeValue>(wire).is_err());
    }
}

#[test]
fn recursive_json_keeps_all_typed_domains_and_literal_strings() {
    let value = AttributeJson::Object(BTreeMap::from([
        ("a".into(), AttributeJson::Integer(1)),
        (
            "b".into(),
            AttributeJson::Array(vec![
                AttributeJson::Boolean(true),
                AttributeJson::Object(BTreeMap::from([(
                    "c".into(),
                    AttributeJson::Float(crate::scalar::FiniteReal::new(2.5).unwrap()),
                )])),
            ]),
        ),
        ("integer_min".into(), AttributeJson::Integer(i64::MIN)),
        ("unsigned_max".into(), AttributeJson::Unsigned(u64::MAX)),
        (
            "test:native:object#literal".into(),
            AttributeJson::String("test:native:object#literal".into()),
        ),
        ("null".into(), AttributeJson::Null),
    ]));
    let wire = json!({"kind":"object","value":{
        "a":{"kind":"integer","value":1},
        "b":{"kind":"array","value":[{"kind":"boolean","value":true},{"kind":"object","value":{"c":{"kind":"float","value":2.5}}}]},
        "integer_min":{"kind":"integer","value":i64::MIN},
        "unsigned_max":{"kind":"unsigned","value":u64::MAX},
        "test:native:object#literal":{"kind":"string","value":"test:native:object#literal"},
        "null":{"kind":"null"}
    }});
    assert_eq!(serde_json::to_value(&value).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<AttributeJson>(wire).unwrap(),
        value
    );
    let ctx = cadmpeg_test_support::service_decode_context();
    let projected = AttributeValue::Json(value);
    let mut seen = Vec::new();
    projected
        .visit_identity_references(&ctx, &mut |id| {
            seen.push(id.to_owned());
            Ok(())
        })
        .unwrap();
    assert!(seen.is_empty());
    let mut map = IdentityMap::new(|_: &str| {
        Err(cadmpeg_core::CodecError::malformed(
            "JSON text is not an identity",
        ))
    });
    assert_eq!(
        projected
            .clone()
            .rewrite_identities(&ctx, &mut map)
            .unwrap(),
        projected
    );
}

#[test]
fn json_unsigned_is_exactly_the_upper_u64_domain() {
    for value in [i64::MAX.unsigned_abs() + 1, u64::MAX] {
        assert_eq!(
            serde_json::from_value::<AttributeJson>(json!({"kind":"unsigned","value":value}))
                .unwrap(),
            AttributeJson::Unsigned(value)
        );
    }
    for value in [0, i64::MAX.unsigned_abs()] {
        assert!(
            serde_json::from_value::<AttributeJson>(json!({"kind":"unsigned","value":value}))
                .is_err()
        );
    }
    assert!(serde_json::from_str::<AttributeJson>(
        r#"{"kind":"unsigned","value":18446744073709551616}"#
    )
    .is_err());
    assert!(serde_json::from_str::<AttributeJson>(
        r#"{"kind":"integer","value":9223372036854775808}"#
    )
    .is_err());
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut ir = crate::CadIr::empty();
    ir.model.attributes.push(attribute(
        &ctx,
        vec![AttributeValue::Json(AttributeJson::Array(vec![
            AttributeJson::Unsigned(1),
        ]))],
    ));
    let report = crate::validate::validate_neutral(&ir, Vec::new()).unwrap();
    assert_eq!(
        report
            .findings
            .iter()
            .filter(|finding| finding.severity == crate::report::Severity::Error)
            .count(),
        1
    );
    assert!(report.findings.iter().any(|finding| finding.check
        == crate::report::check::Check::PayloadIntegrity
        && finding.entity.as_deref() == Some("test:model:attribute#value")));
}

#[test]
fn recursive_json_rejects_open_shapes_and_nonfinite_values() {
    for wire in [
        json!({"kind":"bytes","value":[0]}),
        json!({"kind":"object","value":{},"extra":0}),
        json!({"kind":"array","value":[{"kind":"string","value":"ok","extra":0}]}),
        json!({"kind":"boolean","value":"true"}),
        json!({"kind":"float","value":null}),
    ] {
        assert!(serde_json::from_value::<AttributeJson>(wire).is_err());
    }
    assert!(serde_json::from_str::<AttributeJson>(r#"{"kind":"float","value":1e400}"#).is_err());
}

#[test]
fn known_slot_constructor_uses_working_memory_without_retained_or_entity_charge() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_entities = 1;
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let value = attribute(
        &ctx,
        vec![AttributeValue::Integer(1), AttributeValue::EmptyReference],
    );
    assert_eq!(
        value.values,
        vec![
            Some(AttributeValue::Integer(1)),
            Some(AttributeValue::EmptyReference)
        ]
    );
    assert_eq!(ctx.retained_bytes_available(), 0);
    ctx.charge_entities(1, "actual output record").unwrap();
    ctx.finish_session().unwrap();
}

#[test]
fn known_slot_constructor_preserves_the_actual_caller_refusal_even_when_empty() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let first = ctx
        .charge_entities(1, "original caller refusal")
        .unwrap_err();
    let refusal = ctx.resource_refusal().unwrap();
    assert!(matches!(first, cadmpeg_core::CodecError::ResourceLimit(error) if error == refusal));
    let result = SourceAttribute::new(
        &ctx,
        AttributeId::mint("test:model:attribute#empty").unwrap(),
        None,
        AttributeTarget::Document,
        "empty".into(),
        Vec::new(),
    );
    assert!(
        matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(error)) if error == refusal)
    );
}

#[cfg(feature = "schema")]
#[test]
fn schema_keeps_nullable_slots_and_exact_recursive_integer_bounds() {
    let source = serde_json::to_value(schemars::schema_for!(SourceAttribute)).unwrap();
    let items = &source["properties"]["values"]["items"];
    assert!(items["anyOf"]
        .as_array()
        .unwrap()
        .iter()
        .any(|branch| branch["type"] == "null"));
    let json = serde_json::to_value(schemars::schema_for!(AttributeJson)).unwrap();
    let branches = json["oneOf"].as_array().unwrap();
    let tags = branches
        .iter()
        .map(|branch| branch["properties"]["kind"]["const"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        tags,
        std::collections::BTreeSet::from([
            "null", "boolean", "integer", "unsigned", "float", "string", "array", "object"
        ])
    );
    let unsigned = branches
        .iter()
        .find(|branch| branch["properties"]["kind"]["const"] == "unsigned")
        .unwrap();
    assert_eq!(
        unsigned["properties"]["value"]["minimum"].as_u64(),
        Some(i64::MAX as u64 + 1)
    );
    assert_eq!(
        unsigned["properties"]["value"]["maximum"].as_u64(),
        Some(u64::MAX)
    );
}

#[test]
fn named_nullable_fields_require_their_one_explicit_null_spelling() {
    for wire in [
        json!({"kind":"sub_reference","value":{"object":null,"subelements":["Face1"]}}),
        json!({"kind":"external_reference","value":{"document":"other.FCStd","object":null,"subelements":[]}}),
    ] {
        let value: AttributeValue = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(value).unwrap(), wire);
        let mut missing = wire;
        missing["value"].as_object_mut().unwrap().remove("object");
        assert!(serde_json::from_value::<AttributeValue>(missing).is_err());
    }
    let wire = json!({"path":"Placement.Base.x","expression":{"tokens":[]},"comment":null});
    let value: super::super::AttributeExpression = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(value.comment, None);
    assert_eq!(serde_json::to_value(value).unwrap(), wire);
    let mut missing = wire;
    missing.as_object_mut().unwrap().remove("comment");
    assert!(serde_json::from_value::<super::super::AttributeExpression>(missing).is_err());
}

#[cfg(feature = "schema")]
#[test]
fn named_nullable_schemas_require_keys_and_admit_exact_nullable_string_domains() {
    use schemars::generate::{Contract, SchemaSettings};
    use std::collections::BTreeSet;

    for contract in [Contract::Deserialize, Contract::Serialize] {
        let mut settings = SchemaSettings::default();
        settings.contract = contract;
        settings.inline_subschemas = true;
        let schema = serde_json::to_value(
            settings
                .clone()
                .into_generator()
                .into_root_schema_for::<AttributeValue>(),
        )
        .unwrap();
        for (kind, required) in [
            ("sub_reference", BTreeSet::from(["object", "subelements"])),
            (
                "external_reference",
                BTreeSet::from(["document", "object", "subelements"]),
            ),
        ] {
            let branch = schema["oneOf"]
                .as_array()
                .unwrap()
                .iter()
                .find(|branch| branch["properties"]["kind"]["const"] == kind)
                .unwrap();
            let content = &branch["properties"]["value"];
            let actual_required = content["required"]
                .as_array()
                .unwrap()
                .iter()
                .map(|name| name.as_str().unwrap())
                .collect::<BTreeSet<_>>();
            assert_eq!(actual_required, required);
            let object = &content["properties"]["object"];
            assert_eq!(
                object["type"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|name| name.as_str().unwrap())
                    .collect::<BTreeSet<_>>(),
                BTreeSet::from(["string", "null"])
            );
            assert_eq!(object["minLength"], 1);
        }
        let expression = serde_json::to_value(
            settings
                .into_generator()
                .into_root_schema_for::<super::super::AttributeExpression>(),
        )
        .unwrap();
        assert_eq!(
            expression["required"]
                .as_array()
                .unwrap()
                .iter()
                .map(|name| name.as_str().unwrap())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["path", "expression", "comment"])
        );
        let comment = &expression["properties"]["comment"];
        assert_eq!(
            comment["type"]
                .as_array()
                .unwrap()
                .iter()
                .map(|name| name.as_str().unwrap())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["string", "null"])
        );
        assert!(comment.get("minLength").is_none());
    }
}
