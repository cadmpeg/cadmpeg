//! Component indexes retain only sources named by the path.

use super::{component_path_features, component_path_terminal_feature};
use crate::brep::feature_source::FeatureSourceId;
use crate::records::{Feature, FeatureInputComponentPathEntry, FeatureSource};
use std::collections::BTreeMap;

fn feature(source: u32) -> Feature {
    Feature {
        id: format!("synthetic:feature#{source}"),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some(FeatureSource::Id(
            FeatureSourceId::try_from(source).unwrap(),
        )),
        ordinal: source,
        name: format!("Feature{source}"),
        kind: "Extrude".into(),
        input_class: None,
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    }
}

fn component(source: u32) -> FeatureInputComponentPathEntry {
    let mut type_signature = [0; 12];
    type_signature[4..8].copy_from_slice(&source.to_le_bytes());
    FeatureInputComponentPathEntry {
        instance: None,
        type_signature: type_signature.into(),
        local_id: None,
    }
}

#[test]
fn component_indexes_resolve_referenced_sources() {
    let features = (1..=1000).map(feature).collect::<Vec<_>>();
    let components = [component(500)];
    assert_eq!(
        component_path_features(&components, &features),
        ["synthetic:feature#500"]
    );
    assert_eq!(
        component_path_terminal_feature(&components, &features),
        Some("synthetic:feature#500".into())
    );
}

#[test]
fn component_indexes_preserve_ambiguous_source_identity() {
    let mut second = feature(500);
    second.id = "synthetic:feature#duplicate".into();
    let features = [feature(500), second];
    let components = [component(500)];
    assert!(component_path_features(&components, &features).is_empty());
    assert_eq!(
        component_path_terminal_feature(&components, &features),
        None
    );
}
