//! Resource-limit tests for compact edge projection.

use super::super::project_compact_edge_selections;
use cadmpeg_ir::features::{FeatureDefinition, FeatureId, FeatureOperation, UnresolvedFamily};
use std::collections::BTreeMap;

fn compact_edge_projection_feature() -> cadmpeg_ir::features::Feature {
    cadmpeg_ir::features::Feature {
        id: FeatureId::mint("synthetic:test:id#fillet").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::Fillet,
            }),
        ),
        native_ref: Some("fillet".into()),
    }
}

#[test]
fn compact_edge_projection_refuses_index_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let mut feature = compact_edge_projection_feature();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = project_compact_edge_selections(
        &ctx,
        std::slice::from_mut(&mut feature),
        &[],
        &[],
    )
    .expect_err("feature index exceeds collection limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "index SLDPRT compact edge selections"));
}

#[test]
fn compact_edge_projection_refuses_index_work_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let mut feature = compact_edge_projection_feature();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = project_compact_edge_selections(
        &ctx,
        std::slice::from_mut(&mut feature),
        &[],
        &[],
    )
    .expect_err("feature index exceeds work limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "index SLDPRT compact edge selections"));
}

#[test]
fn compact_edge_projection_refuses_index_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let mut feature = compact_edge_projection_feature();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = project_compact_edge_selections(
        &ctx,
        std::slice::from_mut(&mut feature),
        &[],
        &[],
    )
    .expect_err("feature identity exceeds retained limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "index SLDPRT compact edge selections"));
}

