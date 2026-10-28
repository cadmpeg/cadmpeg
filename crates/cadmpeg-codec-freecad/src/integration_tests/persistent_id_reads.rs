// SPDX-License-Identifier: Apache-2.0
//! Kept declaration IDs read their actual source without crediting ignored attributes.

use cadmpeg_core::decode::u64_from_index;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;
use std::ops::Range;

fn value_range(document: &str, name: &str, raw: &str) -> Range<u64> {
    let start = document.find(&format!("{name}='{raw}'")).unwrap() + name.len() + "='".len();
    u64_from_index(start)..u64_from_index(start + raw.len())
}

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
    let fidelity = decoded.source_fidelity();
    let layout = &fidelity.record_layout()[object.id()];
    assert_eq!(
        layout.offset(),
        u64_from_index(document.find(&declaration).unwrap())
    );
    assert_eq!(layout.byte_len(), u64_from_index(declaration.len()));
    let data_start = document.find(data).unwrap();
    assert_eq!(layout.payloads()["raw_xml"].as_ref(), data.as_bytes());
    let mut expected_reads = vec![
        value_range(&document, "name", "Owner"),
        value_range(&document, "type", "PartDesign::Feature"),
    ];
    if expected.is_some() {
        expected_reads.push(value_range(&document, "id", raw.unwrap()));
    }
    let data_name = data_start + "<Object name='".len();
    expected_reads.push(u64_from_index(data_name)..u64_from_index(data_name + "Owner".len()));
    assert_eq!(layout.reads()[layout.stream()], expected_reads);
    let unread = fidelity.b1_spans()[layout.stream()].spans();
    let vendor = value_range(&document, "Vendor", "opaque-extra");
    assert!(unread.iter().any(|span| {
        let range = span.range();
        range.start <= vendor.start && vendor.end <= range.end
    }));
    if let Some(raw) = raw {
        let id = value_range(&document, "id", raw);
        if expected.is_some() {
            assert!(!unread.iter().any(|span| {
                let range = span.range();
                range.start < id.end && id.start < range.end
            }));
        } else {
            assert!(unread.iter().any(|span| {
                let range = span.range();
                range.start <= id.start && id.end <= range.end
            }));
        }
    }
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
