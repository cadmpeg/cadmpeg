//! Compact selection projection and resource-limit tests.

use super::super::{
    project_compact_body_selections, project_compact_edge_selections,
    project_compact_surface_selections,
};
use crate::records::{FeatureInputBodySelection, FeatureInputLane};
use cadmpeg_ir::features::{
    BodyRetentionMode, BodySelection, FeatureDefinition, FeatureId, FeatureOperation,
    UnresolvedFamily,
};
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

fn compact_body_projection_fixture() -> (cadmpeg_ir::features::Feature, FeatureInputLane) {
    let mut feature = compact_edge_projection_feature();
    feature.evaluation.set_definition(FeatureDefinition::Operation(
        FeatureOperation::DeleteBody {
            bodies: BodySelection::Unresolved,
            mode: BodyRetentionMode::Unresolved,
        },
    ));
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: vec![FeatureInputBodySelection {
            id: "body".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            object_name_ref: "name".into(),
            feature_ref: "fillet".into(),
            local_body_ids: vec![3, 4],
            body_state_ids: Vec::new(),
            mode: Some(BodyRetentionMode::KeepSelected),
        }],
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    (feature, lane)
}

#[test]
fn compact_body_projection_preserves_selection_and_retention_mode() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let (mut feature, lane) = compact_body_projection_fixture();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test context");
    project_compact_body_selections(&ctx, std::slice::from_mut(&mut feature), &[lane])
        .expect("body projection");
    assert!(matches!(feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DeleteBody {
            bodies: BodySelection::Local { bodies, native },
            mode: BodyRetentionMode::KeepSelected,
        }) if bodies.as_slice() == ["3", "4"] && native.as_str() == "sldprt:feature-input:body-ids:3,4"));
}

#[test]
fn compact_body_projection_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let (mut feature, lane) = compact_body_projection_fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = project_compact_body_selections(&ctx, std::slice::from_mut(&mut feature), &[lane])
        .expect_err("two bodies exceed one collection slot");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "project SLDPRT compact body selections"));
}

#[test]
fn compact_body_projection_refuses_uniqueness_index_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let (mut feature, lane) = compact_body_projection_fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = project_compact_body_selections(&ctx, std::slice::from_mut(&mut feature), &[lane])
        .expect_err("uniqueness index exceeds remaining collection slots");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "validate distinct decoded native selections"));
}

#[test]
fn compact_body_projection_keeps_duplicate_selection_unresolved() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let (mut feature, mut lane) = compact_body_projection_fixture();
    lane.body_selections[0].local_body_ids = vec![3, 3];
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test context");
    project_compact_body_selections(&ctx, std::slice::from_mut(&mut feature), &[lane])
        .expect("duplicate is a semantic no-match");
    assert!(matches!(feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DeleteBody {
            bodies: BodySelection::Unresolved,
            mode: BodyRetentionMode::Unresolved,
        })));
}

#[test]
fn compact_body_projection_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let (mut feature, lane) = compact_body_projection_fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = project_compact_body_selections(&ctx, std::slice::from_mut(&mut feature), &[lane])
        .expect_err("body text exceeds retained limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "project SLDPRT compact body selections"));
}

#[test]
fn compact_body_projection_refuses_work_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let (mut feature, lane) = compact_body_projection_fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = project_compact_body_selections(&ctx, std::slice::from_mut(&mut feature), &[lane])
        .expect_err("selection scan exceeds work limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "project SLDPRT compact body selections"));
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

#[test]
fn compact_surface_projection_refuses_index_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let mut feature = compact_edge_projection_feature();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = project_compact_surface_selections(&ctx, std::slice::from_mut(&mut feature), &[], &[])
        .expect_err("surface feature index exceeds collection limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "index SLDPRT compact surface selections"));
}

#[test]
fn compact_surface_projection_refuses_index_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let mut feature = compact_edge_projection_feature();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = project_compact_surface_selections(&ctx, std::slice::from_mut(&mut feature), &[], &[])
        .expect_err("surface feature ID exceeds retained limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "index SLDPRT compact surface selections"));
}

#[test]
fn compact_surface_projection_refuses_index_work_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let mut feature = compact_edge_projection_feature();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = project_compact_surface_selections(&ctx, std::slice::from_mut(&mut feature), &[], &[])
        .expect_err("surface feature scan exceeds work limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "index SLDPRT compact surface selections"));
}
