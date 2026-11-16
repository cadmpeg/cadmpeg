// SPDX-License-Identifier: Apache-2.0

use super::{
    add_surface_prototype_feature_dependencies, feature_dependencies,
    feature_generated_dependencies, reconciled_dependencies,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    edge_treatments::RadiusSpec, EdgeSelection, FaceSelection,
    FeatureDefinition as IrFeatureDefinition, FeatureId as IrFeatureId,
    FeatureOperation as IrFeatureOperation, GeneratedEdgeRef, GeneratedFaceRef,
};
use std::collections::BTreeMap;

#[test]
fn feature_dependency_fixture_retains_source_order() {
    let scan = crate::test_support::empty_container_scan();
    let mut ir = reconciliation_ir_with_generated_dependency();
    ir.model.features[0].id =
        IrFeatureId::mint("creo:model:feature#3").expect("fixture feature ID");
    let dependencies = feature_dependencies(&scan, &ir, 17, &BTreeMap::from([(17, vec![3])]))
        .expect("feature dependencies");
    assert_eq!(
        dependencies,
        vec![IrFeatureId::mint("creo:model:feature#3").expect("fixture feature ID")]
    );
}

#[test]
fn conflicting_creation_recipe_keeps_independent_parent_evidence() {
    use crate::feature::operations::{
        DepdbPrefix, FeatureOperation, FeatureRecipe, OperationKind, OperationName,
        RecipeResolution,
    };
    use crate::feature::rows::{AffectedIdKind, FeatureAffectedIds, FeatureRow};
    use crate::feature::schema::SchemaClass;

    let mut scan = crate::test_support::empty_container_scan();
    let audit = FeatureOperation {
        feature_id: 17,
        kind: OperationKind::Extrude,
        name: OperationName::Derived,
        recipe: RecipeResolution::Resolved(FeatureRecipe::ProtrudeExtrude),
        display_state_conflict: false,
        depdb: Some(DepdbPrefix {
            schema: SchemaClass::Protrusion,
            parent: 3,
        }),
        offset: 0,
    };
    scan.features.operations.push(audit.clone());
    scan.features.rows.push(FeatureRow {
        feature_id: 17,
        root_schema_class: Some(SchemaClass::Cut),
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("row header"),
        body_offset: 0,
        offset: 0,
    });
    let ir = reconciliation_ir_with_emitted_parent();
    assert!(feature_dependencies(&scan, &ir, 17, &BTreeMap::new())
        .expect("dependencies")
        .is_empty());
    scan.features.affected_ids.push(FeatureAffectedIds {
        feature_id: 17,
        kind: AffectedIdKind::Parents,
        ids: vec![3],
        offset: 0,
    });
    let expected = vec![IrFeatureId::mint("creo:model:feature#3").expect("parent")];
    assert_eq!(
        feature_dependencies(&scan, &ir, 17, &BTreeMap::new()).expect("dependencies"),
        expected
    );
    scan.features.affected_ids.clear();
    assert_eq!(
        feature_dependencies(&scan, &ir, 17, &BTreeMap::from([(17, vec![3])]))
            .expect("dependencies"),
        expected
    );
    assert_eq!(scan.features.operations[0], audit);
}

#[test]
fn surface_prototype_dependencies_point_from_consumers_to_unique_producers() {
    let mut dependencies = BTreeMap::new();
    add_surface_prototype_feature_dependencies(&mut dependencies, 40, &[0, 40, 286, 286, 1111]);
    add_surface_prototype_feature_dependencies(&mut dependencies, 41, &[286]);

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
            &emitted
        )
        .expect("reconciled dependencies"),
        vec![sketch, parent]
    );
}

#[test]
fn generated_face_dependencies_follow_the_producer_feature() {
    let producer =
        IrFeatureId::mint("creo:model:feature#97".to_string()).expect("identity grammar");
    let definition = IrFeatureDefinition::Operation(IrFeatureOperation::thicken(
        FaceSelection::generated(
            vec![
                GeneratedFaceRef::new(producer.clone(), "surface#98".to_string())
                    .expect("valid test fixture"),
            ],
            "creo:allfeatur:thicken#9".to_string(),
        )
        .expect("valid test fixture"),
        None,
        None,
    ));
    assert_eq!(
        feature_generated_dependencies(&definition)
            .expect("generated dependencies")
            .into_iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec![producer]
    );
}

fn one_generated_face_dependency() -> IrFeatureDefinition {
    let producer = IrFeatureId::mint("creo:model:feature#97").expect("identity grammar");
    IrFeatureDefinition::Operation(IrFeatureOperation::thicken(
        FaceSelection::generated(
            vec![GeneratedFaceRef::new(producer, "surface#98".to_string())
                .expect("valid test fixture")],
            "creo:allfeatur:thicken#9".to_string(),
        )
        .expect("valid test fixture"),
        None,
        None,
    ))
}

fn reconciliation_ir_with_generated_dependency() -> CadIr {
    let mut ir = CadIr::empty();
    ir.model.features.push(cadmpeg_ir::features::Feature {
        product_definition: None,
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

#[test]
fn feature_order_fixture_preserves_parent_before_child() {
    let scan = crate::test_support::empty_container_scan();
    let mut ir = reconciliation_ir_for_ordering();
    crate::decode::with_service_context(|ctx| {
        super::reconcile_feature_links(ctx, &scan, &mut ir, &BTreeMap::new(), &mut Vec::new())
    })
    .expect("feature ordering");
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
        });
    scan
}

#[test]
fn regeneration_fixture_preserves_parent_relation() {
    let scan = regeneration_scan();
    let mut ir = reconciliation_ir_for_ordering();
    crate::decode::with_service_context(|ctx| {
        super::reconcile_feature_links(ctx, &scan, &mut ir, &BTreeMap::new(), &mut Vec::new())
    })
    .expect("regeneration parent");
    let child = IrFeatureId::mint("creo:model:feature#10").expect("fixture child ID");
    let parent = IrFeatureId::mint("creo:model:feature#3").expect("fixture parent ID");
    assert_eq!(ir.model.feature_regeneration_parent(&child), Some(&parent));
}

#[test]
fn recipe_parent_stored_after_its_child_keeps_its_edge_after_ordering() {
    let scan = regeneration_scan();
    let mut ir = reconciliation_ir_for_ordering();
    ir.model.features.reverse();
    for (ordinal, feature) in ir.model.features.iter_mut().enumerate() {
        feature.ordinal = cadmpeg_core::decode::u64_from_index(ordinal);
    }
    let mut losses = Vec::new();
    crate::decode::with_service_context(|ctx| {
        super::reconcile_feature_links(ctx, &scan, &mut ir, &BTreeMap::new(), &mut losses)
    })
    .expect("a later recipe parent keeps the decode");
    let child = IrFeatureId::mint("creo:model:feature#10").expect("fixture child ID");
    let parent = IrFeatureId::mint("creo:model:feature#3").expect("fixture parent ID");
    assert_eq!(ir.model.features.len(), 2);
    assert_eq!(ir.model.feature_regeneration_parent(&child), Some(&parent));
    assert!(losses.is_empty());
}

#[test]
fn recipe_parent_that_depends_on_its_child_drops_that_edge() {
    let scan = regeneration_scan();
    let mut ir = reconciliation_ir_for_ordering();
    ir.model.features.reverse();
    let child = IrFeatureId::mint("creo:model:feature#10").expect("fixture child ID");
    let parent = IrFeatureId::mint("creo:model:feature#3").expect("fixture parent ID");
    for (ordinal, feature) in ir.model.features.iter_mut().enumerate() {
        feature.ordinal = cadmpeg_core::decode::u64_from_index(ordinal);
        if feature.id == parent {
            feature.dependencies =
                cadmpeg_ir::features::DistinctMembers::try_from(vec![child.clone()])
                    .expect("one dependency");
        }
    }
    let mut losses = Vec::new();
    crate::decode::with_service_context(|ctx| {
        super::reconcile_feature_links(ctx, &scan, &mut ir, &BTreeMap::new(), &mut losses)
    })
    .expect("a cyclic recipe parent keeps the decode");
    assert_eq!(ir.model.features.len(), 2);
    assert_eq!(ir.model.feature_regeneration_parent(&child), None);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::CreoLossCode::FeatureRegenerationParentDropped
            .note(String::new())
            .code
    );
    assert!(losses[0].message.starts_with(
        "feature creo:model:feature#10 keeps no regeneration parent creo:model:feature#3: "
    ));
}

#[test]
fn reconciled_native_dependency_preserves_emitted_parent() {
    let scan = crate::test_support::empty_container_scan();
    let mut ir = reconciliation_ir_with_emitted_parent();
    crate::decode::with_service_context(|ctx| {
        super::reconcile_feature_links(
            ctx,
            &scan,
            &mut ir,
            &BTreeMap::from([(10, vec![3])]),
            &mut Vec::new(),
        )
    })
    .expect("native dependency reconciliation");
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
    let dependencies = feature_generated_dependencies(&definition).expect("unique dependency");
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
        feature_generated_dependencies(&fillet)
            .expect("generated dependencies")
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
        feature_generated_dependencies(&chamfer)
            .expect("generated dependencies")
            .into_iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec![producer]
    );
}

#[test]
fn recipe_parent_naming_its_own_feature_is_charged() {
    let mut scan = regeneration_scan();
    if let Some(depdb) = scan.features.operations[0].depdb.as_mut() {
        depdb.parent = 10;
    }
    let mut ir = reconciliation_ir_for_ordering();
    let mut losses = Vec::new();
    crate::decode::with_service_context(|ctx| {
        super::reconcile_feature_links(ctx, &scan, &mut ir, &BTreeMap::new(), &mut losses)
    })
    .expect("a self-named recipe parent keeps the decode");
    let child = IrFeatureId::mint("creo:model:feature#10").expect("fixture child ID");
    assert_eq!(ir.model.feature_regeneration_parent(&child), None);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].message,
        "feature creo:model:feature#10 names itself as its recipe parent"
    );
}

#[test]
fn recipe_parent_in_a_cycle_stored_first_keeps_its_edge() {
    let scan = regeneration_scan();
    let mut ir = reconciliation_ir_for_ordering();
    let child = IrFeatureId::mint("creo:model:feature#10").expect("fixture child ID");
    let parent = IrFeatureId::mint("creo:model:feature#3").expect("fixture parent ID");
    for feature in &mut ir.model.features {
        if feature.id == parent {
            feature.dependencies =
                cadmpeg_ir::features::DistinctMembers::try_from(vec![child.clone()])
                    .expect("one dependency");
        }
    }
    let mut losses = Vec::new();
    crate::decode::with_service_context(|ctx| {
        super::reconcile_feature_links(ctx, &scan, &mut ir, &BTreeMap::new(), &mut losses)
    })
    .expect("a cyclic recipe parent keeps the decode");
    assert_eq!(ir.model.feature_regeneration_parent(&child), Some(&parent));
    assert!(losses.is_empty());
}
