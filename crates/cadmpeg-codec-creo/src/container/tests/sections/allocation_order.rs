// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

fn modern_table(marker: &str, entry: &str, offset: &str, length: &str, expanded: &str) -> Vec<u8> {
    let mut bytes = format!("{:<80}\n", "#UGC_TOC 2 1 81 17").into_bytes();
    let row = format!("{entry} {offset} {length} {expanded}");
    assert!(row.len() <= 80);
    bytes.extend_from_slice(format!("{row:<80}\n").as_bytes());
    assert_eq!(bytes.len(), 162);
    bytes.extend_from_slice(format!("#{marker}\nabc").as_bytes());
    bytes
}

fn assert_rejected_without_storage(bytes: &[u8], legacy: bool) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &policy).expect("root");
    let mut section_storage = ctx
        .reserve_scoped(0, "test section roster storage")
        .expect("empty storage");
    let mut parse = || {
        if legacy {
            super::super::super::legacy_toc_sections(&ctx, &mut section_storage, bytes, 0)
        } else {
            super::super::super::toc_sections(&ctx, &mut section_storage, bytes, 0)
        }
    };
    assert!(parse().expect("rejected borrowed directory row").is_empty());
    let original = ctx
        .charge_work_limit(u64::MAX, "after rejected TOC row")
        .expect_err("seed original work refusal");
    assert!(matches!(parse(), Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn modern_toc_invalid_numeric_fields_do_not_construct_raw_names() {
    for (marker, entry) in [("Body", "Body"), ("ModelView#42", "ModelView 42")] {
        let length = format!("{:x}", marker.len() + 5);
        for (offset, length, expanded) in [
            ("zz", length.as_str(), "0"),
            ("a2", "zz", "0"),
            ("a2", length.as_str(), "zz"),
        ] {
            assert_rejected_without_storage(
                &modern_table(marker, entry, offset, length, expanded),
                false,
            );
        }
    }
}

#[test]
fn modern_toc_out_of_range_or_mismatched_markers_do_not_construct_raw_names() {
    for (marker, entry) in [("Body", "Body"), ("ModelView#42", "ModelView 42")] {
        let length = format!("{:x}", marker.len() + 5);
        for (offset, length) in [("ffff", length.as_str()), ("a2", "ffff"), ("a2", "0")] {
            assert_rejected_without_storage(
                &modern_table(marker, entry, offset, length, "0"),
                false,
            );
        }
    }
    for (marker, entry) in [
        ("Bady", "Body"),
        ("ModelView#43", "ModelView 42"),
        ("ModelWire#42", "ModelView 42"),
    ] {
        let length = format!("{:x}", marker.len() + 5);
        assert_rejected_without_storage(&modern_table(marker, entry, "a2", &length, "0"), false);
    }
}

#[test]
fn legacy_toc_rejects_the_complete_region_before_copying_its_name() {
    let mut bytes =
        b"#Pro/ENGINEER  TM  Version H-01-21\n@Toc 52 0\n0 52 ->\n@entry 53 10\n1 53 [1]\n"
            .to_vec();
    let row_width = b"2 53 BasicData 00000000 00000000 0 983####\n".len();
    let offset = bytes.len() + row_width;
    bytes.extend_from_slice(format!("2 53 BasicData {offset:08x} 0000ffff 0 983####\n").as_bytes());
    assert_eq!(bytes.len(), offset);
    bytes.extend_from_slice(b"#BasicData\nabc");
    assert_rejected_without_storage(&bytes, true);
}

#[test]
fn deferred_toc_names_keep_decorated_and_variable_modelview_identity() {
    for (marker, entry, normalized) in [
        ("ND:0:VisibGeom:1", "ND:0:VisibGeom:1", "VisibGeom"),
        ("ModelView#cam_a", "ModelView cam_a", "ModelView"),
    ] {
        let length = marker.len() + 5;
        let bytes = modern_table(marker, entry, "a2", &format!("{length:x}"), "2a");
        let sections = crate::decode::with_test_decode_ctx(|ctx| {
            super::super::super::toc_sections(
                ctx,
                &mut ctx
                    .reserve_scoped(0, "test section roster storage")
                    .expect("empty storage"),
                &bytes,
                0,
            )
        })
        .expect("complete directory");
        let [section] = sections.as_slice() else {
            panic!("one complete section");
        };
        assert_eq!(section.section.raw_name(), marker);
        assert_eq!(section.section.name(), normalized);
        assert_eq!(section.section.offset(), 162);
        assert_eq!(section.section.length(), length);
        assert_eq!(section.section.expanded_length, Some(42));
        assert_eq!(section.region, &bytes[162..]);
    }
}
