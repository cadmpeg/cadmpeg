// SPDX-License-Identifier: Apache-2.0
use crate::decode::records::{surface_contour_records, surface_prototype_records, surface_row_records};
    use crate::surface::arrays::{CountedScalars, DimensionedScalars};
    use crate::surface::{
        BoundaryType, SurfaceContourRecord, SurfaceKind, SurfaceNamedParameter, SurfaceNamedValue,
        SurfacePrototypeFamily, SurfacePrototypeRecord, SurfaceRow,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    fn scan() -> crate::container::ContainerScan<'static> {
        let mut scan = crate::test_support::empty_container_scan();
        scan.surfaces.rows.push(SurfaceRow {
            id: 7,
            kind: SurfaceKind::Plane,
            feature_id: 2,
            reversed: false,
            boundary_type: BoundaryType::Code01,
            next_surface: 0,
            offset: 3,
        });
        scan.surfaces.contours.push(SurfaceContourRecord {
            surface_id: 7,
            chain_index: 0,
            curve_header_id: 8,
            trv: 1,
            parameter_envelope: [None; 4],
            separator_reference: None,
            body: vec![8, 0xe3],
            offset: 5,
            envelope_offset: 6,
            surface_row_offset: 3,
        });
        let values = [
            SurfaceNamedValue::Empty,
            SurfaceNamedValue::CompactInt(4),
            SurfaceNamedValue::CompactIntArray(vec![4, 5]),
            SurfaceNamedValue::ContiguousEntityReferences(vec![7, 8]),
            SurfaceNamedValue::ScalarArray(
                DimensionedScalars::empty(1, 2).expect("test scalar grid"),
            ),
            SurfaceNamedValue::CountedScalarArray(
                CountedScalars::empty(2).expect("test scalar array"),
            ),
            SurfaceNamedValue::ScalarSequence(vec![1.0, 2.0]),
            SurfaceNamedValue::Opaque(vec![0xe3]),
        ];
        scan.surfaces
            .prototype_records
            .push(SurfacePrototypeRecord {
                family: SurfacePrototypeFamily::Plane,
                parameters: values
                    .into_iter()
                    .enumerate()
                    .map(|(offset, value)| SurfaceNamedParameter {
                        name: "parameter".into(),
                        value,
                        body: vec![0xe3],
                        offset,
                        value_offset: offset + 1,
                    })
                    .collect(),
                offset: 11,
            });
        scan
    }

    #[test]
    fn surface_row_record_refuses_collection_limit() {
        let scan = scan();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
      ResourceDimension::CollectionItems, Some("creo native surface row records"), |cap| {
          let trial_arena = DecodeArena::new();
          let mut trial_policy = DecodePolicy::service();
          trial_policy.limits.max_collection_items = cap;
          let (trial_ctx, _) = DecodeContext::from_root_bytes(&[], &trial_arena, &trial_policy).expect("root");
          surface_row_records(&trial_ctx, &scan, &scan.surfaces.rows, "visibgeom").map(|_| ())
      });

        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        let Err(error) = surface_row_records(&ctx, &scan, &scan.surfaces.rows, "visibgeom") else {
            panic!("one surface row exceeds the collection limit")
        };
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo native surface row records"),
            "{error:?}"
        );
    }

    #[test]
    fn surface_contour_record_refuses_collection_limit() {
        let scan = scan();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
      ResourceDimension::CollectionItems, Some("creo native surface contour records"), |cap| {
          let trial_arena = DecodeArena::new();
          let mut trial_policy = DecodePolicy::service();
          trial_policy.limits.max_collection_items = cap;
          let (trial_ctx, _) = DecodeContext::from_root_bytes(&[], &trial_arena, &trial_policy).expect("root");
          surface_contour_records(&trial_ctx, &scan, &scan.surfaces.contours, "visibgeom").map(|_| ())
      });

        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        let Err(error) = surface_contour_records(&ctx, &scan, &scan.surfaces.contours, "visibgeom")
        else {
            panic!("one contour exceeds the collection limit")
        };
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo native surface contour records"),
            "{error:?}"
        );
    }

    #[test]
    fn surface_prototype_parameters_refuse_collection_limit() {
        let scan = scan();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
      ResourceDimension::CollectionItems, Some("creo native surface prototype parameters"), |cap| {
          let trial_arena = DecodeArena::new();
          let mut trial_policy = DecodePolicy::service();
          trial_policy.limits.max_collection_items = cap;
          let (trial_ctx, _) = DecodeContext::from_root_bytes(&[], &trial_arena, &trial_policy).expect("root");
          surface_prototype_records(&trial_ctx, &scan, &scan.surfaces.prototype_records, "visibgeom").map(|_| ())
      });

        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        let Err(error) =
            surface_prototype_records(&ctx, &scan, &scan.surfaces.prototype_records, "visibgeom")
        else {
            panic!("one parameter exceeds the collection limit");
        };
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo native surface prototype parameters"),
            "{error:?}"
        );
    }

    #[test]
    fn surface_prototype_row_refuses_collection_limit() {
        let scan = scan();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
      ResourceDimension::CollectionItems, Some("creo native surface prototype records"), |cap| {
          let trial_arena = DecodeArena::new();
          let mut trial_policy = DecodePolicy::service();
          trial_policy.limits.max_collection_items = cap;
          let (trial_ctx, _) = DecodeContext::from_root_bytes(&[], &trial_arena, &trial_policy).expect("root");
          surface_prototype_records(&trial_ctx, &scan, &scan.surfaces.prototype_records, "visibgeom").map(|_| ())
      });

        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        let Err(error) =
            surface_prototype_records(&ctx, &scan, &scan.surfaces.prototype_records, "visibgeom")
        else {
            panic!("prototype row exceeds the collection limit");
        };
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo native surface prototype records"),
            "{error:?}"
        );
    }

    #[test]
    fn surface_prototype_other_family_refuses_retained_limit() {
        let mut scan = scan();
        scan.surfaces.prototype_records[0].family = SurfacePrototypeFamily::Other("unknown".into());
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = crate::test_support::allocation_limit_at(
      ResourceDimension::MaterializedBytes, Some("creo native surface prototype family"), |cap| {
          let trial_arena = DecodeArena::new();
          let mut trial_policy = DecodePolicy::service();
          trial_policy.limits.max_materialized_bytes = cap;
          let (trial_ctx, _) = DecodeContext::from_root_bytes(&[], &trial_arena, &trial_policy).expect("root");
          surface_prototype_records(&trial_ctx, &scan, &scan.surfaces.prototype_records, "visibgeom").map(|_| ())
      });

        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        let Err(error) =
            surface_prototype_records(&ctx, &scan, &scan.surfaces.prototype_records, "visibgeom")
        else {
            panic!("unknown-family copy exceeds the retained limit");
        };
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::MaterializedBytes
                && resource.operation == "creo native surface prototype family"),
            "{error:?}"
        );
    }

    #[test]
    fn surface_named_values_preserve_json_without_copying() {
        let scan = scan();
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        let (records, _records_storage) =
            surface_prototype_records(&ctx, &scan, &scan.surfaces.prototype_records, "visibgeom")
                .expect("prototype is admitted");
        let values = records[0]
            .parameters
            .iter()
            .map(|parameter| serde_json::to_value(parameter).expect("parameter serializes"))
            .collect::<Vec<_>>();
        assert_eq!(values[1]["compact_values"], serde_json::json!([4]));
        assert_eq!(values[2]["compact_values"], serde_json::json!([4, 5]));
        assert_eq!(values[3]["compact_values"], serde_json::json!([7, 8]));
        assert_eq!(values[4]["scalar_values"], serde_json::json!([null, null]));
        assert_eq!(values[4]["scalar_tokens"], serde_json::json!([]));
        assert_eq!(values[5]["scalar_tokens"], serde_json::json!([[], []]));
        assert_eq!(values[6]["scalar_values"], serde_json::json!([1.0, 2.0]));
        assert_eq!(values[7]["opaque"], serde_json::json!([0xe3]));
    }
