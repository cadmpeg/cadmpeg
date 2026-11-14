//! The shared start predicate, the compact start reader and the extrusion
//! enrichment that records the start.

use super::{
    common_condition, condition, extrude_start, insert_condition, object_condition, start_type_at,
    StartCondition, BLOCK_HEADER, DECLARATION, SHORT_BLOCK_LEN, START_CONDITION, WORD_BLOCK_LEN,
};
use crate::records::{
    Feature, FeatureHistory, FeatureInputLane, FeatureInputName, FeatureSource, ObjectId,
};
use crate::resolved_features::terminations::enrich_history_extrusion_terminations;
use cadmpeg_ir::features::ExtrudeStart;
use std::collections::BTreeMap;

/// Where the extrusion's name record starts in the synthetic payload.
const EXTRUSION_NAME_AT: usize = 10;
/// Where the extrusion's block starts.
const BLOCK_AT: usize = 100;
/// A class token the synthetic start object uses.
const START_TOKEN: [u8; 2] = [0xb0, 0x83];

/// A display-dimension child written with a class token.
fn dimension_child() -> Vec<u8> {
    let mut child = vec![0x4f, 0x82];
    child.extend_from_slice(&[0; 16]);
    child.extend_from_slice(&[0xff, 0xff, 0, 0, 1]);
    child.extend_from_slice(&[0xff; 4]);
    child.extend_from_slice(&[0; 4]);
    child.extend_from_slice(&(-1.0f32).to_le_bytes());
    child
}

/// The block of an extrusion: with its u32 word (76 bytes) or without (72).
fn block(with_word: bool) -> Vec<u8> {
    let mut bytes = BLOCK_HEADER.to_vec();
    if with_word {
        bytes.extend_from_slice(&[0; 56]);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 8]);
        assert_eq!(bytes.len(), WORD_BLOCK_LEN);
    } else {
        bytes.extend_from_slice(&[0; 64]);
        assert_eq!(bytes.len(), SHORT_BLOCK_LEN);
    }
    bytes
}

/// An extrusion payload: padding, the block, and a start object of `kind`
/// that names its class by token or by declaration. A nonzero type word is
/// followed by a display dimension.
fn payload(kind: u32, declared: bool, with_word: bool) -> Vec<u8> {
    let mut payload = vec![0; BLOCK_AT];
    payload.extend(block(with_word));
    if declared {
        payload.extend_from_slice(DECLARATION);
    } else {
        payload.extend_from_slice(&START_TOKEN);
    }
    if kind == 0 {
        payload.extend_from_slice(&[0; 40]);
    } else {
        payload.extend_from_slice(&kind.to_le_bytes());
        payload.extend(dimension_child());
        payload.extend_from_slice(&[0; 300]);
    }
    payload.extend_from_slice(&[0; 40]);
    payload
}

fn condition_of(payload: &[u8]) -> Option<StartCondition> {
    object_condition(payload, 0..payload.len())
}

/// The history properties the enrichment gives the extrusion in `payload`.
fn enriched(payload: Vec<u8>) -> BTreeMap<cadmpeg_core::text::NonBlankString, String> {
    let feature = Feature {
        id: "extrusion".into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: FeatureSource::from_value(10),
        ordinal: 10,
        name: "extrusion".into(),
        kind: "Boss-Extrude".into(),
        input_class: Some("moICE_c".into()),
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
        features: vec![feature],
    }];
    let lane = FeatureInputLane {
        id: "lane#7".into(),
        configuration: None,
        native_payload: payload.into(),
        classes: Vec::new(),
        names: vec![FeatureInputName {
            id: "extrusion-name".into(),
            parent: "lane#7".into(),
            ordinal: 0,
            offset: u64::try_from(EXTRUSION_NAME_AT).unwrap(),
            value: "extrusion".into(),
            object_id: ObjectId::from_value(10),
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
        &lane.native_payload,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    enrich_history_extrusion_terminations(&ctx, &mut histories, std::slice::from_ref(&lane))
        .unwrap();
    histories[0].features[0].properties.clone()
}

#[test]
fn a_profile_plane_start_is_type_zero() {
    for declared in [false, true] {
        for with_word in [false, true] {
            let payload = payload(0, declared, with_word);
            assert_eq!(condition_of(&payload), Some(StartCondition::ProfilePlane));
            let properties = enriched(payload);
            assert!(!properties.contains_key(START_CONDITION));
            assert_eq!(extrude_start(&properties), ExtrudeStart::ProfilePlane {});
        }
    }
}

#[test]
fn an_offset_plane_start_is_unresolved() {
    for declared in [false, true] {
        for with_word in [false, true] {
            let payload = payload(5, declared, with_word);
            assert_eq!(condition_of(&payload), Some(StartCondition::Unresolved));
            let properties = enriched(payload);
            assert_eq!(
                properties.get(START_CONDITION).map(String::as_str),
                Some("Unresolved")
            );
            assert_eq!(extrude_start(&properties), ExtrudeStart::Unresolved {});
        }
    }
}

#[test]
fn an_unknown_start_type_is_unresolved() {
    for kind in [1, 2, 3, 4, 6, 99] {
        let payload = payload(kind, false, true);
        assert_eq!(condition_of(&payload), Some(StartCondition::Unresolved));
        assert_eq!(
            extrude_start(&enriched(payload)),
            ExtrudeStart::Unresolved {}
        );
    }
}

#[test]
fn an_object_without_the_block_has_no_start() {
    let mut payload = payload(5, false, true);
    payload[BLOCK_AT] = 1;
    assert_eq!(condition_of(&payload), None);
    assert!(!enriched(payload).contains_key(START_CONDITION));
}

#[test]
fn a_block_whose_tail_is_not_zero_has_no_start() {
    let mut payload = payload(0, false, true);
    payload[BLOCK_AT + WORD_BLOCK_LEN - 1] = 1;
    assert_eq!(condition_of(&payload), None);
}

#[test]
fn a_block_word_other_than_zero_or_one_has_no_start() {
    let mut payload = payload(0, false, true);
    payload[BLOCK_AT + 64] = 2;
    assert_eq!(condition_of(&payload), None);
}

#[test]
fn a_nonzero_type_without_an_object_pointer_is_not_a_start() {
    let mut payload = vec![0; BLOCK_AT];
    payload.extend(block(true));
    payload.extend_from_slice(&START_TOKEN);
    payload.extend_from_slice(&7u32.to_le_bytes());
    payload.extend_from_slice(&[0; 40]);
    assert_eq!(start_type_at(&payload, BLOCK_AT), None);
}

#[test]
fn a_type_zero_start_needs_its_twenty_zero_bytes() {
    let mut payload = payload(0, false, true);
    assert_eq!(start_type_at(&payload, BLOCK_AT), Some(0));
    payload[BLOCK_AT + WORD_BLOCK_LEN + 2 + 10] = 1;
    assert_eq!(start_type_at(&payload, BLOCK_AT), None);
}

#[test]
fn an_unresolved_start_outweighs_a_profile_plane_reading_of_the_same_object() {
    let mut payload = payload(0, false, true);
    payload.extend(self::payload(5, false, false));
    assert_eq!(condition_of(&payload), Some(StartCondition::Unresolved));
    assert_eq!(
        common_condition(&[StartCondition::ProfilePlane, StartCondition::Unresolved]),
        StartCondition::Unresolved
    );
    assert_eq!(
        common_condition(&[StartCondition::ProfilePlane]),
        StartCondition::ProfilePlane
    );
}

#[test]
fn the_condition_of_each_type() {
    assert_eq!(condition(0), StartCondition::ProfilePlane);
    for kind in [1, 2, 3, 4, 5, 6, 99] {
        assert_eq!(condition(kind), StartCondition::Unresolved);
    }
}

#[test]
fn the_profile_plane_leaves_the_properties_empty() {
    let mut properties = BTreeMap::new();
    assert_eq!(extrude_start(&properties), ExtrudeStart::ProfilePlane {});
    insert_condition(&mut properties, StartCondition::ProfilePlane).unwrap();
    assert!(properties.is_empty());
    insert_condition(&mut properties, StartCondition::Unresolved).unwrap();
    assert_eq!(extrude_start(&properties), ExtrudeStart::Unresolved {});
}
