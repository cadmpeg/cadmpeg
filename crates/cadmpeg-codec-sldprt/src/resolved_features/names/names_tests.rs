//! Tests for the `names` module.

use super::super::CLASS_MARKER;
use super::object_names;

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
