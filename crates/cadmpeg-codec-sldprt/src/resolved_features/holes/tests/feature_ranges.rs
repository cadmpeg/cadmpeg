//! Native feature-object range tests.

use super::{lane_with_position_reference, native_history};
use crate::resolved_features::holes::feature_object_byte_ranges;

#[test]
fn feature_object_ranges_preserve_named_object_bounds() {
    let histories = [native_history()];
    let lane = lane_with_position_reference(7);
    let ranges = feature_object_byte_ranges(&histories, &lane);
    assert_eq!(
        ranges.get("native-hole"),
        Some(&(0, 0, lane.native_payload.len()))
    );
}

#[test]
fn feature_object_ranges_preserve_equal_offset_history_order() {
    let mut history = native_history();
    let mut second = history.features[0].clone();
    second.id = "second-hole".into();
    history.features.push(second);
    let lane = lane_with_position_reference(7);
    let histories = [history];
    let ranges = feature_object_byte_ranges(&histories, &lane);
    assert_eq!(ranges.get("native-hole"), Some(&(0, 0, 0)));
    assert_eq!(
        ranges.get("second-hole"),
        Some(&(0, 0, lane.native_payload.len()))
    );
}

#[test]
fn deferred_principal_component_preserves_scoped_and_lane_front_fallbacks() {
    use crate::resolved_features::compact_reference_planes::{
        principal_sketch_frame, CompactReferencePlaneIndex,
    };
    use crate::resolved_features::curves::{SketchPlaneFrame, SketchPlaneUAxisSource};
    use crate::resolved_features::holes::feature_input_sketch_frame;
    use cadmpeg_ir::features::PrincipalPlane;
    use cadmpeg_ir::math::{Point3, Vector3};
    use std::collections::HashMap;

    for (source, selector, matrix) in [
        (
            3u32,
            2u32,
            [1.0f64, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -1.0, 0.0],
        ),
        (
            4u32,
            1u32,
            [0.0f64, 0.0, 1.0, 0.0, 1.0, 0.0, -1.0, 0.0, 0.0],
        ),
    ] {
        let mut payload = super::profile_reference_plane_payload(false);
        let start = payload.len();
        let mut component = [0u8; 138];
        component[..4].copy_from_slice(&source.to_le_bytes());
        component[10..14].copy_from_slice(&selector.to_le_bytes());
        component[14] = 1;
        for (index, value) in matrix.into_iter().chain([0.0, 0.0, 0.0, 1.0]).enumerate() {
            let offset = 15 + index * 8;
            component[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        component[122..126].copy_from_slice(&4u32.to_le_bytes());
        component[126..130].fill(0xff);
        payload.extend(component);
        let index = CompactReferencePlaneIndex::new(&payload);
        let front = principal_sketch_frame(PrincipalPlane::Front);
        let frames = HashMap::from([(
            2,
            SketchPlaneFrame::from_frame(front, SketchPlaneUAxisSource::Native),
        )]);
        let expected = (
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
            Vector3::new(0.0, 0.0, -1.0),
        );
        for context in [0, start] {
            assert_eq!(
                feature_input_sketch_frame(
                    &payload,
                    &frames,
                    &index,
                    context,
                    start,
                    payload.len()
                ),
                Some(expected)
            );
        }
        let mut invalid = payload.clone();
        invalid[start + 15..start + 23].copy_from_slice(&2.0f64.to_le_bytes());
        let invalid_index = CompactReferencePlaneIndex::new(&invalid);
        assert_eq!(
            feature_input_sketch_frame(&invalid, &frames, &invalid_index, 0, start, invalid.len()),
            Some(front)
        );
        let placed = (Point3::new(1.0, 0.0, 0.0), front.1, front.2);
        let frames = HashMap::from([(
            2,
            SketchPlaneFrame::from_frame(placed, SketchPlaneUAxisSource::Native),
        )]);
        assert_eq!(
            feature_input_sketch_frame(&payload, &frames, &index, 0, start, payload.len()),
            Some(placed)
        );
    }
}
