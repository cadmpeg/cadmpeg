// SPDX-License-Identifier: Apache-2.0

#[test]
fn work_plane_binds_explicit_unmarked_and_identity_placements() {
    use crate::records::feature::scope::DesignParameterScope;
    use crate::test_support::work_plane::{placements, scope};
    for placement in placements()
        .into_iter()
        .chain(crate::test_support::placement_construction::modern_placements())
    {
        let mut scope = scope(&placement);
        let expected = placement.fields.matrix.as_ref().map_or(
            crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
            |matrix| matrix.value,
        );
        let losses = super::bind(std::slice::from_mut(&mut scope), &[placement]);
        assert!(losses.is_empty());
        assert_eq!(scope.work_plane_transform(), Some(expected));
        let wire = serde_json::to_value(&scope).unwrap();
        let restored: DesignParameterScope = serde_json::from_value(wire).unwrap();
        assert_eq!(restored, scope);
    }
}

#[test]
fn work_plane_refuses_missing_foreign_ambiguous_and_overflowing_placements_with_loss() {
    use crate::test_support::work_plane::placements;
    let placement = placements().remove(0);
    let mut foreign = placement.clone();
    foreign.id = "f3d:Other/BulkStream.dat:placement#13".into();
    let mut overflow = placement.clone();
    let matrix = overflow.fields.matrix.as_mut().unwrap();
    let mut rows = matrix.value.rows();
    rows[0][3] = f64::MAX;
    matrix.value = rows.try_into().unwrap();
    let mut second = placement.clone();
    second.record_index = placement.record_index + 1;
    second.id = format!(
        "{}:design-placement-construction#{}",
        crate::ids::native_stream(&placement.id).unwrap(),
        second.record_index
    );
    let both = [placement.record_index, second.record_index];
    for (candidates, references) in [
        (vec![], &both[..1]),
        (vec![foreign], &both[..1]),
        (vec![placement.clone(), second], &both[..]),
        (vec![overflow], &both[..1]),
    ] {
        let mut scope = crate::test_support::work_plane::scope_referencing(references);
        let losses = super::bind(std::slice::from_mut(&mut scope), &candidates);
        assert!(scope.work_plane_frame().is_none());
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            crate::loss::F3dLossCode::WorkPlaneFrameUnbound
                .note("")
                .code
        );
    }
}

#[test]
fn work_plane_refuses_nonzero_axis_states_and_direction_controls() {
    use crate::test_support::work_plane::{placements, scope};
    let placement = placements().remove(0);
    for slot in 0..3 {
        let mut candidate = placement.clone();
        match slot {
            0 => candidate.fields.z_flip.value = 1,
            1 => candidate.fields.axis_flipped.as_mut().unwrap().value = 1,
            2 => candidate.fields.use_u_direction.as_mut().unwrap().value = 2,
            _ => unreachable!(),
        }
        let mut scope = scope(&placement);
        let losses = super::bind(std::slice::from_mut(&mut scope), &[candidate]);
        assert!(scope.work_plane_frame().is_none());
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            crate::loss::F3dLossCode::WorkPlaneFrameUnbound
                .note("")
                .code
        );
    }
}

#[test]
fn work_plane_modern_frames_refuse_unchecked_periodic_and_direction_modes_with_loss() {
    use crate::test_support::placement_construction::modern_placements;
    for placement in modern_placements()
        .into_iter()
        .filter(|p| p.type_version == 16)
    {
        for slot in 0..4 {
            let mut candidate = placement.clone();
            match slot {
                0 => candidate.fields.periodic_face_mode.as_mut().unwrap().value = 1,
                1 => candidate.fields.use_u_direction.as_mut().unwrap().value = 2,
                2 => candidate.fields.z_flip.value = 1,
                3 => candidate.fields.axis_flipped.as_mut().unwrap().value = 1,
                _ => unreachable!(),
            }
            let mut scope = crate::test_support::work_plane::scope(&candidate);
            let losses = super::bind(std::slice::from_mut(&mut scope), &[candidate]);
            assert!(scope.work_plane_frame().is_none());
            assert_eq!(losses.len(), 1);
            assert_eq!(
                losses[0].code,
                crate::loss::F3dLossCode::WorkPlaneFrameUnbound
                    .note("")
                    .code
            );
        }
    }
}

#[test]
fn work_plane_modern_join_uses_member_identity_independent_of_table_position_and_count() {
    for placement in crate::test_support::placement_construction::modern_placements() {
        for members in [
            [71, placement.record_index, 73].as_slice(),
            [71, 72, 73, placement.record_index].as_slice(),
        ] {
            let mut scope = crate::test_support::work_plane::scope_referencing(members);
            let losses = super::bind(
                std::slice::from_mut(&mut scope),
                std::slice::from_ref(&placement),
            );
            assert!(losses.is_empty());
            assert!(scope.work_plane_frame().is_some());
            assert!(scope.work_plane_construction().is_none());
            assert!(scope.work_plane_reference().is_none());
        }
    }
}

#[test]
fn work_plane_frames_check_direction_values() {
    let legacy = crate::test_support::work_plane::placements().remove(0);
    assert_eq!(legacy.type_version, 14);
    let mut version_12 = legacy.clone();
    version_12.type_version = 12;
    for placement in [legacy, version_12]
        .into_iter()
        .chain(crate::test_support::placement_construction::modern_placements())
    {
        for direction in [0, 1] {
            let mut candidate = placement.clone();
            candidate.fields.use_u_direction.as_mut().unwrap().value = direction;
            let mut scope = crate::test_support::work_plane::scope(&candidate);
            assert!(super::bind(std::slice::from_mut(&mut scope), &[candidate]).is_empty());
            assert_eq!(
                scope.work_plane_transform(),
                Some(placement.fields.matrix.as_ref().map_or(
                    crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
                    |matrix| matrix.value,
                ))
            );
        }
        let mut candidate = placement.clone();
        candidate.fields.use_u_direction.as_mut().unwrap().value = 2;
        let mut scope = crate::test_support::work_plane::scope(&candidate);
        let losses = super::bind(std::slice::from_mut(&mut scope), &[candidate]);
        assert!(scope.work_plane_frame().is_none());
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            crate::loss::F3dLossCode::WorkPlaneFrameUnbound
                .note("")
                .code
        );
    }
}

#[test]
fn work_plane_binds_stored_matrix_with_nonzero_inferred_axis_bytes_and_reports_assumption() {
    use crate::test_support::work_plane::{placements, scope};
    for placement in placements()
        .into_iter()
        .chain(crate::test_support::placement_construction::modern_placements())
    {
        let expected = placement.fields.matrix.as_ref().map_or(
            crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
            |matrix| matrix.value,
        );
        for (inferred_x_axis, inferred_z_axis) in [(1, 0), (0, 1), (155, 220)] {
            let mut candidate = placement.clone();
            candidate.fields.inferred_x_axis.value = inferred_x_axis;
            candidate.fields.inferred_z_axis.value = inferred_z_axis;
            let mut scope = scope(&candidate);
            let losses = super::bind(std::slice::from_mut(&mut scope), &[candidate]);
            assert_eq!(scope.work_plane_transform(), Some(expected));
            assert_eq!(losses.len(), 1);
            assert_eq!(
                losses[0].code,
                crate::loss::F3dLossCode::WorkPlaneInferredAxisAssumed
                    .note("")
                    .code
            );
        }
    }
}

#[test]
fn work_plane_binds_version_seven_and_ten_grammar_frames() {
    use crate::test_support::placement_construction::{grammar_placement, GRAMMAR_FRAME_ROWS};
    for (version, use_u_direction) in [(7, 0), (10, 0), (10, 1)] {
        let placement = grammar_placement(version, use_u_direction);
        let mut scope =
            crate::test_support::work_plane::scope_referencing(&[38, 39, placement.record_index]);
        assert!(super::bind(std::slice::from_mut(&mut scope), &[placement]).is_empty());
        let transform = scope.work_plane_transform().unwrap();
        assert_eq!(transform.rows(), GRAMMAR_FRAME_ROWS);
        let frame =
            crate::design::feature_project::work_plane_datum_frame(transform.rows()).unwrap();
        assert_eq!(
            frame.origin().get(),
            cadmpeg_ir::math::Point3::new(-35.0, 2.5, 0.0)
        );
        assert_eq!(
            frame.normal().get(),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
        );
        assert_eq!(
            frame.u_axis().get(),
            cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0)
        );
    }
}

#[test]
fn class_pair_frame_with_nonzero_inferred_axis_bytes_keeps_matrix_and_reports_assumption() {
    use crate::records::sketch_placement::SketchPlacementMatrix;
    use crate::test_support::work_plane::{placements, scope, scope_referencing};
    let transform = SketchPlacementMatrix::try_from([
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 2.5],
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ])
    .unwrap();
    let mut class_pair = scope_referencing(&[73]);
    class_pair.with_work_plane_transform(transform);
    let inferred_axes =
        std::collections::HashMap::from([(class_pair.id.clone(), (0, [0x4d, 0x90]))]);
    let losses = super::class_pair_inferred_axis_losses(
        std::slice::from_ref(&class_pair),
        &[],
        &inferred_axes,
    );
    assert_eq!(class_pair.work_plane_transform(), Some(transform));
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::F3dLossCode::WorkPlaneInferredAxisAssumed
            .note("")
            .code
    );
    assert!(losses[0]
        .message
        .contains("isInferredXAxis byte is 77 and isInferredZAxis byte is 144"));

    let replaced = std::collections::HashMap::from([(class_pair.id.clone(), (49, [1, 1]))]);
    assert!(super::class_pair_inferred_axis_losses(
        std::slice::from_ref(&class_pair),
        &[],
        &replaced
    )
    .is_empty());

    let placement = placements().remove(0);
    let mut registered = scope(&placement);
    registered.with_work_plane_transform(transform);
    let registered_axes = std::collections::HashMap::from([(registered.id.clone(), (0, [1, 1]))]);
    assert!(super::class_pair_inferred_axis_losses(
        std::slice::from_ref(&registered),
        &[placement],
        &registered_axes
    )
    .is_empty());
}

fn append_work_plane_frame(
    bulk: &mut Vec<u8>,
    record_start: usize,
    entity_id: u64,
    feature_record_index: u64,
    marker: [u8; 2],
    marked_ref_byte: u8,
    display_name: &str,
) {
    if bulk.len() < record_start + 130 {
        bulk.resize(record_start + 130, 0);
    }
    let entity_id_str = format!("0_{entity_id}");
    bulk.extend_from_slice(&u32::try_from(entity_id_str.len()).unwrap().to_le_bytes());
    for unit in entity_id_str.encode_utf16() {
        bulk.extend_from_slice(&unit.to_le_bytes());
    }
    crate::test_support::push_reference_u64(bulk, 12);
    bulk.extend_from_slice(&1u32.to_le_bytes());
    bulk.extend_from_slice(&[0u8; 5]);
    bulk.push(marked_ref_byte);
    bulk.extend_from_slice(&feature_record_index.to_le_bytes());
    bulk.extend_from_slice(&[0, 0]);
    bulk.extend_from_slice(&marker);
    bulk.extend_from_slice(&u32::try_from(display_name.len()).unwrap().to_le_bytes());
    for unit in display_name.encode_utf16() {
        bulk.extend_from_slice(&unit.to_le_bytes());
    }
}

type WorkPlaneTestSpec<'a> = (&'a str, u64, u64, [u8; 2], u8, &'a str);

fn build_work_plane_test_archive(planes: &[WorkPlaneTestSpec<'_>]) -> Vec<u8> {
    use crate::test_support::lp_ascii;
    use crate::test_support::manifest_test::write_synthetic_manifests;
    use crate::test_support::streams_test::design_metastream_with_records;
    use cadmpeg_core::decode::u64_from_index;
    use std::io::{Cursor, Write};
    use zip::CompressionMethod;

    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);

    let mut stream_names = Vec::new();
    for &(stream, _, _, _, _, _) in planes {
        if !stream_names.contains(&stream) {
            stream_names.push(stream);
        }
    }

    for stream in stream_names {
        let stream_planes: Vec<_> = planes.iter().filter(|p| p.0 == stream).copied().collect();

        let mut bulk = Vec::new();
        let mut records = Vec::new();
        let mut entity_ids = Vec::new();

        for &(_, entity_id, feature_rec, marker, marked_byte, name) in &stream_planes {
            let offset = bulk.len();
            lp_ascii(&mut bulk, "256");
            bulk.extend_from_slice(&entity_id.to_le_bytes());
            append_work_plane_frame(
                &mut bulk,
                offset,
                entity_id,
                feature_rec,
                marker,
                marked_byte,
                name,
            );

            records.push((entity_id, u64_from_index(offset)));
            entity_ids.push(entity_id);
        }

        zip.start_file(stream, stored).unwrap();
        zip.write_all(&bulk).unwrap();

        let meta_name = stream.replace("BulkStream.dat", "MetaStream.dat");
        zip.start_file(&meta_name, stored).unwrap();
        let meta_bytes = design_metastream_with_records(
            &[(
                super::WORK_PLANE_FEATURE_NAME_TYPE_GUID,
                super::WORK_PLANE_FEATURE_NAME_BASE_TYPE_GUID,
                super::WORK_PLANE_FEATURE_NAME_VERSION,
                "Geometry",
                &entity_ids,
            )],
            &records,
        );
        zip.write_all(&meta_bytes).unwrap();
    }

    zip.finish().unwrap().into_inner()
}

#[test]
fn work_plane_feature_name_decodes_cleanly() {
    use crate::test_support::zip_test::with_scan;

    let archive = build_work_plane_test_archive(&[(
        "FusionAssetName[Active]/Design1/BulkStream.dat",
        405,
        407,
        [0, 1],
        1,
        "Body Split",
    )]);

    with_scan(&archive, |scan| {
        let names = super::decode_work_plane_feature_names(scan).unwrap();
        assert_eq!(
            names.get(&(
                "FusionAssetName[Active]/Design1/BulkStream.dat".to_string(),
                407
            )),
            Some(&super::WorkPlaneFeatureName::Unique(Some(
                "Body Split".to_string()
            )))
        );
    });
}

#[test]
fn work_plane_feature_name_conflicts_fail_closed_and_emit_loss() {
    use crate::test_support::zip_test::with_scan;

    let stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let archive = build_work_plane_test_archive(&[
        (stream, 405, 407, [0, 1], 1, "Body Split"),
        (stream, 406, 407, [0, 1], 1, "Conflicting Floor"),
    ]);

    with_scan(&archive, |scan| {
        let mut scope = crate::test_support::work_plane::scope_referencing(&[73]);
        scope.id = format!("f3d:{stream}:design-parameter-scope#407");
        scope.record_index = 407;
        let losses = super::bind_feature_names(scan, std::slice::from_mut(&mut scope));
        assert_eq!(scope.display_name, None);
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            crate::loss::F3dLossCode::WorkPlaneDisplayNameAmbiguous
                .note("")
                .code
        );
    });
}

#[test]
fn work_plane_feature_name_supports_typed_references_and_skips_origin_planes() {
    use crate::test_support::lp_ascii;
    use crate::test_support::manifest_test::write_synthetic_manifests;
    use crate::test_support::streams_test::design_metastream_with_records;
    use crate::test_support::zip_test::with_scan;
    use cadmpeg_core::decode::u64_from_index;
    use std::io::{Cursor, Write};
    use zip::CompressionMethod;

    let stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let mut bulk = Vec::new();
    let origin_offset = bulk.len();
    lp_ascii(&mut bulk, "256");
    bulk.extend_from_slice(&42u64.to_le_bytes());
    bulk.resize(origin_offset + 130, 0);
    bulk.extend_from_slice(&0u32.to_le_bytes());
    bulk.extend_from_slice(&[0xff, 0xff, 0xff, 0xff, 0x01]);
    crate::test_support::lp_utf16(&mut bulk, "XY");

    let push_typed_ref = |out: &mut Vec<u8>, target: u64, guid: &str| {
        out.push(1);
        out.extend_from_slice(&target.to_le_bytes());
        lp_ascii(out, guid);
        out.extend_from_slice(&[0, 0]);
    };

    let constructed_offset = bulk.len();
    lp_ascii(&mut bulk, "256");
    bulk.extend_from_slice(&406u64.to_le_bytes());
    bulk.resize(constructed_offset + 130, 0);
    crate::test_support::lp_utf16(&mut bulk, "12_406");
    push_typed_ref(&mut bulk, 12, "A7AEA631-985B-4DD1-8CE2-DE2C14B54081");
    bulk.extend_from_slice(&2u32.to_le_bytes());
    bulk.extend_from_slice(&[0; 5]);
    push_typed_ref(&mut bulk, 407, "D869265F-F339-4751-A066-91D30F81FF08");
    bulk.extend_from_slice(&[0, 1]);
    crate::test_support::lp_utf16(&mut bulk, "Typed Plane");

    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(stream, stored).unwrap();
    zip.write_all(&bulk).unwrap();
    let meta_name = stream.replace("BulkStream.dat", "MetaStream.dat");
    zip.start_file(&meta_name, stored).unwrap();
    let meta_bytes = design_metastream_with_records(
        &[(
            super::WORK_PLANE_FEATURE_NAME_TYPE_GUID,
            super::WORK_PLANE_FEATURE_NAME_BASE_TYPE_GUID,
            super::WORK_PLANE_FEATURE_NAME_VERSION,
            "Geometry",
            &[42, 406],
        )],
        &[
            (42, u64_from_index(origin_offset)),
            (406, u64_from_index(constructed_offset)),
        ],
    );
    zip.write_all(&meta_bytes).unwrap();
    let archive = zip.finish().unwrap().into_inner();

    with_scan(&archive, |scan| {
        let mut scope = crate::test_support::work_plane::scope_referencing(&[73]);
        scope.id = format!("f3d:{stream}:design-parameter-scope#407");
        scope.record_index = 407;
        let losses = super::bind_feature_names(scan, std::slice::from_mut(&mut scope));
        assert!(losses.is_empty());
        assert_eq!(scope.display_name.as_deref(), Some("Typed Plane"));
    });
}

#[test]
fn work_plane_feature_name_isolated_by_stream() {
    use crate::test_support::zip_test::with_scan;

    let archive = build_work_plane_test_archive(&[
        (
            "FusionAssetName[Active]/Design1/BulkStream.dat",
            405,
            407,
            [0, 1],
            1,
            "Stream1 Feature",
        ),
        (
            "FusionAssetName[Active]/FusionDesignSegmentType2/BulkStream.dat",
            405,
            407,
            [0, 1],
            1,
            "Stream2 Feature",
        ),
    ]);

    with_scan(&archive, |scan| {
        let names = super::decode_work_plane_feature_names(scan).unwrap();
        assert_eq!(
            names.get(&(
                "FusionAssetName[Active]/Design1/BulkStream.dat".to_string(),
                407
            )),
            Some(&super::WorkPlaneFeatureName::Unique(Some(
                "Stream1 Feature".to_string()
            )))
        );
        assert_eq!(
            names.get(&(
                "FusionAssetName[Active]/FusionDesignSegmentType2/BulkStream.dat".to_string(),
                407
            )),
            Some(&super::WorkPlaneFeatureName::Unique(Some(
                "Stream2 Feature".to_string()
            )))
        );
    });
}

#[test]
fn work_plane_feature_name_rejects_corrupt_marker_safely() {
    use crate::test_support::zip_test::with_scan;

    let archive = build_work_plane_test_archive(&[(
        "FusionAssetName[Active]/Design1/BulkStream.dat",
        405,
        407,
        [0, 2],
        1,
        "Should Not Decode",
    )]);

    with_scan(&archive, |scan| {
        let names = super::decode_work_plane_feature_names(scan).unwrap();
        assert!(!names.contains_key(&(
            "FusionAssetName[Active]/Design1/BulkStream.dat".to_string(),
            407
        )));
    });
}

#[test]
fn work_plane_bind_feature_names_handles_metadata_mismatch_without_failing() {
    use crate::test_support::manifest_test::write_synthetic_manifests;
    use crate::test_support::streams_test::design_metastream_with_records;
    use crate::test_support::zip_test::with_scan;
    use cadmpeg_core::decode::u64_from_index;
    use std::io::{Cursor, Write};
    use zip::CompressionMethod;

    let stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let mut bulk = Vec::new();
    let constructed_offset = bulk.len();
    crate::test_support::lp_ascii(&mut bulk, "256");
    bulk.extend_from_slice(&406u64.to_le_bytes());
    bulk.resize(constructed_offset + 130, 0);
    crate::test_support::lp_utf16(&mut bulk, "12_406");
    bulk.push(1);
    bulk.extend_from_slice(&12u64.to_le_bytes());
    crate::test_support::lp_ascii(&mut bulk, "A7AEA631-985B-4DD1-8CE2-DE2C14B54081");
    bulk.extend_from_slice(&[0, 0]);
    bulk.extend_from_slice(&2u32.to_le_bytes());
    bulk.extend_from_slice(&[0; 5]);
    bulk.push(1);
    bulk.extend_from_slice(&407u64.to_le_bytes());
    crate::test_support::lp_ascii(&mut bulk, "D869265F-F339-4751-A066-91D30F81FF08");
    bulk.extend_from_slice(&[0, 0, 0, 1]);
    crate::test_support::lp_utf16(&mut bulk, "Typed Plane");

    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(stream, stored).unwrap();
    zip.write_all(&bulk).unwrap();
    let meta_name = stream.replace("BulkStream.dat", "MetaStream.dat");
    zip.start_file(&meta_name, stored).unwrap();
    // Register with a mismatched base GUID
    let meta_bytes = design_metastream_with_records(
        &[(
            super::WORK_PLANE_FEATURE_NAME_TYPE_GUID,
            "00000000-0000-0000-0000-000000000000",
            super::WORK_PLANE_FEATURE_NAME_VERSION,
            "Geometry",
            &[406],
        )],
        &[(406, u64_from_index(constructed_offset))],
    );
    zip.write_all(&meta_bytes).unwrap();
    let archive = zip.finish().unwrap().into_inner();

    with_scan(&archive, |scan| {
        let mut scope = crate::test_support::work_plane::scope_referencing(&[73]);
        scope.id = format!("f3d:{stream}:design-parameter-scope#407");
        scope.record_index = 407;
        let losses = super::bind_feature_names(scan, std::slice::from_mut(&mut scope));
        assert!(losses.is_empty());
        assert_eq!(scope.display_name, None);
    });
}

#[test]
fn work_plane_bind_feature_names_reports_undecoded_loss_on_corrupt_entry() {
    use crate::test_support::manifest_test::write_synthetic_manifests;
    use crate::test_support::streams_test::design_metastream_with_records;
    use crate::test_support::zip_test::with_scan;
    use std::io::{Cursor, Write};
    use zip::CompressionMethod;

    let stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(stream, stored).unwrap();
    zip.write_all(b"short").unwrap();
    // Missing MetaStream causes decode_work_plane_feature_names to fail or skip
    let archive = zip.finish().unwrap().into_inner();

    with_scan(&archive, |scan| {
        let mut scope = crate::test_support::work_plane::scope_referencing(&[73]);
        scope.id = format!("f3d:{stream}:design-parameter-scope#407");
        scope.record_index = 407;
        let losses = super::bind_feature_names(scan, std::slice::from_mut(&mut scope));
        // With missing MetaStream, metadata_for_bulk_stream returns Ok(None), so no failure
        assert!(losses.is_empty());
    });

    // An archive with duplicate plane entity registration to cause typed_primary_frames to fail
    let mut bulk = Vec::new();
    crate::test_support::lp_ascii(&mut bulk, "256");
    bulk.extend_from_slice(&406u64.to_le_bytes());

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(stream, stored).unwrap();
    zip.write_all(&bulk).unwrap();
    let meta_name = stream.replace("BulkStream.dat", "MetaStream.dat");
    zip.start_file(&meta_name, stored).unwrap();
    let meta_bytes = design_metastream_with_records(
        &[
            (
                super::WORK_PLANE_FEATURE_NAME_TYPE_GUID,
                super::WORK_PLANE_FEATURE_NAME_BASE_TYPE_GUID,
                super::WORK_PLANE_FEATURE_NAME_VERSION,
                "Geometry",
                &[406],
            ),
            (
                super::WORK_PLANE_FEATURE_NAME_TYPE_GUID,
                super::WORK_PLANE_FEATURE_NAME_BASE_TYPE_GUID,
                super::WORK_PLANE_FEATURE_NAME_VERSION,
                "Geometry",
                &[406],
            ),
        ],
        &[(406, 0)],
    );
    zip.write_all(&meta_bytes).unwrap();
    let archive = zip.finish().unwrap().into_inner();

    with_scan(&archive, |scan| {
        let mut scope = crate::test_support::work_plane::scope_referencing(&[73]);
        scope.id = format!("f3d:{stream}:design-parameter-scope#407");
        scope.record_index = 407;
        let losses = super::bind_feature_names(scan, std::slice::from_mut(&mut scope));
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            crate::loss::F3dLossCode::WorkPlaneDisplayNameUndecoded
                .note("")
                .code
        );
        assert_eq!(scope.display_name, None);
    });
}
