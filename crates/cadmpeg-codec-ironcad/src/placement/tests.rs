// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::{records, PropertyValue, NAME_MARKER, PROPERTY_MARKER, PROPERTY_SET, STRING_PROPERTY};
use crate::layout::scene_record_matrix;

fn name(text: &str) -> Vec<u8> {
    let mut out = NAME_MARKER.to_vec();
    out.push(u8::try_from(text.encode_utf16().count()).unwrap());
    out.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    out
}

fn feature(text: &str) -> Vec<u8> {
    let mut out = name(text);
    out.extend_from_slice(&[0, 0, 0, 0, 1, 0, 0, 0]);
    out
}

/// One string property.
fn property(name: &str, value: &str) -> Vec<u8> {
    let utf16 = |text: &str| -> Vec<u8> {
        format!("{text}\0")
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect()
    };
    let (name, value) = (utf16(name), utf16(value));
    let mut out = PROPERTY_MARKER.to_vec();
    out.extend_from_slice(&u32::try_from(name.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&name);
    out.extend_from_slice(&STRING_PROPERTY.to_le_bytes());
    out.extend_from_slice(&u32::try_from(value.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&value);
    out
}

/// A part or assembly record whose matrix holds `rows` and translation `t`.
fn scene_record(text: &str, rows: [[f64; 3]; 3], t: [f64; 3]) -> Vec<u8> {
    let mut out = name(text);
    out.extend_from_slice(&PROPERTY_SET);
    out.extend_from_slice(&[1, 0]);
    out.extend_from_slice(&NAME_MARKER);
    out.push(0);
    out.extend(property("ModelGUID", "{0}"));
    out.extend(std::iter::repeat_n(0, scene_record_matrix::MATRIX));
    for row in rows {
        for value in row {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&0.0_f64.to_le_bytes());
    }
    for value in t {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&1.0_f64.to_le_bytes());
    out
}

const IDENTITY: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

#[test]
fn part_records_follow_their_features_and_carry_row_vector_matrices() {
    let quarter_turn = [[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
    let payload = [
        feature("Block"),
        scene_record("Part", quarter_turn, [0.02, 0.03, 0.05]),
        scene_record("Assembly", IDENTITY, [0.0, 0.0, 0.0]),
    ]
    .concat();
    let found = records(&payload);
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].name, "Part");
    assert_eq!(
        found[0].property("ModelGUID"),
        Some(&PropertyValue::Text("{0}".to_owned()))
    );
    assert!(found[0].is_part());
    assert!(!found[1].is_part());
    let rows = found[0].transform.unwrap().affine_rows();
    assert_eq!(
        rows,
        [
            [0.0, -1.0, 0.0, 20.0],
            [1.0, 0.0, 0.0, 30.0],
            [0.0, 0.0, 1.0, 50.0]
        ]
    );
}

#[test]
fn a_matrix_without_an_affine_column_is_dropped() {
    let mut payload = [feature("Block"), scene_record("Part", IDENTITY, [0.0; 3])].concat();
    let last = payload.len() - 8;
    payload[last..].copy_from_slice(&2.0_f64.to_le_bytes());
    let found = records(&payload);
    assert_eq!(found.len(), 1);
    assert!(found[0].transform.is_none());
}

#[test]
fn a_property_of_another_value_type_ends_the_read_and_is_named() {
    let odd_name: Vec<u8> = "Odd\0".encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut odd = PROPERTY_MARKER.to_vec();
    odd.extend_from_slice(&u32::try_from(odd_name.len()).unwrap().to_le_bytes());
    odd.extend_from_slice(&odd_name);
    odd.extend_from_slice(&9_u16.to_le_bytes());
    odd.extend_from_slice(&[0; 8]);
    let payload = [
        feature("Block"),
        name("Part1"),
        PROPERTY_SET.to_vec(),
        property("Part Number", "P-1"),
        odd,
        property("Description", "after"),
    ]
    .concat();
    let scene = records(&payload);
    assert_eq!(scene.len(), 1);
    assert_eq!(scene[0].unread_property.as_deref(), Some("Odd"));
    let names: Vec<_> = scene[0]
        .properties
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(names, ["Part Number"]);
}
