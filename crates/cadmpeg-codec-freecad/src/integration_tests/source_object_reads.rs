// SPDX-License-Identifier: Apache-2.0
//! Actual object declaration ownership preserves separate data XML payloads.

use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn object_declaration_reads_leave_data_payloads_exact_and_property_reads_in_their_own_record() {
    let declaration = "<Object name='A&amp;B' type='App::Feature' Vendor='opaque-extra'/>";
    let data = "<Object name='A&#38;B'><Properties Count='1'><Property name='Length' type='App::PropertyFloat'><Float value='3.5'/></Property></Properties></Object>";
    let document = format!("<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'>{declaration}</Objects><ObjectData Count='1'>{data}</ObjectData></Document>");
    let bytes = crate::test_support::test_archive::archive(&document);
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let namespace = decoded.ir().native.namespace("fcstd").unwrap();
    let objects = namespace
        .arena_as::<crate::native::ObjectRecord>("objects")
        .unwrap();
    assert_eq!(objects.len(), 1);
    let object = &objects[0];
    assert_eq!(object.name(), "A&B");
    assert_eq!(object.type_name, "App::Feature");
    assert_eq!(object.attributes["Vendor"], "opaque-extra");
    let properties = namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let property = properties
        .iter()
        .find(|property| property.name == "Length")
        .unwrap();
    assert_eq!(property.owner, *object.id());
    assert_eq!(property.values()[0].attributes["value"], "3.5");
}
