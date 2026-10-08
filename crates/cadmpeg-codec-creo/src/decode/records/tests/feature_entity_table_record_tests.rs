// SPDX-License-Identifier: Apache-2.0
use crate::decode::records::feature_entity_table_records;
    use crate::feature::entity::{dummy_table_entry, FeatureEntityTable};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use std::collections::BTreeSet;

    fn scan_with_table() -> crate::container::ContainerScan<'static> {
        let mut scan = crate::test_support::empty_container_scan();
        scan.features.entity_tables.push(FeatureEntityTable::new(
            4,
            29,
            vec![dummy_table_entry(7), dummy_table_entry(9)],
            &BTreeSet::from([7]),
            12,
        ));
        scan
    }

    fn collection_error(limit: u64, operation: &'static str) {
        let scan = scan_with_table();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty source is admitted");
        let Err(error) = feature_entity_table_records(&ctx, &scan) else {
            panic!("record copy exceeds the collection limit");
        };
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == operation),
            "{error:?}"
        );
    }

    #[test]
    fn entity_table_record_entry_ids_refuse_collection_limit() {
        collection_error(0, "creo feature entity table record entry ids");
    }

    #[test]
    fn entity_table_record_entries_refuse_collection_limit() {
        collection_error(1, "creo feature entity table record entries");
    }

    #[test]
    fn entity_table_record_surface_ids_refuse_collection_limit() {
        collection_error(2, "creo feature entity table record surface ids");
    }

    #[test]
    fn entity_table_record_non_surface_ids_refuse_collection_limit() {
        collection_error(5, "creo feature entity table record non surface ids");
    }

    #[test]
    fn entity_table_record_outer_rows_refuse_collection_limit() {
        collection_error(6, "creo feature entity table records");
    }

    #[test]
    fn entity_table_record_id_refuses_retained_limit() {
        let scan = scan_with_table();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
            ResourceDimension::RetainedBytes,
            Some("creo feature entity table record id"),
            |cap| {
                let trial_arena = DecodeArena::new();
                let mut trial_policy = policy;
                trial_policy.limits.max_retained_bytes = cap;
                let (trial_ctx, _) =
                    DecodeContext::from_root_bytes(&[], &trial_arena, &trial_policy).expect("root");
                feature_entity_table_records(&trial_ctx, &scan)
            },
        );
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty source is admitted");
        let Err(error) = feature_entity_table_records(&ctx, &scan) else {
            panic!("record identity exceeds the retained limit");
        };
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo feature entity table record id"),
            "{error:?}"
        );
    }

    #[test]
    fn entity_table_record_preserves_source_order_and_partition() {
        let scan = scan_with_table();
        crate::decode::with_test_decode_ctx(|ctx| {
            let records = feature_entity_table_records(ctx, &scan)?;
            assert_eq!(records.len(), 1);
            let record = &records[0];
            assert_eq!(record.id, "creo:allfeatur:entity_table#12");
            assert_eq!(record.entry_ids, [7, 9]);
            assert_eq!(record.surface_ids, [7]);
            assert_eq!(record.non_surface_entity_ids, [9]);
            Ok::<(), cadmpeg_core::CodecError>(())
        })
        .expect("service profile admits the record");
    }
