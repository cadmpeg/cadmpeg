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
        let total = 8 * cadmpeg_core::decode::u64_from_index(count);
        crate::test_support::assert_refusal_order(ResourceDimension::WorkUnits, &[], |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = expression_dependency_components(&ctx, &dependencies, &reverse);
            let (original, admitted) = if cap == total {
                let expected: Vec<usize> = (0..count).rev().collect();
                assert_eq!(result.expect("all present visits admitted"), expected);
                assert_eq!(ctx.resource_refusal(), None);
                let original = ctx
                    .charge_work_limit(1, "after isolated expression components")
                    .expect_err("exact reached work");
                assert_eq!(
                    (original.dimension, original.used, original.additional),
                    (ResourceDimension::WorkUnits, total, 1)
                );
                (original, true)
            } else {
                let original = ctx.resource_refusal().expect("present visit refusal");
                assert!(
                    matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original)
                );
                assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                assert_eq!(original.limit, cap);
                (original, false)
            };
            assert!(
                matches!(expression_dependency_components(&ctx, &dependencies, &reverse),
                Err(CodecError::ResourceLimit(actual)) if actual == original)
            );
            assert_eq!(ctx.resource_refusal(), Some(original));
            if admitted {
                Ok(())
            } else {
                Err(CodecError::ResourceLimit(original))
            }
        });
    }
}

#[test]
fn empty_expression_ordering_needs_no_allocation_or_terminal_visit() {
    let payload =
        b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\xe0\x0aexpression\0\xf8\x01a=1\0";
    let mut record = crate::curve::expression_records(payload)
        .pop()
        .expect("expression record");
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
        .expect("empty ordering needs no input work")
        .expect("empty order");
    assert!(order.ordinals.is_empty());
    assert!(order.cyclic_edges.is_empty());
    assert_eq!(ctx.resource_refusal(), None);
    let original = ctx
        .charge_work_limit(1, "after empty expression ordering")
        .expect_err("zero cap");
    assert_eq!(
        (original.dimension, original.used, original.additional),
        (ResourceDimension::WorkUnits, 0, 1)
    );
    assert!(
        matches!(curve_expression_parameter_order(&ctx, &record, &indices),
        Err(CodecError::ResourceLimit(actual)) if actual == original)
    );
    assert_eq!(ctx.resource_refusal(), Some(original));
}

fn source_lines(text: &[&str]) -> Vec<crate::curve::CurveExpressionLine> {
    text.iter()
        .enumerate()
        .map(|(offset, text)| crate::curve::CurveExpressionLine {
            text: (*text).to_owned(),
            offset,
        })
        .collect()
}

#[test]
fn expression_source_projection_admits_each_index_once() {
    for (text, expected) in [
        (Vec::new(), ""),
        (vec!["a"], "a"),
        (vec!["a", "bc"], "a\nbc"),
        (vec!["", "bc"], "\nbc"),
        (vec!["é", "β"], "é\nβ"),
    ] {
        let lines = source_lines(&text);
        let count = cadmpeg_core::decode::u64_from_index(lines.len());
        // One projection, one length pass and one output pass per line;
        // then exactly the source and separating newline UTF-8 bytes.
        let output_bytes = cadmpeg_core::decode::u64_from_index(expected.len());
        let total = 3 * count + output_bytes;
        let reference_bytes =
            cadmpeg_core::decode::u64_from_index(lines.len() * std::mem::size_of::<&str>());
        crate::test_support::assert_refusal_order(ResourceDimension::WorkUnits, &[], |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            policy.limits.max_materialized_bytes = reference_bytes;
            policy.limits.max_collection_items = count;
            policy.limits.max_retained_bytes = output_bytes;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = super::super::curve_expression_source_text(&ctx, &lines);
            let (original, admitted) = if cap == total {
                let actual = result.expect("exact source passes and bytes admitted");
                assert_eq!(actual, expected);
                assert_eq!(ctx.resource_refusal(), None);
                // The returned String is retained; the borrowed-view vector
                // has dropped and its entire materialized allowance is free.
                let scratch = ctx
                    .reserve_scoped(reference_bytes, "after expression source views")
                    .expect("source-view scratch refunded");
                drop(scratch);
                let original = ctx
                    .charge_work_limit(1, "after expression source text")
                    .expect_err("exact reached work");
                assert_eq!(
                    (original.dimension, original.used, original.additional),
                    (ResourceDimension::WorkUnits, total, 1)
                );
                (original, true)
            } else {
                let original = ctx.resource_refusal().expect("source work refusal");
                assert!(
                    matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original)
                );
                assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                assert_eq!(original.limit, cap);
                (original, false)
            };
            assert!(
                matches!(super::super::curve_expression_source_text(&ctx, &lines),
                Err(CodecError::ResourceLimit(actual)) if actual == original)
            );
            assert_eq!(ctx.resource_refusal(), Some(original));
            if admitted {
                Ok(())
            } else {
                Err(CodecError::ResourceLimit(original))
            }
        });
    }
}

#[test]
fn expression_source_slots_refuse_before_view_allocation() {
    let lines = source_lines(&["a", "bc"]);
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::CollectionItems,
        "creo curve-expression source text parts",
        |ctx| {
            let result = super::super::curve_expression_source_text(ctx, &lines);
            if let Err(CodecError::ResourceLimit(original)) = &result {
                assert!(
                    matches!(super::super::curve_expression_source_text(ctx, &lines),
                    Err(CodecError::ResourceLimit(actual)) if actual == *original)
                );
            }
            result
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(original)
        if (original.dimension, original.limit, original.used, original.additional, original.operation)
            == (ResourceDimension::CollectionItems, 1, 0, 2, "creo curve-expression source text parts")));
}

#[test]
fn expression_source_view_bytes_refuse_before_allocation() {
    let lines = source_lines(&["a", "bc"]);
    let bytes = cadmpeg_core::decode::u64_from_index(lines.len() * std::mem::size_of::<&str>());
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::MaterializedBytes,
        "creo curve-expression source text parts",
        |ctx| {
            let result = super::super::curve_expression_source_text(ctx, &lines);
            if let Err(CodecError::ResourceLimit(original)) = &result {
                assert!(
                    matches!(super::super::curve_expression_source_text(ctx, &lines),
                    Err(CodecError::ResourceLimit(actual)) if actual == *original)
                );
            }
            result
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(original)
        if (original.dimension, original.used, original.additional, original.operation)
            == (ResourceDimension::MaterializedBytes, 0, bytes, "creo curve-expression source text parts")));
}

#[test]
fn expression_source_output_bytes_refuse_before_copy() {
    let lines = source_lines(&["a", "bc"]);
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::RetainedBytes,
        "creo curve-expression feature source text",
        |ctx| {
            let result = super::super::curve_expression_source_text(ctx, &lines);
            if let Err(CodecError::ResourceLimit(original)) = &result {
                assert!(
                    matches!(super::super::curve_expression_source_text(ctx, &lines),
                    Err(CodecError::ResourceLimit(actual)) if actual == *original)
                );
            }
            result
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(original)
        if (original.dimension, original.used, original.additional, original.operation)
            == (ResourceDimension::RetainedBytes, 0, 4, "creo curve-expression feature source text")));
}

#[test]
fn curve_expression_visit_marks_refuse_before_allocation() {
    let dependencies = [vec![1], vec![]];
    let reverse = [vec![], vec![0]];
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::CollectionItems,
        "creo curve-expression visited dependencies",
        |ctx| expression_dependency_components(ctx, &dependencies, &reverse),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if (limit.dimension, limit.limit, limit.used, limit.additional, limit.operation)
            == (ResourceDimension::CollectionItems, 1, 0, 2, "creo curve-expression visited dependencies")));
}
