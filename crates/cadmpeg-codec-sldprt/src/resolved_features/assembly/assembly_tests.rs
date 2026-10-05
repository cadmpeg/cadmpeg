//! Tests for the `assembly` module.

use super::super::CLASS_MARKER;
use super::legacy_feature_input_section;

#[test]
fn legacy_feature_input_section_is_an_exact_numeric_config_stream() {
    assert!(legacy_feature_input_section("Contents/Config-0"));
    assert!(legacy_feature_input_section("Contents\\Config-37"));
    assert!(!legacy_feature_input_section("Contents/Config-0-Partition"));
    assert!(!legacy_feature_input_section("Contents/Config-name"));
    assert!(!legacy_feature_input_section("Other/Config-0"));
}

#[test]
fn legacy_sketch_object_stream_requires_a_sketch_and_entity_declaration() {
    let declaration = |name: &str| {
        let mut bytes = CLASS_MARKER.to_vec();
        bytes.extend_from_slice(&u16::try_from(name.len()).unwrap().to_le_bytes());
        bytes.extend_from_slice(name.as_bytes());
        bytes
    };
    let mut payload = declaration("sgSketch");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test context");
    assert!(!super::legacy_sketch_object_stream(&ctx, &payload).unwrap());

    payload.extend_from_slice(&declaration("sgPointHandle"));
    assert!(super::legacy_sketch_object_stream(&ctx, &payload).unwrap());

    assert!(!super::legacy_sketch_object_stream(&ctx, &declaration("sgPointHandle")).unwrap());
}

#[test]
fn feature_input_parent_identity_propagates_format_work_refusal() {
    let stream = crate::container::CompoundStream {
        path: cadmpeg_ir::StreamName::try_from("Contents/ResolvedFeatures".to_owned()).unwrap(),
        directory_id: 12,
        start_sector: 0,
        payload: Vec::new(),
        decoded_payload: None,
        ps_streams: Vec::new(),
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    assert!(matches!(
        super::feature_input_lane(
            &ctx, crate::container::Section::Compound(&stream), "Contents/ResolvedFeatures",
            "resolved-features", &mut cadmpeg_ir::annotations::Annotations::default(),
        ),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                && limit.operation == "format SLDPRT supplemental feature-input identity"
    ));
    let lane = super::feature_input_lane(
        &cadmpeg_test_support::service_decode_context(),
        crate::container::Section::Compound(&stream),
        "Contents/ResolvedFeatures",
        "resolved-features",
        &mut cadmpeg_ir::annotations::Annotations::default(),
    )
    .unwrap();
    assert_eq!(lane.id, "sldprt:feature-input:resolved-features#12");
}
