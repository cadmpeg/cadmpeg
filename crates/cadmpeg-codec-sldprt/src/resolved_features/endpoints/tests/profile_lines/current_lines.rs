//! Current compact profile-line records.

use crate::records::{SketchInputEntity, SketchInputKind};
use crate::resolved_features::endpoints::{
    coordinate_roster_endpoint_offset, current_compact_104_indexed_line_endpoint_indices,
    current_compact_104_profile_line, current_direct_92_profile_line_endpoint_indices,
    roster_curve_endpoint_markers,
};
use crate::resolved_features::SKETCH_MARKER;

#[test]
fn current_compact_104_line_indexes_coordinate_markers() {
    let mut payload = vec![0; 104 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&7u16.to_le_bytes());
    payload[66..68].copy_from_slice(&2u16.to_le_bytes());
    payload[68..72].copy_from_slice(&1u32.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[88..92].copy_from_slice(&[0x00, 0x00, 0x01, 0x00]);
    payload[100..104].copy_from_slice(&1u32.to_le_bytes());
    payload[104..].copy_from_slice(SKETCH_MARKER);

    assert_eq!(
        current_compact_104_indexed_line_endpoint_indices(&payload, 0),
        Some([8, 3])
    );
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(64));

    payload[100..104].fill(0);
    assert_eq!(
        current_compact_104_indexed_line_endpoint_indices(&payload, 0),
        None
    );
}

#[test]
fn current_compact_84_line_falls_back_to_zero_based_point_roster() {
    let mut payload = vec![0; 84 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..31].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[58..60].copy_from_slice(&1u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[80..84].copy_from_slice(&2u32.to_le_bytes());
    payload[84..].copy_from_slice(SKETCH_MARKER);

    let entity = |id: &str, offset, object_index, coordinates_m: Option<[f64; 2]>| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker = SketchInputEntity::new(
            marker_id,
            marker_parent,
            0,
            offset,
            if coordinates_m.is_some() {
                SketchInputKind::Point
            } else {
                SketchInputKind::LineOrCircle
            },
        );
        constructed_marker.feature_ref = Some("sketch".into());
        constructed_marker = constructed_marker.with_test_identity(object_index, None);
        constructed_marker.state_value = cadmpeg_ir::scalar::FiniteReal::new(1.0);
        constructed_marker.coordinates_m =
            coordinates_m.and_then(cadmpeg_ir::units::FiniteVector::new);
        constructed_marker.links = None;
        constructed_marker
    };
    let curve = entity("curve", 0, Some(1), None);
    let first = entity("first", 10, Some(10), Some([0.0, 0.0]));
    let second = entity("second", 20, Some(11), Some([1.0, 0.0]));
    let markers = [&curve, &first, &second];

    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(56));
    assert_eq!(
        roster_curve_endpoint_markers(
            &cadmpeg_test_support::service_decode_context(),
            &payload,
            &curve,
            &markers
        )
        .unwrap()
        .iter()
        .map(|endpoint| endpoint.id())
        .collect::<Vec<_>>(),
        ["first", "second"]
    );

    payload.resize(96 + SKETCH_MARKER.len(), 0);
    payload[72..82].fill(0);
    payload[82..84].copy_from_slice(&2u16.to_le_bytes());
    payload[84..88].fill(0);
    payload[88..92].copy_from_slice(&2u32.to_le_bytes());
    payload[92..96].copy_from_slice(&1u32.to_le_bytes());
    payload[96..].copy_from_slice(SKETCH_MARKER);
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(56));
    assert_eq!(
        roster_curve_endpoint_markers(
            &cadmpeg_test_support::service_decode_context(),
            &payload,
            &curve,
            &markers
        )
        .unwrap()
        .iter()
        .map(|endpoint| endpoint.id())
        .collect::<Vec<_>>(),
        ["first", "second"]
    );

    payload.resize(104 + SKETCH_MARKER.len(), 0);
    payload[72..76].copy_from_slice(&1i32.to_le_bytes());
    payload[76..78].copy_from_slice(&2u16.to_le_bytes());
    for relative in [78, 82, 86, 90] {
        payload[relative..relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[94..96].fill(0);
    payload[96..104].copy_from_slice(&[2, 0, 0, 0, 3, 0, 0, 0]);
    payload[104..].copy_from_slice(SKETCH_MARKER);
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(56));
    assert_eq!(
        roster_curve_endpoint_markers(
            &cadmpeg_test_support::service_decode_context(),
            &payload,
            &curve,
            &markers
        )
        .unwrap()
        .iter()
        .map(|endpoint| endpoint.id())
        .collect::<Vec<_>>(),
        ["first", "second"]
    );
}

#[test]
fn current_compact_104_profile_record_is_a_line() {
    let mut payload = vec![0; 104 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&4u16.to_le_bytes());
    payload[58..60].copy_from_slice(&6u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&1u32.to_le_bytes());
    for relative in [78, 82, 86, 90] {
        payload[relative..relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[96..100].copy_from_slice(&2u32.to_le_bytes());
    payload[100..104].copy_from_slice(&2u32.to_le_bytes());
    payload[104..].copy_from_slice(SKETCH_MARKER);

    assert!(current_compact_104_profile_line(&payload, 0));

    payload[100..104].copy_from_slice(&3u32.to_le_bytes());
    assert!(!current_compact_104_profile_line(&payload, 0));
}

#[test]
fn current_direct_92_profile_line_uses_point_object_ids() {
    let mut payload = vec![0; 92 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&6u16.to_le_bytes());
    payload[66..68].copy_from_slice(&9u16.to_le_bytes());
    payload[68..72].copy_from_slice(&1u32.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[84..88].copy_from_slice(&1u32.to_le_bytes());
    payload[88..92].copy_from_slice(&6u32.to_le_bytes());
    payload[92..].copy_from_slice(SKETCH_MARKER);

    assert_eq!(
        current_direct_92_profile_line_endpoint_indices(&payload, 0),
        Some([6, 9])
    );

    payload[88..92].fill(0);
    assert_eq!(
        current_direct_92_profile_line_endpoint_indices(&payload, 0),
        None
    );
}
