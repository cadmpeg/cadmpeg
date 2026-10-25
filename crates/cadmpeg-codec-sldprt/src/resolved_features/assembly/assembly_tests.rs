//! Tests for the `assembly` module.

use super::super::CLASS_MARKER;
use super::{configuration_name, legacy_feature_input_section, scalar_archive_classes};
use crate::carchive::reader::{ParameterRecord, V};
use crate::carchive::{Status, Walk};
use crate::loss::SldprtLossCode;
use crate::records::ArchiveParameterClass;
use crate::test_support::carchive::{
    sldprt_with_archive_parameter, sldprt_with_stopped_archive_walk,
};
use crate::test_support::container::scan;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::report::loss::LossNote;
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
        trace: Vec::new(),
        parameters: vec![
            parameter("moLengthParameter_c", 42),
            parameter("moAngleParameter_c", 43),
            parameter("moNodeName_c", 44),
        ],
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

fn lanes_of(bytes: &[u8]) -> (Vec<crate::records::FeatureInputLane>, Vec<LossNote>) {
    let scan = scan(bytes);
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut annotations = Annotations::default();
    let mut losses = Vec::new();
    let lanes =
        super::lanes(&ctx, &scan, &mut annotations, &mut losses).expect("lane assembly succeeds");
    (lanes, losses)
}

#[test]
fn a_reconciled_archive_walk_types_the_lane_scalar() {
    let (lanes, losses) = lanes_of(&sldprt_with_archive_parameter());
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].scalars.len(), 1);
    assert_eq!(
        lanes[0].scalars[0].archive_class,
        Some(ArchiveParameterClass::Length)
    );
    assert_eq!(losses.len(), 0);
}

#[test]
fn a_stopped_archive_walk_charges_a_loss_and_types_no_scalar() {
    let (lanes, losses) = lanes_of(&sldprt_with_stopped_archive_walk());
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].scalars.len(), 1);
    assert_eq!(lanes[0].scalars[0].archive_class, None);
    assert!(losses
        .iter()
        .any(|note| note.code == SldprtLossCode::FeatureInputArchiveWalkStopped.kind()));
}
