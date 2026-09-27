// SPDX-License-Identifier: Apache-2.0

use super::{
    add_surface_prototype_feature_dependencies, feature_dependencies,
    feature_entity_dependencies, feature_generated_dependencies,
    feature_output_surface_dependencies, native_feature_dependency_ids,
    reconciled_dependencies, surface_merge_entity_dependencies,
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
            payload: crate::feature::entity::entry_payload(
                class_id,
                source_entity_id,
                None,
                None,
            ),
            entity_id,
            prefixed: true,
            offset: offset + 1,
            end_offset: offset + 2,
        }],
        &std::collections::BTreeSet::new(),
        offset,
    )
}

fn dependency_collection_error(
    limit: u64,
    operation: &'static str,
    route: &str,
) {
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
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let error = match route {
        "parent" => native_feature_dependency_ids(
            &ctx, &[parent], &[], &[], &[], &[], 17, &[],
        ).map(|_| ()),
        "owned" | "output" => feature_output_surface_dependencies(
            &ctx, &[owned, consumer], &[surface], 17,
        ).map(|_| ()),
        "entity" => feature_entity_dependencies(
            &ctx, &[producer, consumer], 17,
        ).map(|_| ()),
        "merge" => surface_merge_entity_dependencies(
            &ctx, &[], &[replay], &[producer], 17,
        ).map(|_| ()),
        _ => panic!("unknown dependency fixture route"),
    }
    .expect_err("one dependency exceeds the collection limit");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.operation == operation), "{error:?}");
}

#[test]
fn agreed_feature_parent_ids_refuse_collection_limit() {
    dependency_collection_error(0, "creo agreed feature parent IDs", "parent");
}

#[test]
fn output_surface_owned_entity_nodes_refuse_collection_limit() {
    dependency_collection_error(0, "creo output surface owned entity nodes", "owned");
}

#[test]
fn output_surface_dependencies_refuse_collection_limit() {
    dependency_collection_error(1, "creo output surface dependencies", "output");
}

#[test]
fn feature_entity_dependencies_refuse_collection_limit() {
    dependency_collection_error(0, "creo feature entity dependencies", "entity");
}

#[test]
fn surface_merge_dependencies_refuse_collection_limit() {
    dependency_collection_error(0, "creo surface merge dependencies", "merge");
}

fn feature_dependency_limit_error(
    collection: Option<u64>,
    retained: Option<u64>,
    operation: &'static str,
    native_only: bool,
) {
    let scan = crate::container::scan_bytes_ok(Vec::new());
    let mut ir = reconciliation_ir_with_generated_dependency();
    ir.model.features[0].id = IrFeatureId::mint("creo:model:feature#3")
        .expect("fixture feature ID");
    let prototypes = BTreeMap::from([(17, vec![3])]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    if let Some(limit) = collection {
        policy.limits.max_collection_items = limit;
    }
    if let Some(limit) = retained {
        policy.limits.max_retained_bytes = limit;
    }
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let error = if native_only {
        native_feature_dependency_ids(
            &ctx,
            &scan.features.affected_ids,
            &scan.features.operations,
            &scan.features.entity_tables,
            &scan.features.surface_merge_replay_affected_ids,
            &scan.surfaces.rows,
            17,
            &[3],
        )
        .map(|_| ())
    } else {
        feature_dependencies(&ctx, &scan, &ir, 17, &prototypes).map(|_| ())
    }
    .expect_err("one dependency exceeds the resource limit");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.operation == operation), "{error:?}");
}

#[test]
fn native_feature_dependencies_refuse_collection_limit() {
    feature_dependency_limit_error(Some(0), None, "creo native feature dependencies", true);
}

#[test]
fn feature_dependency_id_refuses_retained_limit() {
    feature_dependency_limit_error(None, Some(0), "creo feature dependency IDs", false);
}

#[test]
fn feature_dependencies_refuse_collection_limit() {
    feature_dependency_limit_error(Some(1), None, "creo feature dependencies", false);
}

#[test]
fn feature_dependency_fixture_retains_source_order_under_service_policy() {
    let scan = crate::container::scan_bytes_ok(Vec::new());
    let mut ir = reconciliation_ir_with_generated_dependency();
    ir.model.features[0].id = IrFeatureId::mint("creo:model:feature#3")
        .expect("fixture feature ID");
    let dependencies = crate::decode::with_test_decode_ctx(|ctx| {
        feature_dependencies(ctx, &scan, &ir, 17, &BTreeMap::from([(17, vec![3])]))
    })
    .expect("service profile admits one dependency");
    assert_eq!(dependencies, vec![IrFeatureId::mint("creo:model:feature#3")
        .expect("fixture feature ID")]);
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
        dependency_result(2).expect("service limit admits the consumer and producer"),
        BTreeMap::from([(286, vec![40])])
    );
    let error = dependency_result(0).expect_err("one consumer requires a map node");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo prototype dependency consumers"
    ));
}

#[test]
fn prototype_dependency_producer_vec_refuses_before_growth() {
    let error = dependency_result(1).expect_err("producer follows its consumer node");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo prototype dependency producers"
    ));
}

#[test]
fn surface_prototype_dependencies_point_from_consumers_to_unique_producers() {
    let mut dependencies = BTreeMap::new();
    crate::decode::with_test_decode_ctx(|ctx| {
        add_surface_prototype_feature_dependencies(
            ctx,
            &mut dependencies,
            40,
            &[0, 40, 286, 286, 1111],
        )?;
        add_surface_prototype_feature_dependencies(ctx, &mut dependencies, 41, &[286])
    })
    .expect("service profile admits prototype dependencies");

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

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| reconciled_dependencies(
            ctx, &owner, &[sketch.clone(), missing],
            [parent.clone(), sketch.clone(), owner.clone()], &emitted,
        )).expect("service profile admits reconciled dependencies"),
        vec![sketch, parent]
    );
}

#[test]
fn established_dependency_id_refuses_retained_limit() {
    let owner = IrFeatureId::mint("creo:model:feature#17")
        .expect("fixture feature ID");
    let dependency = IrFeatureId::mint("creo:model:feature#3")
        .expect("fixture dependency ID");
    let emitted = std::collections::BTreeSet::from([dependency.clone()]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let error = reconciled_dependencies(&ctx, &owner, &[dependency], [], &emitted)
        .expect_err("established dependency ID requires a retained copy");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.operation == "creo established dependency IDs"), "{error:?}");
}

#[test]
fn reconciled_dependencies_refuse_collection_limit() {
    let owner = IrFeatureId::mint("creo:model:feature#17")
        .expect("fixture feature ID");
    let dependency = IrFeatureId::mint("creo:model:feature#3")
        .expect("fixture dependency ID");
    let emitted = std::collections::BTreeSet::from([dependency.clone()]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let error = reconciled_dependencies(&ctx, &owner, &[], [dependency], &emitted)
        .expect_err("one reconciled dependency requires a Vec row");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.operation == "creo reconciled dependencies"), "{error:?}");
}


#[test]
fn generated_face_dependencies_follow_the_producer_feature() {
    let producer =
        IrFeatureId::mint("creo:model:feature#97".to_string()).expect("identity grammar");
    let definition = IrFeatureDefinition::Operation(IrFeatureOperation::Thicken {
        faces: FaceSelection::generated(
            vec![
                GeneratedFaceRef::new(producer.clone(), "surface#98".to_string())
                    .expect("valid test fixture"),
            ],
            "creo:allfeatur:thicken#9".to_string(),
        )
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
            vec![
                GeneratedFaceRef::new(producer, "surface#98".to_string())
                    .expect("valid test fixture"),
            ],
            "creo:allfeatur:thicken#9".to_string(),
        )
        .expect("valid test fixture"),
        thickness: None,
        side: None,
    })
}

#[test]
fn generated_dependency_refuses_before_output_row() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    let error = feature_generated_dependencies(&ctx, &one_generated_face_dependency())
        .expect_err("one generated dependency needs a Vec row");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo generated dependencies"));
}

#[test]
fn generated_dependency_borrows_id_under_zero_retained_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
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

fn emitted_feature_identity_error(retained: bool) {
    let scan = crate::container::scan_bytes_ok(Vec::new());
    let mut ir = reconciliation_ir_with_generated_dependency();
    ir.model.features[0].id = IrFeatureId::mint("creo:model:sketch_feature#10")
        .expect("fixture feature ID");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    if retained {
        policy.limits.max_retained_bytes = 0;
    } else {
        policy.limits.max_collection_items = 0;
    }
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let error = super::reconcile_feature_links(&ctx, &scan, &mut ir, &BTreeMap::new())
        .expect_err("one emitted feature identity exceeds the limit");
    let operation = if retained {
        "creo emitted feature identity text"
    } else {
        "creo emitted feature identity nodes"
    };
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.operation == operation), "{error:?}");
}

#[test]
fn emitted_feature_identity_text_refuses_retained_limit() {
    emitted_feature_identity_error(true);
}

#[test]
fn emitted_feature_identity_nodes_refuse_collection_limit() {
    emitted_feature_identity_error(false);
}

fn reconciliation_ir_with_emitted_parent() -> CadIr {
    let mut ir = reconciliation_ir_with_generated_dependency();
    let mut parent = ir.model.features[0].clone();
    parent.id = IrFeatureId::mint("creo:model:feature#3")
        .expect("fixture parent ID");
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

fn feature_order_collection_error(limit: u64, operation: &'static str) {
    let scan = crate::container::scan_bytes_ok(Vec::new());
    let mut ir = reconciliation_ir_for_ordering();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let error = super::reconcile_feature_links(&ctx, &scan, &mut ir, &BTreeMap::new())
        .expect_err("two feature indices exceed the collection limit");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.operation == operation), "{error:?}");
}

#[test]
fn remaining_feature_order_refuses_collection_limit() {
    feature_order_collection_error(6, "creo remaining feature order");
}

#[test]
fn ordered_feature_indices_refuse_collection_limit() {
    feature_order_collection_error(8, "creo ordered feature indices");
}

#[test]
fn preceding_feature_identity_nodes_refuse_collection_limit() {
    feature_order_collection_error(10, "creo preceding feature identity nodes");
}

#[test]
fn feature_order_fixture_preserves_parent_before_child() {
    let scan = crate::container::scan_bytes_ok(Vec::new());
    let mut ir = reconciliation_ir_for_ordering();
    crate::decode::with_test_decode_ctx(|ctx| {
        super::reconcile_feature_links(ctx, &scan, &mut ir, &BTreeMap::new())
    })
    .expect("service profile admits feature ordering");
    assert_eq!(ir.model.features[0].ordinal, 0);
    assert_eq!(ir.model.features[1].ordinal, 1);
}

fn regeneration_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    scan.features.operations.push(crate::feature::operations::FeatureOperation {
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
    collection: Option<u64>,
    retained: Option<u64>,
    operation: &'static str,
) {
    let scan = regeneration_scan();
    let mut ir = reconciliation_ir_for_ordering();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    if let Some(limit) = collection {
        policy.limits.max_collection_items = limit;
    }
    if let Some(limit) = retained {
        policy.limits.max_retained_bytes = limit;
    }
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let error = super::reconcile_feature_links(&ctx, &scan, &mut ir, &BTreeMap::new())
        .expect_err("one regeneration edge exceeds the resource limit");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.operation == operation), "{error:?}");
}

#[test]
fn regeneration_parent_id_refuses_retained_limit() {
    let parent = "creo:model:feature#3".len();
    let child = "creo:model:feature#10".len();
    regeneration_edge_limit_error(
        None,
        Some((parent * 2 + child) as u64),
        "creo regeneration parent IDs",
    );
}

#[test]
fn regeneration_child_id_refuses_retained_limit() {
    let parent = "creo:model:feature#3".len();
    let child = "creo:model:feature#10".len();
    regeneration_edge_limit_error(
        None,
        Some((parent * 3 + child) as u64),
        "creo regeneration child IDs",
    );
}

#[test]
fn regeneration_edges_refuse_collection_limit() {
    regeneration_edge_limit_error(Some(9), None, "creo regeneration edges");
}

#[test]
fn regeneration_parent_nodes_refuse_collection_limit() {
    regeneration_edge_limit_error(Some(10), None, "creo regeneration parent nodes");
}

#[test]
fn regeneration_fixture_preserves_parent_relation_under_service_policy() {
    let scan = regeneration_scan();
    let mut ir = reconciliation_ir_for_ordering();
    crate::decode::with_test_decode_ctx(|ctx| {
        super::reconcile_feature_links(ctx, &scan, &mut ir, &BTreeMap::new())
    })
    .expect("service profile admits the regeneration parent");
    let child = IrFeatureId::mint("creo:model:feature#10")
        .expect("fixture child ID");
    let parent = IrFeatureId::mint("creo:model:feature#3")
        .expect("fixture parent ID");
    assert_eq!(ir.model.feature_regeneration_parent(&child), Some(&parent));
}

fn reconciled_native_dependency_error(
    collection: Option<u64>,
    retained: Option<u64>,
    operation: &'static str,
) {
    let scan = crate::container::scan_bytes_ok(Vec::new());
    let mut ir = reconciliation_ir_with_emitted_parent();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    if let Some(limit) = collection {
        policy.limits.max_collection_items = limit;
    }
    if let Some(limit) = retained {
        policy.limits.max_retained_bytes = limit;
    }
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let error = super::reconcile_feature_links(
        &ctx,
        &scan,
        &mut ir,
        &BTreeMap::from([(10, vec![3])]),
    )
    .expect_err("one native dependency exceeds the limit");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.operation == operation), "{error:?}");
}

#[test]
fn reconciled_native_dependency_id_refuses_retained_limit() {
    reconciled_native_dependency_error(
        None,
        Some(("creo:model:feature#3".len() + "creo:model:feature#10".len()) as u64),
        "creo reconciled native dependency IDs",
    );
}

#[test]
fn reconciled_native_dependencies_refuse_collection_limit() {
    reconciled_native_dependency_error(
        Some(9),
        None,
        "creo reconciled native dependencies",
    );
}

#[test]
fn reconciled_native_dependency_preserves_emitted_parent_under_service_policy() {
    let scan = crate::container::scan_bytes_ok(Vec::new());
    let mut ir = reconciliation_ir_with_emitted_parent();
    crate::decode::with_test_decode_ctx(|ctx| {
        super::reconcile_feature_links(
            ctx,
            &scan,
            &mut ir,
            &BTreeMap::from([(10, vec![3])]),
        )
    })
    .expect("service profile admits native dependency reconciliation");
    let feature = ir.model.features.iter().find(|feature| feature.id.as_str() == "creo:model:feature#10")
        .expect("child feature exists");
    assert_eq!(feature.dependencies.as_slice(), &[IrFeatureId::mint("creo:model:feature#3")
        .expect("fixture parent ID")]);
}

#[test]
fn reconciled_generated_dependency_refuses_before_retained_id() {
    let scan = crate::container::scan_bytes_ok(Vec::new());
    let mut ir = reconciliation_ir_with_generated_dependency();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = "creo:model:feature#10".len() as u64;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    let error = crate::decode::feature_history::dependencies::reconcile_feature_links(
        &ctx,
        &scan,
        &mut ir,
        &BTreeMap::new(),
    )
    .expect_err("reconciliation retains one generated producer ID");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && resource.operation == "creo reconciled generated dependency IDs"));
}

#[test]
fn reconciled_generated_dependency_refuses_before_owned_row() {
    let scan = crate::container::scan_bytes_ok(Vec::new());
    let mut ir = reconciliation_ir_with_generated_dependency();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 6;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    let error = crate::decode::feature_history::dependencies::reconcile_feature_links(
        &ctx,
        &scan,
        &mut ir,
        &BTreeMap::new(),
    )
    .expect_err("the seventh collection item owns the generated dependency");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo reconciled generated dependencies"));
}

#[test]
fn duplicate_generated_edges_keep_one_dependency() {
    let producer = IrFeatureId::mint("creo:model:feature#97").expect("identity grammar");
    let edges = EdgeSelection::generated(
        vec![
            GeneratedEdgeRef::new(producer.clone(), "curve#77".to_string())
                .expect("valid generated edge"),
            GeneratedEdgeRef::new(producer.clone(), "curve#78".to_string())
                .expect("valid generated edge"),
        ],
        "creo:allfeatur:fillet#9".to_string(),
    )
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
    let dependencies = crate::decode::with_test_decode_ctx(|ctx| {
        feature_generated_dependencies(ctx, &definition)
    })
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
        vec![
            GeneratedEdgeRef::new(producer.clone(), "curve#77".to_string())
                .expect("valid test fixture"),
        ],
        "creo:allfeatur:fillet#9".to_string(),
    )
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
