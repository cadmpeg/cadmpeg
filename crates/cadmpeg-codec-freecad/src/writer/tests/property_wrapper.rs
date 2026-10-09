// SPDX-License-Identifier: Apache-2.0
//! Property-wrapper selection, declaration refusal order and retained byte ranges.

use cadmpeg_core::CodecError;

use crate::native::{PropertyBody, PropertyFamily, PropertyRecord, RetainedXml, ValueRecord};
use crate::writer::{serialize_property, serialize_value};

fn property(xml: &str, body: PropertyBody) -> PropertyRecord {
    PropertyRecord {
        id: "test:property#values".into(),
        owner: "test:object#owner".into(),
        name: "Values".into(),
        type_name: "App::PropertyStringList".into(),
        family: PropertyFamily::List,
        status: None,
        body,
        order: 0,
        xml: RetainedXml::from_text(xml.into(), 0).expect("retained span"),
    }
}

#[test]
fn wrapper_errors_precede_value_count_and_declaration_drift() {
    for (xml, expected) in [
        ("<!-- no property element -->", "retained property has no element"),
        (r#"<Wrong name="Other" type="Other" status="bad"><String/></Wrong>"#,
         "retained property has invalid status"),
    ] {
        let error = serialize_property(&property(xml, PropertyBody::Transient))
            .expect_err("wrapper rejects before later checks");
        assert!(matches!(error, CodecError::Malformed(message) if message == expected));
    }
    let error = serialize_property(&property("<Property>", PropertyBody::Transient))
        .expect_err("malformed wrapper");
    assert!(matches!(error, CodecError::Malformed(message)
        if message.starts_with("invalid retained property XML:")));
    let error = serialize_property(&property(
        r#"<Property name="Other" type="Other"><String/></Property>"#,
        PropertyBody::Transient,
    )).expect_err("declaration drift precedes value count");
    assert!(matches!(error, CodecError::NotImplemented(message)
        if message == "editing FCStd property declaration test:property#values requires a typed serializer"));
}

#[test]
fn transient_wrapper_preserves_utf8_and_exact_bytes() {
    let xml = r#"<!-- leading --><_Property name="Values" type="App::PropertyStringList" status="8" custom="λ"/><!-- trailing -->"#;
    let mut property = property(xml, PropertyBody::Transient);
    property.status = Some(8);
    assert_eq!(serialize_property(&property).expect("unchanged transient").as_ref(), xml.as_bytes());
}

#[test]
fn nested_values_preserve_descendant_order_and_utf8_byte_ranges() {
    let nested = r#"<List marker="λ"><String value="same"/></List>"#;
    let leaf = r#"<String value="same"/>"#;
    let xml = format!(r#"<Property name="Values" type="App::PropertyStringList">{nested}</Property>"#);
    let values = vec![
        ValueRecord {
            tag: "List".into(), order: 0,
            attributes: [("marker".into(), "λ".into())].into(),
            text: None, raw_xml: nested.into(),
        },
        ValueRecord {
            tag: "String".into(), order: 1,
            attributes: [("value".into(), "same".into())].into(),
            text: None, raw_xml: leaf.into(),
        },
    ];
    let mut property = property(&xml, PropertyBody::Persisted {
        values, links: Vec::new(), side_entries: Vec::new(), dynamic: None,
    });
    assert_eq!(serialize_property(&property).expect("unchanged nested values").as_ref(), xml.as_bytes());
    property.values_mut().expect("persisted property")[1]
        .attributes.insert("value".into(), "edited".into());
    let expected = r#"<Property name="Values" type="App::PropertyStringList"><List marker="λ"><String value="edited"/></List></Property>"#;
    assert_eq!(serialize_property(&property).expect("edited nested leaf").as_ref(), expected.as_bytes());
}

#[test]
fn wrapper_value_count_and_changed_value_provenance_remain_distinct_errors() {
    let xml = r#"<Property name="Values" type="App::PropertyStringList"><String value="old"/></Property>"#;
    let mut property = property(xml, PropertyBody::Persisted {
        values: Vec::new(), links: Vec::new(), side_entries: Vec::new(), dynamic: None,
    });
    let error = serialize_property(&property).expect_err("missing value provenance");
    assert!(matches!(error, CodecError::Malformed(message)
        if message == "property test:property#values value provenance count changed"));
    property.values_mut().expect("persisted property").push(ValueRecord {
        tag: "String".into(), order: 0,
        attributes: [("value".into(), "edited".into())].into(),
        text: None, raw_xml: r#"<String value="foreign"/>"#.into(),
    });
    let error = serialize_property(&property).expect_err("changed value has foreign provenance");
    assert!(matches!(error, CodecError::Malformed(message)
        if message == "property test:property#values retained value 0 disagrees with provenance"));
}

#[test]
fn unchanged_value_xml_borrows_its_source_and_edits_own_output() {
    let mut value = ValueRecord {
        tag: "String".into(),
        order: 0,
        attributes: [("value".into(), "λ & 𐀀".into())].into(),
        text: None,
        raw_xml: "<String value='λ &amp; 𐀀'/>".into(),
    };
    let serialized = serialize_value(&value).expect("unchanged value");
    let std::borrow::Cow::Borrowed(original) = serialized else {
        panic!("unchanged XML retains its actual source borrow");
    };
    assert_eq!(original, value.raw_xml);
    assert_eq!(original.as_ptr(), value.raw_xml.as_ptr());
    value.attributes.insert("value".into(), "edited & λ".into());
    let serialized = serialize_value(&value).expect("edited value");
    let std::borrow::Cow::Owned(edited) = serialized else {
        panic!("edited XML owns its actual output");
    };
    assert_eq!(edited, r#"<String value="edited &amp; λ"/>"#);

    let nested = ValueRecord {
        tag: "List".into(), order: 0,
        attributes: [("marker".into(), "λ".into())].into(),
        text: None,
        raw_xml: r#"<List marker="λ"><String/></List>"#.into(),
    };
    assert!(matches!(serialize_value(&nested).expect("unchanged nested value"),
        std::borrow::Cow::Borrowed(raw) if raw == nested.raw_xml));
}

#[test]
fn unchanged_property_borrow_keeps_ordered_attributes_and_decoded_text() {
    let raw = r#"<String z="2" a="λ">λ &amp; 𐀀</String>"#;
    let value = ValueRecord {
        tag: "String".into(), order: 0,
        attributes: [("a".into(), "λ".into()), ("z".into(), "2".into())].into(),
        text: Some("λ & 𐀀".into()), raw_xml: raw.into(),
    };
    assert!(matches!(serialize_value(&value).expect("same semantic attributes and text"),
        std::borrow::Cow::Borrowed(bytes) if bytes == raw));
    let xml = format!(r#"<Property name="Values" type="App::PropertyStringList">{raw}</Property>"#);
    let mut property = property(&xml, PropertyBody::Persisted {
        values: vec![value], links: Vec::new(), side_entries: Vec::new(), dynamic: None,
    });
    let serialized = serialize_property(&property).expect("unchanged property");
    let std::borrow::Cow::Borrowed(bytes) = serialized else {
        panic!("unchanged property retains its actual source borrow");
    };
    assert_eq!(bytes, xml.as_bytes());
    assert_eq!(bytes.as_ptr(), property.xml.text().as_ptr());
    property.values_mut().expect("persisted value")[0].text = Some("edited & λ".into());
    let serialized = serialize_property(&property).expect("edited text");
    let std::borrow::Cow::Owned(bytes) = serialized else {
        panic!("edited property owns its output");
    };
    let expected = r#"<Property name="Values" type="App::PropertyStringList"><String a="λ" z="2">edited &amp; λ</String></Property>"#;
    assert_eq!(bytes, expected.as_bytes());
}
