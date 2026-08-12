//! Tests for the `selections` module.

use super::super::component_paths::{compact_edge_path_value, compact_edge_selection_set_value};
use super::super::{CLASS_MARKER, LEGACY_SKETCH_MARKER};
use super::selection_vector_tail;
use super::*;
use crate::classification::FeatureClass;
use crate::records::{
    Feature, FeatureHistory, FeatureInputClass, FeatureInputClassRole,
    FeatureInputComponentPathEntry, FeatureInputEdgeSelection, FeatureInputLane, FeatureInputName,
    FeatureInputScalar, FeatureInputScalarRole,
};
use std::collections::{BTreeMap, HashSet};

#[test]
fn component_vector_selector_accepts_lane_subtypes() {
    for selector in [
        [0, 2, 0, 0],
        [4, 2, 0, 0],
        [6, 2, 0, 0],
        [4, 3, 0, 0],
        [0x7f, 2, 0, 0],
    ] {
        assert!(is_component_vector_selector(&selector));
    }
    assert!(!is_component_vector_selector(&[4, 4, 0, 0]));
    assert!(!is_component_vector_selector(&[4, 2, 1, 0]));
}

#[test]
fn compact_body_states_require_a_duplicated_local_identity() {
    let token = 0x89a4u16;
    let mut payload = vec![0; 180];
    let header = &mut payload[12..95];
    header[0..2].copy_from_slice(&token.to_le_bytes());
    header[2..11].copy_from_slice(&[0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0]);
    header[11..15].copy_from_slice(&205u32.to_le_bytes());
    header[15..19].copy_from_slice(&205u32.to_le_bytes());
    header[47..63].fill(0xff);

    assert_eq!(compact_body_state_ids(&payload, 0, 180, token), [205]);

    payload[12 + 15..12 + 19].copy_from_slice(&206u32.to_le_bytes());
    assert!(compact_body_state_ids(&payload, 0, 180, token).is_empty());
}

#[test]
fn compact_body_retention_mode_follows_the_state_roster() {
    use cadmpeg_ir::features::BodyRetentionMode::{DeleteSelected, KeepSelected};

    let token = 0x89a4u16;
    let mut payload = vec![0; 112];
    let header = &mut payload[12..95];
    header[0..2].copy_from_slice(&token.to_le_bytes());
    header[2..11].copy_from_slice(&[0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0]);
    header[11..15].copy_from_slice(&205u32.to_le_bytes());
    header[15..19].copy_from_slice(&205u32.to_le_bytes());
    header[47..63].fill(0xff);
    payload[95..97].copy_from_slice(&[0x30, 0x80]);

    assert_eq!(
        compact_body_retention_mode(&payload, 0, payload.len(), token),
        Some(KeepSelected)
    );
    payload[97..101].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        compact_body_retention_mode(&payload, 0, payload.len(), token),
        Some(DeleteSelected)
    );
    payload[101] = 1;
    assert_eq!(
        compact_body_retention_mode(&payload, 0, payload.len(), token),
        None
    );
}

#[test]
fn compact_general_curve_reference_requires_the_nested_profile_prefix() {
    let mut payload = vec![0; 24];
    payload[2..4].copy_from_slice(&0xe1u16.to_le_bytes());
    payload[6..8].copy_from_slice(&0x802du16.to_le_bytes());
    payload[8..18].copy_from_slice(&[0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0, 0]);
    assert!(compact_general_curve_ref_at(&payload, 2));
    payload[12] = 1;
    assert!(!compact_general_curve_ref_at(&payload, 2));
}

#[test]
fn general_curve_component_profile_requires_a_complete_reference_record() {
    let mut payload = vec![0; 192];
    let prefix = 24;
    payload[prefix..prefix + 10].copy_from_slice(&[0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0, 0]);
    payload[prefix + 45..prefix + 61].fill(0xff);
    let source = prefix + 81;
    payload[source..source + 4].copy_from_slice(&134u32.to_le_bytes());
    payload[source + 4..source + 8].copy_from_slice(&0x5edf_5674u32.to_le_bytes());
    payload[source + 16..source + 20].copy_from_slice(&0x65u32.to_le_bytes());
    payload[source + 24..source + 28].fill(0xff);
    for at in [source + 32, source + 36, source + 40] {
        payload[at..at + 4].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
    }
    payload[source + 48..source + 52].copy_from_slice(&[0xf8, 0x2a, 0, 0]);

    assert_eq!(component_profile_source_at(&payload, prefix), Some(134));
    payload[source + 40] ^= 1;
    assert_eq!(component_profile_source_at(&payload, prefix), None);
}

#[test]
fn component_reference_curve_accepts_count_minus_one_with_instance_separator() {
    let marker = 24;
    let mut payload = vec![0; 180];
    payload[marker - 12..marker - 8].copy_from_slice(&5u32.to_le_bytes());
    payload[marker - 8..marker - 4].copy_from_slice(&[4, 2, 0, 0]);
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    let mut cursor = marker + 18;
    let mut signature = [0u8; 12];
    signature[4..8].copy_from_slice(&137u32.to_le_bytes());
    for (index, instance) in [0x8c20u16, 0x8c25, 0x8c1a, 0x8c15].into_iter().enumerate() {
        if index == 1 {
            payload[cursor..cursor + 6].copy_from_slice(&[1, 0, 0, 0, 0, 0]);
            cursor += 6;
        }
        payload[cursor..cursor + 2].copy_from_slice(&instance.to_le_bytes());
        payload[cursor + 4..cursor + 16].copy_from_slice(&signature);
        payload[cursor + 16..cursor + 20].copy_from_slice(&1u32.to_le_bytes());
        cursor += 20;
    }
    payload[cursor + 8..cursor + 12].copy_from_slice(&[0xf8, 0x2a, 0, 0]);

    let components =
        component_reference_curve_path_at(&payload, marker).expect("required invariant");
    assert_eq!(components.len(), 4);
    assert_eq!(components[0].instance, Some(0x8c20));
    assert!(components
        .iter()
        .all(|component| component.local_id == Some(1)));

    payload[cursor + 8] ^= 1;
    assert_eq!(component_reference_curve_path_at(&payload, marker), None);
}

#[test]
fn local_links_require_the_reference_trailer() {
    let mut payload = vec![0; 80];
    payload[64..66].copy_from_slice(&37u16.to_le_bytes());
    payload[66..68].copy_from_slice(&39u16.to_le_bytes());
    payload[68..70].copy_from_slice(&1u16.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    assert_eq!(marker_local_links(&payload, 0), Some(([37, 39], 1)));
    payload[70] = 1;
    assert_eq!(marker_local_links(&payload, 0), None);
    payload[70] = 0;
    payload[72..80].copy_from_slice(&0.0f64.to_le_bytes());
    assert_eq!(marker_local_links(&payload, 0), None);
    payload[5..17].copy_from_slice(&[
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x80, 0xbf,
    ]);
    payload[64..66].copy_from_slice(&[0x1e, 0x00]);
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    assert_eq!(marker_local_links(&payload, 0), Some(([30, 39], 1)));
}

#[test]
fn non_coordinate_legacy_profile_line_carries_counted_endpoint_links() {
    let mut payload = vec![0; 162];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[84..86].copy_from_slice(&2u16.to_le_bytes());
    for (index, local_id) in [2u16, 5].into_iter().enumerate() {
        let start = 86 + index * 12;
        payload[start..start + 2].copy_from_slice(&0x83a9u16.to_le_bytes());
        payload[start + 2..start + 4].copy_from_slice(&local_id.to_le_bytes());
        payload[start + 4..start + 8].fill(0xff);
    }
    payload[112..116].copy_from_slice(&[0xfe, 0xff, 0xff, 0xff]);

    assert_eq!(
        coordinate_marker_local_links(&payload, 0),
        Some((vec![2, 5], 0x83a9))
    );
}

#[test]
fn coordinate_namespace_disambiguates_reused_local_id() {
    let candidates = vec![("relation".into(), false), ("geometry".into(), true)];
    assert_eq!(unique_marker_candidate(&candidates), Some("geometry"));
    let ambiguous = vec![("first".into(), true), ("second".into(), true)];
    assert_eq!(unique_marker_candidate(&ambiguous), None);
}

#[test]
fn compact_body_selection_requires_the_complete_trailer() {
    let mut payload = vec![0xaa; 9];
    payload.extend(11000u32.to_le_bytes());
    payload.extend([0; 8]);
    payload.extend(2u32.to_le_bytes());
    payload.extend(287u32.to_le_bytes());
    payload.extend(115u32.to_le_bytes());
    payload.extend(u32::MAX.to_le_bytes());
    payload.extend([0; 12]);
    payload.extend([0x6a, 0xcb]);
    assert_eq!(
        compact_body_selection_vector(&payload, 100, Some(0xcb6a)),
        Some((109, vec![287, 115]))
    );
    assert_eq!(compact_body_selection_at(&payload, 9), Some(vec![287, 115]));
    let mut embedded_false_header = vec![0xaa; 9];
    embedded_false_header.extend(11000u32.to_le_bytes());
    embedded_false_header.extend([0; 8]);
    embedded_false_header.extend(5u32.to_le_bytes());
    for id in [287, 11000, 0, 0, u32::MAX] {
        embedded_false_header.extend(id.to_le_bytes());
    }
    embedded_false_header.extend(u32::MAX.to_le_bytes());
    embedded_false_header.extend([0; 12]);
    assert_eq!(
        compact_body_selection_vector(&embedded_false_header, 100, None),
        Some((109, vec![287, 11000, 0, 0, u32::MAX]))
    );
    let zero_trailer = payload.len() - 3;
    payload[zero_trailer] = 1;
    assert_eq!(
        compact_body_selection_vector(&payload, 100, Some(0xcb6a)),
        None
    );
}

#[test]
fn compact_edge_selection_is_count_delimited_and_signature_typed() {
    let mut payload = Vec::new();
    payload.extend(3u32.to_le_bytes());
    payload.extend([0x00, 0x02, 0x00, 0x00, 0, 0, 0, 0]);
    payload.extend(COMPACT_EDGE_VECTOR_MARKER);
    payload.extend([0, 0]);
    let signature = [
        0x00, 0x81, 0x03, 0x01, 0x2c, 0, 0, 0, 0x63, 0x18, 0x58, 0x69,
    ];
    for (index, edge_id) in [4u32, 0, 5].into_iter().enumerate() {
        payload.extend((0x818bu32 + index as u32).to_le_bytes());
        payload.extend(signature);
        payload.extend(edge_id.to_le_bytes());
        if index == 0 {
            payload.extend([0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]);
        } else if index == 1 {
            payload.extend([0; 8]);
        }
    }
    assert_eq!(compact_edge_selection_at(&payload, 12), Some(vec![4, 0, 5]));
    payload[12 + 18 + 28 + 4] ^= 1;
    assert_eq!(compact_edge_selection_at(&payload, 12), Some(vec![4, 0, 5]));
}

#[test]
fn compact_edge_selection_accepts_object_terminated_u16_paths() {
    let marker = 12;
    let mut payload = vec![0; marker + 18];
    payload[..4].copy_from_slice(&4u32.to_le_bytes());
    payload[4..8].copy_from_slice(&[0, 2, 0, 0]);
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    payload.extend([0x0e, 0x02, 0x13, 0x02, 0x13, 0x02, 0x13, 0x02]);
    payload.extend([0; 8]);
    payload.extend([0xe2, 0x80, 0, 0]);

    assert_eq!(
        compact_edge_selection_at(&payload, marker),
        Some(vec![526, 531, 531, 531])
    );

    payload[marker + 18 + 8 + 7] = 1;
    assert_eq!(compact_edge_selection_at(&payload, marker), None);
    payload[marker + 18 + 8 + 7] = 0;
    payload[marker + 18 + 8 + 8] = 0xff;
    payload[marker + 18 + 8 + 9] = 0xff;
    assert_eq!(compact_edge_selection_at(&payload, marker), None);
}

#[test]
fn compact_edge_selection_rejects_unbounded_counts_and_short_headers() {
    let mut payload = vec![0; 40];
    payload[..4].copy_from_slice(&u32::MAX.to_le_bytes());
    payload[4..8].copy_from_slice(&[0, 2, 0, 0]);
    payload[12..28].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    assert_eq!(compact_edge_selection_at(&payload, 12), None);
    assert_eq!(compact_edge_component_path_at(&payload, 12), None);

    payload[..16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    assert_eq!(compact_edge_selection_at(&payload, 0), None);
    assert_eq!(compact_edge_component_path_at(&payload, 0), None);
    assert_eq!(compact_surface_selection_at(&payload, 0), None);
}

#[test]
fn compact_edge_selection_accepts_heterogeneous_component_paths() {
    let marker = 12;
    let mut payload = vec![0; 120];
    payload[..4].copy_from_slice(&2u32.to_le_bytes());
    payload[4..8].copy_from_slice(&[0, 2, 0, 0]);
    payload[8..12].copy_from_slice(&37u32.to_le_bytes());
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    let first = marker + 18;
    payload[first..first + 4].copy_from_slice(&[0x3d, 0x80, 0, 0]);
    payload[first + 4..first + 16].copy_from_slice(&[1; 12]);
    payload[first + 16..first + 20].copy_from_slice(&2u32.to_le_bytes());
    let second = first + 28;
    payload[second..second + 4].copy_from_slice(&[0x4a, 0x80, 0, 0]);
    payload[second + 4..second + 16].copy_from_slice(&[2; 12]);
    payload[second + 16..second + 20].copy_from_slice(&3u32.to_le_bytes());
    assert_eq!(
        compact_edge_selection_at(&payload, marker),
        Some(vec![2, 3])
    );
    assert_eq!(
        compact_edge_component_path_at(&payload, marker),
        Some(vec![
            FeatureInputComponentPathEntry {
                instance: Some(0x803d),
                type_signature: [1; 12],
                local_id: Some(2),
            },
            FeatureInputComponentPathEntry {
                instance: Some(0x804a),
                type_signature: [2; 12],
                local_id: Some(3),
            },
        ])
    );

    payload[..4].copy_from_slice(&3u32.to_le_bytes());
    let third = second + 24;
    payload[second + 20..third].fill(0xff);
    payload[third..third + 4].copy_from_slice(&[0x53, 0x80, 0, 0]);
    payload[third + 4..third + 16].copy_from_slice(&[3; 12]);
    payload[third + 16..third + 20].copy_from_slice(&4u32.to_le_bytes());
    assert_eq!(
        compact_edge_selection_at(&payload, marker),
        Some(vec![2, 3, 4])
    );
}

#[test]
fn compact_edge_selection_accepts_root_and_zero_run_separators() {
    let marker = 12;
    let mut payload = vec![0; 180];
    payload[..4].copy_from_slice(&4u32.to_le_bytes());
    payload[4..8].copy_from_slice(&[0, 2, 0, 0]);
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    let signature = [0xff, 0x80, 1, 1, 20, 0, 0, 0, 1, 0x42, 0x3e, 0x4f];
    let entry = |payload: &mut [u8], offset: usize, instance: u16, local_id: u32| {
        payload[offset..offset + 2].copy_from_slice(&instance.to_le_bytes());
        payload[offset + 4..offset + 16].copy_from_slice(&signature);
        payload[offset + 16..offset + 20].copy_from_slice(&local_id.to_le_bytes());
    };
    let first = marker + 18;
    entry(&mut payload, first, 0x862a, 1);
    let second = first + 20 + 12;
    entry(&mut payload, second, 0x8631, 10);
    payload[second + 20..second + 28].copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 1, 0, 0, 0]);
    let third = second + 28;
    entry(&mut payload, third, 0x8102, 1);
    payload[third + 20..third + 24].copy_from_slice(&[0xa3, 0x86, 1, 0]);
    let fourth = third + 24;
    entry(&mut payload, fourth, 0x8102, 0);

    assert_eq!(
        compact_edge_selection_at(&payload, marker),
        Some(vec![1, 10, 1, 0])
    );
}

#[test]
fn compact_edge_selection_with_wide_and_identifierless_entries_is_withheld() {
    let marker = 12;
    let mut payload = vec![0; 160];
    payload[..4].copy_from_slice(&4u32.to_le_bytes());
    payload[4..8].copy_from_slice(&[0, 2, 0, 0]);
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    let signature = [0x2a, 0x81, 0x2c, 1, 28, 0, 0, 0, 0x24, 1, 0xd3, 0x48];
    let entry = |payload: &mut [u8], offset: usize, instance: u16, local_id: u32| {
        payload[offset..offset + 2].copy_from_slice(&instance.to_le_bytes());
        payload[offset + 4..offset + 16].copy_from_slice(&signature);
        payload[offset + 20..offset + 24].copy_from_slice(&local_id.to_le_bytes());
    };
    let first = marker + 18;
    entry(&mut payload, first, 0x8130, 0);
    let second = first + 24;
    entry(&mut payload, second, 0x8130, 2);
    let third = second + 24;
    entry(&mut payload, third, 0x8141, 1);
    let fourth = third + 28;
    entry(&mut payload, fourth, 0x8141, 0);

    assert_eq!(compact_edge_selection_at(&payload, marker), None);
    assert_eq!(compact_edge_component_path_at(&payload, marker), None);
}

#[test]
fn compact_edge_selection_with_ambiguous_entry_widths_is_withheld() {
    let marker = 12;
    let mut payload = vec![0; 120];
    payload[..4].copy_from_slice(&2u32.to_le_bytes());
    payload[4..8].copy_from_slice(&[0, 2, 0, 0]);
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    let signature = [0x2a, 0x81, 0x2c, 1, 28, 0, 0, 0, 0x24, 1, 0xd3, 0x48];
    let first = marker + 18;
    payload[first..first + 2].copy_from_slice(&0x8130u16.to_le_bytes());
    payload[first + 4..first + 16].copy_from_slice(&signature);
    let second = first + 24;
    payload[second..second + 2].copy_from_slice(&0x8141u16.to_le_bytes());
    payload[second + 4..second + 16].copy_from_slice(&signature);
    payload[second + 20..second + 24].copy_from_slice(&5u32.to_le_bytes());

    assert_eq!(compact_edge_component_path_at(&payload, marker), None);
    assert_eq!(compact_edge_selection_at(&payload, marker), None);
}

#[test]
fn compact_edge_selection_accepts_ordinal_and_zero_separator() {
    let marker = 12;
    let mut payload = vec![0; 160];
    payload[..4].copy_from_slice(&3u32.to_le_bytes());
    payload[4..8].copy_from_slice(&[0, 2, 0, 0]);
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    let signature = [0x35, 0x80, 0x38, 0, 13, 1, 0, 0, 0x8a, 0xd8, 0x3f, 0x58];
    let entry = |payload: &mut [u8], offset: usize, instance: u16, local_id: u32| {
        payload[offset..offset + 2].copy_from_slice(&instance.to_le_bytes());
        payload[offset + 4..offset + 16].copy_from_slice(&signature);
        payload[offset + 16..offset + 20].copy_from_slice(&local_id.to_le_bytes());
    };
    let first = marker + 18;
    entry(&mut payload, first, 0x803e, 1);
    payload[first + 20..first + 24].copy_from_slice(&3u32.to_le_bytes());
    let second = first + 28;
    entry(&mut payload, second, 0x8385, 12);
    payload[second + 20..second + 24].copy_from_slice(&[0xff; 4]);
    let third = second + 28;
    entry(&mut payload, third, 0x8385, 12);

    assert_eq!(
        compact_edge_selection_at(&payload, marker),
        Some(vec![1, 12, 12])
    );
}

#[test]
fn compact_edge_selection_accepts_zero_and_state_separator() {
    let marker = 12;
    let mut payload = vec![0; 128];
    payload[..4].copy_from_slice(&2u32.to_le_bytes());
    payload[4..8].copy_from_slice(&[0, 2, 0, 0]);
    payload[8..12].copy_from_slice(&375_491u32.to_le_bytes());
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    let signature = [0x34, 0x80, 0x37, 0, 37, 0, 0, 0, 0x3e, 0x77, 0x0e, 0x60];
    let entry = |payload: &mut [u8], offset: usize, local_id: u32| {
        payload[offset..offset + 2].copy_from_slice(&0x8158u16.to_le_bytes());
        payload[offset + 4..offset + 16].copy_from_slice(&signature);
        payload[offset + 16..offset + 20].copy_from_slice(&local_id.to_le_bytes());
    };
    let first = marker + 18;
    entry(&mut payload, first, 3);
    payload[first + 24..first + 28].copy_from_slice(&1u32.to_le_bytes());
    let second = first + 28;
    entry(&mut payload, second, 2);

    assert_eq!(
        compact_edge_selection_at(&payload, marker),
        Some(vec![3, 2])
    );
}

#[test]
fn compact_edge_selection_preserves_an_idless_path_entry() {
    let marker = 12;
    let mut payload = vec![0; 160];
    payload[..4].copy_from_slice(&4u32.to_le_bytes());
    payload[4..8].copy_from_slice(&[0, 2, 0, 0]);
    payload[8..12].copy_from_slice(&2_366_854u32.to_le_bytes());
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    let entry =
        |payload: &mut [u8], offset: usize, instance: u16, source: u32, local_id: Option<u32>| {
            payload[offset..offset + 2].copy_from_slice(&instance.to_le_bytes());
            payload[offset + 4..offset + 8].copy_from_slice(&[0xe8, 0x80, 0xea, 0]);
            payload[offset + 8..offset + 12].copy_from_slice(&source.to_le_bytes());
            payload[offset + 12..offset + 16].copy_from_slice(&[0x1e, 0x0a, 0xca, 0x5a]);
            if let Some(local_id) = local_id {
                payload[offset + 16..offset + 20].copy_from_slice(&local_id.to_le_bytes());
            }
        };
    let first = marker + 18;
    entry(&mut payload, first, 0x80eb, 130, Some(4));
    let second = first + 20;
    entry(&mut payload, second, 0x86e9, 172, None);
    let third = second + 16;
    entry(&mut payload, third, 0x80ee, 152, Some(4));
    payload[third + 20..third + 24].copy_from_slice(&[0xff; 4]);
    let fourth = third + 28;
    entry(&mut payload, fourth, 0x80f8, 130, Some(0));

    assert_eq!(
        compact_edge_selection_at(&payload, marker),
        Some(vec![4, 4, 0])
    );
    let components = compact_edge_component_path_at(&payload, marker).unwrap();
    assert_eq!(
        components
            .iter()
            .map(|component| component.local_id)
            .collect::<Vec<_>>(),
        [Some(4), None, Some(4), Some(0)]
    );
    let selection = FeatureInputEdgeSelection {
        id: "selection".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: marker as u64,
        object_name_ref: "name".into(),
        feature_ref: "consumer".into(),
        local_edge_ids: vec![4, 4, 0],
        components,
        references: Vec::new(),
        producer_feature_refs: vec!["producer".into()],
        terminal_feature_ref: Some("producer".into()),
    };
    assert_eq!(compact_edge_path_value(&selection), "4,_,4,0");
    assert_eq!(
        compact_edge_selection_set_value(&[&selection]),
        "sldprt:feature-input:edge-ids:4,_,4,0"
    );
}

#[test]
fn compact_edge_selection_marker_does_not_require_a_class_declaration() {
    let native_feature =
        |id: &str, name: &str, source_id: Option<u32>, ordinal: u32, input_class: &str| Feature {
            id: id.into(),
            parent: "history".into(),
            xml_tag: "Feature".into(),
            tree_parent: None,
            source_id: source_id.map(|source_id| source_id.to_string()),
            parent_source_id: None,
            ordinal,
            name: name.into(),
            kind: "Feature".into(),
            input_class: Some(input_class.into()),
            suppressed: false,
            parameters: BTreeMap::new(),
            dimension_properties: BTreeMap::new(),
            properties: BTreeMap::new(),
            text: None,
            content: Vec::new(),
        };
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![
            native_feature("producer", "Producer", None, 0, "moExtrusion_c"),
            native_feature("consumer", "Consumer", Some(2), 1, "Chamfer_c"),
        ],
    };
    let marker = 52;
    let mut payload = vec![0; 96];
    payload[marker - 12..marker - 8].copy_from_slice(&1u32.to_le_bytes());
    payload[marker - 8..marker - 4].copy_from_slice(&[0, 2, 0, 0]);
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    let entry = marker + 18;
    payload[entry..entry + 2].copy_from_slice(&0x8130u16.to_le_bytes());
    payload[entry + 4..entry + 8].copy_from_slice(&[0x2a, 0x81, 0x2c, 1]);
    payload[entry + 8..entry + 12].copy_from_slice(&1u32.to_le_bytes());
    payload[entry + 12..entry + 16].copy_from_slice(&[0x24, 1, 0xd3, 0x48]);
    payload[entry + 16..entry + 20].copy_from_slice(&7u32.to_le_bytes());
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: Vec::new(),
        names: vec![
            FeatureInputName {
                id: "producer-name".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 0,
                object_id: Some(1),
                value: "Producer".into(),
            },
            FeatureInputName {
                id: "consumer-name".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: 24,
                object_id: Some(2),
                value: "Consumer".into(),
            },
        ],
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };

    let selections = compact_edge_selections(&[history], &lane);

    assert_eq!(selections.len(), 1);
    assert_eq!(selections[0].feature_ref, "consumer");
    assert_eq!(selections[0].local_edge_ids, [7]);
    assert_eq!(
        selections[0].terminal_feature_ref.as_deref(),
        Some("producer")
    );
}

#[test]
fn fillet_edge_roster_ends_at_direct_or_repeated_vertex_dimension() {
    let class_name = "moVertDim_c";
    let direct_record = 40;
    let class_offset = direct_record + 4;
    let class_body = class_offset + 6 + class_name.len();
    let repeated_record = 104;
    let class_token = 0x87d3_u16;
    let mut payload = vec![0; 144];
    payload[direct_record..direct_record + 4].copy_from_slice(&[0x20, 0x81, 0x08, 0x00]);
    payload[class_offset..class_offset + 4].copy_from_slice(CLASS_MARKER);
    payload[class_offset + 4..class_offset + 6]
        .copy_from_slice(&(class_name.len() as u16).to_le_bytes());
    payload[class_offset + 6..class_body].copy_from_slice(class_name.as_bytes());
    payload[class_body..class_body + 2].copy_from_slice(&class_token.to_le_bytes());
    payload[repeated_record..repeated_record + 4].copy_from_slice(&[0x20, 0x81, 0x10, 0x00]);
    payload[repeated_record + 4..repeated_record + 6].copy_from_slice(&0x8123_u16.to_le_bytes());
    payload[repeated_record + 6..repeated_record + 8].copy_from_slice(&class_token.to_le_bytes());
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: vec![FeatureInputClass {
            id: "vertex-dimension-class".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: class_offset as u64,
            name: class_name.into(),
            role: FeatureInputClassRole::Dimension,
        }],
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };

    assert_eq!(fillet_edge_roster_end(&lane, 0, 80), Some(direct_record));
    assert_eq!(
        fillet_edge_roster_end(&lane, 80, 144),
        Some(repeated_record)
    );
}

#[test]
fn compact_edge_selection_excludes_terminal_feature_reference_cell() {
    let marker = 12;
    let mut payload = vec![0; 160];
    payload[..4].copy_from_slice(&4u32.to_le_bytes());
    payload[4..8].copy_from_slice(&[0, 2, 0, 0]);
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    let signature = [0x34, 0x80, 0x37, 0, 121, 0, 0, 0, 0x9b, 0x95, 0x90, 0x5f];
    let mut cursor = marker + 18;
    for (index, local_id) in [32u32, 34, 1].into_iter().enumerate() {
        payload[cursor..cursor + 4].copy_from_slice(&[0x3d, 0x80, 0, 0]);
        payload[cursor + 4..cursor + 16].copy_from_slice(&signature);
        payload[cursor + 16..cursor + 20].copy_from_slice(&local_id.to_le_bytes());
        cursor += 20;
        if index != 2 {
            payload[cursor..cursor + 8].copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]);
            cursor += 8;
        }
    }
    payload[cursor..cursor + 36].copy_from_slice(&[
        1, 0, 0, 0, 0, 0, 0, 0, 0x4a, 0x80, 0, 0, 0x34, 0x80, 0x37, 0, 35, 0, 0, 0, 0x89, 0x6b,
        0x90, 0x5f, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ]);
    assert_eq!(
        compact_edge_selection_at(&payload, marker),
        Some(vec![32, 34, 1])
    );
    assert_eq!(
        compact_edge_component_path_at(&payload, marker).map(|components| components.len()),
        Some(3)
    );
}

#[test]
fn compact_reference_list_preserves_reference_and_hop_boundaries() {
    let marker = 12;
    let mut payload = Vec::new();
    payload.extend(2u32.to_le_bytes());
    payload.extend([0, 2, 0, 0]);
    payload.extend(17u32.to_le_bytes());
    payload.extend(COMPACT_EDGE_VECTOR_MARKER);
    payload.extend([0, 0]);
    let append_hop = |payload: &mut Vec<u8>, instance: u16, serial: u32, timestamp: u32| {
        payload.extend(instance.to_le_bytes());
        payload.extend([0, 0]);
        payload.extend([0x38, 0x80, 0x3b, 0]);
        payload.extend(serial.to_le_bytes());
        payload.extend(timestamp.to_le_bytes());
    };
    append_hop(&mut payload, 0x8036, 4, 40);
    append_hop(&mut payload, 0x8041, 5, 50);
    payload.extend(12u32.to_le_bytes());
    append_hop(&mut payload, 0x8083, 9, 90);
    payload.extend(0u32.to_le_bytes());
    payload.extend([0xff; 4]);
    payload.extend([0; 6]);

    let references = compact_component_reference_list_at(&payload, marker).unwrap();
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].len(), 2);
    assert_eq!(references[0][0].local_id, None);
    assert_eq!(references[0][1].local_id, Some(12));
    assert_eq!(references[1][0].instance, Some(0x8083));
    assert_eq!(references[1][0].local_id, Some(0));
    assert_eq!(
        compact_edge_selection_at(&payload, marker),
        Some(vec![12, 0])
    );
    assert_eq!(
        compact_edge_component_path_at(&payload, marker)
            .unwrap()
            .iter()
            .map(|component| component.local_id)
            .collect::<Vec<_>>(),
        [None, Some(12)]
    );

    payload[4] = 0x06;
    for prefix_offset in [34, 50, 70] {
        payload[prefix_offset..prefix_offset + 4].copy_from_slice(&[0xa7, 0x81, 0xa9, 0x01]);
    }
    assert_eq!(
        compact_component_reference_list_at(&payload, marker)
            .unwrap()
            .len(),
        2
    );

    payload[..4].copy_from_slice(&3u32.to_le_bytes());
    payload.extend([0; 10]);
    payload.extend([0xff, 0xfe, 0xff]);
    assert_eq!(
        compact_component_reference_list_at(&payload, marker)
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn compact_reference_list_accepts_unframed_surface_cut_targets() {
    let marker = 12;
    let prefix = [0xa7, 0x81, 0xa9, 0x01];
    let signature = |serial: u32, timestamp: u32| {
        let mut value = [0; 12];
        value[..4].copy_from_slice(&prefix);
        value[4..8].copy_from_slice(&serial.to_le_bytes());
        value[8..].copy_from_slice(&timestamp.to_le_bytes());
        value
    };
    let mut payload = Vec::new();
    payload.extend(5u32.to_le_bytes());
    payload.extend([0, 2, 0, 0]);
    payload.extend(0u32.to_le_bytes());
    payload.extend(COMPACT_EDGE_VECTOR_MARKER);
    payload.extend([0, 0]);
    for local_id in [0u32, 3, 2] {
        payload.extend(0x81a5u16.to_le_bytes());
        payload.extend([0, 0]);
        payload.extend(signature(18, 0x51c5_fde3));
        payload.extend(local_id.to_le_bytes());
    }
    payload.extend([0; 16]);

    let references = compact_component_reference_list(&payload, marker, false)
        .expect("operation target reference list");
    assert_eq!(
        references
            .iter()
            .flatten()
            .filter_map(|component| component.local_id)
            .collect::<Vec<_>>(),
        [0, 3, 2]
    );
    assert!(compact_component_reference_list_at(&payload, marker).is_none());
    assert!(surface_reference_matches_at(
        &payload,
        marker,
        &references.into_iter().flatten().collect::<Vec<_>>()
    ));
}

#[test]
fn compact_edge_selection_accepts_counted_u16_ids() {
    let marker = 12;
    let mut payload = vec![0; 80];
    payload[..4].copy_from_slice(&3u32.to_le_bytes());
    payload[4..8].copy_from_slice(&[0, 2, 0, 0]);
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    let ids = marker + 18;
    payload[ids..ids + 6].copy_from_slice(&[4, 0, 8, 0, 12, 0]);
    payload[ids + 22..ids + 25].copy_from_slice(&[0xff, 0xfe, 0xff]);
    assert_eq!(
        compact_edge_selection_at(&payload, marker),
        Some(vec![4, 8, 12])
    );
    assert_eq!(compact_edge_component_path_at(&payload, marker), None);
}

#[test]
fn compact_surface_selection_ends_with_its_entry_signature() {
    let mut payload = Vec::new();
    payload.extend(6u32.to_le_bytes());
    payload.extend([0x04, 0x02, 0, 0]);
    payload.extend(0x1234u32.to_le_bytes());
    payload.extend(COMPACT_EDGE_VECTOR_MARKER);
    payload.extend([0, 0]);
    let signature = [0x34, 0x80, 0x37, 0, 0x89, 0, 0, 0, 0xe2, 0x56, 0xdf, 0x5e];
    for (index, id) in [2u32, 1, 11, 14, 15, 16, 17].into_iter().enumerate() {
        payload.extend((0x8c20u32 + index as u32).to_le_bytes());
        payload.extend(signature);
        payload.extend(id.to_le_bytes());
        if index == 0 {
            payload.extend(1u32.to_le_bytes());
        }
    }
    payload.extend([0; 24]);
    let components = compact_surface_selection_at(&payload, 12).expect("required invariant");
    assert_eq!(
        components
            .iter()
            .map(|component| (
                component.instance,
                component.type_signature,
                component.local_id
            ))
            .collect::<Vec<_>>(),
        vec![
            (Some(0x8c20), signature, Some(2)),
            (Some(0x8c21), signature, Some(1)),
            (Some(0x8c22), signature, Some(11)),
            (Some(0x8c23), signature, Some(14)),
            (Some(0x8c24), signature, Some(15)),
            (Some(0x8c25), signature, Some(16)),
            (Some(0x8c26), signature, Some(17))
        ]
    );
    payload[12 + 18 + 24 + 4] ^= 1;
    assert_eq!(
        compact_surface_selection_at(&payload, 12)
            .expect("required invariant")
            .iter()
            .map(|component| component.local_id)
            .collect::<Vec<_>>(),
        vec![Some(2)]
    );
    payload[4] = 0x06;
    assert_eq!(
        compact_surface_selection_at(&payload, 12)
            .expect("nonzero selector subtype")
            .first()
            .and_then(|component| component.local_id),
        Some(2)
    );
}

#[test]
fn operation_surface_selection_finds_marker_inside_class_body() {
    let class_name = "moCompSurfaceBody_c";
    let class_body = 6 + class_name.len();
    let marker = class_body + 43;
    let entry = marker + 18;
    let signature = [
        0x23, 0x86, 0x25, 0x06, 0x02, 0x02, 0, 0, 0xc3, 0xea, 0xde, 0x51,
    ];
    let mut payload = vec![0; entry + 20];
    payload[..4].copy_from_slice(CLASS_MARKER);
    payload[4..6].copy_from_slice(&(class_name.len() as u16).to_le_bytes());
    payload[6..class_body].copy_from_slice(class_name.as_bytes());
    payload[class_body..class_body + 2].copy_from_slice(&0x860eu16.to_le_bytes());
    payload[marker - 12..marker - 8].copy_from_slice(&6u32.to_le_bytes());
    payload[marker - 8..marker - 4].copy_from_slice(&[0x04, 0x02, 0, 0]);
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    payload[entry..entry + 2].copy_from_slice(&0x8781u16.to_le_bytes());
    payload[entry + 4..entry + 16].copy_from_slice(&signature);
    payload[entry + 16..entry + 20].copy_from_slice(&6u32.to_le_bytes());
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload.clone(),
        classes: vec![FeatureInputClass {
            id: "class".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            name: class_name.into(),
            role: FeatureInputClassRole::Reference,
        }],
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };

    let selections = operation_surface_selection_candidates(
        FeatureClass::TrimSurface,
        &lane,
        0,
        payload.len(),
        None,
    );

    assert_eq!(selections.len(), 1);
    assert_eq!(selections[0].0, marker);
    assert_eq!(selections[0].1[0].local_id, Some(6));
}

#[test]
fn cosmetic_thread_cylinder_reference_uses_the_typed_child_layout() {
    let body_offset = 30;
    let marker = body_offset + 94;
    let mut payload = vec![0; marker - 12];
    payload[body_offset..body_offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
    payload[body_offset + 2..body_offset + 4].copy_from_slice(&0x802b_u16.to_le_bytes());
    payload[body_offset + 4..body_offset + 8].copy_from_slice(&2u32.to_le_bytes());
    let actual_marker = selection_vector_tail(&mut payload, &[3]);
    assert_eq!(actual_marker, marker);
    let (actual_marker, components) =
        cosmetic_thread_cylinder_reference_at(&payload, body_offset).expect("required invariant");
    assert_eq!(actual_marker, marker);
    assert_eq!(
        components.last().expect("required invariant").local_id,
        Some(3)
    );

    let compact_marker = body_offset + 66;
    let mut compact = vec![0; compact_marker - 12];
    compact[body_offset..body_offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
    compact[body_offset + 2..body_offset + 4].copy_from_slice(&0x802b_u16.to_le_bytes());
    compact[body_offset + 4..body_offset + 8].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(selection_vector_tail(&mut compact, &[5]), compact_marker);
    let (actual_marker, components) =
        cosmetic_thread_cylinder_reference_at(&compact, body_offset).expect("required invariant");
    assert_eq!(actual_marker, compact_marker);
    assert_eq!(
        components.last().expect("required invariant").local_id,
        Some(5)
    );

    let selected_marker = body_offset + 70;
    let mut selected = vec![0; selected_marker - 12];
    selected[body_offset..body_offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
    selected[body_offset + 2..body_offset + 4].copy_from_slice(&0x802b_u16.to_le_bytes());
    selected[body_offset + 4..body_offset + 8].copy_from_slice(&2u32.to_le_bytes());
    selected[body_offset + 8] = 0x40;
    assert_eq!(selection_vector_tail(&mut selected, &[7]), selected_marker);
    let (actual_marker, components) =
        cosmetic_thread_cylinder_reference_at(&selected, body_offset).expect("required invariant");
    assert_eq!(actual_marker, selected_marker);
    assert_eq!(
        components.last().expect("required invariant").local_id,
        Some(7)
    );

    let extended_marker = body_offset + 106;
    let mut extended = vec![0; extended_marker - 12];
    extended[body_offset..body_offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
    extended[body_offset + 2..body_offset + 4].copy_from_slice(&0x802b_u16.to_le_bytes());
    extended[body_offset + 4..body_offset + 8].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(selection_vector_tail(&mut extended, &[9]), extended_marker);
    let (actual_marker, components) =
        cosmetic_thread_cylinder_reference_at(&extended, body_offset).expect("required invariant");
    assert_eq!(actual_marker, extended_marker);
    assert_eq!(
        components.last().expect("required invariant").local_id,
        Some(9)
    );

    let compact_legacy_marker = body_offset + 46;
    let mut compact_legacy = vec![0; compact_legacy_marker - 12];
    compact_legacy[body_offset..body_offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
    compact_legacy[body_offset + 2..body_offset + 4].copy_from_slice(&0x802b_u16.to_le_bytes());
    compact_legacy[body_offset + 4..body_offset + 8].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        selection_vector_tail(&mut compact_legacy, &[10]),
        compact_legacy_marker
    );
    let (actual_marker, components) =
        cosmetic_thread_cylinder_reference_at(&compact_legacy, body_offset)
            .expect("required invariant");
    assert_eq!(actual_marker, compact_legacy_marker);
    assert_eq!(
        components.last().expect("required invariant").local_id,
        Some(10)
    );

    let legacy_marker = body_offset + 102;
    let mut legacy = vec![0; legacy_marker - 12];
    legacy[body_offset..body_offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
    legacy[body_offset + 2..body_offset + 4].copy_from_slice(&0x802b_u16.to_le_bytes());
    legacy[body_offset + 4..body_offset + 8].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(selection_vector_tail(&mut legacy, &[11]), legacy_marker);
    let (actual_marker, components) =
        cosmetic_thread_cylinder_reference_at(&legacy, body_offset).expect("required invariant");
    assert_eq!(actual_marker, legacy_marker);
    assert_eq!(
        components.last().expect("required invariant").local_id,
        Some(11)
    );

    let extended_marker = body_offset + 110;
    let mut extended = vec![0; extended_marker - 12];
    extended[body_offset..body_offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
    extended[body_offset + 2..body_offset + 4].copy_from_slice(&0x802b_u16.to_le_bytes());
    extended[body_offset + 4..body_offset + 8].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(selection_vector_tail(&mut extended, &[12]), extended_marker);
    let (actual_marker, components) =
        cosmetic_thread_cylinder_reference_at(&extended, body_offset).expect("required invariant");
    assert_eq!(actual_marker, extended_marker);
    assert_eq!(
        components.last().expect("required invariant").local_id,
        Some(12)
    );

    for (relative, local_id) in [(62, 13), (90, 14)] {
        let marker = body_offset + relative;
        let mut payload = vec![0; marker - 12];
        payload[body_offset..body_offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
        payload[body_offset + 2..body_offset + 4].copy_from_slice(&0x802b_u16.to_le_bytes());
        payload[body_offset + 4..body_offset + 8].copy_from_slice(&2u32.to_le_bytes());
        assert_eq!(selection_vector_tail(&mut payload, &[local_id]), marker);
        let (actual_marker, components) =
            cosmetic_thread_cylinder_reference_at(&payload, body_offset)
                .expect("required invariant");
        assert_eq!(actual_marker, marker);
        assert_eq!(
            components.last().expect("required invariant").local_id,
            Some(local_id)
        );
    }

    assert_eq!(
        cosmetic_thread_cylinder_reference_at(&payload, body_offset + 1),
        None
    );

    let mut payload = vec![0; marker - 12];
    payload[body_offset..body_offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
    payload[body_offset + 2..body_offset + 4].copy_from_slice(&0x802b_u16.to_le_bytes());
    payload[body_offset + 4..body_offset + 8].copy_from_slice(&2u32.to_le_bytes());
    payload.extend(3u32.to_le_bytes());
    payload.extend([0, 2, 0, 0]);
    payload.extend([0; 4]);
    payload.extend(COMPACT_EDGE_VECTOR_MARKER);
    payload.extend([0; 2]);
    for (instance, signature, local_id, gap) in [
        (0x8032_u16, [1; 12], 3_u32, Some(6_u32)),
        (0x803e, [2; 12], 7, None),
    ] {
        payload.extend(instance.to_le_bytes());
        payload.extend([0; 2]);
        payload.extend(signature);
        payload.extend(local_id.to_le_bytes());
        if let Some(gap) = gap {
            payload.extend(gap.to_le_bytes());
        }
    }
    let (_, components) =
        cosmetic_thread_cylinder_reference_at(&payload, body_offset).expect("required invariant");
    assert_eq!(
        components
            .iter()
            .map(|component| component.local_id)
            .collect::<Vec<_>>(),
        [Some(3), Some(7)]
    );
}

#[test]
fn cosmetic_thread_retains_unique_cylinder_marker_without_component_path() {
    let body_offset = 30;
    let marker = body_offset + 94;
    let mut payload = vec![0; marker - 12];
    payload[body_offset..body_offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
    payload[body_offset + 2..body_offset + 4].copy_from_slice(&0x802b_u16.to_le_bytes());
    payload[body_offset + 4..body_offset + 8].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(selection_vector_tail(&mut payload, &[3]), marker);
    payload.truncate(marker + 18);
    let feature = Feature {
        id: "thread".into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some("20".into()),
        parent_source_id: None,
        ordinal: 0,
        name: "thread".into(),
        kind: "Feature".into(),
        input_class: Some("moCosmeticThread_c".into()),
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
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
        sketch_entities: Vec::new(),
    };

    assert_eq!(
        cosmetic_thread_cylinder_marker_reference(
            &feature,
            &lane,
            0,
            lane.native_payload.len(),
            &HashSet::from([0x802f]),
        ),
        vec![(marker, None)]
    );
}

#[test]
fn cosmetic_thread_cylinder_reference_follows_its_owned_diameter_child() {
    let body_offset = 220;
    let marker = body_offset + 94;
    let mut payload = vec![0; marker - 12];
    payload[body_offset..body_offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
    payload[body_offset + 2..body_offset + 4].copy_from_slice(&0x802d_u16.to_le_bytes());
    payload[body_offset + 4..body_offset + 8].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(selection_vector_tail(&mut payload, &[3]), marker);
    payload.resize(500, 0);

    let feature = Feature {
        id: "thread".into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some("53".into()),
        parent_source_id: None,
        ordinal: 0,
        name: "Thread".into(),
        kind: "Feature".into(),
        input_class: Some("moCosmeticThread_c".into()),
        suppressed: false,
        parameters: BTreeMap::from([("D2".into(), "<MOD-DIAM>8".into())]),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let diameter = FeatureInputScalar {
        id: "diameter".into(),
        parent: "lane".into(),
        feature_ref: Some("other-feature".into()),
        ordinal: 0,
        offset: 150,
        object_id: 52,
        name: "diameter-name".into(),
        value: 0.008,
        role: FeatureInputScalarRole::Native,
        entity_indices: Vec::new(),
        operands: Vec::new(),
    };
    let mut lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: Vec::new(),
        names: vec![
            FeatureInputName {
                id: "diameter-name".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 120,
                object_id: Some(u32::MAX),
                value: "D2".into(),
            },
            FeatureInputName {
                id: "next-feature".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: 400,
                object_id: Some(54),
                value: "Next".into(),
            },
        ],
        scalars: vec![diameter],
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    assert_eq!(
        cosmetic_thread_diameter_child_tail(&feature, &lane),
        Some(158..400)
    );
    let references =
        cosmetic_thread_cylinder_references(&feature, &lane, 20, 100, &HashSet::from([0x802f]));
    assert_eq!(
        references
            .iter()
            .map(|(offset, components)| (*offset, components[0].local_id))
            .collect::<Vec<_>>(),
        [(marker, Some(3))]
    );

    lane.scalars.push(FeatureInputScalar {
        id: "next-scalar".into(),
        parent: "lane".into(),
        feature_ref: None,
        ordinal: 1,
        offset: 200,
        object_id: 54,
        name: "next-feature".into(),
        value: 1.0,
        role: FeatureInputScalarRole::Native,
        entity_indices: Vec::new(),
        operands: Vec::new(),
    });
    assert!(cosmetic_thread_cylinder_references(
        &feature,
        &lane,
        20,
        100,
        &HashSet::from([0x802f]),
    )
    .is_empty());
}

#[test]
fn component_face_reference_accepts_both_nested_body_flags() {
    let body_offset = 30;
    let build_payload = |flag: u8, marker: usize| {
        let mut payload = vec![0; marker - 12];
        payload[body_offset..body_offset + 2].copy_from_slice(&0x802b_u16.to_le_bytes());
        payload[body_offset + 2..body_offset + 6].copy_from_slice(&2u32.to_le_bytes());
        payload[body_offset + 6] = flag;
        assert_eq!(selection_vector_tail(&mut payload, &[6]), marker);
        payload
    };
    let marker = body_offset + 92;
    let mut payload = build_payload(0, marker);

    let (actual_marker, components) =
        component_face_reference_at(&payload, body_offset).expect("required invariant");
    assert_eq!(actual_marker, marker);
    assert_eq!(
        components.last().expect("required invariant").local_id,
        Some(6)
    );

    let compact = build_payload(0, body_offset + 68);
    assert!(component_face_reference_at(&compact, body_offset).is_some());

    let flagged = build_payload(0x40, body_offset + 100);
    assert!(component_face_reference_at(&flagged, body_offset).is_some());
    let mut record = CLASS_MARKER.to_vec();
    record.extend((b"moCompFace_c".len() as u16).to_le_bytes());
    record.extend(b"moCompFace_c");
    record.extend_from_slice(&flagged[body_offset..]);
    assert!(component_face_reference_in_record(&record).is_some());

    payload[body_offset + 6] = 1;
    assert_eq!(component_face_reference_at(&payload, body_offset), None);
}

#[test]
fn sketch_surface_component_path_has_two_implicit_root_slots() {
    let marker = 12;
    let mut payload = Vec::new();
    payload.extend(5u32.to_le_bytes());
    payload.extend([0, 3, 0, 0]);
    payload.extend([0; 4]);
    payload.extend(COMPACT_EDGE_VECTOR_MARKER);
    payload.extend([0; 2]);
    for (index, local_id) in [4u32, 3, 5].into_iter().enumerate() {
        if index == 2 {
            payload.extend([0; 2]);
        }
        payload.extend((0x8094 + index as u16).to_le_bytes());
        payload.extend([0; 2]);
        payload.extend([index as u8 + 1; 12]);
        payload.extend(local_id.to_le_bytes());
    }

    assert_eq!(
        compact_sketch_surface_component_path_at(&payload, marker)
            .expect("required invariant")
            .iter()
            .map(|component| component.local_id)
            .collect::<Vec<_>>(),
        [Some(4), Some(3), Some(5)]
    );
}

#[test]
fn sketch_surface_component_path_accepts_a_slot_cell_between_entries() {
    let marker = 12;
    let mut payload = Vec::new();
    payload.extend(5u32.to_le_bytes());
    payload.extend([0, 3, 0, 0]);
    payload.extend([0; 4]);
    payload.extend(COMPACT_EDGE_VECTOR_MARKER);
    payload.extend([0; 2]);
    for (index, local_id) in [2u32, 0, 1].into_iter().enumerate() {
        if index == 1 {
            payload.extend([0; 4]);
        } else if index == 2 {
            payload.extend([1, 0, 0, 0, 0, 0]);
        }
        payload.extend((0x8034 + index as u16).to_le_bytes());
        payload.extend([0; 2]);
        payload.extend([index as u8 + 1; 12]);
        payload.extend(local_id.to_le_bytes());
    }

    assert_eq!(
        compact_sketch_surface_component_path_at(&payload, marker)
            .expect("required invariant")
            .iter()
            .map(|component| component.local_id)
            .collect::<Vec<_>>(),
        [Some(2), Some(0), Some(1)]
    );

    let slot = marker + 18 + 20 + 4 + 20;
    payload[slot..slot + 6].fill(0);
    assert_eq!(
        compact_sketch_surface_component_path_at(&payload, marker)
            .expect("required invariant")
            .iter()
            .map(|component| component.local_id)
            .collect::<Vec<_>>(),
        [Some(2), Some(0), Some(1)]
    );

    payload[slot..slot + 2].fill(0xff);
    assert_eq!(
        compact_sketch_surface_component_path_at(&payload, marker),
        None
    );
}

#[test]
fn legacy_sketch_surface_component_path_requires_its_ownership_trailer() {
    let marker = 12;
    let mut payload = Vec::new();
    payload.extend(5u32.to_le_bytes());
    payload.extend([0, 2, 0, 0]);
    payload.extend(7u32.to_le_bytes());
    payload.extend(COMPACT_EDGE_VECTOR_MARKER);
    payload.extend([0; 2]);
    for (index, local_id) in [2u32, 1, 0].into_iter().enumerate() {
        if index == 1 {
            payload.extend(3u32.to_le_bytes());
        } else if index == 2 {
            payload.extend(12u16.to_le_bytes());
            payload.extend([0; 4]);
        }
        payload.extend((0x8032 + index as u16).to_le_bytes());
        payload.extend([0; 2]);
        payload.extend([index as u8 + 1; 12]);
        payload.extend(local_id.to_le_bytes());
    }
    let trailer = payload.len();
    payload.extend([0; 20]);
    payload.extend(1u32.to_le_bytes());
    payload.extend(0u32.to_le_bytes());
    payload.extend(175u32.to_le_bytes());
    payload.extend([0; 12]);

    assert_eq!(
        compact_sketch_surface_component_path_at(&payload, marker)
            .expect("required invariant")
            .iter()
            .map(|component| component.local_id)
            .collect::<Vec<_>>(),
        [Some(2), Some(1), Some(0)]
    );

    payload[trailer + 28..trailer + 32].fill(0);
    assert_eq!(
        compact_sketch_surface_component_path_at(&payload, marker),
        None
    );

    payload[trailer + 28..trailer + 32].copy_from_slice(&175u32.to_le_bytes());
    payload.truncate(trailer);
    payload.extend(14u32.to_le_bytes());
    payload.extend([0; 8]);
    payload.extend(1u32.to_le_bytes());
    payload.extend(0u32.to_le_bytes());
    payload.extend(135u32.to_le_bytes());
    payload.extend([0; 12]);
    assert!(compact_sketch_surface_component_path_at(&payload, marker).is_some());

    payload[trailer..trailer + 4].fill(0);
    assert_eq!(
        compact_sketch_surface_component_path_at(&payload, marker),
        None
    );

    payload.truncate(trailer);
    payload.extend([0; 8]);
    payload.extend(1u32.to_le_bytes());
    payload.extend(0u32.to_le_bytes());
    payload.extend(135u32.to_le_bytes());
    payload.extend([0; 12]);
    assert!(compact_sketch_surface_component_path_at(&payload, marker).is_some());

    payload[trailer + 16..trailer + 20].fill(0);
    assert_eq!(
        compact_sketch_surface_component_path_at(&payload, marker),
        None
    );
}

#[test]
fn mirror_pattern_path_count_includes_the_unserialized_root_cell() {
    let marker = 12;
    let mut payload = vec![0; marker];
    payload[..4].copy_from_slice(&4u32.to_le_bytes());
    payload.extend(COMPACT_EDGE_VECTOR_MARKER);
    payload.extend([0, 0]);
    for (index, (instance, signature)) in [
        (
            0x803e_u16,
            [0x34, 0x80, 0x37, 0, 37, 0, 0, 0, 0x7a, 0x83, 0xd9, 0x4a],
        ),
        (
            0x8263,
            [0x34, 0x80, 0x37, 0, 50, 0, 0, 0, 0xf9, 0x83, 0xd9, 0x4a],
        ),
        (
            0x803e,
            [0x34, 0x80, 0x37, 0, 37, 0, 0, 0, 0x7a, 0x83, 0xd9, 0x4a],
        ),
    ]
    .into_iter()
    .enumerate()
    {
        if index == 2 {
            payload.extend([0; 8]);
        }
        payload.extend(instance.to_le_bytes());
        payload.extend([0, 0]);
        payload.extend(signature);
        payload.extend([2u32, 1, 3][index].to_le_bytes());
    }
    payload.extend([0; 32]);

    let path = mirror_pattern_component_path_at(&payload, marker).expect("required invariant");
    assert_eq!(path.len(), 3);
    assert_eq!(path.last().expect("required invariant").local_id, Some(3));
    assert_eq!(
        &path.last().expect("required invariant").type_signature[4..8],
        &37u32.to_le_bytes()
    );

    payload[..4].copy_from_slice(&5u32.to_le_bytes());
    assert_eq!(
        mirror_pattern_component_path_at(&payload, marker)
            .expect("two root slots")
            .len(),
        3
    );
    payload[4] = 1;
    assert!(mirror_pattern_component_path_at(&payload, marker).is_none());

    for (count, separator) in [
        (3u32, &[][..]),
        (4, &[1, 0, 0, 0, 0, 0, 0, 0][..]),
        (5, &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0][..]),
        (4, &[5, 0, 0, 0][..]),
    ] {
        let mut mixed = vec![0; marker];
        mixed[..4].copy_from_slice(&count.to_le_bytes());
        mixed.extend(COMPACT_EDGE_VECTOR_MARKER);
        mixed.extend([0, 0]);
        mixed.extend(0x803e_u16.to_le_bytes());
        mixed.extend([0, 0]);
        mixed.extend([0x34, 0x80, 0x37, 0, 37, 0, 0, 0, 0x7a, 0x83, 0xd9, 0x4a]);
        mixed.extend(2u32.to_le_bytes());
        mixed.extend(separator);
        mixed.extend([0x34, 0x80, 0x37, 0, 50, 0, 0, 0, 0xf9, 0x83, 0xd9, 0x4a]);
        mixed.extend(1u32.to_le_bytes());
        mixed.extend(0x8263_u16.to_le_bytes());
        mixed.extend([0, 0]);
        mixed.extend([0x34, 0x80, 0x37, 0, 37, 0, 0, 0, 0x7a, 0x83, 0xd9, 0x4a]);
        mixed.extend(3u32.to_le_bytes());
        assert_eq!(
            mirror_pattern_component_path_at(&mixed, marker)
                .expect("mixed mirror path")
                .len(),
            3
        );
    }
}

#[test]
fn component_vector_cell_count_includes_interleaved_path_slots() {
    let marker = 12;
    let mut payload = vec![0; marker];
    payload[..4].copy_from_slice(&7u32.to_le_bytes());
    payload.extend(COMPACT_EDGE_VECTOR_MARKER);
    payload.extend([0, 0]);
    for index in 0..4u32 {
        payload.extend(0x803e_u16.to_le_bytes());
        payload.extend([0, 0]);
        payload.extend([0x34, 0x80, 0x37, 0]);
        payload.extend((37 + index).to_le_bytes());
        payload.extend(0x4ad9_837au32.wrapping_add(index).to_le_bytes());
        payload.extend((index + 1).to_le_bytes());
        if index != 3 {
            payload.extend((25 + index * 2).to_le_bytes());
        }
    }

    let path = component_vector_path_at(&payload, marker).expect("interleaved path slots");
    assert_eq!(path.len(), 4);
    assert_eq!(path.last().expect("terminal component").local_id, Some(4));
}

#[test]
fn component_vector_preserves_identifierless_lineage_hops() {
    let marker = 12;
    let mut payload = vec![0; marker];
    payload[..4].copy_from_slice(&5u32.to_le_bytes());
    payload[4..8].copy_from_slice(&[6, 2, 0, 0]);
    payload.extend(COMPACT_EDGE_VECTOR_MARKER);
    payload.extend([0, 0]);

    let append_hop = |payload: &mut Vec<u8>, instance: u16, source: u32, timestamp: u32| {
        payload.extend(instance.to_le_bytes());
        payload.extend([0, 0]);
        payload.extend([0xa7, 0x81, 0xa9, 0x01]);
        payload.extend(source.to_le_bytes());
        payload.extend(timestamp.to_le_bytes());
    };
    append_hop(&mut payload, 0x8675, 230, 0x51c6_17de);
    append_hop(&mut payload, 0x8675, 134, 0x51c6_080c);
    append_hop(&mut payload, 0x81a5, 18, 0x51c5_fde3);
    payload.extend(16u32.to_le_bytes());
    payload.extend([0; 24]);

    let path = component_vector_path_at(&payload, marker).expect("lineage path");
    assert_eq!(path.len(), 3);
    assert_eq!(path[0].instance, Some(0x8675));
    assert_eq!(path[0].local_id, None);
    assert_eq!(path[1].instance, Some(0x8675));
    assert_eq!(path[1].local_id, None);
    assert_eq!(path[2].instance, Some(0x81a5));
    assert_eq!(path[2].local_id, Some(16));
}

#[test]
fn planar_surface_candidates_keep_only_defining_type_two_vectors() {
    let mut payload = Vec::new();
    let append_vector = |payload: &mut Vec<u8>, selector: u8, source: u32, terminal: u32| {
        payload.extend(7u32.to_le_bytes());
        payload.extend([selector, 2, 0, 0]);
        payload.extend(0u32.to_le_bytes());
        payload.extend(COMPACT_EDGE_VECTOR_MARKER);
        payload.extend([0, 0]);
        for index in 0..4u32 {
            payload.extend(0x803e_u16.to_le_bytes());
            payload.extend([0, 0]);
            payload.extend([0x34, 0x80, 0x37, 0]);
            payload.extend((source + index).to_le_bytes());
            payload.extend(0x4ad9_837a_u32.wrapping_add(index).to_le_bytes());
            payload.extend(if index == 3 { terminal } else { index + 1 }.to_le_bytes());
            if index != 3 {
                payload.extend((25 + index * 2).to_le_bytes());
            }
        }
    };
    append_vector(&mut payload, 6, 230, 16);
    payload.extend([0; 4]);
    append_vector(&mut payload, 4, 218, 12);
    payload.extend([0; 4]);

    let candidates = planar_surface_selection_candidates(&payload, 0, payload.len());
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].1.len(), 4);
    assert_eq!(
        candidates[0].1[0].type_signature[4..8],
        230u32.to_le_bytes()
    );
    assert_eq!(
        candidates[0].1.last().and_then(|entry| entry.local_id),
        Some(16)
    );
    assert_eq!(
        candidates[1].1[0].type_signature[4..8],
        218u32.to_le_bytes()
    );
    assert_eq!(
        candidates[1].1.last().and_then(|entry| entry.local_id),
        Some(12)
    );

    payload[4] = 3;
    assert_eq!(
        planar_surface_selection_candidates(&payload, 0, payload.len()).len(),
        1
    );
}

#[test]
fn counted_surface_path_preserves_tagged_and_anonymous_nodes() {
    let marker = 12;
    let mut payload = vec![0; marker];
    payload[..4].copy_from_slice(&2u32.to_le_bytes());
    payload[4..8].copy_from_slice(&[0, 2, 0, 0]);
    payload.extend(COMPACT_EDGE_VECTOR_MARKER);
    payload.extend([0, 0]);
    payload.extend(0x803e_u16.to_le_bytes());
    payload.extend([0, 0]);
    payload.extend([0x34, 0x80, 1, 0, 57, 0, 0, 0, 1, 0, 0, 0]);
    payload.extend(9u32.to_le_bytes());
    payload.extend([0; 4]);
    payload.extend([0x34, 0x80, 1, 0, 56, 0, 0, 0, 2, 0, 0, 0]);
    payload.extend(4u32.to_le_bytes());

    let path = counted_surface_component_path_at(&payload, marker).expect("required invariant");
    assert_eq!(path.len(), 2);
    assert_eq!(path[0].instance, Some(0x803e));
    assert_eq!(path[0].local_id, Some(9));
    assert_eq!(path[1].instance, None);
    assert_eq!(path[1].local_id, Some(4));
    assert_eq!(&path[1].type_signature[4..8], &56u32.to_le_bytes());
    assert!(surface_reference_matches_at(&payload, marker, &path));

    payload[..4].copy_from_slice(&3u32.to_le_bytes());
    assert_eq!(
        counted_surface_component_path_at(&payload, marker)
            .expect("one root slot")
            .len(),
        2
    );
    payload[..4].copy_from_slice(&4u32.to_le_bytes());
    assert!(counted_surface_component_path_at(&payload, marker).is_none());
}

#[test]
fn face_reference_plane_owns_its_counted_surface_path() {
    let class_name = "moFaceRefPlnData_c";
    let class_offset = 32;
    let class_body = class_offset + 6 + class_name.len();
    let marker = class_body + 109;
    let mut payload = vec![0; marker + 18];
    payload[class_offset..class_offset + 4].copy_from_slice(CLASS_MARKER);
    payload[class_offset + 4..class_offset + 6]
        .copy_from_slice(&(class_name.len() as u16).to_le_bytes());
    payload[class_offset + 6..class_body].copy_from_slice(class_name.as_bytes());
    payload[marker - 12..marker - 8].copy_from_slice(&3u32.to_le_bytes());
    payload[marker - 8..marker - 4].copy_from_slice(&[0, 2, 0, 0]);
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    for (index, local_id) in [11u32, 7].into_iter().enumerate() {
        payload.extend((0x8038 + index as u16).to_le_bytes());
        payload.extend([0, 0]);
        payload.extend([0x23, 0x80, 1, 0]);
        payload.extend((40 + index as u32).to_le_bytes());
        payload.extend((90 + index as u32).to_le_bytes());
        payload.extend(local_id.to_le_bytes());
    }
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: vec![FeatureInputClass {
            id: "face-plane-data".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: class_offset as u64,
            name: class_name.into(),
            role: FeatureInputClassRole::Reference,
        }],
        names: vec![
            FeatureInputName {
                id: "producer-40-name".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 0,
                object_id: Some(40),
                value: "Producer40".into(),
            },
            FeatureInputName {
                id: "producer-41-name".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: 8,
                object_id: Some(41),
                value: "Producer41".into(),
            },
            FeatureInputName {
                id: "plane-name".into(),
                parent: "lane".into(),
                ordinal: 2,
                offset: 16,
                object_id: Some(37),
                value: "Plane".into(),
            },
        ],
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };

    let candidates = face_reference_plane_selection_candidates(&lane, 0, lane.native_payload.len());
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].0, marker);
    assert_eq!(
        candidates[0]
            .1
            .iter()
            .filter_map(|component| component.local_id)
            .collect::<Vec<_>>(),
        [11, 7]
    );

    let native_feature = |id: &str, source: u32, input_class: &str| Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some(source.to_string()),
        parent_source_id: None,
        ordinal: source,
        name: id.into(),
        kind: "Feature".into(),
        input_class: Some(input_class.into()),
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let histories = [FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![
            native_feature("producer-40", 40, "moExtrusion_c"),
            native_feature("producer-41", 41, "moExtrusion_c"),
            native_feature("plane", 37, "moRefPlane_c"),
        ],
    }];
    let selections = compact_surface_selections(&histories, &lane);
    assert_eq!(selections.len(), 1);
    assert_eq!(selections[0].feature_ref, "plane");
    assert_eq!(
        selections[0].terminal_feature_ref.as_deref(),
        Some("producer-41")
    );
}

#[test]
fn inline_surface_path_distinguishes_branch_and_selection_nodes() {
    let prefix = [0x54, 0x81, 0x56, 0x01];
    let signature = |source: u32, identity: u32| {
        let mut signature = [0; 12];
        signature[..4].copy_from_slice(&prefix);
        signature[4..8].copy_from_slice(&source.to_le_bytes());
        signature[8..].copy_from_slice(&identity.to_le_bytes());
        signature
    };
    let mut payload = 0x8157_u16.to_le_bytes().to_vec();
    payload.extend([0, 0]);
    payload.extend(signature(20, 1));
    payload.extend(0x8200_u16.to_le_bytes());
    payload.extend([0, 0]);
    payload.extend(signature(10, 2));
    payload.extend(7u32.to_le_bytes());

    let path = inline_surface_reference_at(&payload, 4).expect("required invariant");
    assert_eq!(path.len(), 2);
    assert_eq!(path[0].instance, Some(0x8157));
    assert_eq!(path[0].local_id, None);
    assert_eq!(path[1].instance, Some(0x8200));
    assert_eq!(path[1].local_id, Some(7));
}

#[test]
fn projected_split_line_consumes_self_owned_surface_identity_paths() {
    let class_name = "moPLineSurfIdRep_c";
    let prefix = [0xc3, 0x80, 0xc5, 0x00];
    let signature = |source: u32, identity: u32| {
        let mut signature = [0; 12];
        signature[..4].copy_from_slice(&prefix);
        signature[4..8].copy_from_slice(&source.to_le_bytes());
        signature[8..].copy_from_slice(&identity.to_le_bytes());
        signature
    };
    let mut payload = CLASS_MARKER.to_vec();
    payload.extend((class_name.len() as u16).to_le_bytes());
    payload.extend(class_name.as_bytes());
    payload.extend([0, 0]);
    payload.extend(signature(711, 1));
    payload.extend(0x80a7_u16.to_le_bytes());
    payload.extend([0, 0]);
    payload.extend(signature(314, 2));
    payload.extend(3u32.to_le_bytes());
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload.clone(),
        classes: vec![
            FeatureInputClass {
                id: "surface-class".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 0,
                name: class_name.into(),
                role: FeatureInputClassRole::Auxiliary,
            },
            FeatureInputClass {
                id: "projection-class".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: payload.len() as u64,
                name: "moPLineProjIdRep_c".into(),
                role: FeatureInputClassRole::Auxiliary,
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
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };

    let candidates = operation_surface_selection_candidates(
        FeatureClass::SplitFace,
        &lane,
        0,
        payload.len(),
        Some(711),
    );
    assert_eq!(candidates.len(), 1, "{candidates:#?}");
    assert_eq!(candidates[0].1.len(), 2);
    assert_eq!(
        &candidates[0].1[0].type_signature[4..8],
        &711u32.to_le_bytes()
    );
    assert_eq!(
        &candidates[0].1[1].type_signature[4..8],
        &314u32.to_le_bytes()
    );
    assert_eq!(candidates[0].1[1].local_id, Some(3));
    assert!(operation_surface_selection_candidates(
        FeatureClass::SplitFace,
        &lane,
        0,
        payload.len(),
        Some(712),
    )
    .is_empty());
}

#[test]
fn generated_surface_identities_are_producer_outputs() {
    let class_name = "moWzdHoleSurfIdRep_c";
    let prefix = [0xc3, 0x80, 0xc5, 0x00];
    let mut payload = CLASS_MARKER.to_vec();
    payload.extend((class_name.len() as u16).to_le_bytes());
    payload.extend(class_name.as_bytes());
    payload.extend([0, 0]);
    payload.extend(prefix);
    payload.extend(89u32.to_le_bytes());
    payload.extend(0x52e4_6185u32.to_le_bytes());
    payload.extend(2u32.to_le_bytes());
    payload.extend(0x85b5u16.to_le_bytes());
    payload.extend([0, 0]);
    payload.extend(prefix);
    payload.extend(89u32.to_le_bytes());
    payload.extend(0x52e4_6185u32.to_le_bytes());
    payload.extend(2u32.to_le_bytes());
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: vec![FeatureInputClass {
            id: "class".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            name: class_name.into(),
            role: FeatureInputClassRole::Auxiliary,
        }],
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };

    let identities = generated_surface_identities(&lane);

    assert_eq!(identities.len(), 2, "{identities:#?}");
    assert!(identities.iter().all(|identity| {
        identity.type_prefix == prefix
            && identity.feature_source_id == 89
            && identity.local_identity == 2
    }));
    assert_eq!(identities[0].components[0].instance, None);
    assert_eq!(identities[1].components[0].instance, Some(0x85b5));
}

#[test]
fn idless_history_features_use_unique_feature_input_object_sources() {
    let feature = Feature {
        id: "producer".into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: None,
        parent_source_id: None,
        ordinal: 0,
        name: "Producer".into(),
        kind: "Feature".into(),
        input_class: Some("ProducerClass".into()),
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![feature],
    };
    let mut lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: vec![FeatureInputName {
            id: "name".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            object_id: Some(233),
            value: "Producer".into(),
        }],
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };

    let ambiguous_history = history.clone();
    let resolved = history_features_with_object_sources(&[history], &lane);

    assert_eq!(resolved[0].source_id.as_deref(), Some("233"));

    lane.names.push(FeatureInputName {
        id: "ambiguous-name".into(),
        parent: "lane".into(),
        ordinal: 1,
        offset: 1,
        object_id: Some(234),
        value: "Producer".into(),
    });
    let ambiguous = history_features_with_object_sources(&[ambiguous_history], &lane);
    assert_eq!(ambiguous[0].source_id, None);
}
