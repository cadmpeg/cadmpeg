// SPDX-License-Identifier: Apache-2.0
use crate::decode::records::feature_choice_field_records;
use crate::feature::rows::{FeatureChoiceField, FeatureFieldValue};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

fn scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::test_support::empty_container_scan();
    let values = [
        FeatureFieldValue::Empty,
        FeatureFieldValue::CompactInt(7),
        FeatureFieldValue::CompactIntArray(vec![7, 8]),
        FeatureFieldValue::EntityReference {
            entity_id: 9,
            terminated: true,
        },
        FeatureFieldValue::ScalarArray {
            dimensions: 1,
            count: 2,
            body: vec![0xf9, 2],
            decoded_values: Some(vec![1.0, 2.0]),
        },
        FeatureFieldValue::Raw(vec![0xe3]),
    ];
    for (offset, value) in values.into_iter().enumerate() {
        scan.features.choice_fields.push(FeatureChoiceField {
            feature_id: 4,
            choice_label: "depth_choice".into(),
            name: "value".into(),
            type_byte: 1,
            value,
            offset,
        });
    }
    scan
}

#[test]
fn feature_choice_field_record_refuses_collection_limit() {
    let scan = scan();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
        ResourceDimension::CollectionItems,
        Some("creo native feature choice field records"),
        |cap| {
            let trial_arena = DecodeArena::new();
            let mut trial_policy = DecodePolicy::service();
            trial_policy.limits.max_collection_items = cap;
            let (trial_ctx, _) =
                DecodeContext::from_root_bytes(&[], &trial_arena, &trial_policy).expect("root");
            feature_choice_field_records(&trial_ctx, &scan).map(|_| ())
        },
    );

    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let Err(error) = feature_choice_field_records(&ctx, &scan) else {
        panic!("one choice-field record exceeds the collection limit")
    };
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo native feature choice field records"),
        "{error:?}"
    );
}

#[test]
fn feature_choice_field_record_id_refuses_retained_limit() {
    let scan = scan();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = crate::test_support::allocation_limit_at(
        ResourceDimension::MaterializedBytes,
        Some("creo native feature choice field record id"),
        |cap| {
            let trial_arena = DecodeArena::new();
            let mut trial_policy = DecodePolicy::service();
            trial_policy.limits.max_materialized_bytes = cap;
            let (trial_ctx, _) =
                DecodeContext::from_root_bytes(&[], &trial_arena, &trial_policy).expect("root");
            feature_choice_field_records(&trial_ctx, &scan).map(|_| ())
        },
    );

    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let Err(error) = feature_choice_field_records(&ctx, &scan) else {
        panic!("one choice-field ID exceeds the retained limit")
    };
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::MaterializedBytes
                && resource.operation == "creo native feature choice field record id"),
        "{error:?}"
    );
}

#[test]
fn borrowed_feature_choice_field_values_preserve_json() {
    let scan = scan();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let records_parts =
        feature_choice_field_records(&ctx, &scan).expect("records are admitted");
    let _records_storage = records_parts.1;
    let records = records_parts.0;
    let values = records
        .iter()
        .map(|record| serde_json::to_value(&record.value).expect("value serializes"))
        .collect::<Vec<_>>();
    assert_eq!(
        serde_json::Value::Array(values),
        serde_json::json!([
            {"kind":"empty"},
            {"kind":"compact_int","value":7},
            {"kind":"compact_int_array","values":[7,8]},
            {"kind":"entity_reference","entity_id":9,"terminated":true},
            {"kind":"scalar_array","dimensions":1,"count":2,"body":[249,2],"decoded_values":[1.0,2.0]},
            {"kind":"raw","bytes":[227]}
        ])
    );
}
