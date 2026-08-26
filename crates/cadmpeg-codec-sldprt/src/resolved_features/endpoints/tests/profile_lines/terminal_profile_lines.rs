use super::super::*;
use super::*;

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
