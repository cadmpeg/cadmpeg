// SPDX-License-Identifier: Apache-2.0
//! Design-history unit tests over synthesized `FCStd` archives.

pub(crate) mod booleans_patterns;
pub(crate) mod construction;
mod history;
pub(crate) mod holes_extrude;
mod ordering;
pub(crate) mod primitives;
mod resource_admission;
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
        {
            let property = crate::native::PropertyRecord {
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
            };
            let _source = crate::test_support::source_fidelity::xml_layout_fixture(
                &property.id,
                &("<Property/>"),
            );
            property
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
    let property = {
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
        let _source = crate::test_support::source_fidelity::xml_layout_fixture(
            &property.id,
            &("<Property/>"),
        );
        property
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
    use crate::native::{ObjectRecord, PropertyBody, PropertyRecord};
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
        .filter(|property| matches!(property.body, Some(PropertyBody::Unreadable(_))))
        .collect::<Vec<_>>();
    let losses = &result.report().losses;
    assert_eq!(
        losses
            .iter()
            .filter(|loss| loss.code.namespace() == "fcstd"
                && loss.code.local_code() == "feature.native-kind-retained"
                && loss.severity == cadmpeg_ir::report::Severity::Blocking)
            .count(),
        expected
    );
    for property in &unreadable {
        assert!(property.values().is_empty());
        assert!(
            !crate::test_support::source_fidelity::bytes(result.source_fidelity(), &property.id)
                .is_empty(),
            "unreadable source XML remains in fidelity"
        );
        let owner = format!("{}/{}", property.owner, property.name);
        assert!(
            losses.iter().any(|loss| loss.code.namespace() == "fcstd"
                && loss.code.local_code() == "metadata.persistence-payload-unresolved"
                && loss.severity == cadmpeg_ir::report::Severity::Warning
                && loss.message.contains(&owner)),
            "unreadable {owner} needs its own loss: {losses:?}"
        );
    }
    assert_eq!(losses.len(), expected + unreadable.len(), "{losses:?}");
    let unrepresented = if unreadable.is_empty() {
        cadmpeg_ir::ArenaClasses::new()
    } else {
        [cadmpeg_ir::ArenaClass::Design].into()
    };
    assert_eq!(result.source_fidelity().unrepresented(), unrepresented);
    assert!(result.ir().scope.design, "partial projection holds design");
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
        .filter(|property| {
            matches!(
                property.body,
                Some(crate::native::PropertyBody::Unreadable(_))
            )
        })
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
