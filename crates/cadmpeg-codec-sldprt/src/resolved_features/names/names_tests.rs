//! Tests for the `names` module.

use super::super::CLASS_MARKER;
use super::object_names;
use crate::records::operand_tag::NativeOperandTag;

#[test]
fn empty_object_name_requires_a_typed_identifier() {
    let mut payload = Vec::new();
    payload.extend_from_slice(CLASS_MARKER);
    payload.extend_from_slice(&18_u16.to_le_bytes());
    payload.extend_from_slice(b"moFavoriteFolder_c");
    payload.extend_from_slice(&[0x87, 0x80, 0xff, 0xfe, 0xff, 0]);
    payload.extend_from_slice(&[0; 8]);
    payload.extend_from_slice(&7_u32.to_le_bytes());
    payload.extend_from_slice(&[0x87, 0x80, 0xff, 0xfe, 0xff, 0]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&payload, &arena, &policy).unwrap();
    let names = object_names(&ctx, &payload, "lane").unwrap();
    assert_eq!(names.len(), 1);
    assert_eq!(names[0].value, "");
    assert_eq!(names[0].object_id.and_then(crate::records::ObjectId::value), Some(7));
}

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

    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&payload, &arena, &policy)
        .expect("test context");
    let names = object_names(&ctx, &payload, "lane").unwrap();
    assert_eq!(
        names
            .iter()
            .map(|name| name.value.as_str())
            .collect::<Vec<_>>(),
        ["Favorites", "Boss"]
    );
}

#[test]
fn object_names_accept_the_name_length_field() {
    let mut payload = Vec::new();
    payload.extend_from_slice(CLASS_MARKER);
    payload.extend_from_slice(&18u16.to_le_bytes());
    payload.extend_from_slice(b"moFavoriteFolder_c");
    let expected = [
        "A".repeat(129),
        "B".repeat(254),
        "C".repeat(255),
        "D".repeat(256),
    ];
    for name in &expected {
        payload.extend_from_slice(&[0x87, 0x80, 0xff, 0xfe, 0xff]);
        super::append_name_length(&mut payload, u16::try_from(name.len()).unwrap());
        for unit in name.encode_utf16() {
            payload.extend_from_slice(&unit.to_le_bytes());
        }
        payload.extend_from_slice(&[0; 12]);
    }
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&payload, &arena, &policy).unwrap();
    let names = object_names(&ctx, &payload, "lane").unwrap();
    assert_eq!(
        names.iter().map(|name| &name.value).collect::<Vec<_>>(),
        expected.iter().collect::<Vec<_>>()
    );
}

#[test]
fn class_declarations_accept_the_class_length_field() {
    let class = "C".repeat(129);
    let mut payload = Vec::new();
    payload.extend_from_slice(CLASS_MARKER);
    payload.extend_from_slice(&u16::try_from(class.len()).unwrap().to_le_bytes());
    payload.extend_from_slice(class.as_bytes());
    payload.extend_from_slice(&[0x87, 0x80, 0xff, 0xfe, 0xff, 1, b'N', 0]);
    payload.extend_from_slice(&[0; 12]);
    assert_eq!(
        super::payload_classes(&payload).collect::<Vec<_>>(),
        [(0, class.as_str())]
    );
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&payload, &arena, &policy).unwrap();
    let names = object_names(&ctx, &payload, "lane").unwrap();
    assert_eq!(names.len(), 1);
    assert_eq!(names[0].value, "N");
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
    use cadmpeg_core::nonblank_literal;
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
