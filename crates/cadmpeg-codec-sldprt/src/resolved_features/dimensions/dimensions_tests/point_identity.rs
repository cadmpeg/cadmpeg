use super::super::dimensioned_relation_carrier;
use crate::records::{
    FeatureInputClass, FeatureInputClassRole, FeatureInputLane, FeatureInputOperand,
    FeatureInputOperandKind, FeatureInputReference, SketchInputEntity, SketchInputKind,
};
use crate::resolved_features::relation_geometry::direct_point_dimension_center;
use std::collections::HashMap;

#[test]
fn classless_point_identity_requires_exact_reference_and_center_role() {
    let kind = FeatureInputOperandKind::Native(0x80fe);
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
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
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
            class_ref: None,
            object_index: 0,
        }],
        sketch_entities: vec![marker("center", 10, Some(10), Some(0), [0.010, 0.020])],
    };
    let operand = FeatureInputOperand {
        offset: 100,
        reference_ref: "reference".into(),
        kind,
        entity_index: 0,
        entity_ref: Some("center".into()),
    };

    assert_eq!(
        direct_point_dimension_center(std::slice::from_ref(&lane), "feature", &operand, 5.0)
            .map(|marker| marker.id.as_str()),
        Some("center")
    );
    let markers = lane
        .sketch_entities
        .iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let carrier = dimensioned_relation_carrier(
        std::slice::from_ref(&lane),
        &markers,
        "feature",
        &operand,
        5.0,
    )
    .expect("classless direct point carrier");
    assert_eq!(carrier.marker.id, "center");
    assert_eq!(carrier.construction, Some(false));

    let mismatched = FeatureInputOperand {
        entity_index: 1,
        ..operand.clone()
    };
    assert!(direct_point_dimension_center(
        std::slice::from_ref(&lane),
        "feature",
        &mismatched,
        5.0
    )
    .is_none());

    let mut nonmatching_pair_lane = lane.clone();
    nonmatching_pair_lane.sketch_entities = vec![
        marker("center", 10, Some(20), Some(19), [0.010, 0.020]),
        marker("radial", 20, Some(19), Some(0), [0.020, 0.020]),
    ];
    let nonmatching_pair = FeatureInputOperand {
        entity_ref: Some("radial".into()),
        ..operand.clone()
    };
    assert_eq!(
        direct_point_dimension_center(
            std::slice::from_ref(&nonmatching_pair_lane),
            "feature",
            &nonmatching_pair,
            5.0,
        )
        .map(|marker| marker.id.as_str()),
        Some("radial")
    );

    let mut radial_lane = lane;
    radial_lane.sketch_entities = vec![
        marker("center", 10, Some(20), Some(19), [0.010, 0.020]),
        marker("radial", 20, Some(19), Some(0), [0.015, 0.020]),
    ];
    let radial = FeatureInputOperand {
        entity_ref: Some("radial".into()),
        ..operand
    };
    assert!(direct_point_dimension_center(
        std::slice::from_ref(&radial_lane),
        "feature",
        &radial,
        5.0
    )
    .is_none());
}

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

#[test]
fn native_radial_identity_selects_one_of_equal_radius_pairs() {
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
        classes: vec![
            FeatureInputClass {
                id: "class".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 112,
                name: "sgEntHandle".into(),
                role: FeatureInputClassRole::SketchEntity,
            },
            FeatureInputClass {
                id: "line-class".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: 250,
                name: "sgLineHandle".into(),
                role: FeatureInputClassRole::SketchEntity,
            },
        ],
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
            marker("center-one", 10, Some(2), Some(1), [0.010, 0.020]),
            marker("radial-one", 20, Some(1), Some(0), [0.015, 0.020]),
            marker("center-two", 30, Some(4), Some(3), [0.030, 0.040]),
            marker("radial-two", 40, Some(3), Some(0), [0.035, 0.040]),
            SketchInputEntity {
                id: "line-carrier".into(),
                parent: "lane".into(),
                feature_ref: Some("feature".into()),
                ordinal: 50,
                offset: 50,
                object_index: Some(6),
                local_id: Some(6),
                kind: SketchInputKind::LineOrCircle,
                state_value: Some(1.0),
                coordinates_m: Some([0.020, 0.020]),
                links: Vec::new(),
                link_selector: None,
            },
            marker("line-radial", 60, Some(6), Some(0), [0.025, 0.020]),
        ],
    };
    let markers = lane
        .sketch_entities
        .iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let operand = FeatureInputOperand {
        offset: 100,
        reference_ref: "reference".into(),
        kind,
        entity_index: 0,
        entity_ref: Some("radial-one".into()),
    };

    let carrier = dimensioned_relation_carrier(
        std::slice::from_ref(&lane),
        &markers,
        "feature",
        &operand,
        5.0,
    )
    .expect("the radial identity selects its declared center");
    assert_eq!(carrier.marker.id, "center-one");
    assert_eq!(carrier.center, [0.010, 0.020]);
}
