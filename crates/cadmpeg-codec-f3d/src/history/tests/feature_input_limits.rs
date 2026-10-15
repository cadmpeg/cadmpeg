// SPDX-License-Identifier: Apache-2.0
//! Admission limits for projected feature input topology.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation, UnresolvedFamily};

fn input_fixture() -> (
    Feature,
    crate::records::feature::scope::DesignParameterScope,
    crate::history_records::AsmHistory,
) {
    use crate::history_records::{
        AsmDeltaState, AsmHistoricalTopology, AsmHistory, AsmTopologyCache,
    };
    let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:design:scope#input",
        crate::records::feature::scope::DesignFeatureKind::WorkPoint,
        7,
    );
    scope
        .try_edit(|draft| {
            draft.previous_history_state_id = Some(4);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let feature = Feature {
        id: "f3d:model:feature#input".to_owned().try_into().unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("WorkPoint".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::DatumPoint,
            }),
        ),
        native_ref: Some(scope.id.clone()),
    };
    let history = AsmHistory {
        id: "f3d:history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![AsmDeltaState {
            id: "f3d:history:state#4".into(),
            parent: "f3d:history".into(),
            byte_offset: 0,
            state_id: 4,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref: None,
            node_index: 0,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records: Vec::new(),
            entity_versions: Vec::new(),
            topology_cache: AsmTopologyCache::Complete(AsmHistoricalTopology {
                bodies: vec![1],
                faces: vec![2],
                edges: vec![3],
                vertices: vec![4],
                ..AsmHistoricalTopology::default()
            }),
            transition: None,
        }],
    };
    (feature, scope, history)
}

fn project(
    max_items: u64,
    max_materialized_bytes: u64,
) -> Result<Vec<cadmpeg_ir::features::FeatureInputTopology>, cadmpeg_core::CodecError> {
    let (feature, scope, history) = input_fixture();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    policy.limits.max_materialized_bytes = max_materialized_bytes;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::project_feature_input_topologies(
        &ctx,
        std::slice::from_ref(&feature),
        std::slice::from_ref(&scope),
        std::slice::from_ref(&history),
        &[],
    )
}

#[test]
fn input_body_members_refuse_collection_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "collect F3D input bodies",
        |cap| project(cap, u64::MAX),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D input bodies")
    );
}

#[test]
fn input_face_members_refuse_collection_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "collect F3D input faces",
        |cap| project(cap, u64::MAX),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D input faces")
    );
}

#[test]
fn input_edge_members_refuse_collection_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "collect F3D input edges",
        |cap| project(cap, u64::MAX),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D input edges")
    );
}

#[test]
fn input_vertex_members_refuse_collection_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "collect F3D input vertices",
        |cap| project(cap, u64::MAX),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D input vertices")
    );
}

#[test]
fn input_topologies_refuse_collection_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "collect F3D input topologies",
        |cap| project(cap, u64::MAX),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D input topologies")
    );
}

#[test]
fn input_identity_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        "retain F3D history input identity",
        |cap| project(u64::MAX, cap).map(|_| ()),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D history input identity")
    );
}

#[test]
fn input_projection_preserves_member_order() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "collect F3D input topologies",
        |cap| project(cap, u64::MAX),
    );
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("expected input topology collection refusal");
    };
    let admitted_items = limit.used.checked_add(limit.additional).unwrap();
    let projected = project(admitted_items, u64::MAX).unwrap();
    assert_eq!(projected.len(), 1);
    assert_eq!(
        projected[0].vertices.as_slice()[0].as_str(),
        "f3d:history-input:vertex#5:input:4:4"
    );
}

#[test]
fn duplicate_input_faces_release_preceding_body_projection() {
    let (feature, scope, mut history) = input_fixture();
    history.states[0].topology_cache = crate::history_records::AsmTopologyCache::Complete(
        crate::history_records::AsmHistoricalTopology {
            bodies: vec![1],
            faces: vec![2, 2],
            ..Default::default()
        },
    );
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let projected = crate::history::project_feature_input_topologies(
        &ctx,
        &[feature],
        &[scope],
        &[history],
        &[],
    )
    .unwrap();
    assert!(projected.is_empty());
}
