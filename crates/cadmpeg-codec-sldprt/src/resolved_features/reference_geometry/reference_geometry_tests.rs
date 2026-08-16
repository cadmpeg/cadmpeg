//! Tests for the `reference_geometry` module.

use super::super::curves::{sketch_plane_frames, SketchPlaneUAxisSource};
use super::super::{CLASS_MARKER, NAME_MARKER};
use super::*;
use crate::layout::constructed_reference_plane_fixed_frame as fixed_plane;
use crate::layout::constructed_reference_plane_matrix_frame as matrix_plane;
use crate::records::{
    Feature, FeatureHistory, FeatureInputClass, FeatureInputClassRole, FeatureInputLane,
    FeatureInputName,
};
use cadmpeg_ir::features::{FeatureDefinition, FeatureId, Length, PrincipalPlane};
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::{BTreeMap, HashSet};

const REFERENCE_POINT_NAME_END: usize = NAME_MARKER.len() + 1 + 12;

fn reference_point_lane(layout: usize, form: u16, point: [f64; 3]) -> FeatureInputLane {
    let name = "Point1";
    let name_end = REFERENCE_POINT_NAME_END;
    let point_start = name_end + layout;
    let mut payload = vec![0; point_start + 34];
    payload[..NAME_MARKER.len()].copy_from_slice(NAME_MARKER);
    payload[NAME_MARKER.len()] = name.encode_utf16().count() as u8;
    for (index, code_unit) in name.encode_utf16().enumerate() {
        let start = NAME_MARKER.len() + 1 + index * 2;
        payload[start..start + 2].copy_from_slice(&code_unit.to_le_bytes());
    }
    payload[name_end..name_end + 8].copy_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0xc0]);
    payload[name_end + 8..name_end + 12].copy_from_slice(&2080_u32.to_le_bytes());
    for (index, value) in point.into_iter().enumerate() {
        let start = point_start + index * 8;
        payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[point_start + 24..point_start + 26].copy_from_slice(&form.to_le_bytes());
    FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: Vec::new(),
        names: vec![FeatureInputName {
            id: "name".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            object_id: Some(2080),
            value: name.into(),
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

fn reference_point_history() -> FeatureHistory {
    FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![Feature {
            id: "point".into(),
            parent: "history".into(),
            xml_tag: "Feature".into(),
            tree_parent: None,
            source_id: Some("2080".into()),
            parent_source_id: None,
            ordinal: 0,
            name: "Point1".into(),
            kind: "3DPoint".into(),
            input_class: Some("moRefPoint_c".into()),
            suppressed: false,
            parameters: BTreeMap::new(),
            dimension_properties: BTreeMap::new(),
            properties: BTreeMap::new(),
            text: None,
            content: Vec::new(),
        }],
    }
}

struct CoordinateSystemRecord {
    lane: FeatureInputLane,
    origin: usize,
    axes: Vec<usize>,
    tail: usize,
}

fn coordinate_system_record(
    lane_id: &str,
    origin: [f64; 3],
    axes: &[[f64; 3]],
    flips: [u8; 3],
) -> CoordinateSystemRecord {
    const HANDLES: [u8; 8] = [0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff];
    const GENERATION: u32 = 7000;
    let name = "CS1";
    let mut payload = Vec::new();
    payload.extend_from_slice(NAME_MARKER);
    payload.push(name.encode_utf16().count() as u8);
    for code_unit in name.encode_utf16() {
        payload.extend_from_slice(&code_unit.to_le_bytes());
    }
    payload.extend_from_slice(&[0; 16]);

    let origin_offset = payload.len();
    payload.resize(origin_offset + 151, 0);
    payload[origin_offset..origin_offset + 10]
        .copy_from_slice(&[0x2f, 0x80, 0x02, 0, 0, 0, 0, 0, 0, 0]);
    payload[origin_offset + 45..origin_offset + 61].fill(0xff);
    payload[origin_offset + 69..origin_offset + 73].copy_from_slice(&70u32.to_le_bytes());
    payload[origin_offset + 73..origin_offset + 77].copy_from_slice(&123u32.to_le_bytes());
    payload[origin_offset + 79..origin_offset + 81].copy_from_slice(&1u16.to_le_bytes());
    payload[origin_offset + 87..origin_offset + 91].copy_from_slice(&700u32.to_le_bytes());
    payload[origin_offset + 103..origin_offset + 111].copy_from_slice(&HANDLES);
    payload[origin_offset + 115..origin_offset + 119].copy_from_slice(&GENERATION.to_le_bytes());
    for (index, value) in origin.into_iter().enumerate() {
        let start = origin_offset + 127 + index * 8;
        payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }

    let mut axis_offsets = Vec::new();
    for (index, direction) in axes.iter().enumerate() {
        payload.extend_from_slice(&[0xa0 + index as u8, 0x81, 0x01, 0]);
        let axis = payload.len();
        axis_offsets.push(axis);
        payload.resize(axis + 113, 0);
        payload[axis..axis + 8].copy_from_slice(&HANDLES);
        payload[axis + 12..axis + 16].copy_from_slice(&GENERATION.to_le_bytes());
        payload[axis + 32..axis + 40].copy_from_slice(&1.0f64.to_le_bytes());
        for (component, value) in direction.iter().enumerate() {
            let first = axis + 64 + component * 8;
            let repeated = axis + 89 + component * 8;
            payload[first..first + 8].copy_from_slice(&value.to_le_bytes());
            payload[repeated..repeated + 8].copy_from_slice(&value.to_le_bytes());
        }
    }
    let tail = payload.len();
    payload.extend_from_slice(&flips);
    for value in origin {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.extend_from_slice(&[0xfd, 0x9a]);

    CoordinateSystemRecord {
        lane: FeatureInputLane {
            id: lane_id.into(),
            configuration: None,
            native_payload: payload,
            classes: Vec::new(),
            names: vec![FeatureInputName {
                id: format!("{lane_id}-name"),
                parent: lane_id.into(),
                ordinal: 0,
                offset: 0,
                object_id: Some(500),
                value: name.into(),
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
        },
        origin: origin_offset,
        axes: axis_offsets,
        tail,
    }
}

fn coordinate_system_history() -> FeatureHistory {
    FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![Feature {
            id: "coordinate-system".into(),
            parent: "history".into(),
            xml_tag: "Feature".into(),
            tree_parent: None,
            source_id: Some("500".into()),
            parent_source_id: None,
            ordinal: 0,
            name: "CS1".into(),
            kind: "Coordinate System".into(),
            input_class: Some("moCoordSys_c".into()),
            suppressed: false,
            parameters: BTreeMap::new(),
            dimension_properties: BTreeMap::new(),
            properties: BTreeMap::new(),
            text: None,
            content: Vec::new(),
        }],
    }
}

#[test]
fn solved_reference_point_layouts_project_to_a_datum_point() {
    for (layout, form) in [(243, 4), (259, 5)] {
        let lane = reference_point_lane(layout, form, [0.125, -0.25, 0.0]);
        let mut histories = vec![reference_point_history()];
        super::enrich_history_reference_points(&mut histories, &[lane]);
        assert_eq!(
            histories[0].features[0].properties.get("Position"),
            Some(&"125mm,-250mm,0mm".to_string())
        );
        assert!(matches!(
            crate::history::project_features(&histories)[0].definition,
            FeatureDefinition::DatumPoint {
                position: Point3 {
                    x: 125.0,
                    y: -250.0,
                    z: 0.0
                },
                ..
            }
        ));
    }

    let mut lanes = [
        reference_point_lane(243, 5, [0.125, -0.25, 0.0]),
        reference_point_lane(259, 5, [0.5, -0.25, 0.0]),
    ];
    lanes[1].id = "lane-2".into();
    lanes[1].names[0].parent = "lane-2".into();
    let mut histories = vec![reference_point_history()];
    super::enrich_history_reference_points(&mut histories, &lanes);
    assert!(!histories[0].features[0].properties.contains_key("Position"));
}

#[test]
fn solved_reference_point_requires_one_complete_layout() {
    let mut lane = reference_point_lane(259, 5, [0.125, -0.25, 0.5]);
    let name = lane.names[0].clone();
    let end = lane.native_payload.len();
    assert_eq!(
        resolved_reference_point(&lane.native_payload, &name, end),
        Some(Point3::new(125.0, -250.0, 500.0))
    );
    assert_eq!(
        resolved_reference_point(&lane.native_payload, &name, end - 1),
        None
    );

    lane.native_payload[REFERENCE_POINT_NAME_END + 8..REFERENCE_POINT_NAME_END + 12]
        .copy_from_slice(&2081_u32.to_le_bytes());
    assert_eq!(
        resolved_reference_point(&lane.native_payload, &name, end),
        None
    );
    lane.native_payload[REFERENCE_POINT_NAME_END + 8..REFERENCE_POINT_NAME_END + 12]
        .copy_from_slice(&2080_u32.to_le_bytes());
    lane.native_payload[REFERENCE_POINT_NAME_END + 259..REFERENCE_POINT_NAME_END + 259 + 8]
        .copy_from_slice(&f64::NAN.to_le_bytes());
    assert_eq!(
        resolved_reference_point(&lane.native_payload, &name, end),
        None
    );

    let mut lane = reference_point_lane(243, 5, [0.0, 0.0, 3.0]);
    lane.native_payload
        .resize(REFERENCE_POINT_NAME_END + 259 + 34, 0);
    let second = REFERENCE_POINT_NAME_END + 259;
    for (index, value) in [3.0_f64, f64::from_bits(5), 0.0].into_iter().enumerate() {
        let start = second + index * 8;
        lane.native_payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }
    lane.native_payload[second + 24..second + 26].copy_from_slice(&5_u16.to_le_bytes());
    assert_eq!(
        resolved_reference_point(
            &lane.native_payload,
            &lane.names[0],
            lane.native_payload.len()
        ),
        None
    );
}

#[test]
fn solved_coordinate_system_projects_orthogonalized_flipped_frame() {
    let record = coordinate_system_record(
        "lane",
        [0.125, -0.25, 0.5],
        &[[1.0, 0.0, 0.0], [0.2, 0.979_795_897_113_271_2, 0.0]],
        [1, 1, 0],
    );
    let mut histories = vec![coordinate_system_history()];
    super::enrich_history_coordinate_systems(&mut histories, &[record.lane]);
    assert_eq!(
        histories[0].features[0].properties.get("Origin"),
        Some(&"125mm,-250mm,500mm".to_string())
    );
    assert!(matches!(
        crate::history::project_features(&histories)[0].definition,
        FeatureDefinition::DatumCoordinateSystem {
            origin: Point3 {
                x: 125.0,
                y: -250.0,
                z: 500.0
            },
            x_axis: Vector3 {
                x: -1.0,
                y: 0.0,
                z: 0.0
            },
            y_axis: Vector3 {
                x: 0.0,
                y: -1.0,
                z: 0.0
            },
            z_axis: Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0
            }
        }
    ));
}

#[test]
fn solved_coordinate_system_requires_one_exact_complete_frame() {
    let record = coordinate_system_record(
        "lane",
        [0.125, -0.25, 0.5],
        &[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        [0, 0, 0],
    );
    assert_eq!(
        resolved_coordinate_system(&record.lane.native_payload),
        Some((
            Point3::new(125.0, -250.0, 500.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ))
    );

    let mut other_generation = record.lane.native_payload.clone();
    for offset in
        std::iter::once(record.origin + 115).chain(record.axes.iter().map(|axis| axis + 12))
    {
        other_generation[offset..offset + 4].copy_from_slice(&8000u32.to_le_bytes());
    }
    assert!(resolved_coordinate_system(&other_generation).is_some());

    let mut alternate_family = record.lane.native_payload.clone();
    alternate_family[record.origin] = 0x2d;
    assert!(resolved_coordinate_system(&alternate_family).is_some());
    alternate_family[record.origin] = 0x2e;
    assert_eq!(resolved_coordinate_system(&alternate_family), None);

    let mut extended_origin = record.lane.native_payload.clone();
    extended_origin.splice(record.origin + 103..record.origin + 103, [0; 14]);
    extended_origin[record.origin + 77..record.origin + 81].copy_from_slice(&1234u32.to_le_bytes());
    extended_origin[record.origin + 81..record.origin + 85].fill(0xff);
    extended_origin[record.origin + 85..record.origin + 89].fill(0);
    extended_origin[record.origin + 89..record.origin + 93].copy_from_slice(&2u32.to_le_bytes());
    extended_origin[record.origin + 93..record.origin + 97].copy_from_slice(&1u32.to_le_bytes());
    extended_origin[record.origin + 97..record.origin + 101].fill(0);
    extended_origin[record.origin + 101..record.origin + 105]
        .copy_from_slice(&700u32.to_le_bytes());
    extended_origin[record.origin + 105..record.origin + 117].fill(0);
    assert!(resolved_coordinate_system(&extended_origin).is_some());

    let first_point = extended_origin[record.origin..record.origin + 165].to_vec();
    let mut second_point = first_point.clone();
    for (index, value) in [0.125_f64, 0.75, 0.5].into_iter().enumerate() {
        let offset = 141 + index * 8;
        second_point[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    let mut two_point_frame = record.lane.native_payload[..record.origin].to_vec();
    two_point_frame.extend_from_slice(&first_point);
    two_point_frame
        .extend_from_slice(&[2, 0, 1, 0, 0, 0, 0x99, 0xc4, 1, 0, 0x9b, 0xc4, 0x90, 0x81]);
    two_point_frame.extend_from_slice(&second_point);
    two_point_frame.extend_from_slice(&(-0.25f64).to_le_bytes());
    two_point_frame.extend_from_slice(&0.5f64.to_le_bytes());
    for value in [1.0_f64, 0.0, 0.0] {
        two_point_frame.extend_from_slice(&value.to_le_bytes());
    }
    two_point_frame.push(0);
    for value in [1.0_f64, 0.0, 0.0] {
        two_point_frame.extend_from_slice(&value.to_le_bytes());
    }
    two_point_frame.extend_from_slice(&[0; 3]);
    for value in [0.125_f64, -0.25, 0.5] {
        two_point_frame.extend_from_slice(&value.to_le_bytes());
    }
    two_point_frame.extend_from_slice(&0xc491u16.to_le_bytes());
    assert_eq!(
        resolved_coordinate_system(&two_point_frame),
        Some((
            Point3::new(125.0, -250.0, 500.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ))
    );
    let mut malformed_two_point = two_point_frame.clone();
    let separator = record.origin + first_point.len();
    malformed_two_point[separator] = 3;
    assert_eq!(resolved_coordinate_system(&malformed_two_point), None);
    let mut malformed_two_point = two_point_frame;
    let repeated_direction = record.origin + first_point.len() + 14 + second_point.len() + 41;
    malformed_two_point[repeated_direction..repeated_direction + 8]
        .copy_from_slice(&(-1.0f64).to_le_bytes());
    assert_eq!(resolved_coordinate_system(&malformed_two_point), None);

    let mut component_path_origin = Vec::new();
    component_path_origin
        .extend_from_slice(&record.lane.native_payload[record.origin..record.origin + 73]);
    component_path_origin.extend_from_slice(&[0xff; 7]);
    component_path_origin.extend_from_slice(&3u32.to_le_bytes());
    component_path_origin.extend_from_slice(&[0, 2, 0, 0]);
    component_path_origin.extend_from_slice(&[0; 4]);
    component_path_origin.extend_from_slice(&[
        0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2, 0x54, 0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2,
        0x54,
    ]);
    component_path_origin.extend_from_slice(&[0; 2]);
    for index in 0..3u32 {
        component_path_origin.extend_from_slice(&(0x8001 + index as u16).to_le_bytes());
        component_path_origin.extend_from_slice(&[0; 2]);
        component_path_origin.extend_from_slice(&[0x38, 0x80, 0x3b, 0, 0x68, 1, 0, 0]);
        component_path_origin.extend_from_slice(&(700 + index).to_le_bytes());
        component_path_origin.extend_from_slice(&(10 + index).to_le_bytes());
    }
    component_path_origin.extend_from_slice(&[0; 14]);
    component_path_origin.extend_from_slice(&1u32.to_le_bytes());
    component_path_origin.extend_from_slice(&[0; 4]);
    component_path_origin.extend_from_slice(&700u32.to_le_bytes());
    component_path_origin.extend_from_slice(&[0; 12]);
    component_path_origin.extend_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
    component_path_origin.extend_from_slice(&[0; 4]);
    component_path_origin.extend_from_slice(&7000u32.to_le_bytes());
    component_path_origin.extend_from_slice(&[0; 8]);
    for value in [0.125_f64, -0.25, 0.5] {
        component_path_origin.extend_from_slice(&value.to_le_bytes());
    }
    let mut component_path_record = record.lane.native_payload.clone();
    component_path_record.splice(
        record.origin..record.origin + 151,
        component_path_origin.clone(),
    );
    assert!(resolved_coordinate_system(&component_path_record).is_some());

    let path_end = 110 + 3 * 20;
    component_path_origin.splice(path_end..path_end, [0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]);
    let mut null_terminated_path = record.lane.native_payload.clone();
    null_terminated_path.splice(record.origin..record.origin + 151, component_path_origin);
    assert!(resolved_coordinate_system(&null_terminated_path).is_some());

    let mut malformed_path = component_path_record;
    malformed_path[record.origin + path_end] = 1;
    assert_eq!(resolved_coordinate_system(&malformed_path), None);

    let mut endpoint_origin = Vec::new();
    endpoint_origin.extend_from_slice(&[
        0x2f, 0x80, 0x02, 0, 0, 0, 0x40, 0, 0, 0x75, 0, 0, 0, 0x75, 0, 0, 0,
    ]);
    endpoint_origin.extend_from_slice(&[0; 28]);
    endpoint_origin.extend_from_slice(&[0xff; 16]);
    endpoint_origin.extend_from_slice(&[0; 8]);
    endpoint_origin.extend_from_slice(&0x0001_8528u32.to_le_bytes());
    endpoint_origin.extend_from_slice(&[0; 7]);
    endpoint_origin.extend_from_slice(&3u32.to_le_bytes());
    endpoint_origin.extend_from_slice(&[0, 2, 0, 0]);
    endpoint_origin.extend_from_slice(&0x01ee_b3c6u32.to_le_bytes());
    endpoint_origin.extend_from_slice(&[
        0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2, 0x54, 0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2,
        0x54,
    ]);
    endpoint_origin.extend_from_slice(&[0; 2]);
    for index in 0..3u32 {
        endpoint_origin.extend_from_slice(&(0x8001 + index as u16).to_le_bytes());
        endpoint_origin.extend_from_slice(&[0; 2]);
        endpoint_origin.extend_from_slice(&[0x38, 0x80, 0x3b, 0, 0x68, 1, 0, 0]);
        endpoint_origin.extend_from_slice(&(800 + index).to_le_bytes());
        endpoint_origin.extend_from_slice(&(20 + index).to_le_bytes());
    }
    endpoint_origin.extend_from_slice(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]);
    let endpoint_trailer = endpoint_origin.len();
    endpoint_origin.extend_from_slice(&[0; 70]);
    endpoint_origin.extend_from_slice(&1u32.to_le_bytes());
    endpoint_origin.extend_from_slice(&[0; 4]);
    endpoint_origin.extend_from_slice(&700u32.to_le_bytes());
    endpoint_origin.extend_from_slice(&[0; 12]);
    endpoint_origin.extend_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
    endpoint_origin.extend_from_slice(&[0; 4]);
    endpoint_origin.extend_from_slice(&7000u32.to_le_bytes());
    endpoint_origin.extend_from_slice(&[0; 8]);
    for value in [0.125_f64, -0.25, 0.5] {
        endpoint_origin.extend_from_slice(&value.to_le_bytes());
    }
    let mut endpoint_frame = record.lane.native_payload[..record.origin].to_vec();
    endpoint_frame.extend_from_slice(&endpoint_origin);
    endpoint_frame.extend_from_slice(&record.lane.native_payload[record.origin + 151..]);
    assert_eq!(
        resolved_coordinate_system(&endpoint_frame),
        resolved_coordinate_system(&record.lane.native_payload)
    );
    endpoint_frame[record.origin + endpoint_trailer] = 1;
    assert_eq!(resolved_coordinate_system(&endpoint_frame), None);

    let mut ordinal_frame = record.lane.native_payload.clone();
    ordinal_frame.truncate(record.origin + 151);
    ordinal_frame.extend_from_slice(&2u16.to_le_bytes());
    ordinal_frame.extend_from_slice(&1u16.to_le_bytes());
    ordinal_frame.extend_from_slice(&[0; 23]);
    ordinal_frame.extend_from_slice(&0.5f64.to_le_bytes());
    ordinal_frame.extend_from_slice(&0x8090u16.to_le_bytes());
    assert_eq!(
        resolved_coordinate_system(&ordinal_frame),
        Some((
            Point3::new(125.0, -250.0, 500.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, -1.0),
        ))
    );
    let selector = ordinal_frame.len() - 37;
    ordinal_frame[selector + 2..selector + 4].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(resolved_coordinate_system(&ordinal_frame), None);
    ordinal_frame[selector + 2..selector + 4].copy_from_slice(&1u16.to_le_bytes());
    ordinal_frame[selector + 27..selector + 35].copy_from_slice(&0.25f64.to_le_bytes());
    assert_eq!(resolved_coordinate_system(&ordinal_frame), None);

    let mut malformed = record.lane.native_payload.clone();
    malformed[record.origin + 115..record.origin + 119].copy_from_slice(&9000u32.to_le_bytes());
    assert_eq!(resolved_coordinate_system(&malformed), None);

    let mut malformed = record.lane.native_payload.clone();
    let duplicate_origin = malformed[record.origin..record.origin + 151].to_vec();
    malformed.splice(record.origin..record.origin, duplicate_origin);
    assert_eq!(resolved_coordinate_system(&malformed), None);

    let mut malformed = record.lane.native_payload.clone();
    let extra_axis = malformed[record.axes[0]..record.axes[0] + 113].to_vec();
    malformed.splice(record.axes[0]..record.axes[0], extra_axis);
    assert_eq!(resolved_coordinate_system(&malformed), None);

    let mut malformed = record.lane.native_payload.clone();
    malformed[record.axes[1] + 89..record.axes[1] + 97].copy_from_slice(&0.5f64.to_le_bytes());
    assert_eq!(resolved_coordinate_system(&malformed), None);

    let mut malformed = record.lane.native_payload.clone();
    malformed[record.tail + 2] = 1;
    assert_eq!(resolved_coordinate_system(&malformed), None);

    let mut malformed = record.lane.native_payload.clone();
    malformed[record.tail + 3..record.tail + 11].copy_from_slice(&0.25f64.to_le_bytes());
    assert_eq!(resolved_coordinate_system(&malformed), None);
    assert_eq!(
        resolved_coordinate_system(
            &record.lane.native_payload[..record.lane.native_payload.len() - 1]
        ),
        None
    );

    let collinear = coordinate_system_record(
        "lane",
        [0.0, 0.0, 0.0],
        &[[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0]],
        [0, 0, 0],
    );
    assert_eq!(
        resolved_coordinate_system(&collinear.lane.native_payload),
        None
    );
    for axes in [
        &[[1.0, 0.0, 0.0]][..],
        &[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]][..],
    ] {
        let incomplete = coordinate_system_record("lane", [0.0; 3], axes, [0, 0, 0]);
        assert_eq!(
            resolved_coordinate_system(&incomplete.lane.native_payload),
            None
        );
    }
}

#[test]
fn solved_coordinate_system_constructs_y_from_one_offset_line_axis() {
    let mut record =
        coordinate_system_record("lane", [0.0, 0.0, 0.0], &[[1.0, 0.0, 0.0]], [1, 1, 0]);
    let axis = record.axes[0];
    for (component, value) in [2.0_f64, 3.0, 0.0].into_iter().enumerate() {
        let offset = axis + 40 + component * 8;
        record.lane.native_payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    assert_eq!(
        resolved_coordinate_system(&record.lane.native_payload),
        Some((
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(-1.0, 0.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ))
    );

    record
        .lane
        .native_payload
        .splice(record.tail..record.tail, [0, 0]);
    assert!(resolved_coordinate_system(&record.lane.native_payload).is_some());
    record.lane.native_payload[record.tail] = 1;
    assert_eq!(
        resolved_coordinate_system(&record.lane.native_payload),
        None
    );
}

#[test]
fn solved_coordinate_system_rejects_cross_lane_disagreement() {
    let first = coordinate_system_record(
        "lane-1",
        [0.0, 0.0, 0.0],
        &[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        [0, 0, 0],
    );
    let second = coordinate_system_record(
        "lane-2",
        [0.001, 0.0, 0.0],
        &[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        [0, 0, 0],
    );
    let mut histories = vec![coordinate_system_history()];
    super::enrich_history_coordinate_systems(&mut histories, &[first.lane, second.lane]);
    assert!(histories[0].features[0].properties.is_empty());
}

#[test]
fn sketch_block_terminal_identity_carries_its_origin() {
    let mut payload = vec![0; 100];
    payload[8..12].copy_from_slice(&[0xff; 4]);
    payload[20..26].copy_from_slice(&[0x02, 0, 0, 0, 0, 0]);
    payload[26..28].copy_from_slice(&17_u16.to_le_bytes());
    payload[48..52].copy_from_slice(&[0, 0, 1, 0]);
    payload[52..54].copy_from_slice(&[0x73, 0x81]);
    for (index, value) in [0.125_f64, -0.25, 0.0].into_iter().enumerate() {
        let start = 54 + index * 8;
        payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }
    assert_eq!(
        sketch_block_record_origin(&payload, 0, payload.len()),
        Some(Point3::new(125.0, -250.0, 0.0))
    );

    payload[52..].fill(0);
    payload[52..56].copy_from_slice(CLASS_MARKER);
    payload[56..58].copy_from_slice(&17_u16.to_le_bytes());
    payload[58..75].copy_from_slice(b"moAbsolutePoint_c");
    assert_eq!(
        sketch_block_record_origin(&payload, 0, payload.len()),
        Some(Point3::new(0.0, 0.0, 0.0))
    );
}

#[test]
fn sketch_block_identity_normalization_is_inverted_for_placement() {
    let mut payload = vec![0; 300];
    payload.extend_from_slice(CLASS_MARKER);
    payload.extend_from_slice(&7_u16.to_le_bytes());
    payload.extend_from_slice(b"sgBlock");
    let body = payload.len();
    payload.resize(body + 184, 0);
    for (index, value) in [1.0_f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
        .into_iter()
        .enumerate()
    {
        let start = body + 72 + index * 8;
        payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[body + 144..body + 152].copy_from_slice(&1_u64.to_le_bytes());
    for (index, value) in [-0.21_f64, 0.661, 0.0].into_iter().enumerate() {
        let start = body + 152 + index * 8;
        payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[body + 176..body + 184].copy_from_slice(&1.0_f64.to_le_bytes());

    assert_eq!(
        sketch_block_identity_normalization_origin(&payload, 200, payload.len()),
        Some(Point3::new(210.0, -661.0, 0.0))
    );
}

#[test]
fn plane_intersection_axis_requires_two_complete_known_references() {
    let record = |source: u32, object: u8, selector: u8| {
        let mut bytes = vec![0; 46];
        bytes[..4].copy_from_slice(&source.to_le_bytes());
        bytes[4..8].copy_from_slice(&0x6255_5715u32.to_le_bytes());
        bytes[14..16].copy_from_slice(&[1, 0]);
        bytes[22] = object;
        bytes[30] = selector;
        bytes[38..46].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
        bytes
    };
    let mut payload = record(17, 0xb6, 3);
    payload.extend_from_slice(&record(23, 0x98, 0));
    let known = [17, 23].into_iter().collect();
    assert_eq!(
        plane_intersection_axis_sources(&payload, &known),
        Some([17, 23])
    );

    payload.pop();
    assert_eq!(plane_intersection_axis_sources(&payload, &known), None);
    let incomplete = record(17, 0xb6, 3);
    assert_eq!(plane_intersection_axis_sources(&incomplete, &known), None);
}

#[test]
fn legacy_reference_axis_triad_requires_consecutive_native_records() {
    let feature = |ordinal: u32, source: u32, class: &str| Feature {
        id: format!("feature-{ordinal}"),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some(source.to_string()),
        parent_source_id: None,
        ordinal,
        name: String::new(),
        kind: String::new(),
        input_class: Some(class.into()),
        suppressed: false,
        parameters: BTreeMap::default(),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: Vec::new(),
    };
    let mut features = (0..3)
        .map(|index| feature(10 + index, 40 + index, "moRefPlane_c"))
        .chain((0..3).map(|index| feature(13 + index, 43 + index, "moRefAxis_c")))
        .collect::<Vec<_>>();
    assert_eq!(
        legacy_reference_axis_triads(&features),
        vec![([3, 4, 5], [[40, 41], [40, 42], [42, 41]])]
    );

    features.insert(3, feature(99, 4, "moRefPlane_c"));
    assert_eq!(
        legacy_reference_axis_triads(&features),
        vec![([4, 5, 6], [[40, 41], [40, 42], [42, 41]])]
    );

    features[5].source_id = Some("99".into());
    assert!(legacy_reference_axis_triads(&features).is_empty());
}

#[test]
fn plane_intersection_axis_uses_the_closest_point_to_the_origin() {
    let first = (
        Point3::new(2.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    );
    let second = (
        Point3::new(0.0, -3.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
    );
    assert_eq!(
        plane_intersection_axis_frame(first, second),
        Some((Point3::new(2.0, -3.0, 0.0), Vector3::new(0.0, 0.0, 1.0),))
    );

    let parallel = (
        Point3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    );
    assert_eq!(plane_intersection_axis_frame(first, parallel), None);
}

#[test]
fn explicit_reference_axis_requires_redundant_collinear_witnesses() {
    let mut record = vec![0; 88];
    for (offset, value) in [
        (0, 0.25_f64),
        (8, -0.4),
        (16, 0.1),
        (24, 0.25),
        (32, 0.6),
        (40, 0.1),
        (48, 0.0),
        (56, -0.5),
        (64, 0.0),
        (72, 1.0),
        (80, 0.0),
    ] {
        record[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    let mut payload = vec![0xaa; 17];
    payload.extend_from_slice(&record);
    payload.extend_from_slice(&[0xbb; 11]);
    assert_eq!(
        explicit_reference_axis_frame(&payload),
        Some((Point3::new(250.0, 0.0, 100.0), Vector3::new(0.0, 1.0, 0.0),))
    );

    record[24..32].copy_from_slice(&0.5_f64.to_le_bytes());
    assert_eq!(explicit_reference_axis_frame(&record), None);
}

#[test]
fn explicit_reference_axis_does_not_rank_unanchored_candidates() {
    let frame = |origin_x: f64, first_scalar: f64, second_scalar: f64| {
        let mut record = vec![0; 88];
        for (offset, value) in [
            (0, origin_x),
            (8, -0.4),
            (16, 0.1),
            (24, origin_x),
            (32, 0.6),
            (40, 0.1),
            (48, first_scalar),
            (56, second_scalar),
            (64, 0.0),
            (72, 1.0),
            (80, 0.0),
        ] {
            record[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        record
    };
    let mut payload = frame(0.25, 0.0, -0.5);
    payload.extend_from_slice(&[0xff; 88]);
    payload.extend_from_slice(&frame(0.35, 1.0, 1.0));
    assert_eq!(explicit_reference_axis_frame(&payload), None);
}

#[test]
fn two_points_axis_data_frame_is_anchored_after_class_name() {
    let class_name = b"moTwoPtsAxisData_c";
    let class_offset = 16;
    let body = class_offset + CLASS_MARKER.len() + 2 + class_name.len();
    let mut payload = vec![0; body + 88];
    payload[class_offset..class_offset + CLASS_MARKER.len()].copy_from_slice(CLASS_MARKER);
    payload[class_offset + CLASS_MARKER.len()..class_offset + CLASS_MARKER.len() + 2]
        .copy_from_slice(&(class_name.len() as u16).to_le_bytes());
    payload[class_offset + CLASS_MARKER.len() + 2..body].copy_from_slice(class_name);
    for (offset, value) in [
        (0, 0.25_f64),
        (8, -0.4),
        (16, 0.1),
        (24, 0.25),
        (32, 0.6),
        (40, 0.1),
        (48, 0.0),
        (56, 1.0),
        (64, 0.0),
        (72, 1.0),
        (80, 0.0),
    ] {
        payload[body + offset..body + offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    let mut histories = vec![FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![Feature {
            id: "axis".into(),
            parent: "history".into(),
            xml_tag: "Feature".into(),
            tree_parent: None,
            source_id: Some("2080".into()),
            parent_source_id: None,
            ordinal: 0,
            name: "Axis1".into(),
            kind: String::new(),
            input_class: Some("moRefAxis_c".into()),
            suppressed: false,
            parameters: BTreeMap::new(),
            dimension_properties: BTreeMap::new(),
            properties: BTreeMap::new(),
            text: None,
            content: Vec::new(),
        }],
    }];
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: vec![FeatureInputClass {
            id: "class".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: class_offset as u64,
            name: String::from_utf8(class_name.to_vec()).unwrap(),
            role: FeatureInputClassRole::default(),
        }],
        names: vec![FeatureInputName {
            id: "name".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            object_id: Some(2080),
            value: "Axis1".into(),
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
    };

    super::enrich_history_reference_axes(&mut histories, &[lane]);

    assert_eq!(
        histories[0].features[0].properties.get("Origin"),
        Some(&"250mm,0mm,100mm".to_string())
    );
    assert_eq!(
        histories[0].features[0].properties.get("Direction"),
        Some(&"0,1,0".to_string())
    );
}

#[test]
fn intersecting_reference_axis_pair_completes_legacy_triad() {
    let frames = [
        Some((Point3::new(0.0, 85.0, 0.0), Vector3::new(1.0, 0.0, 0.0))),
        None,
        Some((Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, -1.0, 0.0))),
    ];

    assert_eq!(
        super::complete_reference_axis_triad(frames),
        Some((
            1,
            (Point3::new(0.0, 85.0, 0.0), Vector3::new(0.0, 0.0, -1.0),),
        ))
    );
}

#[test]
fn skew_reference_axes_do_not_complete_legacy_triad() {
    let frames = [
        Some((Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0))),
        None,
        Some((Point3::new(0.0, 1.0, 1.0), Vector3::new(0.0, 1.0, 0.0))),
    ];

    assert_eq!(super::complete_reference_axis_triad(frames), None);
}

#[test]
fn fixed_reference_plane_uses_all_three_stored_basis_vectors() {
    let mut frame = [0; fixed_plane::LEN];
    for (offset, value) in [
        (0, 0.374_f64),
        (8, -0.25),
        (16, 0.125),
        (24, 1.0),
        (32, 0.0),
        (40, 0.0),
        (49, 0.0),
        (57, 0.0),
        (65, 1.0),
        (73, 0.0),
        (81, 1.0),
        (89, 0.0),
    ] {
        frame[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    frame[48] = 1;
    assert_eq!(
        fixed_reference_plane_frame(&frame),
        Some((
            Point3::new(374.0, -250.0, 125.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ))
    );

    frame[73..81].copy_from_slice(&1.0f64.to_le_bytes());
    assert_eq!(fixed_reference_plane_frame(&frame), None);
    assert_eq!(fixed_reference_plane_frame(&frame[..96]), None);

    frame[81..89].fill(0);
    frame[89..97].fill(0);
    assert_eq!(
        explicit_reference_plane_frame(&frame),
        Ok(Some((
            Point3::new(374.0, -250.0, 125.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        )))
    );
}

#[test]
fn reference_plane_frame_identity_canonicalizes_signed_zero() {
    let positive = (
        Point3::new(0.0, 1.0, 2.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
    );
    let negative = (
        Point3::new(-0.0, 1.0, 2.0),
        Vector3::new(1.0, -0.0, 0.0),
        Vector3::new(0.0, -0.0, 1.0),
    );

    assert_eq!(
        reference_plane_frame_key(&positive),
        reference_plane_frame_key(&negative)
    );
}

#[test]
fn offset_plane_frame_pair_stores_result_before_reference() {
    let frame = |origin_x: f64| {
        let mut bytes = [0; fixed_plane::LEN];
        for (offset, value) in [
            (0, origin_x / 1000.0),
            (8, 0.0),
            (16, 0.0),
            (24, 1.0),
            (32, 0.0),
            (40, 0.0),
            (49, 0.0),
            (57, 0.0),
            (65, 1.0),
            (73, 0.0),
            (81, 1.0),
            (89, 0.0),
        ] {
            bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        bytes[48] = 1;
        bytes
    };
    let mut payload = frame(-37.0).to_vec();
    payload.extend([0; 13]);
    payload.extend(frame(0.0));

    assert_eq!(
        offset_reference_plane_frame_pair(&payload, 37.0),
        Some((
            (
                Point3::new(-37.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            ),
            (
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            ),
        ))
    );
    payload[65..73].copy_from_slice(&(-1.0_f64).to_le_bytes());
    assert!(offset_reference_plane_frame_pair(&payload, 37.0).is_some());
    assert_eq!(offset_reference_plane_frame_pair(&payload, 38.0), None);

    let mut antiparallel = frame(-37.0).to_vec();
    antiparallel[24..32].copy_from_slice(&(-1.0_f64).to_le_bytes());
    antiparallel.extend([0; 13]);
    antiparallel.extend(frame(0.0));
    assert!(offset_reference_plane_frame_pair(&antiparallel, 37.0).is_some());
}

#[test]
fn offset_plane_frame_pair_uses_matrix_axes_instead_of_fixed_prefixes() {
    let frame = |origin_x: f64| {
        let mut bytes = [0; matrix_plane::LEN];
        for (offset, value) in [
            (0, origin_x),
            (24, 1.0),
            (49, 0.0),
            (57, 0.0),
            (65, 1.0),
            (73, 0.0),
            (81, 1.0),
            (89, 0.0),
            (97, -1.0),
            (105, 0.0),
            (113, 0.0),
        ] {
            bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        bytes[48] = 1;
        bytes
    };
    let mut payload = frame(-0.037).to_vec();
    payload.extend([0; 13]);
    payload.extend(frame(0.0));

    assert_eq!(
        offset_reference_plane_frame_pair(&payload, 37.0),
        Some((
            (
                Point3::new(-37.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, -1.0),
            ),
            (
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, -1.0),
            ),
        ))
    );
}

#[test]
fn offset_plane_frame_pair_accepts_ordered_mixed_frame_layouts() {
    let mut result = [0; MINIMAL_REFERENCE_PLANE_FRAME_LEN];
    for (offset, value) in [
        (0, 0.0_f64),
        (8, 0.0),
        (16, 0.210),
        (24, 0.0),
        (32, 0.0),
        (40, 1.0),
        (57, -0.0),
        (65, -0.210),
        (73, 1.0),
    ] {
        result[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    result[56] = 0x80;
    let mut reference = [0; 82];
    for (offset, value) in [
        (0, 0.0_f64),
        (8, 0.0),
        (16, 0.235),
        (24, 0.0),
        (32, 0.0),
        (40, 1.0),
        (48, 0.0),
        (56, 0.0),
        (65, 0.0),
        (73, 1.0),
    ] {
        reference[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    let mut payload = result.to_vec();
    payload.extend([0xff; 19]);
    payload.extend(reference);

    assert_eq!(
        offset_reference_plane_frame_pair(&payload, 25.0),
        Some((
            (
                Point3::new(0.0, 0.0, 210.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            ),
            (
                Point3::new(0.0, 0.0, 235.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            ),
        ))
    );
}

#[test]
fn tangent_plane_frame_is_anchored_to_its_constraint_class() {
    const CLASS: &str = "moConstraintPerpPlnTanOneCylinderRefplaneData_c";
    let root = 7;
    let mut payload = vec![0xaa; root];
    payload.extend(CLASS_MARKER);
    payload.extend((CLASS.len() as u16).to_le_bytes());
    payload.extend(CLASS.as_bytes());
    let body = payload.len();
    payload.resize(body + fixed_plane::LEN, 0);
    for (relative, value) in [
        (0, 0.0125_f64),
        (24, 1.0),
        (49, 0.0),
        (57, 0.0),
        (65, 1.0),
        (73, 0.0),
        (81, 1.0),
        (89, 0.0),
    ] {
        payload[body + relative..body + relative + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[body + 48] = 1;

    assert_eq!(
        constraint_reference_plane_frame(&payload, root, CLASS),
        Some((
            Point3::new(12.5, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ))
    );
    assert_eq!(
        constraint_reference_plane_frame(&payload, root, "moRefPlane_c"),
        None
    );
}

#[test]
fn offset_plane_face_reference_owns_a_fixed_plane_frame() {
    const CLASS: &str = "moFaceRefPlnData_c";
    let root = 11;
    let mut payload = vec![0xaa; root];
    payload.extend(CLASS_MARKER);
    payload.extend((CLASS.len() as u16).to_le_bytes());
    payload.extend(CLASS.as_bytes());
    let body = payload.len();
    payload.resize(body + fixed_plane::LEN, 0);
    for (relative, value) in [(0, 0.0025_f64), (24, 1.0), (57, 1.0), (89, 1.0)] {
        payload[body + relative..body + relative + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[body + 48] = 1;

    assert_eq!(
        constraint_reference_plane_frame(&payload, root, CLASS),
        Some((
            Point3::new(2.5, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
        ))
    );
}

#[test]
fn fixed_reference_plane_accepts_repeated_normal_axis_form() {
    const CLASS: &str = "moFixedRefPlnData_c";
    let payload_for = |first_axis: [f64; 3], second_axis: [f64; 3]| {
        let root = 7;
        let mut payload = vec![0xaa; root];
        payload.extend(CLASS_MARKER);
        payload.extend((CLASS.len() as u16).to_le_bytes());
        payload.extend(CLASS.as_bytes());
        let body = payload.len();
        payload.resize(body + fixed_plane::LEN, 0);
        for (offset, value) in [(0, 0.0025_f64), (24, 0.0), (32, 1.0), (40, 0.0)] {
            payload[body + offset..body + offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        for (axis_offset, axis) in [
            (fixed_plane::U_AXIS, first_axis),
            (fixed_plane::V_AXIS, second_axis),
        ] {
            for (index, value) in axis.into_iter().enumerate() {
                let offset = body + axis_offset + index * 8;
                payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
            }
        }
        payload[body + fixed_plane::FRAME_MARKER] = 1;
        (payload, root)
    };

    for (first_axis, second_axis, expected) in [
        (
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            Vector3::new(1.0, 0.0, 0.0),
        ),
        (
            [0.0, -1.0, 0.0],
            [0.0, 0.0, 1.0],
            Vector3::new(0.0, 0.0, 1.0),
        ),
    ] {
        let (payload, root) = payload_for(first_axis, second_axis);
        assert_eq!(
            constraint_reference_plane_frame(&payload, root, CLASS),
            Some((
                Point3::new(2.5, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
                expected,
            ))
        );
    }
}

#[test]
fn named_reference_plane_data_classes_anchor_frame_lengths() {
    let payload_for = |class: &str, frame: &[u8]| {
        let root = 7;
        let mut payload = vec![0xaa; root];
        payload.extend(CLASS_MARKER);
        payload.extend((class.len() as u16).to_le_bytes());
        payload.extend(class.as_bytes());
        payload.extend_from_slice(frame);
        (payload, root)
    };

    let mut fixed = [0; fixed_plane::LEN];
    for (offset, value) in [
        (0, 0.0125_f64),
        (24, 1.0),
        (49, 0.0),
        (57, 0.0),
        (65, 1.0),
        (73, 0.0),
        (81, 1.0),
        (89, 0.0),
    ] {
        fixed[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    fixed[48] = 1;
    for class in [
        "moConstraintCoincLineParallelPlaneRefplaneData_c",
        "moFacePtRefPlnData_c",
        "moFixedRefPlnData_c",
    ] {
        let (payload, root) = payload_for(class, &fixed);
        assert_eq!(
            constraint_reference_plane_frame(&payload, root, class),
            Some((
                Point3::new(12.5, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            )),
            "{class}"
        );
    }

    let mut matrix = [0; 121];
    for (offset, value) in [
        (0, 0.0125_f64),
        (24, 1.0),
        (49, 0.0),
        (57, 0.0),
        (65, 1.0),
        (73, 1.0),
        (81, 0.0),
        (89, 0.0),
        (97, 0.0),
        (105, 1.0),
        (113, 0.0),
    ] {
        matrix[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    matrix[48] = 1;
    let class = "moConstraintCoincLineAtAnglePlaneRefplaneData_c";
    let (payload, root) = payload_for(class, &matrix);
    assert_eq!(
        constraint_reference_plane_frame(&payload, root, class),
        Some((
            Point3::new(12.5, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
        ))
    );

    let mut minimal = [0; MINIMAL_REFERENCE_PLANE_FRAME_LEN];
    for (offset, value) in [
        (0, 0.0125_f64),
        (8, -0.002),
        (16, 0.003),
        (24, 0.0),
        (32, 0.0),
        (40, 1.0),
        (57, -0.0),
        (65, -0.003),
        (73, 1.0),
    ] {
        minimal[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    minimal[56] = 0x80;
    let (payload, root) = payload_for("moDefaultRefPlnData_c", &minimal);
    assert_eq!(
        constraint_reference_plane_frame(&payload, root, "moDefaultRefPlnData_c"),
        Some((
            Point3::new(12.5, -2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        ))
    );
}

#[test]
fn offset_plane_reference_matches_parallel_frame_at_declared_distance() {
    let reference = (
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
    );
    let offset = (
        Point3::new(0.0, 0.0, 6.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
    );
    assert!(offset_plane_reference_frame_matches(reference, offset, 6.0));
    assert!(!offset_plane_reference_frame_matches(
        reference, offset, 5.0
    ));
    assert!(!offset_plane_reference_frame_matches(
        reference,
        (Point3::new(1.0, 0.0, 6.0), offset.1, offset.2,),
        6.0,
    ));
}

#[test]
fn constraint_midplane_uses_its_normal_form_equation() {
    const CLASS: &str = "moConstraintMidPlaneRefplaneData_c";
    let mut payload = vec![0xaa; 19];
    payload.extend(CLASS_MARKER);
    payload.extend((CLASS.len() as u16).to_le_bytes());
    payload.extend(CLASS.as_bytes());
    payload.extend([0; 8]);
    payload.extend(1.0e-16f64.to_le_bytes());
    payload.extend(0.145f64.to_le_bytes());
    payload.extend(0.0f64.to_le_bytes());
    payload.extend(0.0f64.to_le_bytes());
    payload.extend(1.0f64.to_le_bytes());
    assert_eq!(
        constraint_midplane_frame(&payload),
        Some((
            Point3::new(0.0, 0.0, 145.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        ))
    );

    let normal = payload.len() - 24;
    payload[normal..normal + 8].copy_from_slice(&1.0f64.to_le_bytes());
    assert_eq!(constraint_midplane_frame(&payload), None);
}

#[test]
fn classless_reference_plane_enrichment_marks_a_constructed_midplane_axis() {
    const CLASS: &[u8] = b"moConstraintMidPlaneRefplaneData_c";
    let class_offset = 16;
    let body = class_offset + CLASS_MARKER.len() + 2 + CLASS.len();
    let mut payload = vec![0; body + 48 + 16];
    payload[class_offset..class_offset + CLASS_MARKER.len()].copy_from_slice(CLASS_MARKER);
    payload[class_offset + CLASS_MARKER.len()..class_offset + CLASS_MARKER.len() + 2]
        .copy_from_slice(&(CLASS.len() as u16).to_le_bytes());
    payload[class_offset + CLASS_MARKER.len() + 2..body].copy_from_slice(CLASS);
    for (relative, value) in [
        (8, 1.0e-16_f64),
        (16, 0.145),
        (24, 0.0),
        (32, 0.0),
        (40, 1.0),
    ] {
        payload[body + relative..body + relative + 8].copy_from_slice(&value.to_le_bytes());
    }
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
            source_id: Some("2080".into()),
            parent_source_id: None,
            ordinal: 0,
            name: "MidPlane".into(),
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
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: Vec::new(),
        names: vec![FeatureInputName {
            id: "name".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            object_id: Some(2080),
            value: "MidPlane".into(),
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
    };

    super::enrich_history_reference_planes(&mut histories, &[lane]);

    let properties = &histories[0].features[0].properties;
    assert_eq!(properties.get("Origin"), Some(&"0mm,0mm,145mm".to_string()));
    assert_eq!(properties.get("Normal"), Some(&"0,0,1".to_string()));
    assert_eq!(properties.get("UAxis"), Some(&"1,0,0".to_string()));
    assert_eq!(
        properties.get("UAxisSource"),
        Some(&"constructed-mid-plane".to_string())
    );
}

#[test]
fn explicit_plane_basis_precedes_equivalent_constraint_orientation() {
    let explicit = (
        Point3::new(12.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
    );
    let equivalent_constraint = (
        Point3::new(12.0, 4.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    );
    assert_eq!(
        reconcile_reference_plane_frame_with_source(Some(explicit), Some(equivalent_constraint))
            .map(|(frame, _)| frame),
        Some(explicit)
    );

    let conflicting_constraint = (
        Point3::new(13.0, 0.0, 0.0),
        equivalent_constraint.1,
        equivalent_constraint.2,
    );
    assert_eq!(
        reconcile_reference_plane_frame_with_source(Some(explicit), Some(conflicting_constraint))
            .map(|(frame, _)| frame),
        Some(conflicting_constraint)
    );
}

#[test]
fn midplane_constraint_marks_only_its_constructed_axis() {
    let constraint = (
        Point3::new(12.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    );
    let explicit = (
        Point3::new(12.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
    );
    assert_eq!(
        reconcile_reference_plane_frame_with_source(None, Some(constraint)),
        Some((constraint, SketchPlaneUAxisSource::ConstructedMidPlane))
    );
    assert_eq!(
        reconcile_reference_plane_frame_with_source(Some(explicit), Some(constraint)),
        Some((explicit, SketchPlaneUAxisSource::Native))
    );
}

#[test]
fn angled_reference_plane_requires_its_redundant_normal_and_basis() {
    let root = 11;
    let mut payload = vec![0; root + 121];
    let inverse_sqrt_two = std::f64::consts::FRAC_1_SQRT_2;
    for (relative, value) in [
        (0, inverse_sqrt_two),
        (8, inverse_sqrt_two),
        (17, 1.0),
        (25, 0.0),
        (33, 0.0),
        (41, 0.0),
        (49, inverse_sqrt_two),
        (57, inverse_sqrt_two),
        (65, 0.0),
        (73, -inverse_sqrt_two),
        (81, inverse_sqrt_two),
        (113, 1.0),
    ] {
        payload[root + relative..root + relative + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[root + 16] = 1;
    assert_eq!(
        angled_reference_plane_frame_candidates(&payload)[0].1,
        (
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, inverse_sqrt_two, inverse_sqrt_two),
            Vector3::new(1.0, 0.0, 0.0),
        )
    );

    payload[root + 8..root + 16].copy_from_slice(&(-inverse_sqrt_two).to_le_bytes());
    assert!(angled_reference_plane_frame_candidates(&payload).is_empty());
}

#[test]
fn angled_reference_plane_does_not_reinterpret_a_complete_fixed_frame() {
    let mut payload = vec![0; 153];
    for (offset, value) in [
        (24, 0.0_f64),
        (32, -1.0),
        (40, 0.0),
        (49, -1.0),
        (57, 0.0),
        (65, 0.0),
        (73, 0.0),
        (81, 0.0),
        (89, -1.0),
        (97, 0.0),
        (105, -1.0),
        (113, 0.0),
        (145, 1.0),
    ] {
        payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[48] = 1;
    assert!(fixed_reference_plane_frame(&payload[..97]).is_some());
    assert!(angled_reference_plane_frame_candidates(&payload).is_empty());
}

#[test]
fn matrix_reference_plane_uses_basis_columns() {
    let root = 9;
    let mut payload = vec![0; root + 121];
    let sine = 0.390_731_128_489_273_27_f64;
    let cosine = 0.920_504_853_452_440_5_f64;
    for (relative, value) in [
        (0, 0.008_400_719_262_519_38),
        (8, 0.019_790_854_349_227_484),
        (16, 0.0),
        (24, sine),
        (32, cosine),
        (40, 0.0),
        (49, cosine),
        (57, 0.0),
        (65, sine),
        (73, -sine),
        (81, 0.0),
        (89, cosine),
        (97, 0.0),
        (105, -1.0),
        (113, 0.0),
    ] {
        payload[root + relative..root + relative + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[root + 48] = 1;
    assert_eq!(
        matrix_reference_plane_frame(&payload),
        Some((
            Point3::new(
                0.008_400_719_262_519_38 * 1000.0,
                0.019_790_854_349_227_484 * 1000.0,
                0.0,
            ),
            Vector3::new(sine, cosine, 0.0),
            Vector3::new(cosine, -sine, 0.0),
        ))
    );

    payload[root + 113..root + 121].copy_from_slice(&1.0f64.to_le_bytes());
    assert_eq!(matrix_reference_plane_frame(&payload), None);
}

#[test]
fn matrix_reference_plane_owns_its_fixed_frame_prefix() {
    let root = 9;
    let mut payload = vec![0; root + matrix_plane::LEN];
    for (relative, value) in [
        (24, 1.0_f64),
        (49, 0.0),
        (57, 0.0),
        (65, 1.0),
        (73, 0.0),
        (81, 1.0),
        (89, 0.0),
        (97, -1.0),
        (105, 0.0),
        (113, 0.0),
    ] {
        payload[root + relative..root + relative + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[root + 48] = 1;

    assert_eq!(
        fixed_reference_plane_frame(&payload[root..root + fixed_plane::LEN]),
        Some((
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ))
    );
    assert_eq!(
        explicit_reference_plane_frame(&payload),
        Ok(Some((
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, -1.0),
        )))
    );
}

#[test]
fn ambiguous_reference_plane_frame_encodings_are_withheld() {
    let mut payload = vec![0; 260];
    let matrix = 3;
    for (relative, value) in [
        (0, 0.035_f64),
        (8, 0.0),
        (16, 0.0),
        (24, 1.0),
        (32, 0.0),
        (40, 0.0),
        (49, 0.0),
        (57, 0.0),
        (65, 1.0),
        (73, 0.0),
        (81, 1.0),
        (89, 0.0),
        (97, -1.0),
        (105, 0.0),
        (113, 0.0),
    ] {
        payload[matrix + relative..matrix + relative + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[matrix + 48] = 1;

    let compact = 165;
    for (relative, value) in [
        (0, 0.0_f64),
        (8, 0.0),
        (16, 0.0),
        (24, 0.0),
        (32, 0.0),
        (40, 1.0),
        (48, 0.0),
        (56, 0.0),
        (65, 0.0),
        (73, 1.0),
    ] {
        payload[compact + relative..compact + relative + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[compact + 64] = 0;
    payload[compact + 81] = 0;

    assert!(compact_reference_plane_frame(&payload).is_some());
    assert_eq!(explicit_reference_plane_frame(&payload), Err(()));
}

#[test]
fn compact_reference_plane_solves_omitted_basis_components() {
    let root = 7;
    let mut payload = vec![0xaa; root + 82];
    for (relative, value) in [
        (0, 0.001_f64),
        (8, -0.002),
        (16, 0.003),
        (24, 0.0),
        (32, 0.0),
        (40, 1.0),
        (48, 0.0),
        (56, 0.0),
        (65, 0.0),
        (73, 1.0),
    ] {
        payload[root + relative..root + relative + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[root + 64] = 0;
    payload[root + 81] = 0;
    assert_eq!(
        compact_reference_plane_frame(&payload),
        Some((
            Point3::new(1.0, -2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        ))
    );

    payload[root + 73..root + 81].copy_from_slice(&0.5f64.to_le_bytes());
    assert_eq!(compact_reference_plane_frame(&payload), None);
}

#[test]
fn compact_offset_plane_source_requires_the_reference_record() {
    let mut payload = Vec::new();
    payload.extend(3u32.to_le_bytes());
    payload.extend([
        0x02, 0x00, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x2d, 0x80, 0x2b, 0x80,
    ]);
    assert_eq!(compact_offset_plane_source(&payload), Some(3));
    payload[19] ^= 1;
    assert_eq!(compact_offset_plane_source(&payload), None);
}

#[test]
fn legacy_offset_plane_face_alias_requires_the_complete_nested_record() {
    let mut body = vec![0; 115];
    body[..2].copy_from_slice(&0x802d_u16.to_le_bytes());
    body[2..6].copy_from_slice(&2u32.to_le_bytes());
    body[45..61].fill(0xff);
    body[69..73].copy_from_slice(&2u32.to_le_bytes());
    body[73..77].copy_from_slice(&0x4c41_ac95_u32.to_le_bytes());
    body[77..83].copy_from_slice(&[0, 0, 3, 0, 0, 0]);
    body[83..87].copy_from_slice(&1u32.to_le_bytes());
    body[91..95].copy_from_slice(&175u32.to_le_bytes());
    body[99..103].copy_from_slice(&3u32.to_le_bytes());
    body[107..115].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);

    assert_eq!(legacy_offset_plane_face_alias(&body), Some((0, 175)));
    body[91..95].fill(0);
    assert_eq!(legacy_offset_plane_face_alias(&body), None);
    body[91..95].copy_from_slice(&175u32.to_le_bytes());
    body[83] = 2;
    assert_eq!(legacy_offset_plane_face_alias(&body), None);
}

#[test]
fn structured_offset_plane_source_requires_repeated_identities_and_terminator() {
    let mut payload = vec![0; 140];
    let header = 0x8323u32.to_le_bytes();
    let identity = [
        0xd7, 0x81, 0x26, 0x03, 0x1d, 0x00, 0x00, 0x00, 0x5e, 0x2c, 0xdb, 0x54,
    ];
    let link = 0x81dcu32.to_le_bytes();
    payload[..4].copy_from_slice(&4u32.to_le_bytes());
    payload[4..8].copy_from_slice(&header);
    for offset in [8, 32, 52, 76] {
        payload[offset..offset + 12].copy_from_slice(&identity);
    }
    payload[28..32].copy_from_slice(&link);
    payload[44..48].copy_from_slice(&3u32.to_le_bytes());
    payload[48..52].copy_from_slice(&header);
    for offset in [64, 88, 108] {
        payload[offset..offset + 4].copy_from_slice(&1u32.to_le_bytes());
    }
    payload[72..76].copy_from_slice(&link);
    payload[116..120].copy_from_slice(&2600u32.to_le_bytes());
    payload[132..140].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);

    assert_eq!(structured_offset_plane_sources(&payload), [3]);
    payload[80] ^= 1;
    assert!(structured_offset_plane_sources(&payload).is_empty());
}

#[test]
fn classed_offset_plane_source_requires_exact_length_delimited_type() {
    let mut payload = 4u32.to_le_bytes().to_vec();
    payload.extend(b"\xff\xff\x01\x00\x1b\x00moFromSktEnt3IntSurfIdRep_c\x00\x00");

    assert_eq!(classed_offset_plane_sources(&payload), [4]);
    payload[8] = 0;
    assert!(classed_offset_plane_sources(&payload).is_empty());
}

#[test]
fn typed_offset_plane_reference_requires_one_known_plane_target() {
    let record = |source: u32, signature: [u8; 4], selector: u32| {
        let mut bytes = Vec::new();
        bytes.extend(source.to_le_bytes());
        bytes.extend(signature);
        bytes.extend([0; 2]);
        bytes.extend(selector.to_le_bytes());
        bytes.extend(1u32.to_le_bytes());
        bytes.extend([0; 4]);
        bytes.extend(247u32.to_le_bytes());
        bytes.extend([0; 12]);
        bytes.extend([0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
        bytes
    };
    let known = HashSet::from([3, 225]);
    let principal = record(3, [0x43, 0xf6, 0x8a, 0x4d], 3);
    assert_eq!(
        offset_plane_reference_source(&principal, &known, &known, None),
        Some(3)
    );
    let feature = record(225, [0x30, 0x92, 0xab, 0x53], 0);
    assert_eq!(
        offset_plane_reference_source(&feature, &known, &known, None),
        Some(225)
    );
    assert_eq!(
        offset_plane_reference_source(&feature, &known, &known, Some(225)),
        None
    );

    let mut ambiguous = principal.clone();
    ambiguous.extend_from_slice(&feature);
    assert_eq!(
        offset_plane_reference_source(&ambiguous, &known, &known, None),
        None
    );
    let mut repeated = principal.clone();
    repeated.extend_from_slice(&principal);
    assert_eq!(
        offset_plane_reference_source(&repeated, &known, &known, None),
        Some(3)
    );
    ambiguous[38] ^= 1;
    assert_eq!(
        offset_plane_reference_source(&ambiguous, &known, &known, None),
        Some(225)
    );
    let mut malformed = record(3, [0; 4], 2);
    assert_eq!(
        offset_plane_reference_source(&malformed, &known, &known, None),
        None
    );
    malformed[4..8].copy_from_slice(&[1, 2, 3, 4]);
    malformed[10..14].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        offset_plane_reference_source(&malformed, &known, &known, None),
        Some(3)
    );
    let principal_only = HashSet::from([3]);
    assert_eq!(
        offset_plane_reference_source(&feature, &known, &principal_only, None),
        None
    );
}

#[test]
fn frame_only_offset_plane_reference_requires_one_unique_source() {
    assert_eq!(
        select_reference_plane_frame_source(["derived", "principal", "older"].into_iter(),),
        None
    );
    assert_eq!(
        select_reference_plane_frame_source(["same", "same"].into_iter()),
        Some("same".into())
    );
    assert_eq!(
        select_reference_plane_frame_source(["first", "second"].into_iter()),
        None
    );
}

#[test]
fn frame_only_offset_plane_reference_does_not_use_feature_order() {
    assert_eq!(
        select_reference_plane_frame_source(["older", "latest", "latest"].into_iter(),),
        None
    );
    assert_eq!(
        select_reference_plane_frame_source(["source", "source"].into_iter()),
        Some("source".into())
    );
    assert_eq!(
        select_reference_plane_frame_source(["first", "second"].into_iter()),
        None
    );
}

#[test]
fn offset_plane_frame_translates_its_reference_frame() {
    use cadmpeg_ir::features::Feature as NeutralFeature;

    let native = |id: &str, source: &str| Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some(source.into()),
        parent_source_id: None,
        ordinal: source.parse().expect("required invariant"),
        name: id.into(),
        kind: String::new(),
        input_class: None,
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let neutral = |id: &str, native_ref: &str, definition| NeutralFeature {
        id: FeatureId(id.into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition,
        native_ref: Some(native_ref.into()),
    };
    let features = vec![
        neutral(
            "plane",
            "plane-native",
            FeatureDefinition::DatumPrincipalPlane {
                plane: PrincipalPlane::Top,
            },
        ),
        neutral(
            "offset",
            "offset-native",
            FeatureDefinition::DatumOffsetPlane {
                reference: Some(cadmpeg_ir::features::DatumPlaneReference::Feature(
                    FeatureId("plane".into()),
                )),
                distance: Length(3.0),
            },
        ),
    ];
    let history = crate::records::FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![native("plane-native", "3"), native("offset-native", "549")],
    };

    assert_eq!(
        sketch_plane_frames(&features, &[history]).get(&549),
        Some(&super::super::curves::SketchPlaneFrame {
            origin: Point3::new(0.0, 0.0, 3.0),
            normal: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            u_axis: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            u_axis_source: SketchPlaneUAxisSource::Native,
        })
    );
}
