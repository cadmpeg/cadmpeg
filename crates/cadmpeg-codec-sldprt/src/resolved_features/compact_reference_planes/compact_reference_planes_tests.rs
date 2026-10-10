//! Tests for the `compact_reference_planes` module.

use super::{compact_reference_plane_source, principal_sketch_frame, CompactReferencePlaneIndex};
use cadmpeg_ir::features::PrincipalPlane;
use cadmpeg_ir::math::{Point3, Vector3};

const EPS_PRINCIPAL_SKETCH_FRAME_ORTHONORMAL: f64 = 1.0e-12;

#[test]
fn compact_reference_plane_index_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let payload = b"moCompRefPlane_c";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy).expect("root");
    let error = CompactReferencePlaneIndex::new(&ctx, payload)
        .err()
        .expect("class offset exceeds collection limit");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "collect compact reference plane classes"
    ));
}

#[test]
fn compact_reference_plane_index_refuses_work_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let payload = b"moCompRefPlane_c";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::try_from(payload.len()).expect("fixture length") * 3 - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy).expect("root");
    let error = CompactReferencePlaneIndex::new(&ctx, payload)
        .err()
        .expect("index scan exceeds work limit");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "index compact reference planes"
    ));
}

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
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &payload,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("reference plane fixture fits service policy");
    let plane_index = CompactReferencePlaneIndex::new(&ctx, &payload)
        .expect("reference plane index fits service policy");

    assert_eq!(
        plane_index
            .profile_source(&ctx, profile_start, profile_start, payload.len())
            .unwrap(),
        Some(2)
    );
    assert_eq!(
        plane_index
            .profile_source(&ctx, component_start, component_start, payload.len())
            .unwrap(),
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

    let ctx = cadmpeg_test_support::service_decode_context();
    let index = CompactReferencePlaneIndex::new(&ctx, &payload).unwrap();
    assert_eq!(
        index
            .profile_component_frame(&ctx, 0, 0, payload.len())
            .unwrap(),
        Some((
            Point3::new(0.0, 0.0, -31.0),
            Vector3::new(0.0, -1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0)
        ))
    );
}

#[test]
fn indexed_profile_frames_do_not_bill_or_scan_the_whole_lane() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let frame = principal_sketch_frame(PrincipalPlane::Top);
    let records = 4096;
    let width = super::COMPACT_COMPONENT_PLANE_RECORD_LEN;
    let index = CompactReferencePlaneIndex {
        payload_len: records * width,
        class_offsets: Vec::new(),
        declared: Vec::new(),
        components: (0..records).map(|i| (i * width, 7)).collect(),
        component_frames: (0..records).map(|i| (i * width, frame)).collect(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 100_000;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for i in 0..1000 {
        let start = i * width;
        let end = start + width;
        assert_eq!(
            index.profile_source(&ctx, start, start, end).unwrap(),
            Some(7)
        );
        assert_eq!(
            index
                .profile_component_frame(&ctx, start, start, end)
                .unwrap(),
            Some(frame)
        );
    }
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn indexed_component_frames_keep_complete_boundaries_and_ambiguity() {
    let first = principal_sketch_frame(PrincipalPlane::Top);
    let second = principal_sketch_frame(PrincipalPlane::Front);
    let width = super::COMPACT_COMPONENT_PLANE_RECORD_LEN;
    let index = CompactReferencePlaneIndex {
        payload_len: width * 3,
        class_offsets: Vec::new(),
        declared: Vec::new(),
        components: vec![(0, 7), (width, 7), (width * 2, 9)],
        component_frames: vec![(0, first), (width, first), (width * 2, second)],
    };
    let ctx = cadmpeg_test_support::service_decode_context();
    assert_eq!(
        index
            .profile_component_frame(&ctx, 0, 0, width - 1)
            .unwrap(),
        None
    );
    assert_eq!(
        index.profile_component_frame(&ctx, 0, 1, width).unwrap(),
        Some(first)
    );
    assert_eq!(
        index
            .profile_component_frame(&ctx, 0, 0, width * 2)
            .unwrap(),
        Some(first)
    );
    assert_eq!(
        index
            .profile_component_frame(&ctx, 0, 0, width * 3)
            .unwrap(),
        None
    );
    assert_eq!(
        index
            .profile_component_frame(&ctx, 0, width * 2, width * 3)
            .unwrap(),
        Some(second)
    );
    assert_eq!(
        index
            .profile_component_frame(&ctx, 0, 0, width * 3 + 1)
            .unwrap(),
        None
    );
}

#[test]
fn indexed_reference_plane_lookup_preserves_work_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let index = CompactReferencePlaneIndex {
        payload_len: 138,
        class_offsets: Vec::new(),
        declared: Vec::new(),
        components: vec![(0, 7)],
        component_frames: Vec::new(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = index.profile_source(&ctx, 0, 0, 138)
    else {
        panic!("indexed comparisons require work admission");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}
