// SPDX-License-Identifier: Apache-2.0

use super::{
    FeatureIdenticalInstanceOutputLane, FeatureMultiInstanceOutputLane,
    FeaturePatternConstructionFixedLane, FeaturePatternTransformLane,
};

use crate::native::features::test_support::check_lane_wire;

const PATTERN_TRANSFORM_PAYLOAD: &[u8] = b"\xaa\x01\x03\x60\x01\x00\x00\x50\x54\x00\x00\x00\x01\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x02\x01\x01\x00\x00\xff\x00\x00\x60\x01\x00\x00\xd0\x54\x00\x00\x00\x01\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x9f\xfe\x01\x02\x00\x00\xff\x00\x00\x5f\x00\x00\x01";
const MULTI_INSTANCE_OUTPUT_PAYLOAD: &[u8] = b"\x3a\x00\x00\x01\x00\x00\x00\x00\x25\x01\x02\x26\x27\x01\x02\x65\x01\x02\x07\x28\x02\x02\x00\x3b\x09\x01\x02";
const IDENTICAL_INSTANCE_OUTPUT_PAYLOAD: &[u8] = b"\xaa\x34\x13\x01\x04\x14\x15\x01\x02\x16\x80\x20\x00\x02\x14\x15\x01\x02\x16\x0f\x00\x03\x14\x15\x01\x02\x16\x81\x23\x00\x04\x00\x05\xe0\x7f\xff\xff\xff\x00\x00\xbb";
const PATTERN_REFERENCE_PAYLOAD: &[u8] = b"\x61\xf1\x1b\x08\xff\x00\xff\x01\xf1\x1b\x09\xf1\x1b\x0a\x61\xf1\x1b\x0b\xff\x00\xff\x01\xf1\x1b\x0c\xf1\x1b\x0d\xff\x62\xf1\x1b\x0e\xf1\x1b\x0f\xff\x00\x00\x01\xf1\x1b\x10\xff\xff\xff\x01";
const PATTERN_COUNTED_PAYLOAD: &[u8] = b"\xaa\x01\x04\xf1\x06\xb1\xf1\x06\xb2\xf1\x06\xb3\x00\x00\x00\x37\xff\xff\x01\x00\x00\x00\x38\xff\x01\xff\xff\xff\xff\x01\xff";
const PATTERN_CONSTRUCTION_PAYLOAD: &[u8] = b"\x61\xf0\x01\xff\x00\xff\x01\xf0\x02\xf0\x03\x61\xf0\x04\xff\x00\xff\x01\xf0\x05\xf0\x06\xff\x62\xf0\x07\xf0\x08\xff\x00\x00\x01\xf0\x09\xff\xff\xff\x01";

fn pattern_construction_route_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    use crate::native::features::FeatureOperationLabel;
    let store = (0..600).map(|_| b"A".as_slice()).collect::<Vec<_>>();
    let part = crate::test_support::test_om::composed_feature_history_payload(
        &[(&[0xff; 4], "Pattern Feature", PATTERN_CONSTRUCTION_PAYLOAD.to_vec())], &store,
    );
    let file = crate::test_support::test_prt::prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", part),
    ]);
    let container = crate::test_support::with_decode_context(move |ctx| {
        crate::container::scan_bytes(ctx, file)
    }).expect("pattern construction container");
    let references = crate::test_support::with_decode_context(|ctx| {
        super::feature_pattern_references(ctx, &container)
    }).expect("pattern construction references");
    assert_eq!(references.len(), 9);
    assert!(references.iter().all(|reference| reference.data_block.is_some()));
    let labels = [FeatureOperationLabel {
        id: references[0].operation_label.clone(),
        section_link: "section#0".to_string(),
        ordinal: 0,
        value: "Pattern Feature".to_string(),
        objects: crate::om::header_references::HeaderReferences([None; 4]),
        stable_identity: None,
        source_offset: 0,
    }];
    let admitted = crate::test_support::with_decode_context(|ctx| {
        super::feature_pattern_construction_payloads(ctx, &container, &labels, &references)
    }).expect("admitted pattern construction payload");
    assert_eq!(admitted.len(), 1);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    super::feature_pattern_construction_payloads(&ctx, &container, &labels, &references)
        .expect_err("pattern construction resource limit")
}

#[test]
fn pattern_construction_route_refuses_collection_limit() {
    let error = pattern_construction_route_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn pattern_construction_route_refuses_retained_limit() {
    let error = pattern_construction_route_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn pattern_construction_route_refuses_scoped_limit() {
    let error = pattern_construction_route_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn pattern_construction_route_refuses_work_limit() {
    let error = pattern_construction_route_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}

fn pattern_reference_route_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let part = crate::test_support::test_om::composed_feature_history_payload(
        &[(&[0xff; 4], "Pattern Geometry", PATTERN_REFERENCE_PAYLOAD.to_vec())], &[],
    );
    let file = crate::test_support::test_prt::prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", part),
    ]);
    let container = crate::test_support::with_decode_context(move |ctx| {
        crate::container::scan_bytes(ctx, file)
    }).expect("pattern reference container");
    let admitted = crate::test_support::with_decode_context(|ctx| {
        crate::native::features::pattern::feature_pattern_references(ctx, &container)
    }).expect("admitted pattern references");
    assert_eq!(admitted.len(), 9);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    crate::native::features::pattern::feature_pattern_references(&ctx, &container)
        .expect_err("pattern reference resource limit")
}

#[test]
fn pattern_reference_route_refuses_collection_limit() {
    let error = pattern_reference_route_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn pattern_reference_route_refuses_retained_limit() {
    let error = pattern_reference_route_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn pattern_reference_route_refuses_scoped_limit() {
    let error = pattern_reference_route_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn pattern_reference_route_refuses_work_limit() {
    let error = pattern_reference_route_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}

fn pattern_output_lane_refusal<T>(
    label: &'static str,
    payload: &'static [u8],
    route: for<'ctx, 'input> fn(
        &cadmpeg_core::decode::DecodeContext<'ctx>,
        &crate::container::Container<'input>,
    ) -> Result<Vec<T>, cadmpeg_core::CodecError>,
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let part = crate::test_support::test_om::composed_feature_history_payload(
        &[(&[0xff; 4], label, payload.to_vec())], &[],
    );
    let file = crate::test_support::test_prt::prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", part),
    ]);
    let container = crate::test_support::with_decode_context(move |ctx| {
        crate::container::scan_bytes(ctx, file)
    }).expect("pattern output lane container");
    let admitted = crate::test_support::with_decode_context(|ctx| route(ctx, &container))
        .expect("admitted pattern output lane");
    assert_eq!(admitted.len(), 1);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    route(&ctx, &container).err().expect("pattern output lane resource limit")
}

macro_rules! pattern_output_lane_limit_tests {
    ($collection:ident, $retained:ident, $scoped:ident, $work:ident, $label:expr, $payload:expr, $route:path) => {
        #[test]
        fn $collection() {
            let error = pattern_output_lane_refusal($label, $payload, $route,
                |policy| policy.limits.max_collection_items = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
        }

        #[test]
        fn $retained() {
            let error = pattern_output_lane_refusal($label, $payload, $route,
                |policy| policy.limits.max_retained_bytes = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
        }

        #[test]
        fn $scoped() {
            let error = pattern_output_lane_refusal($label, $payload, $route,
                |policy| policy.limits.max_materialized_bytes = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
        }

        #[test]
        fn $work() {
            let error = pattern_output_lane_refusal($label, $payload, $route,
                |policy| policy.limits.max_work_units = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
        }
    };
}

pattern_output_lane_limit_tests!(
    pattern_transform_route_refuses_collection_limit,
    pattern_transform_route_refuses_retained_limit,
    pattern_transform_route_refuses_scoped_limit,
    pattern_transform_route_refuses_work_limit,
    "Pattern Feature",
    PATTERN_TRANSFORM_PAYLOAD,
    crate::native::features::pattern::feature_pattern_transform_lanes
);
pattern_output_lane_limit_tests!(
    multi_instance_output_route_refuses_collection_limit,
    multi_instance_output_route_refuses_retained_limit,
    multi_instance_output_route_refuses_scoped_limit,
    multi_instance_output_route_refuses_work_limit,
    "Multi Instance Output",
    MULTI_INSTANCE_OUTPUT_PAYLOAD,
    crate::native::features::pattern::feature_multi_instance_output_lanes
);
pattern_output_lane_limit_tests!(
    identical_instance_output_route_refuses_collection_limit,
    identical_instance_output_route_refuses_retained_limit,
    identical_instance_output_route_refuses_scoped_limit,
    identical_instance_output_route_refuses_work_limit,
    "IDENTICAL INSTANCE OUTPUT",
    IDENTICAL_INSTANCE_OUTPUT_PAYLOAD,
    crate::native::features::pattern::feature_identical_instance_output_lanes
);
pattern_output_lane_limit_tests!(
    pattern_counted_reference_route_refuses_collection_limit,
    pattern_counted_reference_route_refuses_retained_limit,
    pattern_counted_reference_route_refuses_scoped_limit,
    pattern_counted_reference_route_refuses_work_limit,
    "Pattern Feature",
    PATTERN_COUNTED_PAYLOAD,
    crate::native::features::pattern::feature_pattern_counted_reference_lanes
);

fn pattern_content_refusal<T>(
    bytes: Vec<u8>,
    route: for<'ctx, 'input> fn(
        &cadmpeg_core::decode::DecodeContext<'ctx>,
        &crate::container::Container<'input>,
        &[crate::native::features::FeatureConstructionPayload],
    ) -> Result<Vec<T>, cadmpeg_core::CodecError>,
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    use crate::native::features::payload_content::{FeaturePayloadBlock, FeaturePayloadContent};
    use crate::native::features::{FeatureConstructionOwner, FeatureConstructionPayload, FeaturePatternKind};
    let store = (0..600).map(|_| bytes.as_slice()).collect::<Vec<_>>();
    let part = crate::test_support::test_om::composed_feature_history_payload(
        &[(&[0xff; 4], "Pattern Feature", Vec::new())], &store,
    );
    let file = crate::test_support::test_prt::prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", part),
    ]);
    let container = crate::test_support::with_decode_context(move |ctx| {
        crate::container::scan_bytes(ctx, file)
    }).expect("pattern construction container");
    let payload = crate::test_support::with_decode_context(|ctx| {
        let blocks = crate::native::features::offset_data_block_bytes(ctx, &container)?;
        let id = blocks.keys().find(|key| key.ends_with(":block#1"))
            .cloned().expect("synthetic pattern block");
        let content: FeaturePayloadContent<Vec<FeaturePayloadBlock>> =
            FeaturePayloadContent::from_source(ctx, [id], &blocks)?
                .expect("pattern source block");
        Ok::<FeatureConstructionPayload, cadmpeg_core::CodecError>(FeatureConstructionPayload {
            id: "pattern-payload".to_string(),
            operation_label: "pattern-operation".to_string(),
            owner: FeatureConstructionOwner::Pattern {
                operation_kind: FeaturePatternKind::Feature,
                reference_layout: crate::om::pattern_references::PatternPayloadReferenceLayout::CanonicalGraph,
                construction_references: Vec::new(),
            },
            content,
        })
    }).expect("pattern construction payload");
    let admitted = crate::test_support::with_decode_context(|ctx| {
        route(ctx, &container, std::slice::from_ref(&payload))
    }).expect("admitted pattern construction result");
    assert_eq!(admitted.len(), 1);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    route(&ctx, &container, &[payload]).err()
        .expect("pattern construction resource limit")
}

fn pattern_string_bytes() -> Vec<u8> {
    b"\x66\x32\x03\x03A\0".to_vec()
}

fn pattern_fixed_bytes() -> Vec<u8> {
    let mut bytes = vec![0xff, 0x25, 0x25, 0x41, 0x00, 0x04, 0x01, 0x07, 0x01,
        0xc0, 0x45, 0x10, 0x00, 0x80, 0x86, 0x02, 0x00, 0x01, 0x00];
    bytes.extend_from_slice(&[0x30, 0x40, 0, 0, 0, 0, 0, 0]);
    bytes.extend_from_slice(&[0xb0, 0xc0, 0, 0, 0, 0, 0, 0]);
    bytes.push(0);
    bytes
}

macro_rules! pattern_content_limit_tests {
    ($collection:ident, $retained:ident, $scoped:ident, $work:ident, $bytes:expr, $route:path) => {
        #[test]
        fn $collection() {
            let error = pattern_content_refusal($bytes, $route,
                |policy| policy.limits.max_collection_items = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
        }

        #[test]
        fn $retained() {
            let error = pattern_content_refusal($bytes, $route,
                |policy| policy.limits.max_retained_bytes = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
        }

        #[test]
        fn $scoped() {
            let error = pattern_content_refusal($bytes, $route,
                |policy| policy.limits.max_materialized_bytes = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
        }

        #[test]
        fn $work() {
            let error = pattern_content_refusal($bytes, $route,
                |policy| policy.limits.max_work_units = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
        }
    };
}

pattern_content_limit_tests!(
    pattern_string_route_refuses_collection_limit,
    pattern_string_route_refuses_retained_limit,
    pattern_string_route_refuses_scoped_limit,
    pattern_string_route_refuses_work_limit,
    pattern_string_bytes(),
    crate::native::features::pattern::feature_pattern_construction_strings
);
pattern_content_limit_tests!(
    pattern_fixed_route_refuses_collection_limit,
    pattern_fixed_route_refuses_retained_limit,
    pattern_fixed_route_refuses_scoped_limit,
    pattern_fixed_route_refuses_work_limit,
    pattern_fixed_bytes(),
    crate::native::features::pattern::feature_pattern_construction_fixed_lanes
);

#[test]
fn pattern_fixed_lane_preserves_parallel_wire_and_requires_complete_tokens() {
    check_lane_wire::<FeaturePatternConstructionFixedLane>(
        r#"{"id":"lane","operation_label":"operation","construction_payload":"payload","ordinal":0,"values":[0.25,0.5],"markers":[48,176],"raw_values":[[32,0,0,0,0,0,0],[64,0,0,0,0,0,0]],"payload_offset":0,"value_payload_offsets":[18,26],"source_offset":100,"value_source_offsets":[118,126]}"#,
        &[
            "values",
            "markers",
            "raw_values",
            "value_payload_offsets",
            "value_source_offsets",
        ],
    );
}

#[test]
fn multi_instance_lane_preserves_wire_and_requires_complete_rows_and_references() {
    check_lane_wire::<FeatureMultiInstanceOutputLane>(
        r#"{"id":"lane","operation_label":"operation","declared_count":3,"selectors":[7,8],"raw_selectors":[[7],[8]],"ordinals":[2,2],"row_indices":[2,3],"instance_count":2,"trailing_object_indices":[9],"raw_trailing_object_indices":[[9]],"source_offset":100,"selector_source_offsets":[110,120],"trailing_object_index_source_offsets":[130]}"#,
        &[
            "selectors",
            "raw_selectors",
            "ordinals",
            "row_indices",
            "selector_source_offsets",
            "trailing_object_indices",
            "raw_trailing_object_indices",
            "trailing_object_index_source_offsets",
        ],
    );
}

#[test]
fn identical_instance_lane_preserves_wire_and_requires_complete_selectors() {
    let json = r#"{"id":"lane","operation_label":"operation","leading_schema_index":4,"count_schema_index":5,"row_schema_indices":[6,7,8],"declared_count":3,"selectors":[7,8],"raw_selectors":[[7],[8]],"source_offset":100,"selector_source_offsets":[110,120]}"#;
    check_lane_wire::<FeatureIdenticalInstanceOutputLane>(
        json,
        &["selectors", "raw_selectors", "selector_source_offsets"],
    );
    for (field, value) in [
        ("count_schema_index", serde_json::json!(253)),
        ("row_schema_indices", serde_json::json!([6, 7, 9])),
    ] {
        let mut malformed: serde_json::Value = serde_json::from_str(json).unwrap();
        malformed[field] = value;
        let error =
            serde_json::from_value::<FeatureIdenticalInstanceOutputLane>(malformed).unwrap_err();
        assert!(error.to_string().contains(field));
    }
}

#[test]
fn pattern_transform_lane_preserves_wire_and_requires_complete_rows() {
    let columns = &[
        "encodings",
        "values",
        "raw_values",
        "selectors",
        "raw_selectors",
        "value_source_offsets",
        "selector_source_offsets",
    ];
    check_lane_wire::<FeaturePatternTransformLane>(
        r#"{"id":"lane","operation_label":"operation","row_schema_index":3,"layout":"scalar_rows","declared_count":3,"encodings":["binary32","binary64"],"values":[2.5,4.0],"raw_values":[[80,32,0,0],[48,16,0,0,0,0,0,0]],"selectors":[7,8],"raw_selectors":[[7],[8]],"source_offset":100,"value_source_offsets":[110,120],"selector_source_offsets":[114,128]}"#,
        columns,
    );
    check_lane_wire::<FeaturePatternTransformLane>(
        r#"{"id":"lane","operation_label":"operation","row_schema_index":3,"layout":"wide_rows","declared_count":2,"encodings":["binary64","binary64","binary64","binary64","exact_one"],"values":[2.5,4.0,5.0,6.0,1.0],"raw_values":[[48,4,0,0,0,0,0,0],[48,16,0,0,0,0,0,0],[48,20,0,0,0,0,0,0],[48,24,0,0,0,0,0,0],[1]],"selectors":[7],"raw_selectors":[[7]],"source_offset":100,"value_source_offsets":[110,118,126,134,142],"selector_source_offsets":[143]}"#,
        columns,
    );
}

#[test]
fn pattern_rows_reject_scalar_families_outside_their_layout() {
    let narrow = r#"{"id":"lane","operation_label":"operation","row_schema_index":3,"layout":"scalar_rows","declared_count":2,"encodings":["exact_one"],"values":[1.0],"raw_values":[[1]],"selectors":[7],"raw_selectors":[[7]],"source_offset":100,"value_source_offsets":[110],"selector_source_offsets":[111]}"#;
    assert!(serde_json::from_str::<FeaturePatternTransformLane>(narrow).is_err());
    let wide = r#"{"id":"lane","operation_label":"operation","row_schema_index":3,"layout":"wide_rows","declared_count":2,"encodings":["binary64","binary64","binary64","binary64","exact_one"],"values":[2.5,4.0,5.0,6.0,1.0],"raw_values":[[48,4,0,0,0,0,0,0],[48,16,0,0,0,0,0,0],[48,20,0,0,0,0,0,0],[48,24,0,0,0,0,0,0],[1]],"selectors":[7],"raw_selectors":[[7]],"source_offset":100,"value_source_offsets":[110,118,126,134,142],"selector_source_offsets":[143]}"#;
    let mut wrong_first: serde_json::Value = serde_json::from_str(wide).unwrap();
    wrong_first["encodings"][0] = serde_json::json!("binary32");
    wrong_first["raw_values"][0] = serde_json::json!([80, 32, 0, 0]);
    assert!(serde_json::from_value::<FeaturePatternTransformLane>(wrong_first).is_err());
    let mut wrong_terminal: serde_json::Value = serde_json::from_str(wide).unwrap();
    wrong_terminal["encodings"][4] = serde_json::json!("binary64");
    wrong_terminal["raw_values"][4] = serde_json::json!([47, 240, 0, 0, 0, 0, 0, 0]);
    assert!(serde_json::from_value::<FeaturePatternTransformLane>(wrong_terminal).is_err());
    assert!(serde_json::from_str::<FeaturePatternTransformLane>(
        &wide.replace("\"row_schema_index\":3", "\"row_schema_index\":0")
    )
    .is_err());
}

#[test]
fn pattern_counted_references_preserve_wire_and_reject_token_disagreement() {
    let json = r#"{"id":"lane","operation_label":"operation","declared_count":2,"object_indices":[1],"raw_object_indices":[[240,1]],"data_blocks":[null],"source_offset":18,"object_index_source_offsets":[20]}"#;
    let lane: crate::native::features::pattern::FeaturePatternCountedReferenceLane =
        serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&lane).unwrap(), json);
    for raw in [vec![240, 2], vec![255], vec![240], vec![240, 1, 0]] {
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire["raw_object_indices"] = serde_json::json!([raw]);
        let error = serde_json::from_value::<
            crate::native::features::pattern::FeaturePatternCountedReferenceLane,
        >(wire)
        .unwrap_err();
        assert!(error.to_string().contains("raw_object_indices"), "{error}");
    }
}

#[test]
fn pattern_transform_selectors_require_matching_compact_tokens() {
    let json = r#"{"id":"lane","operation_label":"operation","row_schema_index":3,"layout":"scalar_rows","declared_count":2,"encodings":["binary32"],"values":[2.5],"raw_values":[[80,32,0,0]],"selectors":[4096],"raw_selectors":[[144,0]],"source_offset":100,"value_source_offsets":[110],"selector_source_offsets":[114]}"#;
    check_lane_wire::<FeaturePatternTransformLane>(json, &[]);
    for (field, value) in [
        ("selectors", serde_json::json!([4097])),
        ("raw_selectors", serde_json::json!([[144]])),
        ("raw_selectors", serde_json::json!([[144, 0, 0]])),
        ("raw_selectors", serde_json::json!([[255]])),
    ] {
        let mut malformed: serde_json::Value = serde_json::from_str(json).unwrap();
        malformed[field] = value;
        let error = serde_json::from_value::<FeaturePatternTransformLane>(malformed).unwrap_err();
        assert!(error.to_string().contains("selectors"), "{error}");
    }
}

#[test]
fn identical_instance_selectors_check_tokens_and_terminal_count_capacity() {
    let json = r#"{"id":"lane","operation_label":"operation","leading_schema_index":4,"count_schema_index":5,"row_schema_indices":[6,7,8],"declared_count":2,"selectors":[4096],"raw_selectors":[[144,0]],"source_offset":100,"selector_source_offsets":[110]}"#;
    check_lane_wire::<FeatureIdenticalInstanceOutputLane>(json, &[]);
    for (field, value) in [
        ("selectors", serde_json::json!([4097])),
        ("raw_selectors", serde_json::json!([[144]])),
        ("raw_selectors", serde_json::json!([[144, 0, 0]])),
        ("raw_selectors", serde_json::json!([[255]])),
    ] {
        let mut malformed: serde_json::Value = serde_json::from_str(json).unwrap();
        malformed[field] = value;
        let error =
            serde_json::from_value::<FeatureIdenticalInstanceOutputLane>(malformed).unwrap_err();
        assert!(error.to_string().contains("selectors"), "{error}");
    }
    for count in [253, 254] {
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire["declared_count"] = serde_json::json!(count + 1);
        wire["selectors"] = serde_json::json!(vec![1; count]);
        wire["raw_selectors"] = serde_json::json!(vec![vec![1]; count]);
        wire["selector_source_offsets"] = serde_json::json!(vec![110; count]);
        let result = serde_json::from_value::<FeatureIdenticalInstanceOutputLane>(wire.clone());
        if count == 253 {
            assert_eq!(serde_json::to_value(result.unwrap()).unwrap(), wire);
        } else {
            assert!(result.unwrap_err().to_string().contains("selectors"));
        }
    }
}

#[test]
fn multi_instance_groups_preserve_interleaving_and_reject_invalid_ordinals() {
    let json = r#"{"id":"lane","operation_label":"operation","declared_count":5,"selectors":[7,8,7,8],"raw_selectors":[[7],[8],[128,7],[128,8]],"ordinals":[2,2,3,3],"row_indices":[2,3,4,5],"instance_count":3,"trailing_object_indices":[9,256],"raw_trailing_object_indices":[[9],[144,1,0]],"source_offset":100,"selector_source_offsets":[110,120,130,140],"trailing_object_index_source_offsets":[150,160]}"#;
    check_lane_wire::<FeatureMultiInstanceOutputLane>(json, &[]);
    for (field, value, error_field) in [
        ("ordinals", serde_json::json!([1, 2, 3, 3]), "ordinals"),
        ("ordinals", serde_json::json!([2, 2, 2, 3]), "ordinals"),
        ("ordinals", serde_json::json!([2, 2, 4, 3]), "ordinals"),
        (
            "raw_selectors",
            serde_json::json!([[7], [8], [255], [128, 8]]),
            "selectors",
        ),
        (
            "raw_trailing_object_indices",
            serde_json::json!([[9], [240, 1]]),
            "trailing_object_indices",
        ),
        (
            "trailing_object_indices",
            serde_json::json!([9, 257]),
            "trailing_object_indices",
        ),
    ] {
        let mut malformed: serde_json::Value = serde_json::from_str(json).unwrap();
        malformed[field] = value;
        let error =
            serde_json::from_value::<FeatureMultiInstanceOutputLane>(malformed).unwrap_err();
        assert!(error.to_string().contains(error_field), "{error}");
    }
    let mut incomplete: serde_json::Value = serde_json::from_str(json).unwrap();
    incomplete["declared_count"] = serde_json::json!(4);
    for field in [
        "selectors",
        "raw_selectors",
        "ordinals",
        "row_indices",
        "selector_source_offsets",
    ] {
        incomplete[field].as_array_mut().unwrap().pop();
    }
    let error = serde_json::from_value::<FeatureMultiInstanceOutputLane>(incomplete).unwrap_err();
    assert!(error.to_string().contains("trailing_object_indices"));
}

#[test]
fn pattern_counted_references_require_nonempty_framed_payload_tokens() {
    let json = r#"{"id":"lane","operation_label":"operation","declared_count":3,"object_indices":[1,258],"raw_object_indices":[[240,1],[241,1,2]],"data_blocks":[null,"block"],"source_offset":18,"object_index_source_offsets":[20,22]}"#;
    let lane: super::FeaturePatternCountedReferenceLane = serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&lane).unwrap(), json);
    for (field, value) in [
        ("object_index_source_offsets", serde_json::json!([20, 23])),
        ("source_offset", serde_json::json!(u64::MAX)),
        ("raw_object_indices", serde_json::json!([[1], [241, 1, 2]])),
    ] {
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire[field] = value;
        let error = serde_json::from_value::<super::FeaturePatternCountedReferenceLane>(wire)
            .unwrap_err()
            .to_string();
        assert!(error.contains(field), "{error}");
    }
    let mut empty: serde_json::Value = serde_json::from_str(json).unwrap();
    empty["declared_count"] = serde_json::json!(1);
    for field in [
        "object_indices",
        "raw_object_indices",
        "data_blocks",
        "object_index_source_offsets",
    ] {
        empty[field] = serde_json::json!([]);
    }
    let error = serde_json::from_value::<super::FeaturePatternCountedReferenceLane>(empty)
        .unwrap_err()
        .to_string();
    assert!(error.contains("declared_count"), "{error}");
}
