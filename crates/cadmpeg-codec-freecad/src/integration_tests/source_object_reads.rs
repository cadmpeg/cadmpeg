// SPDX-License-Identifier: Apache-2.0
//! Actual object declaration ownership preserves separate data XML payloads.

use cadmpeg_core::decode::u64_from_index;
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
    let fidelity = decoded.source_fidelity();
    let layout = &fidelity.record_layout()[object.id()];
    let declaration_start = document.find(declaration).unwrap();
    let data_start = document.find(data).unwrap();
    assert_eq!(layout.offset(), u64_from_index(declaration_start));
    assert_eq!(layout.byte_len(), u64_from_index(declaration.len()));
    assert_eq!(layout.pointers()["byte_start"], u64_from_index(data_start));
    assert_eq!(
        layout.pointers()["byte_end"],
        u64_from_index(data_start + data.len())
    );
    assert_eq!(layout.payloads()["raw_xml"].as_ref(), data.as_bytes());
    assert_eq!(
        crate::test_support::source_fidelity::bytes(fidelity, object.id()),
        data.as_bytes()
    );
    let name_start = declaration_start + "<Object name='".len();
    let type_start = document.find("type='App::Feature'").unwrap() + "type='".len();
    let data_name_start = data_start + "<Object name='".len();
    let vendor_key = document.find("Vendor='opaque-extra'").unwrap();
    let vendor_value = vendor_key + "Vendor='".len();
    assert_eq!(
        layout.reads()[layout.stream()],
        [
            u64_from_index(name_start - "name='".len())..u64_from_index(name_start - "='".len()),
            u64_from_index(name_start)..u64_from_index(name_start + "A&amp;B".len()),
            u64_from_index(type_start - "type='".len())..u64_from_index(type_start - "='".len()),
            u64_from_index(type_start)..u64_from_index(type_start + "App::Feature".len()),
            u64_from_index(vendor_key)..u64_from_index(vendor_key + "Vendor".len()),
            u64_from_index(vendor_value)..u64_from_index(vendor_value + "opaque-extra".len()),
            u64_from_index(data_name_start - "name='".len())
                ..u64_from_index(data_name_start - "='".len()),
            u64_from_index(data_name_start)..u64_from_index(data_name_start + "A&#38;B".len()),
        ]
    );
    let properties = namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let property = properties
        .iter()
        .find(|property| property.name == "Length")
        .unwrap();
    assert_eq!(property.owner, *object.id());
    assert_eq!(property.values()[0].attributes["value"], "3.5");
    let property_layout = &fidelity.record_layout()[&property.id];
    let range = |key: &str, raw: &str| {
        let start = document.find(&format!("{key}='{raw}'")).unwrap() + key.len() + "='".len();
        u64_from_index(start)..u64_from_index(start + raw.len())
    };
    let key_range = |key: &str, raw: &str| {
        let at = document.find(&format!("{key}='{raw}'")).unwrap();
        u64_from_index(at)..u64_from_index(at + key.len())
    };
    assert_eq!(
        property_layout.reads()[property_layout.stream()],
        [
            key_range("name", "Length"),
            range("name", "Length"),
            key_range("type", "App::PropertyFloat"),
            range("type", "App::PropertyFloat"),
            {
                let at = document.find("<Float value='3.5'").unwrap() + 1;
                u64_from_index(at)..u64_from_index(at + "Float".len())
            },
            key_range("value", "3.5"),
            range("value", "3.5")
        ]
    );
    let extra = range("Vendor", "opaque-extra");
    assert!(fidelity.b1_spans()[layout.stream()]
        .spans()
        .iter()
        .all(|span| {
            let unread = span.range();
            unread.end <= extra.start || unread.start >= extra.end
        }));
}
