// SPDX-License-Identifier: Apache-2.0
//! Design-history unit tests over synthesized `FCStd` archives.

pub(crate) mod booleans_patterns;
pub(crate) mod construction;
mod history;
pub(crate) mod holes_extrude;
mod ordering;
pub(crate) mod primitives;
mod resource_admission;
mod revolution;
pub(crate) mod sketches;
mod taper;

use cadmpeg_ir::features::FeatureDefinition;

#[test]
fn singular_reference_link_keeps_one_selector_and_rejects_two() {
    let property = |subelements: Vec<String>| {
        let link = crate::native::LinkTarget::optional_from_wire(crate::native::LinkTargetWire {
            document: None,
            document_attribute: None,
            object: Some("source".into()),
            subelements,
        })
        .expect("valid link");
        crate::native::PropertyRecord {
            dynamic: None,
            id: "reference".into(),
            owner: "owner".into(),
            name: "ReferenceAxis".into(),
            type_name: "App::PropertyLinkSub".into(),
            family: crate::native::PropertyFamily::Unknown,
            status: None,
            body: Some(crate::native::PropertyBody::Persisted {
                values: Vec::new(),
                links: vec![link],
                side_entries: Vec::new(),
            }),
            order: 0,
        }
    };
    let one = property(vec!["Edge1".into()]);
    assert!(matches!(
        super::singular_reference_link(&one),
        Some((_, Some("Edge1")))
    ));
    let two = property(vec!["Edge1 Edge2".into()]);
    assert!(super::singular_reference_link(&two).is_none());
}

#[test]
fn sketch_placement_runtime_type_error_is_malformed() {
    let property = crate::native::PropertyRecord {
        dynamic: None,
        id: "placement-property".into(),
        owner: "sketch".into(),
        name: "Placement".into(),
        type_name: "App::PropertyString".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: Some(crate::native::PropertyBody::Transient),
        order: 0,
    };
    let expected = "sketch Placement placement carrier has runtime type App::PropertyString";
    assert!(matches!(super::validate_sketch_placement(&[&property]),
        Err(cadmpeg_core::CodecError::Malformed(message)) if message == expected));
    assert!(matches!(super::sketch_frame(&[&property]),
        Err(cadmpeg_core::CodecError::Malformed(message)) if message == expected));
}

/// Feature definition selected by exact feature name.
fn definition<'a>(
    result: &'a cadmpeg_ir::codec::DecodeResult,
    name: &str,
) -> &'a FeatureDefinition {
    result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("missing {name}"))
        .evaluation
        .definition()
}

/// Definition of the single `Extrusion` feature.
fn extrusion_definition(result: &cadmpeg_ir::codec::DecodeResult) -> &FeatureDefinition {
    result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Extrusion"))
        .expect("extrusion feature")
        .evaluation
        .definition()
}

/// Native feature fallback retains typed records; unreadable payloads also
/// charge their own Warning and explicitly block design patch/replay.
fn assert_native_retention(result: &cadmpeg_ir::codec::DecodeResult, expected: usize) {
    assert_native_retention_as(result, expected, "feature.native-kind-retained", "", 0);
}

/// Native feature fallback that also charges `extra` losses of one known code
/// beyond the native-kind and unreadable sets.
fn assert_native_retention_with_known_losses(
    result: &cadmpeg_ir::codec::DecodeResult,
    expected: usize,
    extra_code: &str,
    extra: usize,
) {
    assert_native_retention_as(
        result,
        expected,
        "feature.native-kind-retained",
        extra_code,
        extra,
    );
}

/// Untransferred revolution fallback: the same retention checks with the
/// revolution-specific loss in place of the native-kind loss.
fn assert_native_revolution_retention(result: &cadmpeg_ir::codec::DecodeResult, expected: usize) {
    assert_native_retention_as(
        result,
        expected,
        "feature.revolution-not-transferred",
        "",
        0,
    );
}

fn assert_native_retention_as(
    result: &cadmpeg_ir::codec::DecodeResult,
    expected: usize,
    native_code: &str,
    extra_code: &str,
    extra: usize,
) {
    use crate::native::{ObjectRecord, PropertyRecord};
    use cadmpeg_ir::features::FeatureOperation;
    let namespace = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("native namespace");
    let objects = namespace
        .arena_as::<ObjectRecord>("objects")
        .expect("typed objects");
    let properties = namespace
        .arena_as::<PropertyRecord>("properties")
        .expect("typed properties");
    let native = result
        .ir()
        .model
        .features
        .iter()
        .filter(|feature| {
            matches!(
                feature.evaluation.definition().operation(),
                FeatureOperation::Native { .. }
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(native.len(), expected);
    for feature in native {
        let owner = feature.native_ref.as_ref().expect("native feature object");
        let object = objects
            .iter()
            .find(|object| object.id().as_str() == owner.as_str())
            .expect("retained typed native object");
        assert!(
            matches!(feature.evaluation.definition().operation(), FeatureOperation::Native { kind, .. } if kind.as_str() == object.type_name)
        );
    }
    let unreadable = properties
        .iter()
        .filter(|property| property.body.is_none())
        .collect::<Vec<_>>();
    let losses = &result.report().losses;
    assert_eq!(
        losses
            .iter()
            .filter(|loss| loss.code.namespace() == "fcstd"
                && loss.code.local_code() == native_code
                && loss.severity == cadmpeg_ir::report::Severity::Blocking)
            .count(),
        expected
    );
    for property in &unreadable {
        assert!(property.values().is_empty());
        let owner = format!("{}/{}", property.owner, property.name);
        let loss = losses
            .iter()
            .find(|loss| {
                loss.code.namespace() == "decode"
                    && loss.code.local_code() == "record.unreadable"
                    && loss.severity == cadmpeg_ir::report::Severity::Error
                    && loss.message.contains("part freecad.property_value")
                    && loss.message.contains(&owner)
            })
            .unwrap_or_else(|| panic!("unreadable {owner} needs its own loss: {losses:?}"));
        let offset = loss
            .message
            .split("offset=")
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
            .map(|token| token.trim_end_matches(|cell: char| !cell.is_ascii_digit()))
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or_else(|| panic!("unreadable {owner} loss names its part offset"));
        let length = loss
            .message
            .split("length=")
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
            .map(|token| token.trim_end_matches(|cell: char| !cell.is_ascii_digit()))
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or_else(|| panic!("unreadable {owner} loss names its part length"));
        let (id, _) = crate::test_support::source_fidelity::given_up(result.source_fidelity())
            .into_iter()
            .find(|(_, record)| record.offset() == offset && record.byte_len() == length)
            .unwrap_or_else(|| panic!("unreadable {owner} part is retained"));
        assert!(
            !crate::test_support::source_fidelity::bytes(result.source_fidelity(), id).is_empty(),
            "unreadable source XML remains in fidelity"
        );
    }
    assert_eq!(
        losses
            .iter()
            .filter(|loss| loss.code.namespace() == "fcstd" && loss.code.local_code() == extra_code)
            .count(),
        extra,
        "{losses:?}"
    );
    let without_model = result.ir().scope
        == (cadmpeg_ir::document::ContentScope {
            model: false,
            design: true,
        });
    assert_eq!(
        losses.len(),
        expected + unreadable.len() + extra + usize::from(without_model),
        "{losses:?}"
    );
    if without_model {
        // #209 §1.1: the fragment locates design parts but no model part,
        // so the wrapper charges depth.not-delivered for the model class.
        assert_eq!(
            losses
                .iter()
                .filter(|loss| loss.code.namespace() == "decode"
                    && loss.code.local_code() == "depth.not-delivered")
                .count(),
            1,
            "{losses:?}"
        );
    }
    let unrepresented = if unreadable.is_empty() {
        cadmpeg_ir::ArenaClasses::new()
    } else {
        [cadmpeg_ir::ArenaClass::Design].into()
    };
    assert_eq!(result.source_fidelity().unrepresented(), unrepresented);
    assert!(result.ir().scope.design, "partial projection holds design");
    assert_eq!(
        result.ir().scope.model,
        !without_model,
        "scope holds model exactly when no class is missing"
    );
}

fn assert_unreadable_properties(
    result: &cadmpeg_ir::codec::DecodeResult,
    expected: &[(&str, &str)],
) {
    let properties = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("namespace")
        .arena_as::<crate::native::PropertyRecord>("properties")
        .expect("typed properties");
    let actual = properties
        .iter()
        .filter(|property| property.body.is_none())
        .map(|property| (property.owner.clone(), property.name.clone()))
        .collect::<std::collections::BTreeSet<_>>();
    let expected: std::collections::BTreeSet<_> = expected
        .iter()
        .map(|(object, property)| {
            (
                crate::native::native_id("object", object),
                (*property).to_owned(),
            )
        })
        .collect();
    assert_eq!(actual, expected);
}

#[test]
fn external_geometry_keys_require_one_selector_per_link() {
    for selectors in [vec![], vec!["Edge1".to_owned(), "Edge2".to_owned()]] {
        let reference =
            crate::native::LinkTarget::optional_from_wire(crate::native::LinkTargetWire {
                document: None,
                document_attribute: None,
                object: Some("Source".into()),
                subelements: selectors,
            })
            .unwrap()
            .unwrap();
        assert!(super::external_link_key(&reference).is_err());
    }
    for selector in ["Edge1", "Edge2", ""] {
        let reference =
            crate::native::LinkTarget::optional_from_wire(crate::native::LinkTargetWire {
                document: None,
                document_attribute: None,
                object: Some("Source".into()),
                subelements: vec![selector.to_owned()],
            })
            .unwrap()
            .unwrap();
        assert_eq!(
            super::external_link_key(&reference).unwrap(),
            Some(format!("Source.{selector}"))
        );
    }
}
