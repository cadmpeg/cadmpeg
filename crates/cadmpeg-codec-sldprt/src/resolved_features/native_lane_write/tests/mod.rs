// SPDX-License-Identifier: Apache-2.0
//! Typed native content survives synthesis without a lane image.

use super::regenerate;
use crate::native::SldprtNative;
use cadmpeg_ir::codec::write::{NativeDisposition, NativeDispositions};

#[test]
fn unheld_native_scalar_is_authored_from_typed_fields_after_wire_parse() {
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
        parameters: std::collections::BTreeMap::default(),
        dimension_properties: std::collections::BTreeMap::default(),
        properties: std::collections::BTreeMap::default(),
        text: None,
        content: Vec::new(),
    };
    feature.name = "ControlOwner".into();
    feature.input_class = Some("moBaseBody_c".into());
    let history = crate::records::FeatureHistory {
        id: "sldprt:history#controls".into(),
        part_name: None,
        properties: std::collections::BTreeMap::default(),
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
        None,
        bytes,
    )
    .expect("authored lane admits");
    let source = SldprtNative {
        feature_histories: vec![history],
        feature_input_lanes: vec![lane],
        ..Default::default()
    };
    // CADIR retains the lane bytes with the typed content. Clear the bytes
    // here so the test exercises sourceless regeneration from typed fields.
    let wire = serde_json::to_value(&source).expect("typed native serializes");
    let mut source: SldprtNative = serde_json::from_value(wire).expect("typed native parses");
    assert!(!source.feature_input_lanes[0].native_payload.is_empty());
    source.feature_input_lanes[0].native_payload.clear();
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

#[test]
fn typed_sketch_record_wins_over_stale_lane_bytes() {
    let mut payload = Vec::new();
    crate::resolved_features::write_generate::append_coordinate_marker(
        &mut payload,
        crate::records::SketchInputKind::Point,
        [0.001, 0.002],
        1,
    );
    let lane = crate::resolved_features::write_prepare::generated_native_lane(
        &[],
        "sldprt:feature-input#sketch",
        None,
        payload,
    )
    .unwrap();
    let mut source = SldprtNative {
        feature_input_lanes: vec![lane],
        ..Default::default()
    };
    source.feature_input_lanes[0].sketch_entities[0] =
        source.feature_input_lanes[0].sketch_entities[0].with_test_identity(Some(0), Some(1));
    source.feature_input_lanes[0].sketch_entities[0]
        .reclassify(crate::records::SketchInputKind::LineOrCircle);
    source.feature_input_lanes[0].sketch_entities[0].coordinates_m =
        cadmpeg_ir::units::FiniteVector::new([0.001, 0.002]);
    let mut target = source.clone();
    target.feature_input_lanes.clear();
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
    .unwrap();
    assert!(
        target
            .feature_input_lanes
            .iter()
            .flat_map(|lane| &lane.sketch_entities)
            .any(|entity| entity.kind() == crate::records::SketchInputKind::LineOrCircle),
        "lanes {:#?}, losses {losses:#?}",
        target.feature_input_lanes
    );
}

#[test]
fn ownerless_native_scalar_survives_when_design_is_held() {
    let mut payload = Vec::new();
    crate::resolved_features::write_generate::append_generated_scalar(
        &mut payload,
        "moLengthParameter_c",
        "Independent",
        0.034,
        23,
        &[],
    )
    .unwrap();
    let lane = crate::resolved_features::write_prepare::generated_native_lane(
        &[],
        "sldprt:feature-input#independent",
        None,
        payload,
    )
    .unwrap();
    assert!(lane.scalars[0].feature_ref.is_none());
    let source = SldprtNative {
        feature_input_lanes: vec![lane],
        ..Default::default()
    };
    let mut target = SldprtNative::default();
    let ir = cadmpeg_ir::CadIr::empty();
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
    .unwrap();
    assert_eq!(target.feature_input_lanes.len(), 1);
    assert_eq!(target.feature_input_lanes[0].scalars.len(), 1);
    assert_eq!(target.feature_input_lanes[0].scalars[0].value.get(), 0.034);
    assert!(losses.is_empty(), "{losses:?}");

    let mut edited = ir.clone();
    edited
        .model
        .parameters
        .push(cadmpeg_ir::features::DesignParameter {
            product_definition: None,
            id: cadmpeg_ir::features::ParameterId::mint("synthetic:test:parameter#independent")
                .unwrap(),
            owner: None,
            ordinal: 0,
            name: "Independent".into(),
            expression: "70mm".into(),
            display: None,
            value: Some(cadmpeg_ir::features::ParameterValue::Length(
                cadmpeg_ir::scalar::Length::new(70.0).unwrap(),
            )),
            driving: true,
            properties: Default::default(),
            pmi: None,
            native_ref: Some(source.feature_input_lanes[0].scalars[0].id.clone()),
        });
    let mut target = SldprtNative::default();
    let mut losses = Vec::new();
    regenerate(
        &edited,
        &edited,
        &source,
        &mut target,
        &mut losses,
        &mut dispositions,
    )
    .unwrap();
    assert!(target
        .feature_input_lanes
        .iter()
        .flat_map(|lane| &lane.scalars)
        .all(|scalar| scalar.value.get() != 0.034));
}
