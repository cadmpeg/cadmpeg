//! Tests for the `reference_geometry` module's plane-frame pairs.

use super::super::super::{CLASS_MARKER, NAME_MARKER};
use super::super::{
    constraint_reference_plane_frame, enrich_history_reference_planes,
    minimal_reference_plane_frame, offset_reference_plane_frame_pair, tagged_plane_data_frame,
};
use crate::layout::constructed_reference_plane_fixed_frame as fixed_plane;
use crate::layout::constructed_reference_plane_matrix_frame as matrix_plane;
use crate::layout::constructed_reference_plane_minimal_frame as minimal_plane;
use crate::records::{
    Feature, FeatureHistory, FeatureInputLane, FeatureInputName, FeatureSource, ObjectId,
};
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::BTreeMap;
#[test]
fn offset_plane_frame_pair_accepts_complete_matrix_frames() {
    let sine = 0.390_731_128_489_273_27_f64;
    let cosine = 0.920_504_853_452_440_5_f64;
    let frame = |distance: f64| {
        let mut bytes = [0; 121];
        for (offset, value) in [
            (0, -sine * distance / 1000.0),
            (8, 0.0),
            (16, cosine * distance / 1000.0),
            (24, -sine),
            (32, 0.0),
            (40, cosine),
            (49, cosine),
            (57, 0.0),
            (65, -sine),
            (73, 0.0),
            (81, 1.0),
            (89, 0.0),
            (97, sine),
            (105, 0.0),
            (113, cosine),
        ] {
            bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        bytes[48] = 1;
        bytes
    };
    let mut payload = frame(27.25).to_vec();
    payload.extend([0; 13]);
    payload.extend(frame(0.0));

    let (offset, reference) = offset_reference_plane_frame_pair(
        &payload,
        cadmpeg_ir::scalar::Length::new(27.25).unwrap(),
    )
    .unwrap();
    assert_eq!(offset.0, Point3::new(-sine * 27.25, 0.0, cosine * 27.25));
    assert_eq!(reference.0, Point3::new(0.0, 0.0, 0.0));
    assert_eq!(offset.1, reference.1);
    assert_eq!(offset.2, reference.2);
}

#[test]
fn minimal_reference_plane_validates_its_redundant_offset_tail() {
    let root = 13;
    let mut payload = vec![0; root + 81];
    let distance = -0.052_f64;
    for (relative, value) in [
        (0, 0.0_f64),
        (8, 0.0),
        (16, distance),
        (24, 0.0),
        (32, 0.0),
        (40, 1.0),
        (57, -0.0),
        (65, -distance),
        (73, 1.0),
    ] {
        payload[root + relative..root + relative + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[root + 56] = 0x80;
    assert_eq!(
        minimal_reference_plane_frame(&payload),
        Some((
            Point3::new(0.0, 0.0, -52.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        ))
    );

    payload[root + 65..root + 73].copy_from_slice(&0.051f64.to_le_bytes());
    assert_eq!(minimal_reference_plane_frame(&payload), None);
}

fn class_payload(class: &str, frame: &[u8]) -> (Vec<u8>, usize) {
    let mut payload = vec![0xaa; 7];
    let root = payload.len();
    payload.extend(CLASS_MARKER);
    payload.extend(u16::try_from(class.len()).unwrap().to_le_bytes());
    payload.extend(class.as_bytes());
    payload.extend_from_slice(frame);
    (payload, root)
}

fn matrix_frame(marker: u8) -> [u8; matrix_plane::LEN] {
    let mut frame = [0; matrix_plane::LEN];
    for (offset, value) in [(0, 0.0125_f64), (24, 1.0), (65, 1.0), (73, 1.0), (105, 1.0)] {
        frame[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    frame[48] = marker;
    frame
}

#[test]
fn line_perpendicular_and_line_point_plane_data_read_a_matrix_frame() {
    for class in [
        "moConstraintCoincLinePerpPlaneRefplaneData_c",
        "moPerLinePtRefPlnData_c",
    ] {
        let (payload, root) = class_payload(class, &matrix_frame(1));
        assert_eq!(
            constraint_reference_plane_frame(&payload, root, class),
            Some((
                Point3::new(12.5, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
            )),
            "{class}"
        );
    }
    let class = "moConstraintCoincLinePerpPlaneRefplaneData_c";
    let (payload, root) = class_payload(class, &matrix_frame(0));
    assert_eq!(
        constraint_reference_plane_frame(&payload, root, class),
        None
    );
}

fn minimal_frame(
    origin: [f64; 3],
    normal: [f64; 3],
    translation: [f64; 3],
) -> [u8; minimal_plane::LEN] {
    let mut frame = [0; minimal_plane::LEN];
    for (index, value) in origin.into_iter().chain(normal).enumerate() {
        frame[8 * index..8 * index + 8].copy_from_slice(&value.to_le_bytes());
    }
    for (index, value) in translation.into_iter().chain([1.0]).enumerate() {
        let start = minimal_plane::TRANSLATION + 8 * index;
        frame[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }
    frame
}

#[test]
fn a_line_point_plane_reads_its_81_byte_frame() {
    let class = "moPerLinePtRefPlnData_c";
    let origin = [-0.75, 0.35, -0.250_000_000_000_000_06];
    let negated = origin.map(|value: f64| -value);
    let up = [0.0, 0.0, 1.0];
    let read = |frame: &[u8]| {
        let (payload, root) = class_payload(class, frame);
        constraint_reference_plane_frame(&payload, root, class)
    };
    let expected = Some((
        Point3::new(origin[0] * 1000.0, origin[1] * 1000.0, origin[2] * 1000.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
    ));
    let frame = minimal_frame(origin, up, negated);
    assert_eq!(read(&frame), expected);
    let mut followed = frame.to_vec();
    followed.extend([0; 40]);
    assert_eq!(read(&followed), expected);

    assert_eq!(read(&minimal_frame(origin, [0.0, 1.0, 0.0], negated)), None);
    assert_eq!(
        read(&minimal_frame(origin, up, [-0.0, -0.0, negated[2]])),
        None
    );
    let mut scaled = frame;
    scaled[minimal_plane::SCALE..minimal_plane::SCALE + 8].copy_from_slice(&2.0_f64.to_le_bytes());
    assert_eq!(read(&scaled), None);
    let mut marked = frame;
    marked[minimal_plane::FRAME_MARKER] = 1;
    assert_eq!(read(&marked), None);
    assert_eq!(
        read(&minimal_frame(
            [f64::NAN, 0.35, 0.0],
            up,
            [f64::NAN, -0.35, -0.0]
        )),
        None
    );
    assert_eq!(
        read(&minimal_frame([1e307, 0.0, 0.0], up, [-1e307, -0.0, -0.0])),
        None
    );
}

/// A matrix frame at origin `(x, 0, 0)` m whose rows are `rows`.
fn matrix_rows(x: f64, normal: [f64; 3], rows: [[f64; 3]; 3]) -> [u8; matrix_plane::LEN] {
    let mut frame = [0; matrix_plane::LEN];
    let values = [x, 0.0, 0.0]
        .into_iter()
        .chain(normal)
        .enumerate()
        .map(|(index, value)| (8 * index, value))
        .chain(
            rows.into_iter()
                .flatten()
                .enumerate()
                .map(|(index, value)| (49 + 8 * index, value)),
        );
    for (offset, value) in values {
        frame[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    frame[48] = 1;
    frame
}

/// A matrix frame whose rows read in no other frame form.
fn matrix_only(x: f64) -> [u8; matrix_plane::LEN] {
    matrix_rows(
        x,
        [1.0, 0.0, 0.0],
        [[0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
    )
}

/// A plane object named `Plane3` whose plane-data object is written as
/// `token` behind six zero bytes and stores `frame`, and the offset of the
/// frame.
fn tagged_plane_object(token: u16, frame: &[u8]) -> (Vec<u8>, usize) {
    let mut payload = plane_name();
    payload.extend([0x05, 0x01, 0x02]);
    payload.extend([0; 6]);
    payload.extend(token.to_le_bytes());
    let at = payload.len();
    payload.extend_from_slice(frame);
    (payload, at)
}

fn plane_name() -> Vec<u8> {
    let mut payload = NAME_MARKER.to_vec();
    payload.push(6);
    payload.extend("Plane3".encode_utf16().flat_map(u16::to_le_bytes));
    payload
}

fn plane_lane(id: &str, payload: Vec<u8>) -> FeatureInputLane {
    FeatureInputLane {
        id: id.into(),
        configuration: None,
        native_payload: payload.into(),
        classes: Vec::new(),
        names: vec![FeatureInputName {
            id: format!("{id} name"),
            parent: id.into(),
            ordinal: 0,
            offset: 0,
            object_id: ObjectId::from_value(2080),
            value: "Plane3".into(),
        }],
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    }
}

/// The frame properties `enrich_history_reference_planes` gives a plane
/// named `Plane3` in `lanes`.
fn enriched_plane(lanes: &[FeatureInputLane]) -> [Option<String>; 3] {
    let mut histories = vec![FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![Feature {
            id: "plane".into(),
            parent: "history".into(),
            xml_tag: "Feature".into(),
            tree_parent: None,
            source_id: FeatureSource::from_value(2080),
            ordinal: 0,
            name: "Plane3".into(),
            kind: "Plane".into(),
            input_class: None,
            suppressed: false,
            parameters: BTreeMap::new(),
            dimension_properties: BTreeMap::new(),
            properties: BTreeMap::new(),
            text: None,
            content: Vec::new(),
        }],
    }];
    enrich_history_reference_planes(
        &cadmpeg_core::decode::test_support::service_decode_context(),
        &mut histories,
        lanes,
        None,
    )
    .unwrap();
    let properties = &histories[0].features[0].properties;
    ["Origin", "Normal", "UAxis"].map(|key| properties.get(key).cloned())
}

/// A tagged plane object whose interval also holds a second, different frame.
fn ambiguous_tagged_plane() -> Vec<u8> {
    let (mut payload, _) = tagged_plane_object(0x816d, &matrix_only(0.0125));
    payload.extend([0xaa; 16]);
    payload.extend_from_slice(&matrix_only(0.5));
    payload.extend([0xaa; 16]);
    payload
}

#[test]
fn a_tagged_plane_data_object_stores_its_frame_after_the_token() {
    let (payload, _) = tagged_plane_object(0x816d, &matrix_only(0.0125));
    let end = payload.len();
    assert_eq!(
        tagged_plane_data_frame(&plane_lane("lane", payload), 0, end),
        Some((
            Point3::new(12.5, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
        ))
    );
    let lane = plane_lane("lane", tagged_plane_object(0, &matrix_only(0.0125)).0);
    assert_eq!(tagged_plane_data_frame(&lane, 0, end), None);
}

#[test]
fn a_tagged_plane_frame_read_in_two_forms_that_differ_is_unread() {
    let frame = matrix_rows(
        0.0125,
        [0.0, 0.0, 1.0],
        [[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
    );
    let (payload, _) = tagged_plane_object(0x816d, &frame);
    let end = payload.len();
    assert_eq!(
        tagged_plane_data_frame(&plane_lane("lane", payload), 0, end),
        None
    );
}

#[test]
fn a_tagged_plane_frame_is_read_within_its_object() {
    let (sine, cosine) = (0.5, 3.0_f64.sqrt() / 2.0);
    let tilted = matrix_rows(
        0.0125,
        [0.0, -sine, cosine],
        [[1.0, 0.0, 0.0], [0.0, cosine, -sine], [0.0, sine, cosine]],
    );
    let (payload, at) = tagged_plane_object(0x816d, &tilted);
    let end = payload.len();
    assert!(tagged_plane_data_frame(&plane_lane("lane", payload.clone()), 0, end).is_some());
    let lane = plane_lane("lane", payload);
    assert_eq!(
        tagged_plane_data_frame(&lane, 0, at + fixed_plane::LEN),
        None
    );
}

#[test]
fn a_plane_with_a_tagged_frame_and_a_second_frame_states_the_tagged_one() {
    assert_eq!(
        enriched_plane(&[plane_lane("lane", ambiguous_tagged_plane())]),
        [
            Some("12.5mm,0mm,0mm".to_string()),
            Some("1,0,0".to_string()),
            Some("0,1,0".to_string()),
        ]
    );
}

#[test]
fn a_frame_another_lane_reads_without_a_token_precedes_a_tagged_frame() {
    let mut untagged = plane_name();
    untagged.extend([0xaa; 16]);
    untagged.extend_from_slice(&matrix_only(0.25));
    untagged.extend([0xaa; 16]);
    assert_eq!(
        enriched_plane(&[
            plane_lane("lane", untagged),
            plane_lane("other", ambiguous_tagged_plane()),
        ]),
        [
            Some("250mm,0mm,0mm".to_string()),
            Some("1,0,0".to_string()),
            Some("0,1,0".to_string()),
        ]
    );
}

/// A 97-byte repeated-normal frame at origin `(0.0125, 0, 0)` m with normal
/// `+z`, u-axis `+x` at `+49` and the normal repeated at `+73`.
fn repeated_normal() -> [u8; fixed_plane::LEN] {
    let mut frame = [0; fixed_plane::LEN];
    for (offset, value) in [(0, 0.0125_f64), (40, 1.0), (49, 1.0), (89, 1.0)] {
        frame[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    frame[48] = 1;
    frame
}

/// A plane object whose tagged plane-data object stores `frame`, followed by
/// a second, different frame in its interval.
fn ambiguous_plane_with(frame: &[u8]) -> Vec<u8> {
    let (mut payload, _) = tagged_plane_object(0x816d, frame);
    payload.extend([0xaa; 40]);
    payload.extend_from_slice(&matrix_only(0.5));
    payload.extend([0xaa; 16]);
    payload
}

#[test]
fn a_tagged_plane_data_object_can_store_the_repeated_normal_frame() {
    let up = Some((
        Point3::new(12.5, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
    ));
    let (payload, _) = tagged_plane_object(0x816d, &repeated_normal());
    let end = payload.len();
    assert_eq!(
        tagged_plane_data_frame(&plane_lane("lane", payload), 0, end),
        up
    );
    assert_eq!(
        enriched_plane(&[plane_lane("lane", ambiguous_plane_with(&repeated_normal()))]),
        [
            Some("12.5mm,0mm,0mm".to_string()),
            Some("0,0,1".to_string()),
            Some("1,0,0".to_string()),
        ]
    );
}

#[test]
fn a_plane_whose_tagged_forms_differ_in_an_ambiguous_interval_stays_unread() {
    let frame = matrix_rows(
        0.0125,
        [0.0, 0.0, 1.0],
        [[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
    );
    assert_eq!(
        enriched_plane(&[plane_lane("lane", ambiguous_plane_with(&frame))]),
        [None, None, None]
    );
}
