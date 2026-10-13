// SPDX-License-Identifier: Apache-2.0
use super::intervals::{owner_at, parameter_at};
use crate::design::decode::dimension_frames::parse_dimension_null_locus_pair;

/// A dimension presentation type and the paired presentation type, both
/// registered by the segment whose type-table stream is `meta_name`.
fn presentation_types(meta_name: &str) -> Vec<crate::records::entity_header::SegmentType> {
    [
        super::super::DIMENSION_PRESENTATION_V3_TYPE_GUID,
        super::super::DIMENSION_PRESENTATION_PAIR_TYPE_GUID,
    ]
    .into_iter()
    .enumerate()
    .map(|(ordinal, type_guid)| {
        let mut design_type =
            crate::design::test_support::design_type(type_guid, None, 0, "MSketch", vec![]);
        design_type.byte_offset = u64::try_from(ordinal).unwrap();
        crate::records::entity_header::SegmentType::try_new(
            crate::test_support::with_decode_context(|ctx| {
                crate::ids::native_design_type_id(ctx, meta_name, design_type.byte_offset)
                    .expect("test F3D native identity")
            }),
            design_type,
        )
        .unwrap()
    })
    .collect()
}

#[test]
fn presentation_streams_are_segments_registering_both_presentation_types() {
    let ctx = cadmpeg_test_support::service_decode_context();
    // A segment name the native scope escapes, a segment registering only the
    // presentation type, and a segment registering both.
    let mut types = presentation_types("Fusion Asset:Name[Active]/Design1/MetaStream.dat");
    types.extend(
        presentation_types("FusionAssetName[Active]/Design2/MetaStream.dat")
            .into_iter()
            .take(1),
    );
    types.extend(presentation_types(
        "FusionAssetName[Active]/Design3/MetaStream.dat",
    ));
    let (streams, _storage) = super::super::presentation_stream_prefixes(&ctx, &types).unwrap();
    let expected = [
        "Fusion Asset:Name[Active]/Design1/BulkStream.dat",
        "FusionAssetName[Active]/Design3/BulkStream.dat",
    ]
    .map(|name| {
        let scope = crate::test_support::with_decode_context(|ctx| {
            crate::ids::native_scope(ctx, name, "retain F3D native scope")
                .expect("test F3D native identity")
        });
        scope
            .strip_suffix(crate::design::decode::sketch::BULK_STREAM_FILE)
            .unwrap()
            .to_owned()
    });
    assert_eq!(streams, expected);
}

#[test]
fn typed_dimension_companions_refuse_collection_limit() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;
    use std::io::{Cursor, Write};
    use zip::CompressionMethod;

    let mut bytes = vec![0; 74];
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"277");
    bytes[7..11].copy_from_slice(&1394u32.to_le_bytes());
    bytes[19] = 1;
    bytes[20..24].copy_from_slice(&2u32.to_le_bytes());
    bytes[24] = 1;
    bytes[35..39].copy_from_slice(&10u32.to_le_bytes());
    bytes[39] = 1;
    bytes[40..44].copy_from_slice(&1109u32.to_le_bytes());
    bytes[50..54].copy_from_slice(&7u32.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"273");
    bytes.extend_from_slice(&1394u32.to_le_bytes());
    let mut pair = crate::design::test_support::with_test_decode_context(|ctx| {
        parse_dimension_null_locus_pair(
            ctx,
            &bytes,
            0,
            1290,
            &[1109],
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        )
    })
    .expect("null-locus dimension frame")
    .expect("valid null-locus dimension frame");
    pair.id = "f3d:Design/BulkStream.dat:design-dimension-null-locus-pair#0".into();

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(&[]).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    // Owner 10 binds the dimension parameter 12 to a companion.
    let parameters = [parameter_at(12, 20)];
    let owners = [owner_at(10, 13, 12, 11, 10)];
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let inputs = crate::design::decode::dimension_frames::DimensionDecodeInputs {
            scan,
            placements: &[],
            parameters: &parameters,
            owners: &owners,
            companions: &[],
            scopes: &[],
            headers: &[],
            points: &[],
            curves: &[],
        };
        let refusal = crate::test_support::resource_refusal_at(
            ResourceDimension::CollectionItems,
            "f3d typed dimension companions",
            0,
            |ctx| {
                crate::design::decode::dimension_frames::decode_dimension_null_locus_pairs(
                    ctx,
                    &inputs,
                    &mut crate::design::decode::sketch::RecordOffsetCache::new(ctx)?,
                    std::slice::from_ref(&pair),
                    &[],
                )
            },
        );
        assert!(matches!(
            refusal,
            CodecError::ResourceLimit(failure)
                if failure.dimension == ResourceDimension::CollectionItems
                    && failure.operation == "f3d typed dimension companions"
        ));
    });
}

#[test]
fn dimension_presentation_sketch_scopes_refuse_collection_limit() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;
    use std::io::{Cursor, Write};
    use zip::CompressionMethod;

    let placement = crate::records::sketch_placement::DesignSketchPlacement {
        frame: crate::records::sketch_placement::DesignSketchFrame::new(
            0,
            crate::records::sketch_placement::DesignSketchFrameForm::MemberCompact {
                paired_byte_offset: 34,
            },
        )
        .unwrap(),
        id: "f3d:design:design-sketch-placement#1".into(),
        scope_record_index: Some(1),
        entity_id: crate::records::identity::DesignEntityId::try_from("Sketch_201".to_owned())
            .unwrap(),
        visibility: None,
        class_tag: crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
        record_index: 1,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("257".to_owned())
            .unwrap(),
    };
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(&[]).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    // The segment registers a presentation type and the paired type.
    let types = presentation_types("FusionAssetName[Active]/Design1/MetaStream.dat");
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let inputs = crate::design::decode::dimension_frames::DimensionDecodeInputs {
            scan,
            placements: std::slice::from_ref(&placement),
            parameters: &[],
            owners: &[],
            companions: &[],
            scopes: &[],
            headers: &[],
            points: &[],
            curves: &[],
        };
        let refusal = crate::test_support::resource_refusal_at(
            ResourceDimension::CollectionItems,
            "f3d dimension presentation sketch scopes",
            0,
            |ctx| {
                crate::design::decode::dimension_frames::decode_dimension_presentation_frames(
                    ctx,
                    &inputs,
                    &mut crate::design::decode::sketch::RecordOffsetCache::new(ctx)?,
                    &types,
                    &[],
                )
            },
        );
        assert!(matches!(
            refusal,
            CodecError::ResourceLimit(failure)
                if failure.dimension == ResourceDimension::CollectionItems
                    && failure.operation == "f3d dimension presentation sketch scopes"
        ));
    });
}

#[test]
fn dimension_annotation_interval_owner_scan_refuses_work_limit() {
    use cadmpeg_core::decode::ResourceDimension;
    use std::io::{Cursor, Write};
    use zip::CompressionMethod;

    const STREAM: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let native_scope = crate::test_support::with_decode_context(|ctx| {
        crate::ids::native_scope(ctx, STREAM, "retain F3D native scope")
            .expect("test F3D native identity")
    });
    let owner = crate::records::parameters::DesignParameterOwner::try_from(
        crate::records::parameters::DesignParameterOwnerWire {
            id: format!("{native_scope}:design-parameter-owner#10"),
            byte_offset: 120,
            frame_length: 104,
            class_tag: crate::records::references::DesignClassTag::try_from("292".to_owned())
                .unwrap(),
            record_index: 10,
            scope_record_index: 13,
            local_ordinal: 0,
            evaluated_value: 1.0,
            evaluated_value_offset: 160,
            parameter_record_index: 11,
            owned_ordinal: 0,
            variant: Some(0),
            companion_record_index: 12,
        },
    )
    .unwrap();
    let companion = crate::records::parameters::DesignParameterCompanion::unbound(
        format!("{native_scope}:design-parameter-companion#12"),
        220,
        crate::records::references::DesignClassTag::try_from("408".to_owned()).unwrap(),
        12,
        10,
        std::num::NonZeroU64::MIN,
        262,
    );
    let scope = crate::records::feature::scope::DesignParameterScope::empty(
        &format!("{native_scope}:design-parameter-scope#13"),
        crate::records::feature::scope::DesignFeatureKind::Extrude,
        13,
    );
    let owners = [owner];
    let companions = [companion];
    let scopes = [scope];
    let parameters = [parameter_at(11, 50)];

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
    zip.start_file(STREAM, stored).unwrap();
    zip.write_all(&[0; 400]).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let inputs = crate::design::decode::dimension_frames::DimensionDecodeInputs {
            scan,
            placements: &[],
            parameters: &parameters,
            owners: &owners,
            companions: &companions,
            scopes: &scopes,
            headers: &[],
            points: &[],
            curves: &[],
        };
        let refusal = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            "scan F3D dimension annotation interval owners",
            0,
            |ctx| {
                crate::design::decode::dimension_frames::decode_dimension_annotation_frames(
                    ctx,
                    &inputs,
                    &mut crate::design::decode::sketch::RecordOffsetCache::new(ctx)?,
                    &[],
                )
            },
        );
        assert!(matches!(
            refusal,
            cadmpeg_core::CodecError::ResourceLimit(failure)
                if failure.dimension == ResourceDimension::WorkUnits
                    && failure.operation == "scan F3D dimension annotation interval owners"
                    && failure.additional == 1
        ));
    });
}
