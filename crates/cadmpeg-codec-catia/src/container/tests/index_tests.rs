// SPDX-License-Identifier: Apache-2.0
//! Indexed containment and transient logical streams.

use super::super::*;
use super::{outer_directory_catpart, test_descriptor};

#[test]
fn long_overlapping_extent_queries_do_not_replay_short_prefixes() {
    let count = 4_096_u32;
    let declarations = ["long", "short"].map(|name| OuterContainerDeclaration {
        data_offset: 0,
        ordinal: 0,
        class_name: "class".into(),
        base_class: "base".into(),
        stream_name: name.into(),
    });
    let outer = InnerDir {
        inner: 0,
        descriptors: vec![
            test_descriptor("long", 0, 2 * count + 100),
            Descriptor {
                name: "short".into(),
                desc_offset: 0,
                extents: (0..count)
                    .map(|index| Extent {
                        phys_off: 2 * index + 1,
                        phys_len: 1,
                        flags: 0,
                    })
                    .collect(),
            },
        ],
    };
    crate::test_support::with_service_context(|ctx| {
        let index = outer_container_extent_index(ctx, &outer, &declarations).expect("extent index");
        crate::test_support::with_work_limit(4_096, |query_ctx| {
            for offset in 1..=64 {
                assert_eq!(
                    outer_container_for_extent(
                        query_ctx,
                        &index,
                        u64::from(2 * count + offset),
                        1
                    )?
                    .map(|declaration| declaration.stream_name.as_str()),
                    Some("long")
                );
            }
            Ok::<_, CodecError>(())
        })
        .expect("64 logarithmic containment queries");
    });
}

#[test]
fn logical_stream_storage_is_scoped_through_its_readers() {
    let bytes = outer_directory_catpart();
    let scan = crate::test_support::with_service_context(|ctx| scan_bytes(ctx, bytes))
        .expect("scan fixture");
    crate::test_support::with_retained_limit(0, |ctx| {
        for _ in 0..4 {
            let streams = logical_record_streams(ctx, &scan).expect("logical streams are scratch");
            assert_eq!(streams.streams.len(), 1);
            assert!(!streams.streams[0].is_empty());
        }
    });
}

#[test]
fn summary_declaration_join_has_one_scoped_name_index() {
    let count = 4_096;
    let declarations: Vec<_> = (0..count)
        .map(|index| OuterContainerDeclaration {
            data_offset: index,
            ordinal: u32::try_from(index).expect("small ordinal"),
            class_name: "Class".into(),
            base_class: "Base".into(),
            stream_name: format!("{index:08x}_12345678_12345678"),
        })
        .collect();
    let descriptors = declarations
        .iter()
        .map(|declaration| Descriptor {
            name: declaration.stream_name.clone(),
            desc_offset: declaration.data_offset,
            extents: Vec::new(),
        })
        .collect();
    let scan = ContainerScan {
        data: Vec::new().into(),
        outer_dir_offset: 0,
        outer_dir_length: 0,
        outer: Some(InnerDir {
            inner: 0,
            descriptors,
        }),
        inner: None,
        brep: None,
        main_data_stream: None,
        e5_record_range: None,
        previews: Vec::new(),
        last_save_version: None,
        external_references: Vec::new(),
        finjpl_segments: Vec::new(),
        outer_container_declarations: declarations,
        census: Census::default(),
        variant: Variant::Unknown,
    };
    let summary =
        crate::test_support::with_work_limit(u64::try_from(count * 10_000).expect("work"), |ctx| {
            summarize(ctx, &scan)
        })
        .expect("one keyed lookup per descriptor");
    assert_eq!(summary.entries.len(), count);
    for (index, entry) in summary.entries.iter().enumerate() {
        assert_eq!(entry.attributes["container_class"], "Class");
        assert_eq!(entry.attributes["container_ordinal"], index.to_string());
    }
}

fn declaration_bytes() -> Vec<u8> {
    let mut data = vec![0; 40];
    data[8..12].copy_from_slice(&[1, 0, 3, 0]);
    data[12..16].copy_from_slice(&2_u32.to_le_bytes());
    data[16..24].copy_from_slice(&[1, 0, 0x6c, 0, 2, 0, 0, 0]);
    data[32..36].copy_from_slice(&[2, 0, 0x81, 0x20]);
    data.extend(b"Class\0Base\0\0");
    data.extend([3, 0, 0xf7, 0, 3, 0, 0, 0]);
    for word in [0_u32, 1, 2, 3] {
        data.extend(word.to_be_bytes());
    }
    data.extend([0; 64]);
    data
}

#[test]
fn unmatched_and_duplicate_declarations_do_not_retain_name_alternatives() {
    let data = declaration_bytes();
    crate::test_support::with_retained_limit(0, |ctx| {
        assert!(parse_outer_container_declarations(ctx, &data, &[])
            .expect("unmatched alternatives are scratch")
            .is_empty());
    });
    let mut duplicate = data.clone();
    duplicate.extend(&data);
    duplicate.extend([0; 10_000]);
    let descriptors = [test_descriptor("1_00000002_3", 0, 0)];
    crate::test_support::with_retained_limit(0, |ctx| {
        assert!(
            parse_outer_container_declarations(ctx, &duplicate, &descriptors)
                .expect("duplicate output is discarded")
                .is_empty()
        );
    });
    let Err(CodecError::ResourceLimit(limit)) =
        crate::test_support::with_work_refusal("catia_container_declaration_scan", |ctx| {
            parse_outer_container_declarations(ctx, &duplicate, &descriptors)
        })
    else {
        panic!("candidate work refusal")
    };
    assert_eq!(limit.additional, 1);
}
