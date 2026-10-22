// SPDX-License-Identifier: Apache-2.0
//! Property interpretation source and typed-edit controls.

use super::PropertyReader;
use crate::native::{PropertyBody, PropertyFamily, PropertyRecord, ValueRecord};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use std::collections::BTreeMap;

fn property() -> PropertyRecord {
    PropertyRecord {
        dynamic: None,
        id: "property".into(),
        owner: "owner".into(),
        name: "Label".into(),
        type_name: "App::PropertyString".into(),
        family: PropertyFamily::String,
        status: None,
        order: 0,
        body: Some(PropertyBody::Persisted {
            values: vec![ValueRecord {
                tag: "String".into(),
                order: 0,
                parent: None,
                attributes: BTreeMap::from([("value".into(), "old".into())]),
                decoded_json: None,
                text: None,
            }],
            links: Vec::new(),
            side_entries: Vec::new(),
        }),
    }
}

#[test]
fn source_reader_keeps_direct_mixed_text_and_never_credits_lookup_as_a_read() {
    let xml = "<Property><String>direct Ω<Sub>middle &amp; content</Sub>tail</String></Property>";
    let fixture = crate::test_support::source_fidelity::PropertyFixture::new(property(), xml);
    crate::test_support::source_fidelity::with_property_reader(
        &[],
        &[&fixture],
        &DecodePolicy::service(),
        |reader| {
            let source = reader.property_xml(&fixture).unwrap();
            assert_eq!(source.as_ref(), xml);
            let tree = reader
                .parse_xml(source.as_ref(), "source reader control")
                .unwrap();
            let text = tree.root_element().first_element_child().unwrap();
            assert_eq!(text.text(), Some("direct Ω"));
            assert_eq!(text.last_child().unwrap().text(), Some("tail"));
            assert!(reader.take_record_captures().unwrap().is_empty());
        },
    );
}

#[test]
fn typed_reader_uses_current_values_after_an_edit_and_deletion() {
    let arena = DecodeArena::new();
    let (context, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let reader = PropertyReader::typed(&context);
    let mut property = property();
    assert!(reader.property_xml(&property).unwrap().contains("old"));
    let Some(PropertyBody::Persisted { values, .. }) = &mut property.body else {
        panic!("persisted fixture")
    };
    values[0].attributes.insert("value".into(), "new Ω".into());
    let edited = reader.property_xml(&property).unwrap();
    assert!(edited.contains("new Ω"));
    assert!(!edited.contains("old"));
    let Some(PropertyBody::Persisted { values, .. }) = &mut property.body else {
        panic!("persisted fixture")
    };
    values.clear();
    let deleted = reader.property_xml(&property).unwrap();
    assert!(!deleted.contains("<String"));
    assert!(!deleted.contains("new Ω"));
}

#[test]
fn source_reader_rejects_a_view_from_another_session() {
    let xml = b"<Property><String value='old'/></Property>";
    let arena = DecodeArena::new();
    let (context, _) =
        DecodeContext::from_root_bytes(xml, &arena, &DecodePolicy::service()).unwrap();
    let other_arena = DecodeArena::new();
    let (_foreign_context, foreign) =
        DecodeContext::from_root_bytes(xml, &other_arena, &DecodePolicy::service()).unwrap();
    let document = roxmltree::Document::parse(std::str::from_utf8(xml).unwrap()).unwrap();
    let property = property();
    let ranges = BTreeMap::from([(
        property.id.clone(),
        crate::fidelity::property_source(document.root_element(), &property).unwrap(),
    )]);
    assert!(PropertyReader::source(&context, foreign, &ranges).is_err());
}
