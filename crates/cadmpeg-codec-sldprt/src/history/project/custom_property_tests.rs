// SPDX-License-Identifier: Apache-2.0
//! Custom-property projection tests.
#![allow(clippy::unwrap_used)]

use super::{custom_property_attributes, project_features};
use crate::history::tests::feature;
use crate::history::write::features::sync_neutral_features;
use crate::records::FeatureHistory;
use cadmpeg_ir::attributes::AttributeValue;
use std::collections::BTreeMap;

fn with_test_ctx<T>(run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("test decode context");
    run(&ctx)
}

#[test]
fn custom_properties_are_document_attributes_not_model_features() {
    let mut property = feature("property # µ%", None, 0);
    property.xml_tag = "CustomProperty".into();
    property.name = "PartNumber".into();
    property.text = Some("A-123".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![property],
    };

    assert!(project_features(
        &cadmpeg_test_support::service_decode_context(),
        std::slice::from_ref(&history)
    )
    .unwrap()
    .is_empty());
    let attributes = with_test_ctx(|ctx| {
        custom_property_attributes(ctx, std::slice::from_ref(&history))
            .expect("custom-property projection")
    });
    assert_eq!(attributes.len(), 1);
    assert_eq!(
        attributes[0].id.as_str(),
        "sldprt:history:custom-property#property%20%23%20µ%25"
    );
    assert_eq!(attributes[0].name, "PartNumber");
    assert_eq!(
        attributes[0].values,
        vec![AttributeValue::String("A-123".into())]
    );

    let mut native = Some(crate::native::SldprtNative {
        feature_histories: vec![history],
        feature_input_lanes: Vec::new(),
        pmi_dimensions: Vec::new(),
    });
    sync_neutral_features(
        &cadmpeg_ir::document::Model::default(),
        &[],
        &[],
        &mut native,
    )
    .expect("required invariant");
    assert_eq!(
        native.expect("required invariant").feature_histories[0]
            .features
            .len(),
        1
    );
}
