//! Tests for the `terminations` module.

use super::super::selections::COMPACT_EDGE_VECTOR_MARKER;
use super::super::selections::{compact_surface_selections, selection_vector_tail};
use super::*;
use crate::records::{Feature, FeatureHistory, FeatureInputLane, FeatureInputName};
use std::collections::BTreeMap;

#[test]
fn compact_extrusion_through_all_requires_the_complete_end_spec() {
    let mut payload = vec![0; 104];
    payload[..2].copy_from_slice(&[0x0c, 0x8e]);
    payload[4] = 1;
    payload[18] = 1;
    payload[30..34].copy_from_slice(&[1, 0, 0, 1]);
    payload[92] = 1;
    assert!(compact_extrusion_through_all_at(&payload, 0));
    payload[8] = 1;
    assert!(compact_extrusion_through_all_at(&payload, 0));
    payload[8] = 0;

    payload[18] = 0;
    assert!(!compact_extrusion_through_all_at(&payload, 0));
    payload[18] = 1;
    payload[103] = 1;
    assert!(!compact_extrusion_through_all_at(&payload, 0));

    let declaration = b"\xff\xff\x01\x00\x0b\x00moEndSpec_c";
    let mut direct = vec![0; declaration.len() + 102];
    direct[..declaration.len()].copy_from_slice(declaration);
    let body = declaration.len();
    direct[body + 2..body + 6].copy_from_slice(&1u32.to_le_bytes());
    direct[body + 16..body + 20].copy_from_slice(&1u32.to_le_bytes());
    direct[body + 28..body + 32].copy_from_slice(&[1, 0, 0, 1]);
    direct[body + 88..body + 92].copy_from_slice(&[0, 0, 1, 0]);
    direct[body + 98..body + 102].copy_from_slice(&[0xff, 0xff, 1, 0]);
    assert!(compact_extrusion_through_all_at(&direct, body - 2));

    direct[body + 6..body + 10].copy_from_slice(&1u32.to_le_bytes());
    assert!(compact_extrusion_through_all_at(&direct, body - 2));
}

#[test]
fn compact_extrusion_to_face_requires_a_single_face_reference_child() {
    let mut payload = vec![0; 200];
    payload[..2].copy_from_slice(&[0x0c, 0x8e]);
    payload[4] = 1;
    payload[18] = 4;
    payload[30..33].copy_from_slice(&[1, 1, 0]);
    payload[33..35].copy_from_slice(&[0x7f, 0x9d]);
    payload[35..46].copy_from_slice(&[0x2d, 0x80, 0x2b, 0x80, 2, 0, 0, 0, 0x40, 0, 0]);
    payload[88..92].copy_from_slice(&1u32.to_le_bytes());
    payload[92..96].copy_from_slice(&[0, 2, 0, 0]);
    payload[100..116].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    payload[118..122].copy_from_slice(&[0x32, 0x80, 0, 0]);
    payload[122..134].fill(1);
    payload[134..138].copy_from_slice(&7u32.to_le_bytes());
    assert_eq!(
        compact_extrusion_to_face_at(&payload, 0, payload.len()),
        Some(100)
    );
    let path = compact_single_face_reference_path_at(&payload, 100).expect("required invariant");
    assert_eq!(path.len(), 1);
    assert_eq!(path[0].instance, Some(0x8032));
    assert_eq!(path[0].type_signature, [1; 12]);
    assert_eq!(path[0].local_id, Some(7));

    for selector in [[4, 2, 0, 0], [6, 2, 0, 0]] {
        payload[92..96].copy_from_slice(&selector);
        assert_eq!(
            compact_extrusion_to_face_at(&payload, 0, payload.len()),
            Some(100)
        );
        let path = compact_single_face_reference_path_at(&payload, 100)
            .expect("lane subtype must not change the component path");
        assert_eq!(path.len(), 1);
        assert_eq!(path[0].local_id, Some(7));
    }
    payload[92..96].copy_from_slice(&[0, 2, 0, 0]);

    payload[35..39].copy_from_slice(&[0xe4, 0x82, 0x07, 0x81]);
    assert_eq!(
        compact_extrusion_to_face_at(&payload, 0, payload.len()),
        Some(100)
    );
    payload[37..39].copy_from_slice(&[0xff, 0xff]);
    assert_eq!(
        compact_extrusion_to_face_at(&payload, 0, payload.len()),
        None
    );
    payload[37..39].copy_from_slice(&[0x07, 0x81]);

    payload[88..92].copy_from_slice(&2u32.to_le_bytes());
    payload[138..158].fill(0);
    payload[158..162].copy_from_slice(&101u32.to_le_bytes());
    let (path, terminal_source) =
        compact_single_face_reference_record_at(&payload, 100).expect("required invariant");
    assert_eq!(path.len(), 1);
    assert_eq!(terminal_source, Some(101));

    payload[88..92].copy_from_slice(&3u32.to_le_bytes());
    payload[138..142].fill(0);
    payload[142..146].copy_from_slice(&[0xf5, 0x81, 0, 0]);
    payload[146..158]
        .copy_from_slice(&[0xf0, 0x81, 0x4d, 2, 0xd6, 0, 0, 0, 0x4d, 0xb8, 0xb0, 0x59]);
    payload[158..162].copy_from_slice(&9u32.to_le_bytes());
    payload[162..186].fill(0);
    payload[186..190].copy_from_slice(&101u32.to_le_bytes());
    let (path, terminal_source) =
        compact_single_face_reference_record_at(&payload, 100).expect("required invariant");
    assert_eq!(path.len(), 2);
    assert_eq!(path[1].local_id, Some(9));
    assert_eq!(terminal_source, Some(101));
    payload[88..92].copy_from_slice(&1u32.to_le_bytes());

    payload[12] = 1;
    payload[22] = 1;
    assert_eq!(
        compact_extrusion_to_face_at(&payload, 0, payload.len()),
        Some(100)
    );

    payload[88..92].fill(0);
    assert_eq!(
        compact_extrusion_to_face_at(&payload, 0, payload.len()),
        None
    );
}

#[test]
fn compact_extrusion_to_face_accepts_root_adjusted_component_paths() {
    fn payload(flag: u8, count: u32) -> Vec<u8> {
        let mut payload = vec![0; 260];
        payload[..2].copy_from_slice(&[0x0c, 0x8e]);
        payload[4] = 1;
        payload[18] = 4;
        payload[30..33].copy_from_slice(&[1, 1, 0]);
        payload[33..35].copy_from_slice(&[0x7f, 0x9d]);
        payload[35..46].copy_from_slice(&[0x2d, 0x80, 0x2b, 0x80, 2, 0, 0, 0, 0x40, 0, 0]);
        payload[88..92].copy_from_slice(&count.to_le_bytes());
        payload[92..96].copy_from_slice(&[0, flag, 0, 0]);
        payload[100..116].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
        payload
    }
    fn entry(payload: &mut [u8], offset: usize, token: u16, signature: u8, local_id: u32) {
        payload[offset..offset + 2].copy_from_slice(&token.to_le_bytes());
        payload[offset + 4..offset + 16].fill(signature);
        payload[offset + 16..offset + 20].copy_from_slice(&local_id.to_le_bytes());
    }

    let mut slotted = payload(3, 3);
    entry(&mut slotted, 118, 0x8049, 1, 0);
    slotted[138..142].copy_from_slice(&34u32.to_le_bytes());
    entry(&mut slotted, 142, 0x8034, 2, 24);
    slotted[162..182].fill(0);
    slotted[182..186].copy_from_slice(&101u32.to_le_bytes());
    let path = compact_single_face_reference_path_at(&slotted, 100).expect("required invariant");
    assert_eq!(path.len(), 2);
    assert_eq!(path[1].instance, Some(0x8034));
    assert_eq!(
        compact_extrusion_to_face_at(&slotted, 0, slotted.len()),
        Some(100)
    );

    let mut aligned = payload(2, 5);
    entry(&mut aligned, 118, 0x8633, 1, 1);
    entry(&mut aligned, 146, 0x830d, 2, 1);
    aligned[166..176].copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 0, 0]);
    entry(&mut aligned, 176, 0x830d, 3, 1);
    aligned[196..204].copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]);
    let path = compact_single_face_reference_path_at(&aligned, 100).expect("required invariant");
    assert_eq!(path.len(), 3);
    assert_eq!(path[2].type_signature, [3; 12]);
    assert_eq!(
        compact_extrusion_to_face_at(&aligned, 0, aligned.len()),
        Some(100)
    );
}

#[test]
fn compact_extrusion_to_face_accepts_the_legacy_end_spec_token() {
    let mut payload = vec![0; 200];
    payload[..2].copy_from_slice(&[3, 0]);
    payload[4] = 1;
    payload[18] = 4;
    payload[30..33].copy_from_slice(&[1, 1, 0]);
    payload[33..35].copy_from_slice(&[0x7f, 0x9d]);
    payload[35..46].copy_from_slice(&[0x2d, 0x80, 0x2b, 0x80, 2, 0, 0, 0, 0x40, 0, 0]);
    payload[88..92].copy_from_slice(&1u32.to_le_bytes());
    payload[92..96].copy_from_slice(&[0, 2, 0, 0]);
    payload[100..116].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    payload[118..122].copy_from_slice(&[0x32, 0x80, 0, 0]);
    payload[122..134].fill(1);
    payload[134..138].copy_from_slice(&7u32.to_le_bytes());

    assert_eq!(
        compact_extrusion_to_face_at(&payload, 0, payload.len()),
        Some(100)
    );
    payload[0] = 2;
    assert_eq!(
        compact_extrusion_to_face_at(&payload, 0, payload.len()),
        None
    );
}

#[test]
fn compact_extrusion_to_face_accepts_a_declared_width_two_child() {
    let mut payload = vec![0; 240];
    payload[..2].copy_from_slice(&[0x09, 0x81]);
    payload[4] = 1;
    payload[18] = 4;
    payload[30..33].copy_from_slice(&[1, 1, 0]);
    let declaration = b"\xff\xff\x01\x00\x11\x00moSingleFaceRef_w";
    payload[33..33 + declaration.len()].copy_from_slice(declaration);
    let body = 33 + declaration.len();
    payload[body..body + 14]
        .copy_from_slice(&[0x7e, 0x81, 0x1f, 0x82, 2, 0, 0x22, 2, 0x4a, 2, 0, 0, 4, 0]);
    let marker = 160;
    payload[marker - 12..marker - 8].copy_from_slice(&1u32.to_le_bytes());
    payload[marker - 8..marker - 4].copy_from_slice(&[0, 2, 0, 0]);
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    payload[marker + 18..marker + 22].copy_from_slice(&[0x32, 0x80, 0, 0]);
    payload[marker + 22..marker + 34].fill(1);
    payload[marker + 34..marker + 38].copy_from_slice(&7u32.to_le_bytes());

    assert_eq!(
        compact_extrusion_to_face_at(&payload, 0, payload.len()),
        Some(marker)
    );
    payload[33] = 0;
    assert_eq!(
        compact_extrusion_to_face_at(&payload, 0, payload.len()),
        None
    );
}

#[test]
fn compact_extrusion_to_face_preserves_an_unparsed_declared_face_child() {
    let end_spec = b"\xff\xff\x01\x00\x0b\x00moEndSpec_c";
    let face_ref = b"\xff\xff\x01\x00\x11\x00moSingleFaceRef_w";
    let mut payload = vec![0; 180];
    payload[..end_spec.len()].copy_from_slice(end_spec);
    let anchor = end_spec.len() - 2;
    payload[anchor + 4..anchor + 8].copy_from_slice(&1u32.to_le_bytes());
    payload[anchor + 18..anchor + 22].copy_from_slice(&4u32.to_le_bytes());
    payload[anchor + 30..anchor + 33].copy_from_slice(&[1, 1, 0]);
    let child = anchor + 33;
    payload[child..child + face_ref.len()].copy_from_slice(face_ref);
    let body = child + face_ref.len();
    payload[body..body + 18].copy_from_slice(&[
        0x18, 0x81, 0xca, 0x80, 2, 0, 0xcc, 0x80, 0, 0, 0xce, 0x80, 1, 0, 0, 0, 0xd0, 0x80,
    ]);

    assert_eq!(
        compact_extrusion_to_face_at(&payload, anchor, payload.len()),
        Some(body)
    );

    let mut lane_token = payload[anchor..].to_vec();
    lane_token[..2].copy_from_slice(&[0x0c, 0x8e]);
    assert_eq!(
        compact_extrusion_to_face_at(&lane_token, 0, lane_token.len()),
        Some(body - anchor)
    );

    let comp_face = b"\xff\xff\x01\x00\x0c\x00moCompFace_c";
    payload[body..body + comp_face.len()].copy_from_slice(comp_face);
    let nested = body + comp_face.len();
    payload[nested..nested + 16].copy_from_slice(&[
        0x86, 0x81, 2, 0, 0x88, 0x81, 0, 0, 0x8a, 0x81, 1, 0, 0, 0, 0x8c, 0x81,
    ]);
    assert_eq!(
        compact_extrusion_to_face_at(&payload, anchor, payload.len()),
        Some(body)
    );

    payload[nested + 2] = 3;
    assert_eq!(
        compact_extrusion_to_face_at(&payload, anchor, payload.len()),
        None
    );
    payload[nested + 2] = 2;

    payload[body + 4] = 3;
    assert_eq!(
        compact_extrusion_to_face_at(&payload, anchor, payload.len()),
        None
    );
}

#[test]
fn compact_extrusion_to_face_prefers_a_modern_marker_over_a_legacy_body_alias() {
    let end_spec = b"\xff\xff\x01\x00\x0b\x00moEndSpec_c";
    let face_ref = b"\xff\xff\x01\x00\x11\x00moSingleFaceRef_w";
    let mut payload = vec![0; 360];
    payload[..end_spec.len()].copy_from_slice(end_spec);
    let anchor = end_spec.len() - 2;
    payload[anchor + 4..anchor + 8].copy_from_slice(&1u32.to_le_bytes());
    payload[anchor + 18..anchor + 22].copy_from_slice(&4u32.to_le_bytes());
    payload[anchor + 30..anchor + 33].copy_from_slice(&[1, 1, 0]);
    let child = anchor + 33;
    payload[child..child + face_ref.len()].copy_from_slice(face_ref);
    let body = child + face_ref.len();

    // This is a valid legacy body path. A modern marker follows it in the
    // same declared child, so accepting both anchors would make the
    // termination ambiguous.
    payload[body..body + 19].copy_from_slice(&[
        0xe5, 0x83, 0x8b, 0x80, 2, 0, 0, 0, 0x40, 0, 0, 17, 0, 0, 0, 17, 0, 0, 0,
    ]);
    let legacy_control = body + 44;
    payload[legacy_control..legacy_control + 2].copy_from_slice(&[0x30, 0x80]);
    payload[legacy_control + 2..legacy_control + 6].copy_from_slice(&1u32.to_le_bytes());
    payload[legacy_control + 10..legacy_control + 14].copy_from_slice(&1u32.to_le_bytes());
    payload[legacy_control + 14..legacy_control + 18].copy_from_slice(&[0, 2, 0, 0]);
    payload[legacy_control + 22..legacy_control + 38].fill(1);
    let legacy_entry = legacy_control + 40;
    payload[legacy_entry..legacy_entry + 4].copy_from_slice(&[0x32, 0x80, 0, 0]);
    payload[legacy_entry + 4..legacy_entry + 16].fill(2);
    payload[legacy_entry + 16..legacy_entry + 20].copy_from_slice(&7u32.to_le_bytes());
    payload[legacy_entry + 20..legacy_entry + 40].fill(0);
    payload[legacy_entry + 40..legacy_entry + 44].copy_from_slice(&101u32.to_le_bytes());

    let marker = body + 140;
    payload[marker - 12..marker - 8].copy_from_slice(&1u32.to_le_bytes());
    payload[marker - 8..marker - 4].copy_from_slice(&[0, 2, 0, 0]);
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    payload[marker + 18..marker + 22].copy_from_slice(&[0x33, 0x80, 0, 0]);
    payload[marker + 22..marker + 34].fill(3);
    payload[marker + 34..marker + 38].copy_from_slice(&9u32.to_le_bytes());

    assert_eq!(
        compact_extrusion_to_face_at(&payload, anchor, payload.len()),
        Some(marker)
    );
}

#[test]
fn extrusion_termination_stops_before_the_following_profile_object() {
    let mut payload = vec![0; 520];
    let anchor = 100;
    payload[anchor..anchor + 2].copy_from_slice(&[0x20, 0x86]);
    payload[anchor + 4..anchor + 8].copy_from_slice(&1u32.to_le_bytes());
    payload[anchor + 12..anchor + 16].copy_from_slice(&1u32.to_le_bytes());
    payload[anchor + 18..anchor + 22].copy_from_slice(&4u32.to_le_bytes());
    payload[anchor + 30..anchor + 33].copy_from_slice(&[1, 1, 0]);
    let face_ref = b"\xff\xff\x01\x00\x11\x00moSingleFaceRef_w";
    let child = anchor + 33;
    payload[child..child + face_ref.len()].copy_from_slice(face_ref);
    let body = child + face_ref.len();
    payload[body..body + 11].copy_from_slice(&[0x31, 0x80, 0x2f, 0x80, 2, 0, 0, 0, 0x40, 0, 0]);
    for (marker, signature) in [(220usize, 1u8), (400, 2)] {
        payload[marker - 12..marker - 8].copy_from_slice(&1u32.to_le_bytes());
        payload[marker - 8..marker - 4].copy_from_slice(&[0, 2, 0, 0]);
        payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
        payload[marker + 18..marker + 22].copy_from_slice(&[0x32, 0x80, 0, 0]);
        payload[marker + 22..marker + 34].fill(signature);
        payload[marker + 34..marker + 38].copy_from_slice(&7u32.to_le_bytes());
    }
    let feature = |id: &str, source_id: &str, kind: &str, input_class: &str| Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: if kind == "Sketch" {
            "Sketch"
        } else {
            "Feature"
        }
        .into(),
        tree_parent: None,
        source_id: Some(source_id.into()),
        parent_source_id: None,
        ordinal: source_id.parse().expect("required invariant"),
        name: id.into(),
        kind: kind.into(),
        input_class: Some(input_class.into()),
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let mut histories = vec![FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![
            feature("extrusion", "10", "Boss-Extrude", "moICE_c"),
            feature("profile", "11", "Sketch", "moProfileFeature_c"),
        ],
    }];
    let lane = FeatureInputLane {
        id: "lane#7".into(),
        configuration: None,
        native_payload: payload,
        classes: Vec::new(),
        names: vec![
            FeatureInputName {
                id: "extrusion-name".into(),
                parent: "lane#7".into(),
                ordinal: 0,
                offset: 10,
                value: "extrusion".into(),
                object_id: Some(10),
            },
            FeatureInputName {
                id: "profile-name".into(),
                parent: "lane#7".into(),
                ordinal: 1,
                offset: 300,
                value: "profile".into(),
                object_id: Some(11),
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

    enrich_history_extrusion_terminations(&mut histories, std::slice::from_ref(&lane));

    assert_eq!(
        histories[0].features[0].properties.get("EndCondition"),
        Some(&"ToFace".to_string())
    );
    assert_eq!(
        histories[0].features[0].properties.get("Face"),
        Some(&"sldprt:feature-input:single-face-ref:7:220".to_string())
    );
    let selections = compact_surface_selections(&histories, &lane);
    assert_eq!(selections.len(), 1);
    assert_eq!(selections[0].offset, 220);
}

#[test]
fn extrusion_termination_includes_cosmetic_children_before_the_end_spec() {
    let mut payload = vec![0; 600];
    let anchor = 350;
    payload[anchor..anchor + 2].copy_from_slice(&[0x20, 0x86]);
    payload[anchor + 4..anchor + 8].copy_from_slice(&1u32.to_le_bytes());
    payload[anchor + 18..anchor + 22].copy_from_slice(&1u32.to_le_bytes());
    payload[anchor + 30..anchor + 34].copy_from_slice(&[1, 0, 0, 1]);
    payload[anchor + 92] = 1;

    let feature = |id: &str, source_id: &str, input_class: &str| Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some(source_id.into()),
        parent_source_id: None,
        ordinal: source_id.parse().expect("required invariant"),
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
    let mut histories = vec![FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![
            feature("extrusion", "10", "moICE_c"),
            feature("cosmetic", "11", "moCosmeticThread_c"),
            feature("next", "12", "Chamfer_c"),
        ],
    }];
    let lane = FeatureInputLane {
        id: "lane#7".into(),
        configuration: None,
        native_payload: payload,
        classes: Vec::new(),
        names: vec![
            FeatureInputName {
                id: "extrusion-name".into(),
                parent: "lane#7".into(),
                ordinal: 0,
                offset: 10,
                value: "extrusion".into(),
                object_id: Some(10),
            },
            FeatureInputName {
                id: "cosmetic-name".into(),
                parent: "lane#7".into(),
                ordinal: 1,
                offset: 200,
                value: "cosmetic".into(),
                object_id: Some(11),
            },
            FeatureInputName {
                id: "next-name".into(),
                parent: "lane#7".into(),
                ordinal: 2,
                offset: 500,
                value: "next".into(),
                object_id: Some(12),
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

    enrich_history_extrusion_terminations(&mut histories, std::slice::from_ref(&lane));

    assert_eq!(
        histories[0].features[0].properties.get("EndCondition"),
        Some(&"ThroughAll".to_string())
    );
}

#[test]
fn compact_extrusion_to_face_preserves_an_unparsed_framed_face_path() {
    let mut payload = vec![0; 240];
    payload[..2].copy_from_slice(&[0x95, 0x81]);
    payload[4] = 1;
    payload[18] = 4;
    payload[30..33].copy_from_slice(&[1, 1, 0]);
    payload[33..46].copy_from_slice(&[0x54, 0x89, 0x30, 0x80, 0x2e, 0x80, 2, 0, 0, 0, 0x40, 0, 0]);
    let marker = 140;
    payload[marker - 12..marker - 8].copy_from_slice(&6u32.to_le_bytes());
    payload[marker - 8..marker - 4].copy_from_slice(&[0, 2, 0, 0]);
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);

    assert_eq!(
        compact_extrusion_to_face_at(&payload, 0, payload.len()),
        Some(marker)
    );
    payload[marker - 12..marker - 8].fill(0);
    assert_eq!(
        compact_extrusion_to_face_at(&payload, 0, payload.len()),
        None
    );
}

#[test]
fn termination_consensus_uses_stable_reference_identity_across_lanes() {
    let vote = |reference: &str, identity: &str| super::TerminationVote {
        condition: "ToFace".into(),
        reference: Some(reference.into()),
        second_condition: None,
        reference_identity: Some(identity.into()),
        canonical_reference: Some("components:1,2,3".into()),
        depth_m: None,
    };
    let first = vote("lane-0:100", "components:1,2,3");
    let second = vote("lane-1:200", "components:1,2,3");
    let consensus =
        super::consensus_termination_vote(&[Some(first.clone()), Some(second)]).unwrap();
    assert_eq!(consensus.reference.as_deref(), Some("components:1,2,3"));

    let exact = super::consensus_termination_vote(&[Some(first.clone())]).unwrap();
    assert_eq!(exact.reference, first.reference);
    assert!(super::consensus_termination_vote(&[
        Some(first),
        Some(vote("lane-1:200", "components:1,2,4")),
    ])
    .is_none());

    let mut first_depth = vote("lane-0:100", "components:1,2,3");
    first_depth.depth_m = Some(0.01);
    let mut second_depth = vote("lane-1:200", "components:1,2,3");
    second_depth.depth_m = Some(0.02);
    assert!(super::consensus_termination_vote(&[Some(first_depth), Some(second_depth),]).is_none());
}

#[test]
fn compact_extrusion_to_face_accepts_the_long_declared_face_path() {
    let end_spec = b"\xff\xff\x01\x00\x0b\x00moEndSpec_c";
    let face_ref = b"\xff\xff\x01\x00\x11\x00moSingleFaceRef_w";
    let mut payload = vec![0; 360];
    payload[..end_spec.len()].copy_from_slice(end_spec);
    let anchor = end_spec.len() - 2;
    payload[anchor + 4..anchor + 8].copy_from_slice(&1u32.to_le_bytes());
    payload[anchor + 18..anchor + 22].copy_from_slice(&4u32.to_le_bytes());
    payload[anchor + 30..anchor + 33].copy_from_slice(&[1, 1, 0]);
    let child = anchor + 33;
    payload[child..child + face_ref.len()].copy_from_slice(face_ref);
    let body = child + face_ref.len();
    let marker = body + 260;
    payload.truncate(marker - 12);
    assert_eq!(selection_vector_tail(&mut payload, &[8, 5, 4]), marker);

    let boundary = payload.len();
    assert_eq!(
        compact_extrusion_to_face_at(&payload, anchor, boundary),
        Some(marker)
    );

    let second = selection_vector_tail(&mut payload, &[3]);
    assert!(second > marker);
    assert_eq!(
        compact_extrusion_to_face_at(&payload, anchor, boundary),
        Some(marker)
    );
    assert_eq!(
        compact_extrusion_to_face_at(&payload, anchor, payload.len()),
        None
    );

    payload[marker - 8] = 1;
    payload[second - 8] = 1;
    assert_eq!(
        compact_extrusion_to_face_at(&payload, anchor, payload.len()),
        None
    );
}

#[test]
fn compact_extrusion_to_face_accepts_extended_legacy_face_path_padding() {
    let mut payload = vec![0; 300];
    payload[..2].copy_from_slice(&[0x34, 0x80]);
    payload[4] = 1;
    payload[18] = 4;
    payload[30..33].copy_from_slice(&[1, 1, 0]);
    let declaration = b"\xff\xff\x01\x00\x11\x00moSingleFaceRef_w";
    payload[33..33 + declaration.len()].copy_from_slice(declaration);
    let body = 33 + declaration.len();
    payload[body..body + 19].copy_from_slice(&[
        0x30, 0x80, 0x2e, 0x80, 2, 0, 0, 0, 0x40, 0, 0, 108, 0, 0, 0, 108, 0, 0, 0,
    ]);
    payload[body + 47..body + 63].fill(0xff);
    let control = body + 84;
    payload[control..control + 2].copy_from_slice(&[0x33, 0x80]);
    payload[control + 2..control + 6].copy_from_slice(&1u32.to_le_bytes());
    payload[control + 10..control + 14].copy_from_slice(&5u32.to_le_bytes());
    payload[control + 14..control + 18].copy_from_slice(&[0, 3, 0, 0]);
    payload[control + 22..control + 30].copy_from_slice(&[1; 8]);
    payload[control + 30..control + 38].copy_from_slice(&[1; 8]);
    let entries = [control + 40, control + 64, control + 90];
    for (entry, local_id) in entries.into_iter().zip([3u32, 2, 4]) {
        payload[entry..entry + 4].copy_from_slice(&[0x4c, 0x80, 0, 0]);
        payload[entry + 4..entry + 16].copy_from_slice(&[2; 12]);
        payload[entry + 16..entry + 20].copy_from_slice(&local_id.to_le_bytes());
        payload[entry + 20..entry + 24].copy_from_slice(&33u32.to_le_bytes());
    }
    let terminal = entries[2] + 24;
    payload[terminal..terminal + 24].fill(0);
    payload[terminal + 24..terminal + 28].copy_from_slice(&101u32.to_le_bytes());

    assert_eq!(
        compact_extrusion_to_face_at(&payload, 0, payload.len()),
        Some(body)
    );
    let path = legacy_single_face_reference_path_at(&payload, body).expect("required invariant");
    assert_eq!(
        path.iter().map(|entry| entry.local_id).collect::<Vec<_>>(),
        [Some(3), Some(2), Some(4)]
    );
    payload[body + 47] = 0xfe;
    assert_eq!(
        compact_extrusion_to_face_at(&payload, 0, payload.len()),
        None
    );
}

#[test]
fn compact_extrusion_through_next_shares_the_traversal_tail() {
    let mut payload = vec![0; 104];
    payload[..2].copy_from_slice(&[0x0c, 0x8e]);
    payload[4] = 1;
    payload[18] = 2;
    payload[30..34].copy_from_slice(&[1, 0, 0, 1]);
    payload[92] = 1;
    assert!(compact_extrusion_through_next_at(&payload, 0));
    assert!(!compact_extrusion_through_all_at(&payload, 0));

    payload[18] = 1;
    assert!(compact_extrusion_through_all_at(&payload, 0));
    assert!(!compact_extrusion_through_next_at(&payload, 0));
    payload[18] = 2;
    payload[103] = 1;
    assert!(!compact_extrusion_through_next_at(&payload, 0));

    payload[103] = 0;
    payload[92] = 0;
    payload[90] = 1;
    assert!(compact_extrusion_through_next_at(&payload, 0));

    payload.resize(108, 0);
    payload[100..102].copy_from_slice(&[0x83, 0x81]);
    payload[102..106].copy_from_slice(&5u32.to_le_bytes());
    payload[106..108].copy_from_slice(&[0x74, 0x81]);
    payload.resize(108 + 16, 0);
    payload.extend_from_slice(&[0xff, 0xff, 0, 0, 1]);
    payload.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);
    payload.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0x80, 0xbf]);
    assert!(compact_extrusion_through_next_at(&payload, 0));
}

#[test]
fn compact_extrusion_through_all_accepts_a_retained_dimension_child() {
    let mut payload = vec![0; 26];
    payload[..2].copy_from_slice(&[0x0c, 0x8e]);
    payload[4] = 1;
    payload[18] = 1;
    payload.extend_from_slice(b"\xff\xff\x01\x00\x16\x00moDisplayDistanceDim_c");
    let block = payload.len();
    payload.resize(block + 16, 0);
    payload[block + 9] = 0x20;
    payload.extend_from_slice(&[0xff, 0xff, 0, 0, 3]);
    payload.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);
    payload.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0x80, 0xbf]);

    assert!(compact_extrusion_through_all_at(&payload, 0));
    payload[22] = 1;
    assert!(!compact_extrusion_through_all_at(&payload, 0));
}

#[test]
fn compact_extrusion_through_all_accepts_a_dimensioned_traversal_body() {
    let mut payload = vec![0; 68];
    payload[..2].copy_from_slice(&[0x0c, 0x8e]);
    payload[4] = 1;
    payload[18] = 1;
    payload[30..34].copy_from_slice(&[1, 0, 0, 1]);
    payload[44..48].copy_from_slice(&1u32.to_le_bytes());
    payload.extend_from_slice(&[0x77, 0x83]);
    let block = payload.len();
    payload.resize(block + 16, 0);
    payload[block + 8] = 0x40;
    payload[block + 9] = 0x28;
    payload.extend_from_slice(&[0xff, 0xff, 0, 0, 1]);
    payload.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);
    payload.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0x80, 0xbf]);

    assert!(compact_extrusion_through_all_at(&payload, 0));
    payload[44] = 0;
    assert!(!compact_extrusion_through_all_at(&payload, 0));
}

#[test]
fn compact_extrusion_mid_plane_requires_the_dimension_child() {
    let dimension_tail = |payload: &mut Vec<u8>| {
        let block = payload.len();
        payload.resize(block + 16, 0);
        payload[block + 9] = 0x20;
        payload.extend_from_slice(&[0xff, 0xff, 0, 0, 3]);
        payload.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);
        payload.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0x80, 0xbf]);
    };

    let mut payload = vec![0; 26];
    payload[..2].copy_from_slice(&[0x0c, 0x8e]);
    payload[4] = 1;
    payload[18] = 6;
    payload.extend_from_slice(&[0x6a, 0x81]);
    dimension_tail(&mut payload);
    assert!(compact_extrusion_mid_plane_at(&payload, 0));

    payload[18] = 5;
    assert!(!compact_extrusion_mid_plane_at(&payload, 0));
    payload[18] = 6;
    let last = payload.len() - 1;
    payload[last] = 0;
    assert!(!compact_extrusion_mid_plane_at(&payload, 0));

    let mut payload = vec![0; 26];
    payload[..2].copy_from_slice(&[0x0c, 0x8e]);
    payload[4] = 1;
    payload[18] = 6;
    payload.extend_from_slice(b"\xff\xff\x01\x00\x16\x00moDisplayDistanceDim_c");
    dimension_tail(&mut payload);
    assert!(compact_extrusion_mid_plane_at(&payload, 0));
}

#[test]
fn compact_extrusion_blind_requires_code_zero_and_the_dimension_child() {
    let mut payload = vec![0; 26];
    payload[..2].copy_from_slice(&[0x0c, 0x8e]);
    payload[4] = 1;
    payload.extend_from_slice(b"\xff\xff\x01\x00\x16\x00moDisplayDistanceDim_c");
    let block = payload.len();
    payload.resize(block + 16, 0);
    payload[block + 9] = 0x20;
    payload.extend_from_slice(&[0xff, 0xff, 0, 0, 3]);
    payload.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);
    payload.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0x80, 0xbf]);

    assert!(compact_extrusion_blind_at(&payload, 0));
    payload[block + 8] = 0x40;
    assert!(compact_extrusion_blind_at(&payload, 0));
    payload[18] = 1;
    assert!(!compact_extrusion_blind_at(&payload, 0));
    payload[18] = 0;
    payload[22] = 1;
    assert!(!compact_extrusion_blind_at(&payload, 0));

    let mut compact = payload[..22].to_vec();
    compact.extend_from_slice(&payload[26..]);
    assert!(compact_extrusion_blind_at(&compact, 0));
}

#[test]
fn compact_extrusion_through_all_both_accepts_both_carriers() {
    let dimension_tail = |payload: &mut Vec<u8>| {
        let block = payload.len();
        payload.resize(block + 16, 0);
        payload[block + 9] = 0x20;
        payload.extend_from_slice(&[0xff, 0xff, 0, 0, 3]);
        payload.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);
        payload.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0x80, 0xbf]);
    };

    // Traversal carrier: first-direction code 1 with second-direction 1.
    let mut payload = vec![0; 104];
    payload[..2].copy_from_slice(&[0x0c, 0x8e]);
    payload[18] = 1;
    payload[22] = 1;
    payload[30..34].copy_from_slice(&[1, 0, 0, 1]);
    payload[92] = 1;
    assert!(compact_extrusion_through_all_both_at(&payload, 0));
    assert!(!compact_extrusion_through_all_at(&payload, 0));
    payload[8] = 1;
    assert!(compact_extrusion_through_all_both_at(&payload, 0));
    payload[8] = 2;
    assert!(!compact_extrusion_through_all_both_at(&payload, 0));
    payload[8] = 0;
    payload[22] = 0;
    assert!(!compact_extrusion_through_all_both_at(&payload, 0));
    payload[22] = 1;
    payload[18] = 2;
    assert!(!compact_extrusion_through_all_both_at(&payload, 0));

    // Dedicated code 9 carrier with the retained dimension child.
    let mut payload = vec![0; 26];
    payload[..2].copy_from_slice(&[0x0c, 0x8e]);
    payload[18] = 9;
    payload[22] = 1;
    payload.extend_from_slice(&[0x6a, 0x81]);
    dimension_tail(&mut payload);
    assert!(compact_extrusion_through_all_both_at(&payload, 0));
    payload[22] = 0;
    assert!(!compact_extrusion_through_all_both_at(&payload, 0));
    payload[22] = 1;
    payload[4] = 2;
    assert!(!compact_extrusion_through_all_both_at(&payload, 0));
}

#[test]
fn compact_extrusion_blind_second_direction_requires_the_dimension_child() {
    let mut payload = vec![0; 26];
    payload[..2].copy_from_slice(&[0x0c, 0x8e]);
    payload[4] = 1;
    payload[22] = 1;
    payload.extend_from_slice(&[0x6a, 0x81]);
    let block = payload.len();
    payload.resize(block + 16, 0);
    payload[block + 9] = 0x20;
    payload.extend_from_slice(&[0xff, 0xff, 0, 0, 3]);
    payload.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);
    payload.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0x80, 0xbf]);
    assert!(compact_extrusion_blind_through_all_second_at(&payload, 0));
    assert!(!compact_extrusion_through_all_both_at(&payload, 0));
    payload[22] = 0;
    assert!(!compact_extrusion_blind_through_all_second_at(&payload, 0));
    payload[22] = 1;
    payload[4] = 0;
    assert!(!compact_extrusion_blind_through_all_second_at(&payload, 0));
    payload[4] = 1;
    let last = payload.len() - 1;
    payload[last] = 0;
    assert!(!compact_extrusion_blind_through_all_second_at(&payload, 0));
}

#[test]
fn end_spec_headers_require_the_anchor_class_identity() {
    let mut payload = vec![0; 104];
    payload[4] = 1;
    payload[18] = 1;
    payload[30..34].copy_from_slice(&[1, 0, 0, 1]);
    payload[92] = 1;
    // Header-shaped run without a class token or declaration at the anchor
    // is a fillet edge-set impostor, not an end spec.
    assert!(!compact_extrusion_through_all_at(&payload, 0));
    payload[..2].copy_from_slice(&[0x0c, 0x8e]);
    assert!(compact_extrusion_through_all_at(&payload, 0));
    payload[..2].copy_from_slice(&[0xff, 0xff]);
    assert!(!compact_extrusion_through_all_at(&payload, 0));

    let mut payload = vec![0; 15];
    payload.extend_from_slice(&[0; 104]);
    payload[15 + 4] = 1;
    payload[15 + 18] = 1;
    payload[15 + 30..15 + 34].copy_from_slice(&[1, 0, 0, 1]);
    payload[15 + 92] = 1;
    assert!(!compact_extrusion_through_all_at(&payload, 15));
    payload[..17].copy_from_slice(b"\xff\xff\x01\x00\x0b\x00moEndSpec_c");
    assert!(compact_extrusion_through_all_at(&payload, 15));
}

#[test]
fn legacy_single_face_reference_requires_a_unique_counted_path() {
    let mut payload = vec![0; 128];
    payload[0..4].copy_from_slice(&[0x53, 0x81, 0x80, 0x80]);
    payload[4..8].copy_from_slice(&2u32.to_le_bytes());
    payload[11..15].copy_from_slice(&101u32.to_le_bytes());
    payload[15..19].copy_from_slice(&101u32.to_le_bytes());

    let control = 44;
    payload[control..control + 2].copy_from_slice(&[0x1e, 0x81]);
    payload[control + 2..control + 6].copy_from_slice(&1u32.to_le_bytes());
    payload[control + 10..control + 14].copy_from_slice(&1u32.to_le_bytes());
    payload[control + 14..control + 18].copy_from_slice(&[0, 2, 0, 0]);
    payload[control + 22..control + 30].copy_from_slice(&[1; 8]);
    payload[control + 30..control + 38].copy_from_slice(&[1; 8]);

    let entry = control + 40;
    payload[entry..entry + 4].copy_from_slice(&[0x32, 0x80, 0, 0]);
    payload[entry + 4..entry + 16].copy_from_slice(&[1; 12]);
    payload[entry + 16..entry + 20].copy_from_slice(&7u32.to_le_bytes());
    payload[entry + 20..entry + 28].copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]);

    let path = legacy_single_face_reference_path_at(&payload, 0).expect("required invariant");
    assert_eq!(path.len(), 1);
    assert_eq!(path[0].instance, Some(0x8032));
    assert_eq!(path[0].type_signature, [1; 12]);
    assert_eq!(path[0].local_id, Some(7));

    payload[entry + 1] = 0;
    assert_eq!(legacy_single_face_reference_path_at(&payload, 0), None);
    payload[entry + 1] = 0x80;
    payload[control + 30] = 2;
    assert_eq!(legacy_single_face_reference_path_at(&payload, 0), None);
}

#[test]
fn compact_extrusion_to_vertex_accepts_both_point_reference_forms() {
    // Variant A, repeated-token form.
    let mut payload = vec![0; 30];
    payload[..2].copy_from_slice(&[0x0c, 0x8e]);
    payload[4] = 1;
    payload[18] = 3;
    payload.extend_from_slice(&[0x82, 0x92, 0x2b, 0x80, 2, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0; 12]);
    let marker = selection_vector_tail(&mut payload, &[4, 7]);
    let (found, kind) =
        compact_extrusion_to_vertex_at(&payload, 0, payload.len()).expect("required invariant");
    assert_eq!(found, marker);
    assert_eq!(kind, CompactPointReferenceKind::Point);
    let path = compact_single_face_reference_path_at(&payload, marker).expect("required invariant");
    assert_eq!(path.last().expect("required invariant").local_id, Some(7));

    // A to-face selector byte is not a point reference.
    payload[38] = 0x40;
    assert_eq!(
        compact_extrusion_to_vertex_at(&payload, 0, payload.len()),
        None
    );
    payload[38] = 0;
    payload[18] = 4;
    assert_eq!(
        compact_extrusion_to_vertex_at(&payload, 0, payload.len()),
        None
    );
    payload[18] = 3;

    // Variant B, edge endpoint reference.
    let mut payload = vec![0; 30];
    payload[..2].copy_from_slice(&[0x0c, 0x8e]);
    payload[4] = 1;
    payload[18] = 3;
    payload.extend_from_slice(b"\xff\xff\x01\x00\x0f\x00moEndPointRef_w");
    payload.extend_from_slice(b"\xff\xff\x01\x00\x0c\x00moCompEdge_c");
    payload.extend_from_slice(&[0xcb, 0x80, 2, 0, 0, 0, 0x40, 0, 0]);
    payload.extend_from_slice(&[0; 12]);
    let marker = selection_vector_tail(&mut payload, &[2]);
    let (found, kind) =
        compact_extrusion_to_vertex_at(&payload, 0, payload.len()).expect("required invariant");
    assert_eq!(found, marker);
    assert_eq!(kind, CompactPointReferenceKind::EdgeEndpoint);
}

#[test]
fn compact_extrusion_to_vertex_requires_one_reference_in_the_feature_interval() {
    let mut payload = vec![0; 30];
    payload[..2].copy_from_slice(&[0x0c, 0x8e]);
    payload[4] = 1;
    payload[18] = 3;
    payload.extend_from_slice(&[0x82, 0x92, 0x2b, 0x80, 2, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0; 12]);
    payload.resize(280, 0);
    let marker = selection_vector_tail(&mut payload, &[4, 7]);
    assert!(marker > 270);
    assert_eq!(
        compact_extrusion_to_vertex_at(&payload, 0, payload.len()).map(|(found, _)| found),
        Some(marker)
    );

    let boundary = payload.len();
    let second = selection_vector_tail(&mut payload, &[5, 8]);
    assert!(second > marker);
    assert_eq!(
        compact_extrusion_to_vertex_at(&payload, 0, boundary).map(|(found, _)| found),
        Some(marker)
    );
    assert_eq!(
        compact_extrusion_to_vertex_at(&payload, 0, payload.len()),
        None
    );
}

#[test]
fn compact_extrusion_offset_from_face_requires_the_late_face_reference() {
    let mut payload = vec![0; 26];
    payload[..2].copy_from_slice(&[0x0c, 0x8e]);
    payload[4] = 1;
    payload[18] = 5;
    payload.extend_from_slice(&[0x6a, 0x81]);
    let block = payload.len();
    payload.resize(block + 16, 0);
    payload[block + 9] = 0x20;
    payload.extend_from_slice(&[0xff, 0xff, 0, 0, 3]);
    payload.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);
    payload.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0x80, 0xbf]);
    payload.extend_from_slice(&[0; 40]);
    payload.extend_from_slice(&[1, 1, 0]);
    payload.extend_from_slice(b"\xff\xff\x01\x00\x11\x00moSingleFaceRef_w");
    payload.extend_from_slice(&[0xf2, 0x82, 0xe6, 0x80, 2, 0, 0, 0, 0x40, 0, 0]);
    payload.extend_from_slice(&[0; 8]);
    let marker = selection_vector_tail(&mut payload, &[9]);
    let end = payload.len();
    assert_eq!(
        compact_extrusion_offset_from_face_at(&payload, 0, end),
        Some(marker)
    );

    payload.extend_from_slice(&[1, 1, 0]);
    payload.extend_from_slice(&[0xf2, 0x82, 0xe6, 0x80, 2, 0, 0, 0, 0x40, 0, 0]);
    payload.extend_from_slice(&[0; 8]);
    payload.resize(payload.len() + 240, 0);
    let second = selection_vector_tail(&mut payload, &[10]);
    assert!(second > marker);
    assert!(second > marker + 200);
    assert_eq!(
        compact_extrusion_offset_from_face_at(&payload, 0, end),
        Some(marker)
    );
    assert_eq!(
        compact_extrusion_offset_from_face_at(&payload, 0, payload.len()),
        None
    );

    // Wrong code or a missing face-reference anchor yields no detection.
    payload[18] = 6;
    assert_eq!(
        compact_extrusion_offset_from_face_at(&payload, 0, end),
        None
    );
    payload[18] = 5;
    let anchor = payload
        .windows(3)
        .position(|window| window == [1, 1, 0])
        .expect("required invariant");
    payload[anchor] = 0;
    assert_eq!(
        compact_extrusion_offset_from_face_at(&payload, 0, end),
        None
    );
}

#[test]
fn compact_body_path_requires_type_three_vector() {
    let marker = 12;
    let mut payload = vec![0; 100];
    payload[..4].copy_from_slice(&2u32.to_le_bytes());
    payload[4..8].copy_from_slice(&[0, 3, 0, 0]);
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    let first = marker + 18;
    payload[first..first + 4].copy_from_slice(&[0x32, 0x80, 0, 0]);
    payload[first + 4..first + 16].copy_from_slice(&[1; 12]);
    payload[first + 16..first + 20].copy_from_slice(&6u32.to_le_bytes());
    let second = first + 28;
    payload[second..second + 4].copy_from_slice(&[0x3b, 0x80, 0, 0]);
    payload[second + 4..second + 16].copy_from_slice(&[2; 12]);
    payload[second + 16..second + 20].copy_from_slice(&7u32.to_le_bytes());
    assert_eq!(compact_body_path_at(&payload, marker), Some(vec![6, 7]));
    assert_eq!(
        compact_body_component_path_at(&payload, marker).map(|components| components.len()),
        Some(2)
    );

    payload[..4].copy_from_slice(&3u32.to_le_bytes());
    payload[second + 20..second + 28].copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]);
    assert_eq!(compact_body_path_at(&payload, marker), Some(vec![6, 7]));

    payload[second + 20..second + 30].copy_from_slice(&[0; 10]);
    assert_eq!(compact_body_path_at(&payload, marker), Some(vec![6, 7]));
    assert_eq!(
        compact_body_component_path_at(&payload, marker).map(|components| components.len()),
        Some(2)
    );
    payload[second + 24] = 1;
    assert_eq!(compact_body_path_at(&payload, marker), None);

    payload[4] = 2;
    assert_eq!(compact_body_path_at(&payload, marker), None);
}

#[test]
fn compact_body_path_accepts_anonymous_mixed_entries() {
    let marker = 12;
    let mut payload = vec![0; marker + 18];
    payload[..4].copy_from_slice(&3u32.to_le_bytes());
    payload[4..8].copy_from_slice(&[0, 3, 0, 0]);
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);

    payload.extend_from_slice(&0x803eu16.to_le_bytes());
    payload.extend_from_slice(&[0, 0]);
    payload.extend_from_slice(&[0x34, 0x80, 0x37, 0, 37, 0, 0, 0, 0x7a, 0x83, 0xd9, 0x4a]);
    payload.extend_from_slice(&2u32.to_le_bytes());
    payload.extend_from_slice(&[5, 0, 0, 0]);
    payload.extend_from_slice(&[0x34, 0x80, 0x37, 0, 50, 0, 0, 0, 0xf9, 0x83, 0xd9, 0x4a]);
    payload.extend_from_slice(&1u32.to_le_bytes());
    payload.extend_from_slice(&[0, 0, 0, 0]);
    payload.extend_from_slice(&0x8263u16.to_le_bytes());
    payload.extend_from_slice(&[0, 0]);
    payload.extend_from_slice(&[0x34, 0x80, 0x37, 0, 37, 0, 0, 0, 0x7a, 0x83, 0xd9, 0x4a]);
    payload.extend_from_slice(&3u32.to_le_bytes());

    assert_eq!(compact_body_path_at(&payload, marker), Some(vec![2, 1, 3]));

    payload[..4].copy_from_slice(&4u32.to_le_bytes());
    payload.extend_from_slice(&[0; 10]);
    assert_eq!(compact_body_path_at(&payload, marker), Some(vec![2, 1, 3]));
    let last = payload.len() - 1;
    payload[last] = 1;
    assert_eq!(compact_body_path_at(&payload, marker), None);
}

#[test]
fn compact_body_component_path_accepts_counted_sentinel_separators() {
    let marker = 12;
    let mut payload = vec![0; marker + 18];
    payload[..4].copy_from_slice(&10u32.to_le_bytes());
    payload[4..8].copy_from_slice(&[0, 3, 0, 0]);
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);

    let signature = |source: u32, timestamp: u32| {
        let mut signature = vec![0x38, 0x80, 0x3b, 0];
        signature.extend_from_slice(&source.to_le_bytes());
        signature.extend_from_slice(&timestamp.to_le_bytes());
        signature
    };
    let entry = |payload: &mut Vec<u8>, instance: u16, source: u32, timestamp: u32| {
        payload.extend_from_slice(&instance.to_le_bytes());
        payload.extend_from_slice(&[0, 0]);
        payload.extend_from_slice(&signature(source, timestamp));
    };
    let local = |payload: &mut Vec<u8>, value: u32| {
        payload.extend_from_slice(&value.to_le_bytes());
        payload.extend_from_slice(&[0xff; 4]);
        payload.extend_from_slice(&[0; 4]);
    };

    entry(&mut payload, 0x8521, 213, 1);
    local(&mut payload, 2);
    entry(&mut payload, 0x8521, 213, 1);
    local(&mut payload, 9);
    entry(&mut payload, 0x8083, 252, 2);
    local(&mut payload, 1);
    entry(&mut payload, 0x8041, 265, 3);
    local(&mut payload, 1);
    entry(&mut payload, 0x8083, 213, 1);
    local(&mut payload, 1);
    entry(&mut payload, 0x8036, 298, 4);
    entry(&mut payload, 0x8041, 265, 5);
    entry(&mut payload, 0x8036, 298, 6);
    entry(&mut payload, 0x8083, 252, 7);
    local(&mut payload, 1);
    entry(&mut payload, 0x8521, 213, 1);
    local(&mut payload, 8);

    let components = compact_body_component_path_at(&payload, marker)
        .expect("counted lineage path with sentinel separators");
    assert_eq!(components.len(), 10);
    assert_eq!(components[5].local_id, None);
    assert_eq!(components[6].local_id, None);
    assert_eq!(components[7].local_id, None);
    assert_eq!(components[8].local_id, Some(1));
    assert_eq!(components[9].local_id, Some(8));
}

#[test]
fn enrich_combine_uses_outermost_body_paths() {
    let mut payload = vec![0; 420];
    for (marker, local_id) in [(100usize, 1u32), (200, 2), (300, 3)] {
        payload[marker - 12..marker - 8].copy_from_slice(&1u32.to_le_bytes());
        payload[marker - 8..marker - 4].copy_from_slice(&[0, 3, 0, 0]);
        payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
        payload[marker + 16..marker + 18].copy_from_slice(&[0, 0]);
        payload[marker + 18..marker + 20].copy_from_slice(&0x8032u16.to_le_bytes());
        payload[marker + 22..marker + 34].copy_from_slice(&[1; 12]);
        payload[marker + 34..marker + 38].copy_from_slice(&local_id.to_le_bytes());
    }
    let mut histories = vec![FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![Feature {
            id: "combine".into(),
            parent: "history".into(),
            xml_tag: "Feature".into(),
            tree_parent: None,
            source_id: Some("119".into()),
            parent_source_id: None,
            ordinal: 0,
            name: "Combine".into(),
            kind: "Combine".into(),
            input_class: Some("moCombineBodies_c".into()),
            suppressed: false,
            parameters: BTreeMap::new(),
            dimension_properties: BTreeMap::new(),
            properties: BTreeMap::new(),
            text: None,
            content: Vec::new(),
        }],
    }];
    let lanes = [FeatureInputLane {
        id: "lane#35".into(),
        configuration: None,
        native_payload: payload,
        classes: Vec::new(),
        names: vec![FeatureInputName {
            id: "combine-name".into(),
            parent: "lane#35".into(),
            ordinal: 0,
            offset: 0,
            object_id: Some(119),
            value: "Combine".into(),
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
    }];

    enrich_history_combine_selections(&mut histories, &lanes);

    let properties = &histories[0].features[0].properties;
    assert_eq!(
        properties.get("Target"),
        Some(&"sldprt:feature-input:body-path:35:100".to_string())
    );
    assert_eq!(
        properties.get("Tools"),
        Some(&"sldprt:feature-input:body-path:35:300".to_string())
    );
}

#[test]
fn compact_combine_operation_is_name_length_relative() {
    let offset = 7;
    let mut payload = vec![0; 180];
    payload[offset..offset + 5].copy_from_slice(&[0x04, 0x80, 0xff, 0xfe, 0xff]);
    payload[offset + 5] = 8;
    let operation = offset + 117 + 16;
    payload[operation..operation + 4].copy_from_slice(&2u32.to_le_bytes());
    payload[operation + 10..operation + 14].copy_from_slice(&[0xff; 4]);
    assert_eq!(
        compact_combine_operation_at(&payload, offset),
        Some("Intersect")
    );
    payload[operation - 1] = 1;
    assert_eq!(compact_combine_operation_at(&payload, offset), None);

    let offset = 11;
    let mut tokenized = vec![0; 180];
    tokenized[offset..offset + 5].copy_from_slice(&[0xe3, 0x85, 0xff, 0xfe, 0xff]);
    tokenized[offset + 5] = 8;
    let operation = offset + 117 + 16;
    tokenized[operation + 4..operation + 10].copy_from_slice(&[0, 0, 0xff, 0xff, 0xff, 0xff]);
    assert_eq!(
        compact_combine_operation_at(&tokenized, offset),
        Some("Join")
    );
    tokenized[operation + 9] = 0;
    assert_eq!(compact_combine_operation_at(&tokenized, offset), None);
}
