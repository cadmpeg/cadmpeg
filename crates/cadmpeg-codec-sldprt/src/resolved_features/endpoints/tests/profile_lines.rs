//! Construction-line and marker-84/104 profile-line tests.
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
fn legacy_compact_84_construction_line_uses_direct_point_ids() {
    let mut payload = vec![0; 84 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[27..29].copy_from_slice(&2u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&7u16.to_le_bytes());
    payload[58..60].copy_from_slice(&10u16.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&[0x00, 0x00, 0x01, 0x00]);
    payload[80..84].copy_from_slice(&4u32.to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_SKETCH_MARKER);

    let entity = |id: &str, object_index, coordinates_m, kind| SketchInputEntity {
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
    let curve = entity("curve", Some(1), None, SketchInputKind::LineOrCircle);
    let impostor = entity(
        "impostor",
        Some(7),
        Some([2.0, 0.0]),
        SketchInputKind::LineOrCircle,
    );
    let first = entity("first", Some(7), Some([0.0, 0.0]), SketchInputKind::Point);
    let second = entity("second", Some(10), Some([1.0, 0.0]), SketchInputKind::Point);
    let markers = [&curve, &impostor, &first, &second];

    assert_eq!(
        legacy_compact_84_construction_line_endpoint_indices(&payload, 0),
        Some([7, 10])
    );
    assert!(marker_is_selected_construction_line(&payload, 0));
    assert_eq!(
        roster_curve_endpoint_markers(&payload, &curve, &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );

    payload[5..13].copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0x04, 0x00, 0xff, 0xff]);
    assert_eq!(
        legacy_compact_84_construction_line_endpoint_indices(&payload, 0),
        Some([7, 10])
    );

    payload[21] = 1;
    assert_eq!(
        legacy_compact_84_construction_line_endpoint_indices(&payload, 0),
        None
    );
    payload[21] = 0;
    payload[39] = 1;
    assert_eq!(
        legacy_compact_84_construction_line_endpoint_indices(&payload, 0),
        None
    );
    payload[39] = 0;

    payload[72..76].fill(0);
    payload[76..80].copy_from_slice(&4u32.to_le_bytes());
    payload[80..84].copy_from_slice(&4u32.to_le_bytes());
    assert_eq!(
        legacy_compact_84_construction_line_endpoint_indices(&payload, 0),
        Some([7, 10])
    );

    payload[80..84].copy_from_slice(&5u32.to_le_bytes());
    assert_eq!(
        legacy_compact_84_construction_line_endpoint_indices(&payload, 0),
        None
    );
    assert!(!marker_is_selected_construction_line(&payload, 0));
}

#[test]
fn legacy_compact_84_curves_use_complete_coordinate_roster() {
    let entity = |id: &str, offset, coordinates_m, kind| SketchInputEntity {
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
    let curve = entity("curve", 0, None, SketchInputKind::LineOrCircle);
    let first = entity("first", 10, Some([0.0, 0.0]), SketchInputKind::Point);
    let relation = entity(
        "relation",
        20,
        Some([0.5, 0.0]),
        SketchInputKind::Relation(SketchRelationKind::Horizontal),
    );
    let second = entity("second", 30, Some([1.0, 0.0]), SketchInputKind::Point);
    let third = entity("third", 40, Some([1.0, 1.0]), SketchInputKind::Point);
    let coordinate_curve = entity(
        "coordinate-curve",
        50,
        Some([0.0, 1.0]),
        SketchInputKind::LineOrCircle,
    );
    let fourth = entity("fourth", 60, Some([2.0, 1.0]), SketchInputKind::Point);
    let markers = [
        &curve,
        &first,
        &relation,
        &second,
        &third,
        &coordinate_curve,
        &fourth,
    ];

    let payload = |native_kind: u32,
                   role: u16,
                   selector: u8,
                   endpoints: [u16; 2],
                   trailer_state: [u8; 4],
                   identities: [u32; 2]| {
        let mut payload = vec![0; 84 + LEGACY_SKETCH_MARKER.len()];
        payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
        payload[5..13].fill(0xff);
        payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[17..21].copy_from_slice(&native_kind.to_le_bytes());
        payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
        payload[27..29].copy_from_slice(&role.to_le_bytes());
        payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, selector, 0x00]);
        payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[56..58].copy_from_slice(&endpoints[0].to_le_bytes());
        payload[58..60].copy_from_slice(&endpoints[1].to_le_bytes());
        payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
        payload[72..76].copy_from_slice(&trailer_state);
        payload[76..80].copy_from_slice(&identities[0].to_le_bytes());
        payload[80..84].copy_from_slice(&identities[1].to_le_bytes());
        payload[84..].copy_from_slice(LEGACY_SKETCH_MARKER);
        payload
    };

    let profile_payload = payload(0u32, 1u16, 5, [2u16, 3], [0, 0, 2, 0], [2, 3]);
    assert_eq!(
        legacy_compact_84_coordinate_roster_endpoint_indices(&profile_payload, 0),
        Some([2, 3])
    );
    assert_eq!(
        roster_curve_endpoint_markers(&profile_payload, &curve, &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["second", "third"]
    );

    let code_one_payload = payload(1u32, 1u16, 0x0c, [2u16, 4], [0, 0, 0, 0], [0, 10]);
    assert_eq!(
        legacy_compact_84_coordinate_roster_endpoint_indices(&code_one_payload, 0),
        Some([2, 4])
    );
    assert_eq!(
        roster_curve_endpoint_markers(&code_one_payload, &curve, &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["second", "coordinate-curve"]
    );
    let mut alternate_header = code_one_payload.clone();
    alternate_header[5..13].copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0x04, 0x00, 0xff, 0xff]);
    assert_eq!(
        legacy_compact_84_coordinate_roster_endpoint_indices(&alternate_header, 0),
        Some([2, 4])
    );

    let construction_payload = payload(2u32, 2u16, 0x0c, [4u16, 5], [0, 0, 1, 0], [0, 4]);
    assert_eq!(
        legacy_compact_84_coordinate_roster_endpoint_indices(&construction_payload, 0),
        Some([4, 5])
    );
    assert!(marker_is_selected_construction_line(
        &construction_payload,
        0
    ));
    assert_eq!(
        roster_curve_endpoint_markers(&construction_payload, &curve, &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["coordinate-curve", "fourth"]
    );

    let construction_zero_payload = payload(2u32, 2u16, 0x0c, [0u16, 5], [0, 0, 1, 0], [0, 4]);
    assert_eq!(
        legacy_compact_84_coordinate_roster_endpoint_indices(&construction_zero_payload, 0),
        Some([0, 5])
    );
    assert!(marker_is_selected_construction_line(
        &construction_zero_payload,
        0
    ));
    assert_eq!(
        roster_curve_endpoint_markers(&construction_zero_payload, &curve, &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "fourth"]
    );

    let mut rejected = profile_payload.clone();
    rejected[56..58].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        legacy_compact_84_coordinate_roster_endpoint_indices(&rejected, 0),
        Some([1, 3])
    );
    assert!(roster_curve_endpoint_markers(&rejected, &curve, &markers).is_empty());
    rejected[58..60].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        legacy_compact_84_coordinate_roster_endpoint_indices(&rejected, 0),
        None
    );
    rejected[56..58].copy_from_slice(&2u16.to_le_bytes());
    rejected[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    assert_eq!(
        legacy_compact_84_coordinate_roster_endpoint_indices(&rejected, 0),
        None
    );
}

#[test]
fn compact_legacy_wide_selected_axis_indexes_the_coordinate_roster() {
    let mut payload = vec![0; 92 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[27..29].copy_from_slice(&2u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0d, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&7u16.to_le_bytes());
    payload[66..68].copy_from_slice(&8u16.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[92..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        legacy_coordinate_roster_selected_axis_endpoint_indices(&payload, 0),
        Some([8, 9])
    );
    assert!(marker_is_selected_construction_line(&payload, 0));
    payload[35..39].copy_from_slice(&[0x00, 0x00, 0x0c, 0x00]);
    assert_eq!(
        legacy_coordinate_roster_selected_axis_endpoint_indices(&payload, 0),
        None
    );
}

#[test]
fn extended_profile_roster_construction_line_indexes_coordinate_markers() {
    let mut payload = vec![0; 92 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&3u16.to_le_bytes());
    payload[66..68].copy_from_slice(&4u16.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[84..88].copy_from_slice(&7u32.to_le_bytes());
    payload[88..92].copy_from_slice(&7u32.to_le_bytes());
    payload[92..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    assert_eq!(
        extended_profile_roster_construction_line_endpoint_indices(&payload, 0),
        Some([4, 5])
    );
    assert_eq!(
        super::coordinate_roster_endpoint_offset(&payload, 0),
        Some(64)
    );
    assert!(marker_is_selected_construction_line(&payload, 0));

    payload[88..92].copy_from_slice(&8u32.to_le_bytes());
    assert_eq!(
        extended_profile_roster_construction_line_endpoint_indices(&payload, 0),
        None
    );
}

#[test]
fn extended_compact_construction_line_distinguishes_direct_ids_from_roster_indices() {
    let mut payload = vec![0; 84 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x02, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..64].copy_from_slice(&[0x00, 0x00, 0x01, 0x00, 0, 0, 0, 0]);
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&[0x00, 0x00, 0x01, 0x00]);
    payload[76..80].copy_from_slice(&8u32.to_le_bytes());
    payload[80..84].copy_from_slice(&2u32.to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    assert_eq!(
        extended_compact_84_construction_line_endpoint_indices(&payload, 0),
        Some([8, 2])
    );
    payload[72..76].fill(0);
    assert_eq!(
        extended_compact_84_construction_line_endpoint_indices(&payload, 0),
        Some([8, 2])
    );
    payload[56..60].copy_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    let entity = |id: &str, offset, coordinates_m: Option<[f64; 2]>| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 0,
        offset,
        object_index: None,
        local_id: None,
        kind: if coordinates_m.is_some() {
            SketchInputKind::Point
        } else {
            SketchInputKind::LineOrCircle
        },
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let curve = entity("curve", 0, None);
    let first = entity("first", 10, Some([1.0, 2.0]));
    let second = entity("second", 20, Some([3.0, 4.0]));
    let markers = [&curve, &first, &second];
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(56));
    assert_eq!(
        roster_curve_endpoint_markers(&payload, &curve, &markers)
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["second", "first"]
    );

    payload[56..64].copy_from_slice(&[0x00, 0x00, 0x01, 0x00, 0, 0, 0, 0]);
    payload[80..84].copy_from_slice(&8u32.to_le_bytes());
    assert_eq!(
        extended_compact_84_construction_line_endpoint_indices(&payload, 0),
        None
    );
}

#[test]
fn extended_shifted_construction_line_indexes_coordinate_roster() {
    let mut payload = vec![0; 84 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x02, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&5u16.to_le_bytes());
    payload[58..60].copy_from_slice(&9u16.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
    payload[80..84].copy_from_slice(&2u32.to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    assert_eq!(
        extended_shifted_construction_line_endpoint_indices(&payload, 0),
        Some([5, 9])
    );
    assert!(marker_is_selected_construction_line(&payload, 0));

    payload.truncate(84);
    payload[72..76].fill(0);
    payload[80..84].fill(0);
    assert_eq!(
        extended_shifted_construction_line_endpoint_indices(&payload, 0),
        Some([5, 9])
    );

    payload[58..60].copy_from_slice(&5u16.to_le_bytes());
    assert_eq!(
        extended_shifted_construction_line_endpoint_indices(&payload, 0),
        None
    );

    let entity = |id: &str, offset: u64, coordinates_m: Option<[f64; 2]>| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset,
        object_index: None,
        local_id: None,
        kind: if coordinates_m.is_some() {
            SketchInputKind::Point
        } else {
            SketchInputKind::Relation(SketchRelationKind::Horizontal)
        },
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let points = (0..10)
        .map(|index| {
            entity(
                &format!("marker-{index}"),
                10 + index * 10,
                matches!(index, 4 | 8).then_some([index as f64, 0.0]),
            )
        })
        .collect::<Vec<_>>();
    let curve = SketchInputEntity {
        id: "curve".into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: 110,
        object_index: None,
        local_id: None,
        kind: SketchInputKind::LineOrCircle,
        state_value: Some(1.0),
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };
    let markers = points
        .iter()
        .chain(std::iter::once(&curve))
        .collect::<Vec<_>>();
    let markers_by_id = markers
        .iter()
        .map(|marker| (marker.id.as_str(), *marker))
        .collect::<HashMap<_, _>>();
    payload[58..60].copy_from_slice(&9u16.to_le_bytes());
    let record = payload[..84].to_vec();
    payload.resize(110 + 84, 0);
    payload[110..110 + 84].copy_from_slice(&record);
    assert_eq!(
        marker_curve_endpoint_markers(&payload, &curve, &markers_by_id, &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["marker-4", "marker-8"]
    );
}

#[test]
fn compact_legacy_142_profile_curve_matches_unique_coordinate_endpoints() {
    let mut payload = vec![0; 142 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&(-1.0f32).to_le_bytes());
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&[0x12, 0x00]);
    payload[66..74].copy_from_slice(&10.0f64.to_le_bytes());
    payload[74..82].copy_from_slice(&10.0f64.to_le_bytes());
    payload[82..86].copy_from_slice(&11u32.to_le_bytes());
    payload[92..96].copy_from_slice(&3u32.to_le_bytes());
    payload[96..104].copy_from_slice(&0.0f64.to_le_bytes());
    payload[104..112].copy_from_slice(&0.0f64.to_le_bytes());
    payload[112..120].copy_from_slice(&1.0f64.to_le_bytes());
    payload[120..128].copy_from_slice(&0.0f64.to_le_bytes());
    payload[138..142].copy_from_slice(&17u32.to_le_bytes());
    payload[142..].copy_from_slice(LEGACY_SKETCH_MARKER);

    let entity = |id: &str, offset, kind, coordinates_m| SketchInputEntity {
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
    let curve = entity("curve", 0, SketchInputKind::LineOrCircle, None);
    let first = entity("first", 100, SketchInputKind::Point, Some([0.0, 0.0]));
    let second = entity("second", 200, SketchInputKind::Point, Some([1.0, 0.0]));
    let markers = [&curve, &first, &second];
    let markers_by_id = markers
        .iter()
        .map(|marker| (marker.id.as_str(), *marker))
        .collect::<HashMap<_, _>>();
    assert_eq!(
        marker_curve_endpoint_markers(&payload, &curve, &markers_by_id, &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );

    let duplicate = entity("duplicate", 300, SketchInputKind::Point, Some([0.0, 0.0]));
    let ambiguous_markers = [&curve, &first, &second, &duplicate];
    let ambiguous_by_id = ambiguous_markers
        .iter()
        .map(|marker| (marker.id.as_str(), *marker))
        .collect::<HashMap<_, _>>();
    assert!(
        marker_curve_endpoint_markers(&payload, &curve, &ambiguous_by_id, &ambiguous_markers,)
            .is_empty()
    );
}

#[test]
fn extended_compact_profile_line_uses_complete_feature_roster_fallback() {
    let mut payload = vec![0; 84 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&1u16.to_le_bytes());
    payload[58..60].copy_from_slice(&3u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    let entity = |id: &str, offset, kind, object_index, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 10_000,
        offset,
        object_index,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let curve = entity("curve", 0, SketchInputKind::LineOrCircle, None, None);
    let first = entity(
        "first",
        10,
        SketchInputKind::Point,
        Some(7),
        Some([1.0, 2.0]),
    );
    let relation = entity(
        "relation",
        20,
        SketchInputKind::Relation(SketchRelationKind::Horizontal),
        None,
        None,
    );
    let second = entity(
        "second",
        30,
        SketchInputKind::Point,
        Some(9),
        Some([3.0, 4.0]),
    );
    let markers = [&curve, &first, &relation, &second];

    assert_eq!(
        extended_compact_endpoint_markers(&payload, &curve, &markers)
            .into_iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
}

#[test]
fn extended_compact_84_profile_roster_uses_one_based_point_objects() {
    let mut payload = vec![0; 84 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x00, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&2u16.to_le_bytes());
    payload[58..60].copy_from_slice(&1u16.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
    payload[76..80].copy_from_slice(&10u32.to_le_bytes());
    payload[80..84].copy_from_slice(&6u32.to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    let entity = |id: &str, offset, kind, object_index, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset,
        object_index,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let curve = entity("curve", 0, SketchInputKind::LineOrCircle, Some(5), None);
    let relation = entity(
        "relation",
        10,
        SketchInputKind::Relation(SketchRelationKind::Distance),
        Some(1),
        None,
    );
    let first = entity(
        "first",
        20,
        SketchInputKind::Point,
        Some(2),
        Some([1.0, 2.0]),
    );
    let second = entity(
        "second",
        30,
        SketchInputKind::Point,
        Some(3),
        Some([3.0, 4.0]),
    );
    let markers = [&curve, &relation, &first, &second];

    assert_eq!(
        super::extended_compact_indexed_curve_endpoint_indices(&payload, 0),
        Some([3, 2])
    );
    assert_eq!(
        roster_curve_endpoint_markers(&payload, &curve, &markers)
            .into_iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["second", "first"]
    );
}

#[test]
fn extended_compact_96_selected_axis_uses_one_based_object_indices() {
    let mut payload = vec![0; 96 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&3u16.to_le_bytes());
    payload[58..60].copy_from_slice(&4u16.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[82..84].copy_from_slice(&5u16.to_le_bytes());
    payload[88..92].copy_from_slice(&5u32.to_le_bytes());
    payload[92..96].copy_from_slice(&1u32.to_le_bytes());
    payload[96..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    assert_eq!(
        extended_compact_96_selected_axis_endpoint_indices(&payload, 0),
        Some([4, 5])
    );
    assert!(marker_is_selected_construction_line(&payload, 0));

    payload[88..92].copy_from_slice(&6u32.to_le_bytes());
    assert_eq!(
        extended_compact_96_selected_axis_endpoint_indices(&payload, 0),
        None
    );
}

#[test]
fn extended_marker84_line_uses_state_selected_point_roster_base() {
    let mut payload = vec![0; 84 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x02, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&1u16.to_le_bytes());
    payload[58..60].copy_from_slice(&3u16.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&[0x00, 0x00, 0x01, 0x00]);
    payload[80..84].copy_from_slice(&2u32.to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    let entity = |id: &str, offset, coordinates_m, kind| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
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
    let curve = entity("curve", 0, None, SketchInputKind::LineOrCircle);
    let points = [
        entity("first", 10, Some([0.0, 0.0]), SketchInputKind::Point),
        entity("second", 20, Some([1.0, 0.0]), SketchInputKind::Point),
        entity("third", 30, Some([1.0, 1.0]), SketchInputKind::Point),
        entity("fourth", 40, Some([0.0, 1.0]), SketchInputKind::Point),
    ];
    let markers = std::iter::once(&curve)
        .chain(points.iter())
        .collect::<Vec<_>>();

    assert!(super::extended_marker84_line_uses_point_roster(&payload, 0));
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(56));
    let endpoints = roster_curve_endpoint_markers(&payload, &curve, &markers);
    assert_eq!(
        endpoints
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "third"]
    );
    payload[56..58].copy_from_slice(&2u16.to_le_bytes());
    payload[58..60].copy_from_slice(&4u16.to_le_bytes());
    payload[72..76].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
    payload[80..84].fill(0xff);
    assert!(!super::extended_marker84_line_uses_point_roster(
        &payload, 0
    ));
    let endpoints = roster_curve_endpoint_markers(&payload, &curve, &markers);
    assert_eq!(
        endpoints
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "third"]
    );
    payload[80..84].fill(0);
    assert!(!super::extended_marker84_line_uses_point_roster(
        &payload, 0
    ));
    assert!(roster_curve_endpoint_markers(&payload, &curve, &markers).is_empty());
    payload[80..84].copy_from_slice(&2u32.to_le_bytes());
    payload[56..58].copy_from_slice(&1u16.to_le_bytes());
    payload[58..60].copy_from_slice(&3u16.to_le_bytes());
    payload[72..76].fill(0);
    assert!(super::extended_marker84_line_uses_point_roster(&payload, 0));
    let endpoints = roster_curve_endpoint_markers(&payload, &curve, &markers);
    assert_eq!(
        endpoints
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["second", "fourth"]
    );
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[29..31].copy_from_slice(&1u16.to_le_bytes());
    payload[35..39].copy_from_slice(&[0x00, 0x00, 0x04, 0x00]);
    assert!(super::extended_marker84_line_uses_point_roster(&payload, 0));
    payload[72..76].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
    payload[56..58].copy_from_slice(&4u16.to_le_bytes());
    payload[58..60].copy_from_slice(&1u16.to_le_bytes());
    let endpoints = roster_curve_endpoint_markers(&payload, &curve, &markers);
    assert_eq!(
        endpoints
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["fourth", "first"]
    );
    payload[72..76].fill(0);
    payload[56..58].copy_from_slice(&1u16.to_le_bytes());
    payload[58..60].copy_from_slice(&3u16.to_le_bytes());
    let endpoints = roster_curve_endpoint_markers(&payload, &curve, &markers);
    assert_eq!(
        endpoints
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["second", "fourth"]
    );
    payload[27..29].copy_from_slice(&2u16.to_le_bytes());
    assert!(!super::extended_marker84_line_uses_point_roster(
        &payload, 0
    ));

    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[56..58].fill(0);
    assert!(super::extended_marker84_line_uses_point_roster(&payload, 0));
    let endpoints = roster_curve_endpoint_markers(&payload, &curve, &markers);
    assert_eq!(
        endpoints
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "fourth"]
    );
    payload[56..58].fill(0xff);
    assert!(!super::extended_marker84_line_uses_point_roster(
        &payload, 0
    ));
}

#[test]
fn legacy_compact_marker84_profile_line_uses_zero_based_point_roster() {
    let mut payload = vec![0; 84 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..41].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x08, 0x00, 0x58, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&0u16.to_le_bytes());
    payload[58..60].copy_from_slice(&2u16.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
    payload[80..84].copy_from_slice(&7u32.to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_SKETCH_MARKER);
    let entity = |id: &str, offset, coordinates_m: Option<[f64; 2]>| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 0,
        offset,
        object_index: None,
        local_id: None,
        kind: if coordinates_m.is_some() {
            SketchInputKind::Point
        } else {
            SketchInputKind::LineOrCircle
        },
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let curve = entity("curve", 0, None);
    let first = entity("first", 10, Some([0.0, 0.0]));
    let second = entity("second", 20, Some([1.0, 0.0]));
    let third = entity("third", 30, Some([1.0, 1.0]));
    let markers = [&curve, &first, &second, &third];

    assert!(super::legacy_compact_84_profile_line_uses_point_roster(
        &payload, 0
    ));
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(56));
    assert_eq!(
        roster_curve_endpoint_markers(&payload, &curve, &markers)
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "third"]
    );

    payload[74..76].fill(0);
    assert!(super::legacy_compact_84_profile_line_uses_point_roster(
        &payload, 0
    ));
    payload[80..84].fill(0);
    assert!(!super::legacy_compact_84_profile_line_uses_point_roster(
        &payload, 0
    ));
    assert!(roster_curve_endpoint_markers(&payload, &curve, &markers).is_empty());
}

#[test]
fn extended_compact_marker84_profile_line_uses_zero_based_geometry_roster() {
    let mut payload = vec![0; 84 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..41].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x08, 0x00, 0x58, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&0u16.to_le_bytes());
    payload[58..60].copy_from_slice(&2u16.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
    payload[80..84].copy_from_slice(&7u32.to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    let entity = |id: &str, offset, coordinates_m: Option<[f64; 2]>| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 0,
        offset,
        object_index: None,
        local_id: None,
        kind: if coordinates_m.is_some() {
            SketchInputKind::Point
        } else {
            SketchInputKind::LineOrCircle
        },
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let curve = entity("curve", 0, None);
    let first = entity("first", 10, Some([0.0, 0.0]));
    let second = entity("second", 20, Some([1.0, 0.0]));
    let third = entity("third", 30, Some([1.0, 1.0]));
    let markers = [&curve, &first, &second, &third];

    assert!(super::extended_compact_84_profile_line_uses_point_roster(
        &payload, 0
    ));
    assert!(!marker_is_selected_construction_line(&payload, 0));
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(56));
    assert_eq!(
        roster_curve_endpoint_markers(&payload, &curve, &markers)
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "third"]
    );

    payload[39] = 0x40;
    assert!(super::extended_compact_84_profile_line_uses_point_roster(
        &payload, 0
    ));
    payload[39] = 0x58;
    payload[80..84].fill(0);
    assert!(!super::extended_compact_84_profile_line_uses_point_roster(
        &payload, 0
    ));
    assert!(roster_curve_endpoint_markers(&payload, &curve, &markers).is_empty());
}

#[test]
fn legacy_referenced_wide_arc_indexes_center_and_endpoints() {
    let mut payload = vec![0; 112 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&1u16.to_le_bytes());
    payload[66..68].copy_from_slice(&2u16.to_le_bytes());
    payload[68..72].copy_from_slice(&1u32.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[80..84].copy_from_slice(&1i32.to_le_bytes());
    for relative in [86, 90, 94, 98] {
        payload[relative..relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[104..108].copy_from_slice(&2u32.to_le_bytes());
    payload[108..112].copy_from_slice(&2u32.to_le_bytes());
    payload[112..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        legacy_referenced_wide_arc_endpoint_indices(&payload, 0),
        Some([2, 3])
    );
    assert_eq!(
        super::coordinate_roster_endpoint_offset(&payload, 0),
        Some(64)
    );
    assert!(super::indexed_arc_uses_coordinate_center(&payload, 0));
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Arc
    );

    payload[108..112].copy_from_slice(&3u32.to_le_bytes());
    assert_eq!(
        legacy_referenced_wide_arc_endpoint_indices(&payload, 0),
        None
    );
}

#[test]
fn current_compact_104_line_indexes_coordinate_markers() {
    let mut payload = vec![0; 104 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&7u16.to_le_bytes());
    payload[66..68].copy_from_slice(&2u16.to_le_bytes());
    payload[68..72].copy_from_slice(&1u32.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[88..92].copy_from_slice(&[0x00, 0x00, 0x01, 0x00]);
    payload[100..104].copy_from_slice(&1u32.to_le_bytes());
    payload[104..].copy_from_slice(SKETCH_MARKER);

    assert_eq!(
        current_compact_104_indexed_line_endpoint_indices(&payload, 0),
        Some([8, 3])
    );
    assert_eq!(
        super::coordinate_roster_endpoint_offset(&payload, 0),
        Some(64)
    );

    payload[100..104].fill(0);
    assert_eq!(
        current_compact_104_indexed_line_endpoint_indices(&payload, 0),
        None
    );
}

#[test]
fn current_compact_84_line_falls_back_to_zero_based_point_roster() {
    let mut payload = vec![0; 84 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..31].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[58..60].copy_from_slice(&1u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[80..84].copy_from_slice(&2u32.to_le_bytes());
    payload[84..].copy_from_slice(SKETCH_MARKER);

    let entity =
        |id: &str, offset, object_index, coordinates_m: Option<[f64; 2]>| SketchInputEntity {
            id: id.into(),
            parent: "lane".into(),
            feature_ref: Some("sketch".into()),
            ordinal: 0,
            offset,
            object_index,
            local_id: None,
            kind: if coordinates_m.is_some() {
                SketchInputKind::Point
            } else {
                SketchInputKind::LineOrCircle
            },
            state_value: Some(1.0),
            coordinates_m,
            links: Vec::new(),
            link_selector: None,
        };
    let curve = entity("curve", 0, Some(1), None);
    let first = entity("first", 10, Some(10), Some([0.0, 0.0]));
    let second = entity("second", 20, Some(11), Some([1.0, 0.0]));
    let markers = [&curve, &first, &second];

    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(56));
    assert_eq!(
        roster_curve_endpoint_markers(&payload, &curve, &markers)
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );

    payload.resize(96 + SKETCH_MARKER.len(), 0);
    payload[72..82].fill(0);
    payload[82..84].copy_from_slice(&2u16.to_le_bytes());
    payload[84..88].fill(0);
    payload[88..92].copy_from_slice(&2u32.to_le_bytes());
    payload[92..96].copy_from_slice(&1u32.to_le_bytes());
    payload[96..].copy_from_slice(SKETCH_MARKER);
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(56));
    assert_eq!(
        roster_curve_endpoint_markers(&payload, &curve, &markers)
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );

    payload.resize(104 + SKETCH_MARKER.len(), 0);
    payload[72..76].copy_from_slice(&1i32.to_le_bytes());
    payload[76..78].copy_from_slice(&2u16.to_le_bytes());
    for relative in [78, 82, 86, 90] {
        payload[relative..relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[94..96].fill(0);
    payload[96..104].copy_from_slice(&[2, 0, 0, 0, 3, 0, 0, 0]);
    payload[104..].copy_from_slice(SKETCH_MARKER);
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(56));
    assert_eq!(
        roster_curve_endpoint_markers(&payload, &curve, &markers)
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
}

#[test]
fn current_compact_104_profile_record_is_a_line() {
    let mut payload = vec![0; 104 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&4u16.to_le_bytes());
    payload[58..60].copy_from_slice(&6u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&1u32.to_le_bytes());
    for relative in [78, 82, 86, 90] {
        payload[relative..relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[96..100].copy_from_slice(&2u32.to_le_bytes());
    payload[100..104].copy_from_slice(&2u32.to_le_bytes());
    payload[104..].copy_from_slice(SKETCH_MARKER);

    assert!(current_compact_104_profile_line(&payload, 0));

    payload[100..104].copy_from_slice(&3u32.to_le_bytes());
    assert!(!current_compact_104_profile_line(&payload, 0));
}

#[test]
fn legacy_compact_104_profile_line_uses_one_based_point_indices() {
    let offset = 4;
    let mut payload = vec![0; offset + 104 + LEGACY_SKETCH_MARKER.len()];
    payload[..offset].copy_from_slice(&2u32.to_le_bytes());
    payload[offset..offset + LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[offset + 5..offset + 13].fill(0xff);
    payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[offset + 23..offset + 31]
        .copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[offset + 31..offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x05, 0x00]);
    payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[offset + 56..offset + 60].copy_from_slice(&[6, 0, 8, 0]);
    payload[offset + 60..offset + 64].copy_from_slice(&1u32.to_le_bytes());
    payload[offset + 64..offset + 72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[offset + 72..offset + 76].copy_from_slice(&1u32.to_le_bytes());
    for relative in [78, 82, 86, 90] {
        payload[offset + relative..offset + relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[offset + 96..offset + 100].copy_from_slice(&2u32.to_le_bytes());
    payload[offset + 100..offset + 104].copy_from_slice(&3u32.to_le_bytes());
    payload[offset + 104..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        legacy_compact_104_profile_line_endpoint_indices(&payload, offset),
        Some([7, 9])
    );
    payload[offset + 96..offset + 100].copy_from_slice(&4u32.to_le_bytes());
    assert_eq!(
        legacy_compact_104_profile_line_endpoint_indices(&payload, offset),
        None
    );
}

#[test]
fn legacy_104_profile_line_uses_zero_based_point_roster() {
    let offset = 4;
    let mut payload = vec![0; offset + 104 + LEGACY_SKETCH_MARKER.len()];
    payload[..offset].copy_from_slice(&29u32.to_le_bytes());
    payload[offset..offset + LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[offset + 5..offset + 13].fill(0xff);
    payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[offset + 17..offset + 21].copy_from_slice(&1u32.to_le_bytes());
    payload[offset + 23..offset + 31]
        .copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x00, 0x00]);
    payload[offset + 31..offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[offset + 56..offset + 60].copy_from_slice(&[1, 0, 2, 0]);
    payload[offset + 60..offset + 64].copy_from_slice(&0u32.to_le_bytes());
    payload[offset + 64..offset + 72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[offset + 72..offset + 76].copy_from_slice(&1u32.to_le_bytes());
    for relative in [78, 82, 86, 90] {
        payload[offset + relative..offset + relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[offset + 94..offset + 96].copy_from_slice(&[0x04, 0x00]);
    payload[offset + 100..offset + 104].copy_from_slice(&30u32.to_le_bytes());
    payload[offset + 104..].copy_from_slice(LEGACY_SKETCH_MARKER);

    let entity = |id: &str, entity_offset, coordinates_m: Option<[f64; 2]>| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: entity_offset,
        object_index: None,
        local_id: None,
        kind: if coordinates_m.is_some() {
            SketchInputKind::Point
        } else {
            SketchInputKind::LineOrCircle
        },
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let curve = entity("curve", offset as u64, None);
    let first = entity("first", 10, Some([0.0, 0.0]));
    let second = entity("second", 20, Some([1.0, 0.0]));
    let third = entity("third", 30, Some([1.0, 1.0]));
    let markers = [&curve, &first, &second, &third];

    assert_eq!(
        legacy_104_profile_line_endpoint_indices(&payload, offset),
        Some([2, 3])
    );
    assert_eq!(
        coordinate_roster_endpoint_offset(&payload, offset),
        Some(56)
    );
    assert_eq!(
        roster_curve_endpoint_markers(&payload, &curve, &markers)
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["second", "third"]
    );

    payload[offset + 94..offset + 96].copy_from_slice(&[0; 2]);
    assert_eq!(
        legacy_104_profile_line_endpoint_indices(&payload, offset),
        None
    );
    payload[offset + 94..offset + 96].copy_from_slice(&[0x04, 0x00]);
    payload[offset + 104..offset + 109].fill(0);
    assert_eq!(
        legacy_104_profile_line_endpoint_indices(&payload, offset),
        None
    );

    payload[offset + 29..offset + 31].copy_from_slice(&1u16.to_le_bytes());
    payload[offset + 35..offset + 39].copy_from_slice(&[0x00, 0x00, 0x44, 0x00]);
    payload[offset + 60..offset + 64].copy_from_slice(&1u32.to_le_bytes());
    payload[offset + 94..offset + 96].fill(0);
    payload[offset + 96..offset + 100].copy_from_slice(&2u32.to_le_bytes());
    payload[offset + 104..offset + 109].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(
        legacy_104_profile_line_endpoint_indices(&payload, offset),
        None
    );
}

#[test]
fn legacy_state_one_profile_line_uses_zero_based_point_roster() {
    let offset = 4;
    let mut payload = vec![0; offset + 104 + LEGACY_SKETCH_MARKER.len()];
    payload[..offset].copy_from_slice(&41u32.to_le_bytes());
    payload[offset..offset + LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[offset + 5..offset + 13].fill(0xff);
    payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[offset + 17..offset + 21].copy_from_slice(&1u32.to_le_bytes());
    payload[offset + 23..offset + 31]
        .copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[offset + 31..offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x44, 0x00]);
    payload[offset + 39..offset + 48].fill(0);
    payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[offset + 56..offset + 60].copy_from_slice(&[1, 0, 2, 0]);
    payload[offset + 60..offset + 64].copy_from_slice(&1u32.to_le_bytes());
    payload[offset + 64..offset + 72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[offset + 72..offset + 76].copy_from_slice(&1u32.to_le_bytes());
    for relative in [78, 82, 86, 90] {
        payload[offset + relative..offset + relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[offset + 94..offset + 96].copy_from_slice(&[0; 2]);
    payload[offset + 96..offset + 100].copy_from_slice(&2u32.to_le_bytes());
    payload[offset + 100..offset + 104].copy_from_slice(&42u32.to_le_bytes());
    payload[offset + 104..].copy_from_slice(LEGACY_SKETCH_MARKER);

    let entity = |id: &str, entity_offset, coordinates_m: Option<[f64; 2]>| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: entity_offset,
        object_index: None,
        local_id: None,
        kind: if coordinates_m.is_some() {
            SketchInputKind::Point
        } else {
            SketchInputKind::LineOrCircle
        },
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let curve = entity("curve", offset as u64, None);
    let first = entity("first", 10, Some([0.0, 0.0]));
    let second = entity("second", 20, Some([1.0, 0.0]));
    let third = entity("third", 30, Some([1.0, 1.0]));
    let markers = [&curve, &first, &second, &third];

    assert!(legacy_state_one_profile_line_uses_point_roster(
        &payload, offset
    ));
    assert_eq!(
        coordinate_roster_endpoint_offset(&payload, offset),
        Some(56)
    );
    assert_eq!(
        roster_curve_endpoint_markers(&payload, &curve, &markers)
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["second", "third"]
    );

    payload[offset + 35..offset + 39].copy_from_slice(&[0x00, 0x00, 0x04, 0x00]);
    assert!(!legacy_state_one_profile_line_uses_point_roster(
        &payload, offset
    ));
    payload[offset + 35..offset + 39].copy_from_slice(&[0x00, 0x00, 0x44, 0x00]);
    payload[offset + 100..offset + 104].copy_from_slice(&43u32.to_le_bytes());
    assert!(!legacy_state_one_profile_line_uses_point_roster(
        &payload, offset
    ));
    payload[offset + 100..offset + 104].copy_from_slice(&42u32.to_le_bytes());
    payload[offset + 96..offset + 100].fill(0xff);
    assert!(!legacy_state_one_profile_line_uses_point_roster(
        &payload, offset
    ));
}

#[test]
fn legacy_state_one_84_profile_line_uses_zero_based_point_roster() {
    let offset = 4;
    let mut payload = vec![0; offset + 84 + LEGACY_SKETCH_MARKER.len()];
    payload[..offset].copy_from_slice(&41u32.to_le_bytes());
    payload[offset..offset + LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[offset + 5..offset + 13].fill(0xff);
    payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[offset + 17..offset + 21].copy_from_slice(&1u32.to_le_bytes());
    payload[offset + 23..offset + 31]
        .copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[offset + 31..offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x44, 0x00]);
    payload[offset + 39..offset + 48].fill(0);
    payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[offset + 56..offset + 60].copy_from_slice(&[1, 0, 2, 0]);
    payload[offset + 60..offset + 64].copy_from_slice(&1u32.to_le_bytes());
    payload[offset + 64..offset + 72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[offset + 72..offset + 76].fill(0);
    payload[offset + 76..offset + 80].copy_from_slice(&7u32.to_le_bytes());
    payload[offset + 80..offset + 84].copy_from_slice(&42u32.to_le_bytes());
    payload[offset + 84..].copy_from_slice(LEGACY_SKETCH_MARKER);

    let entity = |id: &str, entity_offset, coordinates_m: Option<[f64; 2]>| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: entity_offset,
        object_index: None,
        local_id: None,
        kind: if coordinates_m.is_some() {
            SketchInputKind::Point
        } else {
            SketchInputKind::LineOrCircle
        },
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let curve = entity("curve", offset as u64, None);
    let first = entity("first", 10, Some([0.0, 0.0]));
    let second = entity("second", 20, Some([1.0, 0.0]));
    let third = entity("third", 30, Some([1.0, 1.0]));
    let markers = [&curve, &first, &second, &third];

    for selector in [[0x00, 0x00, 0x44, 0x00], [0x00, 0x00, 0x84, 0x00]] {
        payload[offset + 35..offset + 39].copy_from_slice(&selector);
        assert!(legacy_state_one_84_profile_line_uses_point_roster(
            &payload, offset
        ));
    }
    assert_eq!(
        coordinate_roster_endpoint_offset(&payload, offset),
        Some(56)
    );
    assert_eq!(
        roster_curve_endpoint_markers(&payload, &curve, &markers)
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["second", "third"]
    );

    payload[offset + 35..offset + 39].copy_from_slice(&[0x00, 0x00, 0x04, 0x00]);
    assert!(!legacy_state_one_84_profile_line_uses_point_roster(
        &payload, offset
    ));
    payload[offset + 35..offset + 39].copy_from_slice(&[0x00, 0x00, 0x44, 0x00]);
    payload[offset + 76..offset + 80].fill(0);
    assert!(!legacy_state_one_84_profile_line_uses_point_roster(
        &payload, offset
    ));
    payload[offset + 76..offset + 80].copy_from_slice(&7u32.to_le_bytes());
    payload[offset + 80..offset + 84].copy_from_slice(&43u32.to_le_bytes());
    assert!(!legacy_state_one_84_profile_line_uses_point_roster(
        &payload, offset
    ));
    payload[offset + 80..offset + 84].copy_from_slice(&42u32.to_le_bytes());
    payload[offset + 84..].fill(0);
    assert!(!legacy_state_one_84_profile_line_uses_point_roster(
        &payload, offset
    ));
}

#[test]
fn extended_state_one_84_profile_line_uses_one_based_point_roster() {
    let offset = 4;
    let mut payload = vec![0; offset + 84 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..offset].copy_from_slice(&41u32.to_le_bytes());
    payload[offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[offset + 5..offset + 13].fill(0xff);
    payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[offset + 17..offset + 21].copy_from_slice(&1u32.to_le_bytes());
    payload[offset + 23..offset + 31]
        .copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[offset + 31..offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[offset + 39..offset + 48].fill(0);
    payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[offset + 56..offset + 60].copy_from_slice(&[2, 0, 3, 0]);
    payload[offset + 60..offset + 64].copy_from_slice(&1u32.to_le_bytes());
    payload[offset + 64..offset + 72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[offset + 72..offset + 76].fill(0);
    payload[offset + 76..offset + 80].copy_from_slice(&7u32.to_le_bytes());
    payload[offset + 80..offset + 84].copy_from_slice(&42u32.to_le_bytes());
    payload[offset + 84..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    let entity = |id: &str, entity_offset, coordinates_m: Option<[f64; 2]>| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: entity_offset,
        object_index: None,
        local_id: None,
        kind: if coordinates_m.is_some() {
            SketchInputKind::Point
        } else {
            SketchInputKind::LineOrCircle
        },
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let curve = entity("curve", offset as u64, None);
    let first = entity("first", 10, Some([0.0, 0.0]));
    let second = entity("second", 20, Some([1.0, 0.0]));
    let third = entity("third", 30, Some([1.0, 1.0]));
    let markers = [&curve, &first, &second, &third];

    assert!(extended_state_one_84_profile_line_uses_point_roster(
        &payload, offset
    ));
    assert_eq!(
        coordinate_roster_endpoint_offset(&payload, offset),
        Some(56)
    );
    assert_eq!(
        roster_curve_endpoint_markers(&payload, &curve, &markers)
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["second", "third"]
    );

    payload[offset + 35..offset + 39].copy_from_slice(&[0x00, 0x00, 0x44, 0x00]);
    assert!(!extended_state_one_84_profile_line_uses_point_roster(
        &payload, offset
    ));
    payload[offset + 35..offset + 39].copy_from_slice(&[0x00, 0x00, 0x04, 0x00]);
    payload[offset + 76..offset + 80].fill(0);
    assert!(!extended_state_one_84_profile_line_uses_point_roster(
        &payload, offset
    ));
    payload[offset + 76..offset + 80].copy_from_slice(&7u32.to_le_bytes());
    payload[offset + 80..offset + 84].copy_from_slice(&43u32.to_le_bytes());
    assert!(!extended_state_one_84_profile_line_uses_point_roster(
        &payload, offset
    ));
    payload[offset + 80..offset + 84].copy_from_slice(&42u32.to_le_bytes());
    payload[offset + 17..offset + 21].copy_from_slice(&2u32.to_le_bytes());
    assert!(!extended_state_one_84_profile_line_uses_point_roster(
        &payload, offset
    ));
    payload[offset + 17..offset + 21].copy_from_slice(&1u32.to_le_bytes());
    payload[offset + 84..].fill(0);
    assert!(!extended_state_one_84_profile_line_uses_point_roster(
        &payload, offset
    ));
}

#[test]
fn current_direct_92_profile_line_uses_point_object_ids() {
    let mut payload = vec![0; 92 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&6u16.to_le_bytes());
    payload[66..68].copy_from_slice(&9u16.to_le_bytes());
    payload[68..72].copy_from_slice(&1u32.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[84..88].copy_from_slice(&1u32.to_le_bytes());
    payload[88..92].copy_from_slice(&6u32.to_le_bytes());
    payload[92..].copy_from_slice(SKETCH_MARKER);

    assert_eq!(
        current_direct_92_profile_line_endpoint_indices(&payload, 0),
        Some([6, 9])
    );

    payload[88..92].fill(0);
    assert_eq!(
        current_direct_92_profile_line_endpoint_indices(&payload, 0),
        None
    );
}

#[test]
fn current_referenced_compact_line_uses_complete_one_based_marker_roster() {
    let curve_offset = 100;
    let mut payload = vec![0; curve_offset + 104 + SKETCH_MARKER.len()];
    payload[curve_offset..curve_offset + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[curve_offset + 5..curve_offset + 13].fill(0xff);
    payload[curve_offset + 13..curve_offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[curve_offset + 17..curve_offset + 21].copy_from_slice(&2u32.to_le_bytes());
    payload[curve_offset + 23..curve_offset + 31]
        .copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[curve_offset + 31..curve_offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[curve_offset + 48..curve_offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[curve_offset + 56..curve_offset + 58].copy_from_slice(&1u16.to_le_bytes());
    payload[curve_offset + 58..curve_offset + 60].copy_from_slice(&3u16.to_le_bytes());
    payload[curve_offset + 60..curve_offset + 64].copy_from_slice(&1u32.to_le_bytes());
    payload[curve_offset + 64..curve_offset + 72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[curve_offset + 72..curve_offset + 76].copy_from_slice(&1i32.to_le_bytes());
    payload[curve_offset + 76..curve_offset + 78].copy_from_slice(&22u16.to_le_bytes());
    for relative in [78, 82, 86, 90] {
        payload[curve_offset + relative..curve_offset + relative + 4]
            .copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[curve_offset + 96..curve_offset + 100].copy_from_slice(&13u32.to_le_bytes());
    payload[curve_offset + 100..curve_offset + 104].copy_from_slice(&7u32.to_le_bytes());
    payload[curve_offset + 104..].copy_from_slice(SKETCH_MARKER);

    let marker = |id: &str, offset, kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
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
    let entities = [
        marker("first", 0, SketchInputKind::Point, Some([1.0, 2.0])),
        marker(
            "relation",
            10,
            SketchInputKind::Relation(SketchRelationKind::Horizontal),
            None,
        ),
        marker("second", 20, SketchInputKind::Point, Some([3.0, 4.0])),
        marker("curve", 100, SketchInputKind::Arc, None),
    ];
    let markers = entities.iter().collect::<Vec<_>>();

    assert!(current_referenced_compact_curve_uses_marker_roster(
        &payload,
        curve_offset
    ));
    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[3], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );

    let compact_104 = payload.clone();
    payload[curve_offset + 17..curve_offset + 21].copy_from_slice(&1u32.to_le_bytes());
    payload[curve_offset + 72..curve_offset + 76].copy_from_slice(&(-1i32).to_le_bytes());
    assert!(current_referenced_compact_curve_uses_marker_roster(
        &payload,
        curve_offset
    ));
    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[3], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );

    payload = compact_104.clone();
    payload[curve_offset + 72..curve_offset + 104].fill(0);
    payload[curve_offset + 76..curve_offset + 80].copy_from_slice(&8u32.to_le_bytes());
    payload[curve_offset + 80..curve_offset + 84].copy_from_slice(&7u32.to_le_bytes());
    payload[curve_offset + 84..curve_offset + 84 + SKETCH_MARKER.len()]
        .copy_from_slice(SKETCH_MARKER);
    assert!(current_referenced_compact_curve_uses_marker_roster(
        &payload,
        curve_offset
    ));
    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[3], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );

    payload[curve_offset + 17..curve_offset + 21].copy_from_slice(&1u32.to_le_bytes());
    payload[curve_offset + 56..curve_offset + 58].copy_from_slice(&0u16.to_le_bytes());
    payload[curve_offset + 58..curve_offset + 60].copy_from_slice(&1u16.to_le_bytes());
    assert!(current_referenced_compact_curve_uses_marker_roster(
        &payload,
        curve_offset
    ));
    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[3], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );

    payload[curve_offset + 17..curve_offset + 21].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[3], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
    payload = compact_104.clone();
    payload[curve_offset + 72..curve_offset + 104].fill(0);
    payload[curve_offset + 82..curve_offset + 84].copy_from_slice(&12u16.to_le_bytes());
    payload[curve_offset + 88..curve_offset + 92].copy_from_slice(&19u32.to_le_bytes());
    payload[curve_offset + 92..curve_offset + 96].copy_from_slice(&1u32.to_le_bytes());
    payload[curve_offset + 96..curve_offset + 96 + SKETCH_MARKER.len()]
        .copy_from_slice(SKETCH_MARKER);
    assert!(current_referenced_compact_curve_uses_marker_roster(
        &payload,
        curve_offset
    ));
    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[3], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );

    payload = compact_104;
    payload[curve_offset + 100..curve_offset + 104].copy_from_slice(&13u32.to_le_bytes());
    assert!(!current_referenced_compact_curve_uses_marker_roster(
        &payload,
        curve_offset
    ));
}

#[test]
fn extended_terminal_profile_record_is_a_line() {
    let mut payload = vec![0; 170];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..31].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[58..60].copy_from_slice(&1u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[142..144].copy_from_slice(&[0x08, 0x80]);
    payload[154..170].copy_from_slice(&[
        0x01, 0x00, 0x01, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00,
        0x00,
    ]);

    assert!(extended_terminal_profile_line(&payload, 0));

    payload[142..144].fill(0);
    assert!(!extended_terminal_profile_line(&payload, 0));
}

#[test]
fn extended_selector44_indexed_line_requires_a_known_body_ending() {
    let base = |size, locus| {
        let mut payload = vec![0; size];
        payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()]
            .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
        payload[5..13].fill(0xff);
        payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[23..27].copy_from_slice(locus);
        payload[27..31].copy_from_slice(&[0x01, 0x00, 0x01, 0x00]);
        payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x44, 0x00]);
        payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[56..58].copy_from_slice(&0u16.to_le_bytes());
        payload[58..60].copy_from_slice(&1u16.to_le_bytes());
        payload[60..64].copy_from_slice(&1u32.to_le_bytes());
        payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
        payload
    };

    let mut continuation = base(
        84 + LEGACY_EXTENDED_SKETCH_MARKER.len(),
        &[0x04, 0x00, 0x02, 0x00],
    );
    continuation[39..48].copy_from_slice(&[0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    continuation[72..84].copy_from_slice(&[
        0x00, 0x00, 0x01, 0x00, 0x02, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00,
    ]);
    continuation[84..89].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    assert!(extended_selector44_indexed_line(&continuation, 0));
    assert_eq!(
        coordinate_roster_endpoint_offset(&continuation, 0),
        Some(56)
    );

    let mut counted = base(144, &[0x05, 0x00, 0x01, 0x00]);
    counted[128..132].copy_from_slice(&2u32.to_le_bytes());
    counted[138..142].fill(0xff);
    assert!(extended_selector44_indexed_line(&counted, 0));
    assert_eq!(coordinate_roster_endpoint_offset(&counted, 0), Some(56));
    counted[128..132].fill(0);
    assert!(!extended_selector44_indexed_line(&counted, 0));

    let mut control = base(170, &[0x05, 0x00, 0x01, 0x00]);
    control[142..144].copy_from_slice(&[0x08, 0x80]);
    control[154..170].copy_from_slice(&[
        0x01, 0x00, 0x01, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00,
        0x00,
    ]);
    assert!(extended_selector44_indexed_line(&control, 0));
    assert_eq!(coordinate_roster_endpoint_offset(&control, 0), Some(56));
    control[37] = 0x04;
    assert!(!extended_selector44_indexed_line(&control, 0));
}
