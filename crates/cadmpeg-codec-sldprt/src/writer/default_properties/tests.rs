// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::AttributeId;

use super::{is_property, requires_guard, same_attribute, validate};
use crate::test_support::container::{make_block, outer_header, scan};

fn property(id: &str, name: &str, value: AttributeValue) -> SourceAttribute {
    SourceAttribute {
        id: AttributeId::mint(id).unwrap(),
        target: AttributeTarget::Document,
        name: name.into(),
        values: vec![value],
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
    assert!(validate(&forged, &original, &source_scan).is_err());
    original.model.attributes.push(row);
    assert!(validate(&original, &original, &source_scan).is_ok());
    let mut removed = original.clone();
    removed.model.attributes.clear();
    assert!(validate(&removed, &original, &source_scan).is_err());
    let mut changed = original.clone();
    changed.model.attributes[0].values.clear();
    assert!(validate(&changed, &original, &source_scan).is_err());
}

#[test]
fn emitted_configuration_property_owner_stays_under_its_exact_name_and_bytes() {
    let mut bytes = outer_header();
    bytes.extend(make_block(
        0x43,
        "docProps/Config-3-Properties.xml",
        b"literal",
    ));
    let source_scan = scan(&bytes);
    let same = vec![(
        "docProps/Config-3-Properties.xml".into(),
        b"literal".to_vec(),
    )];
    assert!(super::validate_emitted(&source_scan, &same).is_ok());
    for sections in [
        Vec::new(),
        vec![(
            "docProps/Config-4-Properties.xml".into(),
            b"literal".to_vec(),
        )],
        vec![(
            "docProps/Config-3-Properties.xml".into(),
            b"edited".to_vec(),
        )],
        vec![same[0].clone(), same[0].clone()],
    ] {
        assert!(super::validate_emitted(&source_scan, &sections).is_err());
    }
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
    renamed.values = vec![AttributeValue::String("saved".into())];
    assert!(!same_attribute(&original, &renamed));
    for vector in [false, true] {
        original.values = vec![if vector {
            AttributeValue::vector([-0.0]).unwrap()
        } else {
            AttributeValue::float(-0.0).unwrap()
        }];
        let mut positive = original.clone();
        positive.values = vec![if vector {
            AttributeValue::vector([0.0]).unwrap()
        } else {
            AttributeValue::float(0.0).unwrap()
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
    assert!(validate(&original, &original, &source_scan).is_ok());
    assert!(requires_guard(&original, None));
    let mut changed = original.clone();
    changed.model.attributes[0]
        .values
        .push(AttributeValue::String(String::new()));
    assert!(validate(&changed, &original, &source_scan).is_err());
    changed = original.clone();
    changed.model.attributes[0].name = "Other".into();
    assert!(validate(&changed, &original, &source_scan).is_err());
    changed = original.clone();
    changed.model.attributes.clear();
    assert!(validate(&changed, &original, &source_scan).is_err());
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
    assert!(validate(&original, &original, &source_scan).is_ok());
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
            4 => current.model.attributes[0].values = vec![AttributeValue::Integer(5)],
            5 => current
                .model
                .attributes
                .push(current.model.attributes[0].clone()),
            _ => unreachable!(),
        }
        assert!(validate(&current, &original, &source_scan).is_err());
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
        current.model.attributes[0].values = vec![AttributeValue::Integer(3)];
        assert_eq!(validate(&current, &original, &source_scan).is_err(), owned);
        assert!(!is_property(&original.model.attributes[0]));
    }
}

#[test]
fn retained_default_bytes_and_namespace_must_match_the_complete_source_owner() {
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
    assert!(super::validate_retention(&scan, &[record()], &notes).is_ok());
    assert!(super::validate_retention(&scan, &[], &notes).is_err());
    assert!(super::validate_retention(&scan, &[record(), record()], &notes).is_err());
    assert!(super::validate_retention(
        &scan,
        &[crate::SourceRecord {
            id: id.clone(),
            data: Some(b"<changed/>")
        }],
        &notes
    )
    .is_err());
    assert!(super::validate_retention(
        &scan,
        &[crate::SourceRecord {
            id: id.clone(),
            data: None
        }],
        &notes
    )
    .is_err());
    assert!(
        super::validate_retention(&scan, &[record()], &cadmpeg_ir::Annotations::default()).is_err()
    );
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
    assert!(super::validate_retention(&scan, &[record()], &rehomed).is_err());
}
#[test]
fn emitted_default_carrier_cannot_be_dropped_renamed_changed_or_duplicated() {
    let mut bytes = outer_header();
    bytes.extend(make_block(
        0x43,
        "docProps/ISolidWorksInformation.xml",
        b"<authored/>",
    ));
    let scan = scan(&bytes);
    let section = (
        "docProps/ISolidWorksInformation.xml".to_owned(),
        b"<authored/>".to_vec(),
    );
    assert!(super::validate_emitted(&scan, std::slice::from_ref(&section)).is_ok());
    for sections in [
        vec![],
        vec![("docProps/custom.xml".into(), section.1.clone())],
        vec![(section.0.clone(), b"<changed/>".to_vec())],
        vec![section.clone(), section.clone()],
    ] {
        assert!(super::validate_emitted(&scan, &sections).is_err());
    }
}
#[test]
fn semantic_writer_refuses_unrepresentable_compound_default_streams() {
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
    assert!(
        matches!(super::validate_retention(&scan,&[],&cadmpeg_ir::annotations::Annotations::default()),Err(cadmpeg_core::CodecError::NotImplemented(message))if message.contains("retained compound writer"))
    );
    assert!(super::validate_emitted(&scan, &[]).is_err());
}
