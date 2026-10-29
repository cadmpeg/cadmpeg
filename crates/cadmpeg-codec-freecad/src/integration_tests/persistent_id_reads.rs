// SPDX-License-Identifier: Apache-2.0
//! Kept declaration IDs read their actual source without crediting ignored attributes.

use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

fn check_id(raw: Option<&str>, expected: Option<i64>) {
    let id = raw.map_or_else(String::new, |value| format!(" id='{value}'"));
    let declaration =
        format!("<Object name='Owner' type='PartDesign::Feature'{id} Vendor='opaque-extra'/>");
    let data = "<Object name='Owner'><Properties Count='0'/></Object>";
    let document = format!("<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'>{declaration}</Objects><ObjectData Count='1'>{data}</ObjectData></Document>");
    let archive = crate::test_support::test_archive::archive(&document);
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .expect("authored declaration");
    let objects = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::ObjectRecord>("objects")
        .unwrap();
    let [object] = objects.as_slice() else {
        panic!("one authored declaration")
    };
    assert_eq!(object.persistent_id, expected);
    assert_eq!(object.attributes["Vendor"], "opaque-extra");
}

#[test]
fn admitted_persistent_ids_read_exact_authored_values_and_entity_spellings() {
    for (raw, value) in [
        ("42", 42_i64),
        ("-7", -7),
        ("&#49;", 1),
        ("9223372036854775807", i64::MAX),
        ("-9223372036854775808", i64::MIN),
    ] {
        check_id(Some(raw), Some(value));
    }
}

#[test]
fn absent_or_unparseable_persistent_ids_and_vendor_values_gain_no_read_credit() {
    for raw in [
        None,
        Some("bad"),
        Some("9223372036854775808"),
        Some("-9223372036854775809"),
    ] {
        check_id(raw, None);
    }
}
