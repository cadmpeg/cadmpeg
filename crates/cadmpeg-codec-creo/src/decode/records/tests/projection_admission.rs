// SPDX-License-Identifier: Apache-2.0
use crate::feature::definitions::{
    DefinitionIdentity, FeatureDefinition, FeatureVariableRow, FeatureVariableTable, ScalarLane,
    VariableType,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn native_variable_projection_refuses_copy_before_output_slot() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.definitions.push(FeatureDefinition {
        identity: DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(7),
            owner_feature_id: None,
        },
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(FeatureVariableTable {
            declared_count: 2,
            entity_ref: None,
            offset: 0,
            rows: vec![FeatureVariableRow {
                variable_type: VariableType::Selector,
                key: 1,
                value: ScalarLane::Undefined,
                value_body: vec![0; 2000],
                guess: ScalarLane::Undefined,
                guess_body: Vec::new(),
                known: None,
                homogeneity: None,
                uvar_id: None,
                offset: 0,
            }],
        }),
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    });
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
        ResourceDimension::CollectionItems, Some("creo native sketch variables"), |cap| {
            let trial_arena = DecodeArena::new();
            let mut trial_policy = DecodePolicy::service();
            trial_policy.limits.max_collection_items = cap;
            let (trial_ctx, _) = DecodeContext::from_root_bytes(&[], &trial_arena, &trial_policy).expect("root");
            super::super::sketch_records(&trial_ctx, &scan).map(|_| ())
        });
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
        ResourceDimension::RetainedBytes, Some("creo native sketch variable value body"), |cap| {
            let trial_arena = DecodeArena::new();
            let mut trial_policy = DecodePolicy::service();
            trial_policy.limits.max_retained_bytes = cap;
            let (trial_ctx, _) = DecodeContext::from_root_bytes(&[], &trial_arena, &trial_policy).expect("root");
            super::super::sketch_records(&trial_ctx, &scan).map(|_| ())
        });
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let error = super::super::sketch_records(&ctx, &scan)
        .map(|_| ())
        .expect_err("projection copy is refused first");
    let CodecError::ResourceLimit(refusal) = error else {
        panic!("resource refusal")
    };
    assert_eq!(refusal.operation, "creo native sketch variable value body");
    assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(ctx.resource_refusal(), Some(refusal));
}
