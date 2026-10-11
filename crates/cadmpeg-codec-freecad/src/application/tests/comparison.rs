// SPDX-License-Identifier: Apache-2.0
//! Typed application comparison parity with the former JSON comparison.

use crate::native::{EntryRecord, LinkTarget, LinkTargetWire, ObjectRecord, PropertyBody, PropertyFamily, PropertyRecord, RetainedXml};
use cadmpeg_ir::native::NativeNamespace;
use serde_json::Value;

fn inputs() -> (Vec<ObjectRecord>, Vec<PropertyRecord>, Vec<EntryRecord>) {
    let object = ObjectRecord {
        identity: crate::native::object_identity::ObjectIdentity::try_new(
            "fcstd:native:object#Owner".into(), "Owner".into(),
        ).unwrap(),
        type_name: "Vendor::Feature".into(),
        persistent_id: None,
        view_type: None,
        attributes: std::collections::BTreeMap::new(),
        dependencies: vec!["fcstd:native:object#Dependency".into()],
        dependency_allow_partial: None,
        order: 2,
        data: Some(RetainedXml::from_text("<Object/>".into(), 10).unwrap()),
    };
    let link = LinkTarget::try_from(LinkTargetWire {
        document: Some("other.FCStd".to_owned()),
        document_attribute: Some("file".into()),
        object: Some("Other".to_owned()),
        subelements: vec!["Face1".into()],
    }).unwrap();
    let property = PropertyRecord {
        id: "fcstd:native:property#Owner:Data".into(),
        owner: object.id().clone(),
        name: "Data".into(),
        type_name: "App::PropertyFileIncluded".into(),
        family: PropertyFamily::File,
        status: None,
        body: PropertyBody::Persisted {
            values: Vec::new(),
            links: vec![Some(link), None],
            side_entries: vec!["shared.bin".into()],
            dynamic: None,
        },
        order: 3,
        xml: RetainedXml::from_text("<Property/>".into(), 20).unwrap(),
    };
    let entry = crate::test_support::entry_record(
        "fcstd:native:entry#shared.bin".into(),
        "shared.bin".into(),
        cadmpeg_core::container::ContainerRole::Auxiliary,
        Vec::new(), vec![7; 4096],
    );
    (vec![object], vec![property], vec![entry])
}

fn assert_parity(change: impl FnOnce(&mut Vec<Value>), expected_match: bool) {
    let (objects, properties, entries) = inputs();
    let mut expected = crate::test_support::with_service_context(&[], |ctx| {
        super::super::wire_records(ctx, &objects, &properties, &entries).unwrap()
            .into_iter().map(|record| serde_json::to_value(record).unwrap()).collect::<Vec<_>>()
    });
    expected.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
    let mut actual = expected.clone();
    change(&mut actual);
    let mut namespace = NativeNamespace::default();
    crate::test_support::with_service_context(&[], |ctx| {
        namespace.set_arena(ctx, "applications", &actual).unwrap();
    });
    // This is the former comparison: typed JSON arena reads against serialized records.
    let old = crate::test_support::with_service_context(&[], |ctx| {
        let actual = namespace.arena_iter_as_for_decode::<Value>(ctx, "applications")
            .collect::<Result<Vec<_>, _>>().unwrap();
        actual == expected
    });
    let typed = crate::test_support::with_service_context(&[], |ctx| {
        super::super::matches_native(ctx, &namespace, &objects, &properties, &entries).unwrap()
    });
    assert_eq!(old, expected_match, "old comparison result");
    assert_eq!(typed, old, "typed comparison parity");
}

#[test]
fn application_typed_equal_records_match_json() {
    assert_parity(|_| {}, true);
}

macro_rules! field_cases {
    ($prefix:literal; $($test:ident => $field:literal),+ $(,)?) => {$(
        #[test]
        fn $test() {
            assert_parity(|records| {
                let field = records[0].pointer_mut(concat!($prefix, $field)).unwrap();
                match field {
                    Value::String(text) => text.push('x'),
                    Value::Bool(value) => *value = !*value,
                    Value::Number(number) => *number = (number.as_u64().unwrap() + 1).into(),
                    Value::Array(values) => match values.last_mut().unwrap() {
                        Value::String(text) => text.push('x'),
                        Value::Number(number) => *number = (number.as_u64().unwrap() + 1).into(),
                        _ => values.push(values[0].clone()),
                    },
                    _ => panic!("unexpected field shape"),
                }
            }, false);
        }
    )+};
}

field_cases!("/";
    application_typed_record_id => "id",
    application_typed_record_object => "object",
    application_typed_record_type_name => "type_name",
    application_typed_record_domain => "domain",
    application_typed_record_properties => "properties",
    application_typed_record_dependencies => "dependencies",
    application_typed_record_side_entries => "side_entries",
    application_typed_record_inert_payload => "inert_payload",
    application_typed_record_order => "order",
    application_typed_record_byte_start => "byte_start",
    application_typed_record_byte_end => "byte_end",
    application_typed_record_byte_len => "byte_len",
    application_typed_record_sha256 => "sha256",
    application_typed_record_data => "data",
    application_typed_record_property_records => "property_records",
);
field_cases!("/property_records/0/";
    application_typed_property_id => "id",
    application_typed_property_object => "object",
    application_typed_property_property => "property",
    application_typed_property_type_name => "type_name",
    application_typed_property_family => "family",
    application_typed_property_order => "order",
    application_typed_property_links => "links",
    application_typed_property_byte_start => "byte_start",
    application_typed_property_byte_end => "byte_end",
    application_typed_property_byte_len => "byte_len",
    application_typed_property_sha256 => "sha256",
    application_typed_property_data => "data",
    application_typed_property_payloads => "payloads",
    application_typed_property_inert => "inert",
);
field_cases!("/property_records/0/payloads/0/";
    application_typed_payload_entry => "entry",
    application_typed_payload_name => "name",
    application_typed_payload_byte_len => "byte_len",
    application_typed_payload_sha256 => "sha256",
    application_typed_payload_data => "data",
);
field_cases!("/property_records/0/links/0/";
    application_typed_link_document => "document",
    application_typed_link_attribute => "document_attribute",
    application_typed_link_object => "object",
    application_typed_link_subelements => "subelements",
);

#[test]
fn application_typed_payload_last_byte_matches_json() {
    assert_parity(|records| {
        *records[0]["property_records"][0]["payloads"][0]["data"].as_array_mut().unwrap().last_mut().unwrap() = 8.into();
    }, false);
}

#[test]
fn application_typed_fewer_records_matches_json() {
    assert_parity(Vec::clear, false);
}

#[test]
fn application_typed_more_records_matches_json() {
    assert_parity(|records| {
        let mut extra = records[0].clone();
        extra["id"] = "fcstd:native:application#ZExtra".into();
        records.push(extra);
    }, false);
}

#[test]
fn application_typed_rejects_extra_missing_and_wrong_shape_fields() {
    for path in ["", "/property_records/0", "/property_records/0/payloads/0", "/property_records/0/links/0"] {
        assert_parity(|records| {
            records[0].pointer_mut(path).unwrap().as_object_mut().unwrap().insert("extra".into(), Value::Null);
        }, false);
        let fields = match path {
            "" => vec!["object", "order", "data"],
            "/property_records/0" => vec!["property", "family", "links"],
            "/property_records/0/payloads/0" => vec!["entry", "byte_len", "data"],
            _ => vec!["document", "document_attribute", "object", "subelements"],
        };
        for field in fields {
            assert_parity(|records| {
                records[0].pointer_mut(path).unwrap().as_object_mut().unwrap().remove(field);
            }, false);
        }
    }
    for (path, fields) in [
        ("/property_records/0", vec!["id", "object", "property", "type_name", "family", "order", "links", "byte_start", "byte_end", "byte_len", "sha256", "data", "payloads", "inert"]),
        ("/property_records/0/payloads/0", vec!["entry", "name", "byte_len", "sha256", "data"]),
        ("/property_records/0/links/0", vec!["document", "document_attribute", "object", "subelements"]),
    ] {
        assert_parity(|records| {
            let object = records[0].pointer_mut(path).unwrap();
            *object = Value::Array(fields.iter().map(|field| object[*field].clone()).collect());
        }, false);
    }
    assert_parity(|records| {
        records[0]["property_records"][0]["family"] = serde_json::json!({"file": null});
    }, false);
    for path in ["/order", "/byte_start", "/property_records/0/order", "/property_records/0/payloads/0/byte_len"] {
        assert_parity(|records| {
            let field = records[0].pointer_mut(path).unwrap();
            *field = Value::from(field.as_f64().unwrap());
        }, false);
    }
}

#[test]
fn application_typed_comparisons_refuse_before_field_and_payload_work() {
    let (objects, properties, entries) = inputs();
    let mut namespace = NativeNamespace::default();
    crate::test_support::with_service_context(&[], |ctx| {
        super::super::install(ctx, &mut namespace, &objects, &properties, &entries).unwrap();
    });
    for operation in ["FreeCAD application field comparison", "FreeCAD application payload comparison"] {
        crate::test_support::refusal_at(cadmpeg_core::decode::ResourceDimension::WorkUnits, &[], operation, |ctx| {
            super::super::matches_native(ctx, &namespace, &objects, &properties, &entries).map_err(cadmpeg_core::CodecError::from)
        });
    }
}
