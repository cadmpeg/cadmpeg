//! Compact edge-selection and reference-list tests.

use super::super::super::component_paths::{
    compact_edge_path_value, compact_edge_selection_set_value,
};
use super::super::super::{CLASS_MARKER, LEGACY_SKETCH_MARKER};
use super::super::*;
use crate::classification::FeatureClass;
use crate::records::{
    Feature, FeatureHistory, FeatureInputClass, FeatureInputClassRole,
    FeatureInputComponentPathEntry, FeatureInputEdgeSelection, FeatureInputLane, FeatureInputName,
};
use std::collections::BTreeMap;

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

    // SurfaceCut target vectors retain the same role byte with any lane-local
    // low subtype.  The operation scanner must not require subtype zero.
    payload[4] = 0x7f;
    let lane = FeatureInputLane {
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
        sketch_entities: Vec::new(),
    };
    let selections = operation_surface_selection_candidates(
        FeatureClass::CutWithSurface,
        &lane,
        0,
        payload.len(),
        None,
    );
    assert_eq!(selections.len(), 1);
    assert_eq!(selections[0].0, marker);
}

#[test]
fn varfillet_roster_accepts_unframed_reference_lists() {
    let marker = 12;
    let class_offset = 146;
    let class_name = "moVertDim_c";
    let mut payload = vec![0; 220];
    payload[marker - 12..marker - 8].copy_from_slice(&4u32.to_le_bytes());
    payload[marker - 8..marker - 4].copy_from_slice(&[0, 2, 0, 0]);
    payload[marker - 4..marker].copy_from_slice(&37u32.to_le_bytes());
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    payload[marker + 16..marker + 18].copy_from_slice(&[0, 0]);
    let signature = |serial: u32| {
        let mut value = [0; 12];
        value[..4].copy_from_slice(&[0xa7, 0x81, 0xa9, 0x01]);
        value[4..8].copy_from_slice(&serial.to_le_bytes());
        value[8..].copy_from_slice(&0x51c5_fde3u32.to_le_bytes());
        value
    };
    let mut cursor = marker + 18;
    for (instance, serial, local_id) in [
        (0x81a5u16, 18u32, 28u32),
        (0x81a5, 18, 29),
        (0x81ac, 170, 33),
        (0x8083, 252, 1),
    ] {
        payload[cursor..cursor + 2].copy_from_slice(&instance.to_le_bytes());
        payload[cursor + 4..cursor + 16].copy_from_slice(&signature(serial));
        payload[cursor + 16..cursor + 20].copy_from_slice(&local_id.to_le_bytes());
        cursor += 20;
    }
    payload[class_offset - 4..class_offset].copy_from_slice(&[0x20, 0x81, 0x08, 0]);
    payload[class_offset..class_offset + 4].copy_from_slice(CLASS_MARKER);
    payload[class_offset + 4..class_offset + 6]
        .copy_from_slice(&(class_name.len() as u16).to_le_bytes());
    payload[class_offset + 6..class_offset + 6 + class_name.len()]
        .copy_from_slice(class_name.as_bytes());
    payload[class_offset + 6 + class_name.len()..class_offset + 8 + class_name.len()]
        .copy_from_slice(&0x87d3u16.to_le_bytes());

    let feature = Feature {
        id: "varfillet".into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some("37".into()),
        parent_source_id: None,
        ordinal: 0,
        name: "VarFillet1".into(),
        kind: "VarFillet".into(),
        input_class: Some("VarFillet_c".into()),
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
        names: vec![FeatureInputName {
            id: "feature-name".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            object_id: Some(37),
            value: "VarFillet1".into(),
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

    let selections = compact_edge_selections(&[history], &lane);
    assert_eq!(selections.len(), 1);
    assert_eq!(selections[0].references.len(), 4);
    assert_eq!(selections[0].references[3][0].instance, Some(0x8083));
}
