// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};

use crate::container::InventorContainer;
use crate::decode::decode_container;
use crate::property_set::{
    Property, PropertySection, PropertySetDescriptor, PropertySetState, PropertySetStream,
    PropertyValue,
};
use crate::test_support::test_fixtures::primary_envelope_fixture;

#[test]
fn parsed_property_records_are_projected() {
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

    let decoded = decode_container(&setup_ctx, &container).expect("property projection");
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
}
