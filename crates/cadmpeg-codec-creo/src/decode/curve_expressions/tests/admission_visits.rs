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


fn source_lines(text: &[&str]) -> Vec<crate::curve::CurveExpressionLine> {
    text.iter().enumerate().map(|(offset, text)| crate::curve::CurveExpressionLine {
        text: (*text).to_owned(), offset,
    }).collect()
}

#[test]
fn expression_source_projection_admits_each_index_once() {
    for (text, expected) in [
        (Vec::new(), ""), (vec!["a"], "a"), (vec!["a", "bc"], "a\nbc"),
        (vec!["", "bc"], "\nbc"), (vec!["é", "β"], "é\nβ"),
    ] {
        let lines = source_lines(&text);
        let count = lines.len() as u64;
        // One projection, one length pass and one output pass per line;
        // then exactly the source and separating newline UTF-8 bytes.
        let total = 3 * count + expected.len() as u64;
        let parts_operation = "creo curve-expression source text parts";
        let output_operation = "creo curve-expression feature source text";
        let mut plan = vec![(1, parts_operation); lines.len()];
        plan.extend([(count, output_operation); 2]);
        for (index, text) in text.iter().enumerate() {
            if index != 0 { plan.push((1, output_operation)); }
            plan.push((text.len() as u64, output_operation));
        }
        assert_eq!(plan.iter().map(|(units, _)| units).sum::<u64>(), total);
        let reference_bytes = (lines.len() * std::mem::size_of::<&str>()) as u64;
        for cap in 0..=total {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            policy.limits.max_materialized_bytes = reference_bytes;
            policy.limits.max_collection_items = count;
            policy.limits.max_retained_bytes = expected.len() as u64;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = super::super::curve_expression_source_text(&ctx, &lines);
            let original = if cap == total {
                let actual = result.expect("exact source passes and bytes admitted");
                assert_eq!(actual, expected);
                assert_eq!(ctx.resource_refusal(), None);
                // The returned String is retained; the borrowed-view vector
                // has dropped and its entire materialized allowance is free.
                let scratch = ctx.reserve_scoped(reference_bytes, "after expression source views")
                    .expect("source-view scratch refunded");
                drop(scratch);
                let original = ctx.charge_work_limit(1, "after expression source text")
                    .expect_err("exact reached work");
                assert_eq!((original.dimension, original.used, original.additional),
                    (ResourceDimension::WorkUnits, total, 1));
                original
            } else {
                let original = ctx.resource_refusal().expect("source work refusal");
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
                let (used, additional, operation) = reached.expect("first unadmitted source work");
                assert_eq!((original.dimension, original.limit, original.used,
                    original.additional, original.operation),
                    (ResourceDimension::WorkUnits, cap, used, additional, operation));
                original
            };
            assert!(matches!(super::super::curve_expression_source_text(&ctx, &lines),
                Err(CodecError::ResourceLimit(actual)) if actual == original));
            assert_eq!(ctx.resource_refusal(), Some(original));
        }
    }
}

#[test]
fn expression_source_slots_refuse_before_view_allocation() {
    let lines = source_lines(&["a", "bc"]);
    for cap in 0..2 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = super::super::curve_expression_source_text(&ctx, &lines);
        let original = ctx.resource_refusal().expect("two source slots needed");
        assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
        assert_eq!((original.dimension, original.limit, original.used,
            original.additional, original.operation),
            (ResourceDimension::CollectionItems, cap, 0, 2, "creo curve-expression source text parts"));
        assert!(matches!(super::super::curve_expression_source_text(&ctx, &lines),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
}

#[test]
fn expression_source_view_bytes_refuse_before_allocation() {
    let lines = source_lines(&["a", "bc"]);
    let bytes = (lines.len() * std::mem::size_of::<&str>()) as u64;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = bytes - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let result = super::super::curve_expression_source_text(&ctx, &lines);
    let original = ctx.resource_refusal().expect("exact source view bytes needed");
    assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert_eq!((original.dimension, original.used, original.additional, original.operation),
        (ResourceDimension::MaterializedBytes, 0, bytes, "creo curve-expression source text parts"));
    assert!(matches!(super::super::curve_expression_source_text(&ctx, &lines),
        Err(CodecError::ResourceLimit(actual)) if actual == original));
}

#[test]
fn expression_source_output_bytes_refuse_before_copy() {
    let lines = source_lines(&["a", "bc"]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One a, one newline and two bytes bc need four retained bytes.
    policy.limits.max_retained_bytes = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let result = super::super::curve_expression_source_text(&ctx, &lines);
    let original = ctx.resource_refusal().expect("exact joined source bytes needed");
    assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert_eq!((original.dimension, original.used, original.additional, original.operation),
        (ResourceDimension::RetainedBytes, 0, 4, "creo curve-expression feature source text"));
    assert!(matches!(super::super::curve_expression_source_text(&ctx, &lines),
        Err(CodecError::ResourceLimit(actual)) if actual == original));
}
