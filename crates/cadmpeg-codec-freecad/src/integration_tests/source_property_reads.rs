// SPDX-License-Identifier: Apache-2.0
//! Property identity fields are read without promoting their opaque contents.

use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn an_entity_spelled_property_keeps_its_identity_and_status_while_opaque_bytes_remain_b1() {
    let document = "<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object name='Owner' type='App::Feature'/></Objects><ObjectData Count='1'><Object name='Owner'><Properties Count='1'><Property name='A&amp;B' type='Vendor::Futu&#x72;e' status='007'><String value='opaque-state'/></Property></Properties></Object></ObjectData></Document>";
    let bytes = crate::test_support::test_archive::archive(document);
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let properties = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let property = &properties[0];
    assert_eq!(property.name, "A&B");
    assert_eq!(property.type_name, "Vendor::Future");
    assert_eq!(property.status, Some(7));
    assert_eq!(property.values()[0].attributes["value"], "opaque-state");
    assert_eq!(
        property.id,
        crate::native::native_child_id(
            "property",
            &crate::native::native_id("object", "Owner"),
            "A&B"
        )
    );
}
