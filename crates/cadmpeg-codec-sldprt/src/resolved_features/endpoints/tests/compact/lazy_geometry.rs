use super::{compact_complete_marker_roster_pair, SketchInputKind, SKETCH_MARKER};
use crate::resolved_features::endpoints::{compact_indexed_curve_record_end, CompactIndexedCurveRecordEnd};

#[test]
fn one_based_roster_underflow_leaves_geometry_unbuilt() {
    let mut payload = vec![0; 84 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[29..31].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..60].copy_from_slice(&[0, 0, 3, 0]);
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[84..].copy_from_slice(SKETCH_MARKER);
    assert_eq!(compact_indexed_curve_record_end(&payload, 0), Some(CompactIndexedCurveRecordEnd::Marker84));
    let curve = crate::records::SketchInputEntity::new("curve", "lane", 0, 0, SketchInputKind::LineOrCircle);
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut cache = None;
    let mut provider = crate::resolved_features::typed_relations::CurveEndpointGeometry::Cached {
        ctx: &ctx, payload: &payload, cache: &mut cache,
    };
    assert!(compact_complete_marker_roster_pair(&payload, &curve, true, &mut provider, &[]).unwrap().is_none());
    assert!(cache.is_none());
}
