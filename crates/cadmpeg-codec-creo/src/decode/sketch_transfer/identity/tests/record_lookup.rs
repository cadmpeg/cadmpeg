// SPDX-License-Identifier: Apache-2.0

use crate::feature::definitions::{
    FeatureSavedDummy, FeatureSavedEntity, FeatureSavedLine, FeatureSavedSection,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn dummy(entity_id: u32) -> FeatureSavedEntity {
    FeatureSavedEntity::Dummy(FeatureSavedDummy {
        entity_id: Some(entity_id),
        body: Vec::new(),
        offset: 0,
    })
}

fn assert_record_visits(
    entities: Vec<FeatureSavedEntity>,
    internal_id: u32,
    selected: Option<usize>,
    visits: u64,
) {
    let mut definition = super::definition(None);
    definition.saved_section = Some(FeatureSavedSection {
        entities,
        offset: 0,
    });
    let saved = definition.saved_section.as_ref().expect("saved section");
    crate::test_support::assert_refusal_order(ResourceDimension::WorkUnits, &[], |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_entities = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let run =
            || super::super::saved_section_entity_by_internal_id(&ctx, &definition, internal_id);
        let result = run();
        let original = match &result {
            Ok(actual) => {
                match selected {
                    Some(index) => assert!(std::ptr::eq(
                        actual.expect("unique record"),
                        &raw const saved.entities[index]
                    )),
                    None => assert!(actual.is_none()),
                }
                let refusal = ctx
                    .charge_work_limit(u64::MAX, "after unique saved record lookup")
                    .expect_err("probe completed work");
                assert_eq!(refusal.used, visits);
                refusal
            }
            Err(CodecError::ResourceLimit(refusal)) => {
                assert_eq!(
                    (
                        refusal.dimension,
                        refusal.operation,
                        refusal.used,
                        refusal.additional,
                        refusal.limit
                    ),
                    (
                        ResourceDimension::WorkUnits,
                        "creo saved section entities",
                        cap,
                        1,
                        cap
                    )
                );
                *refusal
            }
            Err(error) => panic!("unexpected lookup error: {error:?}"),
        };
        for _ in 0..2 {
            assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
        }
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original)
        );
        result.map(|_| ())
    });
}

#[test]
fn unique_saved_record_visits_each_row_once_and_returns_the_original_record() {
    for count in 0..=4 {
        let entities: Vec<_> = (0..count).map(dummy).collect();
        // Uniqueness visits the complete roster even when the target is first.
        // An empty roster is free. A nonempty complete walk charges its end probe.
        let visits = if count == 0 { 0 } else { u64::from(count) + 1 };
        for internal_id in [0, count.saturating_sub(1), count] {
            let selected =
                (internal_id < count).then(|| usize::try_from(internal_id).expect("fixture index"));
            assert_record_visits(entities.clone(), internal_id, selected, visits);
        }
    }
}

#[test]
fn saved_identity_collision_across_kinds_stops_at_the_second_match() {
    let line = FeatureSavedEntity::Line(FeatureSavedLine {
        entity_id: 3,
        references: Vec::new(),
        attributes: Vec::new(),
        endpoints: [[None; 3]; 2],
        body: Vec::new(),
        offset: 0,
    });
    for entities in [
        vec![line.clone(), dummy(3), dummy(9)],
        vec![dummy(3), line, dummy(9)],
    ] {
        // The second matching semantic identity establishes ambiguity. The
        // later row and the generic end probe are not executed.
        assert_record_visits(entities, 3, None, 2);
    }
}
