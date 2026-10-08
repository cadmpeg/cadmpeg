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
    fn sketch_record_id_refuses_retained_limit() {
        let scan = scan();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        let Err(error) = sketch_records(&ctx, &scan) else {
            panic!("native sketch ID exceeds retained limit")
        };
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo sketch record id"),
            "{error:?}"
        );
    }

    #[test]
    fn sketch_source_section_refuses_retained_limit() {
        let scan = scan();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes =
            cadmpeg_core::decode::u64_from_index("creo:featdefs:sketch#7".len());
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        let Err(error) = sketch_records(&ctx, &scan) else {
            panic!("source section exceeds retained limit")
        };
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo sketch source section"),
            "{error:?}"
        );
    }

    #[test]
    fn sketch_record_refuses_collection_limit() {
        let scan = scan();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
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
        policy.limits.max_collection_items = 1;
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
        let records = sketch_records(&ctx, &scan).expect("sketch projection is admitted");
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
    fn sketch_variable_body_refuses_retained_limit() {
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
        let records = sketch_records(&ctx, &scan).expect("service profile admits one variable");
        let value = serde_json::to_value(&records[0]).expect("record serializes");
        assert_eq!(
            value["variables"][0]["value_body"],
            serde_json::json!([249])
        );

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
            ResourceDimension::RetainedBytes,
            Some("creo native sketch variable value body"),
            |cap| {
                let trial_arena = DecodeArena::new();
                let mut trial_policy = policy;
                trial_policy.limits.max_retained_bytes = cap;
                let (trial_ctx, _) =
                    DecodeContext::from_root_bytes(&[], &trial_arena, &trial_policy).expect("root");
                sketch_records(&trial_ctx, &scan)
            },
        );
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        let Err(error) = sketch_records(&ctx, &scan) else {
            panic!("variable body exceeds retained limit")
        };
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo native sketch variable value body"),
            "{error:?}"
        );
    }
