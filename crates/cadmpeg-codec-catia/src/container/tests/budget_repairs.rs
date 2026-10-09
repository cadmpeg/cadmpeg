// SPDX-License-Identifier: Apache-2.0
//! Container admission and interval-index regressions.

use super::super::*;
use super::{summary_preview_segment, test_descriptor};

#[test]
fn nested_magic_absence_refuses_unadmitted_search() {
    crate::test_support::with_work_limit(0, |ctx| {
        let bytes = [0; 256];
        let CodecError::ResourceLimit(limit) = parse_stream_directory(ctx, &bytes)
            .expect_err("the absent nested magic still requires a search")
        else {
            panic!("work refusal")
        };
        assert_eq!(limit.operation, "catia_nested_magic_scan");
        assert_eq!(ctx.resource_refusal(), Some(limit));
        assert!(matches!(parse_stream_directory(ctx, &bytes),
            Err(CodecError::ResourceLimit(repeated)) if repeated == limit));
    });
}

#[test]
fn duplicate_data_descriptor_does_not_charge_the_descriptor_suffix() {
    let mut descriptors = vec![test_descriptor("Data", 0, 0); 10_000];
    for descriptor in &mut descriptors[2..] {
        descriptor.name = "irrelevant".into();
    }
    let directory = InnerDir {
        inner: 0,
        descriptors,
    };
    assert!(crate::test_support::with_work_limit(2, |ctx| {
        outer_container_declarations(ctx, &[], &directory)
    })
    .expect("the second Data descriptor ends the scan")
    .is_empty());
}

#[test]
fn conflicting_save_versions_charge_only_visited_segments() {
    let first = summary_preview_segment();
    let mut second = first.clone();
    let release = second
        .windows(11)
        .position(|bytes| bytes == b"<Release>27")
        .expect("release");
    second[release + 10] = b'8';
    let mut data = first.clone();
    data.extend(second);
    let segments = crate::test_support::with_service_context(|ctx| {
        finjpl_segments(ctx, &BodyExtent::whole(&data))
    })
    .expect("segments");
    let mut segments_with_suffix = segments.clone();
    segments_with_suffix.extend(std::iter::repeat_n(
        FinjplSegment {
            range: 0..0,
            type_word: 0,
            name: None,
        },
        10_000,
    ));
    // Five tags make ten bounded searches per segment. Value admission, parsing
    // and copying fit six further byte passes; two segment visits end at conflict.
    let work = u64::try_from(16 * data.len() + 2).expect("small fixture work");
    for source in [&segments, &segments_with_suffix] {
        assert!(crate::test_support::with_work_limit(work, |ctx| {
            last_save_version_in_segments(ctx, &data, source)
        })
        .expect("the conflicting prefix ends the scan")
        .is_none());
    }
}

#[test]
fn rejected_directory_extent_candidate_releases_storage() {
    let count_offset = 16 + 0x50;
    let start = count_offset + 4;
    let mut bytes = vec![0; start + 20];
    bytes[..DIR_MAGIC.len()].copy_from_slice(DIR_MAGIC);
    bytes[count_offset..start].copy_from_slice(&1u32.to_be_bytes());
    bytes[start + 4..start + 8].copy_from_slice(&1u32.to_be_bytes());
    bytes[start + 8..start + 12].copy_from_slice(&1u32.to_be_bytes());
    assert!(crate::test_support::with_retained_limit(0, |ctx| {
        parse_directory_region(ctx, &bytes, 0, 0, bytes.len())
    })
    .expect("the extent is scratch until its header agrees")
    .is_none());
}

#[test]
fn missing_brep_surface_descriptor_does_not_retain_main_stream() {
    let directory = InnerDir {
        inner: 0,
        descriptors: vec![test_descriptor("MainDataStream", 0, 16)],
    };
    assert!(crate::test_support::with_retained_limit(0, |ctx| {
        brep_stream(ctx, &[0; 16], &directory)
    })
    .expect("both descriptors precede reconstruction")
    .is_none());
}

#[test]
fn duplicate_save_versions_retain_only_the_selected_date() {
    let segment = summary_preview_segment();
    let mut data = segment.clone();
    data.extend_from_slice(&segment);
    let segments = crate::test_support::with_service_context(|ctx| {
        finjpl_segments(ctx, &BodyExtent::whole(&data))
    })
    .expect("segments");
    let expected = crate::test_support::with_service_context(|ctx| {
        last_save_version_in_segments(ctx, &segment, &segments[..1])
    })
    .expect("version")
    .expect("summary date");
    let retained = u64::try_from(expected.build_date.len()).expect("small date");
    assert_eq!(
        crate::test_support::with_retained_limit(retained, |ctx| {
            last_save_version_in_segments(ctx, &data, &segments)
        })
        .expect("only one date is retained"),
        Some(expected)
    );
}

#[test]
fn rejected_logical_stream_promotes_no_discarded_extent_bytes() {
    let mut descriptor = test_descriptor("MainDataStream", 1, 2);
    descriptor.extents.push(Extent { phys_off: 9, phys_len: 2, flags: 0 });
    crate::test_support::with_retained_limit(0, |ctx| {
        let (stream, storage) = reconstruct_logical_stream(ctx, b"0123456789", &descriptor, 0)
            .expect("invalid extent rejects the complete stream");
        assert!(storage.commit_value(stream).expect("empty result owns no backing").is_empty());
        assert_eq!(ctx.resource_refusal(), None);
        let valid = test_descriptor("MainDataStream", 1, 2);
        let (stream, storage) = reconstruct_logical_stream(ctx, b"0123456789", &valid, 0)
            .expect("valid temporary stream");
        assert_eq!(stream, b"12");
        let Err(CodecError::ResourceLimit(limit)) = storage.commit_value(stream) else {
            panic!("the actual retained stream still needs storage");
        };
        assert_eq!(limit.dimension, cadmpeg_core::decode::ResourceDimension::RetainedBytes);
        assert_eq!(limit.operation, "catia_logical_stream_bytes");
        assert_eq!(ctx.resource_refusal(), Some(limit));
        assert!(matches!(ctx.charge_work(0, "after stream promotion refusal"),
            Err(CodecError::ResourceLimit(later)) if later == limit));
    });
}

#[test]
fn logical_stream_roster_refuses_before_installing_an_unreserved_lease() {
    use cadmpeg_core::decode::{refusal_probe::RefusalProbe, ResourceDimension};
    let bytes = super::outer_directory_catpart();
    let scan = crate::test_support::with_service_context(|ctx| scan_bytes(ctx, bytes))
        .expect("complete fixture scan");
    let _probe = RefusalProbe::arm(ResourceDimension::CollectionItems,
        "catia_logical_stream_reservations", None);
    crate::test_support::with_collection_limit(u64::MAX, |ctx| {
        let Err(CodecError::ResourceLimit(limit)) = logical_record_streams(ctx, &scan) else {
            panic!("the actual parallel reservation roster must refuse");
        };
        assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
        assert_eq!(limit.operation, "catia_logical_stream_reservations");
        assert_eq!(ctx.resource_refusal(), Some(limit));
        assert!(matches!(ctx.charge_work(0, "after stream roster refusal"),
            Err(CodecError::ResourceLimit(later)) if later == limit));
    });
}

#[test]
fn container_output_promotions_preserve_refusal_and_live_parent_storage() {
    use cadmpeg_core::decode::ResourceDimension;
    let summary = summary_preview_segment();
    let directory = InnerDir { inner: 0, descriptors: vec![
        test_descriptor("MainDataStream", 0, 4),
        test_descriptor("SurfacicReps", 4, 2),
    ] };
    for operation in ["catia_last_save_version_storage", "catia_logical_stream_bytes"] {
        let run = |ctx: &DecodeContext<'_>| {
            if operation == "catia_last_save_version_storage" {
                last_save_version(ctx, &summary).map(|_| ())
            } else {
                brep_stream(ctx, b"123456", &directory).map(|_| ())
            }
        };
        let refused = crate::test_support::with_retained_refusal(&summary, operation, |ctx| {
            let result = run(ctx);
            let Err(CodecError::ResourceLimit(limit)) = &result else {
                panic!("actual container output promotion refusal");
            };
            assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
            assert_eq!(limit.operation, operation);
            assert_eq!(ctx.resource_refusal(), Some(*limit));
            assert!(matches!(ctx.charge_work(0, "after container output refusal"),
                Err(CodecError::ResourceLimit(later)) if later == *limit));
            result
        });
        assert!(matches!(refused, Err(CodecError::ResourceLimit(_))));
    }
    crate::test_support::with_retained_limit(0, |ctx| -> Result<(), CodecError> {
        let mut parent = ctx.reserve_scoped(0, "test container output parent")?;
        let version = parent.with_storage(|| last_save_version(ctx, &summary))?
            .expect("summary version");
        let brep = parent.with_storage(|| brep_stream(ctx, b"123456", &directory))?
            .expect("complete BREP");
        let main = parent.with_storage(|| main_data_stream(ctx, b"123456", &directory))?
            .expect("canonical main stream");
        assert_eq!(version.build_date, "03-10-2017.22.00");
        assert_eq!(brep, b"123456");
        assert_eq!(main, b"1234");
        assert_eq!(ctx.resource_refusal(), None);
        drop(main);
        drop(brep);
        drop(version);
        drop(parent);
        Ok(())
    }).expect("actual outputs stay under their live parent");
}
