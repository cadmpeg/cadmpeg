// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use super::{
    append_regeneration_edge, commit_regeneration_edges, compose_feature_id, emit_model_features,
    merge_feature_source_properties, ordered_row_feature_ids, refresh_feature_outputs,
};

#[test]
fn model_feature_identity_composes_model_namespace() {
    let id = compose_feature_id(40).expect("model feature identity");
    assert_eq!(id.as_str(), "creo:model:feature#40");
}

#[test]
fn regeneration_edge_appends_child_parent_pair() {
    use cadmpeg_ir::features::FeatureId;
    let child = FeatureId::mint("creo:model:feature#41").expect("identity grammar");
    let parent = FeatureId::mint("creo:model:feature#40").expect("identity grammar");
    let mut edges = Vec::new();
    append_regeneration_edge(&mut edges, &child, &parent);
    assert_eq!(edges, vec![(child, parent)]);
}

#[test]
fn regeneration_edge_commit_installs_feature_parent() {
    use cadmpeg_ir::features::FeatureId;
    let parent_id = FeatureId::mint("creo:model:feature#40").expect("identity grammar");
    let child_id = FeatureId::mint("creo:model:feature#41").expect("identity grammar");
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let mut parent = feature_for_output_refresh();
    parent.ordinal = 0;
    ir.model.features.push(parent);
    let mut child = feature_for_output_refresh();
    child.id = child_id.clone();
    child.ordinal = 1;
    ir.model.features.push(child);
    ({ commit_regeneration_edges(&mut ir, vec![(child_id.clone(), parent_id.clone())]) })
        .expect("regeneration parent");
    assert_eq!(ir.model.feature_parent(&child_id), Some(&parent_id));
}

fn feature_for_output_refresh() -> cadmpeg_ir::features::Feature {
    use cadmpeg_ir::features::{
        DistinctMembers, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation, FeatureId,
        FeatureOperation,
    };

    Feature {
        id: FeatureId::mint("creo:model:feature#40").expect("identity grammar"),
        ordinal: 3,
        name: None,
        suppressed: Some(false),
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::StoredGeometry {},
        )),
        native_ref: None,
    }
}

#[test]
fn feature_output_refresh_preserves_feature_order() {
    let scan = crate::test_support::empty_container_scan();
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.features.push(feature_for_output_refresh());
    crate::decode::with_test_decode_ctx(|ctx| refresh_feature_outputs(ctx, &scan, &mut ir))
        .expect("output update");
    assert_eq!(ir.model.features[0].ordinal, 3);
    assert!(ir.model.features[0].evaluation.outputs().is_empty());
}

fn property_key(value: &str) -> cadmpeg_core::text::NonBlankString {
    cadmpeg_core::text::NonBlankString::new(value).expect("fixture property key is nonblank")
}

#[test]
fn existing_feature_property_merge_keeps_order_and_replacement() {
    let mut target = BTreeMap::from([(property_key("recipe"), "Native".to_string())]);
    let incoming = BTreeMap::from([
        (property_key("featdefs_schema_state"), "absent".to_string()),
        (property_key("recipe"), "Extrude".to_string()),
    ]);
    merge_feature_source_properties(&mut target, incoming);
    assert_eq!(target["recipe"], "Extrude");
    assert_eq!(target["featdefs_schema_state"], "absent");
    assert_eq!(
        target
            .keys()
            .next()
            .map(cadmpeg_core::text::NonBlankString::as_str),
        Some("featdefs_schema_state")
    );
}

#[test]
fn existing_feature_dependency_merge_preserves_first_order_and_uniqueness() {
    let first = cadmpeg_ir::features::FeatureId::mint("creo:model:feature#12")
        .expect("first dependency identity");
    let second = cadmpeg_ir::features::FeatureId::mint("creo:model:feature#40")
        .expect("second dependency identity");
    let mut target = cadmpeg_ir::features::DistinctMembers::try_from(vec![first.clone()])
        .expect("one member is distinct");
    target.extend([first.clone(), second.clone()]);
    assert_eq!(target.as_slice(), &[first, second]);
}

#[test]
fn native_operation_feature_records_native_kind() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features
        .operations
        .push(crate::feature::operations::FeatureOperation {
            feature_id: 40,
            kind: crate::feature::operations::OperationKind::Native,
            name: crate::feature::operations::OperationName::Derived,
            recipe: crate::feature::operations::RecipeResolution::None,
            display_state_conflict: false,
            depdb: None,
            offset: 0,
            state_offset: 0,
        });
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    crate::decode::with_test_decode_ctx(|ctx| {
        emit_model_features(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    })
    .expect("model features");
    let [feature] = ir.model.features.as_slice() else {
        panic!("one native Feature expected");
    };
    assert!(matches!(feature.evaluation.definition(),
        cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Native { kind, parameters }
        ) if kind.as_str() == "Native Feature" && parameters.is_empty()));
}

#[test]
fn native_row_feature_records_native_kind() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.rows.push(one_feature_row());
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    crate::decode::with_test_decode_ctx(|ctx| {
        emit_model_features(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    })
    .expect("model features");
    let [feature] = ir.model.features.as_slice() else {
        panic!("one row-native Feature expected");
    };
    assert!(matches!(feature.evaluation.definition(),
        cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Native { kind, parameters }
        ) if kind.as_str() == "Native Feature" && parameters.is_empty()));
}

#[test]
fn native_row_feature_derives_name() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.rows.push(one_feature_row());
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    crate::decode::with_test_decode_ctx(|ctx| {
        emit_model_features(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    })
    .expect("model features");
    assert_eq!(
        ir.model.features[0].name.as_deref(),
        Some("Native Feature id 40")
    );
}

#[test]
fn stored_operation_feature_name_strips_prefix() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features
        .operations
        .push(crate::feature::operations::FeatureOperation {
            feature_id: 40,
            kind: crate::feature::operations::OperationKind::Native,
            name: crate::feature::operations::OperationName::Stored {
                bytes: b"~Native Feature id 40".to_vec(),
                keyword: crate::feature::operations::IdKeyword::Id,
                prefix: Some(b'~'),
            },
            recipe: crate::feature::operations::RecipeResolution::None,
            display_state_conflict: false,
            depdb: None,
            offset: 0,
            state_offset: 0,
        });
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    crate::decode::with_test_decode_ctx(|ctx| {
        emit_model_features(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    })
    .expect("model features");
    assert_eq!(
        ir.model.features[0].name.as_deref(),
        Some("Native Feature id 40")
    );
}

#[test]
fn recipe_source_tag_names_recipe() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features
        .operations
        .push(crate::feature::operations::FeatureOperation {
            feature_id: 40,
            kind: crate::feature::operations::OperationKind::Extrude,
            name: crate::feature::operations::OperationName::Derived,
            recipe: crate::feature::operations::RecipeResolution::Resolved(
                crate::feature::operations::FeatureRecipe::ProtrudeExtrude,
            ),
            display_state_conflict: false,
            depdb: None,
            offset: 0,
            state_offset: 0,
        });
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    crate::decode::with_test_decode_ctx(|ctx| {
        emit_model_features(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    })
    .expect("model features");
    assert_eq!(
        ir.model.features[0].source_tag.as_deref(),
        Some("protextrude")
    );
}

#[test]
fn row_feature_ids_preserve_first_source_order() {
    let rows = [
        crate::feature::rows::FeatureRow {
            feature_id: 40,
            root_schema_class: None,
            stream_offset: 0,
            body: vec![0; 2].try_into().expect("row body"),
            body_offset: 30,
            offset: 20,
        },
        crate::feature::rows::FeatureRow {
            feature_id: 12,
            root_schema_class: None,
            stream_offset: 0,
            body: vec![0; 2].try_into().expect("row body"),
            body_offset: 50,
            offset: 40,
        },
        crate::feature::rows::FeatureRow {
            feature_id: 40,
            root_schema_class: None,
            stream_offset: 0,
            body: vec![0; 2].try_into().expect("row body"),
            body_offset: 70,
            offset: 60,
        },
    ];

    assert_eq!(ordered_row_feature_ids(&rows), vec![40, 12]);
}

fn one_feature_row() -> crate::feature::rows::FeatureRow {
    crate::feature::rows::FeatureRow {
        feature_id: 40,
        root_schema_class: None,
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("row body"),
        body_offset: 30,
        offset: 20,
    }
}
