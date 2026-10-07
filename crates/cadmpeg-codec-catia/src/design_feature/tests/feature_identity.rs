// SPDX-License-Identifier: Apache-2.0
//! feature identity tests.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use cadmpeg_ir::codec::Codec;

use super::{
    catalog_stream, entity_backed_object_graph, normalize_parameter_names,
    object_graph_from_records, object_graph_record, parameter,
    standard_catpart_with_definition_value, standard_catpart_with_design_class,
    standard_catpart_with_visualization_values_only, transfer_design_features, value_block_stream,
    CadIr, CatiaCodec, Cursor, DecodeOptions, FeatureId,
};

#[test]
fn incompatible_exact_feature_candidates_on_one_object_remain_unresolved() {
    let records = [
        object_graph_record(&[0x12, 0x84, 0x84], &[0xfe]),
        object_graph_record(&[0x12, 0x84, 0x84], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x85, 0x85], &[0xfe]),
    ];
    let mut bytes = entity_backed_object_graph(&records, &[2, 3, 4]);
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "xy-plane",
        "Sketch",
    ]));
    let native = crate::native::CatiaNative::decode(&bytes);

    let candidate = native
        .design_objects
        .iter()
        .find(|object| object.owner_entity_id == 4)
        .expect("synthetic dual-candidate object");
    assert_eq!(candidate.field_classes[0].name, "xy-plane");
    assert_eq!(candidate.owner_entity_id, 4);
    assert_eq!(
        candidate
            .owner_class
            .as_ref()
            .map(|class| class.name.as_str()),
        Some("Sketch")
    );
    assert_eq!(
        candidate.owner_design_object,
        Some(native.design_objects[1].id.clone())
    );

    let mut ir = CadIr::empty();
    let transfer = crate::test_support::with_service_context(|ctx| {
        crate::design_feature::transfer_design_features(
            ctx,
            &mut ir,
            &crate::design_feature::DesignFeatureSources::new(ctx, &native)?,
            &crate::decode::ModelingGraphScope::Unscoped,
        )
    })
    .unwrap();

    assert!(ir.model.features.is_empty());
    assert!(ir.model.sketches.is_empty());
    assert!(transfer.consumed_records().next().is_none());
    assert!(transfer.feature_ids.is_empty());
}

#[test]
fn parameter_owner_follows_one_exact_child_design_object() {
    let mut native = crate::native::CatiaNative::decode(&standard_catpart_with_definition_value(
        &[0x00, 0x08, 0x32, 4, 0, 0, 0],
        &[0xfe],
        &[0xd1, 0x67, 0x88, 0x81, 0xbd, 0xe8, 0x81, 0x49],
    ));
    let owner_record = native
        .object_graphs
        .iter()
        .flat_map(|graph| graph.records.iter())
        .find(|record| record.design_object.is_some())
        .expect("synthetic owner declaration record")
        .clone();
    let owner_record_id = owner_record.id.clone();
    let owner_design_object = owner_record.design_object.clone();
    let owner_class_entry = "synthetic-sketch-class".to_string();
    let owner_record_mut = native
        .object_graphs
        .iter_mut()
        .flat_map(|graph| graph.records.iter_mut())
        .find(|record| record.id == owner_record_id)
        .expect("mutable synthetic owner declaration record");
    if let Some(class) = &mut owner_record_mut.class {
        class.name = Some("Sketch".to_string());
        class.entry = Some(owner_class_entry.clone());
    } else {
        owner_record_mut.class = Some(crate::native::CatiaObjectClass {
            ordinal: 0,
            name: Some("Sketch".to_string()),
            entry: Some(owner_class_entry.clone()),
        });
    }

    let feature_object = native
        .design_objects
        .first_mut()
        .expect("synthetic design object");
    feature_object.owner_record = Some(owner_record_id);
    feature_object.owner_design_object = owner_design_object.clone();
    feature_object.owner_class = Some(crate::native::CatiaDesignClass {
        entry: owner_class_entry,
        name: "Sketch".to_string(),
    });
    let feature_id = feature_object.id.clone();

    let child_record_id = "synthetic-child-record".to_string();
    let child_entity_id = "synthetic-child-entity".to_string();
    let mut child_record = owner_record.clone();
    child_record.id.clone_from(&child_record_id);
    child_record.entity = Some(crate::native::CatiaObjectEntity {
        record: child_entity_id.clone(),
        id: 2,
    });
    child_record.owner = Some(crate::native::CatiaObjectOwner::Entity(2));
    child_record.design_object = Some("synthetic-child-object".to_string());
    native.object_graphs[0].records.push(child_record);

    let mut child_entity = native.entity_records[0].clone();
    child_entity.id.clone_from(&child_entity_id);
    child_entity.object_record = child_record_id.clone();
    child_entity.entity_id = 2;
    child_entity.ordinal = cadmpeg_core::decode::u64_from_index(native.entity_records.len());
    native.entity_records.push(child_entity);

    let mut child_object = native.design_objects[0].clone();
    child_object.id = "synthetic-child-object".to_string();
    child_object.ordinal += 1;
    child_object.first_field_byte_offset += 1;
    child_object.owner_entity_id = 2;
    child_object.owner_record = Some(child_record_id);
    child_object.owner_design_object = Some(feature_id.clone());
    child_object.owner_class = None;
    child_object.owner_storage_ref = None;
    child_object.fields = vec!["synthetic-child-record".to_string()];
    child_object.field_classes.clear();
    child_object.definition_values.clear();
    child_object.definition_chain_values.clear();
    child_object.relations.clear();
    child_object.parallel_reference_table = None;
    native.design_objects.push(child_object);

    let mut ir = CadIr::empty();
    let transfer = crate::test_support::with_service_context(|ctx| {
        crate::design_feature::transfer_design_features(
            ctx,
            &mut ir,
            &crate::design_feature::DesignFeatureSources::new(ctx, &native)?,
            &crate::decode::ModelingGraphScope::Unscoped,
        )
    })
    .unwrap();
    ir.model
        .parameters
        .push(cadmpeg_ir::features::DesignParameter {
            id: cadmpeg_ir::features::ParameterId::mint(
                "synthetic:test:id#synthetic:child-parameter".to_string(),
            )
            .expect("identity grammar"),
            owner: None,
            ordinal: 0,
            name: "Value".to_string(),
            expression: String::new(),
            display: None,
            value: None,
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            properties: std::collections::BTreeMap::new(),
            pmi: None,
            native_ref: Some(child_entity_id),
        });

    crate::test_support::with_service_context(|ctx| {
        transfer.assign_parameter_owners(
            ctx,
            &mut ir,
            &crate::design_feature::DesignFeatureSources::new(ctx, &native)?,
        )
    })
    .unwrap();

    assert_eq!(ir.model.features.len(), 1);
    assert_eq!(
        ir.model.parameters[0].owner,
        Some(cadmpeg_ir::features::FeatureId::from(
            crate::test_support::with_service_context(|ctx| crate::ids::neutral_history_id(
                ctx,
                &feature_id,
                &cadmpeg_ir::identity_component!("feature")
            ))
            .expect("identity grammar")
        ))
    );
}

#[test]
fn complete_standalone_principal_plane_declarations_transfer_one_history_node() {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, PrincipalPlane};

    for (class, plane) in [
        ("xy-plane", PrincipalPlane::Top),
        ("yz-plane", PrincipalPlane::Right),
        ("zx-plane", PrincipalPlane::Front),
    ] {
        let records = [
            object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
            object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
        ];
        let mut bytes = entity_backed_object_graph(&records, &[2, 3]);
        bytes.extend(catalog_stream(&[
            "CATCatalogManager",
            "catalogManager",
            "catalogLinks",
            "",
            class,
        ]));
        let native = crate::native::CatiaNative::decode(&bytes);
        let mut ir = CadIr::empty();

        let transfer = crate::test_support::with_service_context(|ctx| {
            crate::design_feature::transfer_design_features(
                ctx,
                &mut ir,
                &crate::design_feature::DesignFeatureSources::new(ctx, &native)?,
                &crate::decode::ModelingGraphScope::Unscoped,
            )
        })
        .unwrap();

        assert!(ir.model.sketches.is_empty());
        assert_eq!(ir.model.features.len(), 1);
        assert_eq!(
            ir.model.features[0].evaluation.definition(),
            &FeatureDefinition::Operation(FeatureOperation::DatumPrincipalPlane { plane })
        );
        assert_eq!(ir.model.features[0].source_tag.as_deref(), Some(class));
        assert_eq!(
            ir.model.features[0].ordinal,
            native.design_objects[0].first_field_byte_offset
        );
        assert_eq!(
            transfer.principal_plane_records,
            native.design_objects[0].fields.iter().cloned().collect()
        );

        let mut excluded_ir = CadIr::empty();
        let excluded = crate::test_support::with_service_context(|ctx| {
            crate::design_feature::transfer_design_features(
                ctx,
                &mut excluded_ir,
                &crate::design_feature::DesignFeatureSources::new(ctx, &native)?,
                &crate::decode::ModelingGraphScope::Unresolved,
            )
        })
        .unwrap();
        assert!(excluded_ir.model.features.is_empty());
        assert!(excluded.consumed_records().next().is_none());
    }
}

#[test]
fn principal_plane_history_identity_admission_precedes_transfer() {
    let records = [
        object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
        object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
    ];
    let mut bytes = entity_backed_object_graph(&records, &[2, 3]);
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "xy-plane",
    ]));
    for id in ["catia:graph:object#owner:child", "short"] {
        let mut native = crate::native::CatiaNative::decode(&bytes);
        let old_id = std::mem::replace(&mut native.design_objects[0].id, id.to_owned());
        for record in native
            .object_graphs
            .iter_mut()
            .flat_map(|graph| &mut graph.records)
        {
            if record.design_object.as_deref() == Some(old_id.as_str()) {
                record.design_object = Some(id.to_owned());
            }
        }
        let mut ir = CadIr::empty();
        let result = crate::test_support::with_service_context(|ctx| {
            transfer_design_features(
                ctx,
                &mut ir,
                &crate::design_feature::DesignFeatureSources::new(ctx, &native)?,
                &crate::decode::ModelingGraphScope::Unscoped,
            )
        });
        if id == "short" {
            assert!(matches!(
                result,
                Err(cadmpeg_core::CodecError::Malformed(_))
            ));
            assert!(ir.model.features.is_empty());
        } else {
            let transfer = result.unwrap();
            assert_eq!(ir.model.features.len(), 1);
            assert_eq!(
                ir.model.features[0].id.as_str(),
                "catia:graph:feature#owner:child"
            );
            assert_eq!(transfer.principal_plane_records.len(), 2);
        }
    }
}

#[test]
fn mixed_or_payload_bearing_principal_plane_fields_do_not_transfer() {
    for (records, catalog) in [
        (
            vec![
                object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
                object_graph_record(&[0x12, 0x82, 0x85], &[0xfe]),
            ],
            vec![
                "CATCatalogManager",
                "catalogManager",
                "catalogLinks",
                "",
                "xy-plane",
                "yz-plane",
            ],
        ),
        (
            vec![
                object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
                object_graph_record(&[0x12, 0x82, 0x84], &[0x80, 0xfe]),
            ],
            vec![
                "CATCatalogManager",
                "catalogManager",
                "catalogLinks",
                "",
                "xy-plane",
            ],
        ),
    ] {
        let mut bytes = entity_backed_object_graph(&records, &[2, 3]);
        bytes.extend(catalog_stream(&catalog));
        let native = crate::native::CatiaNative::decode(&bytes);
        let mut ir = CadIr::empty();

        let transfer = crate::test_support::with_service_context(|ctx| {
            crate::design_feature::transfer_design_features(
                ctx,
                &mut ir,
                &crate::design_feature::DesignFeatureSources::new(ctx, &native)?,
                &crate::decode::ModelingGraphScope::Unscoped,
            )
        })
        .unwrap();

        assert!(ir.model.features.is_empty());
        assert!(transfer.principal_plane_records.is_empty());
    }
}

#[test]
fn design_field_vocabulary_distinguishes_equal_names_from_distinct_entries() {
    let mut bytes = object_graph_from_records(&[
        object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
        object_graph_record(&[0x12, 0x82, 0x85], &[0xfe]),
    ]);
    bytes.extend(value_block_stream(&[0x81]));
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Feature",
        "Feature",
    ]));

    let native = crate::native::CatiaNative::decode(&bytes);
    let classes = &native.design_objects[0].field_classes;

    assert_eq!(classes.len(), 2);
    assert_eq!(classes[0].name, classes[1].name);
    assert_ne!(classes[0].entry, classes[1].entry);
}

#[test]
fn visualization_values_do_not_assert_missing_design_intent() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_visualization_values_only()),
            &DecodeOptions::default(),
        )
        .expect("decode visualization-only values");

    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::report::loss::LossCategory::Attribute
            && loss.message.contains("schema-selected presentation value")
    }));
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| loss.code.category() != cadmpeg_ir::report::loss::LossCategory::DesignIntent));
}

#[test]
fn decode_does_not_promote_field_class_names_to_features() {
    for class in [
        "Groove",
        "GSMHelix",
        "CircPattern_RadialNumber",
        "GSMPlaneAngle",
        "GSMPlaneOffset",
    ] {
        let decoded = CatiaCodec
            .decode(
                &mut Cursor::new(standard_catpart_with_design_class(class)),
                &DecodeOptions::default(),
            )
            .expect("decode field-class vocabulary");

        assert!(decoded.ir().model.features.is_empty());
        let native = crate::native::CatiaNative::load(
            decoded
                .ir()
                .native
                .namespace("catia")
                .expect("CATIA native namespace"),
        )
        .expect("load retained field-class vocabulary");
        assert_eq!(
            native.design_objects[0]
                .field_classes
                .iter()
                .map(|class| class.name.as_str())
                .collect::<Vec<_>>(),
            ["CurrentFeature", class]
        );
        assert!(decoded.report().losses.iter().any(|loss| {
            loss.code.category() == cadmpeg_ir::report::loss::LossCategory::DesignIntent
                && loss.message.contains("neutral features")
        }));
    }
}

#[test]
fn normalizes_scopes_containing_only_unnamed_parameters() {
    let mut ir = CadIr::empty();
    for owner in [
        None,
        Some(FeatureId::mint("synthetic:test:id#feature").expect("identity grammar")),
    ] {
        for id in ["first", "second"] {
            let mut value = parameter(id, "native");
            value.owner = owner.clone();
            value.name.clear();
            ir.model.parameters.push(value);
        }
    }
    crate::test_support::with_service_context(|ctx| normalize_parameter_names(ctx, &mut ir))
        .unwrap();
    assert_eq!(
        ir.model
            .parameters
            .iter()
            .map(|value| value.name.as_str())
            .collect::<Vec<_>>(),
        ["Parameter#1", "Parameter#2", "Parameter#1", "Parameter#2"]
    );
    assert!(ir.model.parameters.iter().all(|value| value
        .properties
        .get("source_name")
        .map(String::as_str)
        == Some("")));
}
