//! Circle, radial, and ellipse endpoint tests.

use super::super::super::relation_loci::same_dimension_length;
use super::super::super::{
    CLASS_MARKER, LEGACY_EXTENDED_SKETCH_MARKER, LEGACY_SKETCH_MARKER, SKETCH_MARKER,
};
use super::super::*;
use crate::records::{SketchInputEntity, SketchInputKind, SketchRelationKind};

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
