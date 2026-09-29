// SPDX-License-Identifier: Apache-2.0

use crate::native::features::feature_projected_curve_references;
use crate::native::features::feature_surface_construction_references;
use crate::native::features::feature_surface_construction_payloads;
use crate::native::features::feature_thru_curve_construction_envelopes;
use crate::native::features::draft::feature_draft_construction_references;
use crate::native::features::draft::feature_draft_construction_payloads;
use crate::native::features::draft::feature_draft_construction_graph_payloads;
use crate::native::features::draft::feature_draft_construction_fixed_lanes;
use crate::native::features::draft::feature_draft_construction_binary32_lanes;
use crate::native::features::draft::FeatureDraftConstructionGraphPayload;
use crate::native::features::draft::FeatureDraftConstructionReference;
use crate::native::features::draft::FeatureDraftConstructionIndexLane;

fn reference_container(label: &'static str, payload: Vec<u8>) -> crate::container::Container<'static> {
    let part = crate::test_support::test_om::composed_feature_history_payload(
        &[(&[0xff; 4], label, payload)], &[],
    );
    let file = crate::test_support::test_prt::prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", part)]);
    crate::test_support::with_decode_context(move |ctx| crate::container::scan_bytes(ctx, file))
        .expect("synthetic feature reference container")
}

fn projected_curve_container() -> crate::container::Container<'static> {
    let payload = b"\0\x01\x02\xf1\x02\xc8\xf1\x02\xc9\x80\x57\x00\x02\x01\xf1\x02\xca\xff\x01\x02\x02\x7d\0".to_vec();
    reference_container("CPROJ", payload)
}

fn surface_container() -> crate::container::Container<'static> {
    reference_container("SKIN", surface_payload_bytes())
}

fn surface_payload_bytes() -> Vec<u8> {
    b"\x3f\x00\x00\x01\x00\xf1\x02\x46\xf1\x02\x47\xf1\x02\x48\x01\x09\x03\x03\x04\x05\x02\x01\x01\x01\x01\x09\xf1\x02\x49\xf1\x02\x4a\xf1\x02\x4b\xf1\x02\x4c\xf1\x02\x4d\xf1\x02\x4e\xf1\x02\x4f\xf1\x02\x50\x00\x03\x03\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xf1\x02\x56\xf1\x02\x57\xf1\x02\x58\x01\x01\xff\xff\xff\xff\xff\xff\xff\xff\xff\x00\x00\x00\x00\x01\x02".to_vec()
}

fn surface_payload_container() -> crate::container::Container<'static> {
    let store = (0..600).map(|_| b"A".as_slice()).collect::<Vec<_>>();
    let part = crate::test_support::test_om::composed_feature_history_payload(
        &[(&[0xff; 4], "SKIN", surface_payload_bytes())], &store,
    );
    let file = crate::test_support::test_prt::prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", part)]);
    crate::test_support::with_decode_context(move |ctx| crate::container::scan_bytes(ctx, file))
        .expect("synthetic surface payload container")
}

fn draft_container() -> crate::container::Container<'static> {
    let mut payload = b"\x67\x00\x00\x01\x00\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\x03\xff\xff\xff\xff\xff\xff\xff\xff\x01\x03\x80\x94\x82\x49".to_vec();
    payload.extend_from_slice(b"\x01\x02\xf1\x1b\x7c\x01\x02\xf1\x1b\x7d\x68\x2f\x70\x62\x4d\xd2\xf1\xa9\xfc\x03\x50\x44\x00\x00\x01\x46\x8a\x2a\x01\xa3\x60\x10\x01\x01\x01\x04\x02\x01\x02\x01\x00\x00\x00\x00\x01\xf1\x1b\x7e\xff\x00\x00\x00\xf1\x1b\x7f\xff");
    payload.extend_from_slice(b"\x81\x5e\x80\xb8\x01\x03\x02\x01\x02\x01\x01\x01\x00\x00\x00\x29\x29\x0c\x00");
    reference_container("DRAFT", payload)
}

fn thru_curve_container() -> crate::container::Container<'static> {
    reference_container("THRU_CURVE", b"\x13\x00\x00\x01\x00\xf1\x01\x21\xf1\x01\x22\xf1\x01\x23\x01\x08\x02\x03\x03\x04\x01\x01\x01\x01\x07\xf1\x01\x24\xf1\x01\x25\xf1\x01\x26\xf1\x01\x27\xf1\x01\x28\xf1\x01\x29\x04\x01\xa0\x5e\x38\x13\x01\x03".to_vec())
}

fn projected_curve_route_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let container = projected_curve_container();
    let records = crate::test_support::with_decode_context(|ctx| {
        feature_projected_curve_references(ctx, &container)
    }).expect("admitted projected curve references");
    assert_eq!(records.len(), 3);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    feature_projected_curve_references(&ctx, &container)
        .expect_err("projected curve reference resource limit")
}

fn reference_route_refusal<T>(
    container: crate::container::Container<'static>,
    expected_count: usize,
    route: for<'ctx, 'input> fn(
        &cadmpeg_core::decode::DecodeContext<'ctx>,
        &crate::container::Container<'input>,
    ) -> Result<Vec<T>, cadmpeg_core::CodecError>,
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let records = crate::test_support::with_decode_context(|ctx| route(ctx, &container))
        .expect("admitted feature references");
    assert_eq!(records.len(), expected_count);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    route(&ctx, &container).err().expect("feature reference resource limit")
}

#[test]
fn surface_reference_route_refuses_collection_limit() {
    let error = reference_route_refusal(surface_container(), 14, feature_surface_construction_references,
        |policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn surface_reference_route_refuses_retained_limit() {
    let error = reference_route_refusal(surface_container(), 14, feature_surface_construction_references,
        |policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn surface_reference_route_refuses_scoped_limit() {
    let error = reference_route_refusal(surface_container(), 14, feature_surface_construction_references,
        |policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn surface_reference_route_refuses_work_limit() {
    let error = reference_route_refusal(surface_container(), 14, feature_surface_construction_references,
        |policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}

#[test]
fn draft_reference_route_refuses_collection_limit() {
    let error = reference_route_refusal(draft_container(), 4, feature_draft_construction_references,
        |policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn draft_reference_route_refuses_retained_limit() {
    let error = reference_route_refusal(draft_container(), 4, feature_draft_construction_references,
        |policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn draft_reference_route_refuses_scoped_limit() {
    let error = reference_route_refusal(draft_container(), 4, feature_draft_construction_references,
        |policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn draft_reference_route_refuses_work_limit() {
    let error = reference_route_refusal(draft_container(), 4, feature_draft_construction_references,
        |policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}

#[test]
fn thru_curve_envelope_route_refuses_collection_limit() {
    let error = reference_route_refusal(thru_curve_container(), 1, feature_thru_curve_construction_envelopes,
        |policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn thru_curve_envelope_route_refuses_retained_limit() {
    let error = reference_route_refusal(thru_curve_container(), 1, feature_thru_curve_construction_envelopes,
        |policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn thru_curve_envelope_route_refuses_scoped_limit() {
    let error = reference_route_refusal(thru_curve_container(), 1, feature_thru_curve_construction_envelopes,
        |policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn thru_curve_envelope_route_refuses_work_limit() {
    let error = reference_route_refusal(thru_curve_container(), 1, feature_thru_curve_construction_envelopes,
        |policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}

fn surface_payload_route_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let container = surface_payload_container();
    let references = crate::test_support::with_decode_context(|ctx| {
        feature_surface_construction_references(ctx, &container)
    }).expect("admitted surface references");
    assert_eq!(references.len(), 14);
    let payloads = crate::test_support::with_decode_context(|ctx| {
        feature_surface_construction_payloads(ctx, &container, &references)
    }).expect("admitted surface payload");
    assert_eq!(payloads.len(), 1);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    feature_surface_construction_payloads(&ctx, &container, &references)
        .err().expect("surface payload resource limit")
}

fn draft_resolved_lane() -> FeatureDraftConstructionIndexLane {
    serde_json::from_str(
        r#"{"id":"nx:feature-history:draft-construction-index-lane#0-0000000000","operation_label":"nx:feature-history:operation-label#0-0000000000","declared_count":3,"indices":[1,2],"raw_indices":[[1],[2]],"data_blocks":["nx:om-data-blocks-0:block#1","nx:om-data-blocks-0:block#2"],"source_offsets":[24,25]}"#,
    ).expect("resolved draft lane")
}

fn draft_small_store_container() -> crate::container::Container<'static> {
    let part = crate::test_support::test_om::composed_feature_history_payload(&[], &[b"A", b"B"]);
    let file = crate::test_support::test_prt::prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", part)]);
    crate::test_support::with_decode_context(move |ctx| {
        crate::container::scan_bytes(ctx, file)
    }).expect("synthetic draft payload container")
}

fn draft_payload_route_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let lane = draft_resolved_lane();
    let container = draft_small_store_container();
    let payloads = crate::test_support::with_decode_context(|ctx| {
        feature_draft_construction_payloads(ctx, &container, &[lane.clone()])
    }).expect("admitted draft payload");
    assert_eq!(payloads.len(), 1);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    feature_draft_construction_payloads(&ctx, &container, &[lane])
        .err().expect("draft payload resource limit")
}

#[test]
fn draft_payload_route_refuses_collection_limit() {
    let error = draft_payload_route_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn draft_payload_route_refuses_retained_limit() {
    let error = draft_payload_route_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn draft_payload_route_refuses_scoped_limit() {
    let error = draft_payload_route_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn draft_payload_route_refuses_work_limit() {
    let error = draft_payload_route_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}

fn draft_graph_references() -> Vec<FeatureDraftConstructionReference> {
    (0..4).map(|ordinal| {
        let index = if ordinal == 0 { 1 } else { 2 };
        serde_json::from_value(serde_json::json!({
            "id": format!("nx:feature-history:draft-construction-reference#0-0000000000-{ordinal:010}"),
            "operation_label": "nx:feature-history:operation-label#0-0000000000",
            "ordinal": ordinal,
            "object_index": index,
            "raw_object_index": [240, index],
            "data_block": format!("nx:om-data-blocks-0:block#{index}"),
            "source_offset": 100 + ordinal,
        })).expect("resolved draft graph reference")
    }).collect()
}

fn draft_graph_fixture_with_content(
    bytes: &[u8],
) -> (crate::container::Container<'static>, FeatureDraftConstructionGraphPayload) {
    let part = crate::test_support::test_om::composed_feature_history_payload(&[], &[bytes, b""]);
    let file = crate::test_support::test_prt::prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", part)]);
    let container = crate::test_support::with_decode_context(move |ctx| {
        crate::container::scan_bytes(ctx, file)
    }).expect("synthetic draft graph content container");
    let lane = draft_resolved_lane();
    let references = draft_graph_references();
    let mut payloads = crate::test_support::with_decode_context(|ctx| {
        feature_draft_construction_graph_payloads(ctx, &container, &[lane], &references)
    }).expect("admitted draft graph content");
    assert_eq!(payloads.len(), 1);
    (container, payloads.remove(0))
}

fn draft_fixed_bytes() -> Vec<u8> {
    let discriminator = [
        0x25, 0x25, 0x41, 0x00, 0x04, 0x01, 0x07, 0x01, 0xc0, 0x45, 0x10, 0x00, 0x80, 0x86, 0x02,
        0x00, 0x01, 0x00,
    ];
    let mut bytes = vec![0xff];
    bytes.extend_from_slice(&discriminator);
    bytes.extend_from_slice(&[0x30, 0x40, 0, 0, 0, 0, 0, 0]);
    bytes.extend_from_slice(&[0xb0, 0xc0, 0, 0, 0, 0, 0, 0]);
    bytes.push(0);
    bytes
}

fn draft_fixed_route_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let (container, payload) = draft_graph_fixture_with_content(&draft_fixed_bytes());
    let lanes = crate::test_support::with_decode_context(|ctx| {
        feature_draft_construction_fixed_lanes(ctx, &container, &[payload.clone()])
    }).expect("admitted draft fixed lane");
    assert_eq!(lanes.len(), 1);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    feature_draft_construction_fixed_lanes(&ctx, &container, &[payload])
        .err().expect("draft fixed lane resource limit")
}

#[test]
fn draft_fixed_route_refuses_collection_limit() {
    let error = draft_fixed_route_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn draft_fixed_route_refuses_retained_limit() {
    let error = draft_fixed_route_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn draft_fixed_route_refuses_scoped_limit() {
    let error = draft_fixed_route_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn draft_fixed_route_refuses_work_limit() {
    let error = draft_fixed_route_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}

fn draft_binary32_bytes() -> Vec<u8> {
    let discriminator = [
        0x90, 0x18, 0x45, 0x01, 0x04, 0x01, 0x04, 0x01, 0xc0, 0x45, 0x04, 0x04, 0x80, 0x86, 0x02,
        0x00, 0x03, 0x00,
    ];
    let mut bytes = vec![0xff];
    bytes.extend_from_slice(&discriminator);
    bytes.extend_from_slice(&[0x4f, 0x80, 0, 0]);
    bytes.extend_from_slice(&[0xcf, 0x80, 0, 0]);
    bytes.push(0);
    bytes
}

fn draft_binary32_route_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let (container, payload) = draft_graph_fixture_with_content(&draft_binary32_bytes());
    let lanes = crate::test_support::with_decode_context(|ctx| {
        feature_draft_construction_binary32_lanes(ctx, &container, &[payload.clone()])
    }).expect("admitted draft binary32 lane");
    assert_eq!(lanes.len(), 1);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    feature_draft_construction_binary32_lanes(&ctx, &container, &[payload])
        .err().expect("draft binary32 lane resource limit")
}

#[test]
fn draft_binary32_route_refuses_collection_limit() {
    let error = draft_binary32_route_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn draft_binary32_route_refuses_retained_limit() {
    let error = draft_binary32_route_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn draft_binary32_route_refuses_scoped_limit() {
    let error = draft_binary32_route_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn draft_binary32_route_refuses_work_limit() {
    let error = draft_binary32_route_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}

fn draft_graph_payload_route_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let lane = draft_resolved_lane();
    let references = draft_graph_references();
    let container = draft_small_store_container();
    let payloads = crate::test_support::with_decode_context(|ctx| {
        feature_draft_construction_graph_payloads(ctx, &container, &[lane.clone()], &references)
    }).expect("admitted draft graph payload");
    assert_eq!(payloads.len(), 1);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    feature_draft_construction_graph_payloads(&ctx, &container, &[lane], &references)
        .err().expect("draft graph payload resource limit")
}

#[test]
fn draft_graph_payload_route_refuses_collection_limit() {
    let error = draft_graph_payload_route_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn draft_graph_payload_route_refuses_retained_limit() {
    let error = draft_graph_payload_route_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn draft_graph_payload_route_refuses_scoped_limit() {
    let error = draft_graph_payload_route_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn draft_graph_payload_route_refuses_work_limit() {
    let error = draft_graph_payload_route_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}

#[test]
fn surface_payload_route_refuses_collection_limit() {
    let error = surface_payload_route_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn surface_payload_route_refuses_retained_limit() {
    let error = surface_payload_route_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn surface_payload_route_refuses_scoped_limit() {
    let error = surface_payload_route_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn surface_payload_route_refuses_work_limit() {
    let error = surface_payload_route_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}

#[test]
fn projected_curve_reference_route_refuses_collection_limit() {
    let error = projected_curve_route_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn projected_curve_reference_route_refuses_retained_limit() {
    let error = projected_curve_route_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn projected_curve_reference_route_refuses_scoped_limit() {
    let error = projected_curve_route_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn projected_curve_reference_route_refuses_work_limit() {
    let error = projected_curve_route_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}
