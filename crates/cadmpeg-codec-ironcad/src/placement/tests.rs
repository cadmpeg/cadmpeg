// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::{
    records, PropertyValue, CLASS_TAIL, EMPTY_PROPERTY, FEATURE, INTEGER_PROPERTY, NAME_MARKER,
    PART_CLASS, PART_COPY_CLASS, PROPERTY_MARKER, STRING_PROPERTY,
};
use crate::layout::{scene_record_matrix, scene_record_matrix_short};

fn name(text: &str) -> Vec<u8> {
    let mut out = NAME_MARKER.to_vec();
    out.push(u8::try_from(text.encode_utf16().count()).unwrap());
    out.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    out
}

fn feature(text: &str) -> Vec<u8> {
    let mut out = name(text);
    out.extend_from_slice(&FEATURE);
    out.extend_from_slice(&[0, 0]);
    out
}

/// A record name and the scene object header of `version` and `class`.
fn object(text: &str, version: u32, class: u32) -> Vec<u8> {
    let mut out = name(text);
    out.extend_from_slice(&version.to_le_bytes());
    out.extend_from_slice(&class.to_le_bytes());
    out.extend_from_slice(&CLASS_TAIL);
    out
}

fn utf16(text: &str) -> Vec<u8> {
    format!("{text}\0")
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect()
}

/// The marker, name, and value type of one property.
fn property_head(name: &str, kind: u16) -> Vec<u8> {
    let name = utf16(name);
    let mut out = PROPERTY_MARKER.to_vec();
    out.extend_from_slice(&u32::try_from(name.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&name);
    out.extend_from_slice(&kind.to_le_bytes());
    out
}

/// One string property.
fn property(name: &str, value: &str) -> Vec<u8> {
    let value = utf16(value);
    let mut out = property_head(name, STRING_PROPERTY);
    out.extend_from_slice(&u32::try_from(value.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&value);
    out
}

/// The 128 bytes of a row-vector matrix with rotation `rows` and
/// translation `t`.
fn matrix(rows: [[f64; 3]; 3], t: [f64; 3]) -> Vec<u8> {
    let mut out = Vec::new();
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

/// The long-form bytes between the `ModelGUID` text and the matrix.
fn long_gap() -> Vec<u8> {
    let mut out = vec![0; scene_record_matrix::MATRIX];
    out[scene_record_matrix::FLAG] = scene_record_matrix::FLAG_VALUE;
    out
}

/// A part or assembly record of version 2 whose long-form matrix holds
/// `rows` and translation `t`.
fn scene_record(text: &str, rows: [[f64; 3]; 3], t: [f64; 3]) -> Vec<u8> {
    let mut out = object(text, 2, PART_CLASS);
    out.extend_from_slice(&[1, 0]);
    out.extend_from_slice(&NAME_MARKER);
    out.push(0);
    out.extend(property("ModelGUID", "{0}"));
    out.extend(long_gap());
    out.extend(matrix(rows, t));
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
        object("Part1", 2, PART_CLASS),
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
    assert_eq!(names, ["Part Number", "Description"]);
}

#[test]
fn integer_properties_are_read_and_empty_properties_are_skipped() {
    let mut count = property_head("PartMassType", INTEGER_PROPERTY);
    count.extend_from_slice(&7_i32.to_le_bytes());
    let payload = [
        feature("Block"),
        object("Part1", 2, PART_CLASS),
        property_head("EndTaper", EMPTY_PROPERTY),
        count,
        property("ModelGUID", "{0}"),
    ]
    .concat();
    let scene = records(&payload);
    assert!(scene[0].unread_property.is_none());
    assert_eq!(
        scene[0].properties,
        [
            ("PartMassType".to_owned(), PropertyValue::Integer(7)),
            (
                "ModelGUID".to_owned(),
                PropertyValue::Text("{0}".to_owned())
            ),
        ]
    );
}

#[test]
fn the_short_form_places_the_matrix_after_a_flag_at_offset_32() {
    let mut gap = vec![0; scene_record_matrix_short::MATRIX];
    gap[scene_record_matrix_short::FLAG] = scene_record_matrix::FLAG_VALUE;
    let payload = [
        feature("Block"),
        object("Part1", 2, PART_CLASS),
        property("ModelGUID", "{0}"),
        gap,
        matrix(IDENTITY, [0.0, 0.0, 0.01]),
    ]
    .concat();
    let scene = records(&payload);
    let rows = scene[0].transform.unwrap().affine_rows();
    assert_eq!(rows[2], [0.0, 0.0, 1.0, 10.0]);
    assert!(!scene[0].matrix_searched);
}

#[test]
fn a_matrix_at_no_fixed_offset_is_found_by_a_search() {
    let mut embedded = vec![0; scene_record_matrix::MATRIX + 24];
    embedded[scene_record_matrix::FLAG] = 0x20;
    embedded.push(scene_record_matrix::FLAG_VALUE);
    let payload = [
        feature("Block"),
        object("Part1", 3, PART_CLASS),
        property("ModelGUID", "{0}"),
        embedded,
        matrix(IDENTITY, [0.0, 0.02, 0.0]),
    ]
    .concat();
    let scene = records(&payload);
    let rows = scene[0].transform.unwrap().affine_rows();
    assert_eq!(rows[1], [0.0, 1.0, 0.0, 20.0]);
    assert!(scene[0].matrix_searched);
}

#[test]
fn a_version_2_part_copy_record_is_a_part_and_a_version_1_copy_is_skipped() {
    let mut copy = object("Copy", 2, PART_COPY_CLASS);
    copy.extend(property("ModelGUID", "{1}"));
    copy.extend(vec![0; scene_record_matrix::FLAG]);
    copy.extend(object("", 2, PART_CLASS).split_off(NAME_MARKER.len() + 1));
    copy.extend(property("PartName", "Copied"));
    copy.push(scene_record_matrix::FLAG_VALUE);
    copy.extend(matrix(IDENTITY, [0.03, 0.0, 0.0]));
    let mut link = object("Link", 1, PART_COPY_CLASS);
    link.extend(property("ModelGUID", "{2}"));
    let payload = [
        feature("Block"),
        scene_record("Part", IDENTITY, [0.0; 3]),
        copy,
        link,
        scene_record("Assembly", IDENTITY, [0.0; 3]),
    ]
    .concat();
    let scene = records(&payload);
    let summary: Vec<_> = scene
        .iter()
        .map(|record| (record.name.as_str(), record.is_part()))
        .collect();
    assert_eq!(
        summary,
        [("Part", true), ("Copy", true), ("Assembly", false)]
    );
    assert_eq!(
        scene[1].property("PartName"),
        Some(&PropertyValue::Text("Copied".to_owned()))
    );
    assert_eq!(scene[1].transform.unwrap().affine_rows()[0][3], 30.0);
    assert!(scene[1].matrix_searched);
}

#[test]
fn a_record_of_another_class_does_not_count_as_a_feature() {
    let mut other = object("Hole", 1, 0x3de3_0037);
    other.extend_from_slice(&[0; 8]);
    let payload = [
        feature("Block"),
        scene_record("Part", IDENTITY, [0.0; 3]),
        other,
        scene_record("Assembly", IDENTITY, [0.0; 3]),
    ]
    .concat();
    let parts: Vec<_> = records(&payload)
        .iter()
        .map(super::SceneRecord::is_part)
        .collect();
    assert_eq!(parts, [true, false]);
}
