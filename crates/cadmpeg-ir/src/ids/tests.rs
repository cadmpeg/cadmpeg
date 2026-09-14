// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::{
    format_identity, is_valid_identity, IdentityComponent, IdentityError, IdentityKey,
    IdentityNamespace,
};

#[test]
fn kind_replacement_preserves_admitted_namespace_and_key() {
    for key in ["0", "owner:child", "é:部", ":"] {
        let source = super::Identity::new(format!("catia:graph:object#{key}")).unwrap();
        let feature = source.with_kind(&crate::identity_component!("feature"));
        assert_eq!(feature.as_str(), format!("catia:graph:feature#{key}"));
        assert_eq!(source.as_str(), format!("catia:graph:object#{key}"));
        assert!(is_valid_identity(feature.as_str()));
    }
}

#[test]
fn three_component_ids_are_valid() {
    assert!(is_valid_identity("step:file:signature#0"));
    assert!(format_identity("step", "file", "signature", 0u8).is_ok());
}

#[test]
fn two_component_ids_are_rejected() {
    assert!(!is_valid_identity("step:signature#0"));
    assert!(matches!(
        format_identity("step", "", "signature", 0u8),
        Err(IdentityError::InvalidComponent { label: "scope", .. })
    ));
}

#[test]
fn typed_ids_keep_their_canonical_json_string_shape() {
    let id = crate::ids::BodyId::mint("test:model:body#1").unwrap();
    assert_eq!(serde_json::to_string(&id).unwrap(), "\"test:model:body#1\"");
    assert_eq!(
        serde_json::from_str::<crate::ids::BodyId>("\"test:model:body#1\"").unwrap(),
        id
    );
}

#[test]
fn typed_ids_reject_malformed_grammar() {
    assert!(crate::ids::BodyId::mint("synthetic:scope:point").is_err());
    assert!(serde_json::from_str::<crate::ids::BodyId>("\"synthetic:scope:point\"").is_err());
}

#[test]
fn fallible_conversions_enforce_entity_identity_grammar() {
    use crate::ids::BodyId;

    for invalid in [
        "",
        "body",
        "test:body#1",
        "test::body#1",
        "test:model:body#",
        "test:model:body#a#b",
        "test:model:body#a b",
    ] {
        assert!(BodyId::try_from(invalid).is_err(), "{invalid:?}");
        assert!(BodyId::try_from(invalid.to_owned()).is_err(), "{invalid:?}");
        let wire = serde_json::to_string(invalid).unwrap();
        assert!(
            serde_json::from_str::<BodyId>(&wire).is_err(),
            "{invalid:?}"
        );
    }
    let value = "test:model:body#1";
    let id = BodyId::try_from(value.to_owned()).unwrap();
    assert_eq!(id.as_str(), value);
    assert_eq!(id.into_string(), value);
}

#[test]
fn local_identity_conversions_reject_empty_or_whitespace_keys() {
    use crate::ids::HistoricalBodyId;

    for invalid in ["", " ", "a b", "a\tb", "a\nb"] {
        assert!(HistoricalBodyId::try_from(invalid).is_err(), "{invalid:?}");
        assert!(
            HistoricalBodyId::try_from(invalid.to_owned()).is_err(),
            "{invalid:?}"
        );
        let wire = serde_json::to_string(invalid).unwrap();
        assert!(
            serde_json::from_str::<HistoricalBodyId>(&wire).is_err(),
            "{invalid:?}"
        );
    }
    let id = HistoricalBodyId::try_from("member-1").unwrap();
    assert_eq!(id.as_str(), "member-1");
    assert_eq!(serde_json::to_string(&id).unwrap(), "\"member-1\"");
    assert_eq!(id.into_string(), "member-1");
}

#[test]
fn arena_identity_types_enforce_the_entity_identity_grammar() {
    macro_rules! check {
        ($type:ty, $valid:literal) => {{
            for invalid in [
                "",
                " ",
                "a b",
                "a\tb",
                "a\nb",
                "x",
                "test:entity#0",
                "test::entity#0",
                "test:model:entity#",
                "test:model:entity#a#b",
                "test:model:extra:entity#0",
            ] {
                assert!(<$type>::mint(invalid).is_err(), "{invalid:?}");
                assert!(<$type>::try_from(invalid).is_err(), "{invalid:?}");
                assert!(
                    <$type>::try_from(invalid.to_owned()).is_err(),
                    "{invalid:?}"
                );
                let wire = serde_json::to_string(invalid).unwrap();
                assert!(serde_json::from_str::<$type>(&wire).is_err(), "{invalid:?}");
            }
            let id = <$type>::mint($valid).expect("identity grammar");
            assert_eq!(
                serde_json::from_str::<$type>(&serde_json::to_string($valid).unwrap()).unwrap(),
                id
            );
            assert_eq!(id.as_str(), $valid);
            assert_eq!(
                serde_json::to_string(&id).unwrap(),
                serde_json::to_string($valid).unwrap()
            );
            assert_eq!(id.into_string(), $valid);
        }};
    }

    check!(crate::features::FeatureId, "f3d:model:feature#fillet");
    check!(
        crate::features::ConfigurationId,
        "sldprt:model:configuration#0"
    );
    check!(crate::features::ParameterId, "f3d:model:parameter#width");
    check!(crate::assets::AssetId, "f3d:model:asset#preview");
    check!(crate::products::JointId, "f3d:model:joint#root");
    check!(
        crate::spreadsheets::SpreadsheetId,
        "fcstd:design:spreadsheet#Sheet"
    );
    check!(
        crate::presentation::PresentationId,
        "fcstd:presentation:document#0"
    );
    check!(
        crate::semantic_annotations::SemanticAnnotationId,
        "rhino:dimension:annotation#4"
    );
    check!(crate::drawings::DrawingId, "fcstd:drawing:page#Page");
}

#[test]
fn checked_identity_admission_and_typed_conversion() {
    use super::{BodyId, Identity};
    let text = "test:model:body#1";
    let identity = format_identity("test", "model", "body", 1).unwrap();
    assert_eq!(identity.as_str(), text);
    assert_eq!(
        serde_json::to_string(&identity).unwrap(),
        "\"test:model:body#1\""
    );
    assert_eq!(
        serde_json::from_str::<Identity>("\"test:model:body#1\"").unwrap(),
        identity
    );
    assert_eq!(BodyId::from(identity).as_str(), text);
    for invalid in [
        "",
        "test:body#1",
        "test::body#1",
        "test:model:body#",
        "test:model:body#a#b",
        "test:model:body#a b",
    ] {
        assert!(Identity::new(invalid).is_err());
        assert!(
            serde_json::from_str::<Identity>(&serde_json::to_string(invalid).unwrap()).is_err()
        );
    }
}

#[test]
fn typed_namespace_composition_preserves_the_wire_identity() {
    let namespace = crate::identity_namespace!("step", "file", "signature");
    let identity = super::Identity::compose(&namespace, 7u64);
    assert_eq!(identity.as_str(), "step:file:signature#7");

    let body = crate::ids::BodyId::compose(&namespace, IdentityKey::from(7u64));
    assert_eq!(body.as_str(), "step:file:signature#7");
    assert_eq!(namespace.format(), "step");
    assert_eq!(namespace.scope(), "file");
    assert_eq!(namespace.kind(), "signature");
}

#[test]
// Standard formatting is an independent oracle for the complete byte alphabet.
#[allow(clippy::format_collect)]
fn hexadecimal_identity_keys_encode_every_byte_without_collisions() {
    let bytes = (u8::MIN..=u8::MAX).collect::<Vec<_>>();
    let mut distinct = std::collections::HashSet::new();
    for &byte in &bytes {
        let key = IdentityKey::hex_byte(byte);
        assert_eq!(key.as_str(), format!("{byte:02x}"));
        assert!(distinct.insert(key));
    }
    let prefix = crate::identity_key!("source-");
    assert_eq!(prefix.clone().with_hex_bytes(&[]), prefix);
    let encoded = prefix.with_hex_bytes(&bytes);
    let expected = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(encoded.as_str(), format!("source-{expected}"));
    assert_eq!(
        crate::identity_key!("source-")
            .with_hex_bytes("é:# \n".as_bytes())
            .as_str(),
        "source-c3a93a23200a"
    );
}

#[test]
fn identity_key_projection_and_tails_preserve_namespace_and_key_separators() {
    let namespace = crate::identity_namespace!("format", "scope", "kind");
    for key in ["key", ":", "left:right", "é:端"] {
        let key = IdentityKey::try_new(key).unwrap();
        let identity = super::Identity::compose(&namespace, key.clone());
        assert_eq!(identity.key(), key);
        let curve = crate::ids::CurveId::from(identity.clone());
        assert_eq!(curve.key(), key);
        assert_eq!(
            identity
                .clone()
                .with_key_tail(&super::IdentityKeyTail::empty()),
            identity
        );
        let extended = identity.with_key_tail(
            &super::IdentityKeyTail::empty().dash(crate::identity_key!("construction")),
        );
        assert_eq!(
            extended.as_str(),
            format!("format:scope:kind#{key}-construction")
        );
        assert_eq!(extended.key().as_str(), format!("{key}-construction"));
        assert!(is_valid_identity(extended.as_str()));
    }
}

#[test]
fn signed_record_identity_keys_preserve_decimal_text_at_the_integer_bounds() {
    let namespace = crate::identity_namespace!("f3d", "brep", "attribute");
    for (value, text) in [
        (i64::MIN, "-9223372036854775808"),
        (-1, "-1"),
        (0, "0"),
        (1, "1"),
        (i64::MAX, "9223372036854775807"),
    ] {
        let key = IdentityKey::from(value);
        assert_eq!(key.as_str(), text);
        assert_eq!(IdentityKey::from(&value), key);
        assert_eq!(IdentityKey::from(&&value), key);
        let identity = super::Identity::compose(&namespace, key);
        assert_eq!(identity.as_str(), format!("f3d:brep:attribute#{text}"));
        assert!(is_valid_identity(identity.as_str()));
    }
    assert_eq!(
        IdentityKey::from(i128::MIN).as_str(),
        "-170141183460469231731687303715884105728"
    );
    assert_eq!(
        IdentityKey::from(i128::MAX).as_str(),
        "170141183460469231731687303715884105727"
    );
}

#[test]
fn runtime_namespace_and_key_admission_reports_the_rejected_value() {
    assert!(matches!(
        IdentityNamespace::new("step", "bad scope", "signature"),
        Err(IdentityError::InvalidComponent { label: "scope", .. })
    ));
    assert!(matches!(
        IdentityKey::try_from("a b"),
        Err(IdentityError::InvalidKey { .. })
    ));
    assert!(matches!(
        IdentityKey::try_from("a#b"),
        Err(IdentityError::InvalidKey { .. })
    ));
    assert!(matches!(
        IdentityKey::try_from("a\u{00a0}b"),
        Err(IdentityError::InvalidKey { .. })
    ));
    assert!(matches!(
        IdentityComponent::try_from("a:b"),
        Err(IdentityError::InvalidComponent {
            label: "component",
            ..
        })
    ));
}

#[test]
fn literal_helpers_reject_unicode_whitespace_without_a_runtime_panic() {
    assert!(super::StaticIdentityKey::new("a\u{2003}b").is_none());
    assert!(super::StaticIdentityComponent::new("a\u{3000}b").is_none());
    assert!(super::StaticIdentityNamespace::new("step", "file", "signature").is_some());
    assert!(super::StaticIdentityNamespace::new("step", "bad#scope", "signature").is_none());
}

#[test]
fn const_whitespace_grammar_matches_runtime_identity_grammar_for_every_scalar() {
    let mut component = String::with_capacity(8);
    let mut identity = String::with_capacity(16);
    let mut key = String::with_capacity(8);

    for scalar in 0..=0x10_ffff {
        let Some(character) = char::from_u32(scalar) else {
            continue;
        };
        let is_whitespace = character.is_whitespace();

        component.clear();
        component.push('a');
        component.push(character);
        component.push('b');
        assert_eq!(
            super::contains_unicode_whitespace(&component),
            is_whitespace,
            "const whitespace parser disagrees for U+{scalar:04X}"
        );

        identity.clear();
        identity.push('a');
        identity.push(character);
        identity.push_str("b:c:d#key");
        assert_eq!(
            super::valid_component_text(&component),
            super::is_valid_identity(&identity),
            "namespace grammar disagrees with identity grammar for U+{scalar:04X}"
        );

        key.clear();
        key.push('k');
        key.push(character);
        key.push('z');
        identity.clear();
        identity.push_str("a:b:c#");
        identity.push_str(&key);
        assert_eq!(
            super::valid_key_text(&key),
            super::is_valid_identity(&identity),
            "key grammar disagrees with identity grammar for U+{scalar:04X}"
        );
    }
}
