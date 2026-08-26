use super::super::{dimensioned_relation_carrier, DimensionedCurveNative};
use crate::records::{
    FeatureInputClass, FeatureInputClassRole, FeatureInputLane, FeatureInputOperand,
    FeatureInputOperandKind, FeatureInputReference, SketchInputEntity, SketchInputKind,
    SketchInputLink,
};
use std::collections::HashMap;

#[test]
fn duplicate_link_declared_entity_handle_selects_valid_arc_carrier() {
    let kind = FeatureInputOperandKind::Native(0x8c44);
    let operand = FeatureInputOperand {
        offset: 100,
        reference_ref: "reference".into(),
        kind,
        entity_index: 0,
        entity_ref: None,
    };
    let marker =
        |id: &str, offset, marker_kind, object_index, local_id, coordinates_m| SketchInputEntity {
            id: id.into(),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal: u32::try_from(offset).unwrap(),
            offset,
            object_index,
            local_id,
            kind: marker_kind,
            state_value: None,
            coordinates_m,
            links: Vec::new(),
            link_selector: None,
        };
    let center = marker(
        "unrelated-center",
        10,
        SketchInputKind::Point,
        Some(50),
        Some(49),
        Some([0.010, 0.020]),
    );
    let radial = marker(
        "unrelated-radial",
        20,
        SketchInputKind::Point,
        Some(49),
        Some(0),
        Some([0.014, 0.024]),
    );
    let arc = marker(
        "arc",
        30,
        SketchInputKind::Arc,
        Some(8),
        Some(7),
        Some([0.010, 0.020]),
    );
    let witness = marker(
        "witness",
        40,
        SketchInputKind::Point,
        Some(7),
        Some(9),
        Some([0.013, 0.024]),
    );
    let mut handle = marker(
        "handle",
        50,
        SketchInputKind::LineOrCircle,
        Some(2),
        Some(u32::MAX - 65536),
        None,
    );
    handle.links = vec![
        SketchInputLink {
            entity_ref: "arc".into(),
            local_id: 7,
        },
        SketchInputLink {
            entity_ref: "arc".into(),
            local_id: 7,
        },
    ];
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
            id: operand.reference_ref.clone(),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal: 0,
            offset: operand.offset,
            kind,
            class_ref: Some("class".into()),
            object_index: 0,
        }],
        sketch_entities: vec![center, radial, arc, witness, handle],
    };
    let markers_by_id = lane
        .sketch_entities
        .iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();

    let carrier = dimensioned_relation_carrier(
        std::slice::from_ref(&lane),
        &markers_by_id,
        "feature",
        &operand,
        5.0,
    )
    .expect("duplicate-link arc carrier");
    assert_eq!(carrier.marker.id, "arc");
    assert_eq!(carrier.center, [0.010, 0.020]);
    assert!(matches!(
        carrier.curve,
        Some(DimensionedCurveNative::Circle {
            center: [0.010, 0.020]
        })
    ));
    assert_eq!(carrier.construction, Some(false));

    let mut mismatched_lane = lane.clone();
    mismatched_lane.sketch_entities[4].links[1].local_id = 8;
    let mismatched_markers = mismatched_lane
        .sketch_entities
        .iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    assert!(dimensioned_relation_carrier(
        std::slice::from_ref(&mismatched_lane),
        &mismatched_markers,
        "feature",
        &operand,
        5.0,
    )
    .is_none());

    let mut non_arc_lane = lane.clone();
    non_arc_lane.sketch_entities[2].kind = SketchInputKind::LineOrCircle;
    let non_arc_markers = non_arc_lane
        .sketch_entities
        .iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    assert!(dimensioned_relation_carrier(
        std::slice::from_ref(&non_arc_lane),
        &non_arc_markers,
        "feature",
        &operand,
        5.0,
    )
    .is_none());

    let mut ambiguous_lane = lane.clone();
    let second_arc = marker(
        "second-arc",
        60,
        SketchInputKind::Arc,
        Some(10),
        Some(11),
        Some([0.020, 0.030]),
    );
    let second_witness = marker(
        "second-witness",
        70,
        SketchInputKind::Point,
        Some(11),
        Some(12),
        Some([0.023, 0.034]),
    );
    let mut second_handle = marker(
        "second-handle",
        80,
        SketchInputKind::LineOrCircle,
        Some(3),
        Some(u32::MAX - 65536),
        None,
    );
    second_handle.links = vec![
        SketchInputLink {
            entity_ref: "second-arc".into(),
            local_id: 11,
        },
        SketchInputLink {
            entity_ref: "second-arc".into(),
            local_id: 11,
        },
    ];
    ambiguous_lane
        .sketch_entities
        .extend([second_arc, second_witness, second_handle]);
    let ambiguous_markers = ambiguous_lane
        .sketch_entities
        .iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    assert!(dimensioned_relation_carrier(
        std::slice::from_ref(&ambiguous_lane),
        &ambiguous_markers,
        "feature",
        &operand,
        5.0,
    )
    .is_none());
}
