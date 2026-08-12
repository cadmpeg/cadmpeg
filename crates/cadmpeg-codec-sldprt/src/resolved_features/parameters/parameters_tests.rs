//! Tests for the `parameters` module.

use super::*;
use crate::records::{
    FeatureContent, FeatureHistory, FeatureInputLane, FeatureInputName, FeatureInputScalar,
    FeatureInputScalarRole,
};
use std::collections::BTreeMap;

#[test]
fn native_scalar_must_match_an_existing_discrete_parameter() {
    let feature = crate::records::Feature {
        id: "feature".into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: None,
        parent_source_id: None,
        ordinal: 0,
        name: "Pattern".into(),
        kind: "Pattern".into(),
        input_class: Some("moLPattern_c".into()),
        suppressed: false,
        parameters: BTreeMap::default(),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: Vec::new(),
    };
    assert!(native_scalar_matches_discrete_parameter(
        &feature, "D1", "15", 15.0
    ));
    assert!(!native_scalar_matches_discrete_parameter(
        &feature,
        "D1",
        "15",
        8.371_160_993_642_741e298
    ));
}

#[test]
fn fillet_display_placeholder_establishes_length_unit() {
    let feature = crate::records::Feature {
        id: "feature".into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: None,
        parent_source_id: None,
        ordinal: 0,
        name: "Fillet".into(),
        kind: "Fillet".into(),
        input_class: Some("Fillet_c".into()),
        suppressed: false,
        parameters: BTreeMap::from([("D1".into(), "R0".into())]),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: Vec::new(),
    };
    assert_eq!(
        scalar_unit_from_feature_parameter(&feature, "D1"),
        Some(super::ScalarUnit::Length)
    );
    assert_eq!(
        scalar_unit_from_feature_parameter(&feature, "missing"),
        None
    );
    let mut numeric = feature;
    numeric.parameters.insert("D1".into(), "0".into());
    assert_eq!(scalar_unit_from_feature_parameter(&numeric, "D1"), None);

    let mut cosmetic_thread = numeric.clone();
    cosmetic_thread.kind = "CosmeticThread".into();
    cosmetic_thread.input_class = Some("CosmeticThread_c".into());
    cosmetic_thread.parameters.clear();
    cosmetic_thread
        .parameters
        .insert("D2".into(), "<MOD-DIAM>6".into());
    assert_eq!(
        scalar_unit_from_feature_parameter(&cosmetic_thread, "D2"),
        None
    );

    let mut variable = numeric;
    variable.kind = "VarFillet".into();
    variable.input_class = Some("VarFillet_c".into());
    variable.parameters.insert("D1".into(), "R0".into());
    assert_eq!(scalar_unit_from_feature_parameter(&variable, "D1"), None);
}

#[test]
fn thin_cut_native_dimensions_are_lengths() {
    let feature = crate::records::Feature {
        id: "feature".into(),
        parent: "history".into(),
        xml_tag: "Extrusion".into(),
        tree_parent: None,
        source_id: None,
        parent_source_id: None,
        ordinal: 0,
        name: "Thin cut".into(),
        kind: "Cut-Extrude-Thin".into(),
        input_class: None,
        suppressed: false,
        parameters: BTreeMap::from([
            ("D5".into(), "0.3".into()),
            ("D6".into(), "0.1".into()),
            ("D7".into(), "0.2".into()),
        ]),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: Vec::new(),
    };

    for name in ["D5", "D6", "D7"] {
        assert_eq!(
            scalar_unit_from_feature_parameter(&feature, name),
            Some(ScalarUnit::Length)
        );
    }
    assert_eq!(scalar_unit_from_feature_parameter(&feature, "D8"), None);
}

#[test]
fn sketch_source_dimension_establishes_scalar_unit() {
    let feature = crate::records::Feature {
        id: "feature".into(),
        parent: "history".into(),
        xml_tag: "Sketch".into(),
        tree_parent: None,
        source_id: None,
        parent_source_id: None,
        ordinal: 0,
        name: "Sketch".into(),
        kind: "Sketch".into(),
        input_class: Some("moProfileFeature_c".into()),
        suppressed: false,
        parameters: BTreeMap::from([
            ("depth".into(), "0.75".into()),
            ("angle".into(), "90°".into()),
            ("unowned".into(), "1".into()),
        ]),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: vec![
            FeatureContent::Dimension("depth".into()),
            FeatureContent::Dimension("angle".into()),
        ],
    };

    assert_eq!(
        scalar_unit_from_feature_parameter(&feature, "depth"),
        Some(ScalarUnit::Length)
    );
    assert_eq!(
        scalar_unit_from_feature_parameter(&feature, "angle"),
        Some(ScalarUnit::Angle)
    );
    assert_eq!(
        scalar_unit_from_feature_parameter(&feature, "unowned"),
        None
    );
}

#[test]
fn explicit_sketch_dimension_scalar_preserves_display_outside_object_range() {
    let feature = crate::records::Feature {
        id: "feature".into(),
        parent: "history".into(),
        xml_tag: "Sketch".into(),
        tree_parent: None,
        source_id: Some("1738".into()),
        parent_source_id: None,
        ordinal: 0,
        name: "Sketch".into(),
        kind: "Sketch".into(),
        input_class: Some("moProfileFeature_c".into()),
        suppressed: false,
        parameters: BTreeMap::from([("D1".into(), "<MOD-DIAM>0.281".into())]),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: vec![FeatureContent::Dimension("D1".into())],
    };
    let mut later_feature = feature.clone();
    later_feature.id = "later-feature".into();
    later_feature.source_id = Some("2000".into());
    later_feature.name = "Later".into();
    later_feature.parameters.clear();
    let mut histories = vec![FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::default(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![feature, later_feature],
    }];
    let mut lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: vec![0; 136],
        classes: Vec::new(),
        names: vec![
            FeatureInputName {
                id: "feature-name".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 0,
                object_id: Some(1738),
                value: "Sketch".into(),
            },
            FeatureInputName {
                id: "later-feature-name".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: 64,
                object_id: Some(2000),
                value: "Later".into(),
            },
            FeatureInputName {
                id: "d1-name".into(),
                parent: "lane".into(),
                ordinal: 2,
                offset: 100,
                object_id: None,
                value: "D1".into(),
            },
        ],
        scalars: vec![FeatureInputScalar {
            id: "scalar".into(),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal: 0,
            offset: 128,
            object_id: 1739,
            name: "d1-name".into(),
            value: 0.007_137_4,
            role: FeatureInputScalarRole::Driving,
            entity_indices: Vec::new(),
            operands: Vec::new(),
        }],
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    enrich_history_parameters(&mut histories, [&lane], true);
    assert_eq!(
        histories[0].features[0].parameters["D1"],
        "<MOD-DIAM>7.1374"
    );
    histories[0].features[0]
        .parameters
        .insert("D1".into(), "<MOD-DIAM>8".into());
    sync_changed_feature_scalars(
        &histories,
        std::slice::from_mut(&mut lane),
        &HashSet::from([("feature".into(), "D1".into())]),
    )
    .expect("explicit scalar owner is writable");
    assert_eq!(lane.scalars[0].value, 0.008);
    assert_eq!(&lane.native_payload[128..136], &0.008f64.to_le_bytes());
}
