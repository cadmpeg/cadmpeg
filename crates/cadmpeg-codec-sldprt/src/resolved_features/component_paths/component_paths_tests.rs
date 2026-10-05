//! Tests for the `component_paths` module.

use super::{
    component_path_feature, component_path_features, component_path_input_features,
    component_path_terminal_feature, surface_selection_producer_features, ComponentPathEnd,
};
use crate::records::FeatureSource;
use crate::records::{Feature, FeatureInputComponentPathEntry};
use std::collections::BTreeMap;

#[test]
fn component_path_type_identities_name_ordered_features() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let feature = |id: &str, source_id: &str| Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some(FeatureSource::try_from(source_id).expect("test feature source id")),
        ordinal: 0,
        name: String::new(),
        kind: String::new(),
        input_class: None,
        suppressed: false,
        parameters: BTreeMap::default(),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: Vec::new(),
    };
    let mut signature = [0u8; 12];
    signature[4..8].copy_from_slice(&42u32.to_le_bytes());
    let components = vec![
        FeatureInputComponentPathEntry {
            instance: Some(0x8032),
            type_signature: signature,
            local_id: Some(7),
        },
        FeatureInputComponentPathEntry {
            instance: Some(0x803b),
            type_signature: signature,
            local_id: Some(1),
        },
    ];
    assert_eq!(
        component_path_features(&ctx, &components, &[feature("producer", "42")]).unwrap(),
        vec!["producer"]
    );
    assert_eq!(
        component_path_features(
            &ctx,
            &components,
            &[feature("first", "42"), feature("second", "42")]
        )
        .unwrap(),
        Vec::<String>::new()
    );
    let mut mixed = components;
    mixed[1].type_signature[4..8].copy_from_slice(&43u32.to_le_bytes());
    assert_eq!(
        component_path_features(
            &ctx,
            &mixed,
            &[feature("producer", "42"), feature("other", "43")]
        )
        .unwrap(),
        vec!["producer", "other"]
    );
    assert_eq!(
        component_path_terminal_feature(
            &ctx,
            &mixed,
            &[feature("producer", "42"), feature("other", "43")]
        )
        .unwrap(),
        Some("other".into())
    );
    assert_eq!(
        surface_selection_producer_features(
            &ctx,
            &mixed,
            Some("explicit"),
            &[feature("producer", "42"), feature("other", "43")]
        )
        .unwrap(),
        ["producer", "other", "explicit"]
    );
    mixed.push(FeatureInputComponentPathEntry {
        instance: Some(0x8040),
        type_signature: {
            let mut signature = [0; 12];
            signature[4..8].copy_from_slice(&99u32.to_le_bytes());
            signature
        },
        local_id: Some(5),
    });
    assert_eq!(
        component_path_terminal_feature(
            &ctx,
            &mixed,
            &[feature("producer", "42"), feature("other", "43")]
        )
        .unwrap(),
        Some("other".into())
    );

    let owner = feature("mirror", "44");
    mixed.push(FeatureInputComponentPathEntry {
        instance: None,
        type_signature: {
            let mut signature = [0; 12];
            signature[4..8].copy_from_slice(&44u32.to_le_bytes());
            signature
        },
        local_id: Some(9),
    });
    let producer = feature("producer", "42");
    let other = feature("other", "43");
    let history = [&producer, &other, &owner];
    let (component, preceding) =
        component_path_feature(&ctx, &mixed, &history, "mirror", ComponentPathEnd::Trailing)
            .unwrap()
            .expect("required invariant");
    assert_eq!(preceding.id, "other");
    assert_eq!(component.local_id, Some(1));

    let mut prior = feature("prior", "42");
    prior.ordinal = 3;
    let mut consumer = feature("consumer", "88");
    consumer.ordinal = 2;
    let mut future = feature("future", "99");
    future.ordinal = 1;
    let path = [88_u32, 42, 99, 88]
        .into_iter()
        .map(|source| FeatureInputComponentPathEntry {
            instance: Some(0x8180),
            type_signature: {
                let mut signature = [0; 12];
                signature[4..8].copy_from_slice(&source.to_le_bytes());
                signature
            },
            local_id: Some(1),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        component_path_input_features(&ctx, &path, &[prior, consumer, future], "consumer").unwrap(),
        ["prior"]
    );
}

#[test]
fn dissected_profile_propagates_each_work_refusal() {
    let feature = Feature {
        id: "profile".into(),
        parent: "history".into(),
        xml_tag: "Sketch".into(),
        tree_parent: None,
        source_id: None,
        ordinal: 0,
        name: "Sketch<1>".into(),
        kind: String::new(),
        input_class: None,
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::from([(
            cadmpeg_core::nonblank_const!("Description"),
            "Sketch<1>".into(),
        )]),
        text: None,
        content: Vec::new(),
    };
    for operation in [
        "find SLDPRT dissected profile description",
        "compare SLDPRT dissected profile description",
        "split SLDPRT name ordinal",
        "check SLDPRT name ordinal digits",
    ] {
        crate::test_support::work_refusal_at(operation, |ctx| {
            super::is_dissected_profile_feature(ctx, &feature)
        });
    }
    assert!(super::is_dissected_profile_feature(
        &cadmpeg_test_support::service_decode_context(),
        &feature,
    )
    .unwrap());
}

#[test]
fn adjacent_profile_vote_lookup_propagates_work_refusal() {
    let feature = |id: &str, class: &str, ordinal| Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: None,
        ordinal,
        name: id.into(),
        kind: String::new(),
        input_class: Some(class.into()),
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let histories = [crate::records::FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![
            feature("profile", "moProfileFeature_c", 0),
            feature("extrude", "moExtrusion_c", 1),
        ],
    }];
    let lane = crate::records::FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: ["profile", "extrude"]
            .into_iter()
            .enumerate()
            .map(|(index, value)| crate::records::FeatureInputName {
                id: format!("name-{index}"),
                parent: "lane".into(),
                ordinal: u32::try_from(index).unwrap(),
                offset: cadmpeg_core::decode::u64_from_index(index),
                value: value.into(),
                object_id: None,
            })
            .collect(),
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
    crate::test_support::work_refusal_at("resolve SLDPRT adjacent profile votes", |ctx| {
        super::project_adjacent_extrusion_profiles(
            ctx,
            &mut [],
            &histories,
            std::slice::from_ref(&lane),
        )
    });
    super::project_adjacent_extrusion_profiles(
        &cadmpeg_test_support::service_decode_context(),
        &mut [],
        &histories,
        std::slice::from_ref(&lane),
    )
    .unwrap();
}

#[test]
fn profile_block_identity_parsers_preserve_ownership_and_refusals() {
    let feature = |source: u32, class: &str| Feature {
        id: format!("feature#{source}"),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: FeatureSource::from_value(source),
        ordinal: 0,
        name: String::new(),
        kind: String::new(),
        input_class: Some(class.into()),
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let mut profile = feature(1, "moProfileFeature_c");
    profile.properties.insert(
        cadmpeg_core::nonblank_literal!("DissectableChildren"),
        "23".into(),
    );
    let definition = feature(23, "moSketchBlockDef_c");
    let mut instance = feature(25, "moSketchBlockInst_c");
    instance.properties.insert(
        cadmpeg_core::nonblank_literal!("BlockDefinition"),
        "23".into(),
    );
    let objects = [&instance, &definition];
    let solve = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        super::profile_owns_intervening_sketch_blocks(ctx, &profile, objects)
    };
    assert!(solve(&cadmpeg_test_support::service_decode_context()).unwrap());
    for operation in [
        "parse SLDPRT profile child identity",
        "parse SLDPRT sketch block definition identity",
    ] {
        crate::test_support::work_refusal_at(operation, solve);
    }
}

mod character_growth;
