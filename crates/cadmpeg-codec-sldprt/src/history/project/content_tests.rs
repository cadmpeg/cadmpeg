// SPDX-License-Identifier: Apache-2.0
//! Ordered source content projection and resource refusal.

use super::project_feature_model;
use crate::history::tests::feature;
use crate::records::{FeatureContent, FeatureHistory};
use std::collections::BTreeMap;

fn history() -> FeatureHistory {
    let mut source = feature("sldprt:history:feature#0:1", None, 0);
    source.content = vec![FeatureContent::Text("source text".into())];
    FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![source],
    }
}

#[test]
fn feature_content_projection_keeps_repeated_text() {
    let mut history = history();
    history.features[0]
        .content
        .push(FeatureContent::Text("source text".into()));
    let projection = project_feature_model(&[history]).unwrap();
    assert_eq!(projection.features[0].source_content.len(), 2);
    assert_eq!(
        projection.features[0].source_content[0],
        projection.features[0].source_content[1]
    );
}
