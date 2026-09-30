// SPDX-License-Identifier: Apache-2.0
//! Ordered source content projection and resource refusal.

use super::project_feature_model;
use crate::history::tests::feature;
use crate::records::{FeatureContent, FeatureHistory};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
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
fn feature_content_projection_route_refuses_work_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(b"content", &arena, &policy).unwrap();
    assert!(matches!(
        project_feature_model(&ctx, &[history()]),
        Err(CodecError::ResourceLimit(_))
    ));
}

#[test]
fn feature_content_projection_keeps_repeated_text() {
    let mut history = history();
    history.features[0]
        .content
        .push(FeatureContent::Text("source text".into()));
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(b"content", &arena, &policy).unwrap();
    let projection = project_feature_model(&ctx, &[history]).unwrap();
    assert_eq!(projection.features[0].source_content.len(), 2);
    assert_eq!(
        projection.features[0].source_content[0],
        projection.features[0].source_content[1]
    );
}
