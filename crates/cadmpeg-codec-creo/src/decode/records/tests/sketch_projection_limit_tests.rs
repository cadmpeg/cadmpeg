// SPDX-License-Identifier: Apache-2.0
use crate::decode::records::sketch_records;
    use crate::feature::definitions::{
        DefinitionIdentity, FeatureDefinition, FeatureSavedDummy, FeatureSavedEntity,
        FeatureSavedSection, FeatureSection3d, FeatureSectionOrientation,
        FeatureSectionReferencePlane, FeatureVariableRow, FeatureVariableTable, ReferencePlanes,
        ScalarLane, VariableType,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    fn definition() -> FeatureDefinition {
        FeatureDefinition {
            identity: DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(7),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: Some(FeatureSection3d {
                sketch_plane_entity_id: Some(2),
                sketch_plane_flip: None,
                reference_planes: ReferencePlanes::Named(vec![3]),
                reference_plane_datum_geometry_id: None,
                orientation: FeatureSectionOrientation::default(),
                dimension_ids: vec![4],
                offset: 2,
            }),
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 1,
        }
    }

    fn scan() -> crate::container::ContainerScan<'static> {
        let mut scan = crate::test_support::empty_container_scan();
        scan.features.definitions.push(definition());
        scan
    }

    #[test]
    fn sketch_record_id_refuses_materialized_limit() {
        let scan = scan();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = crate::test_support::allocation_limit_at(
      ResourceDimension::MaterializedBytes, Some("creo sketch record id"), |cap| {
          let trial_arena = DecodeArena::new();
          let mut trial_policy = DecodePolicy::service();
          trial_policy.limits.max_materialized_bytes = cap;
          let (trial_ctx, _) = DecodeContext::from_root_bytes(&[], &trial_arena, &trial_policy).expect("root");
          sketch_records(&trial_ctx, &scan).map(|_| ())
      });

        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        let Err(error) = sketch_records(&ctx, &scan) else {
            panic!("native sketch ID exceeds materialized limit")
        };
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::MaterializedBytes
                && resource.operation == "creo sketch record id"),
            "{error:?}"
        );
    }

    #[test]
    fn sketch_source_section_refuses_materialized_limit() {
        let scan = scan();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = crate::test_support::allocation_limit_at(
      ResourceDimension::MaterializedBytes, Some("creo sketch source section"), |cap| {
          let trial_arena = DecodeArena::new();
          let mut trial_policy = DecodePolicy::service();
          trial_policy.limits.max_materialized_bytes = cap;
          let (trial_ctx, _) = DecodeContext::from_root_bytes(&[], &trial_arena, &trial_policy).expect("root");
          sketch_records(&trial_ctx, &scan).map(|_| ())
      });

        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        let Err(error) = sketch_records(&ctx, &scan) else {
            panic!("source section exceeds materialized limit")
        };
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::MaterializedBytes
                && resource.operation == "creo sketch source section"),
            "{error:?}"
        );
    }

    #[test]
    fn sketch_record_refuses_collection_limit() {
        let scan = scan();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
      ResourceDimension::CollectionItems, Some("creo sketch records"), |cap| {
          let trial_arena = DecodeArena::new();
          let mut trial_policy = DecodePolicy::service();
          trial_policy.limits.max_collection_items = cap;
          let (trial_ctx, _) = DecodeContext::from_root_bytes(&[], &trial_arena, &trial_policy).expect("root");
          sketch_records(&trial_ctx, &scan).map(|_| ())
      });

        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        let Err(error) = sketch_records(&ctx, &scan) else {
            panic!("one sketch projection exceeds collection limit")
        };
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo sketch records"),
            "{error:?}"
        );
    }

    #[test]
    fn sketch_saved_entity_refuses_collection_limit() {
        let mut scan = scan();
        let definition = &mut scan.features.definitions[0];
        definition.section_3d = None;
        definition.saved_section = Some(FeatureSavedSection {
            entities: vec![FeatureSavedEntity::Dummy(FeatureSavedDummy {
                entity_id: Some(8),
                body: vec![0xe3],
                offset: 3,
            })],
            offset: 2,
        });
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
      ResourceDimension::CollectionItems, Some("creo native sketch saved entities"), |cap| {
          let trial_arena = DecodeArena::new();
          let mut trial_policy = DecodePolicy::service();
          trial_policy.limits.max_collection_items = cap;
          let (trial_ctx, _) = DecodeContext::from_root_bytes(&[], &trial_arena, &trial_policy).expect("root");
          sketch_records(&trial_ctx, &scan).map(|_| ())
      });

        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        let Err(error) = sketch_records(&ctx, &scan) else {
            panic!("saved entity follows the table header")
        };
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo native sketch saved entities"),
            "{error:?}"
        );
    }

    #[test]
    fn borrowed_sketch_section_frames_preserve_json() {
        let mut scan = scan();
        scan.features.definitions[0]
            .section_3d
            .as_mut()
            .expect("section frame")
            .reference_planes = ReferencePlanes::Positional(vec![FeatureSectionReferencePlane {
            plane_entity_id: 3,
            reference_type: Some(2),
            external_reference_id: None,
            segment_id: None,
            sub_index: None,
            reference_flip: None,
        }]);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        let (records, _storage) = sketch_records(&ctx, &scan).expect("sketch projection is admitted");
        let value = serde_json::to_value(&records[0]).expect("record serializes");
        assert_eq!(
            value["section_3d"]["reference_plane_entity_ids"],
            serde_json::json!([3])
        );
        assert_eq!(
            value["section_3d"]["reference_plane_rows"][0]["reference_type"],
            2
        );
        assert_eq!(value["section_3d"]["dimension_ids"], serde_json::json!([4]));
        assert!(value["section_3d"]["reference_planes"].is_null());
    }

    #[test]
    fn sketch_variable_body_refuses_materialized_limit() {
        let mut scan = scan();
        scan.features.definitions[0].section_3d = None;
        scan.features.definitions[0].variables = Some(FeatureVariableTable {
            declared_count: 1,
            entity_ref: None,
            rows: vec![FeatureVariableRow {
                variable_type: VariableType::Parameter,
                key: 1,
                value: ScalarLane::Value(2.0),
                value_body: vec![0xf9],
                guess: ScalarLane::Undefined,
                guess_body: vec![0xe3],
                known: None,
                homogeneity: None,
                uvar_id: None,
                offset: 3,
            }],
            offset: 2,
        });
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        let (records, _storage) = sketch_records(&ctx, &scan).expect("service profile admits one variable");
        let value = serde_json::to_value(&records[0]).expect("record serializes");
        assert_eq!(
            value["variables"][0]["value_body"],
            serde_json::json!([249])
        );

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = crate::test_support::allocation_limit_at(
            ResourceDimension::MaterializedBytes,
            Some("creo native sketch variable value body"),
            |cap| {
                let trial_arena = DecodeArena::new();
                let mut trial_policy = policy;
                trial_policy.limits.max_materialized_bytes = cap;
                let (trial_ctx, _) =
                    DecodeContext::from_root_bytes(&[], &trial_arena, &trial_policy).expect("root");
                sketch_records(&trial_ctx, &scan).map(|_| ())
            },
        );
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        let Err(error) = sketch_records(&ctx, &scan) else {
            panic!("variable body exceeds materialized limit")
        };
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::MaterializedBytes
                && resource.operation == "creo native sketch variable value body"),
            "{error:?}"
        );
    }

#[test]
fn sketch_projection_storage_releases_after_serialization() {
    let scan = scan();
    let cap = crate::test_support::allocation_limit_at(
        ResourceDimension::MaterializedBytes,
        None,
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_materialized_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            sketch_records(&ctx, &scan).map(|_| ())
        },
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = cap;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let (records, storage) = sketch_records(&ctx, &scan).expect("scratch projection");
    let value = serde_json::to_value(&records[0]).expect("record serializes");
    assert_eq!(value["section_3d"]["dimension_ids"], serde_json::json!([4]));
    assert!(ctx.reserve_scoped(cap, "live sketch projection").is_err());
    drop((records, storage));
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let (records, storage) = sketch_records(&ctx, &scan).expect("scratch projection");
    drop((records, storage));
    ctx.reserve_scoped(cap, "released sketch projection").expect("all storage released");
}
