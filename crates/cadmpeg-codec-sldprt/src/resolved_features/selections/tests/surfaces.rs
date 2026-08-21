//! Surface-selection, thread, and component-path tests.

use super::super::super::CLASS_MARKER;
use super::super::selection_vector_tail;
use super::super::*;
use crate::classification::FeatureClass;
use crate::records::{
    Feature, FeatureHistory, FeatureInputClass, FeatureInputClassRole, FeatureInputLane,
    FeatureInputName, FeatureInputScalar, FeatureInputScalarRole,
};
use std::collections::{BTreeMap, HashSet};

#[test]
fn compact_edge_selection_accepts_counted_u16_ids() {
    let marker = 12;
    let mut payload = vec![0; 80];
    payload[..4].copy_from_slice(&3u32.to_le_bytes());
    payload[4..8].copy_from_slice(&[0, 2, 0, 0]);
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    let ids = marker + 18;
    payload[ids..ids + 6].copy_from_slice(&[4, 0, 8, 0, 12, 0]);
    payload[ids + 22..ids + 25].copy_from_slice(&[0xff, 0xfe, 0xff]);
    assert_eq!(
        compact_edge_selection_at(&payload, marker),
        Some(vec![4, 8, 12])
    );
    assert_eq!(compact_edge_component_path_at(&payload, marker), None);
}

#[test]
fn compact_surface_selection_ends_with_its_entry_signature() {
    let mut payload = Vec::new();
    payload.extend(6u32.to_le_bytes());
    payload.extend([0x04, 0x02, 0, 0]);
    payload.extend(0x1234u32.to_le_bytes());
    payload.extend(COMPACT_EDGE_VECTOR_MARKER);
    payload.extend([0, 0]);
    let signature = [0x34, 0x80, 0x37, 0, 0x89, 0, 0, 0, 0xe2, 0x56, 0xdf, 0x5e];
    for (index, id) in [2u32, 1, 11, 14, 15, 16, 17].into_iter().enumerate() {
        payload.extend((0x8c20u32 + index as u32).to_le_bytes());
        payload.extend(signature);
        payload.extend(id.to_le_bytes());
        if index == 0 {
            payload.extend(1u32.to_le_bytes());
        }
    }
    payload.extend([0; 24]);
    let components = compact_surface_selection_at(&payload, 12).expect("required invariant");
    assert_eq!(
        components
            .iter()
            .map(|component| (
                component.instance,
                component.type_signature,
                component.local_id
            ))
            .collect::<Vec<_>>(),
        vec![
            (Some(0x8c20), signature, Some(2)),
            (Some(0x8c21), signature, Some(1)),
            (Some(0x8c22), signature, Some(11)),
            (Some(0x8c23), signature, Some(14)),
            (Some(0x8c24), signature, Some(15)),
            (Some(0x8c25), signature, Some(16)),
            (Some(0x8c26), signature, Some(17))
        ]
    );
    payload[12 + 18 + 24 + 4] ^= 1;
    assert_eq!(
        compact_surface_selection_at(&payload, 12)
            .expect("required invariant")
            .iter()
            .map(|component| component.local_id)
            .collect::<Vec<_>>(),
        vec![Some(2)]
    );
    payload[4] = 0x06;
    assert_eq!(
        compact_surface_selection_at(&payload, 12)
            .expect("nonzero selector subtype")
            .first()
            .and_then(|component| component.local_id),
        Some(2)
    );
    payload[4] = 0x7f;
    assert_eq!(
        compact_surface_selection_at(&payload, 12)
            .expect("lane-local selector subtype")
            .first()
            .and_then(|component| component.local_id),
        Some(2)
    );
}

#[test]
fn operation_surface_selection_finds_marker_inside_class_body() {
    let class_name = "moCompSurfaceBody_c";
    let class_body = 6 + class_name.len();
    let marker = class_body + 43;
    let entry = marker + 18;
    let signature = [
        0x23, 0x86, 0x25, 0x06, 0x02, 0x02, 0, 0, 0xc3, 0xea, 0xde, 0x51,
    ];
    let mut payload = vec![0; entry + 20];
    payload[..4].copy_from_slice(CLASS_MARKER);
    payload[4..6].copy_from_slice(&(class_name.len() as u16).to_le_bytes());
    payload[6..class_body].copy_from_slice(class_name.as_bytes());
    payload[class_body..class_body + 2].copy_from_slice(&0x860eu16.to_le_bytes());
    payload[marker - 12..marker - 8].copy_from_slice(&6u32.to_le_bytes());
    payload[marker - 8..marker - 4].copy_from_slice(&[0x04, 0x02, 0, 0]);
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    payload[entry..entry + 2].copy_from_slice(&0x8781u16.to_le_bytes());
    payload[entry + 4..entry + 16].copy_from_slice(&signature);
    payload[entry + 16..entry + 20].copy_from_slice(&6u32.to_le_bytes());
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload.clone(),
        classes: vec![FeatureInputClass {
            id: "class".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            name: class_name.into(),
            role: FeatureInputClassRole::Reference,
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

    let selections = operation_surface_selection_candidates(
        FeatureClass::TrimSurface,
        &lane,
        0,
        payload.len(),
        None,
    );

    assert_eq!(selections.len(), 1);
    assert_eq!(selections[0].0, marker);
    assert_eq!(selections[0].1[0].local_id, Some(6));
}

#[test]
fn cosmetic_thread_cylinder_reference_uses_the_typed_child_layout() {
    let body_offset = 30;
    let marker = body_offset + 94;
    let mut payload = vec![0; marker - 12];
    payload[body_offset..body_offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
    payload[body_offset + 2..body_offset + 4].copy_from_slice(&0x802b_u16.to_le_bytes());
    payload[body_offset + 4..body_offset + 8].copy_from_slice(&2u32.to_le_bytes());
    let actual_marker = selection_vector_tail(&mut payload, &[3]);
    assert_eq!(actual_marker, marker);
    let (actual_marker, components) =
        cosmetic_thread_cylinder_reference_at(&payload, body_offset).expect("required invariant");
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
        cosmetic_thread_cylinder_reference_at(&compact, body_offset).expect("required invariant");
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
        cosmetic_thread_cylinder_reference_at(&selected, body_offset).expect("required invariant");
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
        cosmetic_thread_cylinder_reference_at(&extended, body_offset).expect("required invariant");
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
        cosmetic_thread_cylinder_reference_at(&compact_legacy, body_offset)
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
        cosmetic_thread_cylinder_reference_at(&legacy, body_offset).expect("required invariant");
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
        cosmetic_thread_cylinder_reference_at(&extended, body_offset).expect("required invariant");
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
            cosmetic_thread_cylinder_reference_at(&payload, body_offset)
                .expect("required invariant");
        assert_eq!(actual_marker, marker);
        assert_eq!(
            components.last().expect("required invariant").local_id,
            Some(local_id)
        );
    }

    assert_eq!(
        cosmetic_thread_cylinder_reference_at(&payload, body_offset + 1),
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
        cosmetic_thread_cylinder_reference_at(&payload, body_offset).expect("required invariant");
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
        source_id: Some("20".into()),
        parent_source_id: None,
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

    assert_eq!(
        cosmetic_thread_cylinder_marker_reference(
            &feature,
            &lane,
            0,
            lane.native_payload.len(),
            &HashSet::from([0x802f]),
        ),
        vec![(marker, None)]
    );
}

#[test]
fn cosmetic_thread_cylinder_reference_follows_its_owned_diameter_child() {
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
        source_id: Some("53".into()),
        parent_source_id: None,
        ordinal: 0,
        name: "Thread".into(),
        kind: "Feature".into(),
        input_class: Some("moCosmeticThread_c".into()),
        suppressed: false,
        parameters: BTreeMap::from([("D2".into(), "<MOD-DIAM>8".into())]),
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
        value: 0.008,
        role: FeatureInputScalarRole::Native,
        entity_indices: Vec::new(),
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
                object_id: Some(u32::MAX),
                value: "D2".into(),
            },
            FeatureInputName {
                id: "next-feature".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: 400,
                object_id: Some(54),
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
        cosmetic_thread_diameter_child_tail(&feature, &lane),
        Some(158..400)
    );
    let references =
        cosmetic_thread_cylinder_references(&feature, &lane, 20, 100, &HashSet::from([0x802f]));
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
        value: 1.0,
        role: FeatureInputScalarRole::Native,
        entity_indices: Vec::new(),
        operands: Vec::new(),
    });
    assert!(cosmetic_thread_cylinder_references(
        &feature,
        &lane,
        20,
        100,
        &HashSet::from([0x802f]),
    )
    .is_empty());
}

#[test]
fn component_face_reference_accepts_both_nested_body_flags() {
    let body_offset = 30;
    let build_payload = |flag: u8, marker: usize| {
        let mut payload = vec![0; marker - 12];
        payload[body_offset..body_offset + 2].copy_from_slice(&0x802b_u16.to_le_bytes());
        payload[body_offset + 2..body_offset + 6].copy_from_slice(&2u32.to_le_bytes());
        payload[body_offset + 6] = flag;
        assert_eq!(selection_vector_tail(&mut payload, &[6]), marker);
        payload
    };
    let marker = body_offset + 92;
    let mut payload = build_payload(0, marker);

    let (actual_marker, components) =
        component_face_reference_at(&payload, body_offset).expect("required invariant");
    assert_eq!(actual_marker, marker);
    assert_eq!(
        components.last().expect("required invariant").local_id,
        Some(6)
    );

    let compact = build_payload(0, body_offset + 68);
    assert!(component_face_reference_at(&compact, body_offset).is_some());

    let flagged = build_payload(0x40, body_offset + 100);
    assert!(component_face_reference_at(&flagged, body_offset).is_some());
    let mut record = CLASS_MARKER.to_vec();
    record.extend((b"moCompFace_c".len() as u16).to_le_bytes());
    record.extend(b"moCompFace_c");
    record.extend_from_slice(&flagged[body_offset..]);
    assert!(component_face_reference_in_record(&record).is_some());

    payload[body_offset + 6] = 1;
    assert_eq!(component_face_reference_at(&payload, body_offset), None);
}

#[test]
fn sketch_surface_component_path_has_two_implicit_root_slots() {
    let marker = 12;
    let mut payload = Vec::new();
    payload.extend(5u32.to_le_bytes());
    payload.extend([0, 3, 0, 0]);
    payload.extend([0; 4]);
    payload.extend(COMPACT_EDGE_VECTOR_MARKER);
    payload.extend([0; 2]);
    for (index, local_id) in [4u32, 3, 5].into_iter().enumerate() {
        if index == 2 {
            payload.extend([0; 2]);
        }
        payload.extend((0x8094 + index as u16).to_le_bytes());
        payload.extend([0; 2]);
        payload.extend([index as u8 + 1; 12]);
        payload.extend(local_id.to_le_bytes());
    }

    assert_eq!(
        compact_sketch_surface_component_path_at(&payload, marker)
            .expect("required invariant")
            .iter()
            .map(|component| component.local_id)
            .collect::<Vec<_>>(),
        [Some(4), Some(3), Some(5)]
    );
}

#[test]
fn sketch_surface_component_path_accepts_a_slot_cell_between_entries() {
    let marker = 12;
    let mut payload = Vec::new();
    payload.extend(5u32.to_le_bytes());
    payload.extend([0, 3, 0, 0]);
    payload.extend([0; 4]);
    payload.extend(COMPACT_EDGE_VECTOR_MARKER);
    payload.extend([0; 2]);
    for (index, local_id) in [2u32, 0, 1].into_iter().enumerate() {
        if index == 1 {
            payload.extend([0; 4]);
        } else if index == 2 {
            payload.extend([1, 0, 0, 0, 0, 0]);
        }
        payload.extend((0x8034 + index as u16).to_le_bytes());
        payload.extend([0; 2]);
        payload.extend([index as u8 + 1; 12]);
        payload.extend(local_id.to_le_bytes());
    }

    assert_eq!(
        compact_sketch_surface_component_path_at(&payload, marker)
            .expect("required invariant")
            .iter()
            .map(|component| component.local_id)
            .collect::<Vec<_>>(),
        [Some(2), Some(0), Some(1)]
    );

    let slot = marker + 18 + 20 + 4 + 20;
    payload[slot..slot + 6].fill(0);
    assert_eq!(
        compact_sketch_surface_component_path_at(&payload, marker)
            .expect("required invariant")
            .iter()
            .map(|component| component.local_id)
            .collect::<Vec<_>>(),
        [Some(2), Some(0), Some(1)]
    );

    payload[slot..slot + 2].fill(0xff);
    assert_eq!(
        compact_sketch_surface_component_path_at(&payload, marker),
        None
    );
}

#[test]
fn legacy_sketch_surface_component_path_requires_its_ownership_trailer() {
    let marker = 12;
    let mut payload = Vec::new();
    payload.extend(5u32.to_le_bytes());
    payload.extend([0, 2, 0, 0]);
    payload.extend(7u32.to_le_bytes());
    payload.extend(COMPACT_EDGE_VECTOR_MARKER);
    payload.extend([0; 2]);
    for (index, local_id) in [2u32, 1, 0].into_iter().enumerate() {
        if index == 1 {
            payload.extend(3u32.to_le_bytes());
        } else if index == 2 {
            payload.extend(12u16.to_le_bytes());
            payload.extend([0; 4]);
        }
        payload.extend((0x8032 + index as u16).to_le_bytes());
        payload.extend([0; 2]);
        payload.extend([index as u8 + 1; 12]);
        payload.extend(local_id.to_le_bytes());
    }
    let trailer = payload.len();
    payload.extend([0; 20]);
    payload.extend(1u32.to_le_bytes());
    payload.extend(0u32.to_le_bytes());
    payload.extend(175u32.to_le_bytes());
    payload.extend([0; 12]);

    assert_eq!(
        compact_sketch_surface_component_path_at(&payload, marker)
            .expect("required invariant")
            .iter()
            .map(|component| component.local_id)
            .collect::<Vec<_>>(),
        [Some(2), Some(1), Some(0)]
    );

    payload[trailer + 28..trailer + 32].fill(0);
    assert_eq!(
        compact_sketch_surface_component_path_at(&payload, marker),
        None
    );

    payload[trailer + 28..trailer + 32].copy_from_slice(&175u32.to_le_bytes());
    payload.truncate(trailer);
    payload.extend(14u32.to_le_bytes());
    payload.extend([0; 8]);
    payload.extend(1u32.to_le_bytes());
    payload.extend(0u32.to_le_bytes());
    payload.extend(135u32.to_le_bytes());
    payload.extend([0; 12]);
    assert!(compact_sketch_surface_component_path_at(&payload, marker).is_some());

    payload[trailer..trailer + 4].fill(0);
    assert_eq!(
        compact_sketch_surface_component_path_at(&payload, marker),
        None
    );

    payload.truncate(trailer);
    payload.extend([0; 8]);
    payload.extend(1u32.to_le_bytes());
    payload.extend(0u32.to_le_bytes());
    payload.extend(135u32.to_le_bytes());
    payload.extend([0; 12]);
    assert!(compact_sketch_surface_component_path_at(&payload, marker).is_some());

    payload[trailer + 16..trailer + 20].fill(0);
    assert_eq!(
        compact_sketch_surface_component_path_at(&payload, marker),
        None
    );
}

#[test]
fn mirror_pattern_path_count_includes_the_unserialized_root_cell() {
    let marker = 12;
    let mut payload = vec![0; marker];
    payload[..4].copy_from_slice(&4u32.to_le_bytes());
    payload.extend(COMPACT_EDGE_VECTOR_MARKER);
    payload.extend([0, 0]);
    for (index, (instance, signature)) in [
        (
            0x803e_u16,
            [0x34, 0x80, 0x37, 0, 37, 0, 0, 0, 0x7a, 0x83, 0xd9, 0x4a],
        ),
        (
            0x8263,
            [0x34, 0x80, 0x37, 0, 50, 0, 0, 0, 0xf9, 0x83, 0xd9, 0x4a],
        ),
        (
            0x803e,
            [0x34, 0x80, 0x37, 0, 37, 0, 0, 0, 0x7a, 0x83, 0xd9, 0x4a],
        ),
    ]
    .into_iter()
    .enumerate()
    {
        if index == 2 {
            payload.extend([0; 8]);
        }
        payload.extend(instance.to_le_bytes());
        payload.extend([0, 0]);
        payload.extend(signature);
        payload.extend([2u32, 1, 3][index].to_le_bytes());
    }
    payload.extend([0; 32]);

    let path = mirror_pattern_component_path_at(&payload, marker).expect("required invariant");
    assert_eq!(path.len(), 3);
    assert_eq!(path.last().expect("required invariant").local_id, Some(3));
    assert_eq!(
        &path.last().expect("required invariant").type_signature[4..8],
        &37u32.to_le_bytes()
    );

    payload[..4].copy_from_slice(&5u32.to_le_bytes());
    assert_eq!(
        mirror_pattern_component_path_at(&payload, marker)
            .expect("two root slots")
            .len(),
        3
    );
    payload[4] = 1;
    assert!(mirror_pattern_component_path_at(&payload, marker).is_none());

    for (count, separator) in [
        (3u32, &[][..]),
        (4, &[1, 0, 0, 0, 0, 0, 0, 0][..]),
        (5, &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0][..]),
        (4, &[5, 0, 0, 0][..]),
    ] {
        let mut mixed = vec![0; marker];
        mixed[..4].copy_from_slice(&count.to_le_bytes());
        mixed.extend(COMPACT_EDGE_VECTOR_MARKER);
        mixed.extend([0, 0]);
        mixed.extend(0x803e_u16.to_le_bytes());
        mixed.extend([0, 0]);
        mixed.extend([0x34, 0x80, 0x37, 0, 37, 0, 0, 0, 0x7a, 0x83, 0xd9, 0x4a]);
        mixed.extend(2u32.to_le_bytes());
        mixed.extend(separator);
        mixed.extend([0x34, 0x80, 0x37, 0, 50, 0, 0, 0, 0xf9, 0x83, 0xd9, 0x4a]);
        mixed.extend(1u32.to_le_bytes());
        mixed.extend(0x8263_u16.to_le_bytes());
        mixed.extend([0, 0]);
        mixed.extend([0x34, 0x80, 0x37, 0, 37, 0, 0, 0, 0x7a, 0x83, 0xd9, 0x4a]);
        mixed.extend(3u32.to_le_bytes());
        assert_eq!(
            mirror_pattern_component_path_at(&mixed, marker)
                .expect("mixed mirror path")
                .len(),
            3
        );
    }
}

#[test]
fn component_vector_cell_count_includes_interleaved_path_slots() {
    let marker = 12;
    let mut payload = vec![0; marker];
    payload[..4].copy_from_slice(&7u32.to_le_bytes());
    payload.extend(COMPACT_EDGE_VECTOR_MARKER);
    payload.extend([0, 0]);
    for index in 0..4u32 {
        payload.extend(0x803e_u16.to_le_bytes());
        payload.extend([0, 0]);
        payload.extend([0x34, 0x80, 0x37, 0]);
        payload.extend((37 + index).to_le_bytes());
        payload.extend(0x4ad9_837au32.wrapping_add(index).to_le_bytes());
        payload.extend((index + 1).to_le_bytes());
        if index != 3 {
            payload.extend((25 + index * 2).to_le_bytes());
        }
    }

    let path = component_vector_path_at(&payload, marker).expect("interleaved path slots");
    assert_eq!(path.len(), 4);
    assert_eq!(path.last().expect("terminal component").local_id, Some(4));
}

#[test]
fn component_vector_preserves_identifierless_lineage_hops() {
    let marker = 12;
    let mut payload = vec![0; marker];
    payload[..4].copy_from_slice(&5u32.to_le_bytes());
    payload[4..8].copy_from_slice(&[6, 2, 0, 0]);
    payload.extend(COMPACT_EDGE_VECTOR_MARKER);
    payload.extend([0, 0]);

    let append_hop = |payload: &mut Vec<u8>, instance: u16, source: u32, timestamp: u32| {
        payload.extend(instance.to_le_bytes());
        payload.extend([0, 0]);
        payload.extend([0xa7, 0x81, 0xa9, 0x01]);
        payload.extend(source.to_le_bytes());
        payload.extend(timestamp.to_le_bytes());
    };
    append_hop(&mut payload, 0x8675, 230, 0x51c6_17de);
    append_hop(&mut payload, 0x8675, 134, 0x51c6_080c);
    append_hop(&mut payload, 0x81a5, 18, 0x51c5_fde3);
    payload.extend(16u32.to_le_bytes());
    payload.extend([0; 24]);

    let path = component_vector_path_at(&payload, marker).expect("lineage path");
    assert_eq!(path.len(), 3);
    assert_eq!(path[0].instance, Some(0x8675));
    assert_eq!(path[0].local_id, None);
    assert_eq!(path[1].instance, Some(0x8675));
    assert_eq!(path[1].local_id, None);
    assert_eq!(path[2].instance, Some(0x81a5));
    assert_eq!(path[2].local_id, Some(16));
}

#[test]
fn planar_surface_candidates_keep_only_defining_type_two_vectors() {
    let mut payload = Vec::new();
    let append_vector = |payload: &mut Vec<u8>, selector: u8, source: u32, terminal: u32| {
        payload.extend(7u32.to_le_bytes());
        payload.extend([selector, 2, 0, 0]);
        payload.extend(0u32.to_le_bytes());
        payload.extend(COMPACT_EDGE_VECTOR_MARKER);
        payload.extend([0, 0]);
        for index in 0..4u32 {
            payload.extend(0x803e_u16.to_le_bytes());
            payload.extend([0, 0]);
            payload.extend([0x34, 0x80, 0x37, 0]);
            payload.extend((source + index).to_le_bytes());
            payload.extend(0x4ad9_837a_u32.wrapping_add(index).to_le_bytes());
            payload.extend(if index == 3 { terminal } else { index + 1 }.to_le_bytes());
            if index != 3 {
                payload.extend((25 + index * 2).to_le_bytes());
            }
        }
    };
    append_vector(&mut payload, 6, 230, 16);
    payload.extend([0; 4]);
    append_vector(&mut payload, 4, 218, 12);
    payload.extend([0; 4]);

    let candidates = planar_surface_selection_candidates(&payload, 0, payload.len());
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].1.len(), 4);
    assert_eq!(
        candidates[0].1[0].type_signature[4..8],
        230u32.to_le_bytes()
    );
    assert_eq!(
        candidates[0].1.last().and_then(|entry| entry.local_id),
        Some(16)
    );
    assert_eq!(
        candidates[1].1[0].type_signature[4..8],
        218u32.to_le_bytes()
    );
    assert_eq!(
        candidates[1].1.last().and_then(|entry| entry.local_id),
        Some(12)
    );

    payload[4] = 0x7f;
    assert_eq!(
        planar_surface_selection_candidates(&payload, 0, payload.len()).len(),
        2
    );
    payload[5] = 3;
    assert_eq!(
        planar_surface_selection_candidates(&payload, 0, payload.len()).len(),
        1
    );
}

#[test]
fn counted_surface_path_preserves_tagged_and_anonymous_nodes() {
    let marker = 12;
    let mut payload = vec![0; marker];
    payload[..4].copy_from_slice(&2u32.to_le_bytes());
    payload[4..8].copy_from_slice(&[0, 2, 0, 0]);
    payload.extend(COMPACT_EDGE_VECTOR_MARKER);
    payload.extend([0, 0]);
    payload.extend(0x803e_u16.to_le_bytes());
    payload.extend([0, 0]);
    payload.extend([0x34, 0x80, 1, 0, 57, 0, 0, 0, 1, 0, 0, 0]);
    payload.extend(9u32.to_le_bytes());
    payload.extend([0; 4]);
    payload.extend([0x34, 0x80, 1, 0, 56, 0, 0, 0, 2, 0, 0, 0]);
    payload.extend(4u32.to_le_bytes());

    let path = counted_surface_component_path_at(&payload, marker).expect("required invariant");
    assert_eq!(path.len(), 2);
    assert_eq!(path[0].instance, Some(0x803e));
    assert_eq!(path[0].local_id, Some(9));
    assert_eq!(path[1].instance, None);
    assert_eq!(path[1].local_id, Some(4));
    assert_eq!(&path[1].type_signature[4..8], &56u32.to_le_bytes());
    assert!(surface_reference_matches_at(&payload, marker, &path));

    payload[..4].copy_from_slice(&3u32.to_le_bytes());
    assert_eq!(
        counted_surface_component_path_at(&payload, marker)
            .expect("one root slot")
            .len(),
        2
    );
    payload[..4].copy_from_slice(&4u32.to_le_bytes());
    assert!(counted_surface_component_path_at(&payload, marker).is_none());
}

#[test]
fn face_reference_plane_owns_its_counted_surface_path() {
    let class_name = "moFaceRefPlnData_c";
    let class_offset = 32;
    let class_body = class_offset + 6 + class_name.len();
    let marker = class_body + 109;
    let mut payload = vec![0; marker + 18];
    payload[class_offset..class_offset + 4].copy_from_slice(CLASS_MARKER);
    payload[class_offset + 4..class_offset + 6]
        .copy_from_slice(&(class_name.len() as u16).to_le_bytes());
    payload[class_offset + 6..class_body].copy_from_slice(class_name.as_bytes());
    payload[marker - 12..marker - 8].copy_from_slice(&3u32.to_le_bytes());
    payload[marker - 8..marker - 4].copy_from_slice(&[0, 2, 0, 0]);
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    for (index, local_id) in [11u32, 7].into_iter().enumerate() {
        payload.extend((0x8038 + index as u16).to_le_bytes());
        payload.extend([0, 0]);
        payload.extend([0x23, 0x80, 1, 0]);
        payload.extend((40 + index as u32).to_le_bytes());
        payload.extend((90 + index as u32).to_le_bytes());
        payload.extend(local_id.to_le_bytes());
    }
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: vec![FeatureInputClass {
            id: "face-plane-data".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: class_offset as u64,
            name: class_name.into(),
            role: FeatureInputClassRole::Reference,
        }],
        names: vec![
            FeatureInputName {
                id: "producer-40-name".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 0,
                object_id: Some(40),
                value: "Producer40".into(),
            },
            FeatureInputName {
                id: "producer-41-name".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: 8,
                object_id: Some(41),
                value: "Producer41".into(),
            },
            FeatureInputName {
                id: "plane-name".into(),
                parent: "lane".into(),
                ordinal: 2,
                offset: 16,
                object_id: Some(37),
                value: "Plane".into(),
            },
        ],
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

    let candidates = face_reference_plane_selection_candidates(&lane, 0, lane.native_payload.len());
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].0, marker);
    assert_eq!(
        candidates[0]
            .1
            .iter()
            .filter_map(|component| component.local_id)
            .collect::<Vec<_>>(),
        [11, 7]
    );

    let native_feature = |id: &str, source: u32, input_class: &str| Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some(source.to_string()),
        parent_source_id: None,
        ordinal: source,
        name: id.into(),
        kind: "Feature".into(),
        input_class: Some(input_class.into()),
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let histories = [FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![
            native_feature("producer-40", 40, "moExtrusion_c"),
            native_feature("producer-41", 41, "moExtrusion_c"),
            native_feature("plane", 37, "moRefPlane_c"),
        ],
    }];
    let selections = compact_surface_selections(&histories, &lane);
    assert_eq!(selections.len(), 1);
    assert_eq!(selections[0].feature_ref, "plane");
    assert_eq!(
        selections[0].terminal_feature_ref.as_deref(),
        Some("producer-41")
    );
}

#[test]
fn inline_surface_path_distinguishes_branch_and_selection_nodes() {
    let prefix = [0x54, 0x81, 0x56, 0x01];
    let signature = |source: u32, identity: u32| {
        let mut signature = [0; 12];
        signature[..4].copy_from_slice(&prefix);
        signature[4..8].copy_from_slice(&source.to_le_bytes());
        signature[8..].copy_from_slice(&identity.to_le_bytes());
        signature
    };
    let mut payload = 0x8157_u16.to_le_bytes().to_vec();
    payload.extend([0, 0]);
    payload.extend(signature(20, 1));
    payload.extend(0x8200_u16.to_le_bytes());
    payload.extend([0, 0]);
    payload.extend(signature(10, 2));
    payload.extend(7u32.to_le_bytes());

    let path = inline_surface_reference_at(&payload, 4).expect("required invariant");
    assert_eq!(path.len(), 2);
    assert_eq!(path[0].instance, Some(0x8157));
    assert_eq!(path[0].local_id, None);
    assert_eq!(path[1].instance, Some(0x8200));
    assert_eq!(path[1].local_id, Some(7));
}

#[test]
fn projected_split_line_consumes_self_owned_surface_identity_paths() {
    let class_name = "moPLineSurfIdRep_c";
    let prefix = [0xc3, 0x80, 0xc5, 0x00];
    let signature = |source: u32, identity: u32| {
        let mut signature = [0; 12];
        signature[..4].copy_from_slice(&prefix);
        signature[4..8].copy_from_slice(&source.to_le_bytes());
        signature[8..].copy_from_slice(&identity.to_le_bytes());
        signature
    };
    let mut payload = CLASS_MARKER.to_vec();
    payload.extend((class_name.len() as u16).to_le_bytes());
    payload.extend(class_name.as_bytes());
    payload.extend([0, 0]);
    payload.extend(signature(711, 1));
    payload.extend(0x80a7_u16.to_le_bytes());
    payload.extend([0, 0]);
    payload.extend(signature(314, 2));
    payload.extend(3u32.to_le_bytes());
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload.clone(),
        classes: vec![
            FeatureInputClass {
                id: "surface-class".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 0,
                name: class_name.into(),
                role: FeatureInputClassRole::Auxiliary,
            },
            FeatureInputClass {
                id: "projection-class".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: payload.len() as u64,
                name: "moPLineProjIdRep_c".into(),
                role: FeatureInputClassRole::Auxiliary,
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

    let candidates = operation_surface_selection_candidates(
        FeatureClass::SplitFace,
        &lane,
        0,
        payload.len(),
        Some(711),
    );
    assert_eq!(candidates.len(), 1, "{candidates:#?}");
    assert_eq!(candidates[0].1.len(), 2);
    assert_eq!(
        &candidates[0].1[0].type_signature[4..8],
        &711u32.to_le_bytes()
    );
    assert_eq!(
        &candidates[0].1[1].type_signature[4..8],
        &314u32.to_le_bytes()
    );
    assert_eq!(candidates[0].1[1].local_id, Some(3));
    assert!(operation_surface_selection_candidates(
        FeatureClass::SplitFace,
        &lane,
        0,
        payload.len(),
        Some(712),
    )
    .is_empty());
}

#[test]
fn generated_surface_identities_are_producer_outputs() {
    let class_name = "moWzdHoleSurfIdRep_c";
    let prefix = [0xc3, 0x80, 0xc5, 0x00];
    let mut payload = CLASS_MARKER.to_vec();
    payload.extend((class_name.len() as u16).to_le_bytes());
    payload.extend(class_name.as_bytes());
    payload.extend([0, 0]);
    payload.extend(prefix);
    payload.extend(89u32.to_le_bytes());
    payload.extend(0x52e4_6185u32.to_le_bytes());
    payload.extend(2u32.to_le_bytes());
    payload.extend(0x85b5u16.to_le_bytes());
    payload.extend([0, 0]);
    payload.extend(prefix);
    payload.extend(89u32.to_le_bytes());
    payload.extend(0x52e4_6185u32.to_le_bytes());
    payload.extend(2u32.to_le_bytes());
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: vec![FeatureInputClass {
            id: "class".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            name: class_name.into(),
            role: FeatureInputClassRole::Auxiliary,
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

    let identities = generated_surface_identities(&lane);

    assert_eq!(identities.len(), 2, "{identities:#?}");
    assert!(identities.iter().all(|identity| {
        identity.type_prefix == prefix
            && identity.feature_source_id == 89
            && identity.local_identity == 2
    }));
    assert_eq!(identities[0].components[0].instance, None);
    assert_eq!(identities[1].components[0].instance, Some(0x85b5));
}

#[test]
fn idless_history_features_use_unique_feature_input_object_sources() {
    let feature = Feature {
        id: "producer".into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: None,
        parent_source_id: None,
        ordinal: 0,
        name: "Producer".into(),
        kind: "Feature".into(),
        input_class: Some("ProducerClass".into()),
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
    let mut lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: vec![FeatureInputName {
            id: "name".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            object_id: Some(233),
            value: "Producer".into(),
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

    let ambiguous_history = history.clone();
    let resolved = history_features_with_object_sources(&[history], &lane);

    assert_eq!(resolved[0].source_id.as_deref(), Some("233"));

    lane.names.push(FeatureInputName {
        id: "ambiguous-name".into(),
        parent: "lane".into(),
        ordinal: 1,
        offset: 1,
        object_id: Some(234),
        value: "Producer".into(),
    });
    let ambiguous = history_features_with_object_sources(&[ambiguous_history], &lane);
    assert_eq!(ambiguous[0].source_id, None);
}
