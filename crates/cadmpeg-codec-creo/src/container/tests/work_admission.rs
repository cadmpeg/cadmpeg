// SPDX-License-Identifier: Apache-2.0
use super::super::{read_array_count, structural_feature_ids, Section};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn parent_feature_zero_ids_refuse_search_and_entry_work() {
    let bytes = b"parent_feats\0\xf8\x03\0\0\0";
    let section = Section::scan_for_test("VisibGeom".into(), 0, bytes.len(), None, bytes)
        .expect("bounded section");
    let sections = [section];
    let ids = crate::test_support::assert_work_boundaries(
        &[
            "creo structural feature sections",
            "creo parent-feature search",
            "creo parent-feature entries",
        ],
        |ctx| structural_feature_ids(ctx, &sections, &[], &[]),
    );
    assert!(ids.is_empty());
}

#[test]
fn geometry_census_charges_each_namespace_pass() {
    let region = [0; 64];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let boundary = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "creo geometry census search",
        |ctx| {
            read_array_count(ctx, &region, b"srf_array")?;
            read_array_count(ctx, &region, b"crv_array")
        },
    );
    let CodecError::ResourceLimit(boundary) = boundary else {
        panic!("resource boundary");
    };
    policy.limits.max_work_units = boundary
        .used
        .checked_add(boundary.additional)
        .expect("work need")
        - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(
        read_array_count(&ctx, &region, b"srf_array").expect("first scan"),
        None
    );
    let error =
        read_array_count(&ctx, &region, b"crv_array").expect_err("independent scan needs work");
    let CodecError::ResourceLimit(refusal) = error else {
        panic!("resource refusal");
    };
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(refusal.operation, "creo geometry census search");
    assert_eq!(ctx.resource_refusal(), Some(refusal));
}

#[test]
fn geometry_census_rejects_truncated_structural_counts() {
    for bytes in [
        b"srf_array\0\xf8".as_slice(),
        b"srf_array\0\xf8\x81".as_slice(),
    ] {
        let error =
            crate::decode::with_test_decode_ctx(|ctx| read_array_count(ctx, bytes, b"srf_array"))
                .expect_err("count is incomplete");
        assert!(matches!(error, CodecError::Malformed(_)));
    }
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| read_array_count(
            ctx,
            b"srf_array\0\xf8\0",
            b"srf_array"
        ))
        .expect("complete zero count"),
        Some(0)
    );
}

#[test]
fn parent_feature_arrays_reject_truncated_counts_and_entries() {
    for bytes in [
        b"parent_feats\0\xf8".as_slice(),
        b"parent_feats\0\xf8\x81".as_slice(),
        b"parent_feats\0\xf8\x01\x81".as_slice(),
    ] {
        let sections = [
            Section::scan_for_test("VisibGeom".into(), 0, bytes.len(), None, bytes)
                .expect("section"),
        ];
        let error = crate::decode::with_test_decode_ctx(|ctx| {
            structural_feature_ids(ctx, &sections, &[], &[])
        })
        .expect_err("array is incomplete");
        assert!(matches!(error, CodecError::Malformed(_)));
    }
}

#[test]
fn loop_array_section_duplicates_copy_only_surviving_names() {
    let data = b"loop_array\0";
let sections =
        [Section::scan_for_test("VisibGeom".into(), 0, data.len(), None, data).expect("section")];
let selected = crate::decode::with_test_decode_ctx(|ctx|
super::super::loop_array_sections(ctx, &sections, &sections, &[]))
.expect("two identical loop section witnesses");
assert_eq!(selected.len(), 1);
assert_eq!(selected[0].section.raw_name(), "VisibGeom");
assert_eq!(selected[0].section.offset(), 0);
assert_eq!(selected[0].region, data);
    let retained = selected[0].section.raw_name.capacity()
        + selected.capacity() * std::mem::size_of::<super::super::ScannedSection<'_>>();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(retained).expect("one surviving name");
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let bounded = super::super::loop_array_sections(&ctx, &sections, &sections, &[])
        .expect("duplicate selections copy only one name");
    assert_eq!(bounded.len(), 1);
    assert_eq!(bounded[0].section.raw_name(), selected[0].section.raw_name());
    assert_eq!(bounded[0].section.offset(), selected[0].section.offset());
    assert_eq!(bounded[0].region, selected[0].region);
    let resource = ctx.charge_retained_limit(u64::MAX, "loop section live names").expect_err("read live names");
    assert_eq!(resource.used, u64::try_from(retained).expect("actual vector and name backing"));

}

#[test]
fn appended_topology_row_deduplication_refuses_work() {
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo append topology rows rows deduplication",
        |ctx| {
            let mut rows = vec![crate::curve::CurveTopologyRow {
                id: 1,
                type_byte: 8,
                feature_id: 4,
                directions: [1, 1],
                faces: [None; 2],
                next_edges: [0; 2],
                offset: 0,
            }];
            let original = rows.clone();
            rows.push(rows[0].clone());
            super::super::append_topology_rows(
                ctx,
                &mut rows,
                ctx.admit_iter([], "creo topology row append traversal")?,
                "creo test topology aggregation",
            )?;
            assert_eq!(rows, original);
            Ok::<_, CodecError>(())
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo append topology rows rows deduplication")
    );
}

#[test]
fn appended_legacy_pcurve_deduplication_refuses_work() {
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo append legacy curve witnesses pcurves deduplication",
        |ctx| {
            let mut topology = Vec::new();
            let mut pcurves = vec![crate::curve::PcurveEndpoints {
                curve_id: 1,
                faces: [None; 2],
                face_0_endpoints: [[0.0; 2]; 2],
                face_1_endpoints: [[0.0; 2]; 2],
                offset: 0,
            }];
            let original = pcurves.clone();
            pcurves.push(pcurves[0].clone());
            super::super::append_legacy_curve_witnesses(ctx, &mut topology, &mut pcurves, &[], &[])?;
            assert!(topology.is_empty());
            assert_eq!(pcurves, original);
            Ok::<_, CodecError>(())
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo append legacy curve witnesses pcurves deduplication")
    );
}

#[test]
fn cmnm_forbidden_name_byte_refuses_before_invalid_name() {
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "creo CMNM forbidden name byte traversal",
        |ctx| super::super::cmnm_model_name(ctx, b"#- CMNM 001\0"),
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo CMNM forbidden name byte traversal"));
}

#[test]
fn cmnm_forbidden_name_search_charges_only_visited_bytes() {
    let refusal = |input: &[u8]| {
        let error = crate::test_support::last_refusal_at(
            &[],
            ResourceDimension::WorkUnits,
            "creo CMNM forbidden name byte traversal",
            |ctx| super::super::cmnm_model_name(ctx, input),
        );
        let CodecError::ResourceLimit(resource) = error else {
            panic!("expected visited-byte refusal");
        };
        assert_eq!(resource.additional, 1);
        resource
    };
    let first = refusal(b"#- CMNM 003\0ab");
    let last = refusal(b"#- CMNM 003ab\0");
    // Both routes have identical setup; the later match visits two more bytes.
    assert_eq!(last.used, first.used + 2);
}

#[test]
fn skipped_sections_still_require_traversal_work() {
    let bytes = b"#Other\n";
    let sections =
        [Section::scan_for_test("Other".into(), 0, bytes.len(), None, bytes).expect("section")];
    let selected =
        crate::test_support::assert_work_boundaries(&["creo section traversal"], |ctx| {
            super::super::nonvisible_geometry_sections(ctx, &sections)
        });
    assert!(selected.is_empty());
}

#[test]
fn native_model_name_search_stops_before_unvisited_sections() {
    let bytes = b"model_name\0part\0";
    let first =
        Section::scan_for_test("Other".into(), 0, bytes.len(), None, bytes).expect("section");
    let mut sections = vec![first.clone()];
    let boundary = |sections: &[super::super::ScannedSection<'_>]| {
        let error = crate::test_support::last_refusal_at(
            &[],
            ResourceDimension::WorkUnits,
            "creo native model-name section selection",
            |ctx| super::super::native_model_name(ctx, sections),
        );
        let CodecError::ResourceLimit(resource) = error else {
            panic!("work refusal")
        };
        resource
    };
    let one = boundary(&sections);
    sections.extend(std::iter::repeat_n(first, 64));
    assert_eq!(one, boundary(&sections));
    let name =
        crate::decode::with_test_decode_ctx(|ctx| super::super::native_model_name(ctx, &sections))
            .expect("search");
    assert_eq!(name, Some(("part".into(), 11)));
}

#[test]
fn legacy_witness_merges_admit_each_source_before_copying() {
    let source_topology = [crate::curve::CurveTopologyRow {
        id: 7,
        type_byte: 0x13,
        feature_id: 1,
        directions: [0; 2],
        faces: [None; 2],
        next_edges: [0; 2],
        offset: 7,
    }];
    let source_pcurves = [crate::curve::PcurveEndpoints {
        curve_id: 7,
        faces: [None; 2],
        face_0_endpoints: [[0.0; 2]; 2],
        face_1_endpoints: [[0.0; 2]; 2],
        offset: 7,
    }];
    let (topology, pcurves) = crate::test_support::assert_work_boundaries(
        &[
            "creo topology row append traversal",
            "creo legacy pcurve append traversal",
        ],
        |ctx| {
            let mut topology = Vec::new();
            let mut pcurves = Vec::new();
            super::super::append_legacy_curve_witnesses(
                ctx,
                &mut topology,
                &mut pcurves,
                &source_topology,
                &source_pcurves,
            )?;
            Ok((topology, pcurves))
        },
    );
    assert_eq!(topology, source_topology);
    assert_eq!(pcurves, source_pcurves);
}

#[test]
fn primitive_scalar_merge_admits_the_decoded_rows() {
    let bytes = b"\xe0\x06p1\0\xf8\x01\0";
    let section = super::super::ExpandedSection {
        name: "SolidPrimdata".into(),
        source_offset: 0,
        compressed_length: bytes.len(),
        data: bytes.to_vec(),
    };
    let scan = crate::test_support::assert_work_boundaries(
        &["creo primitive scalar append traversal"],
        |ctx| super::super::scan_primitives(ctx, std::slice::from_ref(&section)),
    );
    assert_eq!(scan.scalar_arrays.len(), 1);
}

#[test]
fn completed_feature_ids_retain_only_the_ordered_output() {
    let refusal = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::RetainedBytes,
        "creo ordered feature ids",
        |ctx| super::super::complete_feature_ids(ctx, std::collections::BTreeSet::new(), [4]),
    );
    let CodecError::ResourceLimit(resource) = refusal else {
        panic!("ordered output refusal");
    };
    assert_eq!(resource.used, 0, "scratch nodes use no retained bytes");
    let ids = crate::decode::with_test_decode_ctx(|ctx| {
        super::super::complete_feature_ids(ctx, std::collections::BTreeSet::new(), [4])
    })
    .expect("one final feature ID");
    assert_eq!(ids, [4]);
}

#[test]
fn legacy_toc_fixed_count_prefix_is_free() {
    let bytes = b"\n@Toc 1 0\n0 1 ->\n@entry 2 10\n1 2 [1]\n";
    assert!(crate::decode::with_test_decode_ctx(|ctx| super::super::legacy_toc_sections(
        ctx, &mut ctx.reserve_scoped(0, "test section roster storage")?, bytes, 0))
        .expect("valid count with absent rows").is_empty());
    let malformed = b"\n@Toc 1 0\n0 1 ->\n@entry 2 10\n1 2 [x\n";
    let refusal = crate::test_support::last_refusal_at(&[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits, "creo text field boundary", |ctx|
        super::super::legacy_toc_sections(ctx,
            &mut ctx.reserve_scoped(0, "test section roster storage")?, malformed, 0));
    let cadmpeg_core::CodecError::ResourceLimit(refusal) = refusal else { panic!("field boundary"); };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = refusal.used.checked_add(refusal.additional).expect("field work");
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let mut storage = ctx.reserve_scoped(0, "test section roster storage").expect("storage");
    assert!(super::super::legacy_toc_sections(&ctx, &mut storage, malformed, 0)
        .expect("fixed count prefix adds no work").is_empty());
}
