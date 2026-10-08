// SPDX-License-Identifier: Apache-2.0
use crate::decode::records::feature_definition_records;
use crate::feature::definitions::{
    DecodedField, DefinitionIdentity, FeatureDefinition, FeatureOutline, FeatureParameterFrame,
    FeatureParameterFrameKind, OutlinePhase,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

fn scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::test_support::empty_container_scan();
    let mut local_scalars = std::array::from_fn(|_| DecodedField {
        value: None,
        body: Vec::new(),
    });
    local_scalars[0] = DecodedField {
        value: Some(2.0),
        body: vec![0xf9],
    };
    scan.features.definitions.push(FeatureDefinition {
        identity: DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(7),
            owner_feature_id: Some(3),
        },
        body: vec![0xe3],
        parameter_frames: vec![FeatureParameterFrame {
            kind: FeatureParameterFrameKind::Transform,
            body: vec![0xf9],
            decoded_values: None,
            offset: 4,
        }],
        outlines: vec![FeatureOutline {
            phase: OutlinePhase::PostRegen,
            local_scalars,
            offset: 5,
        }],
        variables: None,
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 3,
    });
    scan
}

#[test]
fn feature_definition_id_refuses_retained_limit() {
    let scan = scan();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = crate::test_support::allocation_limit_at(
        ResourceDimension::MaterializedBytes,
        Some("creo feature definition record id"),
        |cap| {
            let trial_arena = DecodeArena::new();
            let mut trial_policy = DecodePolicy::service();
            trial_policy.limits.max_materialized_bytes = cap;
            let (trial_ctx, _) =
                DecodeContext::from_root_bytes(&[], &trial_arena, &trial_policy).expect("root");
            feature_definition_records(&trial_ctx, &scan).map(|_| ())
        },
    );

    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let Err(error) = feature_definition_records(&ctx, &scan) else {
        panic!("native ID exceeds retained limit")
    };
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::MaterializedBytes
                && resource.operation == "creo feature definition record id"),
        "{error:?}"
    );
}

#[test]
fn feature_definition_nested_rows_refuse_collection_limit() {
    let scan = scan();
    for operation in [
        "creo native feature parameter frames",
        "creo native feature outlines",
        "creo native feature definition records",
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some(operation),
            |cap| {
                let trial_arena = DecodeArena::new();
                let mut trial_policy = DecodePolicy::service();
                trial_policy.limits.max_collection_items = cap;
                let (trial_ctx, _) =
                    DecodeContext::from_root_bytes(&[], &trial_arena, &trial_policy).expect("root");
                feature_definition_records(&trial_ctx, &scan).map(|_| ())
            },
        );

        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        let Err(error) = feature_definition_records(&ctx, &scan) else {
            panic!("one more projection row exceeds the collection limit")
        };
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
                if resource.dimension == ResourceDimension::CollectionItems
                    && resource.operation == operation),
            "{error:?}"
        );
    }
}

#[test]
fn borrowed_feature_definition_preserves_nested_json() {
    let scan = scan();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let (records, _records_storage) =
        feature_definition_records(&ctx, &scan).expect("definition is admitted");
    let value = serde_json::to_value(&records[0]).expect("record serializes");
    assert_eq!(value["body"], serde_json::json!([227]));
    assert_eq!(
        value["parameter_frames"][0]["body"],
        serde_json::json!([249])
    );
    assert_eq!(
        value["outlines"][0]["local_values"],
        serde_json::json!([2.0, null, null, null, null, null])
    );
    assert_eq!(
        value["outlines"][0]["local_value_bodies"],
        serde_json::json!([[249], [], [], [], [], []])
    );
}
