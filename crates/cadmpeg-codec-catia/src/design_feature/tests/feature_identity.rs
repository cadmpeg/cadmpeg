// SPDX-License-Identifier: Apache-2.0
//! feature identity tests.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use cadmpeg_ir::codec::Codec;

use super::{
    catalog_stream, entity_backed_object_graph, object_graph_from_records, object_graph_record,
    standard_catpart_with_design_class, standard_catpart_with_visualization_values_only,
    transfer_design_features, value_block_stream, CadIr, CatiaCodec, Cursor, DecodeOptions,
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
            &native,
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
                &native,
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
                &native,
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
                &native,
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
                &native,
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
