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
        bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
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
