use super::super::dimensioned_relation_carrier;
use crate::records::{
    FeatureInputClass, FeatureInputClassRole, FeatureInputLane, FeatureInputOperand,
    FeatureInputOperandKind, FeatureInputReference, SketchInputEntity, SketchInputKind,
};
use std::collections::HashMap;

#[test]
fn native_point_identity_rejects_a_declared_radial_marker() {
    let marker = |id: &str, offset, object_index, local_id, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: u32::try_from(offset).unwrap(),
        offset,
        object_index,
        local_id,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m: Some(coordinates_m),
        links: Vec::new(),
        link_selector: None,
    };
    let kind = FeatureInputOperandKind::Native(0x825c);
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: vec![FeatureInputClass {
            id: "class".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 112,
            name: "sgEntHandle".into(),
            role: FeatureInputClassRole::SketchEntity,
        }],
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: vec![FeatureInputReference {
            id: "reference".into(),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal: 0,
            offset: 100,
            kind,
            class_ref: Some("class".into()),
            object_index: 0,
        }],
        sketch_entities: vec![
            marker("center", 10, Some(2), Some(1), [0.010, 0.020]),
            marker("radial", 20, Some(1), Some(0), [0.014, 0.020]),
        ],
    };
    let markers = lane
        .sketch_entities
        .iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let center = FeatureInputOperand {
        offset: 100,
        reference_ref: "reference".into(),
        kind,
        entity_index: 1,
        entity_ref: Some("center".into()),
    };
    let carrier = dimensioned_relation_carrier(
        std::slice::from_ref(&lane),
        &markers,
        "feature",
        &center,
        5.0,
    )
    .expect("center identity survives a mismatched pair radius");
    assert_eq!(carrier.marker.id, "center");

    let radial = FeatureInputOperand {
        entity_index: 0,
        entity_ref: Some("radial".into()),
        ..center
    };
    assert!(dimensioned_relation_carrier(
        std::slice::from_ref(&lane),
        &markers,
        "feature",
        &radial,
        5.0,
    )
    .is_none());
}
