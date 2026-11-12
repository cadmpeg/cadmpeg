//! Tests for the `compact_reference_planes` module.

use super::{
    compact_reference_plane_source, principal_sketch_frame, principal_top_component_frame,
    CompactReferencePlaneIndex,
};
use cadmpeg_ir::features::PrincipalPlane;
use cadmpeg_ir::math::{Point3, Vector3};

const EPS_PRINCIPAL_SKETCH_FRAME_ORTHONORMAL: f64 = 1.0e-12;

#[test]
fn every_principal_plane_has_a_sketch_frame() {
    for plane in [
        PrincipalPlane::Front,
        PrincipalPlane::Top,
        PrincipalPlane::Right,
    ] {
        let (_, normal, u_axis) = principal_sketch_frame(plane);
        assert!((normal.dot(normal) - 1.0).abs() <= EPS_PRINCIPAL_SKETCH_FRAME_ORTHONORMAL);
        assert!((u_axis.dot(u_axis) - 1.0).abs() <= EPS_PRINCIPAL_SKETCH_FRAME_ORTHONORMAL);
        assert!(normal.dot(u_axis).abs() <= EPS_PRINCIPAL_SKETCH_FRAME_ORTHONORMAL);
    }
}

#[test]
fn compact_reference_plane_source_requires_the_complete_trailer() {
    let mut payload = b"moCompRefPlane_c".to_vec();
    payload.extend([0; 12]);
    let start = payload.len();
    payload.extend(2u32.to_le_bytes());
    payload.extend(0x6554_f1b8_u32.to_le_bytes());
    payload.extend([0, 0, 3, 0]);
    payload.extend([0; 27]);
    payload.extend(1.0f64.to_le_bytes());
    payload.extend([
        0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0xf9, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00,
        0x65,
    ]);
    payload.extend([0; 4]);
    assert_eq!(compact_reference_plane_source(&payload), Some(2));
    payload[start + 50] = 3;
    payload[start + 54] = 0xff;
    assert_eq!(compact_reference_plane_source(&payload), Some(2));
    payload[start + 50] = 1;
    assert_eq!(compact_reference_plane_source(&payload), None);
    payload[start + 50] = 3;
    payload[start + 59] ^= 1;
    assert_eq!(compact_reference_plane_source(&payload), None);
}

#[test]
fn compact_legacy_reference_plane_source_uses_the_embedded_u16_id() {
    let mut payload = b"moCompRefPlane_c".to_vec();
    payload.extend([0; 12]);
    let start = payload.len();
    payload.extend(0x4f96_6817u32.to_le_bytes());
    payload.extend([0; 6]);
    payload.extend(3u16.to_le_bytes());
    payload.extend([0; 27]);
    payload.extend(1.0f64.to_le_bytes());
    payload.extend([
        0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0xf9, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00,
        0x65,
    ]);
    payload.extend([0; 4]);

    assert_eq!(compact_reference_plane_source(&payload), Some(3));
    payload[start + 10..start + 12].fill(0);
    assert_eq!(compact_reference_plane_source(&payload), None);
}

#[test]
fn compact_profile_uses_a_unique_lane_scoped_reference_plane() {
    let mut payload = b"moCompRefPlane_c".to_vec();
    payload.extend([0; 11]);
    payload.extend(2u32.to_le_bytes());
    payload.extend(19u32.to_le_bytes());
    payload.extend([0, 0, 3, 0]);
    payload.extend([0; 27]);
    payload.extend(1.0f64.to_le_bytes());
    payload.extend([
        0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0xf9, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00,
        0x65,
    ]);
    payload.extend([0; 80]);
    let component_start = payload.len();
    let mut component = [0u8; 138];
    component[..4].copy_from_slice(&549u32.to_le_bytes());
    component[14] = 1;
    for (offset, value) in [
        (15, 1.0),
        (23, 0.0),
        (31, 0.0),
        (39, 0.0),
        (47, 1.0),
        (55, 0.0),
        (63, 0.0),
        (71, 0.0),
        (79, 1.0),
    ] {
        component[offset..offset + 8].copy_from_slice(&f64::to_le_bytes(value));
    }
    component[122..126].copy_from_slice(&4u32.to_le_bytes());
    component[126..130].fill(0xff);
    payload.extend(component);
    let profile_start = payload.len();
    payload.extend([0xaa; 64]);
    let plane_index = CompactReferencePlaneIndex::new(&payload);

    assert_eq!(
        plane_index.profile_source(profile_start, profile_start, payload.len()),
        Some(2)
    );
    assert_eq!(
        plane_index.profile_source(component_start, component_start, payload.len()),
        Some(549)
    );
}

#[test]
fn compact_component_matrix_places_a_sketch_plane() {
    let mut payload = vec![0; 138];
    payload[..4].copy_from_slice(&89u32.to_le_bytes());
    payload[14] = 1;
    for (index, value) in [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 0.0, -0.031, 1.0,
    ]
    .into_iter()
    .enumerate()
    {
        let offset = 15 + index * 8;
        payload[offset..offset + 8].copy_from_slice(&f64::to_le_bytes(value));
    }
    payload[122..126].copy_from_slice(&4u32.to_le_bytes());
    payload[126..130].copy_from_slice(&[0xff; 4]);

    let index = CompactReferencePlaneIndex::new(&payload);
    assert_eq!(
        index.profile_component_frame(0, 0, payload.len()),
        Some((
            Point3::new(0.0, 0.0, -31.0),
            Vector3::new(0.0, -1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0)
        ))
    );
}

#[test]
fn indexed_profile_frames_resolve_each_record_interval() {
    let frame = principal_sketch_frame(PrincipalPlane::Top);
    let records = 4096;
    let width = super::COMPACT_COMPONENT_PLANE_RECORD_LEN;
    let index = CompactReferencePlaneIndex {
        payload_len: records * width,
        class_offsets: Vec::new(),
        declared: Vec::new(),
        components: (0..records).map(|i| (i * width, 7)).collect(),
        deferred_principal_components: Vec::new(),
        top_components: Vec::new(),
        component_frames: (0..records).map(|i| (i * width, frame)).collect(),
    };
    for i in 0..1000 {
        let start = i * width;
        let end = start + width;
        assert_eq!(index.profile_source(start, start, end), Some(7));
        assert_eq!(
            index.profile_component_frame(start, start, end),
            Some(frame)
        );
    }
}

#[test]
fn indexed_component_frames_keep_complete_boundaries_and_ambiguity() {
    let first = principal_sketch_frame(PrincipalPlane::Right);
    let second = principal_sketch_frame(PrincipalPlane::Front);
    let width = super::COMPACT_COMPONENT_PLANE_RECORD_LEN;
    let index = CompactReferencePlaneIndex {
        payload_len: width * 3,
        class_offsets: Vec::new(),
        declared: Vec::new(),
        components: vec![(0, 7), (width, 7), (width * 2, 9)],
        deferred_principal_components: Vec::new(),
        top_components: Vec::new(),
        component_frames: vec![(0, first), (width, first), (width * 2, second)],
    };
    assert_eq!(index.profile_component_frame(0, 0, width - 1), None);
    assert_eq!(index.profile_component_frame(0, 1, width), Some(first));
    assert_eq!(index.profile_component_frame(0, 0, width * 2), Some(first));
    assert_eq!(index.profile_component_frame(0, 0, width * 3), None);
    assert_eq!(
        index.profile_component_frame(0, width * 2, width * 3),
        Some(second)
    );
    assert_eq!(index.profile_component_frame(0, 0, width * 3 + 1), None);
}

#[test]
fn front_frame_uses_model_xy_with_positive_z_normal() {
    let (origin, normal, u_axis) = principal_sketch_frame(PrincipalPlane::Front);
    assert_eq!(origin, Point3::new(0.0, 0.0, 0.0));
    assert_eq!(normal, Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(u_axis, Vector3::new(1.0, 0.0, 0.0));
    assert_eq!(normal.cross(u_axis), Vector3::new(0.0, 1.0, 0.0));
}

#[test]
fn principal_top_uses_positive_y_normal() {
    assert_eq!(
        principal_top_component_frame(),
        (
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0)
        )
    );
    assert_eq!(
        principal_sketch_frame(PrincipalPlane::Right),
        (
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, -1.0)
        )
    );
}

#[test]
fn top_component_frame_requires_a_complete_local_unmixed_record() {
    use crate::layout::compact_principal_plane_component as layout;
    let mut record = [0u8; super::COMPACT_COMPONENT_PLANE_RECORD_LEN];
    record[layout::SOURCE..layout::SOURCE + 4].copy_from_slice(&3u32.to_le_bytes());
    record[layout::SELECTOR..layout::SELECTOR + 4].copy_from_slice(&2u32.to_le_bytes());
    record[layout::MATRIX_PRESENT] = 1;
    for (index, value) in [
        1.0f64, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
    .into_iter()
    .enumerate()
    {
        let offset = layout::BASIS + index * 8;
        record[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    record[layout::TRAILER_KIND..layout::TRAILER_KIND + 4].copy_from_slice(&4u32.to_le_bytes());
    record[layout::TRAILER_SENTINEL..layout::TRAILER_SENTINEL + 4].fill(0xff);
    let index = CompactReferencePlaneIndex::new(&record);
    assert_eq!(
        index.principal_top_profile_frame(0, record.len()),
        Some(principal_top_component_frame())
    );
    assert_eq!(index.principal_top_profile_frame(1, record.len()), None);
    assert_eq!(index.principal_top_profile_frame(0, record.len() - 1), None);
    let top = record;
    let mut right = record;
    right[layout::SOURCE..layout::SOURCE + 4].copy_from_slice(&4u32.to_le_bytes());
    right[layout::SELECTOR..layout::SELECTOR + 4].copy_from_slice(&1u32.to_le_bytes());
    for (index, value) in [0.0f64, 0.0, 1.0, 0.0, 1.0, 0.0, -1.0, 0.0, 0.0]
        .into_iter()
        .enumerate()
    {
        let offset = layout::BASIS + index * 8;
        right[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    let mut mixed = top.to_vec();
    mixed.extend_from_slice(&right);
    assert_eq!(
        CompactReferencePlaneIndex::new(&mixed).principal_top_profile_frame(0, mixed.len()),
        None
    );
    let mut custom = top;
    custom[layout::SELECTOR..layout::SELECTOR + 4].fill(0);
    for (index, value) in [1.0f64, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 1.0, 0.0]
        .into_iter()
        .enumerate()
    {
        let offset = layout::BASIS + index * 8;
        custom[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    let mut mixed = top.to_vec();
    mixed.extend_from_slice(&custom);
    assert_eq!(
        CompactReferencePlaneIndex::new(&mixed).principal_top_profile_frame(0, mixed.len()),
        None
    );
    record[layout::SELECTOR] = 1;
    assert_eq!(
        CompactReferencePlaneIndex::new(&record).principal_top_profile_frame(0, record.len()),
        None
    );
}

#[test]
fn deferred_principal_planes_retain_existing_frames() {
    assert_eq!(
        principal_sketch_frame(PrincipalPlane::Top),
        (
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0)
        )
    );
    assert_eq!(
        principal_sketch_frame(PrincipalPlane::Right),
        (
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, -1.0)
        )
    );
}
