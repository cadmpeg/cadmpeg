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
    let mut section_storage = ctx.reserve_scoped(0, "test section roster storage").expect("empty storage");
    for source in [b"".as_slice(), b"x".as_slice()] {
        assert!(super::super::scan_sections(&ctx, &mut section_storage, source, 0)
            .expect("no framing pair or header").is_empty());
    }
    let persistence = crate::legacy::Persistence::default();
    assert_eq!(super::super::legacy_first_quilt_ptr(&ctx, &persistence)
        .expect("no legacy value rows"), None);
    let original = ctx.charge_work_limit(1, "seed empty container visits refusal")
        .expect_err("zero work cap");
    assert_eq!((original.used, original.additional), (0, 1));
    assert!(matches!(super::super::scan_sections(&ctx, &mut section_storage, &[], 0),
        Err(CodecError::ResourceLimit(r)) if r == original));
    assert!(matches!(super::super::legacy_first_quilt_ptr(&ctx, &persistence),
        Err(CodecError::ResourceLimit(r)) if r == original));
}

#[test]
fn section_framing_admits_two_present_pairs_and_no_terminal_visit() {
    let sections = crate::test_support::assert_work_boundaries(
        &["creo section framing scan"], |ctx| {
            let mut storage = ctx.reserve_scoped(0, "test section roster storage")?;
            super::super::scan_sections(ctx, &mut storage, b"abc", 0)
        });
    assert!(sections.is_empty());
    let refusal = |source: &[u8]| {
        let error = crate::test_support::last_refusal_at(&[],
            ResourceDimension::WorkUnits, "creo section framing scan", |ctx| {
                let mut storage = ctx.reserve_scoped(0, "test section roster storage")?;
                super::super::scan_sections(ctx, &mut storage, source, 0)
            });
        let CodecError::ResourceLimit(refusal) = error else {
            panic!("section framing work boundary");
        };
        refusal
    };
    assert_ne!(refusal(b"ab"), refusal(b"abc"));
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
        |ctx| super::super::toc_sections(ctx, &mut ctx.reserve_scoped(0, "test section roster storage").expect("empty storage"), bytes, 0).map(|_| ()),
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
            super::super::toc_sections(ctx, &mut ctx.reserve_scoped(0, "test section roster storage").expect("empty storage"), bytes, 0).map(|rows| rows.is_empty()))
            .expect("skip next-directory row and absent tail"));
    }
}

#[test]
fn present_section_headers_preserve_source_bounds_and_identity() {
    let data = b"\n#Body\nabc";
    let sections = crate::test_support::assert_work_boundaries(
        &["creo section header traversal"],
        |ctx| super::super::scan_sections(ctx, &mut ctx.reserve_scoped(0, "test section roster storage").expect("empty storage"), data, 0),
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

#[test]
fn empty_container_rosters_are_free_and_keep_original_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let ids = std::collections::BTreeSet::new();
    for refused in [false, true] {
        if refused { ctx.charge_work_limit(1, "container roster seed").expect_err("zero cap"); }
        let results = [
            super::super::loop_array_sections(&ctx, &[], &[], &[]).map(|v| v.is_empty()),
            super::super::loop_array_scan(&ctx, &[]).map(|v| v.frames.is_empty() && v.records.is_empty()),
            super::super::feature_rows(&ctx, &[], &ids).map(|v| v.is_empty()),
            super::super::feature_definitions(&ctx, &[]).map(|v| v.is_empty()),
            super::super::feature_row_definitions(&ctx, &[]).map(|v| v.is_empty()),
            super::super::feature_geometry_tables(&ctx, &[], &[]).map(|v| v.is_empty()),
            super::super::feature_affected_ids(&ctx, &[], &[]).map(|v| v.is_empty()),
            super::super::feature_revolution_extents(&ctx, &[], &[], &[]).map(|v| v.is_empty()),
            super::super::feature_operations(&ctx, &[]).map(|v| v.is_empty()),
            super::super::depdb_recipe_rows(&ctx, &[]).map(|v| v.is_empty()),
            super::super::collect_section_records_result::<u32>(
                &ctx, std::iter::empty(), |_| panic!("absent section decoder"),
                |_, _| panic!("absent relocation"), |_| panic!("absent ordering"),
            ).map(|v| v.is_empty()),
        ];
        for result in results {
            if refused {
                let original = ctx.resource_refusal().expect("seeded original");
                assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
            } else { assert!(result.expect("empty output")); }
        }
        assert_eq!(ctx.resource_refusal().is_some(), refused);
    }
}

#[test]
fn singleton_section_aggregate_admits_source_and_relocation_without_ordering() {
    let source = b"body";
    let section = super::super::Section::scan_for_test("Body".into(), 0, source.len(), None, source)
        .expect("complete section");
    let records = crate::test_support::assert_work_boundaries(
        &["fixture aggregate source", "creo section record relocation traversal"], |ctx| {
            let sections = ctx.admit_iter(std::slice::from_ref(&section), "fixture aggregate source")?;
            super::super::collect_section_records_result(
                ctx, sections.map(Ok), |bytes| {
                    assert_eq!(bytes, source);
                    let mut records = Vec::new();
                    ctx.reserve_vec(&mut records, 1, "fixture aggregate record")?;
                    records.push(7u32);
                    Ok(records)
                }, |value, base| { assert_eq!(base, 0); assert_eq!(*value, 7); Ok(()) },
                |_| panic!("singleton order projection cannot execute"),
            )
        });
    assert_eq!(records, [7]);
}

#[test]
fn singleton_topology_and_pcurve_ordering_executes_no_scan_or_move() {
    let row = crate::curve::CurveTopologyRow { id: 7, type_byte: 0x13, feature_id: 1,
        directions: [1, 0xf6], faces: [None; 2], next_edges: [7, 7], offset: 12 };
    let pcurve = crate::curve::PcurveEndpoints { curve_id: 7, faces: [None; 2],
        face_0_endpoints: [[0.0; 2]; 2], face_1_endpoints: [[0.0; 2]; 2], offset: 12 };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let mut rows = vec![row.clone()];
    let mut pcurves = vec![pcurve.clone()];
    super::super::append_topology_rows(&ctx, &mut rows, std::iter::empty(), "absent topology source")
        .expect("no append or ordering work");
    super::super::append_legacy_curve_witnesses(&ctx, &mut rows, &mut pcurves, &[], &[])
        .expect("no witness append or ordering work");
    assert_eq!(rows, [row]);
    assert_eq!(pcurves, [pcurve]);
    let original = ctx.charge_work_limit(1, "singleton topology seed").expect_err("zero cap");
    assert!(matches!(super::super::append_topology_rows(&ctx, &mut rows,
        std::iter::empty(), "absent topology source"), Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert!(matches!(super::super::append_legacy_curve_witnesses(&ctx, &mut rows, &mut pcurves, &[], &[]),
        Err(CodecError::ResourceLimit(actual)) if actual == original));
}
