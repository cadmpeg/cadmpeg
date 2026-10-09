// SPDX-License-Identifier: Apache-2.0

use super::super::*;
use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};
const STREAM: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";
use super::intervals::{parameter_at, owner_at};
fn point_at(index: u32) -> SketchPoint {
    let mut point = SketchPoint::try_from(crate::records::sketch_geometry::SketchPointDraft {
        id: "f3d:native:sketch-point#0".into(),
        record_index: 20,
        owner_reference: Some(100),
        class_tag: crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
        byte_offset: 0,
        coordinate_offset: 141,
        companion: crate::records::sketch_geometry::SketchPointCompanion {
            incident_curves: Vec::new(),
        },
        record_form: crate::records::sketch_geometry::SketchPointRecordForm::version11(
            20,
            crate::records::sketch_geometry::SketchPointClosure::Selector0State0,
            Some(2),
            0.0,
        ),
        paired_reference: 0,
        coordinates: cadmpeg_ir::math::Point2::new(120.0, 30.0),
    })
    .unwrap();
    point.id = format!("f3d:{STREAM}:sketch-point#{index}");
    point.record_index = index;
    point
}
fn companion() -> DesignParameterCompanion {
    DesignParameterCompanion::unbound(
        format!("f3d:{STREAM}:parameter-companion#0"),
        0,
        "408".to_owned().try_into().unwrap(),
        11,
        10,
        std::num::NonZeroU64::MIN,
        42,
    )
}
fn archive(bytes: &[u8], present: bool) -> Vec<u8> {
    use std::io::{Cursor, Write};
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
    if present {
        zip.start_file(STREAM, stored).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn decoder_demand(null: bool, missing: bool, target: &'static str) {
    let parameters = [parameter_at(12, 250)];
    let owners = [owner_at(10, 13, 12, 11, 197)];
    let points = [point_at(192)];
    let companions = if missing { vec![companion()] } else { vec![] };
    let archive = archive(&[], false);
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let inputs = DimensionDecodeInputs {
            scan,
            placements: &[],
            parameters: &parameters,
            owners: &owners,
            companions: &companions,
            scopes: &[],
            headers: &[],
            points: &points,
            curves: &[],
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut records = RecordOffsetCache::new(&ctx).unwrap();
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            target,
            None,
        );
        let result = if null {
            decode_dimension_null_locus_pairs(&ctx, &inputs, &mut records, &[], &[])
        } else {
            decode_dimension_locus_pairs(&ctx, &inputs, &mut records)
        };
        eprintln!("F14 null={null} missing={missing} target={target} result={result:?}");
        assert!(
            matches!(result, Ok(ref values) if values.is_empty()),
            "no actual companion stream can reach a pair query"
        );
    });
}

#[test]
fn lazy_locus_locus_empty_skips_governing() {
    decoder_demand(false, false, "index F3D governing dimension owners");
}

#[test]
fn lazy_locus_locus_empty_skips_intervals() {
    decoder_demand(false, false, "f3d companion boundaries");
}

#[test]
fn lazy_locus_locus_empty_skips_geometry() {
    decoder_demand(false, false, "scan F3D dimension sketch points");
}

#[test]
fn lazy_locus_locus_missing_stream_skips_governing() {
    decoder_demand(false, true, "index F3D governing dimension owners");
}

#[test]
fn lazy_locus_locus_missing_stream_skips_intervals() {
    decoder_demand(false, true, "f3d companion boundaries");
}

#[test]
fn lazy_locus_locus_missing_stream_skips_geometry() {
    decoder_demand(false, true, "scan F3D dimension sketch points");
}

#[test]
fn lazy_locus_null_empty_skips_governing() {
    decoder_demand(true, false, "index F3D governing dimension owners");
}

#[test]
fn lazy_locus_null_empty_skips_intervals() {
    decoder_demand(true, false, "f3d companion boundaries");
}

#[test]
fn lazy_locus_null_empty_skips_geometry() {
    decoder_demand(true, false, "scan F3D dimension sketch points");
}

#[test]
fn lazy_locus_null_missing_stream_skips_governing() {
    decoder_demand(true, true, "index F3D governing dimension owners");
}

#[test]
fn lazy_locus_null_missing_stream_skips_intervals() {
    decoder_demand(true, true, "f3d companion boundaries");
}

#[test]
fn lazy_locus_null_missing_stream_skips_geometry() {
    decoder_demand(true, true, "scan F3D dimension sketch points");
}

#[test]
fn lazy_locus_allowed_empty_outputs_release_materialized_backing() {
    let parameters = [parameter_at(12, 250)];
    let owners = [owner_at(10, 13, 12, 11, 197)];
    let points = [point_at(192)];
    let archive = archive(&[], false);
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let inputs = DimensionDecodeInputs {
            scan,
            placements: &[],
            parameters: &parameters,
            owners: &owners,
            companions: &[],
            scopes: &[],
            headers: &[],
            points: &points,
            curves: &[],
        };
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut records = RecordOffsetCache::new(&ctx).unwrap();
        for _ in 0..3 {
            assert!(decode_dimension_locus_pairs(&ctx, &inputs, &mut records)
                .unwrap()
                .is_empty());
            assert!(
                decode_dimension_null_locus_pairs(&ctx, &inputs, &mut records, &[], &[])
                    .unwrap()
                    .is_empty()
            );
        }
        drop(records);
        let CodecError::ResourceLimit(limit) = ctx
            .reserve_scoped(
                policy.limits.max_materialized_bytes + 1,
                "observe decoder lifetime",
            )
            .unwrap_err()
        else {
            panic!("resource refusal")
        };
        assert_eq!(
            limit.used, 0,
            "decoder index storage does not survive either return"
        );
    });
}

fn active_stream(null: bool) -> Vec<u8> {
    let frame = if null {
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
        bytes
    } else {
        let mut bytes = vec![0; 80];
        bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
        bytes[4..7].copy_from_slice(b"277");
        bytes[7..11].copy_from_slice(&233u32.to_le_bytes());
        bytes[19] = 1;
        bytes[20..24].copy_from_slice(&3u32.to_le_bytes());
        bytes[24] = 1;
        bytes[35..39].copy_from_slice(&4u32.to_le_bytes());
        bytes[39] = 1;
        bytes[40..44].copy_from_slice(&192u32.to_le_bytes());
        bytes[50..54].copy_from_slice(&0u32.to_le_bytes());
        bytes[54] = 1;
        bytes[55..59].copy_from_slice(&194u32.to_le_bytes());
        bytes[65..69].copy_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"273");
        bytes.extend_from_slice(&233u32.to_le_bytes());
        bytes
    };
    let mut stream = vec![0; 58];
    stream.extend(frame);
    stream.resize(300, 0);
    stream
}

fn active_decode(null: bool, target: Option<&'static str>) {
    let parameters = [parameter_at(12, 250)];
    // The original paired frames are 80 and 74 bytes. The companion prefix
    // is 58 bytes; governing owners start 59 bytes after the paired header.
    let owners = [owner_at(10, 13, 12, 11, if null { 191 } else { 197 })];
    let points = if null {
        vec![point_at(1109)]
    } else {
        vec![point_at(192), point_at(194)]
    };
    let companions = [companion()];
    let archive = archive(&active_stream(null), true);
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let inputs = DimensionDecodeInputs {
            scan,
            placements: &[],
            parameters: &parameters,
            owners: &owners,
            companions: &companions,
            scopes: &[],
            headers: &[],
            points: &points,
            curves: &[],
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut records = RecordOffsetCache::new(&ctx).unwrap();
        let _probe = target.map(|operation| {
            cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
                ResourceDimension::WorkUnits,
                operation,
                None,
            )
        });
        let result = if null {
            decode_dimension_null_locus_pairs(&ctx, &inputs, &mut records, &[], &[])
        } else {
            decode_dimension_locus_pairs(&ctx, &inputs, &mut records)
        };
        if let Some(target) = target {
            let CodecError::ResourceLimit(limit) = result.unwrap_err() else {
                panic!("targeted resource refusal")
            };
            assert_eq!(limit.operation, target);
            assert_eq!(ctx.resource_refusal(), Some(limit));
            assert!(
                matches!(ctx.charge_work(0, "after active decoder refusal"), Err(CodecError::ResourceLimit(again)) if again == limit)
            );
        } else {
            let pairs = result.unwrap();
            assert_eq!(
                pairs.len(),
                1,
                "genuine decoder reaches the unchanged typed frame"
            );
            assert_eq!(pairs[0].record_index, if null { 1394 } else { 233 });
            assert_eq!(pairs[0].companion_record_index, 11);
            assert_eq!(pairs[0].governing_companion_record_index, 11);
            assert_eq!(pairs[0].frame_length(), if null { 74 } else { 80 });
        }
    });
}
#[test]
fn lazy_locus_active_locus_pair_control() {
    active_decode(false, None);
}
#[test]
fn lazy_locus_active_null_pair_control() {
    active_decode(true, None);
}
#[test]
fn lazy_locus_active_locus_index_refusals_are_sticky() {
    for target in [
        "index F3D governing dimension owners",
        "f3d companion boundaries",
        "scan F3D dimension sketch points",
    ] {
        active_decode(false, Some(target));
    }
}
#[test]
fn lazy_locus_active_null_index_refusals_are_sticky() {
    for target in [
        "index F3D governing dimension owners",
        "f3d companion boundaries",
        "scan F3D dimension sketch points",
    ] {
        active_decode(true, Some(target));
    }
}

fn active_materialized_usage(null: bool, repeats: usize, keep_cache: bool) -> u64 {
    let parameters = [parameter_at(12, 250)];
    let owners = [owner_at(10, 13, 12, 11, if null { 191 } else { 197 })];
    let points = if null {
        vec![point_at(1109)]
    } else {
        vec![point_at(192), point_at(194)]
    };
    let companions = [companion()];
    let archive = archive(&active_stream(null), true);
    let mut used = None;
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let inputs = DimensionDecodeInputs {
            scan,
            placements: &[],
            parameters: &parameters,
            owners: &owners,
            companions: &companions,
            scopes: &[],
            headers: &[],
            points: &points,
            curves: &[],
        };
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut records = Some(RecordOffsetCache::new(&ctx).unwrap());
        for _ in 0..repeats {
            let result = if null {
                decode_dimension_null_locus_pairs(
                    &ctx,
                    &inputs,
                    records.as_mut().unwrap(),
                    &[],
                    &[],
                )
            } else {
                decode_dimension_locus_pairs(&ctx, &inputs, records.as_mut().unwrap())
            };
            assert_eq!(result.unwrap().len(), 1);
        }
        if !keep_cache {
            drop(records.take());
        }
        let CodecError::ResourceLimit(limit) = ctx
            .reserve_scoped(
                policy.limits.max_materialized_bytes + 1,
                "observe active decoder cache lifetime",
            )
            .unwrap_err()
        else {
            panic!("resource refusal")
        };
        used = Some(limit.used);
    });
    used.unwrap()
}

#[test]
fn lazy_locus_active_return_lifetime_and_record_cache_reuse() {
    for null in [false, true] {
        let once = active_materialized_usage(null, 1, true);
        let repeated = active_materialized_usage(null, 3, true);
        let released = active_materialized_usage(null, 3, false);
        eprintln!(
            "F14 null={null} active cache once={once} repeated={repeated} released={released}"
        );
        assert!(once > 0, "the actual stream index survives in the cache");
        assert_eq!(
            repeated, once,
            "repeated decoder use retains one record index"
        );
        assert_eq!(
            released, 0,
            "decoder scratch and dropped cache release materialized storage"
        );
    }
}

#[test]
fn lazy_locus_present_stream_without_pair_skips_governing() {
    let parameters = [parameter_at(12, 250)];
    let owners = [owner_at(10, 13, 12, 11, 197)];
    let points = [point_at(192)];
    let companions = [companion()];
    let archive = archive(&[0; 300], true);
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let inputs = DimensionDecodeInputs {
            scan, placements: &[], parameters: &parameters, owners: &owners,
            companions: &companions, scopes: &[], headers: &[], points: &points, curves: &[],
        };
        for null in [false, true] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = u64::MAX;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut records = RecordOffsetCache::new(&ctx).unwrap();
            let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
                ResourceDimension::WorkUnits, "index F3D governing dimension owners", None,
            );
            let result = if null {
                decode_dimension_null_locus_pairs(&ctx, &inputs, &mut records, &[], &[])
            } else {
                decode_dimension_locus_pairs(&ctx, &inputs, &mut records)
            };
            assert!(result.unwrap().is_empty());
            assert!(ctx.resource_refusal().is_none());
        }
    });
}

#[test]
fn lazy_locus_keeps_the_original_refusal_before_index_demand() {
    let parameters = [parameter_at(12, 250)];
    let owners = [owner_at(10, 13, 12, 11, 197)];
    let points = [point_at(192)];
    let archive = archive(&[], false);
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let inputs = DimensionDecodeInputs {
            scan, placements: &[], parameters: &parameters, owners: &owners,
            companions: &[], scopes: &[], headers: &[], points: &points, curves: &[],
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut records = RecordOffsetCache::new(&ctx).unwrap();
        let CodecError::ResourceLimit(original) = ctx.charge_work(1, "original dimension pair refusal").unwrap_err() else {
            panic!("the initial operation must refuse");
        };
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.operation, "original dimension pair refusal");
        assert_eq!(original.used, 0);
        assert_eq!(original.additional, 1);
        assert_eq!(original.limit, 0);
        for null in [false, true] {
            let result = if null {
                decode_dimension_null_locus_pairs(&ctx, &inputs, &mut records, &[], &[])
            } else {
                decode_dimension_locus_pairs(&ctx, &inputs, &mut records)
            };
            let Err(CodecError::ResourceLimit(later)) = result else {
                panic!("the original refusal must precede any lazy query");
            };
            assert_eq!(later, original);
        }
    });
}

fn sketch_header(index: u32) -> DesignEntityHeader {
    DesignEntityHeader {
        id: format!("f3d:{STREAM}:design-entity-header#{index}"),
        byte_offset: 0,
        entity_id: format!("0_{index}").try_into().unwrap(),
        class_tag: "256".to_owned().try_into().unwrap(),
        optional_slot_present: false,
        registration: crate::records::entity_header::DesignEntityRegistration::new(
            Some("MSketch".to_owned()),
            None,
            crate::records::identity::ReferenceRun::located(Vec::new()),
        ).unwrap(),
    }
}

#[test]
fn lazy_annotation_and_groups_skip_unreached_interval_and_geometry_owners() {
    let parameters = [parameter_at(12, 250)];
    let owners = [owner_at(10, 13, 12, 11, 197)];
    let points = [point_at(192)];
    let entities = [sketch_header(172)];
    let archive = archive(&[], false);
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        for missing in [false, true] {
            let companions = if missing { vec![companion()] } else { vec![] };
            let inputs = DimensionDecodeInputs {
                scan, placements: &[], parameters: &parameters, owners: &owners,
                companions: &companions, scopes: &[], headers: &[], points: &points, curves: &[],
            };
            for annotation in [false, true] {
                for operation in [
                    "f3d companion boundaries", "scan F3D dimension sketch points",
                    "scan F3D dimension sketch entities",
                ] {
                    let arena = DecodeArena::new();
                    let mut policy = DecodePolicy::service();
                    policy.limits.max_work_units = u64::MAX;
                    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                    let mut records = RecordOffsetCache::new(&ctx).unwrap();
                    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
                        ResourceDimension::WorkUnits, operation, None,
                    );
                    if annotation {
                        assert!(decode_dimension_annotation_frames(&ctx, &inputs, &mut records, &entities).unwrap().is_empty());
                    } else {
                        assert!(decode_dimension_locus_groups(&ctx, &inputs, &mut records, &entities).unwrap().is_empty());
                    }
                    assert!(ctx.resource_refusal().is_none());
                }
            }
        }
    });
}

#[test]
fn lazy_group_indexes_keep_the_original_typed_frame_and_refusals() {
    let mut bytes = vec![0; 101];
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"286");
    bytes[7..11].copy_from_slice(&249u32.to_le_bytes());
    bytes[19] = 1;
    bytes[20..24].copy_from_slice(&2u32.to_le_bytes());
    bytes[24] = 1;
    bytes[25..29].copy_from_slice(&175u32.to_le_bytes());
    bytes[35..39].copy_from_slice(&2u32.to_le_bytes());
    bytes[39] = 1;
    bytes[40..44].copy_from_slice(&217u32.to_le_bytes());
    bytes[50..54].copy_from_slice(&1u32.to_le_bytes());
    bytes[55] = 1;
    bytes[56..60].copy_from_slice(&172u32.to_le_bytes());
    bytes[66..70].copy_from_slice(&1u32.to_le_bytes());
    bytes[74..78].copy_from_slice(&2u32.to_le_bytes());
    bytes[78] = 1;
    bytes[79..83].copy_from_slice(&217u32.to_le_bytes());
    bytes[89] = 1;
    bytes[90..94].copy_from_slice(&175u32.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"314");
    bytes.extend_from_slice(&250u32.to_le_bytes());
    let mut stream = vec![0; 58];
    stream.extend(bytes);
    stream.resize(300, 0);
    let archive = archive(&stream, true);
    let parameters = [parameter_at(12, 250)];
    let owners = [owner_at(10, 13, 12, 11, 197)];
    let points = [point_at(175), point_at(217)];
    let entities = [sketch_header(172)];
    let companions = [companion()];
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let inputs = DimensionDecodeInputs {
            scan, placements: &[], parameters: &parameters, owners: &owners,
            companions: &companions, scopes: &[], headers: &[], points: &points, curves: &[],
        };
        for target in [None, Some("f3d companion boundaries"), Some("scan F3D dimension sketch points"), Some("scan F3D dimension sketch entities")] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = u64::MAX;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut records = RecordOffsetCache::new(&ctx).unwrap();
            let _probe = target.map(|operation| cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None));
            let result = decode_dimension_locus_groups(&ctx, &inputs, &mut records, &entities);
            if let Some(target) = target {
                let Err(CodecError::ResourceLimit(original)) = result else { panic!("the real reached index must refuse"); };
                assert_eq!(original.operation, target);
                assert_eq!(ctx.resource_refusal(), Some(original));
                let Err(CodecError::ResourceLimit(later)) = ctx.charge_work(0, "after group index refusal") else { panic!("the original refusal must remain fused"); };
                assert_eq!(later, original);
            } else {
                let groups = result.unwrap();
                assert_eq!(groups.len(), 1);
                let group = &groups[0];
                assert!(group.id.ends_with(":design-dimension-locus-group#58"));
                assert_eq!(group.companion_record_index, 11);
                assert_eq!(group.record_index, 249);
                assert_eq!(group.frame_length, 101);
                assert_eq!(group.owner_reference, 172);
                assert_eq!(group.owner_role, 1);
                assert_eq!(group.state, 0);
                assert_eq!(group.loci.iter().map(|locus| (locus.geometry_record_index, locus.role, locus.returned.value)).collect::<Vec<_>>(), [(175, 2, 217), (217, 1, 175)]);
                assert_eq!(group.next_class_tag.as_str(), "314");
                assert_eq!(group.next_record_index, 250);
            }
        }
    });
}

#[test]
fn lazy_annotation_indexes_keep_the_original_native_frame_and_refusals() {
    use crate::design::test_support::{push_genesis_block, push_reference};

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"298");
    bytes.extend_from_slice(&388u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    for (reference, role) in [(0u32, 6u32), (354, 2), (376, 3)] {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&role.to_le_bytes());
    }
    push_genesis_block(&mut bytes, 0x202);
    let annotation_byte_offset = bytes.len();
    bytes.extend_from_slice(&[0xaa, 0xbb, 0xcc]);
    push_reference(&mut bytes, 390);
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for reference in [376u32, 354] {
        push_reference(&mut bytes, reference);
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&[0; 4]);
    let paired_byte_offset = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"287");
    bytes.extend_from_slice(&388u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    push_reference(&mut bytes, 201);
    bytes.extend_from_slice(&[0; 6]);
    bytes.resize(paired_byte_offset + 59, 0);
    let mut stream = vec![0; 58];
    stream.extend(bytes);
    stream.resize(1000, 0);
    let archive = archive(&stream, true);
    // The real owner uses the OwnerCompanionParameter frame order.
    let parameters = [parameter_at(392, 900)];
    let owners = [owner_at(390, 13, 392, 391, 800)];
    let points = [point_at(354), point_at(376)];
    let entities = [sketch_header(201)];
    let companions = [DesignParameterCompanion::unbound(
        format!("f3d:{STREAM}:parameter-companion#0"),
        0,
        "408".to_owned().try_into().unwrap(),
        391,
        390,
        std::num::NonZeroU64::MIN,
        42,
    )];
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let inputs = DimensionDecodeInputs {
            scan, placements: &[], parameters: &parameters, owners: &owners,
            companions: &companions, scopes: &[], headers: &[], points: &points, curves: &[],
        };
        for target in [None, Some("f3d companion boundaries"), Some("scan F3D dimension sketch points"), Some("scan F3D dimension sketch entities")] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = u64::MAX;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut records = RecordOffsetCache::new(&ctx).unwrap();
            let _probe = target.map(|operation| cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None));
            let result = decode_dimension_annotation_frames(&ctx, &inputs, &mut records, &entities);
            if let Some(target) = target {
                let Err(CodecError::ResourceLimit(original)) = result else { panic!("the real reached annotation index must refuse"); };
                assert_eq!(original.operation, target);
                assert_eq!(ctx.resource_refusal(), Some(original));
                let Err(CodecError::ResourceLimit(later)) = ctx.charge_work(0, "after annotation index refusal") else { panic!("the original refusal must remain fused"); };
                assert_eq!(later, original);
            } else {
                let frames = result.unwrap();
                assert_eq!(frames.len(), 1);
                let frame = &frames[0];
                assert!(frame.id.ends_with(":design-dimension-annotation-frame#58"));
                assert_eq!(frame.companion_record_index, Some(391));
                assert_eq!(frame.governing_owner_record_index, 390);
                assert_eq!(frame.governing_companion_record_index, 391);
                assert_eq!(frame.entity_genesis, 0x202);
                assert_eq!(frame.annotation_byte_offset(), u64_from_index(58 + annotation_byte_offset));
                assert_eq!(frame.clone().into_draft().annotation_bytes, [0xaa, 0xbb, 0xcc]);
                assert_eq!(frame.operands()[0].geometry_record_index, None);
                assert_eq!(frame.clone().into_draft().return_members.iter().map(|member| member.value.get()).collect::<Vec<_>>(), [376, 354]);
                assert_eq!(frame.paired_byte_offset(), u64_from_index(58 + paired_byte_offset));
                assert_eq!(frame.owner_reference, 201);
            }
        }
    });
}
