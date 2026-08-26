//! Construction-line endpoint resolution tests.

use super::super::super::LEGACY_SKETCH_MARKER;
use super::super::*;
use crate::records::{SketchInputEntity, SketchInputKind};

#[test]
fn compact_84_construction_line_prefers_points_and_accepts_one_curve_marker() {
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
    payload[58..60].copy_from_slice(&10u16.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&[0x00, 0x00, 0x01, 0x00]);
    payload[80..84].copy_from_slice(&4u32.to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_SKETCH_MARKER);

    let entity = |id: &str, object_index, coordinates_m, kind| SketchInputEntity {
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
    let curve = entity("curve", Some(1), None, SketchInputKind::LineOrCircle);
    let point_impostor = entity(
        "point-impostor",
        Some(7),
        Some([2.0, 0.0]),
        SketchInputKind::LineOrCircle,
    );
    let first = entity("first", Some(7), Some([0.0, 0.0]), SketchInputKind::Point);
    let second = entity(
        "second-curve",
        Some(10),
        Some([1.0, 0.0]),
        SketchInputKind::LineOrCircle,
    );
    let markers = [&curve, &point_impostor, &first, &second];

    assert_eq!(
        roster_curve_endpoint_markers(&payload, &curve, &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second-curve"]
    );

    let second_collision = entity(
        "second-curve-collision",
        Some(10),
        Some([1.0, 1.0]),
        SketchInputKind::LineOrCircle,
    );
    let ambiguous = [&curve, &point_impostor, &first, &second, &second_collision];
    assert!(roster_curve_endpoint_markers(&payload, &curve, &ambiguous).is_empty());
}
