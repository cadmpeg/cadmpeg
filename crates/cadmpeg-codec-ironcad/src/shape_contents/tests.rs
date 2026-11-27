// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::{objects, ObjectKind, SceneObject, BODY_ITEM, MEMBER_ITEM};
use crate::layout::{shape_object_header as header, shape_object_item as item};
use crate::placement::NAME_MARKER;

/// One item of `kind`.
fn item_of(kind: u16) -> Vec<u8> {
    let mut out = vec![0; item::LEN];
    out[item::TAG..item::TAG + 4].copy_from_slice(&item::TAG_VALUE.to_le_bytes());
    out[item::HANDLE..item::HANDLE + 8].copy_from_slice(&1_u64.to_le_bytes());
    out[item::KIND..item::KIND + 2].copy_from_slice(&kind.to_le_bytes());
    out[item::COUNT..item::COUNT + 2].copy_from_slice(&item::COUNT_VALUE.to_le_bytes());
    out
}

/// The terminating item.
fn terminator() -> Vec<u8> {
    let mut out = vec![0; item::LEN];
    out[item::TAG..item::TAG + 4].copy_from_slice(&item::TAG_VALUE.to_le_bytes());
    out[item::HANDLE..item::HANDLE + 8].copy_from_slice(&u64::MAX.to_le_bytes());
    out
}

/// A scene object entry.
fn entry(id: u32, name: &str, end: u64, kinds: &[u16]) -> Vec<u8> {
    let mut out = vec![0; header::LEN];
    out[..4].copy_from_slice(&0x26d_u32.to_le_bytes());
    out[header::END..header::END + 8].copy_from_slice(&end.to_le_bytes());
    out[header::ID..header::ID + 4].copy_from_slice(&id.to_le_bytes());
    out.extend_from_slice(&NAME_MARKER);
    out.push(u8::try_from(name.encode_utf16().count()).unwrap());
    out.extend(name.encode_utf16().flat_map(u16::to_le_bytes));
    out.extend_from_slice(&0_u32.to_le_bytes());
    for kind in kinds {
        out.extend(item_of(*kind));
    }
    out.extend(terminator());
    out
}

#[test]
fn objects_are_read_with_their_identifier_end_and_kind() {
    let payload = [
        entry(0x31, "Part1", 16_528, &[5, 4, BODY_ITEM, MEMBER_ITEM]),
        entry(0x38, "Assembly1", 126_579, &[5, 4, MEMBER_ITEM]),
        entry(0x3b, "Pattern59", 143_099, &[5, 4, 83]),
    ]
    .concat();
    assert_eq!(
        objects(&payload),
        [
            SceneObject {
                id: 0x31,
                name: "Part1".to_owned(),
                end: 16_528,
                kind: ObjectKind::Body,
            },
            SceneObject {
                id: 0x38,
                name: "Assembly1".to_owned(),
                end: 126_579,
                kind: ObjectKind::Assembly,
            },
            SceneObject {
                id: 0x3b,
                name: "Pattern59".to_owned(),
                end: 143_099,
                kind: ObjectKind::Other,
            },
        ]
    );
}

#[test]
fn an_entry_whose_item_breaks_its_form_is_skipped() {
    let mut broken = entry(0x31, "Part1", 16_528, &[BODY_ITEM]);
    let first_item = broken.len() - 2 * item::LEN;
    broken[first_item + item::COUNT] = 2;
    let payload = [broken, entry(0x32, "Part2", 33_077, &[BODY_ITEM])].concat();
    let found: Vec<_> = objects(&payload)
        .into_iter()
        .map(|object| object.id)
        .collect();
    assert_eq!(found, [0x32]);
}

#[test]
fn a_name_without_an_entry_header_is_not_an_object() {
    let mut payload = vec![1; header::LEN];
    payload.extend_from_slice(&NAME_MARKER);
    payload.push(1);
    payload.extend_from_slice(&u16::from(b'A').to_le_bytes());
    payload.extend_from_slice(&0_u32.to_le_bytes());
    payload.extend(terminator());
    assert!(objects(&payload).is_empty());
}
