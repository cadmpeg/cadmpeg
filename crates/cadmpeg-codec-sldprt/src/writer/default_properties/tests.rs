// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::AttributeId;

use super::{edited_properties, is_property, requires_guard, same_attribute};
use crate::test_support::container::{make_block, outer_header, scan};

fn property(id: &str, name: &str, value: AttributeValue) -> SourceAttribute {
    SourceAttribute {
        id: AttributeId::mint(id).unwrap(),
        target: AttributeTarget::Document.into(),
        name: name.into(),
        values: vec![Some(value)],
        native_ref: None,
    }
}

#[test]
fn configuration_carrier_facts_are_guarded_even_when_removed_or_forged() {
    let mut bytes = outer_header();
    bytes.extend(make_block(
        0x43,
        "docProps/Config-3-Properties.xml",
        b"opaque",
    ));
    let source_scan = scan(&bytes);
    let mut original = CadIr::empty();
    assert!(requires_guard(&original, Some(&source_scan)));
    let row = property(
        "sldprt:metadata:configuration_property#8:3",
        "saved_configuration_property",
        AttributeValue::String("stored".into()),
    );
    let mut forged = original.clone();
    forged.model.attributes.push(row.clone());
    assert!(requires_guard(&forged, None));
    assert!(!edited_properties(&forged, &original, &source_scan).is_empty());
    original.model.attributes.push(row);
    assert!(edited_properties(&original, &original, &source_scan).is_empty());
    let mut removed = original.clone();
    removed.model.attributes.clear();
    assert!(!edited_properties(&removed, &original, &source_scan).is_empty());
    let mut changed = original.clone();
    changed.model.attributes[0].values.clear();
    assert!(!edited_properties(&changed, &original, &source_scan).is_empty());
}

#[test]
fn owned_properties_keep_names_values_and_exact_float_bits() {
    let mut original = property(
        "sldprt:metadata:default_property#8:5:SW-Short%20Date",
        "SW-Short Date",
        AttributeValue::String(String::new()),
    );
    assert!(same_attribute(&original, &original));
    let mut renamed = original.clone();
    renamed.name = "SW-Short Date(Short Date)".into();
    assert!(!same_attribute(&original, &renamed));
    renamed = original.clone();
    renamed.values = vec![Some(AttributeValue::String("saved".into()))];
    assert!(!same_attribute(&original, &renamed));
    for vector in [false, true] {
        original.values = vec![if vector {
            Some(AttributeValue::vector([-0.0]).unwrap())
        } else {
            Some(AttributeValue::float(-0.0).unwrap())
        }];
        let mut positive = original.clone();
        positive.values = vec![if vector {
            Some(AttributeValue::vector([0.0]).unwrap())
        } else {
            Some(AttributeValue::float(0.0).unwrap())
        }];
        assert!(!same_attribute(&original, &positive));
    }
}

#[test]
fn empty_saved_property_remains_read_only_and_distinct_from_a_blank_string() {
    let bytes = outer_header();
    let source_scan = scan(&bytes);
    let mut original = CadIr::empty();
    let mut row = property(
        "sldprt:metadata:default_property#8:5:Empty",
        "Empty",
        AttributeValue::String(String::new()),
    );
    row.values.clear();
    original.model.attributes.push(row);
    assert!(edited_properties(&original, &original, &source_scan).is_empty());
    assert!(requires_guard(&original, None));
    let mut changed = original.clone();
    changed.model.attributes[0]
        .values
        .push(Some(AttributeValue::String(String::new())));
    assert!(!edited_properties(&changed, &original, &source_scan).is_empty());
    changed = original.clone();
    changed.model.attributes[0].name = "Other".into();
    assert!(!edited_properties(&changed, &original, &source_scan).is_empty());
    changed = original.clone();
    changed.model.attributes.clear();
    assert!(!edited_properties(&changed, &original, &source_scan).is_empty());
}

#[test]
fn retained_guard_detects_removed_added_renamed_and_reidentified_properties() {
    let bytes = outer_header();
    let source_scan = scan(&bytes);
    let mut original = CadIr::empty();
    original.model.attributes.push(property(
        "sldprt:metadata:default_property#8:5:Name",
        "Name",
        AttributeValue::String("saved".into()),
    ));
    assert!(edited_properties(&original, &original, &source_scan).is_empty());
    assert!(requires_guard(&original, None));
    for mutation in 0..6 {
        let mut current = original.clone();
        match mutation {
            0 => current.model.attributes.clear(),
            1 => current.model.attributes.push(property(
                "sldprt:metadata:default_property#8:20:Other",
                "Other",
                AttributeValue::Integer(2),
            )),
            2 => current.model.attributes[0].name = "Other".into(),
            3 => {
                current.model.attributes[0].id =
                    AttributeId::mint("other:metadata:default_property#8:5:Name").unwrap();
            }
            4 => current.model.attributes[0].values = vec![Some(AttributeValue::Integer(5))],
            5 => current
                .model
                .attributes
                .push(current.model.attributes[0].clone()),
            _ => unreachable!(),
        }
        assert!(!edited_properties(&current, &original, &source_scan).is_empty());
    }
}

#[test]
fn only_exact_default_carrier_owns_the_unit_mirror_guard() {
    for (name, owned) in [
        ("docProps/ISolidWorksInformation.xml", true),
        ("other/docProps/ISolidWorksInformation.xml", false),
        ("docProps/custom.xml", false),
        ("docProps/Config-0-Properties.xml", false),
    ] {
        let mut bytes = outer_header();
        bytes.extend(make_block(0x42, name, b"<authored/>"));
        let source_scan = scan(&bytes);
        let ordinal = source_scan.blocks[0].offset;
        let mut original = CadIr::empty();
        original.model.attributes.push(property(
            &format!("sldprt:metadata:source_linear_unit_code#{ordinal}:0"),
            "source_linear_unit_code",
            AttributeValue::Integer(0),
        ));
        assert_eq!(
            requires_guard(&original, Some(&source_scan)),
            owned || name == "docProps/Config-0-Properties.xml"
        );
        let mut current = original.clone();
        current.model.attributes[0].values = vec![Some(AttributeValue::Integer(3))];
        assert_eq!(
            !edited_properties(&current, &original, &source_scan).is_empty(),
            owned
        );
        assert!(!is_property(&original.model.attributes[0]));
    }
}

#[test]
fn a_default_carrier_is_carried_only_with_its_complete_source_owner() {
    let mut bytes = outer_header();
    bytes.extend(make_block(
        0x43,
        "docProps/ISolidWorksInformation.xml",
        b"<authored/>",
    ));
    let scan = scan(&bytes);
    let owner = scan
        .sections()
        .find(|section| super::is_owner(*section))
        .unwrap();
    let id = owner.native_id();
    let mut notes = cadmpeg_ir::Annotations::default();
    crate::annotations::note(
        &mut notes,
        id.as_str(),
        owner.source_stream(),
        0,
        "xml",
        cadmpeg_ir::Exactness::ByteExact,
    )
    .unwrap();
    let record = || crate::SourceRecord {
        id: id.clone(),
        data: Some(owner.payload()),
    };
    assert!(super::carried(&scan, &[record()], &notes));
    assert!(!super::carried(&scan, &[], &notes));
    assert!(!super::carried(&scan, &[record(), record()], &notes));
    assert!(!super::carried(
        &scan,
        &[crate::SourceRecord {
            id: id.clone(),
            data: Some(b"<changed/>")
        }],
        &notes
    ));
    assert!(!super::carried(
        &scan,
        &[crate::SourceRecord {
            id: id.clone(),
            data: None
        }],
        &notes
    ));
    assert!(!super::carried(
        &scan,
        &[record()],
        &cadmpeg_ir::Annotations::default()
    ));
    let mut rehomed = cadmpeg_ir::Annotations::default();
    crate::annotations::note(
        &mut rehomed,
        id.as_str(),
        &cadmpeg_ir::StreamName::try_from("docProps/custom.xml".to_owned()).unwrap(),
        0,
        "xml",
        cadmpeg_ir::Exactness::ByteExact,
    )
    .unwrap();
    assert!(!super::carried(&scan, &[record()], &rehomed));
}
#[test]
fn compound_default_streams_are_not_carried() {
    let bytes = outer_header();
    let mut scan = scan(&bytes);
    scan.compound_streams
        .push(crate::container::CompoundStream {
            path: cadmpeg_ir::StreamName::try_from("ISolidWorksInformation".to_owned()).unwrap(),
            directory_id: 9,
            start_sector: 0,
            payload: b"retained compound property data".to_vec(),
            decoded_payload: None,
            ps_streams: Vec::new(),
            ps_probe_capped: false,
            ps_auxiliary_refusal: None,
        });
    assert!(!super::carried(
        &scan,
        &[],
        &cadmpeg_ir::annotations::Annotations::default()
    ));
}

#[test]
fn legacy_compound_configuration_carriers_are_not_carried() {
    let bytes = outer_header();
    let mut scan = scan(&bytes);
    scan.compound_streams
        .push(crate::container::CompoundStream {
            path: cadmpeg_ir::StreamName::try_from("Config-7-Properties".to_owned()).unwrap(),
            directory_id: 19,
            start_sector: 0,
            payload: b"retained compound property data".to_vec(),
            decoded_payload: None,
            ps_streams: Vec::new(),
            ps_probe_capped: false,
            ps_auxiliary_refusal: None,
        });
    assert!(requires_guard(&CadIr::empty(), Some(&scan)));
    assert!(!super::carried(
        &scan,
        &[],
        &cadmpeg_ir::Annotations::default()
    ));
}
