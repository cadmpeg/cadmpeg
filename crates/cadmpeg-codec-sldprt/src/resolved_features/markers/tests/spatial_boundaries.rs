//! Boundary tests for compact, wide, and indexed spatial point prefixes.

use super::super::super::SKETCH_MARKER;
use super::super::*;
use crate::layout::{
    compact_current_spatial_marker_point as compact_spatial,
    current_indexed_spatial_xyz_point_prefix as indexed_xyz_spatial,
    wide_spatial_marker_coordinate_prefix as wide_spatial,
};
use cadmpeg_ir::math::Point3;

pub(super) fn current_indexed_xyz_spatial_point(
    object_index: u32,
    coordinates: [f64; 3],
    next_offset: usize,
) -> Vec<u8> {
    let marker_offset = 4;
    let mut payload = vec![0; marker_offset + next_offset + SKETCH_MARKER.len()];
    payload[..marker_offset].copy_from_slice(&object_index.to_le_bytes());
    payload[marker_offset..marker_offset + indexed_xyz_spatial::HEADER]
        .copy_from_slice(SKETCH_MARKER);
    payload[marker_offset + indexed_xyz_spatial::HEADER
        ..marker_offset + indexed_xyz_spatial::SENTINEL]
        .fill(0xff);
    payload[marker_offset + indexed_xyz_spatial::SENTINEL
        ..marker_offset + indexed_xyz_spatial::NATIVE_KIND]
        .copy_from_slice(&(-1.0f32).to_le_bytes());
    payload[marker_offset + indexed_xyz_spatial::NATIVE_KIND
        ..marker_offset + indexed_xyz_spatial::NATIVE_KIND + 4]
        .copy_from_slice(&0u32.to_le_bytes());
    payload[marker_offset + indexed_xyz_spatial::PROFILE_LOCUS
        ..marker_offset + indexed_xyz_spatial::PROFILE_ROLE]
        .copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[marker_offset + indexed_xyz_spatial::PROFILE_ROLE
        ..marker_offset + indexed_xyz_spatial::PROFILE_ROLE + 2]
        .copy_from_slice(&1u16.to_le_bytes());
    payload[marker_offset + indexed_xyz_spatial::SELECTOR
        ..marker_offset + indexed_xyz_spatial::SELECTOR + 8]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[marker_offset + indexed_xyz_spatial::STATE_VALUE
        ..marker_offset + indexed_xyz_spatial::STATE_VALUE + 8]
        .copy_from_slice(&1.0f64.to_le_bytes());
    payload[marker_offset + indexed_xyz_spatial::COORDINATE_TAG
        ..marker_offset + indexed_xyz_spatial::COORDINATES]
        .copy_from_slice(&[0x0e, 0x00]);
    for (index, value) in coordinates.into_iter().enumerate() {
        let start = marker_offset + indexed_xyz_spatial::COORDINATES + index * 8;
        payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[marker_offset + indexed_xyz_spatial::TAIL_WORD_0
        ..marker_offset + indexed_xyz_spatial::TAIL_WORD_0 + 2]
        .copy_from_slice(&8u16.to_le_bytes());
    payload[marker_offset + indexed_xyz_spatial::TAIL_WORD_1
        ..marker_offset + indexed_xyz_spatial::TAIL_WORD_1 + 2]
        .copy_from_slice(&1u16.to_le_bytes());
    payload[marker_offset + indexed_xyz_spatial::TERMINATOR
        ..marker_offset + indexed_xyz_spatial::TERMINATOR + 4]
        .copy_from_slice(&[0xfe, 0xff, 0xff, 0xff]);
    let next = marker_offset + next_offset;
    payload[next..].copy_from_slice(SKETCH_MARKER);
    payload
}

fn compact_spatial_point() -> Vec<u8> {
    let mut payload = vec![0; compact_spatial::LEN];
    payload[compact_spatial::MARKER..compact_spatial::HEADER].copy_from_slice(SKETCH_MARKER);
    payload[compact_spatial::HEADER..compact_spatial::SENTINEL].fill(0xff);
    payload[compact_spatial::SENTINEL..compact_spatial::NATIVE_KIND]
        .copy_from_slice(&(-1.0f32).to_le_bytes());
    payload[compact_spatial::NATIVE_KIND..compact_spatial::NATIVE_KIND + 4]
        .copy_from_slice(&0u32.to_le_bytes());
    payload[compact_spatial::PROFILE_LOCUS..compact_spatial::PROFILE_LOCUS + 4]
        .copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[compact_spatial::PROFILE_ROLE..compact_spatial::PROFILE_ROLE + 2]
        .copy_from_slice(&1u16.to_le_bytes());
    payload[compact_spatial::SELECTOR..compact_spatial::SELECTOR + 8]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[compact_spatial::STATE_VALUE..compact_spatial::STATE_VALUE + 8]
        .copy_from_slice(&1.0f64.to_le_bytes());
    payload[compact_spatial::COORDINATE_TAG..compact_spatial::COORDINATE_TAG + 2]
        .copy_from_slice(&[0x0e, 0x00]);
    for (index, value) in [0.125_f64, -0.25, 0.375].into_iter().enumerate() {
        let start = compact_spatial::COORDINATES + index * 8;
        payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload
}

#[test]
fn compact_spatial_point_requires_the_declared_boundary() {
    let point = compact_spatial_point();
    let expected = Some(Point3::new(125.0, -250.0, 375.0));
    assert_eq!(marker_spatial_coordinates(&point, 0), expected);

    let mut direct_next_marker = point.clone();
    direct_next_marker.extend_from_slice(SKETCH_MARKER);
    assert_eq!(marker_spatial_coordinates(&direct_next_marker, 0), expected);

    let mut separated_next_marker = point.clone();
    separated_next_marker.extend_from_slice(&[0; 4]);
    separated_next_marker.extend_from_slice(SKETCH_MARKER);
    assert_eq!(
        marker_spatial_coordinates(&separated_next_marker, 0),
        expected
    );

    let mut missing_boundary = point;
    missing_boundary.extend_from_slice(&[0; 8]);
    assert_eq!(marker_spatial_coordinate_offset(&missing_boundary, 0), None);
}

#[test]
fn wide_spatial_point_wins_when_compact_tag_is_inside_its_prefix() {
    let mut payload = vec![0; wide_spatial::LEN + SKETCH_MARKER.len()];
    payload[compact_spatial::MARKER..compact_spatial::HEADER].copy_from_slice(SKETCH_MARKER);
    payload[compact_spatial::HEADER..compact_spatial::SENTINEL].fill(0xff);
    payload[compact_spatial::SENTINEL..compact_spatial::NATIVE_KIND]
        .copy_from_slice(&(-1.0f32).to_le_bytes());
    payload[compact_spatial::NATIVE_KIND..compact_spatial::NATIVE_KIND + 4]
        .copy_from_slice(&0u32.to_le_bytes());
    payload[compact_spatial::PROFILE_LOCUS..compact_spatial::PROFILE_LOCUS + 4]
        .copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[compact_spatial::PROFILE_ROLE..compact_spatial::PROFILE_ROLE + 2]
        .copy_from_slice(&1u16.to_le_bytes());
    payload[compact_spatial::SELECTOR..compact_spatial::SELECTOR + 8]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[compact_spatial::STATE_VALUE..compact_spatial::STATE_VALUE + 8]
        .copy_from_slice(&1.0f64.to_le_bytes());
    payload[compact_spatial::COORDINATE_TAG..compact_spatial::COORDINATE_TAG + 2]
        .copy_from_slice(&[0x0e, 0x00]);
    payload[wide_spatial::COORDINATE_TAG..wide_spatial::COORDINATE_TAG + 2]
        .copy_from_slice(&[0x0e, 0x00]);
    for (index, value) in [0.125_f64, -0.25, 0.375].into_iter().enumerate() {
        let start = wide_spatial::COORDINATES + index * 8;
        payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[wide_spatial::LEN..].copy_from_slice(SKETCH_MARKER);

    assert_eq!(
        marker_spatial_coordinates(&payload, 0),
        Some(Point3::new(125.0, -250.0, 375.0))
    );
}

#[test]
fn current_indexed_profile_spatial_point_requires_its_tail_boundary() {
    for next_offset in [158, 162] {
        let payload = current_indexed_xyz_spatial_point(2, [0.125, -0.25, 0.375], next_offset);
        assert_eq!(
            marker_spatial_coordinates(&payload, 4),
            Some(Point3::new(125.0, -250.0, 375.0))
        );
    }

    let mut missing_boundary = current_indexed_xyz_spatial_point(2, [0.125, -0.25, 0.375], 158);
    missing_boundary.truncate(4 + indexed_xyz_spatial::LEN);
    assert_eq!(marker_spatial_coordinates(&missing_boundary, 4), None);

    let mut unindexed = current_indexed_xyz_spatial_point(2, [0.125, -0.25, 0.375], 158);
    unindexed[..4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(marker_spatial_coordinates(&unindexed, 4), None);

    let mut relation = current_indexed_xyz_spatial_point(2, [0.125, -0.25, 0.375], 158);
    relation[4 + indexed_xyz_spatial::NATIVE_KIND..4 + indexed_xyz_spatial::NATIVE_KIND + 4]
        .copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(marker_spatial_coordinates(&relation, 4), None);
}
