//! Tests for the current four-link profile-point carrier.

use super::super::super::SKETCH_MARKER;
use super::super::*;
use crate::records::{SketchInputEntity, SketchInputKind};

#[test]
fn current_four_link_profile_point_decodes_and_drives_reverse_incidence() {
    let line = 4;
    let first = line + 84;
    let second = first + 154;
    let end = second + 154;
    let long = end;
    let long_end = long + 158;
    let mut payload = vec![0; long_end + SKETCH_MARKER.len()];
    payload[..line].copy_from_slice(&20u32.to_le_bytes());

    payload[line..line + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[line + 5..line + 13].fill(0xff);
    payload[line + 13..line + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[line + 17..line + 21].copy_from_slice(&1u32.to_le_bytes());
    payload[line + 23..line + 29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[line + 31..line + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[line + 48..line + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[line + 56..line + 60].copy_from_slice(&[2, 0, 5, 0]);
    payload[line + 60..line + 64].copy_from_slice(&1u32.to_le_bytes());
    payload[line + 64..line + 72].copy_from_slice(&(-1.0f64).to_le_bytes());

    for (offset, object_index, local_id, coordinates, linked_curves) in [
        (first, 31u32, 32u32, [1.0f64, 2.0], [20u16, 20u16]),
        (second, 32u32, 41u32, [3.0f64, 4.0], [20u16, 20u16]),
    ] {
        payload[offset - 4..offset].copy_from_slice(&object_index.to_le_bytes());
        payload[offset..offset + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 17..offset + 21].copy_from_slice(&0u32.to_le_bytes());
        payload[offset + 23..offset + 29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
        payload[offset + 31..offset + 39]
            .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
        payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[offset + 56..offset + 58].copy_from_slice(&[0x1e, 0x00]);
        payload[offset + 58..offset + 66].copy_from_slice(&coordinates[0].to_le_bytes());
        payload[offset + 66..offset + 74].copy_from_slice(&coordinates[1].to_le_bytes());
        payload[offset + 76..offset + 78].copy_from_slice(&4u16.to_le_bytes());
        for (cell, linked_curve) in [(78, linked_curves[0]), (90, linked_curves[1])] {
            payload[offset + cell..offset + cell + 2].copy_from_slice(&0x815au16.to_le_bytes());
            payload[offset + cell + 2..offset + cell + 4]
                .copy_from_slice(&linked_curve.to_le_bytes());
            payload[offset + cell + 4..offset + cell + 8].fill(0xff);
        }
        payload[offset + 102..offset + 108].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
        payload[offset + 142..offset + 144].copy_from_slice(&[0x02, 0x00]);
        payload[offset + 150..offset + 154].copy_from_slice(&local_id.to_le_bytes());
    }
    payload[long - 4..long].copy_from_slice(&42u32.to_le_bytes());
    payload[long..long + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[long + 5..long + 13].fill(0xff);
    payload[long + 13..long + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[long + 23..long + 29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[long + 31..long + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[long + 48..long + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[long + 56..long + 58].copy_from_slice(&[0x1e, 0x00]);
    payload[long + 58..long + 66].copy_from_slice(&5.0f64.to_le_bytes());
    payload[long + 66..long + 74].copy_from_slice(&6.0f64.to_le_bytes());
    payload[long + 76..long + 78].copy_from_slice(&4u16.to_le_bytes());
    for (cell, linked_curve) in [(78, 20u16), (90, 20u16)] {
        payload[long + cell..long + cell + 2].copy_from_slice(&0x815au16.to_le_bytes());
        payload[long + cell + 2..long + cell + 4].copy_from_slice(&linked_curve.to_le_bytes());
        payload[long + cell + 4..long + cell + 8].fill(0xff);
    }
    payload[long + 102..long + 108].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[long + 142..long + 144].copy_from_slice(&[0x02, 0x00]);
    payload[long + 144..long + 148].copy_from_slice(&42u32.to_le_bytes());
    payload[long + 154..long + 158].copy_from_slice(&1u32.to_le_bytes());
    payload[long_end..].copy_from_slice(SKETCH_MARKER);

    assert_eq!(
        linked_profile_point(&payload, first),
        Some(([1.0, 2.0], [(0x815a, 20), (0x815a, 20)]))
    );
    assert_eq!(marker_coordinates(&payload, first), Some([1.0, 2.0]));
    assert_eq!(
        linked_profile_point(&payload, long),
        Some(([5.0, 6.0], [(0x815a, 20), (0x815a, 20)]))
    );
    let entities = sketch_input_entities(&payload, "lane");
    let first_entity = entities
        .iter()
        .find(|entity| entity.offset == first as u64)
        .expect("first linked profile point");
    assert_eq!(first_entity.kind, SketchInputKind::Point);
    assert_eq!(first_entity.coordinates_m, Some([1.0, 2.0]));
    assert_eq!(first_entity.local_id, Some(32));

    let curve = SketchInputEntity {
        id: "curve".into(),
        parent: "lane".into(),
        feature_ref: Some("profile".into()),
        ordinal: 0,
        offset: line as u64,
        object_index: Some(20),
        local_id: None,
        kind: SketchInputKind::LineOrCircle,
        state_value: Some(1.0),
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };
    let point = |id: &str, offset| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("profile".into()),
        ordinal: 0,
        offset,
        object_index: None,
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };
    let markers = [
        curve.clone(),
        point("first", first as u64),
        point("second", second as u64),
    ];
    let marker_refs = markers.iter().collect::<Vec<_>>();
    assert_eq!(
        current_reverse_incidence_endpoint_offsets(&payload, &curve, &marker_refs),
        Some([first as u64, second as u64])
    );

    payload[first + 76..first + 78].copy_from_slice(&5u16.to_le_bytes());
    assert!(linked_profile_point(&payload, first).is_none());
}
