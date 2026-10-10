// SPDX-License-Identifier: Apache-2.0

use crate::feature::definitions::{DefinitionIdentity, FeatureDefinition, FeatureOrderTable};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

fn assert_absent_order_route(order_table: Option<FeatureOrderTable>) {
    let definition = FeatureDefinition {
        identity: DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(1), owner_feature_id: None,
        },
        body: Vec::new(), parameter_frames: Vec::new(), outlines: Vec::new(),
        variables: None, segments: None, trim_entities: None, trim_vertices: None,
        order_table, section_3d: None, dimensions: None, relations: None,
        saved_section: Some(crate::feature::definitions::FeatureSavedSection {
            entities: vec![crate::feature::definitions::FeatureSavedEntity::Dummy(
                crate::feature::definitions::FeatureSavedDummy {
                    entity_id: Some(3), body: vec![1], offset: 0,
                },
            )],
            offset: 0,
        }),
        offset: 0,
    };
    for refused in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_entities = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let original = refused.then(|| {
            ctx.charge_work_limit(1, "before absent saved entity lookup").expect_err("zero work")
        });
        for _ in 0..2 {
            let result = super::super::section_saved_entity(&ctx, &definition, 42);
            if let Some(original) = original {
                assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
            } else { assert!(result.expect("no generated entity position").is_none()); }
        }
        if let Some(original) = original {
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
        } else { ctx.finish_session().expect("active free route"); }
    }
}

#[test]
fn absent_saved_entity_order_is_free_and_keeps_original_refusal() {
    assert_absent_order_route(None);
}

#[test]
fn empty_saved_entity_order_is_free_and_keeps_original_refusal() {
    for has_prototype in [false, true] {
        assert_absent_order_route(Some(FeatureOrderTable {
            declared_count: u32::from(has_prototype), has_prototype, entity_ref: None,
            rows: Vec::new().into(), offset: 0,
        }));
    }
}
