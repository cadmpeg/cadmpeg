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
fn own_principal_component_record_names_the_profile_plane() {
    use crate::resolved_features::compact_reference_planes::{
        principal_sketch_frame, CompactReferencePlaneIndex, PlaneBinding,
    };
    use crate::resolved_features::curves::{SketchPlaneFrame, SketchPlaneUAxisSource};
    use crate::resolved_features::holes::{
        feature_input_sketch_frame, feature_input_support_frame, principal_profile_frame,
    };
    use cadmpeg_ir::features::PrincipalPlane;
    use cadmpeg_ir::math::Point3;
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
        let plane = if selector == 2 {
            PrincipalPlane::Top
        } else {
            PrincipalPlane::Right
        };
        let principal = Some((principal_sketch_frame(plane), plane));
        // The profile's own principal record names its plane, whether the
        // Front reference comes from the preceding object or the lane default,
        // for the sketch and for the hole positions that the profile gives.
        for context in [0, start] {
            assert_eq!(
                principal_profile_frame(&frames, &index, context, start, payload.len()),
                principal
            );
            // The record's source names the plane object that supports the
            // sketch.
            assert_eq!(
                index.principal_profile_source(start, payload.len()),
                Some(source)
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
                Some(principal_sketch_frame(plane))
            );
            assert_eq!(
                feature_input_support_frame(
                    &payload,
                    &frames,
                    &index,
                    context,
                    start,
                    payload.len()
                )
                .map(|support| support.binding),
                Some(PlaneBinding::Principal(plane))
            );
            let frame = principal_sketch_frame(plane);
            let sketch = cadmpeg_ir::sketches::Sketch {
                id: cadmpeg_ir::sketches::SketchId::mint("sldprt:model:sketch#markers:1:1")
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
                true,
            )
            .unwrap();
            // The marker pair `(a, b)` is the local sketch coordinate: Top
            // `(a, 0, -b)`, Right `(0, b, -a)`.
            assert_eq!(transform.apply(point), point.cells());
            let v_axis = frame.1.cross(frame.2);
            let model = frame.0.translated(frame.2, 3.0).translated(v_axis, -2.0);
            assert_eq!(
                model,
                match plane {
                    PrincipalPlane::Top => Point3::new(3.0, 0.0, 2.0),
                    _ => Point3::new(0.0, -2.0, -3.0),
                }
            );
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
            principal_profile_frame(&frames, &reused_index, context, profile_start, reused.len()),
            principal
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
            Some(principal_sketch_frame(plane))
        );
        assert_eq!(
            principal_profile_frame(&frames, &index, 0, start, payload.len()),
            principal
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
            false,
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
            false,
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
    use crate::resolved_features::holes::{feature_input_sketch_frame, principal_profile_frame};
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
        principal_profile_frame(&frames, &index, 0, start, payload.len()),
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

/// The support frame records whether its Top frame comes from a plane
/// reference that names the principal Top plane: a stored plane frame equal to
/// the Top frame is not that route.
#[test]
fn support_frame_records_a_principal_top_reference() {
    use crate::resolved_features::compact_reference_planes::PlaneBinding;
    use crate::resolved_features::compact_reference_planes::{
        principal_sketch_frame, CompactReferencePlaneIndex,
    };
    use crate::resolved_features::curves::{SketchPlaneFrame, SketchPlaneUAxisSource};
    use crate::resolved_features::holes::{feature_input_support_frame, SupportFrame};
    use cadmpeg_ir::features::PrincipalPlane;
    use std::collections::HashMap;

    let mut payload = super::profile_reference_plane_payload(false);
    let source = b"moCompRefPlane_c".len() + 11;
    payload[source..source + 4].copy_from_slice(&3u32.to_le_bytes());
    let index = CompactReferencePlaneIndex::new(&payload);
    let top = principal_sketch_frame(PrincipalPlane::Top);
    for (reference, binding) in [
        (
            SketchPlaneFrame::principal(PrincipalPlane::Top),
            PlaneBinding::TopReference,
        ),
        (
            SketchPlaneFrame::from_frame(top, SketchPlaneUAxisSource::Native),
            PlaneBinding::Stored,
        ),
    ] {
        let frames = HashMap::from([(3, reference)]);
        assert_eq!(
            feature_input_support_frame(&payload, &frames, &index, 0, 0, payload.len()),
            Some(SupportFrame {
                frame: top,
                binding,
            })
        );
    }
}

/// A profile whose own object holds no plane record takes the reference of
/// the preceding object or the lane default, and that binding is assumed; a
/// record inside the profile is stored.
#[test]
fn support_frame_from_the_preceding_object_or_the_lane_is_assumed() {
    use crate::resolved_features::compact_reference_planes::{
        principal_sketch_frame, CompactReferencePlaneIndex, PlaneBinding,
    };
    use crate::resolved_features::curves::{SketchPlaneFrame, SketchPlaneUAxisSource};
    use crate::resolved_features::holes::feature_input_support_frame;
    use cadmpeg_ir::features::PrincipalPlane;
    use std::collections::HashMap;

    let mut payload = super::profile_reference_plane_payload(false);
    let record_end = payload.len();
    payload.extend([0u8; 64]);
    let index = CompactReferencePlaneIndex::new(&payload);
    let front = principal_sketch_frame(PrincipalPlane::Front);
    let frames = HashMap::from([(
        2,
        SketchPlaneFrame::from_frame(front, SketchPlaneUAxisSource::Native),
    )]);
    let binding = |context, start| {
        feature_input_support_frame(&payload, &frames, &index, context, start, payload.len())
            .map(|support| (support.frame, support.binding))
    };
    assert_eq!(binding(0, 0), Some((front, PlaneBinding::Stored)));
    let borrowed = PlaneBinding::Borrowed { own_records: false };
    assert_eq!(binding(0, record_end), Some((front, borrowed)));
    assert_eq!(binding(record_end, record_end), Some((front, borrowed)));
}

/// A profile whose own object holds a declared Front record and a
/// principal-plane component record takes the principal plane, whether the
/// part numbers the principal planes from 2 or from 3, and its own declared
/// record binds it where it lies after the lane's class name.
#[test]
fn own_declared_front_record_keeps_the_principal_plane_in_both_numberings() {
    use crate::resolved_features::compact_reference_planes::{
        principal_sketch_frame, CompactReferencePlaneIndex, PlaneBinding,
    };
    use crate::resolved_features::curves::{SketchPlaneFrame, SketchPlaneUAxisSource};
    use crate::resolved_features::holes::{feature_input_support_frame, principal_profile_frame};
    use cadmpeg_ir::features::PrincipalPlane;
    use std::collections::HashMap;

    let header = super::profile_reference_plane_payload(false);
    let record_start = b"moCompRefPlane_c".len() + 11;
    let front = principal_sketch_frame(PrincipalPlane::Front);
    for front_number in [2u32, 3] {
        let mut declared = header[record_start..record_start + 67].to_vec();
        declared[..4].copy_from_slice(&front_number.to_le_bytes());
        let mut payload = header[..record_start].to_vec();
        payload.extend(&declared);
        payload.extend([0xaa; 24]);
        let start = payload.len();
        payload.extend(&declared);
        let declared_only_end = payload.len();
        let mut component = [0u8; 138];
        component[..4].copy_from_slice(&(front_number + 1).to_le_bytes());
        component[10..14].copy_from_slice(&2u32.to_le_bytes());
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
        // A trailer count of 3, as some parts write it.
        component[122..126].copy_from_slice(&3u32.to_le_bytes());
        component[126..130].fill(0xff);
        payload.extend(component);
        let index = CompactReferencePlaneIndex::new(&payload);
        let frames = HashMap::from([(
            front_number,
            SketchPlaneFrame::from_frame(front, SketchPlaneUAxisSource::Native),
        )]);
        assert_eq!(
            principal_profile_frame(&frames, &index, 0, start, payload.len()),
            Some((
                principal_sketch_frame(PrincipalPlane::Top),
                PrincipalPlane::Top
            )),
            "Front numbered {front_number}"
        );
        let support =
            feature_input_support_frame(&payload, &frames, &index, 0, start, declared_only_end)
                .map(|support| (support.frame, support.binding));
        assert_eq!(support, Some((front, PlaneBinding::Stored)));
        let support =
            feature_input_support_frame(&payload, &frames, &index, 0, start, payload.len())
                .map(|support| (support.frame, support.binding));
        assert_eq!(
            support,
            Some((
                principal_sketch_frame(PrincipalPlane::Top),
                PlaneBinding::Principal(PrincipalPlane::Top)
            ))
        );
    }
}

/// A marker sketch whose own lane gives no frame takes the frame table's
/// principal Top frame, and reads its marker pairs as Top sketch coordinates
/// (`sldprt.md` §3.3) by the same decision: the projected entities and the
/// readers that follow the feature's coordinate tag agree.
#[test]
fn table_top_frame_reads_marker_pairs_by_the_same_decision() {
    use crate::resolved_features::compact_reference_planes::principal_sketch_frame;
    use crate::resolved_features::grid::quantize;
    use crate::resolved_features::transforms::feature_sketch_marker_transform;
    use cadmpeg_ir::features::PrincipalPlane;
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let (history, lanes, position) = super::table_top_position_sketch();
    let mut features = vec![position];
    let (sketches, entities) = super::project_marker_sketches(&mut features, &history, &lanes);

    let [sketch] = sketches.as_slice() else {
        panic!("one marker sketch: {sketches:?}");
    };
    let (origin, normal, u_axis) = principal_sketch_frame(PrincipalPlane::Top);
    assert_eq!(
        sketch.resolved_placement().map(|(origin, normal, u_axis)| (
            origin.get(),
            normal.get(),
            u_axis.get()
        )),
        Some((origin, normal, u_axis))
    );
    assert_eq!(
        features[0]
            .source_properties
            .get("MarkerCoordinateSystem")
            .map(String::as_str),
        Some("principal-top-component")
    );
    let [entity] = entities.as_slice() else {
        panic!("one point entity: {entities:?}");
    };
    assert!(matches!(
        *entity.geometry.definition(),
        SketchGeometryDefinition::Point { position } if position.get() == Point2::new(2.0, 3.0)
    ));
    let stored = quantize(Point2::new(2.0, 3.0), 1.0e-8);
    assert_eq!(
        feature_sketch_marker_transform(sketch, 1.0e-8, &features)
            .and_then(|transform| transform.apply(stored)),
        stored.cells()
    );
}
