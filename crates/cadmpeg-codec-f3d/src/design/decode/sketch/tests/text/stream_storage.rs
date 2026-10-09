// SPDX-License-Identifier: Apache-2.0

use super::indexed_sketch_fixture;
use crate::design::decode::sketch;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn materialized_used(ctx: &DecodeContext<'_>) -> u64 {
    let CodecError::ResourceLimit(original) = ctx
        .reserve_scoped(ctx.policy().limits.max_materialized_bytes + 1, "observe live sketch output")
        .unwrap_err()
    else { panic!("materialized observation refuses"); };
    assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(ctx.resource_refusal(), Some(original));
    original.used
}

macro_rules! producer_storage_control {
    ($name:ident, $decode:path, $record:ty, $offset:ident, $index:expr, $suffix:literal) => {
        #[test]
        fn $name() {
            let (bytes, meta, point_at, curve_at, text_at, _) = indexed_sketch_fixture();
            let $offset = match $index { 50 => point_at, 51 => curve_at, 52 => text_at, _ => unreachable!() };
            for keep in [false, true] {
                let arena = DecodeArena::new();
                let policy = DecodePolicy::service();
                let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
                let output_storage;
                let (records, held_storage) = $decode(&ctx, &bytes, &meta, "Design/BulkStream.dat").unwrap();
                output_storage = held_storage;
                assert_eq!(records.len(), 1);
                assert_eq!(records[0].record_index, $index);
                assert_eq!(records[0].byte_offset, u64::try_from($offset).unwrap());
                assert_eq!(records[0].id, format!(concat!("f3d:Design/BulkStream.dat:", $suffix, "#{}"), $offset));
                // These actual fixed-size record slots use the core four-slot
                // initial growth. Owned children have independent retention.
                assert!(std::mem::size_of::<$record>() > 1);
                assert!(std::mem::size_of::<$record>() <= 1024);
                assert_eq!(records.capacity(), 4);
                let backing = 4 * u64::try_from(std::mem::size_of::<$record>()).unwrap();
                if keep {
                    assert_eq!(materialized_used(&ctx), backing);
                    assert_eq!(records[0].record_index, $index);
                    drop(records);
                    drop(output_storage);
                } else {
                    drop(records);
                    drop(output_storage);
                    assert_eq!(materialized_used(&ctx), 0);
                }
            }
        }
    };
}

producer_storage_control!(sketch_stream_point_backing_tracks_the_actual_returned_vector,
    sketch::decode_sketch_points_from_stream, crate::records::sketch_geometry::SketchPoint,
    point_at, 50, "sketch-point");
producer_storage_control!(sketch_stream_curve_backing_tracks_the_actual_returned_vector,
    sketch::decode_sketch_curve_identities_from_stream, crate::records::sketch_geometry::SketchCurveIdentity,
    curve_at, 51, "sketch-curve-identity");
producer_storage_control!(sketch_stream_text_backing_tracks_the_actual_returned_vector,
    sketch::decode_sketch_texts_from_stream, crate::records::sketch_geometry::SketchText,
    text_at, 52, "sketch-text");

fn archive() -> Vec<u8> {
    use std::io::{Cursor, Write};
    let (bytes, meta, _, _, _, _) = indexed_sketch_fixture();
    let entities = meta.types.iter().map(|ty| ty.entities.values().copied().collect::<Vec<_>>()).collect::<Vec<_>>();
    let types = meta.types.iter().zip(&entities).map(|(ty, ids)|
        (ty.type_guid.as_str(), "", ty.version, ty.module.as_str(), ids.as_slice())
    ).collect::<Vec<_>>();
    let records = meta.records.iter().map(|record| (record.entity_id, record.bulk_offset)).collect::<Vec<_>>();
    let encoded_meta = crate::test_support::streams_test::design_metastream_with_records(&types, &records);
    let empty_meta = crate::test_support::streams_test::design_metastream(&[]);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
    // Preserve physical member order, including an eligible empty stream and
    // a foreign segment whose invalid bytes must remain unread.
    for (segment, bulk, metadata) in [
        ("Design1", bytes.as_slice(), encoded_meta.as_slice()),
        ("FusionDesignSegmentType2", &[][..], empty_meta.as_slice()),
        ("FusionDesignSegmentType3", bytes.as_slice(), encoded_meta.as_slice()),
        ("Other", &[0xff][..], &[0xff][..]),
    ] {
        zip.start_file(format!("FusionAssetName[Active]/{segment}/MetaStream.dat"), stored).unwrap();
        zip.write_all(metadata).unwrap();
        zip.start_file(format!("FusionAssetName[Active]/{segment}/BulkStream.dat"), stored).unwrap();
        zip.write_all(bulk).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn sketch_stream_aggregate_keeps_two_real_native_streams_in_source_order() {
    let archive = archive();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, root) = DecodeContext::from_root_bytes(&archive, &arena, &policy).unwrap();
    let scan = crate::container::scan(&ctx, root).unwrap();
    let (_, _, point_at, curve_at, text_at, _) = indexed_sketch_fixture();
    let points = sketch::decode_sketch_points(&ctx, &scan).unwrap();
    let curves = sketch::decode_sketch_curve_identities(&ctx, &scan).unwrap();
    let texts = sketch::decode_sketch_texts(&ctx, &scan).unwrap();
    assert_eq!((points.len(), curves.len(), texts.len()), (2, 2, 2));
    for (position, segment) in ["Design1", "FusionDesignSegmentType3"].into_iter().enumerate() {
        let stream = format!("FusionAssetName[Active]/{segment}/BulkStream.dat");
        assert_eq!(points[position].id, format!("f3d:{stream}:sketch-point#{point_at}"));
        assert_eq!(curves[position].id, format!("f3d:{stream}:sketch-curve-identity#{curve_at}"));
        assert_eq!(texts[position].id, format!("f3d:{stream}:sketch-text#{text_at}"));
        assert_eq!(points[position].coordinates(), cadmpeg_ir::math::Point2::new(70.0, -30.0));
        assert_eq!(points[position].persistent_id(), Some(500));
        assert_eq!(curves[position].primary_id.get(), 700);
        assert_eq!(texts[position].persistent_id, Some(800));
        assert_eq!(texts[position].text, "live text");
        assert_eq!(texts[position].font_family, "Arial");
    }
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn sketch_stream_aggregate_publication_preserves_the_original_retained_refusal() {
    let archive = archive();
    for kind in 0..3 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Only the diagnostic probe selects this dimension. The permanent
        // collection and native-member limits remain unchanged elsewhere.
        policy.limits.max_retained_bytes = u64::MAX;
        let (ctx, root) = DecodeContext::from_root_bytes(&archive, &arena, &policy).unwrap();
        let scan = crate::container::scan(&ctx, root).unwrap();
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::RetainedBytes, "f3d sketch stream output", None,
        );
        let result = match kind {
            0 => sketch::decode_sketch_points(&ctx, &scan).map(|_| ()),
            1 => sketch::decode_sketch_curve_identities(&ctx, &scan).map(|_| ()),
            2 => sketch::decode_sketch_texts(&ctx, &scan).map(|_| ()),
            _ => unreachable!(),
        };
        let Err(CodecError::ResourceLimit(original)) = result else { panic!("final retained publication must refuse"); };
        assert_eq!(original.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(original.operation, "f3d sketch stream output");
        assert_eq!(ctx.resource_refusal(), Some(original));
        let Err(CodecError::ResourceLimit(later)) = ctx.charge_work(0, "after sketch publication refusal") else { panic!("first refusal remains fused"); };
        assert_eq!(later, original);
    }
}
