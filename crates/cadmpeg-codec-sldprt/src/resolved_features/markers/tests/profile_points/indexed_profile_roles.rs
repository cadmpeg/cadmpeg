use super::super::*;
use super::*;

#[test]
fn indexed_profile_framing_distinguishes_vertices_lines_and_arcs() {
    let mut vertex = vec![0; 74];
    vertex[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    vertex[5..13].fill(0xff);
    vertex[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    vertex[17..21].copy_from_slice(&1u32.to_le_bytes());
    vertex[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    vertex[27..29].copy_from_slice(&1u16.to_le_bytes());
    vertex[56..58].copy_from_slice(&[0x1e, 0x00]);
    vertex[58..66].copy_from_slice(&0.025f64.to_le_bytes());
    vertex[66..74].copy_from_slice(&0.01f64.to_le_bytes());
    assert!(indexed_profile_vertex(&vertex, 0));
    assert_eq!(marker_coordinates(&vertex, 0), Some([0.025, 0.01]));
    vertex[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    assert!(indexed_profile_vertex(&vertex, 0));
    assert_eq!(marker_coordinates(&vertex, 0), Some([0.025, 0.01]));
    vertex.resize(112 + LEGACY_EXTENDED_SKETCH_MARKER.len(), 0);
    vertex[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    vertex[17..21].copy_from_slice(&4u32.to_le_bytes());
    vertex[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    vertex[27..31].copy_from_slice(&[1, 0, 1, 0]);
    vertex[64..66].copy_from_slice(&[0x1e, 0x00]);
    vertex[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    vertex[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    vertex[68..72].copy_from_slice(&1u32.to_le_bytes());
    vertex[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    vertex[80..84].copy_from_slice(&u32::MAX.to_le_bytes());
    vertex[84..86].copy_from_slice(&1u16.to_le_bytes());
    for offset in (86..102).step_by(4) {
        vertex[offset..offset + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    vertex[112..112 + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    assert_eq!(marker_coordinates(&vertex, 0), None);

    let mut curve = vec![0; 84 + 39];
    curve[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    curve[5..13].fill(0xff);
    curve[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    curve[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    curve[27..29].copy_from_slice(&1u16.to_le_bytes());
    curve[60..64].copy_from_slice(&1u32.to_le_bytes());
    curve[84..84 + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    curve[89..97].fill(0xff);
    curve[97..101].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    assert_eq!(
        legacy_extended_profile_curve_kind(&curve, 0),
        Some(SketchInputKind::LineOrCircle)
    );
    curve[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    assert_eq!(
        legacy_extended_profile_curve_kind(&curve, 0),
        Some(SketchInputKind::LineOrCircle)
    );
    curve[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    curve[89..93].copy_from_slice(&[0xff, 0xff, 0x04, 0x00]);
    assert_eq!(
        legacy_extended_profile_curve_kind(&curve, 0),
        Some(SketchInputKind::Arc)
    );
}

#[test]
fn selector44_terminal_indexed_curve_is_a_line() {
    let mut curve = vec![0; 170];
    curve[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    curve[5..13].fill(0xff);
    curve[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    curve[23..31].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00]);
    curve[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x44, 0x00]);
    curve[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    curve[56..58].copy_from_slice(&0u16.to_le_bytes());
    curve[58..60].copy_from_slice(&1u16.to_le_bytes());
    curve[60..64].copy_from_slice(&1u32.to_le_bytes());
    curve[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    curve[142..144].copy_from_slice(&[0x08, 0x80]);
    curve[154..170].copy_from_slice(&[
        0x01, 0x00, 0x01, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00,
        0x00,
    ]);

    assert_eq!(
        legacy_extended_profile_curve_kind(&curve, 0),
        Some(SketchInputKind::LineOrCircle)
    );
    curve[37] = 0x04;
    assert_eq!(legacy_extended_profile_curve_kind(&curve, 0), None);
}

#[test]
fn geometry_locus_role_excludes_display_handles() {
    let mut payload = vec![0; 27];
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    assert!(marker_is_geometry_locus(&payload, 0));
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    assert!(!marker_is_geometry_locus(&payload, 0));
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x02, 0x00]);
    assert!(!marker_is_geometry_locus(&payload, 0));
}

#[test]
fn coordinate_marker_links_are_sentinel_terminated_reference_cells() {
    let mut payload = vec![0; 118];
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[64..66].copy_from_slice(&[0x1e, 0x00]);
    payload[66..74].copy_from_slice(&1.25f64.to_le_bytes());
    payload[74..82].copy_from_slice(&(-2.5f64).to_le_bytes());
    payload[84..86].copy_from_slice(&3u16.to_le_bytes());
    for (index, local_id) in [7u16, 11].into_iter().enumerate() {
        let start = 86 + index * 12;
        payload[start..start + 2].copy_from_slice(&0x8386u16.to_le_bytes());
        payload[start + 2..start + 4].copy_from_slice(&local_id.to_le_bytes());
        payload[start + 4..start + 8].fill(0xff);
    }
    payload[112..116].copy_from_slice(&[0xfe, 0xff, 0xff, 0xff]);
    assert_eq!(
        coordinate_marker_local_links(&payload, 0),
        Some((vec![7, 11], 0x8386))
    );
    for start in [86, 98] {
        payload[start..start + 2].copy_from_slice(&0xbc87u16.to_le_bytes());
    }
    assert_eq!(
        coordinate_marker_local_links(&payload, 0),
        Some((vec![7, 11], 0xbc87))
    );
    payload[98] ^= 1;
    assert_eq!(coordinate_marker_local_links(&payload, 0), None);
}
