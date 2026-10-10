// SPDX-License-Identifier: Apache-2.0
use crate::native::{attribute_table_value_width, OverdeclaredCounts, UnstatableAttributeTable};
use crate::parameter::{DefaultTailCount, ParameterRecord};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, ResourceLimit};
use cadmpeg_core::CodecError;

fn with_count_owner(mut run: impl FnMut(&mut OverdeclaredCounts<'_, '_>, Option<ResourceLimit>)) {
    for dimension in [None, Some(ResourceDimension::WorkUnits), Some(ResourceDimension::CollectionItems),
        Some(ResourceDimension::MaterializedBytes), Some(ResourceDimension::RetainedBytes),
        Some(ResourceDimension::Entities), Some(ResourceDimension::RecursionDepth)] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_entities = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty caller input");
        let mut counts = OverdeclaredCounts::new(&ctx).expect("empty count owner before refusal");
        let original = dimension.map(|dimension| {
            let refused = match dimension {
                ResourceDimension::WorkUnits => ctx.charge_work(1, "test original native count refusal"),
                ResourceDimension::CollectionItems => ctx.charge_collection_items(1, "test original native count refusal"),
                ResourceDimension::MaterializedBytes => ctx.reserve_scoped(1, "test original native count refusal").map(|_| ()),
                ResourceDimension::RetainedBytes => ctx.charge_retained(1, "test original native count refusal"),
                ResourceDimension::Entities => ctx.charge_entities(1, "test original native count refusal"),
                ResourceDimension::RecursionDepth => ctx.enter_nested("test original native count refusal").map(|_| ()),
                _ => panic!("native count refusal dimension"),
            };
            let Err(CodecError::ResourceLimit(first)) = refused else { panic!("original refusal"); };
            assert_eq!(first.dimension, dimension);
            first
        });
        for _ in 0..64 {
            run(&mut counts, original);
            assert!(counts.entries.is_empty());
        }
        drop(counts);
        match original {
            Some(first) => assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first)),
            None => ctx.finish_session().expect("fresh count owner finishes"),
        }
    }
}

fn scalar_count(result: Result<usize, CodecError>, original: Option<ResourceLimit>, fresh: usize) {
    match original {
        Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
        None => assert_eq!(result.expect("fixed count recovery is free"), fresh),
    }
}

#[test]
fn native_held_count_preserves_original_refusal() {
    with_count_owner(|counts, original| scalar_count(counts.admit(1, DefaultTailCount::Held(3)), original, 3));
}

#[test]
fn native_unreadable_count_preserves_original_refusal() {
    with_count_owner(|counts, original| scalar_count(counts.admit(1, DefaultTailCount::Unreadable), original, 0));
}

#[test]
fn native_counted_tail_without_record_preserves_original_refusal() {
    with_count_owner(|counts, original| scalar_count(counts.counted_tail(1, None, 0, 1, 1), original, 0));
}

#[test]
fn native_counted_tail_at_without_record_preserves_original_refusal() {
    with_count_owner(|counts, original| scalar_count(counts.counted_tail_at(1, None, 0, 1, 2, 1), original, 0));
}

#[test]
fn native_counted_complete_without_record_preserves_original_refusal() {
    with_count_owner(|counts, original| scalar_count(counts.counted_complete(1, None, 0, 1, 2, 1), original, 0));
}

#[test]
fn native_missing_attribute_count_preserves_original_refusal() {
    let record = ParameterRecord::from_test_tokens(1, 1..2, Vec::new(), 0, Vec::new(), Vec::new());
    crate::test_support::with_entry_context(|ctx, original| {
        let result = attribute_table_value_width(&record, 0, ctx);
        match original {
            Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
            None => assert!(matches!(result, Ok(Err(UnstatableAttributeTable::AttributeCount)))),
        }
    });
}

#[test]
fn native_display_number_preserves_original_refusal() {
    crate::test_support::with_entry_context(|ctx, original| {
        let result = crate::native::resolve_display_ref(ctx, None, 3, crate::graph::ReferenceKind::Color, "color");
        match original {
            Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
            None => assert!(matches!(result, Ok(crate::native::DisplayRef::Number(3)))),
        }
    });
}

#[test]
fn native_display_without_references_preserves_original_refusal() {
    crate::test_support::with_entry_context(|ctx, original| {
        let result = crate::native::resolve_display_ref(ctx, None, -3, crate::graph::ReferenceKind::Color, "color");
        match original {
            Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
            None => assert!(matches!(result, Ok(crate::native::DisplayRef::Definition { pointer: -3, target: None }))),
        }
    });
}

#[test]
fn native_absent_label_display_preserves_original_refusal() {
    let references = std::collections::BTreeMap::new();
    crate::test_support::with_entry_context(|ctx, original| {
        let result = crate::native::resolved_label_display_definition(ctx, &references, 1, 0);
        match original {
            Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
            None => assert!(result.expect("absent label display is free").is_none()),
        }
    });
}

fn scalar_token(value: crate::parameter::TokenValue) {
    crate::test_support::with_entry_context(|ctx, original| {
        let result = crate::native::copy_native_token_value(ctx, &value);
        match original {
            Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
            None => assert_eq!(result.expect("scalar token copy is free"), value),
        }
    });
}

#[test]
fn native_omitted_token_copy_preserves_original_refusal() {
    scalar_token(crate::parameter::TokenValue::Omitted);
}

#[test]
fn native_integer_token_copy_preserves_original_refusal() {
    scalar_token(crate::parameter::TokenValue::Integer(7));
}

#[test]
fn native_real_token_copy_preserves_original_refusal() {
    scalar_token(crate::parameter::TokenValue::real(2.5));
}

fn present_record_counts(record: &ParameterRecord, fresh: usize) {
    with_count_owner(|counts, original| {
        scalar_count(counts.counted_tail(1, Some(record), 3, 0, 1), original, fresh);
        scalar_count(counts.counted_tail_at(1, Some(record), 3, 0, 1, 1), original, fresh);
        scalar_count(counts.counted_complete(1, Some(record), 3, 0, 1, 1), original, fresh);
    });
}

#[test]
fn native_present_held_record_counts_preserve_original_refusal() {
    use crate::parameter::{Token, TokenValue};
    let values = [2, 7, 8];
    let record = ParameterRecord::from_test_tokens(1, 1..2, Vec::new(), values.len(),
        values.into_iter().map(|value| Token {
            value: TokenValue::Integer(value), span: 0..0,
        }).collect(), Vec::new());
    // Both one-slot items are present before the record delimiter.
    present_record_counts(&record, 2);
}

#[test]
fn native_present_unreadable_record_counts_preserve_original_refusal() {
    use crate::parameter::{Token, TokenValue};
    let values = [TokenValue::Omitted, TokenValue::Integer(7), TokenValue::Integer(8)];
    let record = ParameterRecord::from_test_tokens(1, 1..2, Vec::new(), values.len(),
        values.into_iter().map(|value| Token { value, span: 0..0 }).collect(), Vec::new());
    present_record_counts(&record, 0);
}
