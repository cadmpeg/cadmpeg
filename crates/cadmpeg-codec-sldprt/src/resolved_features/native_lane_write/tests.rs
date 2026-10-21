// SPDX-License-Identifier: Apache-2.0
//! Typed native content survives synthesis without a lane image.

use super::regenerate;
use crate::native::SldprtNative;
use cadmpeg_ir::codec::write::{NativeDisposition, NativeDispositions};

#[test]
fn unheld_native_scalar_is_authored_from_typed_fields_after_wire_parse() {
    let source = native_control_source(None);
    // CADIR retains evidence and typed content, without native lane bytes.
    let wire = serde_json::to_value(&source).expect("typed native serializes");
    fn no_physical_positions(value: &serde_json::Value) {
        match value {
            serde_json::Value::Object(fields) => {
                for (name, value) in fields {
                    assert!(
                        name != "offset" && !name.ends_with("_offset"),
                        "physical source position {name} leaked into CADIR"
                    );
                    no_physical_positions(value);
                }
            }
            serde_json::Value::Array(values) => values.iter().for_each(no_physical_positions),
            _ => {}
        }
    }
    no_physical_positions(&wire);
    let mut source: SldprtNative = serde_json::from_value(wire).expect("typed native parses");
    assert!(source.feature_input_lanes[0]
        .classes
        .iter()
        .all(|record| record.offset.is_none()));
    assert!(source.feature_input_lanes[0]
        .names
        .iter()
        .all(|record| record.offset.is_none()));
    assert!(source.feature_input_lanes[0]
        .scalars
        .iter()
        .all(|record| record.offset.is_none()));
    assert!(source.feature_input_lanes[0].native_payload.is_empty());
    source.feature_input_lanes[0].scalars[0].value =
        cadmpeg_ir::scalar::FiniteReal::new(0.034).expect("finite control");
    let mut target = source.clone();
    target.feature_input_lanes.clear();
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.set_scope(cadmpeg_ir::ContentScope::container())
        .expect("empty neutral scope");
    let mut losses = Vec::new();
    let mut dispositions = NativeDispositions::for_namespace(
        "sldprt",
        crate::native::SLDPRT_ARENA_NAMES,
        NativeDisposition::Dropped,
    );
    regenerate(
        &ir,
        &ir,
        &source,
        &mut target,
        &mut losses,
        &mut dispositions,
    )
    .expect("typed records produce a native lane");
    assert!(losses.is_empty(), "{losses:?}");
    assert_eq!(target.feature_input_lanes.len(), 1);
    let scalars = &target.feature_input_lanes[0].scalars;
    assert_eq!(scalars.len(), 1);
    assert_eq!(scalars[0].value.get(), 0.034);
    assert!(!target.feature_input_lanes[0].native_payload.is_empty());
}

fn native_control_source(configuration: Option<String>) -> SldprtNative {
    let mut feature = crate::records::Feature {
        id: "sldprt:history:feature#native-control".into(),
        parent: "sldprt:history#controls".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: crate::records::FeatureSource::from_value(1),
        ordinal: 0,
        name: String::new(),
        kind: "Feature".into(),
        input_class: None,
        suppressed: false,
        parameters: Default::default(),
        dimension_properties: Default::default(),
        properties: Default::default(),
        text: None,
        content: Vec::new(),
    };
    feature.name = "ControlOwner".into();
    feature.input_class = Some("moBaseBody_c".into());
    let history = crate::records::FeatureHistory {
        id: "sldprt:history#controls".into(),
        part_name: None,
        properties: Default::default(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![feature],
    };
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&[0xff, 0xff, 1, 0]);
    bytes.extend_from_slice(&12u16.to_le_bytes());
    bytes.extend_from_slice(b"moBaseBody_c");
    crate::resolved_features::write_generate::append_generated_object_name(
        &mut bytes,
        "ControlOwner",
        1,
    )
    .expect("native object name");
    crate::resolved_features::write_generate::append_generated_scalar(
        &mut bytes,
        "moLengthParameter_c",
        "NativeOnly",
        0.012,
        23,
        &[],
    )
    .expect("native scalar");
    let lane = crate::resolved_features::write_prepare::generated_native_lane(
        std::slice::from_ref(&history),
        "sldprt:feature-input#controls",
        configuration,
        bytes,
    )
    .expect("authored lane admits");
    SldprtNative {
        feature_histories: vec![history],
        feature_input_lanes: vec![lane],
        ..Default::default()
    }
}

#[test]
fn native_configuration_statements_join_the_authored_scalar_lane() {
    let mut source = native_control_source(Some("1".into()));
    source.feature_input_lanes[0].scalars[0].value =
        cadmpeg_ir::scalar::FiniteReal::new(0.034).expect("native-only current value");
    let mut target = source.clone();
    target.feature_input_lanes.clear();
    let mut payload = Vec::new();
    super::super::write_generate::append_generated_object_name(&mut payload, "ControlOwner", 1)
        .expect("canonical configuration owner");
    for (name, value, object) in [("Width", 0.050, 41), ("Depth", 0.100, 42)] {
        super::super::write_generate::append_generated_scalar(
            &mut payload,
            "moLengthParameter_c",
            name,
            value,
            object,
            &[],
        )
        .expect("current inactive configuration cache");
    }
    target.feature_input_lanes.push(
        super::super::write_prepare::generated_configuration_lane(
            &target.feature_histories,
            "1",
            payload,
        )
        .expect("authored canonical configuration lane"),
    );
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.set_scope(cadmpeg_ir::ContentScope::container()).unwrap();
    let mut losses = Vec::new();
    let mut dispositions = NativeDispositions::for_namespace(
        "sldprt",
        crate::native::SLDPRT_ARENA_NAMES,
        NativeDisposition::Dropped,
    );
    regenerate(
        &ir,
        &ir,
        &source,
        &mut target,
        &mut losses,
        &mut dispositions,
    )
    .expect("typed native statements and canonical caches share a state lane");
    assert!(losses.is_empty(), "{losses:?}");
    assert_eq!(target.feature_input_lanes.len(), 1);
    let lane = &target.feature_input_lanes[0];
    assert_eq!(lane.configuration.as_deref(), Some("1"));
    assert_eq!(lane.scalars.len(), 3);
    for (name, value) in [("Width", 0.050), ("Depth", 0.100), ("NativeOnly", 0.034)] {
        let named = lane
            .names
            .iter()
            .find(|record| record.value == name)
            .unwrap();
        let scalar = lane
            .scalars
            .iter()
            .find(|record| record.name == named.id)
            .unwrap();
        assert_eq!(scalar.value.get(), value);
        assert_eq!(
            scalar.feature_ref.as_deref(),
            Some(source.feature_histories[0].features[0].id.as_str())
        );
    }
    assert!(lane
        .classes
        .iter()
        .any(|class| class.name == "moBaseBody_c"));
}
