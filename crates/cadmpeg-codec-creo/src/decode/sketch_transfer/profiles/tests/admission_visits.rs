// SPDX-License-Identifier: Apache-2.0

use super::super::{profile_chains, resolved_profile_chains, resolved_segment_profile_chains,
    solver_only_section_entity_family, solver_only_section_entity_offset, ProfileEdge, ProfileTopology};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::sketches::SketchId;
use std::collections::BTreeSet;

#[test]
fn rejected_open_profile_has_one_frontier_visit_and_no_empty_suffix_search() {
    let edges = [ProfileEdge {
        external_id: 7,
        vertices: [1, 2],
        analytic_reversed: false,
    }];
    let sketch = SketchId::mint("creo:model:sketch#1").expect("fixture identity");
    let frontier_visits = std::cell::Cell::new(0);
    let seed_visits = std::cell::Cell::new(0);
    let last_operation = std::cell::Cell::new(None);
    crate::test_support::assert_refusal_order(
        ResourceDimension::WorkUnits,
        &[
            "creo profile incidence sources",
            "creo profile component nodes",
        ],
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
                    if original.operation == "creo profile frontier visits" {
                        frontier_visits.set(frontier_visits.get() + 1);
                    }
                    if original.operation == "creo profile components" {
                        seed_visits.set(seed_visits.get() + 1);
                    }
                    assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                    assert_eq!(original.limit, cap);
                    if cap == 0 {
                        assert_eq!((original.used, original.additional), (0, 1));
                        assert_eq!(original.operation, "creo profile incidence sources");
                    }

                }
                Ok(rows) => {
                    assert!(rows.is_empty());
                    ctx.finish_session().expect("active session");
                }
                Err(error) => panic!("unexpected profile error: {error:?}"),
            }
            result
        },
    );
    assert_eq!(frontier_visits.get(), 1);
    assert_eq!(seed_visits.get(), 1);
    assert_eq!(last_operation.get(), Some("creo profile component nodes"));
}

#[test]
fn empty_profile_and_absent_or_incomplete_segment_routes_are_free() {
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
        let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_entities = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        for result in [
            profile_chains(&ctx, &[], &sketch, ProfileTopology::ClosedSegments),
            resolved_segment_profile_chains(&ctx, &definition, &sketch, &emitted),
            resolved_profile_chains(&ctx, &definition, &sketch, &emitted),
        ] {
            assert!(result.expect("empty profiles").is_empty());
        }
        ctx.finish_session().expect("active session");
    }
}
#[test]
fn existing_segment_solver_recovery_is_free() {
    let definition = super::definition(201, false);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_entities = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(solver_only_section_entity_offset(&ctx, &definition, 7).expect("existing segment offset"), None);
    assert_eq!(solver_only_section_entity_family(&ctx, &definition, 7).expect("existing segment family"), None);
    ctx.finish_session().expect("active session");
}
