//! The end spec's first-direction reversal flag.

use super::{
    enrich_history_extrusion_terminations, BTreeMap, Feature, FeatureHistory, FeatureInputLane,
    FeatureInputName, FeatureSource, ObjectId,
};

/// Anchor +12 of the end spec reverses the first direction; a mid-plane
/// extent has no direction to reverse, and a Keywords end condition of
/// another termination leaves the flag unread.
#[test]
fn the_end_spec_reversal_flag_reverses_the_first_direction() {
    let anchor = 100;
    let payload = |directions: u8, code: u8, reversed: u8| {
        let mut payload = vec![0; anchor + 26];
        payload[anchor..anchor + 2].copy_from_slice(&[0x0c, 0x8e]);
        payload[anchor + 4] = directions;
        payload[anchor + 12] = reversed;
        payload[anchor + 18] = code;
        payload.extend_from_slice(b"\xff\xff\x01\x00\x16\x00moDisplayDistanceDim_c");
        let block = payload.len();
        payload.resize(block + 16, 0);
        payload[block + 9] = 0x20;
        payload.extend_from_slice(&[0xff, 0xff, 0, 0, 3]);
        payload.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);
        payload.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0x80, 0xbf]);
        payload.resize(anchor + 200, 0);
        payload
    };
    let resolve = |native_payload: Vec<u8>, keywords: Option<&str>| {
        let mut properties = BTreeMap::new();
        if let Some(condition) = keywords {
            properties.insert(
                cadmpeg_core::nonblank_literal!("EndCondition"),
                condition.to_owned(),
            );
        }
        let mut histories = vec![FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![Feature {
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
                parameters: BTreeMap::from([(
                    cadmpeg_core::nonblank_literal!("D1"),
                    String::from("5mm"),
                )]),
                dimension_properties: BTreeMap::new(),
                properties,
                text: None,
                content: Vec::new(),
            }],
        }];
        let lane = FeatureInputLane {
            id: "lane#7".into(),
            configuration: None,
            native_payload: native_payload.into(),
            classes: Vec::new(),
            names: vec![FeatureInputName {
                id: "extrusion-name".into(),
                parent: "lane#7".into(),
                ordinal: 0,
                offset: 10,
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
        let properties = &histories[0].features[0].properties;
        (
            properties.get("EndCondition").cloned(),
            properties.get("Reverse").cloned(),
        )
    };
    let blind = Some("Blind".to_owned());
    let reversed = Some("true".to_owned());
    assert_eq!(resolve(payload(1, 0, 0), None), (blind.clone(), None));
    assert_eq!(
        resolve(payload(1, 0, 1), None),
        (blind.clone(), reversed.clone())
    );
    assert_eq!(
        resolve(payload(0, 0, 1), None),
        (blind.clone(), reversed.clone())
    );
    assert_eq!(resolve(payload(1, 0, 1), Some("Blind")), (blind, reversed));
    assert_eq!(
        resolve(payload(1, 0, 1), Some("ThroughAll")),
        (Some("ThroughAll".to_owned()), None)
    );
    assert_eq!(
        resolve(payload(1, 6, 1), None),
        (Some("Symmetric".to_owned()), None)
    );
}
