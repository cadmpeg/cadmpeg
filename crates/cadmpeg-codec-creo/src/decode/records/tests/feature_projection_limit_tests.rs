// SPDX-License-Identifier: Apache-2.0
use crate::decode::records::{
        depdb_recipe_row_records, feature_affected_id_records, feature_choice_records,
        feature_entity_records, feature_entity_reference_records, feature_geometry_table_records,
        feature_loop_history_entry_records, feature_loop_restore_direction_records,
        feature_replay_affected_id_records, feature_revolution_extent_records, feature_row_records,
        surface_merge_replay_affected_id_records,
    };
    use crate::feature::entity::{FeatureEntity, FeatureEntityReference};
    use crate::feature::rows::{
        dummy_loop_history_entry, AffectedIdKind, FeatureAffectedIds, FeatureChoice,
        FeatureGeometryTable, FeatureGeometryTableKind, FeatureLoopRestoreDirection,
        FeatureReplayAffectedIds, FeatureRevolutionExtent, FeatureRow, FeatureRowBody,
        FeatureSurfaceMergeAffectedIds, LoopRestoreDirectionLane, ReplayExtentSource,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    fn scan() -> crate::container::ContainerScan<'static> {
        let mut scan = crate::test_support::empty_container_scan();
        scan.features.entities.push(FeatureEntity {
            entity_id: 4,
            type_byte: 1,
            name: "datum".into(),
            offset: 3,
        });
        scan.features
            .entity_references
            .push(FeatureEntityReference {
                source_entity_id: Some(4),
                target_entity_id: 4,
                offset: 5,
            });
        scan.features.geometry_tables.push(FeatureGeometryTable {
            feature_id: 7,
            kind: FeatureGeometryTableKind::DatumIds(Some(vec![4, 5])),
            count: 2,
            entity_class: 200,
            offset: 13,
        });
        scan.features
            .loop_history_entries
            .push(dummy_loop_history_entry());
        scan.features.affected_ids.push(FeatureAffectedIds {
            feature_id: 7,
            kind: AffectedIdKind::Geometry,
            ids: vec![4, 5],
            offset: 29,
        });
        scan.features
            .replay_affected_ids
            .push(FeatureReplayAffectedIds {
                feature_id: 7,
                geometry_ids: vec![4],
                edge_ids: vec![5],
                geometry_extent: ReplayExtentSource::Explicit,
                edge_extent: ReplayExtentSource::Inherited,
                offset: 31,
            });
        scan.features
            .surface_merge_replay_affected_ids
            .push(FeatureSurfaceMergeAffectedIds {
                feature_id: 7,
                geometry_ids: vec![4],
                edge_ids: vec![5],
                quilt_ids: vec![6],
                geometry_extent: ReplayExtentSource::Explicit,
                edge_extent: ReplayExtentSource::Inherited,
                quilt_extent: ReplayExtentSource::Explicit,
                offset: 33,
            });
        scan.features
            .loop_restore_directions
            .push(FeatureLoopRestoreDirection {
                feature_id: 7,
                lane: LoopRestoreDirectionLane::Primary,
                value: 1,
                offset: 35,
            });
        scan.features
            .revolution_extents
            .push(FeatureRevolutionExtent {
                feature_id: 7,
                offset: 37,
            });
        scan.features.choices.push(FeatureChoice {
            feature_id: 7,
            label: "depth_choice".into(),
            type_byte: Some(1),
            payload: vec![0xe3],
            payload_offset: 41,
            offset: 39,
        });
        let row = FeatureRow {
            feature_id: 7,
            root_schema_class: None,
            stream_offset: 0,
            body: FeatureRowBody::try_from(vec![0, 1, 2]).expect("complete test header"),
            body_offset: 44,
            offset: 42,
        };
        scan.features.rows.push(row.clone());
        scan.features.depdb_recipe_rows.push(row);
        scan
    }

    macro_rules! collection_limit_test {
        ($name:ident, $projection:ident, $operation:literal) => {
            #[test]
            fn $name() {
                let scan = scan();
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = 0;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                    .expect("empty root is admitted");
                let Err(error) = $projection(&ctx, &scan) else { panic!("one native record exceeds the collection limit") };
                assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
                    if resource.dimension == ResourceDimension::CollectionItems
                        && resource.operation == $operation), "{error:?}");
            }
        };
    }

    collection_limit_test!(
        feature_entity_record_refuses_limit,
        feature_entity_records,
        "creo feature entity records"
    );
    collection_limit_test!(
        feature_entity_reference_record_refuses_limit,
        feature_entity_reference_records,
        "creo feature entity reference records"
    );
    collection_limit_test!(
        feature_geometry_table_record_refuses_limit,
        feature_geometry_table_records,
        "creo feature geometry table records"
    );
    collection_limit_test!(
        feature_loop_history_record_refuses_limit,
        feature_loop_history_entry_records,
        "creo feature loop history records"
    );
    collection_limit_test!(
        feature_affected_ids_record_refuses_limit,
        feature_affected_id_records,
        "creo feature affected ids records"
    );
    collection_limit_test!(
        feature_replay_affected_ids_record_refuses_limit,
        feature_replay_affected_id_records,
        "creo feature replay affected ids records"
    );
    collection_limit_test!(
        surface_merge_replay_affected_ids_record_refuses_limit,
        surface_merge_replay_affected_id_records,
        "creo surface merge replay affected ids records"
    );
    collection_limit_test!(
        feature_loop_restore_direction_record_refuses_limit,
        feature_loop_restore_direction_records,
        "creo feature loop restore direction records"
    );
    collection_limit_test!(
        feature_revolution_extent_record_refuses_limit,
        feature_revolution_extent_records,
        "creo feature revolution extent records"
    );
    collection_limit_test!(
        feature_choice_record_refuses_limit,
        feature_choice_records,
        "creo feature choice records"
    );
    collection_limit_test!(
        feature_row_record_refuses_limit,
        feature_row_records,
        "creo feature row records"
    );
    collection_limit_test!(
        depdb_recipe_row_record_refuses_limit,
        depdb_recipe_row_records,
        "creo depdb recipe row records"
    );

    #[test]
    fn borrowed_feature_projection_preserves_json() {
        let scan = scan();
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        let geometry = feature_geometry_table_records(&ctx, &scan).expect("record is admitted");
        let history = feature_loop_history_entry_records(&ctx, &scan).expect("record is admitted");
        let geometry = serde_json::to_value(&geometry[0]).expect("record serializes");
        let history = serde_json::to_value(&history[0]).expect("record serializes");
        assert_eq!(geometry["entry_ids"], serde_json::json!([4, 5]));
        assert_eq!(
            history["field_bytes"],
            serde_json::json!([[1], [2], [3], [4]])
        );
    }
