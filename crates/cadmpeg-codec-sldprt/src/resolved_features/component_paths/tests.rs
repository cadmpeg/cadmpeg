//! Component indexes retain only sources named by the path.

use super::{component_path_features, component_path_terminal_feature};
use crate::brep::feature_source::FeatureSourceId;
use crate::records::{Feature, FeatureInputComponentPathEntry, FeatureSource};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
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
        type_signature,
        local_id: None,
    }
}

#[test]
fn component_indexes_admit_only_referenced_sources_and_release_storage() {
    let features = (1..=1000).map(feature).collect::<Vec<_>>();
    let components = [component(500)];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One map slot per call and one output-list slot.
    policy.limits.max_collection_items = 3;
    policy.limits.max_retained_bytes = 256;
    policy.limits.max_materialized_bytes = 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        component_path_features(&ctx, &components, &features).unwrap(),
        ["synthetic:feature#500"]
    );
    assert_eq!(
        component_path_terminal_feature(&ctx, &components, &features).unwrap(),
        Some("synthetic:feature#500".into())
    );
    let released = ctx
        .reserve_scoped(1024, "component indexes released")
        .unwrap();
    drop(released);
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn component_indexes_preserve_ambiguous_source_identity() {
    let mut second = feature(500);
    second.id = "synthetic:feature#duplicate".into();
    let features = [feature(500), second];
    let components = [component(500)];
    let ctx = cadmpeg_test_support::service_decode_context();
    assert!(component_path_features(&ctx, &components, &features)
        .unwrap()
        .is_empty());
    assert_eq!(
        component_path_terminal_feature(&ctx, &components, &features).unwrap(),
        None
    );
}
