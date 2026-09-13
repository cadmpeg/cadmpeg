//! Tests for the `names` module.

use super::super::CLASS_MARKER;
use super::object_names;
use crate::records::operand_tag::NativeOperandTag;

#[test]
fn object_names_follow_the_lane_name_class_token() {
    let mut payload = vec![0x42, 0, 0, 0, 0x13, 0];
    payload.extend_from_slice(CLASS_MARKER);
    payload.extend_from_slice(&18u16.to_le_bytes());
    payload.extend_from_slice(b"moFavoriteFolder_c");
    payload.extend_from_slice(&[0x87, 0x80, 0xff, 0xfe, 0xff]);
    payload.push(9);
    for unit in "Favorites".encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    payload.resize(payload.len() + 12, 0);
    payload.extend_from_slice(&[0x87, 0x80, 0xff, 0xfe, 0xff]);
    payload.push(4);
    for unit in "Boss".encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    payload.resize(payload.len() + 12, 0);

    let names = object_names(&payload, "lane");
    assert_eq!(
        names
            .iter()
            .map(|name| name.value.as_str())
            .collect::<Vec<_>>(),
        ["Favorites", "Boss"]
    );
}

#[test]
fn operand_kind_names_preserve_wire_spelling() {
    use crate::records::FeatureInputOperandKind;
    for (kind, expected) in [
        (FeatureInputOperandKind::D6, "d6"),
        (FeatureInputOperandKind::E1, "e1"),
        (
            FeatureInputOperandKind::Native(NativeOperandTag::TAG_80D5),
            "d580",
        ),
    ] {
        assert_eq!(super::operand_kind_name(kind).as_str(), expected);
    }
}

#[test]
fn marker_literals_keep_their_wire_spelling() {
    use cadmpeg_ir::nonblank_literal;
    assert_eq!(
        nonblank_literal!("sldprt:marker-local-id").as_str(),
        "sldprt:marker-local-id"
    );
    assert_eq!(
        nonblank_literal!("sldprt:marker-relation:{}", 34).as_str(),
        "sldprt:marker-relation:34"
    );
    assert_eq!(
        nonblank_literal!("sldprt:marker-geometry:{}", 2).as_str(),
        "sldprt:marker-geometry:2"
    );
}
