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
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
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
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
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
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
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
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
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
