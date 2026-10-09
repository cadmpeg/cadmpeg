// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn section_absent_pairs_and_legacy_rows_are_free_and_keep_original_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    for source in [b"".as_slice(), b"x".as_slice()] {
        assert!(super::super::scan_sections(&ctx, source, 0)
            .expect("no framing pair or header").is_empty());
    }
    let persistence = crate::legacy::Persistence::default();
    assert_eq!(super::super::legacy_first_quilt_ptr(&ctx, &persistence)
        .expect("no legacy value rows"), None);
    let original = ctx.charge_work_limit(1, "seed empty container visits refusal")
        .expect_err("zero work cap");
    assert_eq!((original.used, original.additional), (0, 1));
    assert!(matches!(super::super::scan_sections(&ctx, &[], 0),
        Err(CodecError::ResourceLimit(r)) if r == original));
    assert!(matches!(super::super::legacy_first_quilt_ptr(&ctx, &persistence),
        Err(CodecError::ResourceLimit(r)) if r == original));
}

#[test]
fn section_framing_admits_two_present_pairs_and_no_terminal_visit() {
    for allowed in 0..=2 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = allowed;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let result = super::super::scan_sections(&ctx, b"abc", 0);
        if allowed < 2 {
            let CodecError::ResourceLimit(r) = result.expect_err("next framing pair") else {
                panic!("work refusal");
            };
            assert_eq!(r.dimension, ResourceDimension::WorkUnits);
            assert_eq!(r.operation, "creo section framing scan");
            assert_eq!((r.used, r.additional), (allowed, 1));
            assert!(matches!(super::super::scan_sections(&ctx, &[], 0),
                Err(CodecError::ResourceLimit(original)) if original == r));
        } else {
            assert!(result.expect("two framing pairs").is_empty());
            let r = ctx.charge_work_limit(1, "after two framing pairs").expect_err("exact cap");
            assert_eq!((r.used, r.additional), (2, 1));
        }
    }
}

#[test]
fn missing_declared_toc_rows_do_not_add_row_visit_work() {
    // Both directories have one physical 81-byte row. The second declaration
    // states another row beyond EOF. Only the physical row is traversed.
    let directory = |declared| {
        let mut bytes = format!("{:<80}\n", format!("#UGC_TOC 2 {declared} 81 17")).into_bytes();
        bytes.extend_from_slice(format!("{:<80}\n", "NEXT_TOC_ENTRY").as_bytes());
        bytes
    };
    let one = directory(1);
    let missing_tail = directory(2);
    let refusal = |bytes: &[u8]| crate::test_support::last_refusal_at(
        &[], ResourceDimension::WorkUnits, "creo TOC row traversal",
        |ctx| super::super::toc_sections(ctx, bytes, 0).map(|_| ()),
    );
    let CodecError::ResourceLimit(one_refusal) = refusal(&one) else {
        panic!("present row refusal");
    };
    let CodecError::ResourceLimit(tail_refusal) = refusal(&missing_tail) else {
        panic!("present row refusal");
    };
    assert_eq!(one_refusal, tail_refusal);
    for bytes in [&one, &missing_tail] {
        assert!(crate::decode::with_test_decode_ctx(|ctx|
            super::super::toc_sections(ctx, bytes, 0).map(|rows| rows.is_empty()))
            .expect("skip next-directory row and absent tail"));
    }
}

#[test]
fn present_section_headers_preserve_source_bounds_and_identity() {
    let data = b"\n#Body\nabc";
    let sections = crate::test_support::assert_work_boundaries(
        &["creo section header traversal"],
        |ctx| super::super::scan_sections(ctx, data, 0),
    );
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].section.raw_name(), "Body");
    assert_eq!(sections[0].section.offset(), 1);
    assert_eq!(sections[0].section.end(), data.len());
}

#[test]
fn absent_parent_feature_entry_keeps_original_malformed_route() {
    let bytes = b"parent_feats\0\xf8\x01";
    let sections = [super::super::Section::scan_for_test(
        "VisibGeom".into(), 0, bytes.len(), None, bytes,
    ).expect("bounded source section")];
    let error = crate::decode::with_test_decode_ctx(|ctx|
        super::super::structural_feature_ids(ctx, &sections, &[], &[]))
        .expect_err("declared entry has no source byte");
    assert!(matches!(error, CodecError::Malformed(_)));
    assert!(error.to_string().contains("incomplete parent-feature entry"));
}

#[test]
fn invalid_section_extents_are_free_and_keep_original_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    for (offset, end) in [(1, 0), (0, 2)] {
        assert!(super::super::Section::scan(&ctx, "Body".to_owned(), offset, end, None, &[0])
            .expect("invalid extent precedes name parsing").is_none());
    }
    let original = ctx.charge_work_limit(1, "seed invalid section refusal").expect_err("zero cap");
    for (offset, end) in [(1, 0), (0, 2)] {
        assert!(matches!(super::super::Section::scan(&ctx, "Body".to_owned(), offset, end, None, &[0]),
            Err(CodecError::ResourceLimit(r)) if r == original));
    }
}

#[test]
fn wrong_container_signature_is_fixed_and_keeps_original_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    assert!(matches!(super::super::scan_bytes(&ctx, b"wrong".as_slice()),
        Err(CodecError::WrongFormat(message)) if message == "missing Creo #UGC:2 signature"));
    let original = ctx.charge_work_limit(1, "seed wrong container signature refusal")
        .expect_err("zero work cap");
    for bytes in [b"".as_slice(), b"wrong".as_slice(), b"#UGC:2".as_slice()] {
        assert!(matches!(super::super::scan_bytes(&ctx, bytes),
            Err(CodecError::ResourceLimit(r)) if r == original));
    }
}
