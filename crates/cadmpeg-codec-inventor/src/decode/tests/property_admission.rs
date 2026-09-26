// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, View};
use cadmpeg_core::CodecError;

use crate::container::InventorContainer;
use crate::decode::decode_container;
use crate::property_set::{
    Property, PropertySection, PropertySetDescriptor, PropertySetState, PropertySetStream,
    PropertyValue,
};
use crate::test_support::test_fixtures::primary_envelope_fixture;

#[test]
fn parsed_property_records_refuse_each_copy_before_materialization() {
    let bytes = primary_envelope_fixture();
    let arena = DecodeArena::new();
    let (setup_ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("fixture context");
    let mut container = InventorContainer::open(&setup_ctx, root).expect("fixture container");
    container.property_sets.push(PropertySetDescriptor {
        stream: container.rse.segments[0].pair.metadata,
        path: "test-property-set".into(),
        state: PropertySetState::Parsed(PropertySetStream {
            version: 1,
            system_identifier: 0,
            clsid: [1; 16],
            sections: vec![PropertySection {
                fmtid: [2; 16],
                code_page: None,
                offsets_ordered: true,
                dictionary_entries: 0,
                properties: vec![Property {
                    id: 2,
                    name: Some("Name".into()),
                    value: PropertyValue::String {
                        type_code: 31,
                        value: "Value".into(),
                    },
                    raw: View::over_retained(b"raw"),
                }],
            }],
        }),
    });

    let mut cap = 0;
    let mut operations = Vec::new();
    let mut decoded = None;
    for _ in 0..1024 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cap;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited context");
        match decode_container(&ctx, &container) {
            Err(CodecError::ResourceLimit(limit)) => {
                assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
                assert!(limit.used + limit.additional > cap);
                operations.push(limit.operation);
                cap = limit.used + limit.additional;
            }
            Ok(value) => {
                decoded = Some(value);
                break;
            }
            Err(error) => panic!("unexpected property projection error: {error}"),
        }
    }
    let decoded = decoded.expect("property projection reaches service success");
    let namespace = decoded
        .ir
        .native
        .namespace("inventor")
        .expect("native namespace");
    assert_eq!(
        namespace
            .arena_as::<serde_json::Value>("property_sets")
            .expect("set arena")
            .len(),
        1
    );
    assert_eq!(
        namespace
            .arena_as::<serde_json::Value>("property_sections")
            .expect("section arena")
            .len(),
        1
    );
    assert_eq!(
        namespace
            .arena_as::<serde_json::Value>("properties")
            .expect("value arena")
            .len(),
        1
    );
    for operation in [
        "retain Inventor property-set id",
        "retain Inventor property-set path",
        "retain Inventor property-set CLSID",
        "retain Inventor property section id",
        "retain Inventor property section path",
        "retain Inventor property section FMTID",
        "retain Inventor property name",
        "retain Inventor property value id",
        "retain Inventor property raw digest",
        "retain Inventor property value path",
    ] {
        assert!(
            operations.contains(&operation),
            "no refusal for {operation}"
        );
    }
}
