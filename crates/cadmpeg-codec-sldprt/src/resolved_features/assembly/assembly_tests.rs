//! Tests for the `assembly` module.

use super::super::CLASS_MARKER;
use super::{configuration_name, legacy_feature_input_section, scalar_archive_classes};
use crate::carchive::reader::{ParameterRecord, V};
use crate::carchive::{Status, Walk};
use crate::records::ArchiveParameterClass;
use std::sync::Arc;

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
    assert!(!super::legacy_sketch_object_stream(&payload).unwrap());

    payload.extend_from_slice(&declaration("sgPointHandle"));
    assert!(super::legacy_sketch_object_stream(&payload).unwrap());

    assert!(!super::legacy_sketch_object_stream(&declaration("sgPointHandle")).unwrap());
}

fn parameter(class: &str, value_at: usize) -> ParameterRecord {
    ParameterRecord {
        class: Arc::from(class),
        name: V::None,
        value: 0.0,
        at: 0,
        value_at,
    }
}

#[test]
fn configuration_names_strip_the_resolved_features_suffix() {
    assert_eq!(
        configuration_name("Contents/Config-3-ResolvedFeatures").as_deref(),
        Some("Contents/Config-3")
    );
    assert_eq!(
        configuration_name("contents\\config-0-resolvedfeatures").as_deref(),
        Some("contents/config-0")
    );
    assert_eq!(configuration_name("Contents/Config-3"), None);
    assert_eq!(configuration_name("Contents/Config-3-Other"), None);
}

#[test]
fn scalar_archive_classes_map_each_value_offset() {
    let walk = Walk {
        status: Status::End,
        p: 0,
        trace: Vec::new(),
        parameters: vec![
            parameter("moLengthParameter_c", 42),
            parameter("moAngleParameter_c", 43),
            parameter("moNodeName_c", 44),
        ],
        named: Vec::new(),
        tops: 1,
    };
    let classes = scalar_archive_classes(&walk);
    assert_eq!(classes.get(&42), Some(&ArchiveParameterClass::Length));
    assert_eq!(classes.get(&43), Some(&ArchiveParameterClass::Angle));
    assert!(!classes.contains_key(&44));
}

#[test]
fn archive_parameter_classes_serialize_as_snake_case() {
    for (class, text) in [
        (ArchiveParameterClass::Length, "\"length\""),
        (ArchiveParameterClass::Angle, "\"angle\""),
        (ArchiveParameterClass::Scaler, "\"scaler\""),
        (ArchiveParameterClass::Integer, "\"integer\""),
    ] {
        assert_eq!(serde_json::to_string(&class).unwrap(), text);
    }
}
