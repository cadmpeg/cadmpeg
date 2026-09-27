// SPDX-License-Identifier: Apache-2.0

use super::{
    add_surface_prototype_feature_dependencies, feature_generated_dependencies,
    reconciled_dependencies,
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
        reconciled_dependencies(
            &owner,
            &[sketch.clone(), missing],
            [parent.clone(), sketch.clone(), owner.clone()],
            &emitted,
        ),
        vec![sketch, parent]
    );
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

#[test]
fn reconciled_generated_dependency_refuses_before_retained_id() {
    let scan = crate::container::scan_bytes_ok(Vec::new());
    let mut ir = reconciliation_ir_with_generated_dependency();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
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
    policy.limits.max_collection_items = 5;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    let error = crate::decode::feature_history::dependencies::reconcile_feature_links(
        &ctx,
        &scan,
        &mut ir,
        &BTreeMap::new(),
    )
    .expect_err("the sixth collection item owns the generated dependency");
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
