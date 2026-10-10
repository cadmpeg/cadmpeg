// SPDX-License-Identifier: Apache-2.0

use super::{
    add_surface_prototype_feature_dependencies, feature_dependencies, feature_entity_dependencies,
    feature_generated_dependencies, feature_output_surface_dependencies,
    native_feature_dependency_ids, reconciled_dependencies, surface_merge_entity_dependencies,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    edge_treatments::RadiusSpec, EdgeSelection, FaceSelection,
    FeatureDefinition as IrFeatureDefinition, FeatureId as IrFeatureId,
    FeatureOperation as IrFeatureOperation, GeneratedEdgeRef, GeneratedFaceRef,
};
use std::collections::BTreeMap;

fn one_dependency_table(
    feature_id: u32,
    table_class_id: u32,
    entity_id: u32,
    class_id: u32,
    source_entity_id: Option<u32>,
    offset: usize,
) -> crate::feature::entity::FeatureEntityTable {
    crate::feature::entity::FeatureEntityTable::new(
        feature_id,
        table_class_id,
        vec![crate::feature::entity::FeatureEntityTableEntry {
            payload: crate::feature::entity::entry_payload(class_id, source_entity_id, None, None),
            entity_id,
            prefixed: true,
            offset: offset + 1,
            end_offset: offset + 2,
        }],
        &std::collections::BTreeSet::new(),
        offset,
    )
}

fn dependency_collection_error(operation: &'static str, route: &str) {
    let producer = one_dependency_table(3, 67, 103, 200, Some(3), 10);
    let consumer = one_dependency_table(17, 100, 103, 201, None, 20);
    let owned = one_dependency_table(17, 67, 103, 200, Some(17), 30);
    let surface = crate::surface::SurfaceRow {
        id: 201,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 3,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    };
    let replay = crate::feature::rows::FeatureSurfaceMergeAffectedIds {
        feature_id: 17,
        geometry_ids: Vec::new(),
        edge_ids: Vec::new(),
        quilt_ids: vec![103],
        geometry_extent: crate::feature::rows::ReplayExtentSource::Explicit,
        edge_extent: crate::feature::rows::ReplayExtentSource::Explicit,
        quilt_extent: crate::feature::rows::ReplayExtentSource::Explicit,
        offset: 100,
    };
    let parent = crate::feature::rows::FeatureAffectedIds {
        feature_id: 17,
        kind: crate::feature::rows::AffectedIdKind::StrongParents,
        ids: vec![3],
        offset: 0,
    };
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        operation,
        |ctx| match route {
            "parent" => native_feature_dependency_ids(
                ctx,
                std::slice::from_ref(&parent),
                &[],
                &[],
                &[],
                &crate::surface::unique_rows::UniqueIdRows::from_rows([].to_vec()),
                (17, &[]),
            )
            .map(|_| ()),
            "owned" | "output" => feature_output_surface_dependencies(
                ctx,
                &[owned.clone(), consumer.clone()],
                &crate::surface::unique_rows::UniqueIdRows::from_rows([surface.clone()].to_vec()),
                17,
            )
            .map(|_| ()),
            "entity" => feature_entity_dependencies(ctx, &[producer.clone(), consumer.clone()], 17)
                .map(|_| ()),
            "merge" => surface_merge_entity_dependencies(
                ctx,
                &[],
                std::slice::from_ref(&replay),
                std::slice::from_ref(&producer),
                17,
            )
            .map(|_| ()),
            _ => panic!("unknown dependency fixture route"),
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(resource)
        if resource.operation == operation),
        "{error:?}"
    );
}

#[test]
fn agreed_feature_parent_ids_refuse_collection_limit() {
    dependency_collection_error("creo agreed feature parent IDs", "parent");
}

#[test]
fn output_surface_owned_entity_nodes_refuse_collection_limit() {
    dependency_collection_error("creo output surface owned entity nodes", "owned");
}

#[test]
fn output_surface_dependencies_refuse_collection_limit() {
    dependency_collection_error("creo output surface dependencies", "output");
}

#[test]
fn feature_entity_dependencies_refuse_collection_limit() {
    dependency_collection_error("creo feature entity dependencies", "entity");
}

#[test]
fn surface_merge_dependencies_refuse_collection_limit() {
    dependency_collection_error("creo surface merge dependencies", "merge");
}

fn feature_dependency_limit_error(
    dimension: cadmpeg_core::decode::ResourceDimension,
    operation: &'static str,
    native_only: bool,
) {
    let scan = crate::test_support::empty_container_scan();
    let mut ir = reconciliation_ir_with_generated_dependency();
    ir.model.features[0].id =
        IrFeatureId::mint("creo:model:feature#3").expect("fixture feature ID");
    let prototypes = BTreeMap::from([(17, vec![3])]);
    let error = crate::test_support::last_refusal_at(&[], dimension, operation, |ctx| {
        if native_only {
            native_feature_dependency_ids(
                ctx,
                &scan.features.affected_ids,
                &scan.features.operations,
                &scan.features.entity_tables,
                &scan.features.surface_merge_replay_affected_ids,
                &scan.surfaces.rows,
                (17, &[3]),
            )
            .map(|_| ())
        } else {
            feature_dependencies(ctx, &scan, &ir, 17, &prototypes).map(|_| ())
        }
    });
    assert!(
        matches!(error, CodecError::ResourceLimit(resource)
        if resource.operation == operation),
        "{error:?}"
    );
}

#[test]
fn native_feature_dependencies_refuse_collection_limit() {
    feature_dependency_limit_error(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "creo native feature dependencies",
        true,
    );
}

#[test]
fn feature_dependency_id_refuses_retained_limit() {
    feature_dependency_limit_error(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "creo feature dependency IDs",
        false,
    );
}

#[test]
fn feature_dependencies_refuse_collection_limit() {
    feature_dependency_limit_error(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "creo feature dependencies",
        false,
    );
}

#[test]
fn feature_dependency_fixture_retains_source_order_under_service_policy() {
    let scan = crate::test_support::empty_container_scan();
    let mut ir = reconciliation_ir_with_generated_dependency();
    ir.model.features[0].id =
        IrFeatureId::mint("creo:model:feature#3").expect("fixture feature ID");
    let dependencies = crate::decode::with_test_decode_ctx(|ctx| {
        feature_dependencies(ctx, &scan, &ir, 17, &BTreeMap::from([(17, vec![3])]))
    })
    .expect("service profile admits one dependency");
    assert_eq!(
        dependencies,
        vec![IrFeatureId::mint("creo:model:feature#3").expect("fixture feature ID")]
    );
}

fn dependency_result(limit: u64) -> Result<BTreeMap<u32, Vec<u32>>, CodecError> {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("root input is admitted");
    let mut dependencies = BTreeMap::new();
    add_surface_prototype_feature_dependencies(&ctx, &mut dependencies, 40, &[286])?;
    Ok(dependencies)
}

#[test]
fn prototype_dependency_consumer_node_refuses_before_insertion() {
    assert_eq!(
        dependency_result(DecodePolicy::service().limits.max_collection_items)
            .expect("service limit admits the consumer and producer"),
        BTreeMap::from([(286, vec![40])])
    );
    let error = dependency_result(crate::test_support::allocation_limit_at(
        ResourceDimension::CollectionItems,
        Some("creo prototype dependency consumers"),
        dependency_result,
    ))
    .expect_err("one consumer requires a map node");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo prototype dependency consumers"
    ));
}

#[test]
fn prototype_dependency_producer_vec_refuses_before_growth() {
    let error = dependency_result(crate::test_support::allocation_limit_at(
        ResourceDimension::CollectionItems,
        Some("creo prototype dependency producers"),
        dependency_result,
    ))
    .expect_err("producer follows its consumer node");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo prototype dependency producers"
    ));
}

#[test]
fn surface_prototype_dependencies_point_from_consumers_to_unique_producers() {
    let dependencies = crate::test_support::assert_work_boundaries(
        &["creo prototype dependency producer lookup"],
        |ctx| {
            let mut dependencies = BTreeMap::new();
            add_surface_prototype_feature_dependencies(
                ctx,
                &mut dependencies,
                40,
                &[0, 40, 286, 286, 1111],
            )?;
            add_surface_prototype_feature_dependencies(ctx, &mut dependencies, 41, &[286])?;
            Ok::<_, CodecError>(dependencies)
        },
    );
    assert_eq!(
        dependencies,
        BTreeMap::from([(286, vec![40, 41]), (1111, vec![40])])
    );
}

#[test]
fn dependency_reconciliation_preserves_typed_history_edges() {
    let owner = IrFeatureId::mint("creo:model:feature#40".to_string()).expect("identity grammar");
    let sketch =
        IrFeatureId::mint("creo:model:sketch_feature#917".to_string()).expect("identity grammar");
    let parent = IrFeatureId::mint("creo:model:feature#3".to_string()).expect("identity grammar");
    let missing =
        IrFeatureId::mint("creo:model:feature#999".to_string()).expect("identity grammar");
    let emitted = [owner.clone(), sketch.clone(), parent.clone()]
        .into_iter()
        .collect();

    let dependencies = crate::test_support::assert_work_boundaries(
        &[
            "creo established dependency emission lookup",
            "creo native dependency emission lookup",
        ],
        |ctx| {
            reconciled_dependencies(
                ctx,
                &owner,
                &[sketch.clone(), missing.clone()],
                [parent.clone(), sketch.clone(), owner.clone()],
                &emitted,
            )
        },
    );
    assert_eq!(dependencies, vec![sketch, parent]);
}

#[test]
fn native_dependency_duplicate_membership_refuses_work_and_preserves_order() {
    let owner = IrFeatureId::mint("creo:model:feature#40").expect("identity grammar");
    let sketch = IrFeatureId::mint("creo:model:sketch_feature#917").expect("identity grammar");
    let parent = IrFeatureId::mint("creo:model:feature#3").expect("identity grammar");
    let missing = IrFeatureId::mint("creo:model:feature#999").expect("identity grammar");
    let emitted = [owner.clone(), sketch.clone(), parent.clone()]
        .into_iter()
        .collect();
    let dependencies = crate::test_support::assert_work_boundaries(
        &["creo native dependency duplicate lookup"],
        |ctx| {
            reconciled_dependencies(
                ctx,
                &owner,
                &[sketch.clone(), missing.clone()],
                [parent.clone(), sketch.clone(), owner.clone()],
                &emitted,
            )
        },
    );
    assert_eq!(dependencies, vec![sketch, parent]);
}

#[test]
fn unemitted_native_dependency_skips_duplicate_membership_at_work_limit() {
    let owner = IrFeatureId::mint("creo:model:feature#40").expect("identity grammar");
    let parent = IrFeatureId::mint("creo:model:feature#3").expect("identity grammar");
    let missing = IrFeatureId::mint("creo:model:feature#999").expect("identity grammar");
    let emitted = std::collections::BTreeSet::from([owner.clone(), parent.clone()]);
    let duplicate_query_reached = std::cell::Cell::new(false);
    let bounded = crate::test_support::assert_work_boundaries(
        &["creo native dependency emission lookup"],
        |ctx| {
            let result = reconciled_dependencies(
                ctx,
                &owner,
                std::slice::from_ref(&parent),
                [missing.clone()],
                &emitted,
            );
            if ctx.resource_refusal().is_some_and(|resource| {
                resource.operation == "creo native dependency duplicate lookup"
            }) {
                duplicate_query_reached.set(true);
            }
            result
        },
    );
    assert!(
        !duplicate_query_reached.get(),
        "an un-emitted dependency skips duplicate membership"
    );
    assert_eq!(bounded, vec![parent.clone()]);
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            reconciled_dependencies(
                ctx,
                &owner,
                std::slice::from_ref(&parent),
                [missing],
                &emitted,
            )
        })
        .expect("service profile preserves the established dependency"),
        vec![parent]
    );
}

#[test]
fn established_dependency_id_refuses_retained_limit() {
    let owner = IrFeatureId::mint("creo:model:feature#17").expect("fixture feature ID");
    let dependency = IrFeatureId::mint("creo:model:feature#3").expect("fixture dependency ID");
    let emitted = std::collections::BTreeSet::from([dependency.clone()]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        Some("creo established dependency IDs"),
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = policy;
            policy.limits.max_retained_bytes = cap;
            let dependency = dependency.clone();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                .expect("empty root is admitted");
            reconciled_dependencies(&ctx, &owner, &[dependency], [], &emitted).map(|_| ())
        },
    );
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let error = reconciled_dependencies(&ctx, &owner, &[dependency], [], &emitted)
        .expect_err("established dependency ID requires a retained copy");
    assert!(
        matches!(error, CodecError::ResourceLimit(resource)
        if resource.operation == "creo established dependency IDs"),
        "{error:?}"
    );
}

#[test]
fn reconciled_dependencies_refuse_collection_limit() {
    let owner = IrFeatureId::mint("creo:model:feature#17").expect("fixture feature ID");
    let dependency = IrFeatureId::mint("creo:model:feature#3").expect("fixture dependency ID");
    let emitted = std::collections::BTreeSet::from([dependency.clone()]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        Some("creo reconciled dependencies"),
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = policy;
            policy.limits.max_collection_items = cap;
            let dependency = dependency.clone();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                .expect("empty root is admitted");
            reconciled_dependencies(&ctx, &owner, &[], [dependency], &emitted).map(|_| ())
        },
    );
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let error = reconciled_dependencies(&ctx, &owner, &[], [dependency], &emitted)
        .expect_err("one reconciled dependency requires a Vec row");
    assert!(
        matches!(error, CodecError::ResourceLimit(resource)
        if resource.operation == "creo reconciled dependencies"),
        "{error:?}"
    );
}

#[test]
fn generated_face_dependencies_follow_the_producer_feature() {
    let producer =
        IrFeatureId::mint("creo:model:feature#97".to_string()).expect("identity grammar");
    let definition = IrFeatureDefinition::Operation(IrFeatureOperation::Thicken {
        faces: FaceSelection::generated(
            vec![GeneratedFaceRef::new(
                producer.clone(),
                "surface#98".to_string(),
                &cadmpeg_test_support::service_decode_context(),
            )
            .expect("selection reference admission")
            .expect("valid test fixture")],
            "creo:allfeatur:thicken#9".to_string(),
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("selection reference admission")
        .expect("valid test fixture"),
        thickness: None,
        side: None,
    });
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| feature_generated_dependencies(ctx, &definition))
            .expect("service profile admits generated dependencies")
            .into_iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec![producer]
    );
}

fn one_generated_face_dependency() -> IrFeatureDefinition {
    let producer = IrFeatureId::mint("creo:model:feature#97").expect("identity grammar");
    IrFeatureDefinition::Operation(IrFeatureOperation::Thicken {
        faces: FaceSelection::generated(
            vec![GeneratedFaceRef::new(
                producer,
                "surface#98".to_string(),
                &cadmpeg_test_support::service_decode_context(),
            )
            .expect("selection reference admission")
            .expect("valid test fixture")],
            "creo:allfeatur:thicken#9".to_string(),
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("selection reference admission")
        .expect("valid test fixture"),
        thickness: None,
        side: None,
    })
}

#[test]
fn generated_dependency_refuses_before_output_row() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        Some("creo generated dependencies"),
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = policy;
            policy.limits.max_collection_items = cap;
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                    .expect("empty root");
            feature_generated_dependencies(&ctx, &one_generated_face_dependency()).map(|_| ())
        },
    );
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    let error = feature_generated_dependencies(&ctx, &one_generated_face_dependency())
        .expect_err("one generated dependency needs a Vec row");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo generated dependencies")
    );
}

#[test]
fn generated_dependency_borrows_id_with_only_vector_storage() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes =
        cadmpeg_core::decode::u64_from_index(4 * std::mem::size_of::<&IrFeatureId>());
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    let definition = one_generated_face_dependency();
    let dependencies = feature_generated_dependencies(&ctx, &definition)
        .expect("borrowing a source ID needs no retained copy");
    assert_eq!(dependencies.len(), 1);
    assert_eq!(dependencies[0].as_str(), "creo:model:feature#97");
}

fn reconciliation_ir_with_generated_dependency() -> CadIr {
    let mut ir = CadIr::empty();
    ir.model.features.push(cadmpeg_ir::features::Feature {
        id: IrFeatureId::mint("creo:model:feature#10").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            one_generated_face_dependency(),
        ),
        native_ref: None,
    });
    ir
}

fn emitted_feature_identity_error(scoped: bool) {
    let scan = crate::test_support::empty_container_scan();
    let mut ir = reconciliation_ir_with_generated_dependency();
    ir.model.features[0].id =
        IrFeatureId::mint("creo:model:sketch_feature#10").expect("fixture feature ID");
    let operation = if scoped {
        "creo emitted feature identity text"
    } else {
        "creo emitted feature identity nodes"
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    if scoped {
        policy.limits.max_materialized_bytes = crate::test_support::allocation_limit_at(
            ResourceDimension::MaterializedBytes,
            Some(operation),
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_materialized_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                    .expect("empty root is admitted");
                super::reconcile_feature_links(&ctx, &scan, &mut ir.clone(), &BTreeMap::new())
            },
        );
    } else {
        policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            Some(operation),
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_collection_items = cap;
                let mut ir = ir.clone();
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                    .expect("empty root is admitted");
                super::reconcile_feature_links(&ctx, &scan, &mut ir, &BTreeMap::new())
            },
        );
    }
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let error = super::reconcile_feature_links(&ctx, &scan, &mut ir, &BTreeMap::new())
        .expect_err("one emitted feature identity exceeds the limit");
    assert!(
        matches!(error, CodecError::ResourceLimit(resource)
        if resource.operation == operation),
        "{error:?}"
    );
}

#[test]
fn emitted_feature_identity_text_refuses_scoped_limit() {
    emitted_feature_identity_error(true);
}

#[test]
fn emitted_feature_identity_nodes_refuse_collection_limit() {
    emitted_feature_identity_error(false);
}

fn reconciliation_ir_with_emitted_parent() -> CadIr {
    let mut ir = reconciliation_ir_with_generated_dependency();
    let mut parent = ir.model.features[0].clone();
    parent.id = IrFeatureId::mint("creo:model:feature#3").expect("fixture parent ID");
    parent.ordinal = 0;
    parent.evaluation = cadmpeg_ir::features::FeatureEvaluation::from_definition(
        IrFeatureDefinition::Operation(IrFeatureOperation::KnitSurface {
            faces: FaceSelection::Unresolved,
            merge_entities: Some(true),
            create_solid: Some(false),
            gap_tolerance: None,
        }),
    );
    ir.model.features[0].ordinal = 1;
    ir.model.features.insert(0, parent);
    ir
}

fn reconciliation_ir_for_ordering() -> CadIr {
    let mut ir = reconciliation_ir_with_emitted_parent();
    for feature in &mut ir.model.features {
        feature.evaluation = cadmpeg_ir::features::FeatureEvaluation::from_definition(
            IrFeatureDefinition::Operation(IrFeatureOperation::KnitSurface {
                faces: FaceSelection::Unresolved,
                merge_entities: Some(true),
                create_solid: Some(false),
                gap_tolerance: None,
            }),
        );
    }
    ir
}

fn feature_order_collection_error(operation: &'static str) {
    let scan = crate::test_support::empty_container_scan();
    let ir = reconciliation_ir_for_ordering();
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        operation,
        |ctx| super::reconcile_feature_links(ctx, &scan, &mut ir.clone(), &BTreeMap::new()),
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(resource)
        if resource.operation == operation),
        "{error:?}"
    );
}

#[test]
fn remaining_feature_order_refuses_collection_limit() {
    feature_order_collection_error("creo remaining feature order");
}

#[test]
fn ordered_feature_indices_refuse_collection_limit() {
    feature_order_collection_error("creo ordered feature indices");
}

#[test]
fn preceding_feature_identity_nodes_refuse_collection_limit() {
    feature_order_collection_error("creo preceding feature identity nodes");
}

#[test]
fn feature_order_fixture_preserves_parent_before_child() {
    let scan = crate::test_support::empty_container_scan();
    let mut ir = reconciliation_ir_for_ordering();
    crate::decode::with_test_decode_ctx(|ctx| {
        super::reconcile_feature_links(ctx, &scan, &mut ir, &BTreeMap::new())
    })
    .expect("service profile admits feature ordering");
    assert_eq!(ir.model.features[0].ordinal, 0);
    assert_eq!(ir.model.features[1].ordinal, 1);
}

fn regeneration_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features
        .operations
        .push(crate::feature::operations::FeatureOperation {
            feature_id: 10,
            kind: crate::feature::operations::OperationKind::Stored("Sweep".to_string()),
            name: crate::feature::operations::OperationName::Derived,
            recipe: crate::feature::operations::RecipeResolution::Resolved(
                crate::feature::operations::FeatureRecipe::ProtrudeExtrude,
            ),
            display_state_conflict: false,
            depdb: Some(crate::feature::operations::DepdbPrefix {
                schema: crate::feature::schema::SchemaClass::Protrusion,
                parent: 3,
            }),
            offset: 100,
            state_offset: 100,
        });
    scan
}

fn regeneration_edge_limit_error(
    dimension: cadmpeg_core::decode::ResourceDimension,
    operation: &'static str,
) {
    let scan = regeneration_scan();
    let ir = reconciliation_ir_for_ordering();
    let error = crate::test_support::last_refusal_at(&[], dimension, operation, |ctx| {
        super::reconcile_feature_links(ctx, &scan, &mut ir.clone(), &BTreeMap::new())
    });
    assert!(
        matches!(error, CodecError::ResourceLimit(resource)
        if resource.operation == operation),
        "{error:?}"
    );
}

#[test]
fn regeneration_parent_id_refuses_retained_limit() {
    regeneration_edge_limit_error(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "creo regeneration parent IDs",
    );
}

#[test]
fn regeneration_child_id_refuses_retained_limit() {
    regeneration_edge_limit_error(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "creo regeneration child IDs",
    );
}

#[test]
fn regeneration_edges_refuse_collection_limit() {
    // The dependency uniqueness index admits one borrowed member first.
    regeneration_edge_limit_error(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "creo regeneration edges",
    );
}

#[test]
fn regeneration_parent_nodes_refuse_collection_limit() {
    // The dependency uniqueness index stores one borrowed member.
    regeneration_edge_limit_error(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "install decoded feature regeneration parent",
    );
}

#[test]
fn regeneration_fixture_preserves_parent_relation_under_service_policy() {
    let scan = regeneration_scan();
    let mut ir = reconciliation_ir_for_ordering();
    crate::decode::with_test_decode_ctx(|ctx| {
        super::reconcile_feature_links(ctx, &scan, &mut ir, &BTreeMap::new())
    })
    .expect("service profile admits the regeneration parent");
    let child = IrFeatureId::mint("creo:model:feature#10").expect("fixture child ID");
    let parent = IrFeatureId::mint("creo:model:feature#3").expect("fixture parent ID");
    assert_eq!(ir.model.feature_regeneration_parent(&child), Some(&parent));
}

fn reconciled_native_dependency_error(
    dimension: cadmpeg_core::decode::ResourceDimension,
    operation: &'static str,
) {
    let scan = crate::test_support::empty_container_scan();
    let ir = reconciliation_ir_with_emitted_parent();
    let error = crate::test_support::last_refusal_at(&[], dimension, operation, |ctx| {
        super::reconcile_feature_links(
            ctx,
            &scan,
            &mut ir.clone(),
            &BTreeMap::from([(10, vec![3])]),
        )
    });
    assert!(
        matches!(error, CodecError::ResourceLimit(resource)
        if resource.operation == operation),
        "{error:?}"
    );
}

#[test]
fn reconciled_native_dependency_id_refuses_retained_limit() {
    reconciled_native_dependency_error(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "creo reconciled native dependency IDs",
    );
}

#[test]
fn reconciled_native_dependencies_refuse_collection_limit() {
    reconciled_native_dependency_error(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "creo reconciled native dependencies",
    );
}

#[test]
fn reconciled_native_dependency_preserves_emitted_parent_under_service_policy() {
    let scan = crate::test_support::empty_container_scan();
    let mut ir = reconciliation_ir_with_emitted_parent();
    crate::decode::with_test_decode_ctx(|ctx| {
        super::reconcile_feature_links(ctx, &scan, &mut ir, &BTreeMap::from([(10, vec![3])]))
    })
    .expect("service profile admits native dependency reconciliation");
    let feature = ir
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#10")
        .expect("child feature exists");
    assert_eq!(
        feature.dependencies.as_slice(),
        &[IrFeatureId::mint("creo:model:feature#3").expect("fixture parent ID")]
    );
}

#[test]
fn duplicate_generated_edges_keep_one_dependency() {
    let producer = IrFeatureId::mint("creo:model:feature#97").expect("identity grammar");
    let edges = EdgeSelection::generated(
        vec![
            GeneratedEdgeRef::new(
                producer.clone(),
                "curve#77".to_string(),
                &cadmpeg_test_support::service_decode_context(),
            )
            .expect("selection reference admission")
            .expect("valid generated edge"),
            GeneratedEdgeRef::new(
                producer.clone(),
                "curve#78".to_string(),
                &cadmpeg_test_support::service_decode_context(),
            )
            .expect("selection reference admission")
            .expect("valid generated edge"),
        ],
        "creo:allfeatur:fillet#9".to_string(),
        &cadmpeg_test_support::service_decode_context(),
    )
    .expect("selection reference admission")
    .expect("valid edge selection");
    let definition = IrFeatureDefinition::Operation(IrFeatureOperation::Fillet {
        groups: cadmpeg_ir::features::NonEmptyMembers::one(
            cadmpeg_ir::features::edge_treatments::FilletGroup {
                edges,
                radius: RadiusSpec::Unresolved { form: None },
                tangency_weight: None,
            },
        ),
    });
    let dependencies =
        crate::decode::with_test_decode_ctx(|ctx| feature_generated_dependencies(ctx, &definition))
            .expect("service profile admits one unique dependency");
    assert_eq!(
        dependencies.into_iter().cloned().collect::<Vec<_>>(),
        vec![producer]
    );
}

#[test]
fn generated_edge_dependencies_follow_the_producer_feature() {
    let producer =
        IrFeatureId::mint("creo:model:feature#97".to_string()).expect("identity grammar");
    let generated_edges = EdgeSelection::generated(
        vec![GeneratedEdgeRef::new(
            producer.clone(),
            "curve#77".to_string(),
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("selection reference admission")
        .expect("valid test fixture")],
        "creo:allfeatur:fillet#9".to_string(),
        &cadmpeg_test_support::service_decode_context(),
    )
    .expect("selection reference admission")
    .expect("valid test fixture");
    let fillet = IrFeatureDefinition::Operation(IrFeatureOperation::Fillet {
        groups: cadmpeg_ir::features::NonEmptyMembers::one(
            cadmpeg_ir::features::edge_treatments::FilletGroup {
                edges: generated_edges.clone(),
                radius: RadiusSpec::Unresolved { form: None },
                tangency_weight: None,
            },
        ),
    });
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| feature_generated_dependencies(ctx, &fillet))
            .expect("service profile admits generated dependencies")
            .into_iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec![producer.clone()]
    );

    let chamfer = IrFeatureDefinition::Operation(IrFeatureOperation::Chamfer {
        groups: cadmpeg_ir::features::NonEmptyMembers::one(
            cadmpeg_ir::features::edge_treatments::ChamferGroup {
                edges: generated_edges,
                spec: cadmpeg_ir::features::edge_treatments::ChamferSpec::Unresolved { form: None },
            },
        ),
        flip_direction: false,
    });
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| feature_generated_dependencies(ctx, &chamfer))
            .expect("service profile admits generated dependencies")
            .into_iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec![producer]
    );
}

#[test]
fn feature_dependency_copy_refuses_at_work_boundary() {
    let scan = crate::test_support::empty_container_scan();
    let mut ir = reconciliation_ir_with_generated_dependency();
    ir.model.features[0].id =
        IrFeatureId::mint("creo:model:feature#3").expect("fixture feature ID");
    let dependencies =
        crate::test_support::assert_work_boundaries(&["creo feature dependency IDs"], |ctx| {
            feature_dependencies(ctx, &scan, &ir, 17, &BTreeMap::from([(17, vec![3])]))
        });
    assert_eq!(
        dependencies,
        vec![IrFeatureId::mint("creo:model:feature#3").expect("fixture dependency ID")],
    );
}

#[test]
fn reconciliation_identity_and_order_work_boundaries_preserve_parent_edges() {
    let scan = regeneration_scan();
    let ir = crate::test_support::assert_work_boundaries(
        &[
            "creo emitted feature identity lookup",
            "creo reconciled feature emission lookup",
            "creo regeneration parent identity lookup",
            "creo emitted dependency identity lookup",
            "creo preceding dependency identity lookup",
            "creo remaining feature order search",
        ],
        |ctx| {
            let mut ir = reconciliation_ir_for_ordering();
            super::reconcile_feature_links(ctx, &scan, &mut ir, &BTreeMap::new())?;
            Ok::<_, CodecError>(ir)
        },
    );
    let child = IrFeatureId::mint("creo:model:feature#10").expect("fixture child ID");
    let parent = IrFeatureId::mint("creo:model:feature#3").expect("fixture parent ID");
    let child_record = ir
        .model
        .features
        .iter()
        .find(|feature| feature.id == child)
        .expect("child feature exists");
    assert_eq!(
        child_record.dependencies.as_slice(),
        std::slice::from_ref(&parent)
    );
    assert_eq!(ir.model.feature_regeneration_parent(&child), Some(&parent));
    assert_eq!(ir.model.features[0].ordinal, 0);
    assert_eq!(ir.model.features[1].ordinal, 1);
}

#[test]
fn duplicate_emitted_feature_identity_membership_refuses_work_and_preserves_order() {
    let scan = regeneration_scan();
    let mut initial_ir = reconciliation_ir_with_emitted_parent();
    let duplicate_parent = initial_ir.model.features[0].clone();
    initial_ir.model.features.push(duplicate_parent);
    let ir = crate::test_support::assert_work_boundaries(
        &["creo emitted feature identity lookup"],
        |ctx| {
            let mut ir = initial_ir.clone();
            super::reconcile_feature_links(ctx, &scan, &mut ir, &BTreeMap::new())?;
            Ok::<_, CodecError>(ir)
        },
    );
    let parent = IrFeatureId::mint("creo:model:feature#3").expect("fixture parent ID");
    let child = IrFeatureId::mint("creo:model:feature#10").expect("fixture child ID");
    let feature_ids = ir
        .model
        .features
        .iter()
        .map(|feature| feature.id.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        feature_ids,
        vec![
            parent,
            child,
            IrFeatureId::mint("creo:model:feature#3").expect("duplicate parent ID"),
        ]
    );
    let ordinals = ir
        .model
        .features
        .iter()
        .map(|feature| feature.ordinal)
        .collect::<Vec<_>>();
    assert_eq!(ordinals, vec![0, 1, 2]);
}

#[test]
fn remaining_feature_order_removal_charges_only_suffix_bytes() {
    let scan = regeneration_scan();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "creo remaining feature order removal shifts",
        |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                .expect("empty root is admitted");
            let mut ir = reconciliation_ir_for_ordering();
            super::reconcile_feature_links(&ctx, &scan, &mut ir, &BTreeMap::new())
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(ref resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo remaining feature order removal shifts"
            && resource.additional == cadmpeg_core::decode::u64_from_index(
                std::mem::size_of::<usize>(),
            )),
        "{error:?}"
    );

    let mut ir = reconciliation_ir_for_ordering();
    crate::decode::with_test_decode_ctx(|ctx| {
        super::reconcile_feature_links(ctx, &scan, &mut ir, &BTreeMap::new())
    })
    .expect("service profile admits feature ordering");
    assert_eq!(ir.model.features[0].ordinal, 0);
    assert_eq!(ir.model.features[1].ordinal, 1);
}

#[test]
fn empty_feature_reconciliation_skips_operation_index() {
    let scan = regeneration_scan();
    let operation_query = std::cell::Cell::new(false);
    let ir = crate::test_support::assert_work_boundaries(&[], |ctx| {
        let mut ir = CadIr::empty();
        let result = super::reconcile_feature_links(ctx, &scan, &mut ir, &BTreeMap::new());
        if let Err(CodecError::ResourceLimit(resource)) = &result {
            if matches!(
                resource.operation,
                "creo current operation rows" | "creo current operation index nodes"
            ) {
                operation_query.set(true);
            }
        }
        result.map(|()| ir)
    });
    assert!(ir.model.features.is_empty());
    assert!(!operation_query.get());
}
