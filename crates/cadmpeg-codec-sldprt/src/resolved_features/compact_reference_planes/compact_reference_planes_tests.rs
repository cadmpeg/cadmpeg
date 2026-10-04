//! Tests for the `compact_reference_planes` module.

use super::{
    compact_component_plane_frame, compact_profile_component_plane_frame,
    compact_reference_plane_source, principal_sketch_frame, CompactReferencePlaneIndex,
};
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
fn compact_profile_source_refuses_class_scan_work_limit() {
    use cadmpeg_core::decode::{
        DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, ResourceFailure,
    };

    let payload = b"moCompRefPlane_c";
    let index_arena = DecodeArena::new();
    let (index_ctx, _) = DecodeContext::from_root_bytes(
        payload,
        &index_arena,
        &DecodePolicy::service(),
    )
    .expect("index context");
    let index = CompactReferencePlaneIndex::new(&index_ctx, payload).expect("index");

    let query_arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The lookup must admit its one class-offset slot before filtering.
    policy.limits.max_work_units = 0;
    let (query_ctx, _) = DecodeContext::from_root_bytes(payload, &query_arena, &policy)
        .expect("query context");
    let error = index
        .profile_source(&query_ctx, 0, 0, payload.len())
        .expect_err("class offset scan exceeds work limit");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.reason == ResourceFailure::BudgetExceeded
                && limit.limit == 0
                && limit.used == 0
                && limit.additional == 1
                && limit.operation == "count compact reference plane classes"
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
    assert_eq!(
        compact_reference_plane_source(&payload).expect("source lookup"),
        Some(2)
    );
    payload[start + 50] = 3;
    payload[start + 54] = 0xff;
    assert_eq!(
        compact_reference_plane_source(&payload).expect("source lookup"),
        Some(2)
    );
    payload[start + 50] = 1;
    assert_eq!(
        compact_reference_plane_source(&payload).expect("source lookup"),
        None
    );
    payload[start + 50] = 3;
    payload[start + 59] ^= 1;
    assert_eq!(
        compact_reference_plane_source(&payload).expect("source lookup"),
        None
    );
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

    assert_eq!(
        compact_reference_plane_source(&payload).expect("source lookup"),
        Some(3)
    );
    payload[start + 10..start + 12].fill(0);
    assert_eq!(
        compact_reference_plane_source(&payload).expect("source lookup"),
        None
    );
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
            .expect("profile source"),
        Some(2)
    );
    assert_eq!(
        plane_index
            .profile_source(&ctx, component_start, component_start, payload.len())
            .expect("component source"),
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

    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &payload,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("component plane context");
    assert_eq!(
        compact_component_plane_frame(&ctx, &payload).expect("component plane frame"),
        Some((
            Point3::new(0.0, 0.0, -31.0),
            Vector3::new(0.0, -1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0)
        ))
    );
}

#[test]
fn compact_profile_component_plane_frame_refuses_window_scan_work_limit() {
    use cadmpeg_core::decode::{
        DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, ResourceFailure,
    };

    let payload = vec![0; 138];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The scan admits all 138 byte-source slots before window filtering.
    policy.limits.max_work_units = u64::try_from(payload.len()).expect("fixture length") - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy).expect("context");
    let error = compact_profile_component_plane_frame(&ctx, &payload, 0, 0, payload.len())
        .expect_err("window scan exceeds work limit");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.reason == ResourceFailure::BudgetExceeded
                && limit.limit == 137
                && limit.used == 0
                && limit.additional == 138
                && limit.operation == "scan compact component plane frames"
    ));
}
