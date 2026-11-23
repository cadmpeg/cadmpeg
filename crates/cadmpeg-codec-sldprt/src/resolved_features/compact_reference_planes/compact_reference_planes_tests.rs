//! Tests for the `compact_reference_planes` module.

use super::{compact_reference_plane_source, principal_sketch_frame, CompactReferencePlaneIndex};
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
    let records = 16;
    let width = super::COMPACT_COMPONENT_PLANE_RECORD_LEN;
    let index = CompactReferencePlaneIndex {
        payload_len: records * width,
        class_end: None,
        declared: Vec::new(),
        components: (0..records).map(|i| (i * width, 7)).collect(),
        principal_components: Vec::new(),
        component_frames: (0..records).map(|i| (i * width, frame)).collect(),
    };
    for i in 0..records {
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
        class_end: None,
        declared: Vec::new(),
        components: vec![(0, 7), (width, 7), (width * 2, 9)],
        principal_components: Vec::new(),
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
        principal_sketch_frame(PrincipalPlane::Top),
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
        index.principal_profile_plane(0, record.len()),
        Some(PrincipalPlane::Top)
    );
    assert_eq!(index.principal_profile_plane(1, record.len()), None);
    assert_eq!(index.principal_profile_plane(0, record.len() - 1), None);
    // The trailer count word is 2, 3 or 4.
    for (count, expected) in [
        (1u32, None),
        (2, Some(PrincipalPlane::Top)),
        (3, Some(PrincipalPlane::Top)),
        (5, None),
    ] {
        let mut counted = record;
        counted[layout::TRAILER_KIND..layout::TRAILER_KIND + 4]
            .copy_from_slice(&count.to_le_bytes());
        assert_eq!(
            CompactReferencePlaneIndex::new(&counted).principal_profile_plane(0, counted.len()),
            expected,
            "trailer count {count}"
        );
    }
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
    assert_eq!(
        CompactReferencePlaneIndex::new(&right).principal_profile_plane(0, right.len()),
        Some(PrincipalPlane::Right)
    );
    // The selector names the plane; the source is the plane's feature number,
    // which is one higher in parts that number the planes from 3.
    let mut shifted_top = top;
    shifted_top[layout::SOURCE..layout::SOURCE + 4].copy_from_slice(&4u32.to_le_bytes());
    let mut shifted_right = right;
    shifted_right[layout::SOURCE..layout::SOURCE + 4].copy_from_slice(&5u32.to_le_bytes());
    assert_eq!(
        CompactReferencePlaneIndex::new(&shifted_top).principal_profile_plane(0, top.len()),
        Some(PrincipalPlane::Top)
    );
    assert_eq!(
        CompactReferencePlaneIndex::new(&shifted_right).principal_profile_plane(0, right.len()),
        Some(PrincipalPlane::Right)
    );
    let mut both_right = right.to_vec();
    both_right.extend_from_slice(&right);
    assert_eq!(
        CompactReferencePlaneIndex::new(&both_right).principal_profile_plane(0, both_right.len()),
        Some(PrincipalPlane::Right)
    );
    let mut mixed = top.to_vec();
    mixed.extend_from_slice(&right);
    assert_eq!(
        CompactReferencePlaneIndex::new(&mixed).principal_profile_plane(0, mixed.len()),
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
        CompactReferencePlaneIndex::new(&mixed).principal_profile_plane(0, mixed.len()),
        None
    );
    // A Right selector with the Top matrix is no principal record.
    record[layout::SELECTOR] = 1;
    assert_eq!(
        CompactReferencePlaneIndex::new(&record).principal_profile_plane(0, record.len()),
        None
    );
}

/// The padded form writes the plane's feature number one word before the
/// record, a time stamp at +0 and a zero word at +4 (`sldprt.md` §2). Its
/// selector names the plane as in the first form.
#[test]
fn padded_principal_component_record_names_its_plane_by_the_selector() {
    use crate::layout::compact_principal_plane_component_padded as layout;
    let mut record = [0u8; 142];
    record[layout::FEATURE_NUMBER..layout::FEATURE_NUMBER + 4].copy_from_slice(&4u32.to_le_bytes());
    record[layout::TIME_STAMP..layout::TIME_STAMP + 4]
        .copy_from_slice(&0x4d5e_6f70u32.to_le_bytes());
    record[layout::PADDING..layout::PADDING + 4]
        .copy_from_slice(&layout::PADDING_VALUE.to_le_bytes());
    record[layout::SELECTOR..layout::SELECTOR + 4].copy_from_slice(&1u32.to_le_bytes());
    record[layout::MATRIX_PRESENT] = 1;
    for (index, value) in [
        0.0f64, 0.0, 1.0, 0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0,
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
    // The 138-byte record starts at the time stamp, so an object that starts
    // there holds it whole.
    for start in [0, layout::TIME_STAMP] {
        assert_eq!(
            index.principal_profile_plane(start, record.len()),
            Some(PrincipalPlane::Right)
        );
    }
    assert_eq!(
        index.principal_profile_plane(layout::TIME_STAMP + 1, record.len()),
        None
    );
}

/// A lane that writes the `moCompRefPlane_c` class name writes it once, so a
/// later profile's own declared record follows the name without holding it; it
/// is still that profile's own record. The decoder reads no declared record in
/// a lane without the class name (DI-73).
#[test]
fn own_declared_record_after_the_lane_class_name_is_the_profile_source() {
    use super::SourceRoute;
    let declared = |source: u32| {
        let mut record = Vec::new();
        record.extend(source.to_le_bytes());
        record.extend(19u32.to_le_bytes());
        record.extend([0, 0, 3, 0]);
        record.extend([0; 27]);
        record.extend(1.0f64.to_le_bytes());
        record.extend([
            0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0xf9, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00,
            0x00, 0x65,
        ]);
        record.extend([0; 4]);
        record
    };
    let mut payload = b"moCompRefPlane_c".to_vec();
    payload.extend([0; 11]);
    payload.extend(declared(2));
    payload.extend([0xaa; 32]);
    let second = payload.len();
    payload.extend([0xbb; 16]);
    payload.extend(declared(2));
    payload.extend([0xaa; 32]);
    let index = CompactReferencePlaneIndex::new(&payload);
    assert_eq!(
        index.profile_source_route(0, second, payload.len()),
        Some((2, SourceRoute::Profile))
    );
    assert!(index.holds_plane_record(second, payload.len()));
    assert!(!index.holds_plane_record(second + 16 + 67, payload.len()));

    let unnamed = payload[b"moCompRefPlane_c".len()..].to_vec();
    let index = CompactReferencePlaneIndex::new(&unnamed);
    assert_eq!(index.profile_source_route(0, 0, unnamed.len()), None);
    assert!(!index.holds_plane_record(0, unnamed.len()));
}
