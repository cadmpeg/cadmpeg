//! Tests for the `reference_geometry` module.

use super::super::curves::SketchPlaneUAxisSource;
use super::super::{CLASS_MARKER, NAME_MARKER};
use super::{
    angled_reference_plane_frame_candidates, compact_offset_plane_source,
    compact_reference_plane_frame, constraint_midplane_frame, constraint_reference_plane_frame,
    explicit_reference_axis_frame, explicit_reference_plane_frame, fixed_reference_plane_frame,
    legacy_reference_axis_triads, matrix_reference_plane_frame,
    offset_plane_reference_frame_matches, offset_reference_plane_frame_pair,
    plane_intersection_axis_frame, plane_intersection_axis_sources,
    reconcile_reference_plane_frame_with_source, reference_plane_frame_key,
    resolved_reference_point, sketch_block_identity_normalization_origin,
    sketch_block_record_origin, MINIMAL_REFERENCE_PLANE_FRAME_LEN,
};
use crate::layout::constructed_reference_plane_fixed_frame as fixed_plane;
use crate::layout::constructed_reference_plane_matrix_frame as matrix_plane;
use crate::records::FeatureSource;
use crate::records::ObjectId;
use crate::records::{
    Feature, FeatureHistory, FeatureInputClass, FeatureInputLane, FeatureInputName,
};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::BTreeMap;

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
            object_id: ObjectId::from_value(2080),
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
            source_id: FeatureSource::from_value(2080),
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

#[test]
fn solved_reference_point_layouts_project_to_a_datum_point() {
    for (layout, form) in [(243, 4), (259, 5)] {
        let lane = reference_point_lane(layout, form, [0.125, -0.25, 0.0]);
        let mut histories = vec![reference_point_history()];
        super::enrich_history_reference_points(
            &cadmpeg_test_support::service_decode_context(),
            &mut histories,
            &[lane],
        )
        .unwrap();
        assert_eq!(
            histories[0].features[0].properties.get("Position"),
            Some(&"125mm,-250mm,0mm".to_string())
        );
        assert!(matches!(
           crate::history::project::project_features(&cadmpeg_test_support::service_decode_context(), &histories).unwrap()[0].evaluation.definition(),
           FeatureDefinition::Operation(FeatureOperation::DatumPoint {
               position: geometry_1,
               ..
           })
        if matches!(geometry_1.get(), Point3 {
                   x: 125.0,
                   y: -250.0,
                   z: 0.0
               })));
    }

    let mut lanes = [
        reference_point_lane(243, 5, [0.125, -0.25, 0.0]),
        reference_point_lane(259, 5, [0.5, -0.25, 0.0]),
    ];
    lanes[1].id = "lane-2".into();
    lanes[1].names[0].parent = "lane-2".into();
    let mut histories = vec![reference_point_history()];
    super::enrich_history_reference_points(
        &cadmpeg_test_support::service_decode_context(),
        &mut histories,
        &lanes,
    )
    .unwrap();
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
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        plane_intersection_axis_sources(&ctx, &payload, &known).unwrap(),
        Some([17, 23])
    );

    payload.pop();
    assert_eq!(
        plane_intersection_axis_sources(&ctx, &payload, &known).unwrap(),
        None
    );
    let incomplete = record(17, 0xb6, 3);
    assert_eq!(
        plane_intersection_axis_sources(&ctx, &incomplete, &known).unwrap(),
        None
    );
}

#[test]
fn legacy_reference_axis_triad_requires_consecutive_native_records() {
    let feature = |ordinal: u32, source: u32, class: &str| Feature {
        id: format!("feature-{ordinal}"),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: FeatureSource::from_value(source),
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
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    assert_eq!(
        legacy_reference_axis_triads(&ctx, &features).unwrap(),
        vec![super::ReferenceAxisTriad(
            [3, 4, 5],
            [[40, 41], [40, 42], [42, 41]]
        )]
    );

    features.insert(3, feature(99, 4, "moRefPlane_c"));
    assert_eq!(
        legacy_reference_axis_triads(&ctx, &features).unwrap(),
        vec![super::ReferenceAxisTriad(
            [4, 5, 6],
            [[40, 41], [40, 42], [42, 41]]
        )]
    );

    features[5].source_id = FeatureSource::from_value(99);
    assert!(legacy_reference_axis_triads(&ctx, &features)
        .unwrap()
        .is_empty());
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
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        explicit_reference_axis_frame(&ctx, &payload).unwrap(),
        Some((Point3::new(250.0, 0.0, 100.0), Vector3::new(0.0, 1.0, 0.0),))
    );

    record[24..32].copy_from_slice(&0.5_f64.to_le_bytes());
    assert_eq!(explicit_reference_axis_frame(&ctx, &record).unwrap(), None);
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
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&payload, &arena, &policy).unwrap();
    assert_eq!(explicit_reference_axis_frame(&ctx, &payload).unwrap(), None);
}

#[test]
fn reference_axis_enrichment_refuses_collection_limit() {
    let feature = Feature {
        id: "axis".into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: FeatureSource::from_value(2080),
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
    };
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![feature],
    };
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::enrich_history_reference_axes(&ctx, &mut [history], &[]).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && limit.operation == "index SLDPRT reference axis sources")
    );
}

#[test]
fn reference_axis_enrichment_refuses_work_limit() {
    let feature = Feature {
        id: "axis".into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: FeatureSource::from_value(2080),
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
    };
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![feature],
    };
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::enrich_history_reference_axes(&ctx, &mut [history], &[]).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && limit.operation == "scan SLDPRT reference axis triad candidates")
    );
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
            source_id: FeatureSource::from_value(2080),
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
        }],
        names: vec![FeatureInputName {
            id: "name".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            object_id: ObjectId::from_value(2080),
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

    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    super::enrich_history_reference_axes(&ctx, &mut histories, &[lane]).unwrap();

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
        offset_reference_plane_frame_pair(
            &cadmpeg_test_support::service_decode_context(),
            &payload,
            cadmpeg_ir::scalar::Length::new(37.0).unwrap()
        )
        .unwrap(),
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
    assert!(offset_reference_plane_frame_pair(
        &cadmpeg_test_support::service_decode_context(),
        &payload,
        cadmpeg_ir::scalar::Length::new(37.0).unwrap()
    )
    .unwrap()
    .is_some());
    assert_eq!(
        offset_reference_plane_frame_pair(
            &cadmpeg_test_support::service_decode_context(),
            &payload,
            cadmpeg_ir::scalar::Length::new(38.0).unwrap()
        )
        .unwrap(),
        None
    );

    let mut antiparallel = frame(-37.0).to_vec();
    antiparallel[24..32].copy_from_slice(&(-1.0_f64).to_le_bytes());
    antiparallel.extend([0; 13]);
    antiparallel.extend(frame(0.0));
    assert!(offset_reference_plane_frame_pair(
        &cadmpeg_test_support::service_decode_context(),
        &antiparallel,
        cadmpeg_ir::scalar::Length::new(37.0).unwrap()
    )
    .unwrap()
    .is_some());
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
        offset_reference_plane_frame_pair(
            &cadmpeg_test_support::service_decode_context(),
            &payload,
            cadmpeg_ir::scalar::Length::new(37.0).unwrap()
        )
        .unwrap(),
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
        offset_reference_plane_frame_pair(
            &cadmpeg_test_support::service_decode_context(),
            &payload,
            cadmpeg_ir::scalar::Length::new(25.0).unwrap()
        )
        .unwrap(),
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
fn constraint_midplane_uses_its_normal_form_with_opaque_prefix() {
    const CLASS: &str = "moConstraintMidPlaneRefplaneData_c";
    let mut payload = vec![0xaa; 19];
    payload.extend(CLASS_MARKER);
    payload.extend((CLASS.len() as u16).to_le_bytes());
    payload.extend(CLASS.as_bytes());
    payload.extend([1, 2, 3, 4, 5, 6, 7, 8]);
    payload.extend(1.0e-16f64.to_le_bytes());
    payload.extend(0.145f64.to_le_bytes());
    payload.extend(0.0f64.to_le_bytes());
    payload.extend(0.0f64.to_le_bytes());
    payload.extend(1.0f64.to_le_bytes());
    assert_eq!(
        constraint_midplane_frame(&cadmpeg_test_support::service_decode_context(), &payload)
            .unwrap(),
        Some((
            Point3::new(0.0, 0.0, 145.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        ))
    );

    let normal = payload.len() - 24;
    payload[normal..normal + 8].copy_from_slice(&1.0f64.to_le_bytes());
    assert_eq!(
        constraint_midplane_frame(&cadmpeg_test_support::service_decode_context(), &payload)
            .unwrap(),
        None
    );
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
            source_id: FeatureSource::from_value(2080),
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
            object_id: ObjectId::from_value(2080),
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

    super::enrich_history_reference_planes(
        &cadmpeg_test_support::service_decode_context(),
        &mut histories,
        &[lane],
    )
    .unwrap();

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
        angled_reference_plane_frame_candidates(&payload)
            .next()
            .unwrap()
            .1,
        (
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, inverse_sqrt_two, inverse_sqrt_two),
            Vector3::new(1.0, 0.0, 0.0),
        )
    );

    payload[root + 8..root + 16].copy_from_slice(&(-inverse_sqrt_two).to_le_bytes());
    assert!(angled_reference_plane_frame_candidates(&payload)
        .next()
        .is_none());
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
    assert!(angled_reference_plane_frame_candidates(&payload)
        .next()
        .is_none());
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
mod coordinate_systems;
mod offset_planes;
mod plane_frames;
mod reference_planes;
mod reference_points;
mod sketch_blocks;
