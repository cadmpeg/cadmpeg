// SPDX-License-Identifier: Apache-2.0
use super::super::{curve_expression_parameter_order, expression_dependency_components};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

#[test]
fn isolated_expression_components_charge_only_present_stack_entries() {
    for count in 0..=2usize {
        let dependencies: Vec<Vec<usize>> = (0..count).map(|_| Vec::new()).collect();
        let reverse: Vec<Vec<usize>> = (0..count).map(|_| Vec::new()).collect();
        // Each vertex has one visited fill, one root, two forward pops,
        // one component fill, one mark reset, one finish and one reverse pop.
        // Both stacks fit their initial capacities; growth moves no entries.
        let total = 8 * count as u64;
        let count_work = count as u64;
        let mut plan = vec![
            (count_work, "creo curve-expression visited dependencies"),
            (count_work, "creo curve-expression component roots"),
        ];
        for _ in 0..count {
            plan.extend([(1, "walk Creo curve-expression dependencies"); 2]);
        }
        plan.extend([
            (count_work, "creo curve-expression components"),
            (count_work, "creo curve-expression component marks reset"),
            (count_work, "creo curve-expression component finish traversal"),
        ]);
        for _ in 0..count {
            plan.push((1, "walk Creo curve-expression reverse dependencies"));
        }
        assert_eq!(plan.iter().map(|(units, _)| units).sum::<u64>(), total);
        for cap in 0..=total {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = expression_dependency_components(&ctx, &dependencies, &reverse);
            let original = if cap == total {
                let expected: Vec<usize> = (0..count).rev().collect();
                assert_eq!(result.expect("all present visits admitted"), expected);
                assert_eq!(ctx.resource_refusal(), None);
                let original = ctx.charge_work_limit(1, "after isolated expression components")
                    .expect_err("exact reached work");
                assert_eq!((original.dimension, original.used, original.additional),
                    (ResourceDimension::WorkUnits, total, 1));
                original
            } else {
                let original = ctx.resource_refusal().expect("present visit refusal");
                assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
                let mut used = 0;
                let mut reached = None;
                for &(additional, operation) in &plan {
                    if used + additional > cap {
                        reached = Some((used, additional, operation));
                        break;
                    }
                    used += additional;
                }
                let (used, additional, operation) = reached.expect("first unadmitted visit");
                assert_eq!((original.dimension, original.limit, original.used,
                    original.additional, original.operation),
                    (ResourceDimension::WorkUnits, cap, used, additional, operation));
                original
            };
            assert!(matches!(expression_dependency_components(&ctx, &dependencies, &reverse),
                Err(CodecError::ResourceLimit(actual)) if actual == original));
            assert_eq!(ctx.resource_refusal(), Some(original));
        }
    }
}

#[test]
fn empty_expression_ordering_needs_no_allocation_or_terminal_visit() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\xe0\x0aexpression\0\xf8\x01a=1\0";
    let mut record = crate::curve::expression_records(payload).pop().expect("expression record");
    record.assignments.clear();
    let indices = BTreeMap::new();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let order = curve_expression_parameter_order(&ctx, &record, &indices)
        .expect("empty ordering needs no input work").expect("empty order");
    assert!(order.ordinals.is_empty());
    assert!(order.cyclic_edges.is_empty());
    assert_eq!(ctx.resource_refusal(), None);
    let original = ctx.charge_work_limit(1, "after empty expression ordering")
        .expect_err("zero cap");
    assert_eq!((original.dimension, original.used, original.additional),
        (ResourceDimension::WorkUnits, 0, 1));
    assert!(matches!(curve_expression_parameter_order(&ctx, &record, &indices),
        Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert_eq!(ctx.resource_refusal(), Some(original));
}
