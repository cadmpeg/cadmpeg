use super::super::*;
use super::*;
use cadmpeg_ir::math::Point2;

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
