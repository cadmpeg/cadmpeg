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
