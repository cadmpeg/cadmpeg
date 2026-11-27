// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
use std::collections::HashSet;

use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{AttributeId, UnknownId};
use cadmpeg_ir::Annotations;

use super::{carrier_scan, changed, same_attribute, Held, StoredProperties};

/// Each saved-property attribute of `ir` that the write does not write as
/// `ir` states it, against the carriers among `records`.
fn unwritten(
    ir: &CadIr,
    records: &[crate::SourceRecord<'_>],
    notes: &Annotations,
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
) -> Result<std::collections::BTreeMap<String, Held>, cadmpeg_core::CodecError> {
    Ok(super::unwritten(
        ir,
        &StoredProperties::read(records, notes, ctx)?,
    ))
}

const DEFAULT: &str = "docProps/ISolidWorksInformation.xml";
const DEFAULT_XML: &[u8] = br#"<p:Properties xmlns:p="http://schemas.openxmlformats.org/officeDocument/2006/SolidworksInformation-properties" xmlns:v="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes" SW_UnitsLinear="0"><propertySection fmtid="{D5CDD505-2E9C-101B-9397-08002B2CF9AE}"><property pid="2" name="Designer"><v:lpstr>Merlin</v:lpstr></property><property pid="3" name="Mass"><v:r8>-0.0</v:r8></property></propertySection></p:Properties>"#;
const CONFIGURATION: &str = "docProps/Config-3-Properties.xml";
const CONFIGURATION_XML: &[u8] = br#"<ConfigProperties xmlns="http://www.solidworks.com/config-properties" xmlns:v="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes"><propertySection xmlns="" fmtid="{D5CDD505-2E9C-101B-9397-08002B2CF9AE}"><property name="Finish" pid="3" IsEquation="False"><v:lpstr>Anodized</v:lpstr></property><propertyNameDictionaryElement name="Finish" pid="3"/></propertySection></ConfigProperties>"#;

/// One retained record of `id` with `data`, and its provenance in `stream`.
fn record(
    notes: &mut Annotations,
    id: &str,
    stream: &str,
    data: Option<&'static [u8]>,
) -> crate::SourceRecord<'static> {
    crate::annotations::note(
        notes,
        cadmpeg_ir::ids::Identity::new(id).expect("identity grammar"),
        &cadmpeg_ir::StreamName::try_from(stream.to_owned()).unwrap(),
        0,
        "xml",
        cadmpeg_ir::Exactness::ByteExact,
    )
    .unwrap();
    crate::SourceRecord {
        id: UnknownId::mint(id).unwrap(),
        data,
    }
}

/// The saved-property rows a decode of `records` states.
fn decoded(records: &[crate::SourceRecord<'_>], notes: &Annotations) -> CadIr {
    let scan = carrier_scan(records, notes);
    let mut ir = CadIr::empty();
    ir.model.attributes = crate::metadata::saved_property_rows(
        &cadmpeg_core::decode::test_support::service_decode_context(),
        &scan,
    )
    .unwrap();
    ir
}

fn property(id: &str, name: &str, value: AttributeValue) -> SourceAttribute {
    SourceAttribute {
        id: AttributeId::mint(id).unwrap(),
        target: AttributeTarget::Document.into(),
        name: name.into(),
        values: vec![Some(value)],
        native_ref: None,
    }
}

fn charged(stated: &CadIr, stored: &CadIr) -> Vec<(String, Held)> {
    changed(
        &stated.model.attributes,
        &stored.model.attributes,
        &HashSet::new(),
    )
    .into_iter()
    .collect()
}

#[test]
fn configuration_properties_are_compared_even_when_removed_or_forged() {
    let mut original = CadIr::empty();
    let row = property(
        "sldprt:metadata:configuration_property#8:3",
        "saved_configuration_property",
        AttributeValue::String("stored".into()),
    );
    let mut forged = original.clone();
    forged.model.attributes.push(row.clone());
    assert_eq!(
        charged(&forged, &original),
        [(row.id.as_str().to_owned(), Held::Nothing)]
    );
    original.model.attributes.push(row);
    assert!(charged(&original, &original).is_empty());
    let mut removed = original.clone();
    removed.model.attributes.clear();
    assert_eq!(
        charged(&removed, &original),
        [(
            original.model.attributes[0].id.as_str().to_owned(),
            Held::Source
        )]
    );
    let mut edited = original.clone();
    edited.model.attributes[0].values.clear();
    assert!(!charged(&edited, &original).is_empty());
}

#[test]
fn owned_properties_keep_names_and_values_with_one_zero() {
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
        let written = |value: f64| {
            let mut written = original.clone();
            written.values = vec![if vector {
                Some(AttributeValue::vector([value]).unwrap())
            } else {
                Some(AttributeValue::float(value).unwrap())
            }];
            written
        };
        assert!(same_attribute(&original, &written(0.0)));
        assert!(!same_attribute(&original, &written(0.5)));
    }
}

#[test]
fn empty_saved_property_remains_read_only_and_distinct_from_a_blank_string() {
    let mut original = CadIr::empty();
    let mut row = property(
        "sldprt:metadata:default_property#8:5:Empty",
        "Empty",
        AttributeValue::String(String::new()),
    );
    row.values.clear();
    original.model.attributes.push(row);
    assert!(charged(&original, &original).is_empty());
    let mut edited = original.clone();
    edited.model.attributes[0]
        .values
        .push(Some(AttributeValue::String(String::new())));
    assert!(!charged(&edited, &original).is_empty());
    edited = original.clone();
    edited.model.attributes[0].name = "Other".into();
    assert!(!charged(&edited, &original).is_empty());
    edited = original.clone();
    edited.model.attributes.clear();
    assert!(!charged(&edited, &original).is_empty());
}

#[test]
fn removed_added_renamed_and_reidentified_properties_are_charged() {
    let mut original = CadIr::empty();
    original.model.attributes.push(property(
        "sldprt:metadata:default_property#8:5:Name",
        "Name",
        AttributeValue::String("saved".into()),
    ));
    assert!(charged(&original, &original).is_empty());
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
        assert!(!charged(&current, &original).is_empty(), "{mutation}");
    }
}

#[test]
fn a_unit_mirror_is_compared_only_for_an_emitted_carrier() {
    let mirror = property(
        "sldprt:metadata:source_linear_unit_code#40:0",
        "source_linear_unit_code",
        AttributeValue::Integer(0),
    );
    let mut edited = mirror.clone();
    edited.values = vec![Some(AttributeValue::Integer(3))];
    for (carriers, charged) in [(HashSet::from([40]), true), (HashSet::from([41]), false)] {
        assert_eq!(
            !changed(
                std::slice::from_ref(&edited),
                std::slice::from_ref(&mirror),
                &carriers
            )
            .is_empty(),
            charged
        );
    }
}

#[test]
fn the_carriers_are_the_retained_owner_blocks_with_bytes() {
    let mut notes = Annotations::default();
    let records = [
        record(
            &mut notes,
            "sldprt:file:block#40",
            DEFAULT,
            Some(DEFAULT_XML),
        ),
        record(
            &mut notes,
            "sldprt:file:block#41",
            CONFIGURATION,
            Some(CONFIGURATION_XML),
        ),
        record(&mut notes, "sldprt:file:block#42", DEFAULT, None),
        record(
            &mut notes,
            "sldprt:file:block#43",
            "other/docProps/ISolidWorksInformation.xml",
            Some(DEFAULT_XML),
        ),
        record(
            &mut notes,
            "sldprt:file:block#44",
            "docProps/custom.xml",
            Some(DEFAULT_XML),
        ),
        record(
            &mut notes,
            "sldprt:file:compound-stream#45",
            "ISolidWorksInformation",
            Some(DEFAULT_XML),
        ),
        crate::SourceRecord {
            id: UnknownId::mint("sldprt:file:block#46").unwrap(),
            data: Some(DEFAULT_XML),
        },
    ];
    let scan = carrier_scan(&records, &notes);
    assert_eq!(
        scan.blocks
            .iter()
            .map(|block| (block.offset, block.payload.as_slice()))
            .collect::<Vec<_>>(),
        [(40, DEFAULT_XML), (41, CONFIGURATION_XML)]
    );
}

/// The output holds each property of an emitted carrier as stored: an
/// unedited one is written, an edited or removed one is charged as kept,
/// and an added one, or one whose carrier has no bytes, as written nowhere.
#[test]
fn each_property_is_charged_by_the_carrier_bytes_the_write_emits() {
    let mut notes = Annotations::default();
    let records = [
        record(
            &mut notes,
            "sldprt:file:block#40",
            DEFAULT,
            Some(DEFAULT_XML),
        ),
        record(
            &mut notes,
            "sldprt:file:block#41",
            CONFIGURATION,
            Some(CONFIGURATION_XML),
        ),
    ];
    let source = decoded(&records, &notes);
    let id = |name: &str| {
        source
            .model
            .attributes
            .iter()
            .find(|row| row.name == name)
            .unwrap()
            .id
            .as_str()
            .to_owned()
    };
    assert_eq!(source.model.attributes.len(), 4);
    assert!(unwritten(
        &source,
        &records,
        &notes,
        &cadmpeg_core::decode::test_support::service_decode_context()
    )
    .unwrap()
    .is_empty());
    let mut read_back = source.clone();
    for row in &mut read_back.model.attributes {
        if row.name == "Mass" {
            assert!(matches!(
                row.values.as_slice(),
                [Some(AttributeValue::Float(stored))] if stored.get().is_sign_negative()
            ));
            row.values = vec![Some(AttributeValue::float(0.0).unwrap())];
        }
    }
    assert!(unwritten(
        &read_back,
        &records,
        &notes,
        &cadmpeg_core::decode::test_support::service_decode_context()
    )
    .unwrap()
    .is_empty());

    let mut edited = source.clone();
    for row in &mut edited.model.attributes {
        if row.name == "Designer" {
            row.values = vec![Some(AttributeValue::String("edited".into()))];
        }
    }
    edited.model.attributes.retain(|row| row.name != "Mass");
    let added = "sldprt:metadata:default_property#40:9:Added";
    edited
        .model
        .attributes
        .push(property(added, "Added", AttributeValue::Integer(1)));
    let mut expected = vec![
        (id("Designer"), Held::Source),
        (id("Mass"), Held::Source),
        (added.to_owned(), Held::Nothing),
    ];
    expected.sort();
    assert_eq!(
        unwritten(
            &edited,
            &records,
            &notes,
            &cadmpeg_core::decode::test_support::service_decode_context()
        )
        .unwrap()
        .into_iter()
        .collect::<Vec<_>>(),
        expected
    );

    let without_bytes = [
        crate::SourceRecord {
            id: records[0].id.clone(),
            data: None,
        },
        crate::SourceRecord {
            id: records[1].id.clone(),
            data: records[1].data,
        },
    ];
    let mut expected = vec![(id("Designer"), Held::Nothing), (id("Mass"), Held::Nothing)];
    expected.sort();
    assert_eq!(
        unwritten(
            &source,
            &without_bytes,
            &notes,
            &cadmpeg_core::decode::test_support::service_decode_context()
        )
        .unwrap()
        .into_iter()
        .collect::<Vec<_>>(),
        expected
    );
}
