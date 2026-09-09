use super::super::{
    dimensioned_arc_native_geometry, extended_radial_circle_index,
    extended_terminal_repeated_radial_circle_index, terminal_repeated_radial_circle_pairs,
    DimensionedCurveNative,
};
use crate::records::{FeatureInputLane, SketchInputEntity, SketchInputKind, SketchInputLink};
use crate::resolved_features::LEGACY_EXTENDED_SKETCH_MARKER;

#[test]
fn arc_dimension_center_requires_one_matching_radial_witness() {
    let marker = |id: &str, offset: u64, kind, coordinates_m| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker = SketchInputEntity::new(
            marker_id,
            marker_parent,
            u32::try_from(offset).unwrap(),
            offset,
            kind,
        );
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker.state_value = Some(1.0);
        constructed_marker.coordinates_m = coordinates_m;
        constructed_marker.links = None;
        constructed_marker
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
    let marker = |id: &str, offset: u64, coordinates_m| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker = SketchInputEntity::new(
            marker_id,
            marker_parent,
            u32::try_from(offset).unwrap(),
            offset,
            SketchInputKind::Point,
        );
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker.state_value = Some(1.0);
        constructed_marker.coordinates_m = Some(coordinates_m);
        constructed_marker.links = None;
        constructed_marker
    };
    let center = {
        let marker_id: String = "center".into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker =
            SketchInputEntity::new(marker_id, marker_parent, 1, 10, SketchInputKind::Arc);
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker.state_value = Some(1.0);
        constructed_marker.coordinates_m = Some([0.0, 0.0]);
        constructed_marker.links = crate::records::SketchInputLinks::new(
            0,
            vec![
                SketchInputLink {
                    local_id: 0,
                    entity_ref: "start".into(),
                },
                SketchInputLink {
                    local_id: 0,
                    entity_ref: "end".into(),
                },
            ],
        );
        constructed_marker
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
    assert_eq!(arc.endpoints, Some(["start".into(), "end".into()]));

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
    let marker = |ordinal: u32, object_index: u32, coordinates_m: [f64; 2]| {
        let marker_id: String = format!("marker-{ordinal}");
        let marker_parent: String = "lane".into();
        let mut constructed_marker = SketchInputEntity::new(
            marker_id,
            marker_parent,
            ordinal,
            u64::from(ordinal) * 100,
            SketchInputKind::Point,
        );
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker = constructed_marker.with_test_identity(Some(object_index), None);
        constructed_marker.state_value = Some(1.0);
        constructed_marker.coordinates_m = Some(coordinates_m);
        constructed_marker.links = None;
        constructed_marker
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
            .map(|(center, radial)| (center.object_index(), radial.object_index()))
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
