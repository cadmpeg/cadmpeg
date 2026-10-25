// SPDX-License-Identifier: Apache-2.0
//! Property identity fields are read without promoting their opaque contents.

use cadmpeg_core::decode::u64_from_index;
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
    assert!(property.values().is_empty());
    assert_eq!(
        property.id,
        crate::native::native_child_id(
            "property",
            &crate::native::native_id("object", "Owner"),
            "A&B"
        )
    );
    let fidelity = decoded.source_fidelity();
    let layout = &fidelity.record_layout()[&property.id];
    let element = roxmltree::Document::parse(document)
        .unwrap()
        .descendants()
        .find(|node| node.has_tag_name("Property"))
        .unwrap()
        .range();
    assert_eq!(
        crate::test_support::source_fidelity::bytes(fidelity, &property.id),
        &document.as_bytes()[element]
    );
    let range = |name: &str, raw: &str| {
        let start = document.find(&format!("{name}='{raw}'")).unwrap() + name.len() + "='".len();
        u64_from_index(start)..u64_from_index(start + raw.len())
    };
    let key_range = |key: &str, raw: &str| {
        let at = document.find(&format!("{key}='{raw}'")).unwrap();
        u64_from_index(at)..u64_from_index(at + key.len())
    };
    let expected = [
        key_range("name", "A&amp;B"),
        range("name", "A&amp;B"),
        key_range("type", "Vendor::Futu&#x72;e"),
        range("type", "Vendor::Futu&#x72;e"),
        key_range("status", "007"),
        range("status", "007"),
    ];
    assert_eq!(layout.reads()[layout.stream()], expected);
    let spans = fidelity.b1_spans()[layout.stream()].spans();
    for read in &expected {
        assert!(spans.iter().all(|span| {
            let unread = span.range();
            unread.end <= read.start || unread.start >= read.end
        }));
    }
    let opaque = range("value", "opaque-state");
    assert!(spans.iter().any(|span| {
        let unread = span.range();
        unread.start <= opaque.start && opaque.end <= unread.end
    }));
}
