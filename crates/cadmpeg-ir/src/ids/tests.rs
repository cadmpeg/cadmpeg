// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::{format_identity, is_valid_identity, IdentityError};

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
