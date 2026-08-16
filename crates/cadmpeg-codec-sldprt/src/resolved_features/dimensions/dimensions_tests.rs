//! Tests for the `dimensions` module.

use super::super::{LEGACY_EXTENDED_SKETCH_MARKER, LEGACY_SKETCH_MARKER};
use super::*;
use crate::records::{
    FeatureInputClass, FeatureInputClassRole, FeatureInputLane, FeatureInputOperand,
    FeatureInputOperandKind, FeatureInputReference, FeatureInputRelationFamily,
    FeatureInputRelationInstance, SketchInputEntity, SketchInputKind, SketchInputLink,
    SketchRelationKind,
};
use cadmpeg_ir::features::{
    DesignParameter, DimensionDisplay, Feature, FeatureDefinition, FeatureId, Length, ParameterId,
    ParameterValue, SketchSpace,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchGeometry, SketchId, SketchPlacement,
};
use std::collections::{BTreeMap, HashMap};

#[test]
fn declared_entity_handle_precedes_generic_operand_resolution() {
    let kind = FeatureInputOperandKind::Native(0x81d5);
    let operand = FeatureInputOperand {
        offset: 100,
        reference_ref: "reference".into(),
        kind,
        entity_index: 0,
        entity_ref: Some("wrong".into()),
    };
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
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let wrong = marker("wrong", 5, Some(1), Some(1), Some([0.100, 0.100]));
    let center = marker("center", 10, Some(50), Some(49), Some([0.010, 0.020]));
    let radial = marker("radial", 20, Some(49), Some(0), Some([0.013, 0.024]));
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
        sketch_entities: vec![wrong, center, radial],
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
    .expect("declared entity-handle carrier");

    assert_eq!(carrier.marker.id, "center");
    assert_eq!(carrier.center, [0.010, 0.020]);
    assert_eq!(carrier.construction, Some(false));

    let mut terminal_lane = lane.clone();
    terminal_lane.sketch_entities[2].local_id = None;
    let terminal_markers = terminal_lane
        .sketch_entities
        .iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let terminal_carrier = dimensioned_relation_carrier(
        std::slice::from_ref(&terminal_lane),
        &terminal_markers,
        "feature",
        &operand,
        5.0,
    )
    .expect("terminal radial address carrier");
    assert_eq!(terminal_carrier.marker.id, "center");
    assert_eq!(terminal_carrier.center, [0.010, 0.020]);

    terminal_lane.sketch_entities[2].kind = SketchInputKind::LineOrCircle;
    let circular_radial_markers = terminal_lane
        .sketch_entities
        .iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let circular_radial_carrier = dimensioned_relation_carrier(
        std::slice::from_ref(&terminal_lane),
        &circular_radial_markers,
        "feature",
        &operand,
        5.0,
    )
    .expect("terminal circular radial address carrier");
    assert_eq!(circular_radial_carrier.marker.id, "center");
    assert_eq!(circular_radial_carrier.center, [0.010, 0.020]);

    let mut ambiguous_lane = lane.clone();
    ambiguous_lane.sketch_entities.extend([
        marker(
            "second-center",
            30,
            Some(60),
            Some(59),
            Some([0.020, 0.030]),
        ),
        marker("second-radial", 40, Some(59), Some(0), Some([0.023, 0.034])),
    ]);
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

    let mut direct_lane = ambiguous_lane;
    direct_lane.sketch_entities[0].kind = SketchInputKind::LineOrCircle;
    let direct_markers = direct_lane
        .sketch_entities
        .iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let direct_carrier = dimensioned_relation_carrier(
        std::slice::from_ref(&direct_lane),
        &direct_markers,
        "feature",
        &operand,
        5.0,
    )
    .expect("explicit circular marker remains a carrier");
    assert_eq!(direct_carrier.marker.id, "wrong");
    assert_eq!(direct_carrier.construction, Some(false));
}

#[test]
fn unlinked_declared_entity_handle_uses_one_circular_marker_with_one_radial_witness() {
    let kind = FeatureInputOperandKind::Native(0x8452);
    let operand = FeatureInputOperand {
        offset: 100,
        reference_ref: "reference".into(),
        kind,
        entity_index: 0,
        entity_ref: None,
    };
    let marker = |id: &str, offset, marker_kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: u32::try_from(offset).unwrap(),
        offset,
        object_index: None,
        local_id: None,
        kind: marker_kind,
        state_value: None,
        coordinates_m: Some(coordinates_m),
        links: Vec::new(),
        link_selector: None,
    };
    let lane = |circular_kind| FeatureInputLane {
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
        sketch_entities: vec![
            marker("circular", 10, circular_kind, [0.010, 0.020]),
            marker("radial", 20, SketchInputKind::Point, [0.013, 0.024]),
        ],
    };

    for circular_kind in [SketchInputKind::Arc, SketchInputKind::LineOrCircle] {
        let lane = lane(circular_kind);
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
        .expect("unlinked declared entity-handle carrier");

        assert_eq!(carrier.marker.id, "circular");
        assert_eq!(carrier.center, [0.010, 0.020]);
        assert!(matches!(
            carrier.curve,
            Some(DimensionedCurveNative::Circle {
                center: [0.010, 0.020]
            })
        ));
        assert_eq!(carrier.construction, Some(false));
    }

    let mut missing_witness_lane = lane(SketchInputKind::LineOrCircle);
    missing_witness_lane.sketch_entities.pop();
    let missing_witness_markers = missing_witness_lane
        .sketch_entities
        .iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    assert!(dimensioned_relation_carrier(
        std::slice::from_ref(&missing_witness_lane),
        &missing_witness_markers,
        "feature",
        &operand,
        5.0,
    )
    .is_none());

    let point_operand = FeatureInputOperand {
        entity_ref: Some("radial".into()),
        ..operand.clone()
    };
    let point_lane = lane(SketchInputKind::Arc);
    let point_markers = point_lane
        .sketch_entities
        .iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let point_carrier = dimensioned_relation_carrier(
        std::slice::from_ref(&point_lane),
        &point_markers,
        "feature",
        &point_operand,
        5.0,
    )
    .expect("point-resolved entity handle carrier");
    assert_eq!(point_carrier.marker.id, "circular");

    let mut ambiguous_lane = lane(SketchInputKind::Arc);
    ambiguous_lane.sketch_entities.extend([
        marker("second-circular", 30, SketchInputKind::Arc, [0.020, 0.030]),
        marker("second-radial", 40, SketchInputKind::Point, [0.023, 0.034]),
    ]);
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

#[test]
fn declared_entity_handle_uses_curve_child_declaration_before_radius_uniqueness() {
    let kind = FeatureInputOperandKind::Native(0x8263);
    let operand = FeatureInputOperand {
        offset: 100,
        reference_ref: "reference".into(),
        kind,
        entity_index: 0,
        entity_ref: Some("arc-radial".into()),
    };
    let marker = |id: &str, offset, marker_kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: u32::try_from(offset).unwrap(),
        offset,
        object_index: None,
        local_id: None,
        kind: marker_kind,
        state_value: None,
        coordinates_m: Some(coordinates_m),
        links: Vec::new(),
        link_selector: None,
    };
    let mut lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: vec![
            FeatureInputClass {
                id: "entity-class".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 100,
                name: "sgEntHandle".into(),
                role: FeatureInputClassRole::SketchEntity,
            },
            FeatureInputClass {
                id: "arc-class".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: 25,
                name: "sgArcHandle".into(),
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
            id: operand.reference_ref.clone(),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal: 0,
            offset: operand.offset,
            kind,
            class_ref: Some("entity-class".into()),
            object_index: 0,
        }],
        sketch_entities: vec![
            marker("arc", 10, SketchInputKind::Arc, [0.0, 0.0]),
            marker("arc-radial", 20, SketchInputKind::Point, [0.003, 0.004]),
            marker(
                "other-circle",
                30,
                SketchInputKind::LineOrCircle,
                [0.1, 0.0],
            ),
            marker("other-radial", 40, SketchInputKind::Point, [0.103, 0.004]),
        ],
    };
    lane.sketch_entities[0].object_index = Some(50);
    lane.sketch_entities[0].local_id = Some(49);
    lane.sketch_entities[1].object_index = Some(49);
    lane.sketch_entities[1].local_id = Some(0);
    lane.sketch_entities[2].object_index = Some(70);
    lane.sketch_entities[2].local_id = Some(69);
    lane.sketch_entities[3].object_index = Some(69);
    lane.sketch_entities[3].local_id = Some(0);
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
    .expect("declared curve child pair");

    assert_eq!(carrier.marker.id, "arc");
    assert_eq!(carrier.center, [0.0, 0.0]);
    assert!(matches!(
        carrier.curve,
        Some(DimensionedCurveNative::Circle { center: [0.0, 0.0] })
    ));

    let mut unbound_operand = operand.clone();
    unbound_operand.entity_ref = None;
    let unbound_markers = lane
        .sketch_entities
        .iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    assert!(dimensioned_relation_carrier(
        std::slice::from_ref(&lane),
        &unbound_markers,
        "feature",
        &unbound_operand,
        5.0,
    )
    .is_none());

    let mut multiple_declared = lane.clone();
    multiple_declared.classes.push(FeatureInputClass {
        id: "line-class".into(),
        parent: "lane".into(),
        ordinal: 2,
        offset: 45,
        name: "sgLineHandle".into(),
        role: FeatureInputClassRole::SketchEntity,
    });
    let multiple_markers = multiple_declared
        .sketch_entities
        .iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    assert!(dimensioned_relation_carrier(
        std::slice::from_ref(&multiple_declared),
        &multiple_markers,
        "feature",
        &operand,
        5.0,
    )
    .is_none());

    let mut mismatched_declared = lane;
    mismatched_declared.sketch_entities[1].coordinates_m = Some([0.004, 0.0]);
    let mismatched_markers = mismatched_declared
        .sketch_entities
        .iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    assert!(dimensioned_relation_carrier(
        std::slice::from_ref(&mismatched_declared),
        &mismatched_markers,
        "feature",
        &operand,
        5.0,
    )
    .is_none());
}

#[test]
fn transformed_dimensioned_arc_swaps_endpoint_identity_with_minor_geometry() {
    let sketch = Sketch {
        id: SketchId("sketch".into()),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: Vec::new(),
        native_ref: None,
    };
    let transform = super::super::transforms::sketch_frame_marker_transform(&sketch, 1.0e-8)
        .expect("axis-aligned sketch has a marker transform");
    let arc = super::DimensionedArcNative {
        center: [0.0, 0.0],
        start: [0.001, 0.0],
        end: [0.0, -0.001],
        endpoint_refs: vec!["start".into(), "end".into()],
    };

    let (geometry, endpoint_refs) =
        super::transformed_dimensioned_arc(transform, &arc, 1000.0, 1.0e-8)
            .expect("valid dimensioned arc");
    assert_eq!(endpoint_refs, vec!["end", "start"]);
    let SketchGeometry::Arc {
        start_angle,
        end_angle,
        ..
    } = geometry
    else {
        panic!("dimensioned carrier should remain an arc");
    };
    let sweep = (end_angle.0 - start_angle.0).rem_euclid(std::f64::consts::TAU);
    assert!(sweep <= std::f64::consts::PI + 1.0e-9);
}

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
fn native_radial_role_propagates_omitted_circle_construction_state() {
    let payload = |construction: bool| {
        let mut payload = vec![0; 104 + LEGACY_SKETCH_MARKER.len()];
        payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
        payload[5..13].copy_from_slice(if construction {
            &[0xff, 0xff, 0xff, 0xff, 0x04, 0x00, 0xff, 0xff]
        } else {
            &[0xff; 8]
        });
        payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[17..21].copy_from_slice(&(if construction { 7u32 } else { 2 }).to_le_bytes());
        payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
        payload[27..29].copy_from_slice(&(if construction { 2u16 } else { 1 }).to_le_bytes());
        payload[29..31].copy_from_slice(&(u16::from(!construction)).to_le_bytes());
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
        payload[56..58].copy_from_slice(&1u16.to_le_bytes());
        payload[58..60].copy_from_slice(&1u16.to_le_bytes());
        payload[60..64].copy_from_slice(&u32::from(!construction).to_le_bytes());
        payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
        payload[72..76].copy_from_slice(&1i32.to_le_bytes());
        payload[76..78].copy_from_slice(&(if construction { 8u16 } else { 4 }).to_le_bytes());
        for at in (78..94).step_by(4) {
            payload[at..at + 4].copy_from_slice(&(-2i32).to_le_bytes());
        }
        payload[104..].copy_from_slice(LEGACY_SKETCH_MARKER);
        payload
    };
    let marker = |id: &str, offset, kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: offset as u32,
        offset,
        object_index: None,
        local_id: None,
        kind,
        state_value: None,
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let center = marker("center", 10, SketchInputKind::Point, Some([0.0, 0.0]));
    let radial = marker("radial", 20, SketchInputKind::Point, Some([0.005, 0.0]));
    let record = marker("radial-record", 0, SketchInputKind::LineOrCircle, None);
    for construction in [true, false] {
        let native = payload(construction);
        assert_eq!(compact_radial_circle_index(&native, 0), Some(1));
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: native,
            classes: Vec::new(),
            names: Vec::new(),
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: vec![record.clone(), center.clone(), radial.clone()],
        };
        let markers_by_id = lane
            .sketch_entities
            .iter()
            .map(|marker| (marker.id.as_str(), marker))
            .collect::<HashMap<_, _>>();
        let operand = FeatureInputOperand {
            offset: 0,
            reference_ref: "reference".into(),
            kind: FeatureInputOperandKind::Native(0x83fe),
            entity_index: 0,
            entity_ref: Some(center.id.clone()),
        };
        let carrier = dimensioned_relation_carrier(
            std::slice::from_ref(&lane),
            &markers_by_id,
            "feature",
            &operand,
            5.0,
        )
        .expect("dimensioned carrier");
        assert_eq!(carrier.construction, Some(construction));
    }
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
fn arc_dimension_center_requires_one_matching_radial_witness() {
    let marker = |id: &str, offset: u64, kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: u32::try_from(offset).unwrap(),
        offset,
        object_index: None,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let center = marker("center", 10, SketchInputKind::Arc, Some([0.1, 0.2]));
    let radial = marker("radial", 20, SketchInputKind::Point, Some([0.103, 0.2]));
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
        references: Vec::new(),
        sketch_entities: vec![center.clone(), radial],
    };

    assert!(matches!(
        dimensioned_arc_native_geometry(std::slice::from_ref(&lane), &center, 3.0),
        Some(DimensionedCurveNative::Circle { center: [u, v] })
            if [u, v] == [0.1, 0.2]
    ));

    let mut ambiguous_lane = lane;
    ambiguous_lane.sketch_entities.push(marker(
        "second-radial",
        30,
        SketchInputKind::ConstrainedPoint,
        Some([0.1, 0.203]),
    ));
    assert!(dimensioned_arc_native_geometry(
        std::slice::from_ref(&ambiguous_lane),
        &ambiguous_lane.sketch_entities[0],
        3.0
    )
    .is_none());
}

#[test]
fn arc_dimension_uses_two_endpoint_markers_for_a_bounded_arc() {
    let marker = |id: &str, offset: u64, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: u32::try_from(offset).unwrap(),
        offset,
        object_index: None,
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m: Some(coordinates_m),
        links: Vec::new(),
        link_selector: None,
    };
    let center = SketchInputEntity {
        id: "center".into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 1,
        offset: 10,
        object_index: None,
        local_id: None,
        kind: SketchInputKind::Arc,
        state_value: Some(1.0),
        coordinates_m: Some([0.0, 0.0]),
        links: vec![
            SketchInputLink {
                local_id: 0,
                entity_ref: "start".into(),
            },
            SketchInputLink {
                local_id: 0,
                entity_ref: "end".into(),
            },
        ],
        link_selector: None,
    };
    let start = marker("start", 20, [0.003, 0.0]);
    let end = marker("end", 30, [0.0, 0.003]);
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
        references: Vec::new(),
        sketch_entities: vec![center.clone(), start, end],
    };

    let Some(DimensionedCurveNative::Arc(arc)) =
        dimensioned_arc_native_geometry(std::slice::from_ref(&lane), &center, 3.0)
    else {
        panic!("two endpoints should define a bounded arc");
    };
    assert_eq!(arc.center, [0.0, 0.0]);
    assert_eq!(arc.start, [0.003, 0.0]);
    assert_eq!(arc.end, [0.0, 0.003]);
    assert_eq!(arc.endpoint_refs, vec!["start", "end"]);

    let mut invalid_end = lane.sketch_entities[2].clone();
    invalid_end.coordinates_m = Some([0.0, 0.004]);
    let invalid_lane = FeatureInputLane {
        sketch_entities: vec![center, lane.sketch_entities[1].clone(), invalid_end],
        ..lane
    };
    assert!(dimensioned_arc_native_geometry(
        std::slice::from_ref(&invalid_lane),
        &invalid_lane.sketch_entities[0],
        3.0
    )
    .is_none());
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
