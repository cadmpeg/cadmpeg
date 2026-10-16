use super::{lane_added_source, FeatureSources};
use crate::records::{
    Feature, FeatureInputComponentPathEntry, FeatureInputLane, FeatureInputName,
    FeatureInputSurfaceSelection, FeatureInputSurfaceSelectionKind, FeatureSource, ObjectId,
};
use crate::resolved_features::component_paths::{
    component_path_input_features, component_path_terminal_feature,
    surface_selection_producer_features,
};
use crate::resolved_features::selections::enrich_feature_object_sources;
use std::collections::BTreeMap;

fn feature(name: &str, source_id: Option<FeatureSource>, ordinal: u32) -> Feature {
    Feature {
        id: name.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id,
        ordinal,
        name: name.into(),
        kind: "Feature".into(),
        input_class: None,
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    }
}

fn lane(names: &[(&str, Option<u32>)]) -> FeatureInputLane {
    FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new().into(),
        classes: Vec::new(),
        names: names
            .iter()
            .map(|(name, source)| FeatureInputName {
                id: format!("name:{name}"),
                parent: "lane".into(),
                ordinal: 0,
                offset: 0,
                object_id: source.and_then(ObjectId::from_value),
                value: (*name).into(),
            })
            .collect(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: vec![FeatureInputSurfaceSelection {
            id: "selection".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            selector: 0,
            kind: FeatureInputSurfaceSelectionKind::Component,
            object_name_ref: "name:consumer".into(),
            feature_ref: "consumer".into(),
            producer_feature_refs: Vec::new(),
            terminal_feature_ref: None,
            components: Vec::new(),
        }],
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    }
}

fn component(source: u32) -> FeatureInputComponentPathEntry {
    let mut signature = [0; 12];
    signature[4..8].copy_from_slice(&source.to_le_bytes());
    FeatureInputComponentPathEntry {
        instance: None,
        type_signature: signature.into(),
        local_id: Some(1),
    }
}

#[test]
fn lane_sources_match_single_lane_enrichment_and_selection_resolution() {
    let sources = [
        None,
        Some(FeatureSource::Reserved),
        FeatureSource::from_value(42),
    ];
    let paths = [
        vec![],
        vec![component(42)],
        vec![component(88), component(42), component(99)],
    ];
    let lanes = [
        lane(&[]),
        lane(&[("producer", Some(42)), ("consumer", Some(88))]),
        lane(&[("producer", Some(99)), ("consumer", Some(42))]),
        lane(&[("producer", Some(42)), ("producer", Some(99))]),
        lane(&[("producer", None), ("consumer", Some(88))]),
    ];
    for producer_source in sources {
        for consumer_source in sources {
            let features = [
                feature("producer", producer_source, 3),
                feature("consumer", consumer_source, 2),
                feature("future", FeatureSource::from_value(99), 1),
            ];
            for lane in &lanes {
                let view = FeatureSources::for_lane(&features, lane);
                let mut enriched = features.clone();
                enrich_feature_object_sources(&mut enriched, std::slice::from_ref(lane));
                for ((original, resolved), expected) in
                    features.iter().zip(view.iter()).zip(&enriched)
                {
                    assert!(std::ptr::eq(original, resolved.feature));
                    assert_eq!(resolved.source, expected.source_id);
                }
                let enriched = FeatureSources::new(&enriched);
                for path in &paths {
                    assert_eq!(
                        component_path_input_features(path, &view, "consumer"),
                        component_path_input_features(path, &enriched, "consumer")
                    );
                    assert_eq!(
                        component_path_terminal_feature(path, view.iter()),
                        component_path_terminal_feature(path, enriched.iter())
                    );
                    for terminal in [None, Some("explicit")] {
                        assert_eq!(
                            surface_selection_producer_features(path, terminal, &view),
                            surface_selection_producer_features(path, terminal, &enriched)
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn lane_source_rule_preserves_identity_and_requires_a_unique_named_object() {
    let named = lane(&[("producer", Some(42))]);
    assert_eq!(
        lane_added_source(&feature("producer", None, 0), &named),
        FeatureSource::from_value(42)
    );
    for source in [Some(FeatureSource::Reserved), FeatureSource::from_value(99)] {
        assert_eq!(
            lane_added_source(&feature("producer", source, 0), &named),
            None
        );
    }
    for names in [
        vec![],
        vec![("producer", None)],
        vec![("producer", Some(42)), ("producer", Some(42))],
    ] {
        assert_eq!(
            lane_added_source(&feature("producer", None, 0), &lane(&names)),
            None
        );
    }
}

#[test]
fn multi_lane_sources_require_agreement_among_lanes_with_an_identity() {
    for candidates in [
        vec![],
        vec![None],
        vec![Some(42)],
        vec![Some(42), None],
        vec![Some(42), Some(42)],
        vec![Some(42), Some(99)],
    ] {
        let lanes: Vec<_> = candidates
            .iter()
            .map(|source| lane(&[("producer", *source)]))
            .collect();
        let mut features = [
            feature("producer", None, 0),
            feature("producer", Some(FeatureSource::Reserved), 0),
            feature("producer", FeatureSource::from_value(99), 0),
        ];
        enrich_feature_object_sources(&mut features, &lanes);
        let expected = if candidates.contains(&Some(42)) && !candidates.contains(&Some(99)) {
            FeatureSource::from_value(42)
        } else {
            None
        };
        assert_eq!(features[0].source_id, expected);
        assert_eq!(features[1].source_id, Some(FeatureSource::Reserved));
        assert_eq!(features[2].source_id, FeatureSource::from_value(99));
    }
}

#[test]
fn lane_sources_keep_records_with_duplicate_ids_distinct() {
    let mut features = [feature("first", None, 0), feature("second", None, 1)];
    features[1].id = features[0].id.clone();
    let lane = lane(&[("first", Some(42)), ("second", Some(88))]);
    let view = FeatureSources::for_lane(&features, &lane);
    assert_eq!(
        view.iter()
            .map(super::SourcedFeature::source_value)
            .collect::<Vec<_>>(),
        [Some(42), Some(88)]
    );
}
