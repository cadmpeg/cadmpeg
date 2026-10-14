// SPDX-License-Identifier: Apache-2.0
//! Design-history unit tests over synthesized `FCStd` archives.

pub(crate) mod booleans_patterns;
pub(crate) mod construction;
mod history;
pub(crate) mod holes_extrude;
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
        crate::native::PropertyRecord {
            id: "reference".into(),
            owner: "owner".into(),
            name: "ReferenceAxis".into(),
            type_name: "App::PropertyLinkSub".into(),
            family: crate::native::PropertyFamily::Unknown,
            status: None,
            body: crate::native::PropertyBody::Persisted {
                values: Vec::new(),
                links: vec![link],
                side_entries: Vec::new(),
                dynamic: None,
            },
            order: 0,
            xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0)
                .expect("valid XML span"),
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
        id: "placement-property".into(),
        owner: "sketch".into(),
        name: "Placement".into(),
        type_name: "App::PropertyString".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Transient,
        order: 0,
        xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0)
            .expect("valid XML span"),
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
