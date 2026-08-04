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
    assert!(!super::legacy_sketch_object_stream(&payload));

    payload.extend_from_slice(&declaration("sgPointHandle"));
    assert!(super::legacy_sketch_object_stream(&payload));

    assert!(!super::legacy_sketch_object_stream(&declaration(
        "sgPointHandle"
    )));
}
