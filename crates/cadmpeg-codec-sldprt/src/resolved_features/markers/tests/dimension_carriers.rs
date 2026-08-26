//! Dimension-carrier marker tests.

use super::super::super::SKETCH_MARKER;
use super::super::current_geometry_locus_arc_handle_point;

#[test]
fn current_geometry_locus_arc_handle_point_requires_bounded_child_and_next_marker() {
    use crate::layout::current_geometry_locus_arc_handle_point as arc_handle;
    use crate::layout::current_geometry_locus_arc_handle_point_terminal as terminal;

    for (record_len, next_index) in [
        (arc_handle::LEN, arc_handle::FOLLOWING_OBJECT_INDEX),
        (terminal::LEN, terminal::FOLLOWING_OBJECT_INDEX),
    ] {
        let offset = 4;
        let mut payload = vec![0; offset + record_len + SKETCH_MARKER.len()];
        payload[..offset].copy_from_slice(&11u32.to_le_bytes());
        payload[offset..offset + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
        payload[offset + arc_handle::HEADER..offset + arc_handle::SHARED_SELECTOR].fill(0xff);
        payload[offset + arc_handle::SHARED_SELECTOR..offset + arc_handle::NATIVE_KIND]
            .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + arc_handle::NATIVE_KIND..offset + arc_handle::ZERO_LOCUS_PREFIX]
            .copy_from_slice(&0u32.to_le_bytes());
        payload[offset + arc_handle::GEOMETRY_LOCUS..offset + arc_handle::ROLE]
            .copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
        payload[offset + arc_handle::ROLE..offset + arc_handle::ZERO_STATE]
            .copy_from_slice(&1u16.to_le_bytes());
        payload[offset + arc_handle::SELECTOR..offset + arc_handle::ZERO_BEFORE_STATE_VALUE]
            .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
        payload[offset + arc_handle::STATE_VALUE..offset + arc_handle::ZERO_BEFORE_COORDINATE]
            .copy_from_slice(&1.0f64.to_le_bytes());
        payload[offset + arc_handle::COORDINATE_TAG..offset + arc_handle::COORDINATE_FIRST]
            .copy_from_slice(&[0x1e, 0x00]);
        payload[offset + arc_handle::COORDINATE_FIRST..offset + arc_handle::COORDINATE_SECOND]
            .copy_from_slice(&1.25f64.to_le_bytes());
        payload[offset + arc_handle::COORDINATE_SECOND..offset + arc_handle::HANDLE_PREFIX]
            .copy_from_slice(&(-2.5f64).to_le_bytes());
        payload[offset + arc_handle::HANDLE_PREFIX..offset + arc_handle::CLASS_MARKER]
            .copy_from_slice(&[0x02, 0x00, 0x02, 0x00]);
        payload[offset + arc_handle::CLASS_MARKER..offset + arc_handle::CLASS_LENGTH]
            .copy_from_slice(&[0xff, 0xff, 0x01, 0x00]);
        payload[offset + arc_handle::CLASS_LENGTH..offset + arc_handle::CLASS_NAME]
            .copy_from_slice(&11u16.to_le_bytes());
        payload[offset + arc_handle::CLASS_NAME..offset + arc_handle::HANDLE_ID]
            .copy_from_slice(b"sgArcHandle");
        payload[offset + arc_handle::REFERENCE_SENTINEL..offset + arc_handle::ZERO_REFERENCE_TAIL]
            .fill(0xff);
        payload[offset + arc_handle::TERMINATOR..offset + arc_handle::ZERO_TRAILER]
            .copy_from_slice(&[0xfe, 0xff, 0xff, 0xff]);
        payload[offset + arc_handle::ZERO_TRAILER..offset + next_index].fill(0);
        payload[offset + next_index..offset + next_index + 4].copy_from_slice(&12u32.to_le_bytes());
        payload[offset + record_len..].copy_from_slice(SKETCH_MARKER);

        assert!(current_geometry_locus_arc_handle_point(&payload, offset));

        payload[offset + arc_handle::CLASS_NAME] = b'x';
        assert!(!current_geometry_locus_arc_handle_point(&payload, offset));

        payload[offset + arc_handle::CLASS_NAME] = b's';
        payload[offset + record_len] = 0;
        assert!(!current_geometry_locus_arc_handle_point(&payload, offset));
    }
}
