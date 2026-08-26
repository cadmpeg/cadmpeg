//! Indexed-curve trailer and arc-center tests.

use super::super::super::bindings::normalize_indexed_curve_entities;
use super::super::super::curves::compact_bounded_curve_tangent;
use super::super::super::markers::sketch_input_entities;
use super::super::super::selections::marker_local_links;
use super::super::super::typed_relations::{
    current_undetailed_bounded_curve_is_line, legacy_marker104_arc_endpoints,
};
use super::super::super::{
    CLASS_MARKER, LEGACY_EXTENDED_SKETCH_MARKER, LEGACY_SKETCH_MARKER, SKETCH_MARKER,
};
use super::super::*;
use crate::records::{FeatureInputLane, SketchInputEntity, SketchInputKind, SketchRelationKind};
use cadmpeg_ir::math::Point2;

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
fn legacy_geometry_locus_84_bounded_arc_without_detail_is_line() {
    let mut payload = vec![0; 84 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[21..23].fill(0);
    payload[23..31].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00, 0x02, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[39..48].fill(0);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&0u16.to_le_bytes());
    payload[58..60].copy_from_slice(&6u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].fill(0);
    payload[76..80].copy_from_slice(&4u32.to_le_bytes());
    payload[80..84].copy_from_slice(&3u32.to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        compact_indexed_curve_endpoint_indices(&payload, 0),
        Some([1, 7])
    );
    assert!(legacy_undetailed_profile_line(&payload, 0));

    payload[29..31].copy_from_slice(&1u16.to_le_bytes());
    assert!(!legacy_undetailed_profile_line(&payload, 0));
    payload[29..31].copy_from_slice(&2u16.to_le_bytes());
    payload[80..84].copy_from_slice(&4u32.to_le_bytes());
    assert!(!legacy_undetailed_profile_line(&payload, 0));
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
fn extended_terminal_102_profile_arc_uses_object_center_fallback() {
    let mut payload = vec![0; 102];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&0u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&7u16.to_le_bytes());
    payload[58..60].copy_from_slice(&5u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&1i32.to_le_bytes());
    for relative in (78..94).step_by(4) {
        payload[relative..relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }

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
    let center = entity(
        "center",
        10,
        Some(4),
        Some([2.0, 0.0]),
        SketchInputKind::Point,
    );
    let start = entity(
        "start",
        20,
        Some(5),
        Some([0.0, 1.0]),
        SketchInputKind::Point,
    );
    let end = entity(
        "end",
        30,
        Some(7),
        Some([0.0, -1.0]),
        SketchInputKind::Point,
    );
    let markers = [&curve, &center, &start, &end];

    assert!(extended_profile_terminal_102_indexed_arc(&payload, 0));
    assert!(indexed_arc_uses_coordinate_center(&payload, 0));
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(56));
    assert_eq!(
        roster_curve_endpoint_markers(&payload, &curve, &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["end", "start"]
    );
    assert_eq!(
        coordinate_roster_arc_center(&payload, &curve, &markers, [&end, &start]),
        Some([2.0, 0.0])
    );

    payload[94] = 1;
    assert!(!extended_profile_terminal_102_indexed_arc(&payload, 0));
    assert!(!indexed_arc_uses_coordinate_center(&payload, 0));
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
