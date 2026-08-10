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
fn native_feature_scalar_replaces_placeholder_length_expression() {
    let feature = crate::records::Feature {
        id: "feature".into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some("1738".into()),
        parent_source_id: None,
        ordinal: 0,
        name: "Fillet103".into(),
        kind: "Fillet".into(),
        input_class: Some("Fillet_c".into()),
        suppressed: false,
        parameters: BTreeMap::from([("D1".into(), "R0".into())]),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: vec![FeatureContent::Dimension("D1".into())],
    };
    let mut histories = vec![FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::default(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![feature],
    }];
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: vec![
            FeatureInputName {
                id: "feature-name".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 0,
                object_id: Some(1738),
                value: "Fillet103".into(),
            },
            FeatureInputName {
                id: "d1-name".into(),
                parent: "lane".into(),
                ordinal: 1,
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
            value: 0.002,
            role: FeatureInputScalarRole::Native,
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
    assert_eq!(histories[0].features[0].parameters["D1"], "2mm");
}
