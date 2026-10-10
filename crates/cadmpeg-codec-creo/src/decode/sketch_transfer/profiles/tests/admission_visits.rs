// SPDX-License-Identifier: Apache-2.0

use super::super::{
    profile_chains, resolved_profile_chains, resolved_segment_profile_chains,
    solver_only_section_entity_family, solver_only_section_entity_offset, ProfileEdge,
    ProfileTopology,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::sketches::SketchId;
use std::collections::BTreeSet;

#[test]
fn rejected_open_profile_has_one_frontier_visit_and_no_empty_suffix_search() {
    let edges = [ProfileEdge { external_id: 7, vertices: [1, 2], analytic_reversed: false }];
    let sketch = SketchId::mint("creo:model:sketch#1").expect("fixture identity");
    let frontier_visits = std::cell::Cell::new(0);
    let seed_visits = std::cell::Cell::new(0);
    let last_operation = std::cell::Cell::new(None);
    crate::test_support::assert_refusal_order(
        ResourceDimension::WorkUnits,
        &["creo profile incidence sources", "creo profile component nodes"],
        |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_entities = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = profile_chains(&ctx, &edges, &sketch, ProfileTopology::ClosedSegments);
        match &result {
            Err(CodecError::ResourceLimit(original)) => {
                last_operation.set(Some(original.operation));
                if original.operation == "creo profile frontier visits" { frontier_visits.set(frontier_visits.get() + 1); }
                if original.operation == "creo profile components" { seed_visits.set(seed_visits.get() + 1); }
                assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                assert_eq!(original.limit, cap);
                if cap == 0 {
                    assert_eq!((original.used, original.additional), (0, 1));
                    assert_eq!(original.operation, "creo profile incidence sources");
                }
                assert!(matches!(profile_chains(&ctx, &[], &sketch, ProfileTopology::ClosedSegments),
                    Err(CodecError::ResourceLimit(actual)) if actual == *original));
                assert!(matches!(ctx.finish_session(),
                    Err(CodecError::ResourceLimit(actual)) if actual == *original));
            }
            Ok(rows) => {
                assert!(rows.is_empty());
                ctx.finish_session().expect("active session");
            }
            Err(error) => panic!("unexpected profile error: {error:?}"),
        }
        result
    });
    assert_eq!(frontier_visits.get(), 1);
    assert_eq!(seed_visits.get(), 1);
    assert_eq!(last_operation.get(), Some("creo profile component nodes"));
}

#[test]
fn empty_profile_and_absent_or_incomplete_segment_routes_are_free_and_keep_refusal() {
    let sketch = SketchId::mint("creo:model:sketch#1").expect("fixture identity");
    let emitted = BTreeSet::new();
    for variant in 0..3 {
        let mut definition = super::definition(201, false);
        if variant == 0 {
            definition.segments = None;
        } else {
            let table = definition.segments.as_mut().expect("segments");
            table.rows = Default::default();
            table.declared_count = if variant == 1 { 0 } else { 1 };
        }
        for refused in [false, true] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = 0;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
            policy.limits.max_entities = 0;
            policy.limits.max_recursion_depth = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let original = refused.then(|| ctx.charge_work_limit(1, "before empty profiles")
                .expect_err("zero work"));
            for _ in 0..2 {
                let results = [
                    profile_chains(&ctx, &[], &sketch, ProfileTopology::ClosedSegments),
                    resolved_segment_profile_chains(&ctx, &definition, &sketch, &emitted),
                    resolved_profile_chains(&ctx, &definition, &sketch, &emitted),
                ];
                for result in results {
                    if let Some(original) = original {
                        assert!(matches!(result,
                            Err(CodecError::ResourceLimit(actual)) if actual == original));
                    } else {
                        assert!(result.expect("empty profiles").is_empty());
                    }
                }
            }
            if let Some(original) = original {
                assert!(matches!(ctx.finish_session(),
                    Err(CodecError::ResourceLimit(actual)) if actual == original));
            } else {
                ctx.finish_session().expect("active session");
            }
        }
    }
}

#[test]
fn existing_segment_solver_recovery_is_free_and_keeps_original_refusal() {
    let definition = super::definition(201, false);
    for refused in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_entities = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let original = refused.then(|| ctx.charge_work_limit(1, "before existing segment")
            .expect_err("zero work"));
        for _ in 0..2 {
            let offset = solver_only_section_entity_offset(&ctx, &definition, 7);
            let family = solver_only_section_entity_family(&ctx, &definition, 7);
            if let Some(original) = original {
                assert!(matches!(offset,
                    Err(CodecError::ResourceLimit(actual)) if actual == original));
                assert!(matches!(family,
                    Err(CodecError::ResourceLimit(actual)) if actual == original));
            } else {
                assert_eq!(offset.expect("existing segment offset"), None);
                assert_eq!(family.expect("existing segment family"), None);
            }
        }
        if let Some(original) = original {
            assert!(matches!(ctx.finish_session(),
                Err(CodecError::ResourceLimit(actual)) if actual == original));
        } else {
            ctx.finish_session().expect("active session");
        }
    }
}
