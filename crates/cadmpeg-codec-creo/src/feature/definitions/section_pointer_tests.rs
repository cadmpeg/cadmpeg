// SPDX-License-Identifier: Apache-2.0

use super::positional_section_3d;

fn section(tail: &[u8]) -> super::FeatureSection3d {
    let mut payload = b"\x07S2D0001\0\x01".to_vec();
    payload.extend_from_slice(tail);
    positional_section_3d(&payload, 0, payload.len()).expect("section header")
}

#[test]
fn section_pointer_plain_empty_chain_preserves_outer_plane() {
    for (tail, expected) in [
        (vec![0xf6, 0xe1, 0xf6, 0x80, 0x87, 1], 135),
        (vec![0xf6, 0xe1, 0xf6, 0x98, 0x41, 1], 6209),
    ] {
        let parsed = section(&tail);
        assert_eq!(parsed.sketch_plane_entity_id, Some(expected));
        assert_eq!(parsed.sketch_plane_flip, Some(super::BinaryFlag::Set));
    }
}

#[test]
fn section_pointer_inline_body_does_not_supply_outer_plane() {
    let parsed = section(b"\xf6\xe3\xe0\x01attributes\0\x05\xf6\x82\xc4\x01");
    assert_eq!(parsed.sketch_plane_entity_id, None);
    assert_eq!(parsed.sketch_plane_flip, None);
    assert!(parsed.dimension_ids.is_empty());
}

#[test]
fn section_pointer_class_suffix_and_truncation_do_not_supply_plane() {
    for tail in [
        b"\xf6\xe1\xf7\x80\xc5\xf6\x83\x53\x01".as_slice(),
        b"\xf6\xe1\xf7".as_slice(),
        b"\xf6\xe1\x80".as_slice(),
        b"\xe3\xe1\xf6\x82\xc4".as_slice(),
    ] {
        let parsed = section(tail);
        assert_eq!(parsed.sketch_plane_entity_id, None);
        assert_eq!(parsed.sketch_plane_flip, None);
    }
}
