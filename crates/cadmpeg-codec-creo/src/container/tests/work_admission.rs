// SPDX-License-Identifier: Apache-2.0
use super::super::{read_array_count, structural_feature_ids, Section};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn parent_feature_zero_ids_refuse_search_and_entry_work() {
    let bytes = b"parent_feats\0\xf8\x03\0\0\0";
    let section =
        Section::scan("VisibGeom".into(), 0, bytes.len(), None, bytes).expect("bounded section");
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
    // Search admits the 64-byte haystack and the nine-byte namespace marker.
    policy.limits.max_work_units = 64 + cadmpeg_core::decode::u64_from_index(b"srf_array".len());
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
        let sections =
            [Section::scan("VisibGeom".into(), 0, bytes.len(), None, bytes).expect("section")];
        let error = crate::decode::with_test_decode_ctx(|ctx| {
            structural_feature_ids(ctx, &sections, &[], &[])
        })
        .expect_err("array is incomplete");
        assert!(matches!(error, CodecError::Malformed(_)));
    }
}

#[test]
fn legacy_toc_count_prefix_refuses_work() {
    let bytes = b"\n@Toc 1 0\n0 1 ->\n@entry 2 10\n1 2 [1]\n";
    let error = crate::test_support::last_refusal_at(
        &[], cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo legacy TOC count prefix",
        |ctx| super::super::legacy_toc_sections(ctx, bytes, 0),
    );
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo legacy TOC count prefix"));
}

#[test]
fn feature_identity_family_prefix_refuses_work() {
    let row = crate::feature::rows::FeatureRow {
        feature_id: 87,
        root_schema_class: Some(crate::feature::schema::SchemaClass::DatumPlane),
        stream_offset: 0, body: vec![0; 2].try_into().expect("row body"),
        body_offset: 0, offset: 0,
    };
    let reference = crate::feature::operations::FeatureReferenceName {
        feature_id: 87, name_bytes: b"Datum Plane id 87".to_vec(),
        own_reference_id: 10, reference_type: 1, offset: 0,
    };
    let structural = std::collections::BTreeSet::new();
    let error = crate::test_support::last_refusal_at(
        &[], cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo feature identity family prefix",
        |ctx| super::super::feature_row_has_model_identity(ctx, &row, &structural, &[], std::slice::from_ref(&reference)),
    );
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo feature identity family prefix"));
}

#[test]
fn feature_identity_ordinal_prefix_refuses_work() {
    let row = crate::feature::rows::FeatureRow {
        feature_id: 87,
        root_schema_class: Some(crate::feature::schema::SchemaClass::DatumPlane),
        stream_offset: 0, body: vec![0; 2].try_into().expect("row body"),
        body_offset: 0, offset: 0,
    };
    let reference = crate::feature::operations::FeatureReferenceName {
        feature_id: 87, name_bytes: b"Datum Plane id 87".to_vec(),
        own_reference_id: 10, reference_type: 1, offset: 0,
    };
    let structural = std::collections::BTreeSet::new();
    let error = crate::test_support::last_refusal_at(
        &[], cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo feature identity ordinal prefix",
        |ctx| super::super::feature_row_has_model_identity(ctx, &row, &structural, &[], std::slice::from_ref(&reference)),
    );
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo feature identity ordinal prefix"));
}

#[test]
fn feature_identity_datum_prefix_refuses_work() {
    let row = crate::feature::rows::FeatureRow {
        feature_id: 87,
        root_schema_class: Some(crate::feature::schema::SchemaClass::DatumPlane),
        stream_offset: 0, body: vec![0; 2].try_into().expect("row body"),
        body_offset: 0, offset: 0,
    };
    let reference = crate::feature::operations::FeatureReferenceName {
        feature_id: 87, name_bytes: b"DTM87".to_vec(),
        own_reference_id: 10, reference_type: 1, offset: 0,
    };
    let structural = std::collections::BTreeSet::new();
    let error = crate::test_support::last_refusal_at(
        &[], cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo feature identity datum prefix",
        |ctx| super::super::feature_row_has_model_identity(ctx, &row, &structural, &[], std::slice::from_ref(&reference)),
    );
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo feature identity datum prefix"));
}

#[test]
fn loop_array_section_deduplication_refuses_work() {
    let data = b"loop_array\0";
    let sections = [Section::scan("VisibGeom".into(), 0, data.len(), None, data).expect("section")];
    let error = crate::test_support::last_refusal_at(
        &[], cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo loop array sections selected deduplication", |ctx| super::super::loop_array_sections(ctx, &sections, &[], &[]),
    );
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo loop array sections selected deduplication"));
}

#[test]
fn appended_topology_row_deduplication_refuses_work() {
    
    let error = crate::test_support::last_refusal_at(
        &[], cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo append topology rows rows deduplication", |ctx| {
            let mut rows = vec![crate::curve::CurveTopologyRow {
        id: 1, type_byte: 8, feature_id: 4, directions: [1, 1],
        faces: [None; 2], next_edges: [0; 2], offset: 0,
    }];
            super::super::append_topology_rows(ctx, &mut rows, std::iter::empty(), "creo test topology aggregation")
        },
    );
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo append topology rows rows deduplication"));
}

#[test]
fn appended_legacy_pcurve_deduplication_refuses_work() {
    
    let error = crate::test_support::last_refusal_at(
        &[], cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo append legacy curve witnesses pcurves deduplication", |ctx| {
            let mut topology = Vec::new();
            let mut pcurves = vec![crate::curve::PcurveEndpoints {
                curve_id: 1, faces: [None; 2], face_0_endpoints: [[0.0; 2]; 2],
                face_1_endpoints: [[0.0; 2]; 2], offset: 0,
            }];
            super::super::append_legacy_curve_witnesses(ctx, &mut topology, &mut pcurves, &[], &[])
        },
    );
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo append legacy curve witnesses pcurves deduplication"));
}
