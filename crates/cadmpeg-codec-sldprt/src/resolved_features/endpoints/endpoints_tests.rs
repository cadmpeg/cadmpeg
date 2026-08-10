//! Tests for the `endpoints` module.

use super::super::bindings::normalize_indexed_curve_entities;
use super::super::curves::compact_bounded_curve_tangent;
use super::super::markers::{marker_coordinates, sketch_input_entities};
use super::super::relation_loci::same_dimension_length;
use super::super::selections::marker_local_links;
use super::super::typed_relations::{
    current_undetailed_bounded_curve_is_line, extended_direct_object_line_endpoints,
    legacy_marker104_arc_endpoints, marker_curve_endpoint_markers,
};
use super::super::{
    CLASS_MARKER, LEGACY_EXTENDED_SKETCH_MARKER, LEGACY_SKETCH_MARKER, SKETCH_MARKER,
};
use super::{
    alternate_current_indexed_curve_endpoint_indices,
    alternate_current_selected_axis_endpoint_indices, auxiliary_profile_record,
    compact_complete_marker_roster_pair, compact_curve_endpoint_indices,
    compact_indexed_curve_endpoint_indices, compact_indexed_curve_raw_endpoint_indices,
    compact_legacy_90_geometry_line_roster_indices, compact_legacy_code_one_line_endpoint_indices,
    compact_legacy_curve_endpoint_indices, compact_legacy_selected_axis_endpoint_indices,
    compact_legacy_short_role_one_curve_endpoint_indices,
    compact_legacy_short_role_two_curve_endpoint_indices, coordinate_circle_radius,
    coordinate_roster_arc_center, coordinate_roster_curve_endpoint_markers,
    coordinate_roster_endpoint_offset, coordinate_roster_full_circle,
    current_compact_104_indexed_line_endpoint_indices, current_compact_104_profile_line,
    current_direct_92_profile_line_endpoint_indices,
    current_identity_linked_wide_curve_uses_one_based_roster,
    current_indexed_arc_reverses_center_sweep, current_long_full_circle_radial_index,
    current_referenced_compact_curve_uses_marker_roster, current_wide_arc_direct_markers,
    direct_indexed_curve_endpoint_indices, equal_index_coordinate_roster_full_circle,
    extended_compact_84_construction_line_endpoint_indices,
    extended_compact_96_selected_axis_endpoint_indices, extended_compact_endpoint_markers,
    extended_declared_inline_line_endpoints, extended_direct_object_line_endpoint_ids,
    extended_geometry_locus_construction_line_endpoint_indices,
    extended_identity_inline_line_endpoints, extended_linked_inline_line_endpoints,
    extended_profile_roster_construction_line_endpoint_indices, extended_selector44_indexed_line,
    extended_shifted_construction_line_endpoint_indices,
    extended_state_one_84_profile_line_uses_point_roster,
    extended_tagged_indexed_curve_endpoint_indices, extended_terminal_profile_line,
    extended_wide_construction_line_roster_indices, indexed_arc_uses_coordinate_center,
    legacy_104_profile_line_endpoint_indices, legacy_compact_104_profile_line_endpoint_indices,
    legacy_compact_diameter_arc_center, legacy_compact_direct_endpoint_markers,
    legacy_compact_profile_line, legacy_coordinate_circle_radius,
    legacy_coordinate_roster_selected_axis_endpoint_indices,
    legacy_direct_compact_selected_axis_endpoint_indices,
    legacy_long_profile_line_endpoint_indices, legacy_referenced_wide_arc_endpoint_indices,
    legacy_state_five_curve_endpoint_indices, legacy_state_one_84_profile_line_uses_point_roster,
    legacy_state_one_profile_line_uses_point_roster, legacy_terminal_profile_endpoint_offset,
    legacy_undetailed_profile_line, legacy_unlocated_geometry_handle,
    marker_is_selected_construction_line, packed_legacy_curve_endpoint_indices,
    relation_reference_curve_record, roster_curve_endpoint_markers, unique_arc_center_marker,
    wide_direct_line_endpoint_markers, wide_indexed_curve_endpoint_indices,
};
use crate::records::{
    FeatureInputLane, SketchInputEntity, SketchInputKind, SketchInputLink, SketchRelationKind,
};
use cadmpeg_ir::math::Point2;
use std::collections::HashMap;

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
fn current_referenced_compact_roster_rejects_conflicting_fallbacks() {
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
    assert!(coordinate_roster_curve_endpoint_markers(&payload, &curve, &markers).is_empty());
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
fn linked_profile_curve_uses_its_two_typed_endpoint_cells() {
    let offset = 4;
    let mut payload = vec![0; offset + 146 + SKETCH_MARKER.len()];
    payload[offset..offset + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[offset + 5..offset + 13].fill(0xff);
    payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[offset + 17..offset + 21].copy_from_slice(&2u32.to_le_bytes());
    payload[offset + 23..offset + 29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[offset + 31..offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[offset + 56..offset + 58].copy_from_slice(&[0x1e, 0x00]);
    payload[offset + 58..offset + 66].copy_from_slice(&1.25f64.to_le_bytes());
    payload[offset + 66..offset + 74].copy_from_slice(&(-2.5f64).to_le_bytes());
    payload[offset + 76..offset + 78].copy_from_slice(&3u16.to_le_bytes());
    for (relative, endpoint) in [(78, 2u16), (86, 3u16)] {
        payload[offset + relative..offset + relative + 2].copy_from_slice(&0x8137u16.to_le_bytes());
        payload[offset + relative + 2..offset + relative + 4]
            .copy_from_slice(&endpoint.to_le_bytes());
        payload[offset + relative + 4..offset + relative + 8].fill(0xff);
    }
    payload[offset + 94..offset + 100].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[offset + 142..offset + 146].copy_from_slice(&5u32.to_le_bytes());
    for prefix in [SKETCH_MARKER, LEGACY_EXTENDED_SKETCH_MARKER] {
        payload[offset..offset + prefix.len()].copy_from_slice(prefix);
        payload[offset + 146..offset + 146 + prefix.len()].copy_from_slice(prefix);
        assert_eq!(
            super::linked_profile_curve_endpoint_indices(&payload, offset),
            Some([2, 3])
        );
    }
}

#[test]
fn extended_linked_line_uses_inline_self_endpoint() {
    let mut payload = vec![0; 146 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..66].copy_from_slice(&0.007f64.to_le_bytes());
    payload[66..74].copy_from_slice(&0.0075f64.to_le_bytes());
    payload[76..78].copy_from_slice(&2u16.to_le_bytes());
    for (relative, endpoint) in [(78, 2u16), (86, 5u16)] {
        payload[relative..relative + 2].copy_from_slice(&0x810cu16.to_le_bytes());
        payload[relative + 2..relative + 4].copy_from_slice(&endpoint.to_le_bytes());
        payload[relative + 4..relative + 8].fill(0xff);
    }
    payload[94..100].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[142..146].fill(0xff);
    payload[146..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    let mut external = SketchInputEntity {
        id: "external".into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 1,
        offset: 0,
        object_index: Some(3),
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m: Some([0.0, 0.0075]),
        links: Vec::new(),
        link_selector: None,
    };
    let mut curve = SketchInputEntity {
        id: "curve".into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 2,
        offset: 0,
        object_index: Some(6),
        local_id: None,
        kind: SketchInputKind::LineOrCircle,
        state_value: Some(1.0),
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };

    assert_eq!(
        extended_linked_inline_line_endpoints(&payload, &curve, &[&external, &curve]),
        Some([[0.0, 0.0075], [0.007, 0.0075]])
    );
    payload[80..82].copy_from_slice(&1u16.to_le_bytes());
    payload[88..90].copy_from_slice(&4u16.to_le_bytes());
    payload[136..140].copy_from_slice(&1u32.to_le_bytes());
    external.object_index = Some(1);
    curve.object_index = Some(4);
    assert_eq!(
        extended_linked_inline_line_endpoints(&payload, &curve, &[&external, &curve]),
        Some([[0.0, 0.0075], [0.007, 0.0075]])
    );
    payload[140] = 1;
    assert_eq!(
        extended_linked_inline_line_endpoints(&payload, &curve, &[&external, &curve]),
        None
    );
}

#[test]
fn extended_identity_line_uses_inline_and_identified_point_endpoints() {
    let mut payload = vec![0; 134 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..66].copy_from_slice(&0.007f64.to_le_bytes());
    payload[66..74].copy_from_slice(&0.0075f64.to_le_bytes());
    payload[74..78].copy_from_slice(&[0x00, 0x00, 0x01, 0x00]);
    payload[82..84].copy_from_slice(&1u16.to_le_bytes());
    payload[84..88].copy_from_slice(&(-2i32).to_le_bytes());
    payload[130..134].copy_from_slice(&5u32.to_le_bytes());
    payload[134..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    let point = SketchInputEntity {
        id: "point".into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 1,
        offset: 200,
        object_index: Some(5),
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m: Some([0.01, 0.012]),
        links: Vec::new(),
        link_selector: None,
    };
    let curve = SketchInputEntity {
        id: "curve".into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 2,
        offset: 0,
        object_index: Some(6),
        local_id: None,
        kind: SketchInputKind::LineOrCircle,
        state_value: Some(1.0),
        coordinates_m: Some([0.007, 0.0075]),
        links: Vec::new(),
        link_selector: None,
    };

    assert_eq!(
        extended_identity_inline_line_endpoints(&payload, &curve, &[&point, &curve]),
        Some([[0.007, 0.0075], [0.01, 0.012]])
    );
    let chained_curve = SketchInputEntity {
        id: "chained-curve".into(),
        kind: SketchInputKind::Arc,
        ..point.clone()
    };
    assert_eq!(
        extended_identity_inline_line_endpoints(&payload, &curve, &[&chained_curve, &curve],),
        Some([[0.007, 0.0075], [0.01, 0.012]])
    );
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        extended_identity_inline_line_endpoints(&payload, &curve, &[&point, &curve]),
        Some([[0.007, 0.0075], [0.01, 0.012]])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::LineOrCircle
    );
    payload[74..84].copy_from_slice(&[0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    payload[126..130].copy_from_slice(&4u32.to_le_bytes());
    let direct_curve = SketchInputEntity {
        kind: SketchInputKind::Arc,
        ..curve.clone()
    };
    assert_eq!(
        extended_identity_inline_line_endpoints(
            &payload,
            &direct_curve,
            &[&chained_curve, &direct_curve],
        ),
        Some([[0.007, 0.0075], [0.01, 0.012]])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::LineOrCircle
    );
    payload[126..130].fill(0);
    assert_eq!(
        extended_identity_inline_line_endpoints(
            &payload,
            &direct_curve,
            &[&chained_curve, &direct_curve],
        ),
        None
    );
    payload[126..130].copy_from_slice(&4u32.to_le_bytes());
    let duplicate = SketchInputEntity {
        id: "duplicate".into(),
        ..point.clone()
    };
    assert_eq!(
        extended_identity_inline_line_endpoints(&payload, &curve, &[&point, &duplicate, &curve],),
        None
    );
    payload[130..134].fill(0);
    assert_eq!(
        extended_identity_inline_line_endpoints(&payload, &curve, &[&point, &curve]),
        None
    );
}

#[test]
fn extended_declared_line_uses_its_typed_point_selector() {
    let mut payload = vec![0; 170 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..66].copy_from_slice(&0.0165f64.to_le_bytes());
    payload[66..74].copy_from_slice(&0.029f64.to_le_bytes());
    payload[76..78].copy_from_slice(&2u16.to_le_bytes());
    payload[78..84].copy_from_slice(&[0xff, 0xff, 0x01, 0x00, 0x0c, 0x00]);
    payload[84..96].copy_from_slice(b"sgLineHandle");
    payload[96..106].copy_from_slice(&[0x08, 0x00, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]);
    payload[106..108].copy_from_slice(&0x8155u16.to_le_bytes());
    payload[108..110].copy_from_slice(&7u16.to_le_bytes());
    payload[110..114].fill(0xff);
    payload[118..124].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[166..170].copy_from_slice(&4u32.to_le_bytes());
    payload[170..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    let external = SketchInputEntity {
        id: "external".into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 7,
        offset: 0,
        object_index: Some(7),
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m: Some([0.014, 0.016]),
        links: Vec::new(),
        link_selector: None,
    };
    let curve = SketchInputEntity {
        id: "curve".into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 3,
        offset: 0,
        object_index: Some(3),
        local_id: None,
        kind: SketchInputKind::LineOrCircle,
        state_value: Some(1.0),
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };

    assert_eq!(
        extended_declared_inline_line_endpoints(&payload, &curve, &[&external, &curve]),
        Some([[0.014, 0.016], [0.0165, 0.029]])
    );
    payload[96..98].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        extended_declared_inline_line_endpoints(&payload, &curve, &[&external, &curve]),
        Some([[0.014, 0.016], [0.0165, 0.029]])
    );
    payload[96..98].fill(0);
    assert_eq!(
        extended_declared_inline_line_endpoints(&payload, &curve, &[&external, &curve]),
        None
    );
    payload[96..98].fill(0xff);
    assert_eq!(
        extended_declared_inline_line_endpoints(&payload, &curve, &[&external, &curve]),
        None
    );
    payload[96..98].copy_from_slice(&8u16.to_le_bytes());
    payload[110] = 0;
    assert_eq!(
        extended_declared_inline_line_endpoints(&payload, &curve, &[&external, &curve]),
        None
    );
}

#[test]
fn compact_indexed_curve_stores_endpoints_in_both_generations() {
    let mut payload = vec![0; 84 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..35].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[35..39].copy_from_slice(&[0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&6u16.to_le_bytes());
    payload[58..60].copy_from_slice(&10u16.to_le_bytes());
    payload[80..84].copy_from_slice(&19u32.to_le_bytes());
    payload[84..].copy_from_slice(SKETCH_MARKER);

    assert_eq!(
        compact_indexed_curve_endpoint_indices(&payload, 0),
        Some([7, 11])
    );
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[84..84 + LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(
        compact_indexed_curve_endpoint_indices(&payload, 0),
        Some([7, 11])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Arc
    );

    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    assert!(!marker_is_selected_construction_line(&payload, 0));
    payload[17..21].fill(0);
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[35..39].copy_from_slice(&[0x00, 0x00, 0x45, 0x00]);
    assert_eq!(
        super::extended_compact_indexed_curve_endpoint_indices(&payload, 0),
        Some([7, 11])
    );
    assert!(current_undetailed_bounded_curve_is_line(&payload, 0));
    payload[35..39].copy_from_slice(&[0x00, 0x00, 0x04, 0x00]);
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[60..64].fill(0);

    assert_eq!(
        compact_indexed_curve_endpoint_indices(&payload, 0),
        Some([7, 11])
    );
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    assert_eq!(
        compact_indexed_curve_endpoint_indices(&payload, 0),
        Some([7, 11])
    );
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[56..58].copy_from_slice(&30u16.to_le_bytes());
    payload[58..60].copy_from_slice(&31u16.to_le_bytes());
    assert_eq!(
        compact_indexed_curve_endpoint_indices(&payload, 0),
        Some([31, 32])
    );
    assert_eq!(marker_coordinates(&payload, 0), None);
    payload[56..58].copy_from_slice(&6u16.to_le_bytes());
    payload[58..60].copy_from_slice(&10u16.to_le_bytes());
    payload[27..29].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(compact_indexed_curve_endpoint_indices(&payload, 0), None);
}

#[test]
fn direct_indexed_curve_stores_feature_local_point_ids() {
    let mut payload = vec![0; 84 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x05, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&6u16.to_le_bytes());
    payload[58..60].copy_from_slice(&15u16.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        direct_indexed_curve_endpoint_indices(&payload, 0),
        Some([6, 15])
    );
    assert_eq!(compact_indexed_curve_endpoint_indices(&payload, 0), None);
    payload[58..60].copy_from_slice(&6u16.to_le_bytes());
    assert_eq!(direct_indexed_curve_endpoint_indices(&payload, 0), None);
    payload[58..60].copy_from_slice(&15u16.to_le_bytes());
    payload[35..39].copy_from_slice(&[0x00, 0x00, 0x04, 0x00]);
    assert_eq!(direct_indexed_curve_endpoint_indices(&payload, 0), None);
}

#[test]
fn extended_direct_object_line_uses_exact_point_identities() {
    let mut payload = vec![0; 84 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x44, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&0u16.to_le_bytes());
    payload[58..60].copy_from_slice(&4u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[76..84].copy_from_slice(&3u64.to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    assert_eq!(
        extended_direct_object_line_endpoint_ids(&payload, 0),
        Some([0, 4])
    );
    payload[17..21].fill(0);
    assert_eq!(
        extended_direct_object_line_endpoint_ids(&payload, 0),
        Some([0, 4])
    );
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[37] = 0x04;
    assert_eq!(extended_direct_object_line_endpoint_ids(&payload, 0), None);
    payload[37] = 0x44;

    let entity = |id: &str, object_index, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("profile".into()),
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
    let curve = SketchInputEntity {
        kind: SketchInputKind::LineOrCircle,
        ..entity("curve", Some(2), None)
    };
    let implicit = entity("implicit", None, Some([1.0, 2.0]));
    let explicit = entity("explicit", Some(4), Some([3.0, 4.0]));
    let markers = [&curve, &implicit, &explicit];
    assert_eq!(
        extended_direct_object_line_endpoints(&payload, &curve, &markers)
            .map(|endpoints| endpoints.map(|endpoint| endpoint.id.as_str())),
        Some(["implicit", "explicit"])
    );
    let arc = SketchInputEntity {
        kind: SketchInputKind::Arc,
        ..curve.clone()
    };
    assert_eq!(
        extended_direct_object_line_endpoints(&payload, &arc, &markers),
        None
    );
    let wrong_first = entity("wrong-first", Some(5), Some([5.0, 6.0]));
    let wrong_second = entity("wrong-second", Some(6), Some([7.0, 8.0]));
    let mut linked_curve = curve.clone();
    linked_curve.links = vec![
        SketchInputLink {
            local_id: 5,
            entity_ref: wrong_first.id.clone(),
        },
        SketchInputLink {
            local_id: 6,
            entity_ref: wrong_second.id.clone(),
        },
    ];
    let markers = [
        &linked_curve,
        &implicit,
        &explicit,
        &wrong_first,
        &wrong_second,
    ];
    let markers_by_id = markers
        .iter()
        .map(|marker| (marker.id.as_str(), *marker))
        .collect::<HashMap<_, _>>();
    assert_eq!(
        marker_curve_endpoint_markers(&payload, &linked_curve, &markers_by_id, &markers)
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["implicit", "explicit"]
    );

    payload[58..60].fill(0);
    assert_eq!(extended_direct_object_line_endpoint_ids(&payload, 0), None);
    payload[58..60].copy_from_slice(&4u16.to_le_bytes());
    payload[37] = 0x0c;
    assert_eq!(extended_direct_object_line_endpoint_ids(&payload, 0), None);
    payload[37] = 0x44;
    payload[74] = 2;
    assert_eq!(extended_direct_object_line_endpoint_ids(&payload, 0), None);
}

#[test]
fn legacy_state_five_identity_curve_uses_coordinate_roster_indices() {
    let mut payload = vec![0; 84 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x05, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&6u16.to_le_bytes());
    payload[58..60].copy_from_slice(&9u16.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[76..80].copy_from_slice(&11u32.to_le_bytes());
    payload[80..84].copy_from_slice(&25u32.to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        legacy_state_five_curve_endpoint_indices(&payload, 0),
        Some([7, 10])
    );
    assert_eq!(
        super::coordinate_roster_endpoint_offset(&payload, 0),
        Some(56)
    );

    payload[80..84].copy_from_slice(&11u32.to_le_bytes());
    assert_eq!(legacy_state_five_curve_endpoint_indices(&payload, 0), None);
    payload[80..84].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(legacy_state_five_curve_endpoint_indices(&payload, 0), None);
}

#[test]
fn extended_tagged_indexed_curve_uses_direct_point_ids() {
    let mut payload = vec![0; 104 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[29..31].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..60].copy_from_slice(&31u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&1i32.to_le_bytes());
    payload[76..78].copy_from_slice(&24u16.to_le_bytes());
    for relative in [78, 82, 86, 90] {
        payload[relative..relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[104..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    assert_eq!(
        extended_tagged_indexed_curve_endpoint_indices(&payload, 0),
        Some([31, 24])
    );
    assert_eq!(marker_coordinates(&payload, 0), None);
    payload[76..78].copy_from_slice(&31u16.to_le_bytes());
    assert_eq!(
        extended_tagged_indexed_curve_endpoint_indices(&payload, 0),
        None
    );

    payload[76..78].copy_from_slice(&24u16.to_le_bytes());
    payload.resize(370, 0);
    payload[94..150].fill(0);
    payload[150..152].copy_from_slice(&[0x08, 0x80]);
    payload[152..162].fill(0);
    payload[162..166].copy_from_slice(&[0x01, 0x00, 0x01, 0x00]);
    for (relative, count) in [(166, 65u32), (170, 57), (174, 33), (178, 13)] {
        payload[relative..relative + 4].copy_from_slice(&count.to_le_bytes());
    }
    for relative in (182..230).step_by(4) {
        payload[relative..relative + 4].copy_from_slice(&1u32.to_le_bytes());
    }
    payload[230..258].copy_from_slice(&[
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xfe, 0xff, 0x00, 0xff, 0xff, 0x00, 0x00, 0x80,
        0xbf, 0xff, 0xff, 0xff, 0xff, 0x01, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff,
    ]);
    payload[258..282].fill(0);
    payload[282..286].copy_from_slice(&49u32.to_le_bytes());
    payload[286..338].fill(0);
    payload[338..342].copy_from_slice(&3u32.to_le_bytes());
    payload[342..346].copy_from_slice(&1u32.to_le_bytes());
    payload[346..353].fill(0);
    payload[353..357].copy_from_slice(&0x0001_86a5u32.to_le_bytes());
    payload[357..359].copy_from_slice(&5u16.to_le_bytes());
    payload[359..363].copy_from_slice(CLASS_MARKER);
    payload[363..365].copy_from_slice(&5u16.to_le_bytes());
    payload[365..370].copy_from_slice(b"class");
    assert_eq!(
        extended_tagged_indexed_curve_endpoint_indices(&payload, 0),
        Some([31, 24])
    );
    payload[338..342].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        extended_tagged_indexed_curve_endpoint_indices(&payload, 0),
        None
    );
}

#[test]
fn extended_compact_curve_resolves_zero_based_point_object_ids() {
    let mut payload = vec![0; 84 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&16u16.to_le_bytes());
    payload[58..60].copy_from_slice(&0u16.to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    let entity = |id: &str, object_index, coordinates_m, kind| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("profile".into()),
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
    let entities = [
        entity("curve", Some(8), None, SketchInputKind::LineOrCircle),
        entity(
            "explicit",
            Some(16),
            Some([0.0, 0.006]),
            SketchInputKind::Point,
        ),
        entity(
            "implicit-zero",
            None,
            Some([0.0, 0.0]),
            SketchInputKind::Point,
        ),
        entity(
            "explicit-fourteen",
            Some(14),
            Some([0.022, 0.0075]),
            SketchInputKind::Point,
        ),
    ];
    let markers = entities.iter().collect::<Vec<_>>();

    assert_eq!(
        extended_compact_endpoint_markers(&payload, &entities[0], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["explicit", "implicit-zero"]
    );
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[27..29].copy_from_slice(&2u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    assert_eq!(
        extended_compact_endpoint_markers(&payload, &entities[0], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["explicit", "implicit-zero"]
    );
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    let duplicate = entity(
        "duplicate-zero",
        None,
        Some([1.0, 0.0]),
        SketchInputKind::Point,
    );
    let ambiguous = [&entities[0], &entities[1], &entities[2], &duplicate];
    assert!(extended_compact_endpoint_markers(&payload, &entities[0], &ambiguous).is_empty());

    payload.resize(96 + LEGACY_EXTENDED_SKETCH_MARKER.len(), 0);
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..96].fill(0);
    payload[82..84].copy_from_slice(&2u16.to_le_bytes());
    payload[88..92].copy_from_slice(&2u32.to_le_bytes());
    payload[92..96].copy_from_slice(&1u32.to_le_bytes());
    payload[96..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    assert_eq!(
        extended_compact_endpoint_markers(&payload, &entities[0], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["explicit", "implicit-zero"]
    );
    payload[82..84].fill(0);
    assert!(extended_compact_endpoint_markers(&payload, &entities[0], &markers).is_empty());

    payload.resize(102, 0);
    payload[56..58].copy_from_slice(&14u16.to_le_bytes());
    payload[58..60].copy_from_slice(&16u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..102].fill(0);
    assert_eq!(
        extended_compact_endpoint_markers(&payload, &entities[0], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["explicit-fourteen", "explicit"]
    );
    payload[17..21].copy_from_slice(&0u32.to_le_bytes());
    assert_eq!(
        extended_compact_endpoint_markers(&payload, &entities[0], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["explicit-fourteen", "explicit"]
    );
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());

    let mut roster_indexed = entities.clone();
    roster_indexed[1].object_index = None;
    roster_indexed[3].object_index = None;
    payload[56..58].copy_from_slice(&1u16.to_le_bytes());
    payload[58..60].copy_from_slice(&3u16.to_le_bytes());
    let markers = roster_indexed.iter().collect::<Vec<_>>();
    assert_eq!(
        extended_compact_endpoint_markers(&payload, &roster_indexed[0], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["explicit", "explicit-fourteen"]
    );

    payload.resize(116, 0);
    payload[56..58].copy_from_slice(&1u16.to_le_bytes());
    payload[58..60].copy_from_slice(&2u16.to_le_bytes());
    payload[60..64].fill(0);
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..116].fill(0);
    assert_eq!(
        extended_compact_endpoint_markers(&payload, &roster_indexed[0], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["explicit", "implicit-zero"]
    );
}

#[test]
fn extended_geometry_locus_terminal_curve_resolves_point_object_ids() {
    let mut payload = vec![0; 102];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[29..31].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&7u16.to_le_bytes());
    payload[58..60].copy_from_slice(&10u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    let entity = |id: &str, offset, object_index, kind, coordinates_m| SketchInputEntity {
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
    let entities = [
        entity("curve", 0, Some(8), SketchInputKind::LineOrCircle, None),
        entity(
            "first",
            100,
            Some(7),
            SketchInputKind::Point,
            Some([0.0, 0.0]),
        ),
        entity(
            "second",
            200,
            Some(10),
            SketchInputKind::Point,
            Some([1.0, 0.0]),
        ),
    ];
    let markers = entities.iter().collect::<Vec<_>>();

    assert_eq!(
        extended_compact_endpoint_markers(&payload, &entities[0], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
    payload[29..31].copy_from_slice(&[0; 2]);
    assert_eq!(
        extended_compact_endpoint_markers(&payload, &entities[0], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
    payload[29..31].copy_from_slice(&2u16.to_le_bytes());
    assert!(extended_compact_endpoint_markers(&payload, &entities[0], &markers).is_empty());
}

#[test]
fn wide_profile_curves_index_the_coordinate_roster() {
    let curve_offset = 402;
    let mut payload = vec![0; curve_offset + 92 + LEGACY_SKETCH_MARKER.len()];
    for (offset, coordinate) in [
        (0, [1.0_f64, 2.0]),
        (134, [3.0_f64, 4.0]),
        (268, [5.0_f64, 6.0]),
    ] {
        payload[offset..offset + LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 23..offset + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
        payload[offset + 27..offset + 29].copy_from_slice(&1u16.to_le_bytes());
        payload[offset + 56..offset + 58].copy_from_slice(&[0x1e, 0x00]);
        payload[offset + 58..offset + 66].copy_from_slice(&coordinate[0].to_le_bytes());
        payload[offset + 66..offset + 74].copy_from_slice(&coordinate[1].to_le_bytes());
    }
    payload[curve_offset..curve_offset + LEGACY_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[curve_offset + 5..curve_offset + 13].fill(0xff);
    payload[curve_offset + 13..curve_offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[curve_offset + 17..curve_offset + 21].copy_from_slice(&2u32.to_le_bytes());
    payload[curve_offset + 23..curve_offset + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[curve_offset + 27..curve_offset + 29].copy_from_slice(&1u16.to_le_bytes());
    payload[curve_offset + 31..curve_offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[curve_offset + 48..curve_offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[curve_offset + 64..curve_offset + 66].copy_from_slice(&0u16.to_le_bytes());
    payload[curve_offset + 66..curve_offset + 68].copy_from_slice(&2u16.to_le_bytes());
    payload[curve_offset + 68..curve_offset + 72].copy_from_slice(&1u32.to_le_bytes());
    payload[curve_offset + 72..curve_offset + 80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[curve_offset + 92..].copy_from_slice(LEGACY_SKETCH_MARKER);
    let mut entities = sketch_input_entities(&payload, "lane");
    entities.truncate(4);
    for entity in &mut entities {
        entity.feature_ref = Some("sketch".into());
    }
    let markers = entities.iter().collect::<Vec<_>>();

    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[3], &markers)
            .iter()
            .map(|marker| marker.coordinates_m)
            .collect::<Vec<_>>(),
        vec![Some([1.0, 2.0]), Some([5.0, 6.0])]
    );

    payload[curve_offset..curve_offset + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[curve_offset + 92..curve_offset + 92 + SKETCH_MARKER.len()]
        .copy_from_slice(SKETCH_MARKER);
    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[3], &markers)
            .iter()
            .map(|marker| marker.coordinates_m)
            .collect::<Vec<_>>(),
        vec![Some([1.0, 2.0]), Some([5.0, 6.0])]
    );

    payload[curve_offset + 64..curve_offset + 66].copy_from_slice(&1u16.to_le_bytes());
    payload[curve_offset + 66..curve_offset + 68].copy_from_slice(&3u16.to_le_bytes());
    payload[curve_offset + 84..curve_offset + 88].copy_from_slice(&4u32.to_le_bytes());
    payload[curve_offset + 88..curve_offset + 92].copy_from_slice(&7u32.to_le_bytes());
    assert!(current_identity_linked_wide_curve_uses_one_based_roster(
        &payload,
        curve_offset
    ));
    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[3], &markers)
            .iter()
            .map(|marker| marker.coordinates_m)
            .collect::<Vec<_>>(),
        vec![Some([1.0, 2.0]), Some([5.0, 6.0])]
    );

    payload[curve_offset + 84..curve_offset + 88].copy_from_slice(&1u32.to_le_bytes());
    payload[curve_offset + 29..curve_offset + 31].copy_from_slice(&1u16.to_le_bytes());
    assert!(current_direct_92_profile_line_endpoint_indices(&payload, curve_offset).is_some());
    assert!(!current_identity_linked_wide_curve_uses_one_based_roster(
        &payload,
        curve_offset
    ));

    payload[curve_offset + 64..curve_offset + 66].copy_from_slice(&1u16.to_le_bytes());
    payload[curve_offset + 66..curve_offset + 68].copy_from_slice(&2u16.to_le_bytes());
    payload[curve_offset + 29..curve_offset + 31].fill(0);
    payload[curve_offset + 84..curve_offset + 92].fill(0);
    let mut centered_entities = entities.clone();
    centered_entities[0].coordinates_m = Some([0.0, 0.0]);
    centered_entities[0].kind = SketchInputKind::Relation(SketchRelationKind::Horizontal);
    centered_entities[1].coordinates_m = Some([1.0, 0.0]);
    centered_entities[2].coordinates_m = Some([0.0, 1.0]);
    let centered_markers = centered_entities.iter().collect::<Vec<_>>();
    assert_eq!(
        coordinate_roster_arc_center(
            &payload,
            &centered_entities[3],
            &centered_markers,
            [&centered_entities[1], &centered_entities[2]],
        ),
        Some([0.0, 0.0])
    );
    let mut hybrid_entities = centered_entities.clone();
    let mut additional_endpoint = hybrid_entities[2].clone();
    additional_endpoint.id.push_str(":additional");
    additional_endpoint.offset += 1;
    additional_endpoint.coordinates_m = Some([-1.0, 0.0]);
    hybrid_entities.insert(3, additional_endpoint);
    payload[curve_offset + 64..curve_offset + 66].copy_from_slice(&2u16.to_le_bytes());
    payload[curve_offset + 66..curve_offset + 68].copy_from_slice(&1u16.to_le_bytes());
    let hybrid_markers = hybrid_entities.iter().collect::<Vec<_>>();
    assert_eq!(
        coordinate_roster_arc_center(
            &payload,
            &hybrid_entities[4],
            &hybrid_markers,
            [&hybrid_entities[3], &hybrid_entities[2]],
        ),
        None
    );
    hybrid_entities[0].coordinates_m = Some([4.0, 4.0]);
    hybrid_entities[1].coordinates_m = Some([0.0, 0.0]);
    hybrid_entities[1].object_index = Some(0);
    let hybrid_markers = hybrid_entities.iter().collect::<Vec<_>>();
    assert_eq!(
        coordinate_roster_arc_center(
            &payload,
            &hybrid_entities[4],
            &hybrid_markers,
            [&hybrid_entities[3], &hybrid_entities[2]],
        ),
        Some([0.0, 0.0])
    );
    payload[curve_offset + 64..curve_offset + 66].copy_from_slice(&0u16.to_le_bytes());
    payload[curve_offset + 66..curve_offset + 68].copy_from_slice(&2u16.to_le_bytes());
    payload[curve_offset..curve_offset + LEGACY_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[curve_offset + 92..curve_offset + 92 + LEGACY_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_SKETCH_MARKER);

    payload[curve_offset + 23..curve_offset + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[curve_offset + 35..curve_offset + 39].copy_from_slice(&[0x00, 0x00, 0x05, 0x00]);
    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[3], &markers)
            .iter()
            .map(|marker| marker.coordinates_m)
            .collect::<Vec<_>>(),
        vec![Some([1.0, 2.0]), Some([5.0, 6.0])]
    );
    payload[curve_offset + 23..curve_offset + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[curve_offset + 35..curve_offset + 39].copy_from_slice(&[0x00, 0x00, 0x04, 0x00]);

    payload[curve_offset + 56..curve_offset + 58].copy_from_slice(&1u16.to_le_bytes());
    payload[curve_offset + 58..curve_offset + 60].copy_from_slice(&2u16.to_le_bytes());
    payload[curve_offset + 84..curve_offset + 84 + LEGACY_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[3], &markers)
            .iter()
            .map(|marker| marker.coordinates_m)
            .collect::<Vec<_>>(),
        vec![Some([3.0, 4.0]), Some([5.0, 6.0])]
    );
    assert!(legacy_undetailed_profile_line(&payload, curve_offset));

    payload[curve_offset..curve_offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[curve_offset + 84..curve_offset + 84 + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    assert_eq!(
        super::extended_compact_indexed_curve_endpoint_indices(&payload, curve_offset),
        Some([2, 3])
    );
    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[3], &markers)
            .iter()
            .map(|marker| marker.coordinates_m)
            .collect::<Vec<_>>(),
        vec![Some([3.0, 4.0]), Some([5.0, 6.0])]
    );

    payload.resize(curve_offset + 104 + LEGACY_EXTENDED_SKETCH_MARKER.len(), 0);
    payload[curve_offset + 84..].fill(0);
    payload[curve_offset + 60..curve_offset + 64].copy_from_slice(&1u32.to_le_bytes());
    payload[curve_offset + 64..curve_offset + 72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[curve_offset + 72..curve_offset + 76].copy_from_slice(&1i32.to_le_bytes());
    for at in (curve_offset + 78..curve_offset + 94).step_by(4) {
        payload[at..at + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[curve_offset + 104..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    let mut complete_roster_entities = entities.clone();
    complete_roster_entities[0].coordinates_m = None;
    complete_roster_entities[0].kind = SketchInputKind::Relation(SketchRelationKind::Horizontal);
    let complete_roster_markers = complete_roster_entities.iter().collect::<Vec<_>>();
    assert_eq!(
        roster_curve_endpoint_markers(
            &payload,
            &complete_roster_entities[3],
            &complete_roster_markers,
        )
        .iter()
        .map(|marker| marker.coordinates_m)
        .collect::<Vec<_>>(),
        vec![Some([3.0, 4.0]), Some([5.0, 6.0])]
    );
    payload[curve_offset + 56..curve_offset + 58].fill(0);
    assert!(roster_curve_endpoint_markers(
        &payload,
        &complete_roster_entities[3],
        &complete_roster_markers,
    )
    .is_empty());
}

#[test]
fn extended_terminal_wide_profile_curve_uses_coordinate_roster() {
    let curve_offset = 536;
    let mut payload = vec![0; curve_offset + 148];
    for (offset, coordinate) in [
        (0, [1.0_f64, 2.0]),
        (134, [3.0_f64, 4.0]),
        (268, [5.0_f64, 6.0]),
        (402, [7.0_f64, 8.0]),
    ] {
        payload[offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
            .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 23..offset + 29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
        payload[offset + 56..offset + 58].copy_from_slice(&[0x1e, 0x00]);
        payload[offset + 58..offset + 66].copy_from_slice(&coordinate[0].to_le_bytes());
        payload[offset + 66..offset + 74].copy_from_slice(&coordinate[1].to_le_bytes());
    }
    payload[curve_offset..curve_offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[curve_offset + 5..curve_offset + 13].fill(0xff);
    payload[curve_offset + 13..curve_offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[curve_offset + 17..curve_offset + 21].copy_from_slice(&2u32.to_le_bytes());
    payload[curve_offset + 23..curve_offset + 29]
        .copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[curve_offset + 31..curve_offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[curve_offset + 48..curve_offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[curve_offset + 64..curve_offset + 66].copy_from_slice(&3u16.to_le_bytes());
    payload[curve_offset + 66..curve_offset + 68].copy_from_slice(&1u16.to_le_bytes());
    payload[curve_offset + 68..curve_offset + 72].copy_from_slice(&1u32.to_le_bytes());
    payload[curve_offset + 72..curve_offset + 80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[curve_offset + 128..curve_offset + 130].copy_from_slice(&[0x0a, 0x00]);
    payload[curve_offset + 130..curve_offset + 134].copy_from_slice(CLASS_MARKER);
    payload[curve_offset + 134..curve_offset + 136].copy_from_slice(&12u16.to_le_bytes());
    payload[curve_offset + 136..curve_offset + 148].copy_from_slice(b"sgPntPntDist");

    let point = |id: &str, offset, coordinates_m| SketchInputEntity {
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
        id: "curve".into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: curve_offset as u64,
        object_index: None,
        local_id: None,
        kind: SketchInputKind::LineOrCircle,
        state_value: Some(1.0),
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };
    let entities = [
        point("first", 0, Some([1.0, 2.0])),
        point("second", 134, Some([3.0, 4.0])),
        point("third", 268, Some([5.0, 6.0])),
        point("fourth", 402, Some([7.0, 8.0])),
        curve,
    ];
    let markers = entities.iter().collect::<Vec<_>>();

    assert_eq!(
        wide_indexed_curve_endpoint_indices(&payload, curve_offset),
        Some([4, 2])
    );
    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[4], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["fourth", "second"]
    );
    assert_eq!(
        roster_curve_endpoint_markers(&payload, &entities[4], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["fourth", "second"]
    );
}

#[test]
fn extended_wide_104_profile_curve_uses_coordinate_roster() {
    let curve_offset = 536;
    let mut payload = vec![0; curve_offset + 104 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    for (offset, coordinate) in [
        (0, [1.0_f64, 2.0]),
        (134, [3.0_f64, 4.0]),
        (268, [5.0_f64, 6.0]),
        (402, [7.0_f64, 8.0]),
    ] {
        payload[offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
            .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 23..offset + 29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
        payload[offset + 56..offset + 58].copy_from_slice(&[0x1e, 0x00]);
        payload[offset + 58..offset + 66].copy_from_slice(&coordinate[0].to_le_bytes());
        payload[offset + 66..offset + 74].copy_from_slice(&coordinate[1].to_le_bytes());
    }
    payload[curve_offset..curve_offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[curve_offset + 5..curve_offset + 13].fill(0xff);
    payload[curve_offset + 13..curve_offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[curve_offset + 17..curve_offset + 21].copy_from_slice(&2u32.to_le_bytes());
    payload[curve_offset + 23..curve_offset + 29]
        .copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[curve_offset + 31..curve_offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[curve_offset + 48..curve_offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[curve_offset + 64..curve_offset + 66].copy_from_slice(&3u16.to_le_bytes());
    payload[curve_offset + 66..curve_offset + 68].copy_from_slice(&1u16.to_le_bytes());
    payload[curve_offset + 68..curve_offset + 72].copy_from_slice(&1u32.to_le_bytes());
    payload[curve_offset + 72..curve_offset + 80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[curve_offset + 88..curve_offset + 92].copy_from_slice(&[0x00, 0x00, 0x01, 0x00]);
    payload[curve_offset + 92..curve_offset + 96].copy_from_slice(&[0x00, 0x00, 0x01, 0x00]);
    payload[curve_offset + 100..curve_offset + 104].copy_from_slice(&1u32.to_le_bytes());
    payload[curve_offset + 104..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    let point = |id: &str, offset, coordinates_m| SketchInputEntity {
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
        id: "curve".into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: curve_offset as u64,
        object_index: None,
        local_id: None,
        kind: SketchInputKind::LineOrCircle,
        state_value: Some(1.0),
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };
    let entities = [
        point("first", 0, Some([1.0, 2.0])),
        point("second", 134, Some([3.0, 4.0])),
        point("third", 268, Some([5.0, 6.0])),
        point("fourth", 402, Some([7.0, 8.0])),
        curve,
    ];
    let markers = entities.iter().collect::<Vec<_>>();

    assert_eq!(
        wide_indexed_curve_endpoint_indices(&payload, curve_offset),
        Some([4, 2])
    );
    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[4], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["fourth", "second"]
    );

    payload[curve_offset + 92..curve_offset + 96].fill(0);
    assert_eq!(
        wide_indexed_curve_endpoint_indices(&payload, curve_offset),
        None
    );
}

#[test]
fn extended_terminal_164_wide_profile_curve_uses_coordinate_roster() {
    let curve_offset = 8 * 134;
    let mut payload = vec![0; curve_offset + 164];
    for (index, offset) in (0..8).map(|index| (index, index * 134)) {
        payload[offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
            .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 23..offset + 29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
        payload[offset + 56..offset + 58].copy_from_slice(&[0x1e, 0x00]);
        payload[offset + 58..offset + 66].copy_from_slice(&(f64::from(index as u32)).to_le_bytes());
        payload[offset + 66..offset + 74]
            .copy_from_slice(&(f64::from((index + 1) as u32)).to_le_bytes());
    }
    payload[curve_offset..curve_offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[curve_offset + 5..curve_offset + 13].fill(0xff);
    payload[curve_offset + 13..curve_offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[curve_offset + 17..curve_offset + 21].copy_from_slice(&2u32.to_le_bytes());
    payload[curve_offset + 23..curve_offset + 29]
        .copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[curve_offset + 31..curve_offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[curve_offset + 48..curve_offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[curve_offset + 64..curve_offset + 66].copy_from_slice(&5u16.to_le_bytes());
    payload[curve_offset + 66..curve_offset + 68].copy_from_slice(&7u16.to_le_bytes());
    payload[curve_offset + 68..curve_offset + 72].copy_from_slice(&1u32.to_le_bytes());
    payload[curve_offset + 72..curve_offset + 80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[curve_offset + 134..curve_offset + 136].copy_from_slice(&3u16.to_le_bytes());
    payload[curve_offset + 144..curve_offset + 148].copy_from_slice(&u32::MAX.to_le_bytes());

    let point = |id: &str, offset, coordinates_m| SketchInputEntity {
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
        id: "curve".into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: curve_offset as u64,
        object_index: None,
        local_id: None,
        kind: SketchInputKind::LineOrCircle,
        state_value: Some(1.0),
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };
    let mut entities = (0..8)
        .map(|index| {
            point(
                &format!("point{index}"),
                index * 134,
                Some([f64::from(index as u32), f64::from((index + 1) as u32)]),
            )
        })
        .collect::<Vec<_>>();
    entities.push(curve);
    let markers = entities.iter().collect::<Vec<_>>();

    assert_eq!(
        wide_indexed_curve_endpoint_indices(&payload, curve_offset),
        Some([6, 8])
    );
    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[8], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["point5", "point7"]
    );
    assert!(current_undetailed_bounded_curve_is_line(
        &payload,
        curve_offset
    ));

    payload[curve_offset + 134..curve_offset + 136].fill(0);
    assert_eq!(
        wide_indexed_curve_endpoint_indices(&payload, curve_offset),
        None
    );
}

#[test]
fn current_coordinate_circle_uses_its_complete_square_handle_grid() {
    let mut payload = vec![0; 284];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].copy_from_slice(&[0xff; 8]);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..35].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[35..39].copy_from_slice(&[0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&[0x1e, 0x00]);
    payload[82..86].copy_from_slice(&1u32.to_le_bytes());
    payload[92..96].copy_from_slice(&(-2i32).to_le_bytes());
    payload[142..142 + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    let entity = |id: &str, ordinal, offset, kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal,
        offset,
        object_index: None,
        local_id: None,
        kind,
        state_value: None,
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let center = entity("center", 0, 0, SketchInputKind::Arc, Some([2.0, 3.0]));
    let points = [
        [1.0, 2.0],
        [2.0, 2.0],
        [3.0, 2.0],
        [1.0, 4.0],
        [2.0, 4.0],
        [3.0, 4.0],
    ]
    .into_iter()
    .enumerate()
    .map(|(index, point)| {
        entity(
            &format!("point-{index}"),
            index as u32 + 1,
            index as u64 + 143,
            SketchInputKind::Point,
            Some(point),
        )
    })
    .collect::<Vec<_>>();
    let mut entities = vec![center.clone()];
    entities.extend(points);
    let markers = entities.iter().collect::<Vec<_>>();
    assert_eq!(
        coordinate_circle_radius(&payload, &center, &markers),
        Some(1.0)
    );
    entities[6].coordinates_m = Some([3.0, 5.0]);
    let markers = entities.iter().collect::<Vec<_>>();
    assert_eq!(coordinate_circle_radius(&payload, &center, &markers), None);
}

#[test]
fn legacy_coordinate_circle_uses_its_trailing_radial_point() {
    let mut payload = vec![0; 162 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&[0x1e, 0x00]);
    payload[66..74].copy_from_slice(&0.037f64.to_le_bytes());
    payload[74..82].copy_from_slice(&0.012f64.to_le_bytes());
    payload[84..86].copy_from_slice(&2u16.to_le_bytes());
    payload[86..90].copy_from_slice(&[0x19, 0x82, 0x02, 0x00]);
    payload[90..94].fill(0xff);
    payload[98..102].copy_from_slice(&[0x19, 0x82, 0x01, 0x00]);
    payload[102..106].fill(0xff);
    payload[110..116].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[158..162].copy_from_slice(&21u32.to_le_bytes());
    payload[162..].copy_from_slice(LEGACY_SKETCH_MARKER);
    let entity = |id: &str, ordinal, offset, object_index, kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("profile".into()),
        ordinal,
        offset,
        object_index,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let circle = entity(
        "circle",
        10,
        0,
        Some(20),
        SketchInputKind::Arc,
        Some([0.037, 0.012]),
    );
    let radial = entity(
        "radial",
        11,
        162,
        Some(21),
        SketchInputKind::Point,
        Some([0.049, 0.012]),
    );

    assert!(
        legacy_coordinate_circle_radius(&payload, &circle, &[&circle, &radial])
            .is_some_and(|radius| same_dimension_length(radius, 0.012))
    );
    payload[158..162].copy_from_slice(&22u32.to_le_bytes());
    assert_eq!(
        legacy_coordinate_circle_radius(&payload, &circle, &[&circle, &radial]),
        None
    );
}

#[test]
fn extended_full_circle_uses_center_and_radial_point_roster() {
    let mut payload = vec![0; 104 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[29..31].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..60].copy_from_slice(&[0x02, 0x00, 0x02, 0x00]);
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&1i32.to_le_bytes());
    payload[78..94].copy_from_slice(&[
        0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff,
        0xff,
    ]);
    payload[104..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
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
    let entities = [
        entity("center", 1, SketchInputKind::Point, Some([0.0, 0.0])),
        entity("inner", 2, SketchInputKind::Point, Some([3.0, 0.0])),
        entity("radial", 3, SketchInputKind::Point, Some([0.0, 4.0])),
        entity("circle", 0, SketchInputKind::Arc, None),
    ];
    let markers = entities.iter().collect::<Vec<_>>();

    assert_eq!(
        coordinate_roster_full_circle(&payload, &entities[3], &markers),
        Some(([0.0, 0.0], 4.0))
    );
    payload[58..60].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        coordinate_roster_full_circle(&payload, &entities[3], &markers),
        None
    );
}

#[test]
fn extended_profile_circle_accepts_one_unambiguous_radial_interpretation() {
    let mut payload = vec![0; 104 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[29..31].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..60].copy_from_slice(&[0x03, 0x00, 0x03, 0x00]);
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&1i32.to_le_bytes());
    payload[78..94].copy_from_slice(&[
        0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff,
        0xff,
    ]);
    payload[104..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    let entity = |id: &str, offset, object_index, kind, coordinates_m| SketchInputEntity {
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
    let entities = [
        entity(
            "center",
            1,
            Some(2),
            SketchInputKind::Point,
            Some([0.0, 0.0]),
        ),
        entity(
            "direct",
            2,
            Some(3),
            SketchInputKind::Point,
            Some([3.0, 0.0]),
        ),
        entity(
            "roster",
            3,
            Some(4),
            SketchInputKind::Point,
            Some([3.0, 0.0]),
        ),
        entity("circle", 0, Some(1), SketchInputKind::LineOrCircle, None),
    ];
    let markers = entities.iter().collect::<Vec<_>>();

    assert_eq!(
        super::compact_profile_full_circle(&payload, &entities[3], &markers),
        Some(([0.0, 0.0], 3.0))
    );
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[104..].copy_from_slice(SKETCH_MARKER);
    let mut current_circle = entities[3].clone();
    current_circle.kind = SketchInputKind::Arc;
    assert_eq!(
        super::compact_profile_full_circle(&payload, &current_circle, &markers),
        Some(([0.0, 0.0], 3.0))
    );
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[56..60].copy_from_slice(&[0x01, 0x00, 0x01, 0x00]);
    payload[104..].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(
        super::equal_index_coordinate_roster_full_circle(&payload, &current_circle, &markers,),
        Some(([0.0, 0.0], 3.0))
    );
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[56..60].copy_from_slice(&[0x03, 0x00, 0x03, 0x00]);
    payload[104..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    let mut conflicting = entities.clone();
    conflicting[1].coordinates_m = Some([4.0, 0.0]);
    let markers = conflicting.iter().collect::<Vec<_>>();
    assert_eq!(
        super::compact_profile_full_circle(&payload, &conflicting[3], &markers),
        None
    );
}

#[test]
fn compact_legacy_repeated_radial_records_define_full_circles() {
    let mut record = vec![0; 90 + LEGACY_SKETCH_MARKER.len()];
    record[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    record[5..13].fill(0xff);
    record[13..17].copy_from_slice(&1u32.to_le_bytes());
    record[19..25].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    record[25..27].copy_from_slice(&1u16.to_le_bytes());
    record[31] = 4;
    record[42..46].copy_from_slice(&[1, 0, 1, 0]);
    record[46..50].copy_from_slice(&1u32.to_le_bytes());
    record[50..58].copy_from_slice(&(-1.0f64).to_le_bytes());
    record[58..62].copy_from_slice(&1u32.to_le_bytes());
    for cell in record[64..80].chunks_exact_mut(4) {
        cell.copy_from_slice(&(-2i32).to_le_bytes());
    }
    record[82..86].copy_from_slice(&2u32.to_le_bytes());
    record[86..90].copy_from_slice(&3u32.to_le_bytes());
    record[90..].copy_from_slice(LEGACY_SKETCH_MARKER);
    let circle_offset = 500;
    let mut payload = vec![0; circle_offset + record.len()];
    for marker_offset in [0, 100, 200, 300] {
        payload[marker_offset..marker_offset + LEGACY_SKETCH_MARKER.len()]
            .copy_from_slice(LEGACY_SKETCH_MARKER);
        payload[marker_offset + 5..marker_offset + 13].fill(0xff);
        payload[marker_offset + 13..marker_offset + 17].copy_from_slice(&1u32.to_le_bytes());
        payload[marker_offset + 19..marker_offset + 25]
            .copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
        payload[marker_offset + 31] = 4;
    }
    payload[205..213].fill(0);
    payload[circle_offset..].copy_from_slice(&record);
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
    let entities = [
        entity("center", 0, SketchInputKind::Point, Some([0.0, 0.0])),
        entity("radial", 100, SketchInputKind::Point, Some([0.0, 12.0])),
        entity("handle", 200, SketchInputKind::Native(1), None),
        entity(
            "terminal-radial",
            300,
            SketchInputKind::Point,
            Some([0.0, 5.5]),
        ),
        entity(
            "circle",
            circle_offset as u64,
            SketchInputKind::LineOrCircle,
            None,
        ),
    ];
    let markers = entities.iter().collect::<Vec<_>>();

    assert_eq!(
        super::compact_legacy_profile_full_circle(&payload, &entities[4], &markers),
        Some(([0.0, 0.0], 12.0))
    );

    payload.resize(circle_offset + 131, 0);
    payload[circle_offset + 42..circle_offset + 46].copy_from_slice(&[3, 0, 3, 0]);
    payload[circle_offset + 82..circle_offset + 112].fill(0);
    payload[circle_offset + 112..circle_offset + 114].copy_from_slice(&4u16.to_le_bytes());
    payload[circle_offset + 114..circle_offset + 118].copy_from_slice(CLASS_MARKER);
    payload[circle_offset + 118..circle_offset + 120].copy_from_slice(&11u16.to_le_bytes());
    payload[circle_offset + 120..circle_offset + 131].copy_from_slice(b"sgCircleDim");
    assert_eq!(
        super::compact_legacy_profile_full_circle(&payload, &entities[4], &markers),
        Some(([0.0, 0.0], 5.5))
    );
    payload[circle_offset + 120] = b'x';
    assert_eq!(
        super::compact_legacy_profile_full_circle(&payload, &entities[4], &markers),
        None
    );
}

#[test]
fn compact_legacy_terminal_diameter_circle_uses_embedded_coordinate_roster() {
    let marker_size = LEGACY_SKETCH_MARKER.len();
    let circle_offset = 768;
    let mut payload = vec![0; circle_offset + 121];
    let profile_point = |payload: &mut [u8], offset: usize, coordinates: [f64; 2]| {
        payload[offset..offset + marker_size].copy_from_slice(LEGACY_SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&2u32.to_le_bytes());
        payload[offset + 19..offset + 25].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
        payload[offset + 31..offset + 42].copy_from_slice(&[0x04, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        payload[offset + 42..offset + 44].copy_from_slice(&[0x1e, 0x00]);
        payload[offset + 44..offset + 52].copy_from_slice(&coordinates[0].to_le_bytes());
        payload[offset + 52..offset + 60].copy_from_slice(&coordinates[1].to_le_bytes());
        payload[offset + 62..offset + 64].copy_from_slice(&4u16.to_le_bytes());
        for (relative, id) in [(64, 8u16), (72, 11u16)] {
            payload[offset + relative..offset + relative + 2]
                .copy_from_slice(&0x811au16.to_le_bytes());
            payload[offset + relative + 2..offset + relative + 4]
                .copy_from_slice(&id.to_le_bytes());
            payload[offset + relative + 4..offset + relative + 8].fill(0xff);
        }
        payload[offset + 80..offset + 86].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
        payload[offset + 120..offset + 124].copy_from_slice(&2u32.to_le_bytes());
        payload[offset + 128..offset + 132].copy_from_slice(&10u32.to_le_bytes());
    };
    let embedded_geometry = |payload: &mut [u8], offset: usize, coordinates: [f64; 2]| {
        payload[offset..offset + marker_size].copy_from_slice(LEGACY_SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 19..offset + 25].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
        payload[offset + 31..offset + 42].copy_from_slice(&[0x05, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        payload[offset + 42..offset + 44].copy_from_slice(&[0x1e, 0x00]);
        payload[offset + 44..offset + 52].copy_from_slice(&coordinates[0].to_le_bytes());
        payload[offset + 52..offset + 60].copy_from_slice(&coordinates[1].to_le_bytes());
        payload[offset + 70..offset + 74].copy_from_slice(&[0xfe, 0xff, 0xff, 0xff]);
        payload[offset + 116..offset + 120].copy_from_slice(&12u32.to_le_bytes());
    };
    for (offset, coordinates) in [
        (0, [0.03, 0.005]),
        (132, [-0.03, 0.005]),
        (384, [-0.03, -0.005]),
        (516, [0.03, -0.0048]),
    ] {
        profile_point(&mut payload, offset, coordinates);
    }
    embedded_geometry(&mut payload, 264, [0.03, 0.005]);
    embedded_geometry(&mut payload, 648, [-0.03, 0.005]);

    payload[circle_offset..circle_offset + marker_size].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[circle_offset + 5..circle_offset + 13].fill(0xff);
    payload[circle_offset + 13..circle_offset + 17].copy_from_slice(&1u32.to_le_bytes());
    payload[circle_offset + 19..circle_offset + 25]
        .copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[circle_offset + 25..circle_offset + 27].copy_from_slice(&1u16.to_le_bytes());
    payload[circle_offset + 31..circle_offset + 42]
        .copy_from_slice(&[0x04, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    payload[circle_offset + 42..circle_offset + 44].copy_from_slice(&4u16.to_le_bytes());
    payload[circle_offset + 46..circle_offset + 50].copy_from_slice(&1u32.to_le_bytes());
    payload[circle_offset + 50..circle_offset + 58].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[circle_offset + 102..circle_offset + 104].copy_from_slice(&3u16.to_le_bytes());
    payload[circle_offset + 104..circle_offset + 108].copy_from_slice(CLASS_MARKER);
    payload[circle_offset + 108..circle_offset + 110].copy_from_slice(&11u16.to_le_bytes());
    payload[circle_offset + 110..circle_offset + 121].copy_from_slice(b"sgCircleDim");

    let entity = |id: &str, offset: u64, kind, coordinates_m| SketchInputEntity {
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
    let entities = [
        entity("center", 0, SketchInputKind::Point, Some([0.03, 0.005])),
        entity("second", 132, SketchInputKind::Point, Some([-0.03, 0.005])),
        entity("third", 384, SketchInputKind::Point, Some([-0.03, -0.005])),
        entity("radial", 516, SketchInputKind::Point, Some([0.03, -0.0048])),
        entity(
            "circle",
            circle_offset as u64,
            SketchInputKind::LineOrCircle,
            None,
        ),
    ];
    let markers = entities.iter().collect::<Vec<_>>();
    let Some((center, radius)) =
        super::compact_legacy_terminal_diameter_circle(&payload, &entities[4], &markers)
    else {
        panic!("terminal circle did not resolve");
    };
    assert_eq!(center, [0.03, 0.005]);
    assert!((radius - 0.0098).abs() < 1e-12);

    payload[circle_offset + 44..circle_offset + 46].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        super::compact_legacy_terminal_diameter_circle(&payload, &entities[4], &markers),
        None
    );
}

#[test]
fn packed_compact_legacy_curves_use_the_coordinate_roster() {
    let mut payload = vec![0; 76 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[19..25].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[29] = 5;
    payload[40..48].copy_from_slice(&1.0f64.to_le_bytes());
    payload[48..52].copy_from_slice(&[1, 0, 2, 0]);
    payload[56..64].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[66..68].copy_from_slice(&1u16.to_le_bytes());
    payload[72..76].copy_from_slice(&3u32.to_le_bytes());
    payload[76..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        super::packed_compact_legacy_curve_endpoint_indices(&payload, 0),
        Some([1, 2])
    );
    assert_eq!(
        super::coordinate_roster_endpoint_offset(&payload, 0),
        Some(48)
    );
    assert!(super::legacy_undetailed_profile_line(&payload, 0));
    assert!(!super::marker_is_selected_construction_line(&payload, 0));

    payload[13..17].copy_from_slice(&1u32.to_le_bytes());
    payload[23..25].copy_from_slice(&2u16.to_le_bytes());
    payload[29] = 12;
    payload[66..68].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(
        super::packed_compact_legacy_curve_endpoint_indices(&payload, 0),
        Some([1, 2])
    );
    assert!(!super::legacy_undetailed_profile_line(&payload, 0));
    assert!(super::marker_is_selected_construction_line(&payload, 0));

    payload[68] = 1;
    assert_eq!(
        super::packed_compact_legacy_curve_endpoint_indices(&payload, 0),
        None
    );
}

#[test]
fn sole_out_of_roster_packed_curve_closes_one_open_profile_chain() {
    let mut payload = vec![0; 328 + LEGACY_SKETCH_MARKER.len()];
    for point_offset in [0, 10, 20] {
        payload[point_offset..point_offset + LEGACY_SKETCH_MARKER.len()]
            .copy_from_slice(LEGACY_SKETCH_MARKER);
    }
    for (curve_offset, endpoints, identity) in [
        (100, [0u16, 1], 1u32),
        (176, [1u16, 2], 2),
        (252, [3u16, 4], 3),
    ] {
        payload[curve_offset..curve_offset + LEGACY_SKETCH_MARKER.len()]
            .copy_from_slice(LEGACY_SKETCH_MARKER);
        payload[curve_offset + 5..curve_offset + 13].fill(0xff);
        payload[curve_offset + 19..curve_offset + 25]
            .copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
        payload[curve_offset + 29] = 5;
        payload[curve_offset + 40..curve_offset + 48].copy_from_slice(&1.0f64.to_le_bytes());
        payload[curve_offset + 48..curve_offset + 50].copy_from_slice(&endpoints[0].to_le_bytes());
        payload[curve_offset + 50..curve_offset + 52].copy_from_slice(&endpoints[1].to_le_bytes());
        payload[curve_offset + 56..curve_offset + 64].copy_from_slice(&(-1.0f64).to_le_bytes());
        payload[curve_offset + 72..curve_offset + 76].copy_from_slice(&identity.to_le_bytes());
    }
    payload[328..].copy_from_slice(LEGACY_SKETCH_MARKER);
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
    let entities = [
        entity("point-0", 0, SketchInputKind::Point, Some([0.0, 0.0])),
        entity("point-1", 10, SketchInputKind::Point, Some([1.0, 0.0])),
        entity("point-2", 20, SketchInputKind::Point, Some([1.0, 1.0])),
        entity("line-0", 100, SketchInputKind::LineOrCircle, None),
        entity("line-1", 176, SketchInputKind::LineOrCircle, None),
        entity("closure", 252, SketchInputKind::LineOrCircle, None),
    ];
    let markers = entities.iter().collect::<Vec<_>>();

    assert_eq!(
        super::implicit_profile_chain_closure_endpoints(&payload, &entities[5], &markers),
        Some([[0.0, 0.0], [1.0, 1.0]])
    );

    payload[176 + 48..176 + 52].copy_from_slice(&[0, 0, 1, 0]);
    assert_eq!(
        super::implicit_profile_chain_closure_endpoints(&payload, &entities[5], &markers),
        None
    );
}

#[test]
fn equal_index_coordinate_roster_carries_center_and_following_radial_point() {
    let mut payload = vec![0; 104 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[29..31].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x01, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..60].copy_from_slice(&[2, 0, 2, 0]);
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&1u32.to_le_bytes());
    for cell in payload[78..94].chunks_exact_mut(4) {
        cell.copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[104..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    let marker = |id: &str, offset, coordinates_m, kind| SketchInputEntity {
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
    let circle = marker("circle", 0, None, SketchInputKind::Arc);
    let points = [
        marker("first", 10, Some([0.0, 0.0]), SketchInputKind::Point),
        marker("center", 20, Some([1.0, 1.0]), SketchInputKind::Point),
        marker("radial", 30, Some([1.0, 3.0]), SketchInputKind::Point),
    ];
    let markers = std::iter::once(&circle)
        .chain(points.iter())
        .collect::<Vec<_>>();

    assert_eq!(
        equal_index_coordinate_roster_full_circle(&payload, &circle, &markers),
        Some(([1.0, 1.0], 2.0))
    );
    payload[35..39].copy_from_slice(&[0x00, 0x00, 0x04, 0x00]);
    assert_eq!(
        equal_index_coordinate_roster_full_circle(&payload, &circle, &markers),
        Some(([1.0, 1.0], 2.0))
    );
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[104..104 + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    assert_eq!(
        equal_index_coordinate_roster_full_circle(&payload, &circle, &markers),
        Some(([1.0, 1.0], 2.0))
    );
}

#[test]
fn dimensioned_extended_full_circle_uses_center_and_radial_point_roster() {
    let mut payload = vec![0; 72];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[29..31].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..60].copy_from_slice(&[0x02, 0x00, 0x02, 0x00]);
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload.extend_from_slice(CLASS_MARKER);
    payload.extend_from_slice(&11u16.to_le_bytes());
    payload.extend_from_slice(b"moDimText_c");
    let marker = |id: &str, offset, coordinates_m, kind| SketchInputEntity {
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
    let circle = marker("circle", 0, None, SketchInputKind::Arc);
    let points = [
        marker("first", 10, Some([0.0, 0.0]), SketchInputKind::Point),
        marker("center", 20, Some([1.0, 1.0]), SketchInputKind::Point),
        marker("radial", 30, Some([1.0, 3.0]), SketchInputKind::Point),
    ];
    let markers = std::iter::once(&circle)
        .chain(points.iter())
        .collect::<Vec<_>>();

    assert_eq!(
        equal_index_coordinate_roster_full_circle(&payload, &circle, &markers),
        Some(([1.0, 1.0], 2.0))
    );
    let mut tagged = payload[..72].to_vec();
    tagged.extend_from_slice(&[0x1d, 0x81, 0xff, 0xfe, 0xff, 0x02, 0x4d, 0x00]);
    tagged.extend_from_slice(&[0x35, 0x00]);
    tagged.extend_from_slice(&[0; 16]);
    tagged.extend_from_slice(&[0; 8]);
    tagged.extend_from_slice(&1u32.to_le_bytes());
    tagged.extend_from_slice(&[0x1f, 0x81, 0xff, 0xfe, 0xff, 0x06]);
    assert_eq!(
        equal_index_coordinate_roster_full_circle(&tagged, &circle, &markers),
        Some(([1.0, 1.0], 2.0))
    );
    tagged[80] = 0x34;
    assert_eq!(
        equal_index_coordinate_roster_full_circle(&tagged, &circle, &markers),
        Some(([1.0, 1.0], 2.0))
    );
    tagged[72] = 0;
    assert_eq!(
        equal_index_coordinate_roster_full_circle(&tagged, &circle, &markers),
        None
    );
    payload[72] = 0;
    assert_eq!(
        equal_index_coordinate_roster_full_circle(&payload, &circle, &markers),
        None
    );
}

#[test]
fn wide_legacy_full_circle_uses_adjacent_center_and_radial_markers() {
    let mut payload = vec![0; 112 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[29..31].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..68].copy_from_slice(&[0x02, 0x00, 0x02, 0x00]);
    payload[68..72].copy_from_slice(&1u32.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[80..84].copy_from_slice(&1i32.to_le_bytes());
    payload[84..86].copy_from_slice(&4u16.to_le_bytes());
    payload[86..102].copy_from_slice(&[
        0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff,
        0xff,
    ]);
    payload[104..108].copy_from_slice(&6u32.to_le_bytes());
    payload[108..112].copy_from_slice(&3u32.to_le_bytes());
    payload[112..].copy_from_slice(LEGACY_SKETCH_MARKER);
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
    let entities = [
        entity("unrelated", 1, SketchInputKind::Point, Some([9.0, 9.0])),
        entity("center", 2, SketchInputKind::Arc, Some([2.0, 3.0])),
        entity("radial", 3, SketchInputKind::Point, Some([5.0, 7.0])),
        entity("circle", 0, SketchInputKind::LineOrCircle, None),
    ];
    let markers = entities.iter().collect::<Vec<_>>();

    assert_eq!(
        super::wide_coordinate_roster_full_circle(&payload, &entities[3], &markers),
        Some(([2.0, 3.0], 5.0))
    );
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[112..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    let mut extended_circle = entities[3].clone();
    extended_circle.kind = SketchInputKind::LineOrCircle;
    assert_eq!(
        super::wide_coordinate_roster_full_circle(&payload, &extended_circle, &markers),
        Some(([2.0, 3.0], 5.0))
    );
    payload[104..108].copy_from_slice(&3u32.to_le_bytes());
    assert_eq!(
        super::wide_coordinate_roster_full_circle(&payload, &extended_circle, &markers),
        Some(([2.0, 3.0], 5.0))
    );
    payload[104..108].copy_from_slice(&6u32.to_le_bytes());
    let mut terminal = payload[..102].to_vec();
    terminal.resize(153, 0);
    terminal[134..136].copy_from_slice(&[0x04, 0x00]);
    terminal[136..140].copy_from_slice(CLASS_MARKER);
    terminal[140..142].copy_from_slice(&11u16.to_le_bytes());
    terminal[142..153].copy_from_slice(b"sgCircleDim");
    terminal[64..68].copy_from_slice(&[0x03, 0x00, 0x03, 0x00]);
    let mut terminal_entities = entities.clone();
    terminal_entities[0].kind = SketchInputKind::Relation(SketchRelationKind::Horizontal);
    let terminal_markers = terminal_entities.iter().collect::<Vec<_>>();
    assert_eq!(
        super::wide_coordinate_roster_full_circle(&terminal, &extended_circle, &terminal_markers,),
        Some(([2.0, 3.0], 5.0))
    );
    terminal[64..68].copy_from_slice(&[0x01, 0x00, 0x01, 0x00]);
    terminal[84..86].copy_from_slice(&[0; 2]);
    let mut direct_entities = entities.clone();
    direct_entities[2].object_index = Some(1);
    let direct_markers = direct_entities.iter().collect::<Vec<_>>();
    assert_eq!(
        super::wide_coordinate_roster_full_circle(&terminal, &extended_circle, &direct_markers,),
        Some(([2.0, 3.0], 5.0))
    );
    let mut short_terminal = payload[..102].to_vec();
    short_terminal.resize(145, 0);
    short_terminal[64..68].copy_from_slice(&[0x01, 0x00, 0x01, 0x00]);
    short_terminal[84..86].copy_from_slice(&[0; 2]);
    short_terminal[128..132].copy_from_slice(CLASS_MARKER);
    short_terminal[132..134].copy_from_slice(&11u16.to_le_bytes());
    short_terminal[134..145].copy_from_slice(b"sgCircleDim");
    assert_eq!(
        super::wide_coordinate_roster_full_circle(
            &short_terminal,
            &extended_circle,
            &direct_markers,
        ),
        Some(([2.0, 3.0], 5.0))
    );
    terminal[133] = 1;
    assert_eq!(
        super::wide_coordinate_roster_full_circle(&terminal, &extended_circle, &terminal_markers,),
        None
    );
    payload[66..68].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(
        super::wide_coordinate_roster_full_circle(&payload, &entities[3], &markers),
        None
    );
}

#[test]
fn legacy_profile_radial_circle_requires_one_selected_radial_locus() {
    let mut payload = vec![0; 112 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[29..31].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..68].copy_from_slice(&[0x01, 0x00, 0x01, 0x00]);
    payload[68..72].copy_from_slice(&1u32.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[80..84].copy_from_slice(&1i32.to_le_bytes());
    payload[86..102].copy_from_slice(&[
        0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff,
        0xff,
    ]);
    payload[104..108].copy_from_slice(&2u32.to_le_bytes());
    payload[108..112].copy_from_slice(&2u32.to_le_bytes());
    payload[112..].copy_from_slice(LEGACY_SKETCH_MARKER);
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
    let entities = [
        entity("center", 1, SketchInputKind::LineOrCircle, Some([0.0, 0.0])),
        entity("radial", 2, SketchInputKind::Point, Some([3.0, 0.0])),
        entity("other", 3, SketchInputKind::Point, Some([0.0, 4.0])),
        entity("circle", 0, SketchInputKind::LineOrCircle, None),
    ];
    let markers = entities.iter().collect::<Vec<_>>();

    assert_eq!(
        super::legacy_profile_radial_circle(&payload, &entities[3], &markers),
        Some(([0.0, 0.0], 3.0))
    );
    payload[64..68].copy_from_slice(&[0x02, 0x00, 0x02, 0x00]);
    assert_eq!(
        super::legacy_profile_radial_circle(&payload, &entities[3], &markers),
        None
    );

    payload.resize(128, 0);
    payload[64..68].copy_from_slice(&[0x03, 0x00, 0x03, 0x00]);
    payload[104..128].fill(0);
    assert_eq!(
        super::legacy_profile_radial_circle(&payload, &entities[3], &markers),
        Some(([0.0, 0.0], 4.0))
    );
}

#[test]
fn extended_coordinate_ellipse_uses_its_complete_corner_grid() {
    let mut payload = vec![0; 134 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[134..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    let entity = |id: &str, ordinal, offset, kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal,
        offset,
        object_index: None,
        local_id: None,
        kind,
        state_value: None,
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let ellipse = entity("ellipse", 0, 0, SketchInputKind::Arc, Some([2.0, 3.0]));
    let points = [
        [-2.0, 2.0],
        [-2.0, 4.0],
        [6.0, 2.0],
        [6.0 + f64::EPSILON * 4.0, 4.0],
    ]
    .into_iter()
    .enumerate()
    .map(|(index, point)| {
        entity(
            &format!("point-{index}"),
            index as u32 + 1,
            index as u64 + 134,
            SketchInputKind::Point,
            Some(point),
        )
    })
    .collect::<Vec<_>>();
    let mut entities = vec![ellipse.clone()];
    entities.extend(points);
    let markers = entities.iter().collect::<Vec<_>>();
    assert!(
        super::coordinate_ellipse_axes(&payload, &ellipse, &markers).is_some_and(
            |(axis, major, minor)| {
                axis == [1.0, 0.0]
                    && same_dimension_length(major, 4.0)
                    && same_dimension_length(minor, 1.0)
            }
        )
    );

    entities[4].coordinates_m = Some([6.0, 5.0]);
    let markers = entities.iter().collect::<Vec<_>>();
    assert_eq!(
        super::coordinate_ellipse_axes(&payload, &ellipse, &markers),
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

#[test]
fn legacy_long_profile_line_uses_point_object_ids() {
    let mut payload = vec![0; 124];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[19..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..33].copy_from_slice(&4u16.to_le_bytes());
    payload[42..44].copy_from_slice(&6u16.to_le_bytes());
    payload[44..46].copy_from_slice(&8u16.to_le_bytes());
    payload[46..50].copy_from_slice(&1u32.to_le_bytes());
    payload[50..58].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[58..62].copy_from_slice(&1u32.to_le_bytes());
    payload[62..64].copy_from_slice(&7u16.to_le_bytes());
    for relative in [64, 68, 72, 76] {
        payload[relative..relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[120..124].copy_from_slice(&16u32.to_le_bytes());

    assert_eq!(
        legacy_long_profile_line_endpoint_indices(&payload, 0),
        Some([6, 8])
    );

    payload[120..124].fill(0);
    assert_eq!(legacy_long_profile_line_endpoint_indices(&payload, 0), None);
}

#[test]
fn current_long_full_circle_indexes_its_radial_point() {
    let mut payload = vec![0; 154];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..68].copy_from_slice(&[1, 0, 1, 0]);
    payload[68..72].copy_from_slice(&1u32.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[80..84].copy_from_slice(&1u32.to_le_bytes());
    for relative in [86, 90, 94, 98] {
        payload[relative..relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[134..136].copy_from_slice(&4u16.to_le_bytes());
    payload[136..154].copy_from_slice(&[
        0xf1, 0x80, 0x00, 0x00, 0x00, 0x00, 0xf3, 0x80, 0x04, 0x80, 0xff, 0xfe, 0xff, 0x02, 0x44,
        0x00, 0x31, 0x00,
    ]);

    assert_eq!(current_long_full_circle_radial_index(&payload, 0), Some(1));

    payload[66..68].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(current_long_full_circle_radial_index(&payload, 0), None);
}

#[test]
fn extended_wide_construction_line_indexes_the_complete_marker_roster() {
    let mut payload = vec![0; 92 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x02, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&14u16.to_le_bytes());
    payload[66..68].copy_from_slice(&15u16.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[84..88].copy_from_slice(&1u32.to_le_bytes());
    payload[88..92].copy_from_slice(&5u32.to_le_bytes());
    payload[92..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    assert_eq!(
        extended_wide_construction_line_roster_indices(&payload, 0),
        Some([14, 15])
    );
    payload[66..68].copy_from_slice(&14u16.to_le_bytes());
    assert_eq!(
        extended_wide_construction_line_roster_indices(&payload, 0),
        None
    );
    payload[66..68].copy_from_slice(&15u16.to_le_bytes());
    payload[82] = 1;
    payload[84..88].fill(0);
    assert_eq!(
        extended_wide_construction_line_roster_indices(&payload, 0),
        Some([14, 15])
    );
    payload[82] = 2;
    assert_eq!(
        extended_wide_construction_line_roster_indices(&payload, 0),
        None
    );
    payload[82] = 0;
    assert_eq!(
        extended_wide_construction_line_roster_indices(&payload, 0),
        None
    );
}

#[test]
fn extended_geometry_locus_construction_line_uses_direct_point_object_ids() {
    let mut payload = vec![0; 96 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0x04, 0x00, 0xff, 0xff]);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&0u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x02, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&13u16.to_le_bytes());
    payload[58..60].copy_from_slice(&14u16.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[80..84].copy_from_slice(&[0x00, 0x00, 0x04, 0x00]);
    payload[88..92].copy_from_slice(&2u32.to_le_bytes());
    payload[92..96].copy_from_slice(&3u32.to_le_bytes());
    payload[96..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    assert_eq!(
        extended_geometry_locus_construction_line_endpoint_indices(&payload, 0),
        Some([13, 14])
    );
    assert!(marker_is_selected_construction_line(&payload, 0));

    let entity = |id: &str, offset, object_index, kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset,
        object_index: Some(object_index),
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let curve = entity("curve", 0, 8, SketchInputKind::LineOrCircle, None);
    let first = entity("first", 100, 13, SketchInputKind::Point, Some([0.0, 0.0]));
    let second = entity("second", 200, 14, SketchInputKind::Point, Some([1.0, 0.0]));
    let markers = [&curve, &first, &second];
    assert_eq!(
        super::roster_curve_endpoint_markers(&payload, &curve, &markers)
            .into_iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );

    payload[58..60].copy_from_slice(&13u16.to_le_bytes());
    assert_eq!(
        extended_geometry_locus_construction_line_endpoint_indices(&payload, 0),
        None
    );
    assert!(!marker_is_selected_construction_line(&payload, 0));
}

#[test]
fn terminal_legacy_wide_curve_indexes_the_coordinate_roster() {
    let mut payload = vec![0; 128];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&12u16.to_le_bytes());
    payload[66..68].copy_from_slice(&13u16.to_le_bytes());
    payload[68..72].copy_from_slice(&1u32.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());

    assert_eq!(
        super::wide_indexed_curve_endpoint_indices(&payload, 0),
        Some([13, 14])
    );
    assert_eq!(
        super::coordinate_roster_endpoint_offset(&payload, 0),
        Some(64)
    );

    payload[127] = 1;
    assert_eq!(
        super::wide_indexed_curve_endpoint_indices(&payload, 0),
        None
    );
}

#[test]
fn terminal_legacy_profile_curve_addresses_consecutive_point_identities() {
    let mut wide = vec![0; 92 + LEGACY_SKETCH_MARKER.len()];
    wide[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    wide[5..13].fill(0xff);
    wide[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    wide[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    wide[27..29].copy_from_slice(&1u16.to_le_bytes());
    wide[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x05, 0x00]);
    wide[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    wide[64..66].copy_from_slice(&15u16.to_le_bytes());
    wide[66..68].copy_from_slice(&16u16.to_le_bytes());
    wide[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    wide[80..84].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
    wide[84..88].copy_from_slice(&9u32.to_le_bytes());
    wide[88..92].copy_from_slice(&12u32.to_le_bytes());
    wide[92..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(legacy_terminal_profile_endpoint_offset(&wide, 0), Some(64));
    assert_eq!(legacy_state_five_curve_endpoint_indices(&wide, 0), None);
    assert!(legacy_undetailed_profile_line(&wide, 0));
    let mut compact = wide;
    compact.copy_within(64..84, 56);
    compact.truncate(84 + LEGACY_SKETCH_MARKER.len());
    compact[76..80].copy_from_slice(&9u32.to_le_bytes());
    compact[80..84].copy_from_slice(&12u32.to_le_bytes());
    compact[84..].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(
        legacy_terminal_profile_endpoint_offset(&compact, 0),
        Some(56)
    );
    assert_eq!(legacy_state_five_curve_endpoint_indices(&compact, 0), None);
    assert!(legacy_undetailed_profile_line(&compact, 0));

    compact[72..76].fill(0);
    assert_eq!(legacy_terminal_profile_endpoint_offset(&compact, 0), None);
}

#[test]
fn unlocated_legacy_geometry_handle_has_no_neutral_geometry() {
    let mut payload = vec![0; 142 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x05, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&[0x12, 0x00]);
    payload[92..96].copy_from_slice(&[0xfe, 0xff, 0xff, 0xff]);
    payload[142..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert!(legacy_unlocated_geometry_handle(&payload, 0));
    payload[92] = 0;
    assert!(!legacy_unlocated_geometry_handle(&payload, 0));
}

#[test]
fn compact_legacy_profile_selected_axis_indexes_the_coordinate_roster() {
    let mut payload = vec![0; 92 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x02, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&7u16.to_le_bytes());
    payload[66..68].copy_from_slice(&8u16.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[80..84].copy_from_slice(&[0x00, 0x00, 0x01, 0x00]);
    payload[92..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        super::legacy_profile_roster_selected_axis_endpoint_indices(&payload, 0),
        Some([8, 9])
    );
    assert!(marker_is_selected_construction_line(&payload, 0));
    payload[80..84].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
    assert_eq!(
        super::legacy_profile_roster_selected_axis_endpoint_indices(&payload, 0),
        Some([8, 9])
    );
    assert!(marker_is_selected_construction_line(&payload, 0));
    payload[80..84].fill(0);
    assert_eq!(
        super::legacy_profile_roster_selected_axis_endpoint_indices(&payload, 0),
        None
    );
    payload[84..88].copy_from_slice(&1u32.to_le_bytes());
    payload[88..92].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        super::legacy_profile_roster_selected_axis_endpoint_indices(&payload, 0),
        Some([8, 9])
    );
    assert!(marker_is_selected_construction_line(&payload, 0));
    payload[88..92].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        super::legacy_profile_roster_selected_axis_endpoint_indices(&payload, 0),
        None
    );

    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[80..84].fill(0);
    payload[84..88].fill(0);
    payload[88..92].copy_from_slice(&29u32.to_le_bytes());
    assert_eq!(
        super::legacy_profile_roster_selected_axis_endpoint_indices(&payload, 0),
        Some([8, 9])
    );
    assert!(marker_is_selected_construction_line(&payload, 0));
    payload[88..92].fill(0);
    assert_eq!(
        super::legacy_profile_roster_selected_axis_endpoint_indices(&payload, 0),
        None
    );
}

#[test]
fn standard_legacy_compact_selected_axis_indexes_the_coordinate_roster() {
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
    payload[58..60].copy_from_slice(&8u16.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[74..76].copy_from_slice(&2u16.to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        super::standard_legacy_compact_selected_axis_endpoint_indices(&payload, 0),
        Some([8, 9])
    );
    assert!(marker_is_selected_construction_line(&payload, 0));
}

#[test]
fn compact_legacy_selected_axis_distinguishes_direct_and_roster_ids() {
    let mut payload = vec![0; 84 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x02, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&2u16.to_le_bytes());
    payload[58..60].copy_from_slice(&3u16.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[74] = 1;
    payload[80..84].copy_from_slice(&2u32.to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        legacy_direct_compact_selected_axis_endpoint_indices(&payload, 0),
        Some([2, 3])
    );
    payload[72..76].fill(0);
    payload[76..80].copy_from_slice(&1u32.to_le_bytes());
    payload[80..84].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        legacy_direct_compact_selected_axis_endpoint_indices(&payload, 0),
        None
    );
    assert_eq!(
        super::legacy_compact_roster_selected_axis_endpoint_indices(&payload, 0),
        Some([3, 4])
    );
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(56));
    assert!(marker_is_selected_construction_line(&payload, 0));
    payload[80..84].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        super::legacy_compact_roster_selected_axis_endpoint_indices(&payload, 0),
        None
    );
    payload[80..84].copy_from_slice(&1u32.to_le_bytes());
    payload[58..60].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(
        super::legacy_compact_roster_selected_axis_endpoint_indices(&payload, 0),
        None
    );
}

#[test]
fn legacy_code_six_axis_excludes_role_two_code_three_chords() {
    let mut payload = vec![0; 92 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&6u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x02, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&7u16.to_le_bytes());
    payload[66..68].copy_from_slice(&8u16.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[80..84].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
    payload[88..92].copy_from_slice(&2u32.to_le_bytes());
    payload[92..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        super::legacy_code_five_or_six_selected_axis_endpoint_indices(&payload, 0),
        Some([8, 9])
    );
    assert!(marker_is_selected_construction_line(&payload, 0));
    payload[17..21].copy_from_slice(&3u32.to_le_bytes());
    assert_eq!(
        super::legacy_code_five_or_six_selected_axis_endpoint_indices(&payload, 0),
        None
    );
    assert!(!marker_is_selected_construction_line(&payload, 0));
}

#[test]
fn legacy_code_five_axis_requires_distinct_trailing_identities() {
    let mut payload = vec![0; 92 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&5u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x02, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&7u16.to_le_bytes());
    payload[66..68].copy_from_slice(&8u16.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[84..88].copy_from_slice(&1u32.to_le_bytes());
    payload[88..92].copy_from_slice(&3u32.to_le_bytes());
    payload[92..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        super::legacy_code_five_or_six_selected_axis_endpoint_indices(&payload, 0),
        Some([8, 9])
    );
    assert!(marker_is_selected_construction_line(&payload, 0));
    payload[88..92].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        super::legacy_code_five_or_six_selected_axis_endpoint_indices(&payload, 0),
        None
    );
    assert!(!marker_is_selected_construction_line(&payload, 0));
}

#[test]
fn compact_legacy_state_five_line_indexes_the_coordinate_roster() {
    let mut payload = vec![0; 92 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x05, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&6u16.to_le_bytes());
    payload[66..68].copy_from_slice(&9u16.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[92..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        legacy_state_five_curve_endpoint_indices(&payload, 0),
        Some([7, 10])
    );
    assert!(legacy_undetailed_profile_line(&payload, 0));
    payload[68..70].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        legacy_state_five_curve_endpoint_indices(&payload, 0),
        Some([7, 10])
    );
    payload[70..72].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(legacy_state_five_curve_endpoint_indices(&payload, 0), None);
    payload[68..72].fill(0);

    let mut compact = vec![0; 84 + LEGACY_SKETCH_MARKER.len()];
    compact[..56].copy_from_slice(&payload[..56]);
    compact[56..58].copy_from_slice(&6u16.to_le_bytes());
    compact[58..60].copy_from_slice(&9u16.to_le_bytes());
    compact[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    compact[84..].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(
        legacy_state_five_curve_endpoint_indices(&compact, 0),
        Some([7, 10])
    );
    assert!(legacy_undetailed_profile_line(&compact, 0));
    compact[74..76].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        legacy_state_five_curve_endpoint_indices(&compact, 0),
        Some([7, 10])
    );
    compact[74..76].fill(0);
    compact[60..64].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        legacy_state_five_curve_endpoint_indices(&compact, 0),
        Some([7, 10])
    );
    compact[74..76].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(
        legacy_state_five_curve_endpoint_indices(&compact, 0),
        Some([7, 10])
    );
}

#[test]
fn terminal_compact_indexed_curve_owns_its_endpoint_trailer() {
    let mut payload = vec![0; 102];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&7u16.to_le_bytes());
    payload[58..60].copy_from_slice(&9u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&u32::MAX.to_le_bytes());
    payload[76..78].copy_from_slice(&8u16.to_le_bytes());
    for at in (78..94).step_by(4) {
        payload[at..at + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }

    assert_eq!(
        compact_indexed_curve_endpoint_indices(&payload, 0),
        Some([8, 10])
    );

    payload[90] = 0;
    assert_eq!(compact_indexed_curve_endpoint_indices(&payload, 0), None);
}

#[test]
fn extended_compact_indexed_curves_own_their_endpoint_trailers() {
    let marker = |size: usize| {
        let mut payload = vec![0; size + LEGACY_SKETCH_MARKER.len()];
        payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
        payload[5..13].fill(0xff);
        payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
        payload[27..29].copy_from_slice(&1u16.to_le_bytes());
        payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
        payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[56..58].copy_from_slice(&4u16.to_le_bytes());
        payload[58..60].copy_from_slice(&8u16.to_le_bytes());
        payload[60..64].copy_from_slice(&1u32.to_le_bytes());
        payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
        payload[size..].copy_from_slice(LEGACY_SKETCH_MARKER);
        payload
    };

    let mut compact_96 = marker(96);
    compact_96[82..84].copy_from_slice(&3u16.to_le_bytes());
    compact_96[88..92].copy_from_slice(&4u32.to_le_bytes());
    compact_96[92..96].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        compact_indexed_curve_endpoint_indices(&compact_96, 0),
        Some([5, 9])
    );
    let valid_compact_96 = compact_96.clone();
    compact_96[84] = 1;
    assert_eq!(compact_indexed_curve_endpoint_indices(&compact_96, 0), None);

    let mut compact_104 = marker(104);
    compact_104[72..76].copy_from_slice(&(-1i32).to_le_bytes());
    compact_104[76..78].copy_from_slice(&5u16.to_le_bytes());
    for at in (78..94).step_by(4) {
        compact_104[at..at + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    compact_104[96..100].copy_from_slice(&6u32.to_le_bytes());
    compact_104[100..104].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        compact_indexed_curve_endpoint_indices(&compact_104, 0),
        Some([5, 9])
    );
    let valid_compact_104 = compact_104.clone();
    compact_104[94] = 1;
    assert_eq!(
        compact_indexed_curve_endpoint_indices(&compact_104, 0),
        None
    );
    let mut current_compact_104 = valid_compact_104.clone();
    current_compact_104[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    current_compact_104[17..21].copy_from_slice(&2u32.to_le_bytes());
    current_compact_104[104..].copy_from_slice(SKETCH_MARKER);
    assert!(current_undetailed_bounded_curve_is_line(
        &current_compact_104,
        0
    ));
    current_compact_104[58..60].copy_from_slice(&4u16.to_le_bytes());
    assert!(!current_undetailed_bounded_curve_is_line(
        &current_compact_104,
        0
    ));

    let extended = |mut payload: Vec<u8>| {
        payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()]
            .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
        let size = payload.len() - LEGACY_SKETCH_MARKER.len();
        payload[size..size + LEGACY_EXTENDED_SKETCH_MARKER.len()]
            .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
        payload
    };
    let mut extended_code_one_104 = extended(valid_compact_104.clone());
    extended_code_one_104[17..21].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        super::extended_compact_indexed_curve_endpoint_indices(&extended_code_one_104, 0),
        Some([5, 9])
    );
    let entity = |id: &str, offset, object_index, coordinates_m, kind| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("profile".into()),
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
    let curve = entity("curve", 0, None, None, SketchInputKind::LineOrCircle);
    let start = entity(
        "start",
        1,
        Some(5),
        Some([0.0, 0.0]),
        SketchInputKind::Point,
    );
    let end = entity("end", 2, Some(9), Some([1.0, 0.0]), SketchInputKind::Point);
    let markers = [&curve, &start, &end];
    assert_eq!(
        roster_curve_endpoint_markers(&extended_code_one_104, &curve, &markers)
            .into_iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        vec!["start", "end"]
    );
    let normalized_payload = extended(valid_compact_96);
    assert_eq!(
        super::extended_compact_indexed_curve_endpoint_indices(&normalized_payload, 0),
        Some([5, 9])
    );
    assert!(current_undetailed_bounded_curve_is_line(
        &normalized_payload,
        0
    ));
    let entity = |id: &str, offset, object_index, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("profile".into()),
        ordinal: 0,
        offset,
        object_index,
        local_id: None,
        kind: SketchInputKind::LineOrCircle,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let mut lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: normalized_payload,
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
        sketch_entities: vec![
            entity("curve", 0, None, None),
            entity("start", 1, Some(5), Some([0.0, 0.0])),
            entity("end", 2, Some(9), Some([1.0, 0.0])),
        ],
    };
    normalize_indexed_curve_entities(&mut lane);
    assert_eq!(lane.sketch_entities[1].kind, SketchInputKind::Point);
    assert_eq!(lane.sketch_entities[2].kind, SketchInputKind::Point);
    assert_eq!(
        super::extended_compact_indexed_curve_endpoint_indices(&extended(valid_compact_104), 0,),
        None
    );

    let mut continuation_120 = vec![0; 140];
    continuation_120[..80].copy_from_slice(&marker(84)[..80]);
    continuation_120[..LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    continuation_120[120..122].copy_from_slice(&32u16.to_le_bytes());
    continuation_120[122..126].copy_from_slice(CLASS_MARKER);
    continuation_120[126..128].copy_from_slice(&12u16.to_le_bytes());
    continuation_120[128..].copy_from_slice(b"sgPntPntDist");
    assert_eq!(
        super::extended_compact_indexed_curve_endpoint_indices(&continuation_120, 0),
        Some([5, 9])
    );
    continuation_120[122..140].copy_from_slice(&[
        0xf7, 0x81, 0x00, 0x00, 0x00, 0x00, 0xe6, 0x81, 0x1c, 0x81, 0xff, 0xfe, 0xff, 0x02, 0x44,
        0x00, 0x31, 0x00,
    ]);
    assert_eq!(
        super::extended_compact_indexed_curve_endpoint_indices(&continuation_120, 0),
        Some([5, 9])
    );
    continuation_120[130..132].copy_from_slice(&[0xe6, 0x81]);
    assert_eq!(
        super::extended_compact_indexed_curve_endpoint_indices(&continuation_120, 0),
        None
    );
    continuation_120[130..132].copy_from_slice(&[0x1c, 0x81]);
    continuation_120[119] = 1;
    assert_eq!(
        super::extended_compact_indexed_curve_endpoint_indices(&continuation_120, 0),
        None
    );

    let mut reference_table_126 = vec![0; 206];
    reference_table_126[..80].copy_from_slice(&marker(84)[..80]);
    reference_table_126[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    reference_table_126[126..128].copy_from_slice(&12u16.to_le_bytes());
    reference_table_126[136..140].fill(0xff);
    reference_table_126[154..158].copy_from_slice(&5u32.to_le_bytes());
    reference_table_126[158..162].copy_from_slice(&2u32.to_le_bytes());
    reference_table_126[166..170].copy_from_slice(&[0xfe, 0xff, 0x00, 0x00]);
    reference_table_126[170..172].copy_from_slice(&0x88c5u16.to_le_bytes());
    reference_table_126[174..178].fill(0xff);
    reference_table_126[190..194].fill(0xff);
    assert_eq!(
        compact_indexed_curve_endpoint_indices(&reference_table_126, 0),
        Some([5, 9])
    );
    reference_table_126[126..128].fill(0);
    assert_eq!(
        compact_indexed_curve_endpoint_indices(&reference_table_126, 0),
        None
    );
}

#[test]
fn legacy_compact_96_profile_line_falls_back_to_one_based_complete_roster() {
    let mut payload = vec![0; 96 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[29..31].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&15u16.to_le_bytes());
    payload[58..60].copy_from_slice(&3u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[82..84].copy_from_slice(&1u16.to_le_bytes());
    payload[88..92].copy_from_slice(&6u32.to_le_bytes());
    payload[92..96].copy_from_slice(&1u32.to_le_bytes());
    payload[96..].copy_from_slice(LEGACY_SKETCH_MARKER);

    let entity = |id: String, offset, kind, coordinates_m| SketchInputEntity {
        id,
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
    let mut entities = vec![entity(
        "curve".into(),
        0,
        SketchInputKind::LineOrCircle,
        None,
    )];
    entities.extend((1..=15).map(|index| {
        entity(
            format!("point-{index}"),
            index,
            SketchInputKind::Point,
            Some([index as f64, 0.0]),
        )
    }));
    let markers = entities.iter().collect::<Vec<_>>();

    assert_eq!(
        roster_curve_endpoint_markers(&payload, &entities[0], &markers)
            .into_iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        vec!["point-14", "point-2"]
    );
}

#[test]
fn wide_indexed_curve_owns_its_endpoint_trailer_in_all_generations() {
    let detail = 92;
    let mut payload = vec![0; detail + 80];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..35].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[35..39].copy_from_slice(&[0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&6u16.to_le_bytes());
    payload[66..68].copy_from_slice(&10u16.to_le_bytes());
    payload[68..72].copy_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[detail..detail + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[detail + 5..detail + 13]
        .copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0x04, 0x00, 0xff, 0xff]);
    payload[detail + 13..detail + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[detail + 23..detail + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[detail + 27..detail + 29].copy_from_slice(&2u16.to_le_bytes());
    payload[detail + 31..detail + 35].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[detail + 35..detail + 39].copy_from_slice(&[0x00, 0x00, 0x0c, 0x00]);
    payload[detail + 48..detail + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[detail + 64..detail + 72].copy_from_slice(&(-1.0f64).to_le_bytes());

    assert_eq!(
        wide_indexed_curve_endpoint_indices(&payload, 0),
        Some([7, 11])
    );
    assert_eq!(marker_local_links(&payload, 0), None);
    assert!(!marker_is_selected_construction_line(&payload, 0));
    assert_eq!(
        compact_bounded_curve_tangent(&payload, 0),
        Some([-1.0, 0.0])
    );

    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[detail + 23..detail + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    assert_eq!(
        wide_indexed_curve_endpoint_indices(&payload, 0),
        Some([7, 11])
    );

    let entity = |id: &str,
                  offset: u64,
                  object_index: Option<u32>,
                  coordinates_m: Option<[f64; 2]>,
                  kind: SketchInputKind| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("profile".into()),
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
    let mut lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload.clone(),
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
        sketch_entities: vec![
            entity("curve", 0, None, None, SketchInputKind::Arc),
            entity(
                "start",
                1,
                Some(7),
                Some([0.0, 0.0]),
                SketchInputKind::LineOrCircle,
            ),
            entity(
                "end",
                2,
                Some(11),
                Some([1.0, 0.0]),
                SketchInputKind::LineOrCircle,
            ),
        ],
    };
    normalize_indexed_curve_entities(&mut lane);
    assert_eq!(lane.sketch_entities[1].kind, SketchInputKind::Point);
    assert_eq!(lane.sketch_entities[2].kind, SketchInputKind::Point);

    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(
        wide_indexed_curve_endpoint_indices(&payload, 0),
        Some([7, 11])
    );
    payload[35..39].copy_from_slice(&[0x00, 0x00, 0x44, 0x00]);
    assert_eq!(
        wide_indexed_curve_endpoint_indices(&payload, 0),
        Some([7, 11])
    );
    payload[35..39].copy_from_slice(&[0x00, 0x00, 0x84, 0x00]);
    assert_eq!(
        wide_indexed_curve_endpoint_indices(&payload, 0),
        Some([7, 11])
    );
    payload[35..39].copy_from_slice(&[0x00, 0x00, 0x04, 0x00]);

    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[detail + 23..detail + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    assert_eq!(
        wide_indexed_curve_endpoint_indices(&payload, 0),
        Some([7, 11])
    );
    assert!(!marker_is_selected_construction_line(&payload, 0));
    assert_eq!(
        compact_bounded_curve_tangent(&payload, 0),
        Some([-1.0, 0.0])
    );

    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[detail + 23..detail + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    assert_eq!(
        wide_indexed_curve_endpoint_indices(&payload, 0),
        Some([7, 11])
    );
    assert_eq!(marker_local_links(&payload, 0), None);
    assert_eq!(
        compact_bounded_curve_tangent(&payload, 0),
        Some([-1.0, 0.0])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::LineOrCircle
    );

    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Arc
    );

    let mut coordinate_line = vec![0; 134 + SKETCH_MARKER.len()];
    coordinate_line[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    coordinate_line[5..13].fill(0xff);
    coordinate_line[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    coordinate_line[17..21].copy_from_slice(&2u32.to_le_bytes());
    coordinate_line[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    coordinate_line[27..29].copy_from_slice(&1u16.to_le_bytes());
    coordinate_line[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    coordinate_line[64..66].copy_from_slice(&[0x1e, 0x00]);
    coordinate_line[66..74].copy_from_slice(&0.015f64.to_le_bytes());
    coordinate_line[74..82].copy_from_slice(&0.0f64.to_le_bytes());
    coordinate_line[134..].copy_from_slice(SKETCH_MARKER);
    assert_eq!(
        sketch_input_entities(&coordinate_line, "lane")[0].kind,
        SketchInputKind::LineOrCircle
    );

    let mut legacy_112 = vec![0; 112 + LEGACY_SKETCH_MARKER.len()];
    legacy_112[..80].copy_from_slice(&payload[..80]);
    legacy_112[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    legacy_112[80..84].copy_from_slice(&1i32.to_le_bytes());
    legacy_112[84..86].copy_from_slice(&4u16.to_le_bytes());
    for offset in (86..102).step_by(4) {
        legacy_112[offset..offset + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    legacy_112[104..108].copy_from_slice(&583u32.to_le_bytes());
    legacy_112[108..112].copy_from_slice(&450u32.to_le_bytes());
    legacy_112[112..].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(
        wide_indexed_curve_endpoint_indices(&legacy_112, 0),
        Some([7, 11])
    );
    legacy_112[98] = 0;
    assert_eq!(wide_indexed_curve_endpoint_indices(&legacy_112, 0), None);

    let mut current_112 = legacy_112;
    current_112[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    current_112[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    current_112[29..31].copy_from_slice(&1u16.to_le_bytes());
    current_112[35..39].copy_from_slice(&[0x00, 0x00, 0x44, 0x00]);
    current_112[80..84].copy_from_slice(&(-1i32).to_le_bytes());
    current_112[98..102].copy_from_slice(&(-2i32).to_le_bytes());
    current_112[112..112 + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    assert_eq!(
        wide_indexed_curve_endpoint_indices(&current_112, 0),
        Some([7, 11])
    );
    let mut current_112_with_detail = current_112.clone();
    current_112_with_detail.resize(112 + 80, 0);
    current_112_with_detail[112..112 + 80].copy_from_slice(&payload[detail..detail + 80]);
    assert_eq!(
        compact_bounded_curve_tangent(&current_112_with_detail, 0),
        Some([-1.0, 0.0])
    );
    current_112_with_detail[84..86].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(
        compact_bounded_curve_tangent(&current_112_with_detail, 0),
        None
    );
    current_112[17..21].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(wide_indexed_curve_endpoint_indices(&current_112, 0), None);
    current_112[17..21].copy_from_slice(&2u32.to_le_bytes());
    current_112[84..86].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(wide_indexed_curve_endpoint_indices(&current_112, 0), None);

    let mut legacy_terminal = vec![0; 156];
    legacy_terminal[..80].copy_from_slice(&payload[..80]);
    legacy_terminal[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    legacy_terminal[17..21].copy_from_slice(&1u32.to_le_bytes());
    legacy_terminal[80..84].copy_from_slice(&1i32.to_le_bytes());
    legacy_terminal[84..86].copy_from_slice(&12u16.to_le_bytes());
    for offset in (86..102).step_by(4) {
        legacy_terminal[offset..offset + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    legacy_terminal[136..138].copy_from_slice(&[0x05, 0x00]);
    legacy_terminal[138..142].copy_from_slice(CLASS_MARKER);
    legacy_terminal[142..144].copy_from_slice(&12u16.to_le_bytes());
    legacy_terminal[144..].copy_from_slice(b"sgPntPntDist");
    assert_eq!(
        wide_indexed_curve_endpoint_indices(&legacy_terminal, 0),
        Some([7, 11])
    );
    assert_eq!(
        coordinate_roster_endpoint_offset(&legacy_terminal, 0),
        Some(64)
    );
    legacy_terminal[135] = 1;
    assert_eq!(
        wide_indexed_curve_endpoint_indices(&legacy_terminal, 0),
        None
    );
}

#[test]
fn current_wide_arc_uses_direct_point_ids_with_an_arc_center_carrier() {
    let mut payload = vec![0; 92 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&6u16.to_le_bytes());
    payload[66..68].copy_from_slice(&5u16.to_le_bytes());
    payload[68..72].copy_from_slice(&1u32.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[84..88].copy_from_slice(&4u32.to_le_bytes());
    payload[88..92].copy_from_slice(&3u32.to_le_bytes());
    payload[92..].copy_from_slice(SKETCH_MARKER);
    let entity = |id: &str, object_index, coordinates_m, kind: SketchInputKind| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
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
    let entities = [
        entity("curve", Some(2), None, SketchInputKind::Arc),
        entity("center", Some(4), Some([0.0, 0.0]), SketchInputKind::Arc),
        entity("start", Some(6), Some([0.0, 1.0]), SketchInputKind::Point),
        entity("end", Some(5), Some([0.0, -1.0]), SketchInputKind::Point),
        entity("shifted", Some(7), Some([2.0, 0.0]), SketchInputKind::Point),
    ];
    let markers = entities.iter().collect::<Vec<_>>();

    let (endpoints, center) = current_wide_arc_direct_markers(&payload, &entities[0], &markers)
        .expect("direct endpoint IDs");
    assert_eq!(
        endpoints
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["start", "end"]
    );
    assert_eq!(
        coordinate_roster_arc_center(
            &payload,
            &entities[0],
            &markers,
            [endpoints[0], endpoints[1]],
        ),
        Some(center)
    );
}

#[test]
fn wide_line_uses_direct_point_ids_after_one_based_resolution_fails() {
    let mut payload = vec![0; 92 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&7u16.to_le_bytes());
    payload[66..68].copy_from_slice(&8u16.to_le_bytes());
    payload[68..72].copy_from_slice(&1u32.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[92..].copy_from_slice(SKETCH_MARKER);
    let entity = |id: &str, object_index, coordinates_m, kind: SketchInputKind| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
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
    let entities = [
        entity("curve", Some(6), None, SketchInputKind::LineOrCircle),
        entity("start", Some(7), Some([-1.0, 0.0]), SketchInputKind::Point),
        entity("end", Some(8), Some([1.0, 0.0]), SketchInputKind::Point),
    ];
    let markers = entities.iter().collect::<Vec<_>>();

    let endpoints = wide_direct_line_endpoint_markers(&payload, &entities[0], &markers)
        .expect("direct point IDs");
    assert_eq!(
        endpoints
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["start", "end"]
    );

    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[64..66].fill(0);
    let zero = entity("zero", None, Some([0.0, 0.0]), SketchInputKind::Point);
    let extended = [&entities[0], &zero, &entities[2]];
    let endpoints = wide_direct_line_endpoint_markers(&payload, &entities[0], &extended)
        .expect("unique zero-identity point");
    assert_eq!(endpoints[0].id, "zero");

    let other_zero = entity("other-zero", None, Some([2.0, 0.0]), SketchInputKind::Point);
    let ambiguous = [&entities[0], &zero, &other_zero, &entities[2]];
    assert_eq!(
        wide_direct_line_endpoint_markers(&payload, &entities[0], &ambiguous),
        None
    );
    payload[92] = 0;
    assert_eq!(
        wide_direct_line_endpoint_markers(&payload, &entities[0], &extended),
        None
    );
}

#[test]
fn extended_marker104_arc_prefers_point_roster_endpoints() {
    let mut payload = vec![0; 104 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&0u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&4u16.to_le_bytes());
    payload[58..60].copy_from_slice(&6u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&1i32.to_le_bytes());
    for start in (78..94).step_by(4) {
        payload[start..start + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[96..100].copy_from_slice(&2u32.to_le_bytes());
    payload[100..104].copy_from_slice(&2u32.to_le_bytes());
    payload[104..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    let entity = |id: &str, offset, object_index, coordinates_m, kind| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
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
    let curve = entity("curve", 0, None, None, SketchInputKind::Arc);
    let object_indices = [1, 2, 4, 5, 6, 7, 8];
    let points = object_indices.map(|object_index| {
        entity(
            &format!("point-{object_index}"),
            u64::from(object_index) * 10,
            Some(object_index),
            Some([f64::from(object_index), 0.0]),
            SketchInputKind::Point,
        )
    });
    let markers = std::iter::once(&curve)
        .chain(points.iter())
        .collect::<Vec<_>>();

    assert!(indexed_arc_uses_coordinate_center(&payload, 0));
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(56));
    let endpoints = roster_curve_endpoint_markers(&payload, &curve, &markers);
    assert_eq!(
        endpoints
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["point-6", "point-8"]
    );
}

#[test]
fn extended_geometry_104_arc_uses_zero_based_roster_and_center_index() {
    let mut payload = vec![0; 104 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&0u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&1u16.to_le_bytes());
    payload[58..60].copy_from_slice(&2u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&1i32.to_le_bytes());
    payload[76..78].copy_from_slice(&0u16.to_le_bytes());
    for relative in (78..94).step_by(4) {
        payload[relative..relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[94..96].fill(0);
    payload[104..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

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
    let curve = entity("curve", 0, None, SketchInputKind::Arc);
    let center = entity("center", 10, Some([0.0, 0.0]), SketchInputKind::Point);
    let start = entity("start", 20, Some([1.0, 0.0]), SketchInputKind::Point);
    let end = entity("end", 30, Some([0.0, 1.0]), SketchInputKind::Point);
    let markers = [&curve, &center, &start, &end];

    assert!(indexed_arc_uses_coordinate_center(&payload, 0));
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(56));
    assert_eq!(
        roster_curve_endpoint_markers(&payload, &curve, &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["start", "end"]
    );
    assert_eq!(
        coordinate_roster_arc_center(&payload, &curve, &markers, [&start, &end]),
        Some([0.0, 0.0])
    );

    payload[72..76].copy_from_slice(&(-1i32).to_le_bytes());
    assert!(indexed_arc_uses_coordinate_center(&payload, 0));
    assert_eq!(
        coordinate_roster_arc_center(&payload, &curve, &markers, [&start, &end]),
        Some([0.0, 0.0])
    );

    payload[76..78].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        coordinate_roster_arc_center(&payload, &curve, &markers, [&start, &end]),
        None
    );
}

#[test]
fn extended_compact_104_arc_uses_geometry_roster_for_center_index() {
    let mut payload = vec![0; 104 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&0u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&3u16.to_le_bytes());
    payload[58..60].copy_from_slice(&4u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&1i32.to_le_bytes());
    for relative in (78..94).step_by(4) {
        payload[relative..relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[104..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

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
    let curve = entity("curve", 0, None, SketchInputKind::Arc);
    let relation = entity(
        "relation",
        1,
        Some([9.0, 9.0]),
        SketchInputKind::Relation(SketchRelationKind::Horizontal),
    );
    let first = entity("first", 10, Some([5.0, 5.0]), SketchInputKind::Point);
    let second = entity("second", 20, Some([6.0, 6.0]), SketchInputKind::Point);
    let center = entity("center", 30, Some([0.0, 0.0]), SketchInputKind::Point);
    let start = entity("start", 40, Some([1.0, 0.0]), SketchInputKind::Point);
    let end = entity("end", 50, Some([0.0, 1.0]), SketchInputKind::Point);
    let markers = [&curve, &relation, &first, &second, &center, &start, &end];

    assert!(indexed_arc_uses_coordinate_center(&payload, 0));
    assert_eq!(
        coordinate_roster_arc_center(&payload, &curve, &markers, [&start, &end]),
        Some([0.0, 0.0])
    );
}

#[test]
fn coordinate_roster_arc_center_requires_matching_indexed_endpoints() {
    let mut payload = vec![0; 104 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&0u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&2u16.to_le_bytes());
    payload[58..60].copy_from_slice(&3u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&1i32.to_le_bytes());
    payload[76..78].copy_from_slice(&3u16.to_le_bytes());
    for relative in (78..94).step_by(4) {
        payload[relative..relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[104..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

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
    let curve = entity("curve", 0, SketchInputKind::Arc, None);
    let relation = entity(
        "relation",
        1,
        SketchInputKind::Relation(SketchRelationKind::Horizontal),
        Some([9.0, 9.0]),
    );
    let start = entity("start", 10, SketchInputKind::Point, Some([1.0, 0.0]));
    let end = entity("end", 20, SketchInputKind::Point, Some([0.0, 1.0]));
    let center = entity("center", 30, SketchInputKind::Point, Some([0.0, 0.0]));
    let distractor = entity("distractor", 40, SketchInputKind::Point, Some([4.0, 4.0]));
    let markers = [&curve, &relation, &start, &end, &center, &distractor];

    assert_eq!(
        coordinate_roster_arc_center(&payload, &curve, &markers, [&start, &end]),
        None
    );
}

#[test]
fn extended_geometry_116_arc_uses_relation_tail_and_center_index() {
    let mut payload = vec![0; 116 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&0u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&1u16.to_le_bytes());
    payload[58..60].copy_from_slice(&3u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&(-1i32).to_le_bytes());
    payload[76..78].copy_from_slice(&2u16.to_le_bytes());
    for relative in (78..94).step_by(4) {
        payload[relative..relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[94..102].fill(0);
    payload[102..106].copy_from_slice(&4u32.to_le_bytes());
    payload[106..112].fill(0);
    payload[112..116].copy_from_slice(&3u32.to_le_bytes());
    payload[116..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

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
    let curve = entity("curve", 0, None, SketchInputKind::Arc);
    let first = entity("first", 10, Some([9.0, 9.0]), SketchInputKind::Point);
    let start = entity("start", 20, Some([1.0, 0.0]), SketchInputKind::Point);
    let center = entity("center", 30, Some([0.0, 0.0]), SketchInputKind::Point);
    let end = entity("end", 40, Some([0.0, 1.0]), SketchInputKind::Point);
    let markers = [&curve, &first, &start, &center, &end];

    assert!(indexed_arc_uses_coordinate_center(&payload, 0));
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(56));
    assert_eq!(
        coordinate_roster_arc_center(&payload, &curve, &markers, [&start, &end]),
        Some([0.0, 0.0])
    );

    payload[58..60].copy_from_slice(&1u16.to_le_bytes());
    let (circle_center, radius) =
        equal_index_coordinate_roster_full_circle(&payload, &curve, &markers)
            .expect("116-byte equal-index circle");
    assert_eq!(circle_center, [9.0, 9.0]);
    assert!((radius - 145.0_f64.sqrt()).abs() < 1.0e-12);
}

#[test]
fn extended_geometry_terminal_circle_uses_dimension_tail() {
    let mut payload = vec![0; 160];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&0u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&2u16.to_le_bytes());
    payload[58..60].copy_from_slice(&2u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&1i32.to_le_bytes());
    for relative in (78..94).step_by(4) {
        payload[relative..relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[94..128].fill(0);
    payload[128..130].copy_from_slice(&4u16.to_le_bytes());
    payload[130..134].copy_from_slice(&7u32.to_le_bytes());
    payload[134..136].fill(0);
    payload[136..140].copy_from_slice(&9u32.to_le_bytes());
    payload[140..148].copy_from_slice(&[0xff, 0xfe, 0xff, 0x02, 0x44, 0x00, 0x31, 0x00]);
    payload[148..156].copy_from_slice(&2.0f64.to_le_bytes());
    payload[156..160].fill(0xff);

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
    let circle = entity("circle", 0, None, SketchInputKind::Arc);
    let witness = entity("witness", 5, Some([9.0, 9.0]), SketchInputKind::Arc);
    let center = entity("center", 10, Some([0.0, 0.0]), SketchInputKind::Point);
    let radial = entity("radial", 20, Some([1.0, 0.0]), SketchInputKind::Point);
    let markers = [&circle, &witness, &center, &radial];

    assert_eq!(
        equal_index_coordinate_roster_full_circle(&payload, &circle, &markers),
        Some(([0.0, 0.0], 1.0))
    );
}

#[test]
fn legacy_compact_geometry_locus_code_two_is_a_profile_line() {
    let mut payload = vec![0; 84 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&1u16.to_le_bytes());
    payload[58..60].copy_from_slice(&2u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[74..76].copy_from_slice(&2u16.to_le_bytes());
    payload[76..80].copy_from_slice(&4u32.to_le_bytes());
    payload[80..84].copy_from_slice(&3u32.to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert!(legacy_compact_profile_line(&payload, 0));
    let entities = sketch_input_entities(&payload, "lane");
    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0].kind, SketchInputKind::LineOrCircle);

    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    assert!(!legacy_compact_profile_line(&payload, 0));
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Arc
    );

    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload.resize(96 + LEGACY_SKETCH_MARKER.len(), 0);
    payload[72..96].fill(0);
    payload[82..84].copy_from_slice(&2u16.to_le_bytes());
    payload[88..92].copy_from_slice(&3u32.to_le_bytes());
    payload[92..96].copy_from_slice(&1u32.to_le_bytes());
    payload[96..].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert!(legacy_compact_profile_line(&payload, 0));
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::LineOrCircle
    );

    payload[82..84].copy_from_slice(&10u16.to_le_bytes());
    assert!(legacy_compact_profile_line(&payload, 0));
    payload[82..84].copy_from_slice(&0u16.to_le_bytes());
    assert!(!legacy_compact_profile_line(&payload, 0));
    payload[82..84].copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(!legacy_compact_profile_line(&payload, 0));
}

#[test]
fn compact_legacy_bounded_curve_can_use_direct_point_ids() {
    let mut payload = vec![0; 84 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[29..31].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&7u16.to_le_bytes());
    payload[58..60].copy_from_slice(&10u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_SKETCH_MARKER);
    let entity = |id: &str, object_index, coordinates_m, kind| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("profile".into()),
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
    let entities = [
        entity("curve", Some(1), None, SketchInputKind::Arc),
        entity("start", Some(7), Some([-1.0, 0.0]), SketchInputKind::Point),
        entity("end", Some(10), Some([1.0, 0.0]), SketchInputKind::Point),
        entity(
            "one-based-start",
            Some(8),
            Some([0.0, 1.0]),
            SketchInputKind::Point,
        ),
    ];
    let markers = entities.iter().collect::<Vec<_>>();

    let endpoints = legacy_compact_direct_endpoint_markers(&payload, 0, &entities[0], &markers);
    assert_eq!(
        endpoints
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["start", "end"]
    );
    assert_eq!(
        roster_curve_endpoint_markers(&payload, &entities[0], &markers)
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["start", "end"]
    );

    payload.resize(104 + LEGACY_SKETCH_MARKER.len(), 0);
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[72..76].copy_from_slice(&1i32.to_le_bytes());
    payload[76..104].fill(0);
    for relative in (78..94).step_by(4) {
        payload[relative..relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[96..100].copy_from_slice(&3u32.to_le_bytes());
    payload[100..104].copy_from_slice(&2u32.to_le_bytes());
    payload[104..].copy_from_slice(LEGACY_SKETCH_MARKER);
    let endpoints =
        legacy_marker104_arc_endpoints(&payload, &entities[0], &markers).expect("endpoints");
    assert_eq!(
        endpoints.map(|endpoint| endpoint.id.as_str()),
        ["start", "end"]
    );
    assert_eq!(
        super::legacy_marker104_arc_center(&payload, &entities[0], &markers, endpoints,),
        Some([0.0, 1.0])
    );
}

#[test]
fn indexed_arcs_use_one_equidistant_center_marker() {
    let mut payload = vec![0; 104 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&1i32.to_le_bytes());
    for offset in (78..94).step_by(4) {
        payload[offset..offset + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[104..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    assert!(indexed_arc_uses_coordinate_center(&payload, 0));

    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[35..39].copy_from_slice(&[0x00, 0x00, 0x05, 0x00]);
    payload[56..58].copy_from_slice(&8u16.to_le_bytes());
    payload[58..60].copy_from_slice(&10u16.to_le_bytes());
    let entity = |id: String, offset, object_index, coordinates_m| SketchInputEntity {
        id,
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
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
    let mut coordinates = (0..11)
        .map(|index| {
            entity(
                format!("point-{index}"),
                u64::from(index),
                Some(100 + index),
                Some([f64::from(index), f64::from(index)]),
            )
        })
        .collect::<Vec<_>>();
    coordinates[4].object_index = Some(7);
    coordinates[4].coordinates_m = Some([0.0, -0.02]);
    coordinates[8].coordinates_m = Some([-0.015, 0.02]);
    coordinates[10].coordinates_m = Some([0.015, 0.02]);
    let mut curve = entity("curve".into(), 0, Some(3), None);
    curve.kind = SketchInputKind::Arc;
    let markers = coordinates
        .iter()
        .chain(std::iter::once(&curve))
        .collect::<Vec<_>>();
    assert_eq!(
        coordinate_roster_arc_center(
            &payload,
            &curve,
            &markers,
            [&coordinates[8], &coordinates[10]],
        ),
        Some([0.0, -0.02])
    );

    let mut compact_84 = vec![0; 84 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    compact_84[..LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    compact_84[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    compact_84[29..31].copy_from_slice(&1u16.to_le_bytes());
    compact_84[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    compact_84[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    compact_84[56..60].copy_from_slice(&[15, 0, 16, 0]);
    compact_84[60..64].copy_from_slice(&1u32.to_le_bytes());
    compact_84[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    compact_84[72..76].copy_from_slice(&[0x00, 0x00, 0x01, 0x00]);
    compact_84[80..84].copy_from_slice(&8u32.to_le_bytes());
    compact_84[84..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    assert!(indexed_arc_uses_coordinate_center(&compact_84, 0));
    compact_84[58..60].copy_from_slice(&15u16.to_le_bytes());
    assert!(!indexed_arc_uses_coordinate_center(&compact_84, 0));

    let mut current = vec![0; 92 + SKETCH_MARKER.len()];
    current[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    current[5..13].fill(0xff);
    current[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    current[17..21].copy_from_slice(&2u32.to_le_bytes());
    current[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    current[27..29].copy_from_slice(&1u16.to_le_bytes());
    current[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    current[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    current[64..66].copy_from_slice(&1u16.to_le_bytes());
    current[66..68].copy_from_slice(&2u16.to_le_bytes());
    current[68..72].copy_from_slice(&1u32.to_le_bytes());
    current[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    current[92..].copy_from_slice(SKETCH_MARKER);
    assert!(indexed_arc_uses_coordinate_center(&current, 0));
    assert!(current_undetailed_bounded_curve_is_line(&current, 0));
    let mut extended = current.clone();
    extended[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    assert!(indexed_arc_uses_coordinate_center(&extended, 0));
    assert!(current_undetailed_bounded_curve_is_line(&extended, 0));
    extended[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    assert!(indexed_arc_uses_coordinate_center(&extended, 0));
    assert!(current_undetailed_bounded_curve_is_line(&extended, 0));
    extended[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert!(!current_undetailed_bounded_curve_is_line(&extended, 0));
    let mut current_compact = current[..84].to_vec();
    current_compact[29..31].copy_from_slice(&1u16.to_le_bytes());
    current_compact[56..58].copy_from_slice(&1u16.to_le_bytes());
    current_compact[58..60].copy_from_slice(&2u16.to_le_bytes());
    current_compact[60..64].copy_from_slice(&1u32.to_le_bytes());
    current_compact[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    current_compact[72..84].fill(0);
    current_compact.extend_from_slice(SKETCH_MARKER);
    assert!(indexed_arc_uses_coordinate_center(&current_compact, 0));
    assert!(current_undetailed_bounded_curve_is_line(
        &current_compact,
        0
    ));
    let mut extended_compact = current_compact.clone();
    extended_compact[..LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    extended_compact[84..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    assert!(current_undetailed_bounded_curve_is_line(
        &extended_compact,
        0
    ));
    extended_compact[17..21].copy_from_slice(&0u32.to_le_bytes());
    extended_compact[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    assert!(current_undetailed_bounded_curve_is_line(
        &extended_compact,
        0
    ));
    current_compact[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    assert!(!indexed_arc_uses_coordinate_center(&current_compact, 0));
    let mut detailed = current.clone();
    detailed.resize(172, 0);
    detailed[97..105].copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0x04, 0x00, 0xff, 0xff]);
    detailed[105..109].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    detailed[115..119].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    detailed[119..121].copy_from_slice(&2u16.to_le_bytes());
    detailed[123..131].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    detailed[140..148].copy_from_slice(&1.0f64.to_le_bytes());
    detailed[156..164].copy_from_slice(&(-1.0f64).to_le_bytes());
    assert!(!current_undetailed_bounded_curve_is_line(&detailed, 0));
    assert!(!current_indexed_arc_reverses_center_sweep(&current, 0));
    current[80..84].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
    assert!(current_indexed_arc_reverses_center_sweep(&current, 0));
    current[17..21].copy_from_slice(&1u32.to_le_bytes());
    assert!(!indexed_arc_uses_coordinate_center(&current, 0));

    let start = Point2::new(1.0, 0.0);
    let end = Point2::new(0.0, 1.0);
    assert_eq!(
        unique_arc_center_marker(
            start,
            end,
            &[Point2::new(0.0, 0.0), Point2::new(4.0, 3.0)],
            1.0e-8,
        ),
        Some(Point2::new(0.0, 0.0))
    );
    assert_eq!(
        unique_arc_center_marker(
            start,
            end,
            &[Point2::new(0.0, 0.0), Point2::new(0.5, 0.5)],
            1.0e-8,
        ),
        None
    );
}

#[test]
fn compact_legacy_bounded_arc_uses_its_diameter_center_marker() {
    let mut payload = vec![0; 102];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..60].copy_from_slice(&[4, 0, 0, 0]);
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&1i32.to_le_bytes());
    payload[76..78].copy_from_slice(&5u16.to_le_bytes());
    for relative in (78..94).step_by(4) {
        payload[relative..relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    let marker = |id: &str, offset, kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("profile".into()),
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
    let curve = marker("arc", 0, SketchInputKind::Arc, None);
    let start = marker("start", 1, SketchInputKind::Point, Some([1.0, 0.0]));
    let center = marker("center", 2, SketchInputKind::Point, Some([0.0, 0.0]));
    let end = marker("end", 3, SketchInputKind::Point, Some([-1.0, 0.0]));
    let off_axis = marker("handle", 4, SketchInputKind::Point, Some([0.0, 2.0]));
    let markers = [&start, &center, &end, &off_axis];

    assert_eq!(
        legacy_compact_diameter_arc_center(&payload, &curve, &markers, [&start, &end]),
        Some([0.0, 0.0])
    );
}
