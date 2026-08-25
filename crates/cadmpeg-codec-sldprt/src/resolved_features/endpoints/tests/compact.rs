//! Compact and current-generation curve endpoint tests.
#![allow(unused_imports)]

use super::super::super::bindings::normalize_indexed_curve_entities;
use super::super::super::curves::compact_bounded_curve_tangent;
use super::super::super::markers::{marker_coordinates, sketch_input_entities};
use super::super::super::relation_loci::same_dimension_length;
use super::super::super::selections::marker_local_links;
use super::super::super::typed_relations::{
    current_undetailed_bounded_curve_is_line, extended_direct_object_line_endpoints,
    legacy_marker104_arc_endpoints, marker_curve_endpoint_markers,
};
use super::super::super::{
    CLASS_MARKER, LEGACY_EXTENDED_SKETCH_MARKER, LEGACY_SKETCH_MARKER, SKETCH_MARKER,
};
use super::super::*;
use crate::records::{
    FeatureInputLane, SketchInputEntity, SketchInputKind, SketchInputLink, SketchRelationKind,
};
use cadmpeg_ir::math::Point2;
use std::collections::HashMap;

#[test]
fn one_ended_line_uses_its_same_index_radius_relation_pair() {
    let marker = |id: &str, kind, object_index, coordinates_m, links| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: 0,
        object_index,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links,
        link_selector: None,
    };
    let first = marker(
        "first",
        SketchInputKind::Point,
        Some(1),
        Some([0.0, 0.0]),
        Vec::new(),
    );
    let second = marker(
        "second",
        SketchInputKind::Point,
        Some(2),
        Some([1.0, 0.0]),
        Vec::new(),
    );
    let third = marker(
        "third",
        SketchInputKind::Point,
        Some(3),
        Some([2.0, 0.0]),
        Vec::new(),
    );
    let line = marker(
        "line",
        SketchInputKind::LineOrCircle,
        Some(4),
        None,
        vec![SketchInputLink {
            local_id: 2,
            entity_ref: second.id.clone(),
        }],
    );
    let relation = |id: &str, other: &SketchInputEntity| {
        marker(
            id,
            SketchInputKind::Relation(SketchRelationKind::Radius),
            line.object_index,
            None,
            vec![
                SketchInputLink {
                    local_id: 1,
                    entity_ref: other.id.clone(),
                },
                SketchInputLink {
                    local_id: 2,
                    entity_ref: second.id.clone(),
                },
            ],
        )
    };
    let radius = relation("radius", &first);
    let markers_by_id = [&first, &second, &third, &line, &radius]
        .into_iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let markers = [&first, &second, &third, &line, &radius];

    assert_eq!(
        output_curve_endpoint_markers(&[], &line, &markers_by_id, &markers)
            .into_iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );

    let competing = relation("competing", &third);
    let markers_by_id = [&first, &second, &third, &line, &radius, &competing]
        .into_iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let markers = [&first, &second, &third, &line, &radius, &competing];
    assert_ne!(
        output_curve_endpoint_markers(&[], &line, &markers_by_id, &markers).len(),
        2
    );
}

#[test]
fn one_ended_line_accepts_a_direct_radius_relation_link_only_when_unique() {
    let marker = |id: &str, kind, object_index, coordinates_m, links| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: 0,
        object_index,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links,
        link_selector: None,
    };
    let first = marker(
        "first",
        SketchInputKind::Point,
        Some(1),
        Some([0.0, 0.0]),
        Vec::new(),
    );
    let second = marker(
        "second",
        SketchInputKind::Point,
        Some(2),
        Some([1.0, 0.0]),
        Vec::new(),
    );
    let third = marker(
        "third",
        SketchInputKind::Point,
        Some(3),
        Some([2.0, 0.0]),
        Vec::new(),
    );
    let mut line = marker(
        "line",
        SketchInputKind::LineOrCircle,
        Some(4),
        None,
        vec![SketchInputLink {
            local_id: 2,
            entity_ref: second.id.clone(),
        }],
    );
    let relation = |id: &str, other: &SketchInputEntity| {
        marker(
            id,
            SketchInputKind::Relation(SketchRelationKind::Radius),
            line.object_index,
            None,
            vec![
                SketchInputLink {
                    local_id: 1,
                    entity_ref: other.id.clone(),
                },
                SketchInputLink {
                    local_id: 2,
                    entity_ref: second.id.clone(),
                },
            ],
        )
    };
    let radius = relation("radius", &first);
    line.links.push(SketchInputLink {
        local_id: 1,
        entity_ref: radius.id.clone(),
    });
    let markers_by_id = [&first, &second, &third, &line, &radius]
        .into_iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let markers = [&first, &second, &third, &line, &radius];

    assert_eq!(
        output_curve_endpoint_markers(&[], &line, &markers_by_id, &markers)
            .into_iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );

    let competing = relation("competing", &third);
    let markers_by_id = [&first, &second, &third, &line, &radius, &competing]
        .into_iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let markers = [&first, &second, &third, &line, &radius, &competing];
    assert_ne!(
        output_curve_endpoint_markers(&[], &line, &markers_by_id, &markers).len(),
        2
    );
}

#[test]
fn coordinate_profile_line_uses_its_own_coordinate_and_one_point_link() {
    let point = SketchInputEntity {
        id: "point".into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 1,
        offset: 1,
        object_index: Some(2),
        local_id: Some(2),
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m: Some([0.0, 1.0]),
        links: Vec::new(),
        link_selector: None,
    };
    let relation = SketchInputEntity {
        id: "relation".into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 2,
        offset: 2,
        object_index: Some(3),
        local_id: Some(3),
        kind: SketchInputKind::Relation(SketchRelationKind::Horizontal),
        state_value: None,
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };
    let curve = SketchInputEntity {
        id: "curve".into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: 0,
        object_index: Some(1),
        local_id: Some(1),
        kind: SketchInputKind::LineOrCircle,
        state_value: Some(1.0),
        coordinates_m: Some([1.0, 0.0]),
        links: vec![
            SketchInputLink {
                local_id: 3,
                entity_ref: relation.id.clone(),
            },
            SketchInputLink {
                local_id: 2,
                entity_ref: point.id.clone(),
            },
        ],
        link_selector: None,
    };
    let markers = [&curve, &point, &relation];
    let markers_by_id = markers
        .iter()
        .map(|marker| (marker.id.as_str(), *marker))
        .collect::<HashMap<_, _>>();

    for prefix in [SKETCH_MARKER, LEGACY_EXTENDED_SKETCH_MARKER] {
        let mut payload = vec![0; 100];
        payload[..prefix.len()].copy_from_slice(prefix);
        payload[17..21].copy_from_slice(&1u32.to_le_bytes());
        payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
        payload[27..29].copy_from_slice(&1u16.to_le_bytes());
        payload[64..66].copy_from_slice(&[0x1e, 0x00]);

        assert_eq!(
            marker_curve_endpoint_markers(&payload, &curve, &markers_by_id, &markers)
                .into_iter()
                .map(|marker| marker.id.as_str())
                .collect::<Vec<_>>(),
            ["curve", "point"]
        );
    }
}

#[test]
fn shared_endpoint_resolution_uses_compact_legacy_code_one_line_records() {
    let mut payload = vec![0; 68 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&1u32.to_le_bytes());
    payload[17..23].copy_from_slice(&[0x00, 0x00, 0x04, 0x00, 0x02, 0x00]);
    payload[23..25].copy_from_slice(&1u16.to_le_bytes());
    payload[25..27].copy_from_slice(&1u16.to_le_bytes());
    payload[31] = 0x04;
    payload[42..44].copy_from_slice(&0u16.to_le_bytes());
    payload[44..46].copy_from_slice(&1u16.to_le_bytes());
    payload[46..50].copy_from_slice(&1u32.to_le_bytes());
    payload[50..58].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[68..].copy_from_slice(LEGACY_SKETCH_MARKER);

    let point = |id: &str, offset: u64, object_index: u32, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset,
        object_index: Some(object_index),
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let curve = SketchInputEntity {
        id: "line".into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: 0,
        object_index: Some(3),
        local_id: None,
        kind: SketchInputKind::LineOrCircle,
        state_value: Some(1.0),
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };
    let first = point("first", 100, 1, Some([0.0, 0.0]));
    let second = point("second", 200, 2, Some([1.0, 0.0]));
    let markers = [&curve, &first, &second];

    assert_eq!(
        compact_legacy_code_one_line_endpoint_indices(&payload, 0),
        Some([1, 2])
    );
    assert_eq!(
        roster_curve_endpoint_markers(&payload, &curve, &markers)
            .into_iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        vec!["first", "second"]
    );
}

#[test]
fn compact_legacy_90_geometry_line_uses_feature_marker_roster() {
    let marker_size = LEGACY_SKETCH_MARKER.len();
    let mut payload = vec![0; 90 + marker_size];
    payload[..marker_size].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&1u32.to_le_bytes());
    payload[19..23].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[23..25].copy_from_slice(&1u16.to_le_bytes());
    payload[25..27].copy_from_slice(&1u16.to_le_bytes());
    payload[31] = 0x04;
    payload[42..44].copy_from_slice(&1u16.to_le_bytes());
    payload[44..46].copy_from_slice(&3u16.to_le_bytes());
    payload[46..50].copy_from_slice(&1u32.to_le_bytes());
    payload[50..58].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[58..62].copy_from_slice(&1u32.to_le_bytes());
    payload[62..64].copy_from_slice(&31u16.to_le_bytes());
    for cell in payload[64..80].chunks_exact_mut(4) {
        cell.copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[82..86].copy_from_slice(&9u32.to_le_bytes());
    payload[86..90].copy_from_slice(&10u32.to_le_bytes());
    payload[90..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        compact_legacy_90_geometry_line_roster_indices(&payload, 0),
        Some([1, 3])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::LineOrCircle
    );

    let point = |id: &str, offset: u64, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset,
        object_index: None,
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let curve = SketchInputEntity {
        id: "line".into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: 0,
        object_index: None,
        local_id: None,
        kind: SketchInputKind::LineOrCircle,
        state_value: Some(1.0),
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };
    let first = point("first", 100, Some([0.0, 0.0]));
    let non_point = SketchInputEntity {
        id: "handle".into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: 200,
        object_index: None,
        local_id: None,
        kind: SketchInputKind::Native(7),
        state_value: None,
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };
    let second = point("second", 300, Some([1.0, 0.0]));
    let markers = [&curve, &first, &non_point, &second];

    assert_eq!(
        roster_curve_endpoint_markers(&payload, &curve, &markers)
            .into_iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        vec!["first", "second"]
    );

    payload[19..23].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    assert_eq!(
        compact_legacy_90_geometry_line_roster_indices(&payload, 0),
        None
    );

    payload[19..23].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload.resize(138, 0);
    payload[82..136].fill(0);
    payload[136..138].copy_from_slice(&[0x08, 0x80]);
    assert_eq!(
        compact_legacy_90_geometry_line_roster_indices(&payload, 0),
        Some([1, 3])
    );
}

#[test]
fn compact_legacy_embedded_geometry_preserves_coordinate_roster_ordinals() {
    let marker_size = LEGACY_SKETCH_MARKER.len();
    let mut payload = vec![0; 132 + 120 + 120 + 120 + 68 + marker_size];
    let profile_point = |payload: &mut [u8], offset: usize, code: u32, coordinate: [f64; 2]| {
        payload[offset..offset + marker_size].copy_from_slice(LEGACY_SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&code.to_le_bytes());
        payload[offset + 19..offset + 25].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
        payload[offset + 31..offset + 42].copy_from_slice(&[0x04, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        payload[offset + 42..offset + 44].copy_from_slice(&[0x1e, 0x00]);
        payload[offset + 44..offset + 52].copy_from_slice(&coordinate[0].to_le_bytes());
        payload[offset + 52..offset + 60].copy_from_slice(&coordinate[1].to_le_bytes());
    };
    profile_point(&mut payload, 0, 2, [0.03, 0.005]);
    payload[62..64].copy_from_slice(&4u16.to_le_bytes());
    for (relative, id) in [(64, 8u16), (72, 11u16)] {
        payload[relative..relative + 2].copy_from_slice(&0x811au16.to_le_bytes());
        payload[relative + 2..relative + 4].copy_from_slice(&id.to_le_bytes());
        payload[relative + 4..relative + 8].fill(0xff);
    }
    payload[80..86].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[120..124].copy_from_slice(&2u32.to_le_bytes());
    payload[128..132].copy_from_slice(&10u32.to_le_bytes());

    let second_offset = 132;
    profile_point(&mut payload, second_offset, 0, [-0.03, 0.005]);
    let embedded_offset = second_offset + 120;
    payload[embedded_offset..embedded_offset + marker_size].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[embedded_offset + 5..embedded_offset + 13].fill(0xff);
    payload[embedded_offset + 19..embedded_offset + 25]
        .copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[embedded_offset + 31..embedded_offset + 42]
        .copy_from_slice(&[0x05, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    payload[embedded_offset + 42..embedded_offset + 44].copy_from_slice(&[0x1e, 0x00]);
    payload[embedded_offset + 44..embedded_offset + 52].copy_from_slice(&0.03f64.to_le_bytes());
    payload[embedded_offset + 52..embedded_offset + 60].copy_from_slice(&0.005f64.to_le_bytes());
    payload[embedded_offset + 70..embedded_offset + 74].copy_from_slice(&[0xfe, 0xff, 0xff, 0xff]);
    payload[embedded_offset + 116..embedded_offset + 120].copy_from_slice(&12u32.to_le_bytes());

    let third_offset = embedded_offset + 120;
    profile_point(&mut payload, third_offset, 0, [-0.03, -0.005]);
    let curve_offset = third_offset + 120;
    payload[curve_offset..curve_offset + marker_size].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[curve_offset + 5..curve_offset + 13].fill(0xff);
    payload[curve_offset + 19..curve_offset + 25]
        .copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[curve_offset + 25..curve_offset + 27].copy_from_slice(&1u16.to_le_bytes());
    payload[curve_offset + 31..curve_offset + 42]
        .copy_from_slice(&[0x04, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    payload[curve_offset + 42..curve_offset + 44].copy_from_slice(&1u16.to_le_bytes());
    payload[curve_offset + 44..curve_offset + 46].copy_from_slice(&3u16.to_le_bytes());
    payload[curve_offset + 46..curve_offset + 50].copy_from_slice(&1u32.to_le_bytes());
    payload[curve_offset + 50..curve_offset + 58].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[curve_offset + 68..].copy_from_slice(LEGACY_SKETCH_MARKER);

    let mut entities = sketch_input_entities(&payload, "lane");
    assert_eq!(
        entities
            .iter()
            .map(|entity| entity.kind)
            .collect::<Vec<_>>(),
        vec![
            SketchInputKind::Point,
            SketchInputKind::Point,
            SketchInputKind::Point,
            SketchInputKind::LineOrCircle,
        ]
    );
    for entity in &mut entities {
        entity.feature_ref = Some("feature".into());
    }
    let markers = entities.iter().collect::<Vec<_>>();
    let endpoints = coordinate_roster_curve_endpoint_markers(&payload, &entities[3], &markers);
    assert_eq!(
        endpoints
            .iter()
            .map(|marker| marker.coordinates_m)
            .collect::<Vec<_>>(),
        vec![Some([-0.03, 0.005]), Some([-0.03, -0.005])]
    );
}

#[test]
fn compact_legacy_generation_carries_points_curves_and_selected_axes() {
    let mut payload = vec![0; 280 + LEGACY_SKETCH_MARKER.len()];
    let header = |payload: &mut [u8], offset: usize, code: u32, role: u16, flag: u8| {
        payload[offset..offset + 5].copy_from_slice(LEGACY_SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&code.to_le_bytes());
        payload[offset + 17..offset + 23].copy_from_slice(&[0x00, 0x00, 0x04, 0x00, 0x02, 0x00]);
        payload[offset + 23..offset + 25].copy_from_slice(&role.to_le_bytes());
        payload[offset + 31] = flag;
    };

    header(&mut payload, 0, 1, 1, 4);
    payload[42..44].copy_from_slice(&[0x1e, 0x00]);
    payload[44..52].copy_from_slice(&0.029f64.to_le_bytes());
    payload[52..60].copy_from_slice(&0.0f64.to_le_bytes());

    header(&mut payload, 132, 0, 1, 4);
    payload[157..159].copy_from_slice(&1u16.to_le_bytes());
    payload[174..176].copy_from_slice(&0u16.to_le_bytes());
    payload[176..178].copy_from_slice(&1u16.to_le_bytes());
    payload[178..182].copy_from_slice(&1u32.to_le_bytes());
    payload[182..190].copy_from_slice(&(-1.0f64).to_le_bytes());

    header(&mut payload, 200, 0, 2, 12);
    payload[205..209].fill(0xff);
    payload[209..213].copy_from_slice(&[0x04, 0x00, 0xff, 0xff]);
    payload[242..244].copy_from_slice(&15u16.to_le_bytes());
    payload[244..246].copy_from_slice(&0u16.to_le_bytes());
    payload[250..258].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[280..285].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(marker_coordinates(&payload, 0), Some([0.029, 0.0]));
    assert_eq!(
        compact_legacy_curve_endpoint_indices(&payload, 132),
        Some([1, 2])
    );
    assert_eq!(
        compact_legacy_selected_axis_endpoint_indices(&payload, 200),
        Some([16, 1])
    );
}

#[test]
fn compact_legacy_geometry_locus_carries_curve_endpoint_indices() {
    let mut payload = vec![0; 68 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&0u32.to_le_bytes());
    payload[17..23].copy_from_slice(&[0x00, 0x00, 0x05, 0x00, 0x01, 0x00]);
    payload[23..25].copy_from_slice(&1u16.to_le_bytes());
    payload[25..27].copy_from_slice(&1u16.to_le_bytes());
    payload[31] = 0x04;
    payload[42..44].copy_from_slice(&29u16.to_le_bytes());
    payload[44..46].copy_from_slice(&30u16.to_le_bytes());
    payload[46..50].copy_from_slice(&1u32.to_le_bytes());
    payload[50..58].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[68..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        compact_legacy_curve_endpoint_indices(&payload, 0),
        Some([30, 31])
    );
    let entities = sketch_input_entities(&payload, "lane");
    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0].kind, SketchInputKind::LineOrCircle);

    payload[68..].fill(0);
    payload.resize(90 + LEGACY_SKETCH_MARKER.len(), 0);
    payload[58..62].copy_from_slice(&1u32.to_le_bytes());
    payload[62..64].copy_from_slice(&41u16.to_le_bytes());
    for cell in payload[64..80].chunks_exact_mut(4) {
        cell.copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[82..86].copy_from_slice(&76u32.to_le_bytes());
    payload[90..].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(
        compact_legacy_curve_endpoint_indices(&payload, 0),
        Some([30, 31])
    );
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(42));

    payload[82..].fill(0);
    payload.resize(138, 0);
    payload[136..138].copy_from_slice(&[0x08, 0x80]);
    assert_eq!(
        compact_legacy_curve_endpoint_indices(&payload, 0),
        Some([30, 31])
    );
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(42));
}

#[test]
fn compact_legacy_short_role_two_curve_carries_endpoint_indices() {
    let mut payload = vec![0; 68 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&1u32.to_le_bytes());
    payload[17..23].copy_from_slice(&[0x00, 0x00, 0x04, 0x00, 0x02, 0x00]);
    payload[23..25].copy_from_slice(&2u16.to_le_bytes());
    payload[31] = 0x0c;
    payload[42..44].copy_from_slice(&1u16.to_le_bytes());
    payload[44..46].copy_from_slice(&3u16.to_le_bytes());
    payload[50..58].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[58..60].copy_from_slice(&1u16.to_le_bytes());
    payload[64..68].copy_from_slice(&2u32.to_le_bytes());
    payload[68..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        compact_legacy_short_role_two_curve_endpoint_indices(&payload, 0),
        Some([2, 4])
    );
    assert!(marker_is_selected_construction_line(&payload, 0));
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(42));
    payload[23..25].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        compact_legacy_short_role_two_curve_endpoint_indices(&payload, 0),
        None
    );
    payload[23..25].copy_from_slice(&2u16.to_le_bytes());
    payload[25..27].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        compact_legacy_short_role_two_curve_endpoint_indices(&payload, 0),
        None
    );
    payload[25..27].fill(0);
    payload[64..68].fill(0xff);
    assert_eq!(
        compact_legacy_short_role_two_curve_endpoint_indices(&payload, 0),
        None
    );
}

#[test]
fn compact_legacy_short_role_one_curve_indexes_the_coordinate_roster() {
    let mut payload = vec![0; 68 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&1u32.to_le_bytes());
    payload[17..23].copy_from_slice(&[0x00, 0x00, 0x04, 0x00, 0x02, 0x00]);
    payload[23..27].copy_from_slice(&[0x01, 0x00, 0x01, 0x00]);
    payload[31] = 0x04;
    payload[42..44].copy_from_slice(&0u16.to_le_bytes());
    payload[44..46].copy_from_slice(&2u16.to_le_bytes());
    payload[46..50].copy_from_slice(&1u32.to_le_bytes());
    payload[50..58].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[60..62].copy_from_slice(&3u16.to_le_bytes());
    payload[64..68].copy_from_slice(&2u32.to_le_bytes());
    payload[68..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        compact_legacy_short_role_one_curve_endpoint_indices(&payload, 0),
        Some([1, 3])
    );
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(42));
    payload[33..35].copy_from_slice(&0x18u16.to_le_bytes());
    assert_eq!(
        compact_legacy_short_role_one_curve_endpoint_indices(&payload, 0),
        Some([1, 3])
    );
    assert_eq!(
        compact_legacy_code_one_line_endpoint_indices(&payload, 0),
        Some([1, 3])
    );
    payload[25..27].fill(0);
    payload[46..50].fill(0);
    assert_eq!(
        compact_legacy_short_role_one_curve_endpoint_indices(&payload, 0),
        Some([1, 3])
    );
    payload[23..25].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(
        compact_legacy_short_role_one_curve_endpoint_indices(&payload, 0),
        None
    );
    payload[23..25].copy_from_slice(&1u16.to_le_bytes());
    payload[46..50].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        compact_legacy_short_role_one_curve_endpoint_indices(&payload, 0),
        None
    );
}

#[test]
fn packed_legacy_curve_codes_carry_coordinate_roster_indices() {
    let mut payload = vec![0; 76 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[17..23].copy_from_slice(&[0x00, 0x00, 0x04, 0x00, 0x02, 0x00]);
    payload[23..25].copy_from_slice(&1u16.to_le_bytes());
    payload[25..27].copy_from_slice(&1u16.to_le_bytes());
    payload[29] = 0x04;
    payload[40..48].copy_from_slice(&1.0f64.to_le_bytes());
    payload[48..50].copy_from_slice(&3u16.to_le_bytes());
    payload[50..52].copy_from_slice(&4u16.to_le_bytes());
    payload[52..56].copy_from_slice(&1u32.to_le_bytes());
    payload[56..64].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[76..].copy_from_slice(LEGACY_SKETCH_MARKER);

    for code in 0u32..=2 {
        payload[13..17].copy_from_slice(&code.to_le_bytes());
        assert_eq!(
            packed_legacy_curve_endpoint_indices(&payload, 0),
            Some([3, 4])
        );
    }
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(48));

    payload[13..17].copy_from_slice(&3u32.to_le_bytes());
    assert_eq!(packed_legacy_curve_endpoint_indices(&payload, 0), None);
}

#[test]
fn compact_curve_uses_one_based_endpoint_indices() {
    for prefix in [
        LEGACY_SKETCH_MARKER,
        LEGACY_EXTENDED_SKETCH_MARKER,
        SKETCH_MARKER,
    ] {
        let mut payload = vec![0; 84 + prefix.len()];
        payload[..prefix.len()].copy_from_slice(prefix);
        payload[5..13].fill(0xff);
        payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
        payload[27..29].copy_from_slice(&2u16.to_le_bytes());
        payload[35..39].copy_from_slice(&[0x00, 0x00, 0x0d, 0x00]);
        payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[56..58].copy_from_slice(&6u16.to_le_bytes());
        payload[58..60].copy_from_slice(&11u16.to_le_bytes());
        payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
        payload[84..].copy_from_slice(prefix);

        assert_eq!(compact_curve_endpoint_indices(&payload, 0), Some([7, 12]));
        assert_eq!(
            sketch_input_entities(&payload, "lane")[0].kind,
            SketchInputKind::LineOrCircle
        );
    }
}

#[test]
fn alternate_current_curve_roster_distinguishes_the_selected_axis() {
    let mut payload = vec![0; 168 + SKETCH_MARKER.len()];
    let record = |payload: &mut [u8], offset: usize, role: u16, state: u8| {
        payload[offset..offset + 5].copy_from_slice(SKETCH_MARKER);
        payload[offset + 5..offset + 9].fill(0xff);
        payload[offset + 9..offset + 13].copy_from_slice(&if role == 1 {
            [0x00, 0x00, 0xff, 0xff]
        } else {
            [0x04, 0x00, 0xff, 0xff]
        });
        payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 23..offset + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
        payload[offset + 27..offset + 29].copy_from_slice(&role.to_le_bytes());
        payload[offset + 29..offset + 31].copy_from_slice(&u16::from(role == 1).to_le_bytes());
        payload[offset + 31..offset + 35].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 35..offset + 39].copy_from_slice(&[0x00, 0x00, state, 0x00]);
        payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[offset + 56..offset + 58].copy_from_slice(&56u16.to_le_bytes());
        payload[offset + 58..offset + 60].copy_from_slice(&57u16.to_le_bytes());
        payload[offset + 60..offset + 64].copy_from_slice(&u32::from(role == 1).to_le_bytes());
        payload[offset + 64..offset + 72].copy_from_slice(&(-1.0f64).to_le_bytes());
    };

    record(&mut payload, 0, 1, 5);
    payload[76..80].copy_from_slice(&8u32.to_le_bytes());
    payload[80..84].copy_from_slice(&5u32.to_le_bytes());
    record(&mut payload, 84, 2, 13);
    payload[160..164].copy_from_slice(&43u32.to_le_bytes());
    payload[164..168].copy_from_slice(&47u32.to_le_bytes());
    payload[168..173].copy_from_slice(SKETCH_MARKER);

    assert_eq!(
        alternate_current_indexed_curve_endpoint_indices(&payload, 0),
        Some([57, 58])
    );
    assert_eq!(
        alternate_current_selected_axis_endpoint_indices(&payload, 84),
        Some([57, 58])
    );
    payload[160..168].fill(0);
    assert_eq!(
        alternate_current_selected_axis_endpoint_indices(&payload, 84),
        None
    );
    payload[119..123].copy_from_slice(&[0x00, 0x00, 0x0c, 0x00]);
    assert_eq!(
        alternate_current_selected_axis_endpoint_indices(&payload, 84),
        None
    );
}

#[test]
fn current_compact_selected_axis_indexes_the_zero_based_coordinate_roster() {
    let mut payload = vec![0; 84 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x02, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0d, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&10u16.to_le_bytes());
    payload[58..60].copy_from_slice(&11u16.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[76..80].copy_from_slice(&6u32.to_le_bytes());
    payload[80..84].copy_from_slice(&6u32.to_le_bytes());
    payload[84..].copy_from_slice(SKETCH_MARKER);

    assert!(super::current_compact_roster_selected_axis(&payload, 0));
    assert_eq!(super::coordinate_roster_endpoint_offset(&payload, 0), None);
    assert!(!super::marker_is_selected_construction_line(&payload, 0));

    payload[72..76].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
    assert!(super::current_compact_roster_selected_axis(&payload, 0));

    payload[58..60].copy_from_slice(&10u16.to_le_bytes());
    assert!(!super::current_compact_roster_selected_axis(&payload, 0));
}

#[test]
fn unrecognized_role_two_records_are_auxiliary() {
    let mut payload = vec![0; 112 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x02, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&2u16.to_le_bytes());
    payload[66..68].copy_from_slice(&3u16.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[112..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert!(auxiliary_profile_record(&payload, 0));
    payload[17..21].fill(0);
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[35..39].copy_from_slice(&[0x00, 0x00, 0x0d, 0x00]);
    assert!(auxiliary_profile_record(&payload, 0));

    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[35..39].copy_from_slice(&[0x00, 0x00, 0x0c, 0x00]);
    payload[56..58].copy_from_slice(&2u16.to_le_bytes());
    payload[58..60].copy_from_slice(&3u16.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..80].copy_from_slice(&[0x00, 0x00, 0x02, 0x00, 0, 0, 0, 0]);
    payload[84..84 + LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert!(marker_is_selected_construction_line(&payload, 0));
    assert!(!auxiliary_profile_record(&payload, 0));
}

#[test]
fn compact_curve_with_relation_endpoint_is_a_display_carrier() {
    let mut payload = vec![0; 84 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[29..31].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..60].copy_from_slice(&[2, 0, 4, 0]);
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[84..].copy_from_slice(SKETCH_MARKER);

    let marker = |id: &str, object_index, kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: 0,
        object_index,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let curve = marker("curve", Some(1), SketchInputKind::LineOrCircle, None);
    let relation = marker(
        "relation",
        Some(3),
        SketchInputKind::Relation(SketchRelationKind::Distance),
        None,
    );
    let duplicate_curve = marker(
        "duplicate-curve",
        Some(3),
        SketchInputKind::LineOrCircle,
        None,
    );
    let point = marker("point", Some(5), SketchInputKind::Point, Some([1.0, 0.0]));
    let markers = [&curve, &relation, &duplicate_curve, &point];

    assert!(relation_reference_curve_record(&payload, &curve, &markers));

    let first_point = marker(
        "first-point",
        Some(3),
        SketchInputKind::Point,
        Some([0.0, 0.0]),
    );
    let second_point = marker(
        "second-point",
        Some(5),
        SketchInputKind::Point,
        Some([1.0, 0.0]),
    );
    let markers = [&curve, &first_point, &second_point];
    assert!(!relation_reference_curve_record(&payload, &curve, &markers));
}

#[test]
fn current_compact_curve_resolves_complete_marker_roster_endpoints() {
    let mut payload = vec![0; 84 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[29..31].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..60].copy_from_slice(&[2, 0, 3, 0]);
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[84..].copy_from_slice(SKETCH_MARKER);

    let marker = |id: &str, offset, object_index, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset,
        object_index,
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let curve = SketchInputEntity {
        kind: SketchInputKind::LineOrCircle,
        coordinates_m: None,
        ..marker("curve", 0, Some(1), None)
    };
    let first = marker("first", 100, Some(99), Some([0.0, 0.0]));
    let second = marker("second", 200, Some(100), Some([1.0, 0.0]));
    let markers = [&curve, &first, &second];

    for prefix in [SKETCH_MARKER, LEGACY_EXTENDED_SKETCH_MARKER] {
        payload[..prefix.len()].copy_from_slice(prefix);
        payload[84..84 + prefix.len()].copy_from_slice(prefix);
        let pair = compact_complete_marker_roster_pair(&payload, &curve, &markers, true)
            .expect("complete marker roster endpoints");
        assert_eq!(
            [pair[0].id.as_str(), pair[1].id.as_str()],
            ["first", "second"]
        );
    }

    let mut terminal = payload[..84].to_vec();
    terminal[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    let pair = compact_complete_marker_roster_pair(&terminal, &curve, &markers, true)
        .expect("terminal complete marker roster endpoints");
    assert_eq!(
        [pair[0].id.as_str(), pair[1].id.as_str()],
        ["first", "second"]
    );
    assert_eq!(
        roster_curve_endpoint_markers(&terminal, &curve, &markers)
            .into_iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );

    assert_eq!(
        roster_curve_endpoint_markers(&payload, &curve, &markers)
            .into_iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );

    let relation = SketchInputEntity {
        id: "relation".into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: 200,
        object_index: Some(100),
        local_id: None,
        kind: SketchInputKind::Relation(SketchRelationKind::Distance),
        state_value: Some(1.0),
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };
    let markers = [&curve, &first, &relation];
    assert!(relation_reference_curve_record(&payload, &curve, &markers));
}

#[test]
fn compact_complete_marker_roster_rejects_conflicting_index_bases() {
    let mut payload = vec![0; 84 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..60].copy_from_slice(&[2, 0, 3, 0]);
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[84..].copy_from_slice(SKETCH_MARKER);

    let marker = |id: &str, offset, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset,
        object_index: None,
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let curve = SketchInputEntity {
        kind: SketchInputKind::LineOrCircle,
        coordinates_m: None,
        ..marker("curve", 0, None)
    };
    let first = marker("first", 100, Some([0.0, 0.0]));
    let second = marker("second", 200, Some([1.0, 0.0]));
    let third = marker("third", 300, Some([2.0, 0.0]));
    let markers = [&curve, &first, &second, &third];

    assert_eq!(
        compact_complete_marker_roster_pair(&payload, &curve, &markers, true)
            .map(|pair| [pair[0].id.as_str(), pair[1].id.as_str()]),
        Some(["first", "second"])
    );
    assert_eq!(
        compact_complete_marker_roster_pair(&payload, &curve, &markers, false)
            .map(|pair| [pair[0].id.as_str(), pair[1].id.as_str()]),
        Some(["second", "third"])
    );
    assert!(super::compact_complete_marker_roster_endpoints(&payload, &curve, &markers).is_empty());
}

#[test]
fn current_referenced_compact_roster_prefers_complete_roster() {
    let mut payload = vec![0; 84 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&2u16.to_le_bytes());
    payload[58..60].copy_from_slice(&4u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].fill(0);
    payload[76..80].copy_from_slice(&13u32.to_le_bytes());
    payload[80..84].copy_from_slice(&7u32.to_le_bytes());
    payload[84..].copy_from_slice(SKETCH_MARKER);

    let marker = |id: &str, offset, kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset,
        object_index: None,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let curve = marker("curve", 0, SketchInputKind::Arc, None);
    let first = marker("first", 10, SketchInputKind::Point, Some([1.0, 0.0]));
    let relation = marker(
        "relation",
        20,
        SketchInputKind::Relation(SketchRelationKind::Horizontal),
        None,
    );
    let second = marker("second", 30, SketchInputKind::Point, Some([0.0, 1.0]));
    let third = marker("third", 40, SketchInputKind::Point, Some([2.0, 0.0]));
    let fourth = marker("fourth", 50, SketchInputKind::Point, Some([0.0, 2.0]));
    let fifth = marker("fifth", 60, SketchInputKind::Point, Some([3.0, 0.0]));
    let markers = [&curve, &first, &relation, &second, &third, &fourth, &fifth];

    assert!(current_referenced_compact_curve_uses_marker_roster(
        &payload, 0
    ));
    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &curve, &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
}

#[test]
fn current_referenced_compact_roster_falls_back_when_complete_slot_is_not_a_point() {
    let mut payload = vec![0; 84 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&1u16.to_le_bytes());
    payload[58..60].copy_from_slice(&2u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].fill(0);
    payload[76..80].copy_from_slice(&13u32.to_le_bytes());
    payload[80..84].copy_from_slice(&7u32.to_le_bytes());
    payload[84..].copy_from_slice(SKETCH_MARKER);

    let marker = |id: &str, offset, kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset,
        object_index: None,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let curve = marker("curve", 0, SketchInputKind::LineOrCircle, None);
    let relation = marker(
        "relation",
        10,
        SketchInputKind::Relation(SketchRelationKind::Horizontal),
        None,
    );
    let first = marker("first", 20, SketchInputKind::Point, Some([0.0, 0.0]));
    let second = marker("second", 30, SketchInputKind::Point, Some([1.0, 0.0]));
    let third = marker("third", 40, SketchInputKind::Point, Some([2.0, 0.0]));
    let markers = [&curve, &relation, &first, &second, &third];

    assert!(current_referenced_compact_curve_uses_marker_roster(
        &payload, 0
    ));
    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &curve, &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["second", "third"]
    );
}

#[test]
fn current_compact_curve_falls_back_to_raw_object_indices() {
    let mut payload = vec![0; 84 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[29..31].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..60].copy_from_slice(&[15, 0, 6, 0]);
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[84..].copy_from_slice(SKETCH_MARKER);

    assert_eq!(
        compact_indexed_curve_raw_endpoint_indices(&payload, 0),
        Some([15, 6])
    );
    let marker = |id: &str, object_index, offset, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset,
        object_index,
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let first = marker("first", Some(15), 1, Some([0.0, 0.0]));
    let second = marker("second", Some(6), 2, Some([1.0, 0.0]));
    let curve = SketchInputEntity {
        id: "curve".into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: 0,
        object_index: Some(1),
        local_id: None,
        kind: SketchInputKind::LineOrCircle,
        state_value: Some(1.0),
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };
    let markers = [&first, &second];
    let endpoints = roster_curve_endpoint_markers(&payload, &curve, &markers);
    assert_eq!(
        endpoints
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
}

#[test]
fn overlapping_endpoint_index_bases_use_the_marker_roster() {
    let mut payload = vec![0; 84 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&0u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x05, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&1u16.to_le_bytes());
    payload[58..60].copy_from_slice(&2u16.to_le_bytes());
    payload[60..64].copy_from_slice(&0u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&0u32.to_le_bytes());
    payload[76..80].copy_from_slice(&1u32.to_le_bytes());
    payload[80..84].copy_from_slice(&2u32.to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        direct_indexed_curve_endpoint_indices(&payload, 0),
        Some([1, 2])
    );
    assert_eq!(
        legacy_state_five_curve_endpoint_indices(&payload, 0),
        Some([2, 3])
    );
    let candidates = super::curve_endpoint_index_candidates(&payload, 0);
    assert!(candidates.contains(&[1, 2]));
    assert!(candidates.contains(&[2, 3]));

    let marker = |id: &str, object_index, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: 0,
        object_index,
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let first = marker("first", Some(1), Some([0.0, 0.0]));
    let second = marker("second", Some(2), Some([1.0, 0.0]));
    let coincident = marker("coincident", Some(3), Some([0.0, 0.0]));
    let curve = SketchInputEntity {
        id: "curve".into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: 0,
        object_index: None,
        local_id: None,
        kind: SketchInputKind::LineOrCircle,
        state_value: Some(1.0),
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };
    let markers = [&first, &second];
    assert_eq!(
        roster_curve_endpoint_markers(&payload, &curve, &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );

    let pairs = vec![vec![&first, &second], vec![&second, &coincident]];
    let (selected, ambiguous) = super::resolve_indexed_marker_candidates(pairs.clone());
    assert!(!ambiguous);
    assert_eq!(
        selected
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
    let (reversed, ambiguous) = super::resolve_indexed_marker_candidates(pairs.into_iter().rev());
    assert!(!ambiguous);
    assert_eq!(
        reversed
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
    let different = marker("different", Some(3), Some([2.0, 0.0]));
    let (_, ambiguous) = super::resolve_indexed_marker_candidates([
        vec![&first, &second],
        vec![&second, &different],
    ]);
    assert!(ambiguous);
}
