//! Coordinate-system frames and route admission tests.

use crate::records::{
    Feature, FeatureHistory, FeatureInputLane, FeatureInputName, FeatureSource, ObjectId,
};
use crate::resolved_features::{
    reference_geometry::{enrich_history_coordinate_systems, resolved_coordinate_system},
    NAME_MARKER,
};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn coordinate_system_error(policy: DecodePolicy) -> CodecError {
    let record = coordinate_system_record(
        "lane",
        [0.125, -0.25, 0.5],
        &[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        [0, 0, 0],
    );
    let lanes = [record.lane];
    let mut histories = [coordinate_system_history()];
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&lanes[0].native_payload, &arena, &policy).unwrap();
    super::super::enrich_history_coordinate_systems(&ctx, &mut histories, &lanes).unwrap_err()
}

#[test]
fn coordinate_system_enrichment_refuses_collection_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let error = coordinate_system_error(policy);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "collect SLDPRT coordinate system starts"));
}

#[test]
fn coordinate_system_enrichment_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain SLDPRT coordinate system origin",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            Err::<(), cadmpeg_core::CodecError>(coordinate_system_error(policy))
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "retain SLDPRT coordinate system origin"));
}

#[test]
fn coordinate_system_enrichment_refuses_work_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let error = coordinate_system_error(policy);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "scan SLDPRT coordinate system features"));
}

#[test]
fn coordinate_system_enrichment_refuses_path_nesting_limit() {
    let mut record = coordinate_system_record(
        "lane",
        [0.125, -0.25, 0.5],
        &[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        [0, 0, 0],
    );
    let prefix = record.origin;
    record.lane.native_payload[prefix + 73..prefix + 80].fill(0xff);
    record.lane.native_payload[prefix + 80..prefix + 84].copy_from_slice(&1u32.to_le_bytes());
    record.lane.native_payload[prefix + 84..prefix + 92].fill(0);
    record.lane.native_payload[prefix + 85] = 2;
    let marker = prefix + 92;
    record.lane.native_payload[marker..marker + 16]
        .copy_from_slice(&crate::resolved_features::selections::COMPACT_EDGE_VECTOR_MARKER);
    record.lane.native_payload[marker + 16..marker + 18].fill(0);
    let entry = marker + 18;
    record.lane.native_payload[entry..entry + 4].copy_from_slice(&[1, 0x80, 0, 0]);
    record.lane.native_payload[entry + 4..entry + 16]
        .copy_from_slice(&[0x38, 0x80, 0x3b, 0, 0x68, 1, 0, 0, 0xbc, 2, 0, 0]);
    record.lane.native_payload[entry + 16..entry + 20].copy_from_slice(&10u32.to_le_bytes());
    let lanes = [record.lane];
    let mut histories = [coordinate_system_history()];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&lanes[0].native_payload, &arena, &policy).unwrap();
    let error =
        super::super::enrich_history_coordinate_systems(&ctx, &mut histories, &lanes).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RecursionDepth
            && limit.operation == "decode SLDPRT sparse component path"));
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
    payload.push(u8::try_from(name.encode_utf16().count()).unwrap());
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
        payload.extend_from_slice(&[0xa0 + u8::try_from(index).unwrap(), 0x81, 0x01, 0]);
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
            native_payload: payload.into(),
            classes: Vec::new(),
            names: vec![FeatureInputName {
                id: format!("{lane_id}-name"),
                parent: lane_id.into(),
                ordinal: 0,
                offset: 0,
                object_id: ObjectId::from_value(500),
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
            source_id: FeatureSource::from_value(500),
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
fn solved_coordinate_system_projects_orthogonalized_flipped_frame() {
    let record = coordinate_system_record(
        "lane",
        [0.125, -0.25, 0.5],
        &[[1.0, 0.0, 0.0], [0.2, 0.979_795_897_113_271_2, 0.0]],
        [1, 1, 0],
    );
    let mut histories = vec![coordinate_system_history()];
    enrich_history_coordinate_systems(
        &cadmpeg_test_support::service_decode_context(),
        &mut histories,
        &[record.lane],
    )
    .unwrap();
    assert_eq!(
        histories[0].features[0].properties.get("Origin"),
        Some(&"125mm,-250mm,500mm".to_string())
    );
    assert!(matches!(
        crate::history::project::project_features(&cadmpeg_test_support::service_decode_context(), &histories).unwrap()[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DatumCoordinateSystem { frame }) if matches!(frame.origin().get(), Point3 {
                x: 125.0,
                y: -250.0,
                z: 500.0
            }) && matches!(*frame.x_axis().as_raw(), Vector3 {
                x: -1.0,
                y: 0.0,
                z: 0.0
            }) && matches!(*frame.y_axis().as_raw(), Vector3 {
                x: 0.0,
                y: -1.0,
                z: 0.0
            }) && matches!(*frame.z_axis().as_raw(), Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0
            })
    ));
}

#[test]
fn solved_coordinate_system_requires_one_exact_complete_frame() {
    let path_arena = cadmpeg_core::decode::DecodeArena::new();
    let (path_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &path_arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let record = coordinate_system_record(
        "lane",
        [0.125, -0.25, 0.5],
        &[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        [0, 0, 0],
    );
    assert_eq!(
        resolved_coordinate_system(&path_ctx, &record.lane.native_payload).unwrap(),
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
    assert!(resolved_coordinate_system(&path_ctx, &other_generation)
        .unwrap()
        .is_some());

    let mut alternate_family = record.lane.native_payload.clone();
    alternate_family[record.origin] = 0x2d;
    assert!(resolved_coordinate_system(&path_ctx, &alternate_family)
        .unwrap()
        .is_some());
    alternate_family[record.origin] = 0x2e;
    assert_eq!(
        resolved_coordinate_system(&path_ctx, &alternate_family).unwrap(),
        None
    );

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
    assert!(resolved_coordinate_system(&path_ctx, &extended_origin)
        .unwrap()
        .is_some());

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
        resolved_coordinate_system(&path_ctx, &two_point_frame).unwrap(),
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
    assert_eq!(
        resolved_coordinate_system(&path_ctx, &malformed_two_point).unwrap(),
        None
    );
    let mut malformed_two_point = two_point_frame;
    let repeated_direction = record.origin + first_point.len() + 14 + second_point.len() + 41;
    malformed_two_point[repeated_direction..repeated_direction + 8]
        .copy_from_slice(&(-1.0f64).to_le_bytes());
    assert_eq!(
        resolved_coordinate_system(&path_ctx, &malformed_two_point).unwrap(),
        None
    );

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
        component_path_origin
            .extend_from_slice(&(0x8001 + u16::try_from(index).unwrap()).to_le_bytes());
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
    assert!(
        resolved_coordinate_system(&path_ctx, &component_path_record)
            .unwrap()
            .is_some()
    );

    let path_end = 110 + 3 * 20;
    component_path_origin.splice(path_end..path_end, [0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]);
    let mut null_terminated_path = record.lane.native_payload.clone();
    null_terminated_path.splice(record.origin..record.origin + 151, component_path_origin);
    assert!(resolved_coordinate_system(&path_ctx, &null_terminated_path)
        .unwrap()
        .is_some());

    let mut malformed_path = component_path_record;
    malformed_path[record.origin + path_end] = 1;
    assert_eq!(
        resolved_coordinate_system(&path_ctx, &malformed_path).unwrap(),
        None
    );

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
        endpoint_origin.extend_from_slice(&(0x8001 + u16::try_from(index).unwrap()).to_le_bytes());
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
        resolved_coordinate_system(&path_ctx, &endpoint_frame).unwrap(),
        resolved_coordinate_system(&path_ctx, &record.lane.native_payload).unwrap()
    );
    endpoint_frame[record.origin + endpoint_trailer] = 1;
    assert_eq!(
        resolved_coordinate_system(&path_ctx, &endpoint_frame).unwrap(),
        None
    );

    let mut ordinal_frame = record.lane.native_payload.clone();
    ordinal_frame.truncate(record.origin + 151);
    ordinal_frame.extend_from_slice(&2u16.to_le_bytes());
    ordinal_frame.extend_from_slice(&1u16.to_le_bytes());
    ordinal_frame.extend_from_slice(&[0; 23]);
    ordinal_frame.extend_from_slice(&0.5f64.to_le_bytes());
    ordinal_frame.extend_from_slice(&0x8090u16.to_le_bytes());
    assert_eq!(
        resolved_coordinate_system(&path_ctx, &ordinal_frame).unwrap(),
        Some((
            Point3::new(125.0, -250.0, 500.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, -1.0),
        ))
    );
    let selector = ordinal_frame.len() - 37;
    ordinal_frame[selector + 2..selector + 4].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(
        resolved_coordinate_system(&path_ctx, &ordinal_frame).unwrap(),
        None
    );
    ordinal_frame[selector + 2..selector + 4].copy_from_slice(&1u16.to_le_bytes());
    ordinal_frame[selector + 27..selector + 35].copy_from_slice(&0.25f64.to_le_bytes());
    assert_eq!(
        resolved_coordinate_system(&path_ctx, &ordinal_frame).unwrap(),
        None
    );

    let mut malformed = record.lane.native_payload.clone();
    malformed[record.origin + 115..record.origin + 119].copy_from_slice(&9000u32.to_le_bytes());
    assert_eq!(
        resolved_coordinate_system(&path_ctx, &malformed).unwrap(),
        None
    );

    let mut malformed = record.lane.native_payload.clone();
    let duplicate_origin = malformed[record.origin..record.origin + 151].to_vec();
    malformed.splice(record.origin..record.origin, duplicate_origin);
    assert_eq!(
        resolved_coordinate_system(&path_ctx, &malformed).unwrap(),
        None
    );

    let mut malformed = record.lane.native_payload.clone();
    let extra_axis = malformed[record.axes[0]..record.axes[0] + 113].to_vec();
    malformed.splice(record.axes[0]..record.axes[0], extra_axis);
    assert_eq!(
        resolved_coordinate_system(&path_ctx, &malformed).unwrap(),
        None
    );

    let mut malformed = record.lane.native_payload.clone();
    malformed[record.axes[1] + 89..record.axes[1] + 97].copy_from_slice(&0.5f64.to_le_bytes());
    assert_eq!(
        resolved_coordinate_system(&path_ctx, &malformed).unwrap(),
        None
    );

    let mut malformed = record.lane.native_payload.clone();
    malformed[record.tail + 2] = 1;
    assert_eq!(
        resolved_coordinate_system(&path_ctx, &malformed).unwrap(),
        None
    );

    let mut malformed = record.lane.native_payload.clone();
    malformed[record.tail + 3..record.tail + 11].copy_from_slice(&0.25f64.to_le_bytes());
    assert_eq!(
        resolved_coordinate_system(&path_ctx, &malformed).unwrap(),
        None
    );
    assert_eq!(
        resolved_coordinate_system(
            &path_ctx,
            &record.lane.native_payload[..record.lane.native_payload.len() - 1]
        )
        .unwrap(),
        None
    );

    let collinear = coordinate_system_record(
        "lane",
        [0.0, 0.0, 0.0],
        &[[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0]],
        [0, 0, 0],
    );
    assert_eq!(
        resolved_coordinate_system(&path_ctx, &collinear.lane.native_payload).unwrap(),
        None
    );
    for axes in [
        &[[1.0, 0.0, 0.0]][..],
        &[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]][..],
    ] {
        let incomplete = coordinate_system_record("lane", [0.0; 3], axes, [0, 0, 0]);
        assert_eq!(
            resolved_coordinate_system(&path_ctx, &incomplete.lane.native_payload).unwrap(),
            None
        );
    }
}

#[test]
fn solved_coordinate_system_constructs_y_from_one_offset_line_axis() {
    let path_arena = cadmpeg_core::decode::DecodeArena::new();
    let (path_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &path_arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let mut record =
        coordinate_system_record("lane", [0.0, 0.0, 0.0], &[[1.0, 0.0, 0.0]], [1, 1, 0]);
    let axis = record.axes[0];
    for (component, value) in [2.0_f64, 3.0, 0.0].into_iter().enumerate() {
        let offset = axis + 40 + component * 8;
        record.lane.native_payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    assert_eq!(
        resolved_coordinate_system(&path_ctx, &record.lane.native_payload).unwrap(),
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
    assert!(
        resolved_coordinate_system(&path_ctx, &record.lane.native_payload)
            .unwrap()
            .is_some()
    );
    record.lane.native_payload[record.tail] = 1;
    assert_eq!(
        resolved_coordinate_system(&path_ctx, &record.lane.native_payload).unwrap(),
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
    enrich_history_coordinate_systems(
        &cadmpeg_test_support::service_decode_context(),
        &mut histories,
        &[first.lane, second.lane],
    )
    .unwrap();
    assert!(histories[0].features[0].properties.is_empty());
}
