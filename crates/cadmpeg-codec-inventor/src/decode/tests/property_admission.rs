// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::InventorCodec;

/// A one-section OLE property-set stream holding property id 2 with an i32
/// value of 17.
fn stream() -> Vec<u8> {
    let values = vec![[3_i32.to_le_bytes(), 17_i32.to_le_bytes()].concat()];
    let mut section = vec![0; 8 + 8 * values.len()];
    section[4..8].copy_from_slice(
        &u32::try_from(values.len())
            .expect("the property count fits u32")
            .to_le_bytes(),
    );
    for (index, value) in values.iter().enumerate() {
        section[8 + index * 8..12 + index * 8].copy_from_slice(
            &u32::try_from(index + 2)
                .expect("the property id fits u32")
                .to_le_bytes(),
        );
        let offset = u32::try_from(section.len()).expect("the value offset fits u32");
        section[12 + index * 8..16 + index * 8].copy_from_slice(&offset.to_le_bytes());
        section.extend_from_slice(value);
    }
    let size = u32::try_from(section.len()).expect("the section size fits u32");
    section[..4].copy_from_slice(&size.to_le_bytes());
    let mut bytes = vec![0; 48];
    bytes[..2].copy_from_slice(&0xfffe_u16.to_le_bytes());
    bytes[2..4].copy_from_slice(&1_u16.to_le_bytes());
    bytes[24..28].copy_from_slice(&1_u32.to_le_bytes());
    bytes[28..44].copy_from_slice(&crate::decode::FMTID_SUMMARY_INFORMATION);
    bytes[44..48].copy_from_slice(&48_u32.to_le_bytes());
    bytes.extend_from_slice(&section);
    bytes
}

#[test]
fn parsed_property_records_are_projected() {
    let bytes =
        crate::test_support::test_fixtures::fixture_with_property_set_peers(&stream(), &stream());

    let decoded = InventorCodec
        .decode(&mut std::io::Cursor::new(&bytes), &DecodeOptions::default())
        .expect("property projection");
    let namespace = decoded
        .ir()
        .native
        .namespace("inventor")
        .expect("native namespace");
    assert_eq!(
        namespace
            .arena_as::<serde_json::Value>("property_sets")
            .expect("set arena")
            .len(),
        2
    );
    assert_eq!(
        namespace
            .arena_as::<serde_json::Value>("property_sections")
            .expect("section arena")
            .len(),
        2
    );
    assert_eq!(
        namespace
            .arena_as::<serde_json::Value>("properties")
            .expect("value arena")
            .len(),
        2
    );
    assert!(
        !namespace.arenas().contains_key("property_set_issues"),
        "no failure arena survives a readable property set"
    );
}
