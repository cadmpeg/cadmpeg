// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn mesh_side_wrapper_degrades_truncated_present_slot_without_losing_parent() {
    let bytes = anonymous(&[1]);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
    let mut warnings = Diagnostics::new();
    let (slots, _) = with_test_context(&bytes, |ctx| {
        read_mesh_sides(
            ctx,
            &bytes,
            &mut reader,
            ArchiveVersion::V5,
            1,
            &mut warnings,
        )
    })
    .expect("degraded cache");
    assert!(slots[0].is_none());
    assert!(!warnings.is_empty());
    assert_eq!(reader.remaining(), 0);
}

#[test]
fn degraded_mesh_slots_refuse_collection_limit_without_warning() {
    let bytes = anonymous(&[1]);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
    let mut warnings = Diagnostics::new();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("test input fits service profile");
    let error = read_mesh_sides(
        &ctx,
        &bytes,
        &mut reader,
        ArchiveVersion::V5,
        1,
        &mut warnings,
    )
    .expect_err("one degraded mesh slot exceeds the remaining collection items");
    assert!(matches!(
        error,
        GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino Brep degraded mesh slots"
    ));
    assert!(warnings.is_empty());
}

#[test]
fn parsed_mesh_slots_refuse_collection_limit_without_warning() {
    let bytes = anonymous(&[0]);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
    let mut warnings = Diagnostics::new();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("test input fits service profile");
    let error = read_mesh_sides(
        &ctx,
        &bytes,
        &mut reader,
        ArchiveVersion::V5,
        1,
        &mut warnings,
    )
    .expect_err("one parsed mesh slot exceeds zero collection items");
    assert!(matches!(
        error,
        GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino Brep mesh cache slots"
    ));
    assert!(warnings.is_empty());
}

#[test]
fn mesh_child_ranges_refuse_collection_limit_without_warning() {
    let presence = [1_u8];
    let wrapper = mesh_class_wrapper_with_userdata();
    let bytes = anonymous_mixed(&[(&presence, false), (&wrapper, true)]);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
    let mut warnings = Diagnostics::new();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("test input fits service profile");
    let error = read_mesh_sides(
        &ctx,
        &bytes,
        &mut reader,
        ArchiveVersion::V5,
        1,
        &mut warnings,
    )
    .expect_err("one child range exceeds the remaining collection items");
    assert!(matches!(
        error,
        GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino Brep mesh cache child ranges"
    ));
    assert!(warnings.is_empty());
}

#[test]
fn legacy_mesh_side_degrades_truncated_present_slot() {
    let bytes = [1_u8];
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
    let mut warnings = Diagnostics::new();
    let (slots, range) = with_test_context(&bytes, |ctx| {
        read_legacy_mesh_sides(
            ctx,
            &bytes,
            &mut reader,
            ArchiveVersion::V5,
            1,
            &mut warnings,
        )
    })
    .expect("legacy cache degradation");
    assert_eq!(range, 0..bytes.len());
    assert_eq!(slots.len(), 1);
    assert!(slots[0].is_none());
    assert!(!warnings.is_empty());
    assert_eq!(reader.remaining(), 0);
}

#[test]
fn legacy_brep_mesh_slots_refuse_collection_limit() {
    let bytes = [0_u8];
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
    let mut warnings = Diagnostics::new();
    let error = with_collection_limit(&bytes, 0, |ctx| {
        read_legacy_mesh_sides(
            ctx,
            &bytes,
            &mut reader,
            ArchiveVersion::V5,
            1,
            &mut warnings,
        )
    })
    .expect_err("one legacy mesh slot exceeds zero collection items");
    assert!(
        matches!(error, GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino legacy Brep mesh slots")
    );
    assert!(warnings.is_empty());
}

#[test]
fn legacy_brep_degraded_mesh_slots_refuse_without_warning() {
    let bytes = [1_u8];
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
    let mut warnings = Diagnostics::new();
    let error = with_collection_limit(&bytes, 1, |ctx| {
        read_legacy_mesh_sides(
            ctx,
            &bytes,
            &mut reader,
            ArchiveVersion::V5,
            1,
            &mut warnings,
        )
    })
    .expect_err("degraded mesh slot exceeds one parsed slot item");
    assert!(
        matches!(error, GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino legacy Brep degraded mesh slots")
    );
    assert!(warnings.is_empty());
}

#[test]
fn mesh_side_wrapper_starts_with_face_zero_presence() {
    let bytes = anonymous(&[0]);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
    let mut warnings = Diagnostics::new();
    let (slots, _) = with_test_context(&bytes, |ctx| {
        read_mesh_sides(
            ctx,
            &bytes,
            &mut reader,
            ArchiveVersion::V5,
            1,
            &mut warnings,
        )
    })
    .expect("empty cache slot");
    assert_eq!(slots.len(), 1);
    assert!(slots[0].is_none());
    assert!(warnings.is_empty());
    assert_eq!(reader.remaining(), 0);
}

#[test]
fn mesh_side_wrapper_retains_nested_class_userdata() {
    let presence = [1_u8];
    let wrapper = mesh_class_wrapper_with_userdata();
    let bytes = anonymous_mixed(&[(&presence, false), (&wrapper, true)]);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
    let mut warnings = Diagnostics::new();
    let (slots, _) = with_test_context(&bytes, |ctx| {
        read_mesh_sides(
            ctx,
            &bytes,
            &mut reader,
            ArchiveVersion::V5,
            1,
            &mut warnings,
        )
    })
    .expect("mesh cache with userdata");
    assert_eq!(slots.len(), 1);
    assert!(slots[0].is_some(), "warnings: {warnings:?}");
    assert_eq!(slots[0].as_ref().unwrap().userdata.len(), 1);
    assert_eq!(
        slots[0].as_ref().unwrap().userdata[0]
            .known()
            .unwrap()
            .item_uuid,
        crate::mesh::V5_MESH_DOUBLE_VERTICES
    );
    assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    assert_eq!(reader.remaining(), 0);
}

#[test]
fn mesh_side_userdata_refuses_collection_limit_without_cache_warning() {
    let presence = [1_u8];
    let wrapper = mesh_class_wrapper_with_userdata();
    let bytes = anonymous_mixed(&[(&presence, false), (&wrapper, true)]);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
    let mut warnings = Diagnostics::new();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("root bytes admitted");
    let error = read_mesh_sides(
        &ctx,
        &bytes,
        &mut reader,
        ArchiveVersion::V5,
        1,
        &mut warnings,
    )
    .expect_err("class userdata exceeds remaining collection items");
    assert!(matches!(
        error,
        GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino class userdata"
    ));
    assert!(warnings.is_empty());
}

fn assert_discarded_mesh_candidates_release_storage(bytes: &[u8], legacy: bool, face_count: usize) {
    use cadmpeg_core::decode::refusal_probe::RefusalProbe;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    for operation in ["Rhino class userdata", "Rhino Brep mesh userdata"] {
        let _probe = RefusalProbe::arm(ResourceDimension::RetainedBytes, operation, None);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 64 * 1024;
        policy.limits.max_retained_bytes = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &policy).expect("root");
        let mut reader = BoundedReader::new(bytes, 0, bytes.len()).expect("reader");
        let mut warnings = Diagnostics::new();
        let (slots, _) = if legacy {
            read_legacy_mesh_sides(
                &ctx,
                bytes,
                &mut reader,
                ArchiveVersion::V5,
                face_count,
                &mut warnings,
            )
        } else {
            read_mesh_sides(
                &ctx,
                bytes,
                &mut reader,
                ArchiveVersion::V5,
                face_count,
                &mut warnings,
            )
        }
        .expect("optional cache degrades");
        assert_eq!(slots.len(), face_count);
        assert!(slots.iter().all(Option::is_none));
        assert_eq!(warnings.len(), 1);
        assert_eq!(reader.remaining(), 0);
        let scratch = ctx
            .reserve_scoped(policy.limits.max_materialized_bytes, "mesh scratch reuse")
            .expect("all discarded candidate storage is released");
        drop(scratch);
        ctx.finish_session()
            .expect("discarded userdata is not retained");
    }
}

#[test]
fn mesh_wrong_class_releases_userdata_candidates() {
    let presence = [1_u8];
    let wrapper = class_wrapper_with_mesh_userdata(ON_BREP);
    let bytes = anonymous_mixed(&[(&presence, false), (&wrapper, true)]);
    assert_discarded_mesh_candidates_release_storage(&bytes, false, 1);
}

#[test]
fn mesh_invalid_presence_releases_previous_slot_userdata() {
    let presence = [1_u8];
    let invalid_presence = [2_u8];
    let wrapper = mesh_class_wrapper_with_userdata();
    let bytes = anonymous_mixed(&[
        (&presence, false),
        (&wrapper, true),
        (&invalid_presence, false),
    ]);
    assert_discarded_mesh_candidates_release_storage(&bytes, false, 2);
}

#[test]
fn legacy_mesh_truncation_releases_previous_slot_userdata() {
    let mut bytes = vec![1_u8];
    bytes.extend(mesh_class_wrapper_with_userdata());
    assert_discarded_mesh_candidates_release_storage(&bytes, true, 2);
}

#[test]
fn mesh_recovery_releases_slots_before_scoped_fallback_admission() {
    use cadmpeg_core::decode::refusal_probe::RefusalProbe;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    for legacy in [false, true] {
        let bytes = if legacy { vec![1_u8] } else { anonymous(&[1]) };
        let operation = if legacy {
            "Rhino legacy Brep degraded mesh slots"
        } else {
            "Rhino Brep degraded mesh slots"
        };
        let _probe = RefusalProbe::arm(ResourceDimension::MaterializedBytes, operation, None);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let mut outer = ctx
            .reserve_scoped(0, "temporary mesh cache output")
            .expect("outer scope");
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        let mut warnings = Diagnostics::new();
        let (slots, _) = outer
            .with_storage(|| {
                if legacy {
                    read_legacy_mesh_sides(
                        &ctx,
                        &bytes,
                        &mut reader,
                        ArchiveVersion::V5,
                        1,
                        &mut warnings,
                    )
                } else {
                    read_mesh_sides(
                        &ctx,
                        &bytes,
                        &mut reader,
                        ArchiveVersion::V5,
                        1,
                        &mut warnings,
                    )
                }
            })
            .expect("replacement slots reuse the released candidate peak");
        assert_eq!(slots.len(), 1);
        assert!(slots[0].is_none());
        assert_eq!(warnings.len(), 1);
        drop(slots);
        drop(warnings);
        drop(outer);
        ctx.finish_session()
            .expect("fallback allocation does not raise the candidate peak");
    }
}
