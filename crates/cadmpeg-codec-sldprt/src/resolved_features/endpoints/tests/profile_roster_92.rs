//! Compact-legacy 92-byte profile-roster tests.

use super::super::super::LEGACY_SKETCH_MARKER;
use super::super::*;
use crate::records::{SketchInputEntity, SketchInputKind};

fn profile_payload(native_code: u32, selector: u8, endpoints: [u16; 2], terminal: bool) -> Vec<u8> {
    let length = if terminal {
        128
    } else {
        92 + LEGACY_SKETCH_MARKER.len()
    };
    let mut payload = vec![0; length];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&native_code.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, selector, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&endpoints[0].to_le_bytes());
    payload[66..68].copy_from_slice(&endpoints[1].to_le_bytes());
    payload[68..72].copy_from_slice(&1u32.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    if selector != 0x04 {
        payload[84..88].copy_from_slice(&9u32.to_le_bytes());
        payload[88..92].copy_from_slice(&10u32.to_le_bytes());
    }
    if !terminal {
        payload[92..].copy_from_slice(LEGACY_SKETCH_MARKER);
    }
    payload
}

#[test]
fn compact_legacy_92_profile_prefers_roster_and_recovers_direct_object_ids() {
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
    let curve = entity("curve", 0, Some(9), SketchInputKind::LineOrCircle, None);
    let first = entity(
        "first",
        10,
        Some(1),
        SketchInputKind::Point,
        Some([0.0, 0.0]),
    );
    let second = entity(
        "second",
        20,
        Some(2),
        SketchInputKind::Point,
        Some([1.0, 0.0]),
    );
    let third = entity(
        "third",
        30,
        Some(4),
        SketchInputKind::Point,
        Some([2.0, 0.0]),
    );
    let fourth = entity(
        "fourth",
        40,
        Some(5),
        SketchInputKind::Point,
        Some([3.0, 0.0]),
    );
    let coordinate_line = entity(
        "coordinate-line",
        50,
        Some(13),
        SketchInputKind::LineOrCircle,
        Some([4.0, 0.0]),
    );
    let zero_object = entity(
        "zero-object",
        60,
        Some(0),
        SketchInputKind::Point,
        Some([5.0, 0.0]),
    );
    let markers = [
        &curve,
        &first,
        &second,
        &third,
        &fourth,
        &coordinate_line,
        &zero_object,
    ];
    let endpoint_ids = |payload: &[u8]| {
        roster_curve_endpoint_markers(payload, &curve, &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>()
    };

    let roster = profile_payload(1, 0x44, [2, 4], false);
    assert_eq!(
        wide_indexed_curve_endpoint_indices(&roster, 0),
        Some([3, 5])
    );
    assert_eq!(coordinate_roster_endpoint_offset(&roster, 0), Some(64));
    assert_eq!(endpoint_ids(&roster), ["third", "coordinate-line"]);

    let direct = profile_payload(1, 0x44, [4, 13], false);
    assert!(coordinate_roster_curve_endpoint_markers(&direct, &curve, &markers).is_empty());
    assert_eq!(endpoint_ids(&direct), ["third", "coordinate-line"]);

    let terminal = profile_payload(0, 0x04, [4, 13], true);
    assert!(coordinate_roster_curve_endpoint_markers(&terminal, &curve, &markers).is_empty());
    assert_eq!(endpoint_ids(&terminal), ["third", "coordinate-line"]);

    let zero_direct = profile_payload(1, 0x44, [0, 13], false);
    assert!(coordinate_roster_curve_endpoint_markers(&zero_direct, &curve, &markers).is_empty());
    assert!(endpoint_ids(&zero_direct).is_empty());
}
