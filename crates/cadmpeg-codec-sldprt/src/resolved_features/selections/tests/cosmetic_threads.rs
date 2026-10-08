//! Cosmetic-thread selection and child-reference tests.

use super::super::super::CLASS_MARKER;
use super::super::selection_vector_tail;
use crate::records::{
    Feature, FeatureInputClass, FeatureInputLane, FeatureInputName, FeatureInputScalar,
    FeatureInputScalarRole, FeatureSource, ObjectId,
};
use crate::resolved_features::selections::{
    cosmetic_thread_component_references, cosmetic_thread_cylinder_marker_reference,
    cosmetic_thread_cylinder_reference_at, cosmetic_thread_cylinder_references,
    COMPACT_EDGE_VECTOR_MARKER,
};
use std::collections::{BTreeMap, HashSet};

#[test]
fn cosmetic_thread_cylinder_reference_uses_the_typed_child_layout() {
    let reference_arena = cadmpeg_core::decode::DecodeArena::new();
    let (reference_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &reference_arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let body_offset = 30;
    let marker = body_offset + 94;
    let mut payload = vec![0; marker - 12];
    payload[body_offset..body_offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
    payload[body_offset + 2..body_offset + 4].copy_from_slice(&0x802b_u16.to_le_bytes());
    payload[body_offset + 4..body_offset + 8].copy_from_slice(&2u32.to_le_bytes());
    let actual_marker = selection_vector_tail(&mut payload, &[3]);
    assert_eq!(actual_marker, marker);
    let (actual_marker, components) =
        cosmetic_thread_cylinder_reference_at(&reference_ctx, &payload, body_offset)
            .unwrap()
            .expect("required invariant");
    assert_eq!(actual_marker, marker);
    assert_eq!(
        components.last().expect("required invariant").local_id,
        Some(3)
    );

    let compact_marker = body_offset + 66;
    let mut compact = vec![0; compact_marker - 12];
    compact[body_offset..body_offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
    compact[body_offset + 2..body_offset + 4].copy_from_slice(&0x802b_u16.to_le_bytes());
    compact[body_offset + 4..body_offset + 8].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(selection_vector_tail(&mut compact, &[5]), compact_marker);
    let (actual_marker, components) =
        cosmetic_thread_cylinder_reference_at(&reference_ctx, &compact, body_offset)
            .unwrap()
            .expect("required invariant");
    assert_eq!(actual_marker, compact_marker);
    assert_eq!(
        components.last().expect("required invariant").local_id,
        Some(5)
    );

    let selected_marker = body_offset + 70;
    let mut selected = vec![0; selected_marker - 12];
    selected[body_offset..body_offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
    selected[body_offset + 2..body_offset + 4].copy_from_slice(&0x802b_u16.to_le_bytes());
    selected[body_offset + 4..body_offset + 8].copy_from_slice(&2u32.to_le_bytes());
    selected[body_offset + 8] = 0x40;
    assert_eq!(selection_vector_tail(&mut selected, &[7]), selected_marker);
    let (actual_marker, components) =
        cosmetic_thread_cylinder_reference_at(&reference_ctx, &selected, body_offset)
            .unwrap()
            .expect("required invariant");
    assert_eq!(actual_marker, selected_marker);
    assert_eq!(
        components.last().expect("required invariant").local_id,
        Some(7)
    );

    let extended_marker = body_offset + 106;
    let mut extended = vec![0; extended_marker - 12];
    extended[body_offset..body_offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
    extended[body_offset + 2..body_offset + 4].copy_from_slice(&0x802b_u16.to_le_bytes());
    extended[body_offset + 4..body_offset + 8].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(selection_vector_tail(&mut extended, &[9]), extended_marker);
    let (actual_marker, components) =
        cosmetic_thread_cylinder_reference_at(&reference_ctx, &extended, body_offset)
            .unwrap()
            .expect("required invariant");
    assert_eq!(actual_marker, extended_marker);
    assert_eq!(
        components.last().expect("required invariant").local_id,
        Some(9)
    );

    let compact_legacy_marker = body_offset + 46;
    let mut compact_legacy = vec![0; compact_legacy_marker - 12];
    compact_legacy[body_offset..body_offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
    compact_legacy[body_offset + 2..body_offset + 4].copy_from_slice(&0x802b_u16.to_le_bytes());
    compact_legacy[body_offset + 4..body_offset + 8].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        selection_vector_tail(&mut compact_legacy, &[10]),
        compact_legacy_marker
    );
    let (actual_marker, components) =
        cosmetic_thread_cylinder_reference_at(&reference_ctx, &compact_legacy, body_offset)
            .unwrap()
            .expect("required invariant");
    assert_eq!(actual_marker, compact_legacy_marker);
    assert_eq!(
        components.last().expect("required invariant").local_id,
        Some(10)
    );

    let legacy_marker = body_offset + 102;
    let mut legacy = vec![0; legacy_marker - 12];
    legacy[body_offset..body_offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
    legacy[body_offset + 2..body_offset + 4].copy_from_slice(&0x802b_u16.to_le_bytes());
    legacy[body_offset + 4..body_offset + 8].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(selection_vector_tail(&mut legacy, &[11]), legacy_marker);
    let (actual_marker, components) =
        cosmetic_thread_cylinder_reference_at(&reference_ctx, &legacy, body_offset)
            .unwrap()
            .expect("required invariant");
    assert_eq!(actual_marker, legacy_marker);
    assert_eq!(
        components.last().expect("required invariant").local_id,
        Some(11)
    );

    let extended_marker = body_offset + 110;
    let mut extended = vec![0; extended_marker - 12];
    extended[body_offset..body_offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
    extended[body_offset + 2..body_offset + 4].copy_from_slice(&0x802b_u16.to_le_bytes());
    extended[body_offset + 4..body_offset + 8].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(selection_vector_tail(&mut extended, &[12]), extended_marker);
    let (actual_marker, components) =
        cosmetic_thread_cylinder_reference_at(&reference_ctx, &extended, body_offset)
            .unwrap()
            .expect("required invariant");
    assert_eq!(actual_marker, extended_marker);
    assert_eq!(
        components.last().expect("required invariant").local_id,
        Some(12)
    );

    for (relative, local_id) in [(62, 13), (90, 14)] {
        let marker = body_offset + relative;
        let mut payload = vec![0; marker - 12];
        payload[body_offset..body_offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
        payload[body_offset + 2..body_offset + 4].copy_from_slice(&0x802b_u16.to_le_bytes());
        payload[body_offset + 4..body_offset + 8].copy_from_slice(&2u32.to_le_bytes());
        assert_eq!(selection_vector_tail(&mut payload, &[local_id]), marker);
        let (actual_marker, components) =
            cosmetic_thread_cylinder_reference_at(&reference_ctx, &payload, body_offset)
                .unwrap()
                .expect("required invariant");
        assert_eq!(actual_marker, marker);
        assert_eq!(
            components.last().expect("required invariant").local_id,
            Some(local_id)
        );
    }

    assert_eq!(
        cosmetic_thread_cylinder_reference_at(&reference_ctx, &payload, body_offset + 1).unwrap(),
        None
    );

    let mut payload = vec![0; marker - 12];
    payload[body_offset..body_offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
    payload[body_offset + 2..body_offset + 4].copy_from_slice(&0x802b_u16.to_le_bytes());
    payload[body_offset + 4..body_offset + 8].copy_from_slice(&2u32.to_le_bytes());
    payload.extend(3u32.to_le_bytes());
    payload.extend([0, 2, 0, 0]);
    payload.extend([0; 4]);
    payload.extend(COMPACT_EDGE_VECTOR_MARKER);
    payload.extend([0; 2]);
    for (instance, signature, local_id, gap) in [
        (0x8032_u16, [1; 12], 3_u32, Some(6_u32)),
        (0x803e, [2; 12], 7, None),
    ] {
        payload.extend(instance.to_le_bytes());
        payload.extend([0; 2]);
        payload.extend(signature);
        payload.extend(local_id.to_le_bytes());
        if let Some(gap) = gap {
            payload.extend(gap.to_le_bytes());
        }
    }
    let (_, components) =
        cosmetic_thread_cylinder_reference_at(&reference_ctx, &payload, body_offset)
            .unwrap()
            .expect("required invariant");
    assert_eq!(
        components
            .iter()
            .map(|component| component.local_id)
            .collect::<Vec<_>>(),
        [Some(3), Some(7)]
    );
}

#[test]
fn cosmetic_thread_retains_unique_cylinder_marker_without_component_path() {
    let body_offset = 30;
    let marker = body_offset + 94;
    let mut payload = vec![0; marker - 12];
    payload[body_offset..body_offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
    payload[body_offset + 2..body_offset + 4].copy_from_slice(&0x802b_u16.to_le_bytes());
    payload[body_offset + 4..body_offset + 8].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(selection_vector_tail(&mut payload, &[3]), marker);
    payload.truncate(marker + 18);
    let feature = Feature {
        id: "thread".into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: FeatureSource::from_value(20),
        ordinal: 0,
        name: "thread".into(),
        kind: "Feature".into(),
        input_class: Some("moCosmeticThread_c".into()),
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: Vec::new(),
        names: Vec::new(),
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
        &lane.native_payload,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("test context");
    crate::test_support::work_refusal_at(
        "deduplicate SLDPRT cosmetic thread cylinder markers",
        |ctx| {
            cosmetic_thread_cylinder_marker_reference(
                ctx,
                &feature,
                &lane,
                0,
                lane.native_payload.len(),
                &HashSet::from([0x802f]),
                &crate::resolved_features::selections::diameter_index::CosmeticDiameterIndex::new(
                    ctx, &lane,
                )
                .unwrap(),
            )
        },
    );
    assert_eq!(
        cosmetic_thread_cylinder_marker_reference(
            &ctx,
            &feature,
            &lane,
            0,
            lane.native_payload.len(),
            &HashSet::from([0x802f]),
            &crate::resolved_features::selections::diameter_index::CosmeticDiameterIndex::new(
                &ctx, &lane
            )
            .unwrap()
        )
        .expect("charged cylinder marker scan"),
        vec![crate::resolved_features::selections::CylinderMarkerReference(marker, None)]
    );
}

#[test]
fn cosmetic_thread_cylinder_reference_follows_its_owned_diameter_child() {
    let references_arena = cadmpeg_core::decode::DecodeArena::new();
    let (references_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &references_arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let body_offset = 220;
    let marker = body_offset + 94;
    let mut payload = vec![0; marker - 12];
    payload[body_offset..body_offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
    payload[body_offset + 2..body_offset + 4].copy_from_slice(&0x802d_u16.to_le_bytes());
    payload[body_offset + 4..body_offset + 8].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(selection_vector_tail(&mut payload, &[3]), marker);
    payload.resize(500, 0);

    let feature = Feature {
        id: "thread".into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: FeatureSource::from_value(53),
        ordinal: 0,
        name: "Thread".into(),
        kind: "Feature".into(),
        input_class: Some("moCosmeticThread_c".into()),
        suppressed: false,
        parameters: BTreeMap::from([(cadmpeg_core::nonblank_literal!("D2"), "<MOD-DIAM>8".into())]),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let diameter = FeatureInputScalar {
        id: "diameter".into(),
        parent: "lane".into(),
        feature_ref: Some("other-feature".into()),
        ordinal: 0,
        offset: 150,
        object_id: 52,
        name: "diameter-name".into(),
        value: cadmpeg_ir::scalar::FiniteReal::new(0.008).expect("finite test scalar"),
        role: FeatureInputScalarRole::Native,

        operands: Vec::new(),
    };
    let mut lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: Vec::new(),
        names: vec![
            FeatureInputName {
                id: "diameter-name".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 120,
                object_id: Some(ObjectId::Absent),
                value: "D2".into(),
            },
            FeatureInputName {
                id: "next-feature".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: 400,
                object_id: ObjectId::from_value(54),
                value: "Next".into(),
            },
        ],
        scalars: vec![diameter],
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    assert_eq!(
        crate::resolved_features::selections::diameter_index::CosmeticDiameterIndex::new(
            &references_ctx,
            &lane
        )
        .unwrap()
        .tail(&references_ctx, &feature)
        .unwrap(),
        Some(158..400)
    );
    crate::test_support::work_refusal_at(
        "deduplicate SLDPRT cosmetic thread cylinder offsets",
        |ctx| {
            cosmetic_thread_cylinder_references(
                ctx,
                &feature,
                &lane,
                20,
                100,
                &HashSet::from([0x802f]),
                &crate::resolved_features::selections::diameter_index::CosmeticDiameterIndex::new(
                    ctx, &lane,
                )
                .unwrap(),
            )
        },
    );
    let references = cosmetic_thread_cylinder_references(
        &references_ctx,
        &feature,
        &lane,
        20,
        100,
        &HashSet::from([0x802f]),
        &crate::resolved_features::selections::diameter_index::CosmeticDiameterIndex::new(
            &references_ctx,
            &lane,
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        references
            .iter()
            .map(|(offset, components)| (*offset, components[0].local_id))
            .collect::<Vec<_>>(),
        [(marker, Some(3))]
    );

    lane.scalars.push(FeatureInputScalar {
        id: "next-scalar".into(),
        parent: "lane".into(),
        feature_ref: None,
        ordinal: 1,
        offset: 200,
        object_id: 54,
        name: "next-feature".into(),
        value: cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite test scalar"),
        role: FeatureInputScalarRole::Native,

        operands: Vec::new(),
    });
    assert!(cosmetic_thread_cylinder_references(
        &references_ctx,
        &feature,
        &lane,
        20,
        100,
        &HashSet::from([0x802f]),
        &crate::resolved_features::selections::diameter_index::CosmeticDiameterIndex::new(
            &references_ctx,
            &lane
        )
        .unwrap()
    )
    .unwrap()
    .is_empty());
}

#[test]
fn cosmetic_thread_reads_a_direct_component_edge_reference() {
    let references_arena = cadmpeg_core::decode::DecodeArena::new();
    let (references_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &references_arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let class_name = "moCompEdge_c";
    let class_offset = 40;
    let body_offset = class_offset + 6 + class_name.len();
    let marker = body_offset + 36;
    let mut payload = vec![0; marker + 18];
    payload[class_offset..class_offset + 4].copy_from_slice(CLASS_MARKER);
    payload[class_offset + 4..class_offset + 6].copy_from_slice(
        &u16::try_from(class_name.len())
            .expect("length fits u16")
            .to_le_bytes(),
    );
    payload[class_offset + 6..body_offset].copy_from_slice(class_name.as_bytes());
    payload[marker - 12..marker - 8].copy_from_slice(&2u32.to_le_bytes());
    payload[marker - 8..marker - 4].copy_from_slice(&[0, 2, 0, 0]);
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    payload[marker + 16..marker + 18].copy_from_slice(&[0, 0]);
    let signature = [0x00, 0x81, 0x03, 0x01, 42, 0, 0, 0, 0x63, 0x18, 0x58, 0x69];
    let first = marker + 18;
    payload.resize(first + 48, 0);
    payload[first..first + 4].copy_from_slice(&[0x3d, 0x80, 0, 0]);
    payload[first + 4..first + 16].copy_from_slice(&signature);
    payload[first + 16..first + 20].copy_from_slice(&2u32.to_le_bytes());
    let second = first + 28;
    payload[second..second + 4].copy_from_slice(&[0x4a, 0x80, 0, 0]);
    payload[second + 4..second + 16].copy_from_slice(&signature);
    payload[second + 16..second + 20].copy_from_slice(&3u32.to_le_bytes());

    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: vec![FeatureInputClass {
            id: "component-edge".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: cadmpeg_core::decode::u64_from_index(class_offset),
            name: class_name.into(),
        }],
        names: Vec::new(),
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

    let references =
        cosmetic_thread_component_references(&references_ctx, &lane, 0, lane.native_payload.len())
            .unwrap();
    assert_eq!(references.len(), 1);
    assert_eq!(references[0].0, marker);
    assert_eq!(
        references[0]
            .1
            .iter()
            .map(|component| component.local_id)
            .collect::<Vec<_>>(),
        [Some(2), Some(3)]
    );
}

#[test]
fn cosmetic_thread_reads_component_edge_reference_through_edge_ref_child() {
    let references_arena = cadmpeg_core::decode::DecodeArena::new();
    let (references_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &references_arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let component_edge_name = "moCompEdge_c";
    let edge_ref_name = "moEdgeRef_c";
    let component_edge_offset = 40;
    let component_edge_body = component_edge_offset + 6 + component_edge_name.len();
    let edge_ref_offset = component_edge_body + 64;
    let edge_ref_body = edge_ref_offset + 6 + edge_ref_name.len();
    let mut payload = vec![0; edge_ref_body];
    payload[component_edge_offset..component_edge_offset + 4].copy_from_slice(CLASS_MARKER);
    payload[component_edge_offset + 4..component_edge_offset + 6].copy_from_slice(
        &u16::try_from(component_edge_name.len())
            .expect("length fits u16")
            .to_le_bytes(),
    );
    payload[component_edge_offset + 6..component_edge_body]
        .copy_from_slice(component_edge_name.as_bytes());
    payload[component_edge_body..component_edge_body + 9]
        .copy_from_slice(&[0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0]);
    payload[component_edge_body + 9..component_edge_body + 13]
        .copy_from_slice(&102u32.to_le_bytes());
    payload[component_edge_body + 13..component_edge_body + 17]
        .copy_from_slice(&102u32.to_le_bytes());
    payload[edge_ref_offset..edge_ref_offset + 4].copy_from_slice(CLASS_MARKER);
    payload[edge_ref_offset + 4..edge_ref_offset + 6].copy_from_slice(
        &u16::try_from(edge_ref_name.len())
            .expect("length fits u16")
            .to_le_bytes(),
    );
    payload[edge_ref_offset + 6..edge_ref_body].copy_from_slice(edge_ref_name.as_bytes());
    payload.extend(4u32.to_le_bytes());
    payload.extend([0, 2, 0, 0]);
    payload.extend([0; 4]);
    let marker = payload.len();
    payload.extend(COMPACT_EDGE_VECTOR_MARKER);
    payload.extend([0; 2]);
    let signature = [0x35, 0x80, 0x38, 0, 0x1c, 0, 0, 0, 0x3a, 0x44, 0x97, 0x61];
    for local_id in [3u32, 4, 4, 4] {
        payload.extend(0x803e_u16.to_le_bytes());
        payload.extend([0; 2]);
        payload.extend(signature);
        payload.extend(local_id.to_le_bytes());
    }

    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: vec![
            FeatureInputClass {
                id: "component-edge".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: cadmpeg_core::decode::u64_from_index(component_edge_offset),
                name: component_edge_name.into(),
            },
            FeatureInputClass {
                id: "edge-ref".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: cadmpeg_core::decode::u64_from_index(edge_ref_offset),
                name: edge_ref_name.into(),
            },
        ],
        names: Vec::new(),
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

    let references =
        cosmetic_thread_component_references(&references_ctx, &lane, 0, lane.native_payload.len())
            .unwrap();
    assert_eq!(references.len(), 1);
    assert_eq!(references[0].0, marker);
    assert_eq!(
        references[0]
            .1
            .iter()
            .map(|component| component.local_id)
            .collect::<Vec<_>>(),
        [Some(3), Some(4), Some(4), Some(4)]
    );
}

#[test]
fn cosmetic_thread_reads_repeated_component_edge_reference_through_edge_ref_child() {
    let references_arena = cadmpeg_core::decode::DecodeArena::new();
    let (references_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &references_arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let component_token_offset = 40;
    let component_body = component_token_offset + 2;
    let edge_ref_token_offset = component_body + 70;
    let edge_ref_body = edge_ref_token_offset + 2;
    let mut payload = vec![0; edge_ref_body];
    payload[component_token_offset..component_token_offset + 2]
        .copy_from_slice(&0x82e6_u16.to_le_bytes());
    payload[component_body..component_body + 9]
        .copy_from_slice(&[0x37, 0x81, 0x02, 0, 0, 0, 0, 0, 0]);
    payload[component_body + 9..component_body + 13].copy_from_slice(&103u32.to_le_bytes());
    payload[component_body + 13..component_body + 17].copy_from_slice(&103u32.to_le_bytes());
    payload[edge_ref_token_offset..edge_ref_token_offset + 2]
        .copy_from_slice(&0x82e9_u16.to_le_bytes());
    payload.resize(edge_ref_body + 8, 0);
    payload[edge_ref_body..edge_ref_body + 8].copy_from_slice(&[1, 0, 0, 0, 0, 0, 0, 0]);
    payload.extend(4u32.to_le_bytes());
    payload.extend([0, 2, 0, 0]);
    payload.extend([0; 4]);
    let marker = payload.len();
    payload.extend(COMPACT_EDGE_VECTOR_MARKER);
    payload.extend([0; 2]);
    let signature = [0x35, 0x80, 0x38, 0, 0x1c, 0, 0, 0, 0x3a, 0x44, 0x97, 0x61];
    for local_id in [3u32, 4, 4, 4] {
        payload.extend(0x803e_u16.to_le_bytes());
        payload.extend([0; 2]);
        payload.extend(signature);
        payload.extend(local_id.to_le_bytes());
    }

    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: Vec::new(),
        names: Vec::new(),
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

    let references =
        cosmetic_thread_component_references(&references_ctx, &lane, 0, lane.native_payload.len())
            .unwrap();
    assert_eq!(references.len(), 1);
    assert_eq!(references[0].0, marker);
    assert_eq!(
        references[0]
            .1
            .iter()
            .map(|component| component.local_id)
            .collect::<Vec<_>>(),
        [Some(3), Some(4), Some(4), Some(4)]
    );
}

#[test]
fn cosmetic_cylinder_scan_visits_overlapping_and_touching_ranges_once() {
    let mut payload = [0; 32];
    for offset in [3, 18, 28] {
        payload[offset..offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
    }
    let ctx = cadmpeg_test_support::service_decode_context();
    let tokens = HashSet::from([0x802f]);
    for (object, tail, expected) in [
        (0..20, Some(10..30), vec![3, 18, 28]),
        (0..18, Some(18..30), vec![3, 18, 28]),
        (22..30, Some(0..20), vec![28, 3, 18]),
        (5..5, Some(10..30), vec![18, 28]),
        (0..20, None, vec![3, 18]),
    ] {
        let (offsets, _storage) = super::super::cosmetic_thread_cylinder_offsets(
            &ctx,
            &payload,
            object,
            tail,
            &tokens,
            Some,
            "scan synthetic cosmetic cylinders",
        )
        .unwrap();
        assert_eq!(offsets, expected);
    }
    crate::test_support::work_refusal_at("scan synthetic cosmetic cylinders", |ctx| {
        super::super::cosmetic_thread_cylinder_offsets(
            ctx,
            &payload,
            0..20,
            Some(10..30),
            &tokens,
            Some,
            "scan synthetic cosmetic cylinders",
        )
        .map(|(offsets, _storage)| offsets)
    });
}
