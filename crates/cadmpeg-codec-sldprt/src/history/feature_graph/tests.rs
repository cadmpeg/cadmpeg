// SPDX-License-Identifier: Apache-2.0
//! Feature graph validation owns no retained output.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::records::{Feature, FeatureSource, TreeParent};
use super::validate_feature_graph;

const SCRATCH_BYTES: u64 = 16 * 1024 * 1024;

fn feature(id: &str, source: u32, parent: Option<(&str, u32)>) -> Feature {
    Feature {
        id: id.into(),
        parent: "test:history#graph".into(),
        xml_tag: "Feature".into(),
        tree_parent: parent.map(|(record_id, source)| TreeParent::Record {
            record_id: record_id.into(),
            source_id: Some(FeatureSource::from_value(source).expect("native source id")),
        }),
        source_id: Some(FeatureSource::from_value(source).expect("native source id")),
        ordinal: source,
        name: id.into(),
        kind: "Native".into(),
        input_class: None,
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    }
}

fn features() -> [Feature; 3] {
    // Both ancestry relations visit a two-link path before reaching the root.
    [
        feature("test:feature#leaf", 3, Some(("test:feature#middle", 2))),
        feature("test:feature#middle", 2, Some(("test:feature#root", 1))),
        feature("test:feature#root", 1, None),
    ]
}

fn policy() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    policy
}

#[test]
fn feature_graph_workspace_is_released_after_success() {
    let features = features();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).expect("root context");
    validate_feature_graph(&ctx, &features).expect("valid source and tree ancestry");
    // Every admitted allocation was local to validation. The output is unit.
    drop(ctx.reserve_scoped(SCRATCH_BYTES, "test released feature graph workspace").expect("full scratch capacity"));
    validate_feature_graph(&ctx, &features).expect("same valid ancestry remains usable");
    ctx.finish_session().expect("no refused resource");
}

#[test]
fn feature_graph_workspace_is_released_after_structural_rejection() {
    let mut features = features();
    features[2].tree_parent = Some(TreeParent::Record {
        record_id: "test:feature#leaf".into(),
        source_id: Some(FeatureSource::from_value(3).expect("native source id")),
    });
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).expect("root context");
    assert!(matches!(validate_feature_graph(&ctx, &features),
        Err(CodecError::Malformed(message)) if message == "feature parent cycle"));
    drop(ctx.reserve_scoped(SCRATCH_BYTES, "test rejected feature graph workspace").expect("full scratch capacity"));
    ctx.finish_session().expect("structural recovery does not refuse resources");
}

#[test]
fn feature_graph_workspace_refuses_before_backing_allocation() {
    let features = features();
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root context");
    let (result, allocations) = crate::test_support::allocation::count_allocations(||
        validate_feature_graph(&ctx, &features));
    let Err(CodecError::ResourceLimit(limit)) = result else { panic!("graph workspace limit"); };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "index SLDPRT feature graph");
    assert_eq!(limit.used, 0);
    // The first empty source-id HashMap grows to the minimum three-entry
    // capacity: core hash_storage_bytes bounds four buckets, alignment
    // padding, four control bytes and the sixteen trailing control bytes.
    type Entry = (FeatureSource, usize);
    let first_growth = 4 * std::mem::size_of::<Entry>()
        + std::mem::align_of::<Entry>().max(16) - 1 + 4 + 16;
    assert_eq!(limit.additional, u64::try_from(first_growth).expect("hash backing bound"));
    assert_eq!(allocations, 0);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn feature_graph_validation_preserves_the_original_sticky_refusal() {
    let features = features();
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root context");
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(1, "test original graph refusal") else {
        panic!("original work limit");
    };
    assert!(matches!(validate_feature_graph(&ctx, &features), Err(CodecError::ResourceLimit(actual)) if actual == limit));
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}
