//! Tests for the `dimensions` module.

use super::super::{LEGACY_EXTENDED_SKETCH_MARKER, LEGACY_SKETCH_MARKER};
use super::{
    compact_radial_circle_index, extended_radial_circle_index,
    extended_terminal_repeated_radial_circle_index, project_relation_point_dimensioned_circles,
    radial_dimension_radius, terminal_repeated_radial_circle_pairs,
};
use crate::records::{
    FeatureInputLane, FeatureInputOperand, FeatureInputOperandKind, FeatureInputRelationFamily,
    FeatureInputRelationInstance, SketchInputEntity, SketchInputKind, SketchInputLink,
    SketchRelationKind,
};
use cadmpeg_ir::features::{
    DesignParameter, DimensionDisplay, Feature, FeatureDefinition, FeatureId, Length, ParameterId,
    ParameterValue, SketchSpace,
};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{SketchEntity, SketchEntityId, SketchGeometry, SketchId};
use std::collections::BTreeMap;

#[test]
fn duplicated_compact_curve_address_identifies_a_radial_circle_witness() {
    let marker = |construction: bool| {
        let mut payload = vec![0; 104 + LEGACY_SKETCH_MARKER.len()];
        payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
        payload[5..13].fill(0xff);
        payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[17..21].copy_from_slice(&(if construction { 7u32 } else { 2 }).to_le_bytes());
        payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
        payload[27..29].copy_from_slice(&(if construction { 2u16 } else { 1 }).to_le_bytes());
        payload[31..39].copy_from_slice(&[
            0x00,
            0x00,
            0x80,
            0xbf,
            0x00,
            0x00,
            if construction { 0x0c } else { 0x04 },
            0x00,
        ]);
        payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[56..58].copy_from_slice(&9u16.to_le_bytes());
        payload[58..60].copy_from_slice(&9u16.to_le_bytes());
        payload[60..64].copy_from_slice(&u32::from(!construction).to_le_bytes());
        payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
        payload[72..76].copy_from_slice(&1i32.to_le_bytes());
        payload[76..78].copy_from_slice(&(if construction { 8u16 } else { 4 }).to_le_bytes());
        for at in (78..94).step_by(4) {
            payload[at..at + 4].copy_from_slice(&(-2i32).to_le_bytes());
        }
        payload[104..].copy_from_slice(LEGACY_SKETCH_MARKER);
        if construction {
            payload[9..13].copy_from_slice(&[0x04, 0x00, 0xff, 0xff]);
        } else {
            payload[29..31].copy_from_slice(&1u16.to_le_bytes());
        }
        payload
    };

    for construction in [false, true] {
        let mut payload = marker(construction);
        assert_eq!(compact_radial_circle_index(&payload, 0), Some(9));
        payload[58..60].copy_from_slice(&10u16.to_le_bytes());
        assert_eq!(compact_radial_circle_index(&payload, 0), None);
    }
    let mut extended_envelope = marker(false);
    extended_envelope[..LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    assert_eq!(compact_radial_circle_index(&extended_envelope, 0), Some(9));
    let mut terminal = marker(false);
    terminal.truncate(102);
    assert_eq!(compact_radial_circle_index(&terminal, 0), Some(9));
}

#[test]
fn radial_dimensions_normalize_radius_and_diameter_displays() {
    let parameter = |display, value| DesignParameter {
        id: ParameterId("radial".into()),
        owner: Some(FeatureId("sketch".into())),
        ordinal: 0,
        name: "radial".into(),
        expression: String::new(),
        display,
        value: Some(ParameterValue::Length(Length(value))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };

    assert_eq!(
        radial_dimension_radius(&parameter(Some(DimensionDisplay::Radius), 2.0)),
        Some(2.0)
    );
    assert_eq!(
        radial_dimension_radius(&parameter(Some(DimensionDisplay::Diameter), 4.0)),
        Some(2.0)
    );
    assert_eq!(radial_dimension_radius(&parameter(None, 2.0)), None);
    assert_eq!(
        radial_dimension_radius(&parameter(Some(DimensionDisplay::Radius), -2.0)),
        None
    );
}

#[test]
fn point_dimension_projects_only_from_one_same_sketch_center_witness() {
    let feature_id = FeatureId("feature".into());
    let feature_ref = "feature";
    let sketch_id = SketchId("sketch".into());
    let marker_id = "marker";
    let relation = FeatureInputRelationInstance {
        id: "relation".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 42,
        family: FeatureInputRelationFamily::CircleDiameter,
        class_ref: "class".into(),
        feature_ref: feature_ref.into(),
        scalar_refs: vec!["scalar".into()],
        parameter_scalar_ref: Some("scalar".into()),
        display_scalar_ref: None,
        operands: vec![FeatureInputOperand {
            offset: 0,
            reference_ref: "reference".into(),
            kind: FeatureInputOperandKind::Native(0x829a),
            entity_index: 0,
            entity_ref: Some(marker_id.into()),
        }],
    };
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: vec![relation],
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: vec![SketchInputEntity {
            id: marker_id.into(),
            parent: "lane".into(),
            feature_ref: Some(feature_ref.into()),
            ordinal: 0,
            offset: 10,
            object_index: Some(0),
            local_id: Some(0),
            kind: SketchInputKind::Point,
            state_value: Some(1.0),
            coordinates_m: Some([0.001, 0.002]),
            links: Vec::new(),
            link_selector: None,
        }],
    };
    let feature = Feature {
        id: feature_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: SketchSpace::Planar,
            sketch: Some(sketch_id.clone()),
        },
        native_ref: Some(feature_ref.into()),
    };
    let parameter = DesignParameter {
        id: ParameterId("parameter".into()),
        owner: Some(feature_id),
        ordinal: 0,
        name: "D1".into(),
        expression: "<MOD-DIAM>4".into(),
        display: Some(DimensionDisplay::Diameter),
        value: Some(ParameterValue::Length(Length(4.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some("scalar".into()),
    };
    let center = SketchEntity {
        id: SketchEntityId("center".into()),
        sketch: sketch_id,
        construction: true,
        native_ref: Some(marker_id.into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(1.0, 2.0),
        },
    };
    let mut entities = vec![center];

    project_relation_point_dimensioned_circles(
        &mut entities,
        std::slice::from_ref(&feature),
        std::slice::from_ref(&parameter),
        std::slice::from_ref(&lane),
    );

    assert!(matches!(
        entities.get(1).map(|entity| &entity.geometry),
        Some(SketchGeometry::Circle { center, radius: Length(2.0) })
            if *center == Point2::new(1.0, 2.0)
    ));
    assert_eq!(entities[1].geometry_ref.as_deref(), Some("relation"));

    let mut ambiguous = entities[..1].to_vec();
    ambiguous.push(SketchEntity {
        id: SketchEntityId("second-center".into()),
        sketch: entities[0].sketch.clone(),
        construction: true,
        native_ref: Some(marker_id.into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(1.0, 2.0),
        },
    });
    project_relation_point_dimensioned_circles(
        &mut ambiguous,
        std::slice::from_ref(&feature),
        std::slice::from_ref(&parameter),
        std::slice::from_ref(&lane),
    );
    assert_eq!(ambiguous.len(), 2);

    let mut missing = entities[..1].to_vec();
    let mut missing_lane = lane.clone();
    missing_lane.relation_instances[0].operands[0].entity_ref = None;
    project_relation_point_dimensioned_circles(
        &mut missing,
        std::slice::from_ref(&feature),
        std::slice::from_ref(&parameter),
        std::slice::from_ref(&missing_lane),
    );
    assert_eq!(missing.len(), 1);

    let mut implicit_lane = lane.clone();
    implicit_lane.relation_instances[0].operands[0].entity_ref = None;
    implicit_lane.sketch_entities.extend([
        SketchInputEntity {
            id: "implicit-center".into(),
            parent: "lane".into(),
            feature_ref: Some(feature_ref.into()),
            ordinal: 1,
            offset: 20,
            object_index: Some(1),
            local_id: Some(0),
            kind: SketchInputKind::Point,
            state_value: Some(1.0),
            coordinates_m: Some([0.0, 0.0]),
            links: Vec::new(),
            link_selector: None,
        },
        SketchInputEntity {
            id: "implicit-relation".into(),
            parent: "lane".into(),
            feature_ref: Some(feature_ref.into()),
            ordinal: 2,
            offset: 25,
            object_index: Some(1),
            local_id: None,
            kind: SketchInputKind::Relation(SketchRelationKind::Distance),
            state_value: Some(1.0),
            coordinates_m: None,
            links: vec![
                SketchInputLink {
                    local_id: 0,
                    entity_ref: "implicit-center".into(),
                },
                SketchInputLink {
                    local_id: 0,
                    entity_ref: "implicit-center".into(),
                },
            ],
            link_selector: None,
        },
        SketchInputEntity {
            id: "implicit-radial".into(),
            parent: "lane".into(),
            feature_ref: Some(feature_ref.into()),
            ordinal: 3,
            offset: 30,
            object_index: Some(2),
            local_id: None,
            kind: SketchInputKind::Point,
            state_value: Some(1.0),
            coordinates_m: Some([0.002, 0.0]),
            links: Vec::new(),
            link_selector: None,
        },
    ]);
    let mut implicit_entities = vec![SketchEntity {
        id: SketchEntityId("implicit-center".into()),
        sketch: entities[0].sketch.clone(),
        construction: true,
        native_ref: Some("implicit-center".into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(3.0, 4.0),
        },
    }];
    project_relation_point_dimensioned_circles(
        &mut implicit_entities,
        std::slice::from_ref(&feature),
        std::slice::from_ref(&parameter),
        std::slice::from_ref(&implicit_lane),
    );
    assert!(matches!(
        implicit_entities.get(1).map(|entity| &entity.geometry),
        Some(SketchGeometry::Circle { center, radius: Length(2.0) })
            if *center == Point2::new(3.0, 4.0)
    ));
}

#[test]
fn terminal_radial_address_resolves_every_consecutive_equal_radius_pair() {
    let marker = |ordinal: u32, object_index: u32, coordinates_m: [f64; 2]| SketchInputEntity {
        id: format!("marker-{ordinal}"),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal,
        offset: u64::from(ordinal) * 100,
        object_index: Some(object_index),
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m: Some(coordinates_m),
        links: Vec::new(),
        link_selector: None,
    };
    let markers = [
        marker(0, 2, [0.0, 0.0]),
        marker(1, 1, [0.021, 0.0]),
        marker(2, 7, [-0.012, 0.012]),
        marker(3, 6, [-0.0095, 0.012]),
        marker(4, 9, [-0.012, -0.012]),
        marker(5, 8, [-0.0095, -0.012]),
        marker(6, 11, [0.012, -0.012]),
        marker(7, 10, [0.0145, -0.012]),
        marker(8, 13, [0.012, 0.012]),
        marker(9, 12, [0.0145, 0.012]),
    ];
    let roster = markers.iter().collect::<Vec<_>>();

    let pairs = terminal_repeated_radial_circle_pairs(roster.len(), &roster, 0.0025)
        .expect("terminal one-based address and repeated radius");
    assert_eq!(pairs.len(), 4);
    assert_eq!(
        pairs
            .iter()
            .map(|(center, radial)| (center.object_index, radial.object_index))
            .collect::<Vec<_>>(),
        vec![
            (Some(7), Some(6)),
            (Some(9), Some(8)),
            (Some(11), Some(10)),
            (Some(13), Some(12)),
        ]
    );
    assert!(terminal_repeated_radial_circle_pairs(roster.len() - 1, &roster, 0.0025).is_none());
    assert!(terminal_repeated_radial_circle_pairs(roster.len(), &roster, 0.003).is_none());
}

#[test]
fn extended_terminal_radial_record_carries_a_one_based_roster_address() {
    let mut payload = vec![0; 112];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&12u16.to_le_bytes());
    payload[58..60].copy_from_slice(&12u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&(-1i32).to_le_bytes());
    payload[76..78].copy_from_slice(&11u16.to_le_bytes());
    for at in (78..94).step_by(4) {
        payload[at..at + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }

    assert_eq!(
        extended_terminal_repeated_radial_circle_index(&payload, 0),
        Some(12)
    );
    payload[58..60].copy_from_slice(&13u16.to_le_bytes());
    assert_eq!(
        extended_terminal_repeated_radial_circle_index(&payload, 0),
        None
    );
}

#[test]
fn duplicated_extended_curve_address_identifies_a_radial_circle_roster() {
    let mut payload = vec![0; 112 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&7u16.to_le_bytes());
    payload[66..68].copy_from_slice(&7u16.to_le_bytes());
    payload[68..72].copy_from_slice(&1u32.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[80..84].copy_from_slice(&1u32.to_le_bytes());
    payload[112..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    assert_eq!(extended_radial_circle_index(&payload, 0), Some(7));
    payload[66..68].copy_from_slice(&8u16.to_le_bytes());
    assert_eq!(extended_radial_circle_index(&payload, 0), None);
}
