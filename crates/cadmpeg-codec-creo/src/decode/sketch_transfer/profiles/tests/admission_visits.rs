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

fn node_bytes<K, V>() -> u64 {
    let alignment = std::mem::align_of::<K>()
        .max(std::mem::align_of::<V>())
        .max(std::mem::align_of::<usize>());
    u64::try_from(
        11 * (std::mem::size_of::<K>() + std::mem::size_of::<V>())
            + 16 * std::mem::size_of::<usize>()
            + 2 * alignment,
    ).expect("fixed node bound")
}

#[test]
fn rejected_open_profile_has_one_frontier_visit_and_no_empty_suffix_search() {
    let edges = [ProfileEdge { external_id: 7, vertices: [1, 2], analytic_reversed: false }];
    let sketch = SketchId::mint("creo:model:sketch#1").expect("fixture identity");
    // Each two-key tree performs three initial node passes and one later pass.
    // The component tree inserts one seed and tests that seed at both endpoints.
    // Seven visits/fills: edge, remaining fill, seed search, frontier fill/pop,
    // and the two one-row incidence lists. The seed search matches before its end probe.
    let word = u64::try_from(std::mem::size_of::<usize>()).expect("word width");
    let key = u64::try_from(std::mem::size_of::<u32>()).expect("key width");
    let need = 4 * node_bytes::<u32, Vec<usize>>()
        + 8 * node_bytes::<u32, ()>()
        + 3 * node_bytes::<usize, ()>()
        + 2 * word + 10 * key + 7;
    for cap in [0, need - 1, need, need + 1] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_entities = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = profile_chains(&ctx, &edges, &sketch, ProfileTopology::ClosedSegments);
        if cap < need {
            let Err(CodecError::ResourceLimit(original)) = result else {
                panic!("insufficient work must refuse");
            };
            assert_eq!(original.dimension, ResourceDimension::WorkUnits);
            assert_eq!(original.limit, cap);
            if cap == 0 {
                assert_eq!((original.used, original.additional), (0, 1));
                assert_eq!(original.operation, "creo profile incidence sources");
            } else {
                assert_eq!((original.used, original.additional), (need - word, word));
                assert_eq!(original.operation, "creo profile component nodes");
            }
            assert!(matches!(profile_chains(&ctx, &[], &sketch, ProfileTopology::ClosedSegments),
                Err(CodecError::ResourceLimit(actual)) if actual == original));
            assert!(matches!(ctx.finish_session(),
                Err(CodecError::ResourceLimit(actual)) if actual == original));
        } else {
            assert!(result.expect("exact bound admits open-profile rejection").is_empty());
            ctx.finish_session().expect("active session");
        }
    }
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
