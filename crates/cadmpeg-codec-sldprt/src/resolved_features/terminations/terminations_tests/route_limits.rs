//! Resource admission for history termination enrichment.

use super::{Feature, FeatureHistory, FeatureInputLane, FeatureInputName, FeatureSource, ObjectId};
use super::super::enrich_history_extrusion_terminations;
use std::collections::BTreeMap;

fn extrusion_termination_error(policy: cadmpeg_core::decode::DecodePolicy) -> cadmpeg_core::CodecError {
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
        source_id: Some(FeatureSource::try_from(source_id).expect("test feature source id")),
        ordinal: source_id.parse().expect("required invariant"),
        name: id.to_string(),
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
                object_id: ObjectId::from_value(10),
            },
            FeatureInputName {
                id: "cosmetic-name".into(),
                parent: "lane#7".into(),
                ordinal: 1,
                offset: 200,
                value: "cosmetic".into(),
                object_id: ObjectId::from_value(11),
            },
            FeatureInputName {
                id: "next-name".into(),
                parent: "lane#7".into(),
                ordinal: 2,
                offset: 500,
                value: "next".into(),
                object_id: ObjectId::from_value(12),
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

    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (service, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &lane.native_payload, &arena, &cadmpeg_core::decode::DecodePolicy::service(),
    ).unwrap();
    let mut admitted = histories.clone();
    enrich_history_extrusion_terminations(&service, &mut admitted, std::slice::from_ref(&lane)).unwrap();
    assert_eq!(admitted[0].features[0].properties.get("EndCondition").map(String::as_str), Some("ThroughAll"));
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&lane.native_payload, &arena, &policy).unwrap();
    enrich_history_extrusion_terminations(&ctx, &mut histories, std::slice::from_ref(&lane)).unwrap_err()
}

#[test]
fn extrusion_termination_enrichment_refuses_collection_limit() {
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    assert!(matches!(extrusion_termination_error(policy), cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn extrusion_termination_enrichment_refuses_retained_limit() {
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    assert!(matches!(extrusion_termination_error(policy), cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn extrusion_termination_enrichment_refuses_work_limit() {
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 0;
    assert!(matches!(extrusion_termination_error(policy), cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}
