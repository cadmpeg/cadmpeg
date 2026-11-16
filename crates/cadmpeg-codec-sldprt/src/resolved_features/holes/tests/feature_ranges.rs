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
        principal_sketch_frame, principal_top_component_frame, CompactReferencePlaneIndex,
    };
    use crate::resolved_features::curves::{SketchPlaneFrame, SketchPlaneUAxisSource};
    use crate::resolved_features::holes::{
        feature_input_sketch_frame, principal_top_profile_frame,
    };
    use cadmpeg_ir::features::PrincipalPlane;
    use cadmpeg_ir::math::{Point3, Vector3};
    use std::collections::HashMap;

    const QUANTUM: f64 = 1.0e-8;

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
            let expected_top = (source == 3 && context == 0).then(principal_top_component_frame);
            assert_eq!(
                principal_top_profile_frame(&frames, &index, context, start, payload.len()),
                expected_top
            );
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
            let frame = expected_top.unwrap_or(expected);
            let sketch = cadmpeg_ir::sketches::Sketch {
                id: cadmpeg_ir::sketches::SketchId::mint("sldprt:model:sketch#synthetic-principal")
                    .unwrap(),
                name: None,
                configuration: None,
                visible: None,
                placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
                    frame.0, frame.1, frame.2,
                )
                .unwrap(),
                profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
                native_ref: None,
            };
            let point = crate::resolved_features::grid::quantize(
                cadmpeg_ir::math::Point2::new(3.0, -2.0),
                QUANTUM,
            );
            let transform = crate::resolved_features::holes::feature_input_sketch_marker_transform(
                &frames,
                &index,
                [context, start, payload.len()],
                &sketch,
                QUANTUM,
            )
            .unwrap();
            if expected_top.is_some() {
                assert_eq!(transform.apply(point), point.cells());
            } else {
                let fallback = crate::resolved_features::transforms::sketch_frame_marker_transform(
                    &sketch, QUANTUM,
                )
                .unwrap();
                assert_eq!(transform.apply(point), fallback.apply(point));
            }
        }
        let declared = super::profile_reference_plane_payload(false);
        let declared_start = b"moCompRefPlane_c".len() + 11;
        let mut reused = payload[..start].to_vec();
        let context = reused.len();
        reused.extend(&declared[declared_start..declared_start + 67]);
        let profile_start = reused.len();
        reused.extend(&payload[start..]);
        let reused_index = CompactReferencePlaneIndex::new(&reused);
        assert_eq!(
            principal_top_profile_frame(
                &frames,
                &reused_index,
                context,
                profile_start,
                reused.len()
            ),
            (source == 3).then(principal_top_component_frame)
        );
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
        assert_eq!(
            principal_top_profile_frame(&frames, &index, 0, start, payload.len()),
            None
        );
    }
}

#[test]
fn serialized_offset_component_uses_model_plane_and_local_markers() {
    use crate::resolved_features::compact_reference_planes::CompactReferencePlaneIndex;
    use crate::resolved_features::curves::{SketchPlaneFrame, SketchPlaneUAxisSource};
    use crate::resolved_features::grid::quantize;
    use crate::resolved_features::holes::{
        feature_input_sketch_frame, feature_input_sketch_marker_transform,
    };
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::sketches::{Sketch, SketchId, SketchPlacement, SketchProfiles};
    use std::collections::HashMap;

    const QUANTUM: f64 = 1.0e-8;
    for distance in [-6.0, 6.0] {
        let mut payload = [0u8; 138];
        payload[..4].copy_from_slice(&7u32.to_le_bytes());
        payload[14] = 1;
        for (index, value) in [
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            -1.0,
            0.0,
            0.0,
            0.0,
            -distance / 1000.0,
            1.0,
        ]
        .into_iter()
        .enumerate()
        {
            let offset = 15 + index * 8;
            payload[offset..offset + 8].copy_from_slice(&f64::to_le_bytes(value));
        }
        payload[122..126].copy_from_slice(&4u32.to_le_bytes());
        payload[126..130].fill(0xff);
        let expected = (
            Point3::new(0.0, distance, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        );
        let frames = HashMap::from([(
            7,
            SketchPlaneFrame::from_frame(expected, SketchPlaneUAxisSource::ResolvedSupportPlane),
        )]);
        let index = CompactReferencePlaneIndex::new(&payload);
        let frame = feature_input_sketch_frame(&payload, &frames, &index, 0, 0, payload.len())
            .expect("paired component plane");
        assert_eq!(frame, expected);
        let sketch = Sketch {
            id: SketchId::mint("sldprt:model:sketch#synthetic-offset").unwrap(),
            name: None,
            configuration: None,
            visible: None,
            placement: SketchPlacement::try_resolved(frame.0, frame.1, frame.2).unwrap(),
            profiles: SketchProfiles::default(),
            native_ref: None,
        };
        let transform = feature_input_sketch_marker_transform(
            &frames,
            &index,
            [0, 0, payload.len()],
            &sketch,
            QUANTUM,
        )
        .expect("local marker coordinates");
        let point = quantize(Point2::new(3.0, -2.0), QUANTUM);
        assert_eq!(transform.apply(point), point.cells());
        let v_axis = frame.1.cross(frame.2);
        let world = frame.0.translated(frame.2, 3.0).translated(v_axis, -2.0);
        assert_eq!(world, Point3::new(3.0, distance, 2.0));
        let opposite = (Point3::new(0.0, -distance, 0.0), frame.1, frame.2);
        let opposite_frames = HashMap::from([(
            7,
            SketchPlaneFrame::from_frame(opposite, SketchPlaneUAxisSource::ResolvedSupportPlane),
        )]);
        assert_eq!(
            feature_input_sketch_frame(&payload, &opposite_frames, &index, 0, 0, payload.len()),
            Some(opposite)
        );
        let rejected = feature_input_sketch_marker_transform(
            &opposite_frames,
            &index,
            [0, 0, payload.len()],
            &sketch,
            QUANTUM,
        )
        .unwrap();
        assert_ne!(rejected.apply(point), point.cells());
    }
}

#[test]
fn enclosing_custom_component_retains_its_profile_frame() {
    use crate::resolved_features::compact_reference_planes::{
        principal_sketch_frame, CompactReferencePlaneIndex,
    };
    use crate::resolved_features::curves::{SketchPlaneFrame, SketchPlaneUAxisSource};
    use crate::resolved_features::holes::{
        feature_input_sketch_frame, principal_top_profile_frame,
    };
    use cadmpeg_ir::features::PrincipalPlane;
    use cadmpeg_ir::math::{Point3, Vector3};
    use std::collections::HashMap;

    let mut component = [0u8; 138];
    component[..4].copy_from_slice(&7u32.to_le_bytes());
    component[14] = 1;
    for (index, value) in [
        1.0f64, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
    .into_iter()
    .enumerate()
    {
        let offset = 15 + index * 8;
        component[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    component[122..126].copy_from_slice(&4u32.to_le_bytes());
    component[126..130].fill(0xff);
    let mut payload = super::profile_reference_plane_payload(false);
    payload.extend(component);
    let start = payload.len();
    component[..4].copy_from_slice(&3u32.to_le_bytes());
    component[10..14].copy_from_slice(&2u32.to_le_bytes());
    payload.extend(component);
    let index = CompactReferencePlaneIndex::new(&payload);
    let frames = HashMap::from([(
        2,
        SketchPlaneFrame::from_frame(
            principal_sketch_frame(PrincipalPlane::Front),
            SketchPlaneUAxisSource::Native,
        ),
    )]);
    assert_eq!(
        principal_top_profile_frame(&frames, &index, 0, start, payload.len()),
        None
    );
    assert_eq!(
        feature_input_sketch_frame(&payload, &frames, &index, 0, start, payload.len()),
        Some((
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0)
        ))
    );
}
