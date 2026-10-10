// SPDX-License-Identifier: Apache-2.0

use crate::brep::attributes::{attribute_chain_color_carrier, attribute_chain_name,
    attribute_value, collect_attributes, direct_attribute_color};
use crate::sab::{Record, Token};
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
use std::collections::{HashMap, HashSet};

fn record(tokens: Vec<Token>) -> Record {
    crate::test_support::sab::record(
1,
"unknown".into(),
tokens.into(),
0,
0
)
}

#[test]
fn attribute_fixed_scalar_preserves_original_refusal() {
    let token = Token::Long(42);
    crate::test_support::with_entry_context(|ctx, original| {
        let result = attribute_value(ctx, &token, crate::asm_format!("f3d"));
        match original {
            Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
            None => { assert_eq!(result.expect("fixed scalar is free"), Some(AttributeValue::Integer(42))); }
        }
    });
}

#[test]
fn attribute_nonfinite_scalar_preserves_original_refusal() {
    let token = Token::Double(f64::NAN);
    crate::test_support::with_entry_context(|ctx, original| {
        let result = attribute_value(ctx, &token, crate::asm_format!("f3d"));
        match original {
            Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
            None => { assert_eq!(result.expect("nonfinite scalar recovery is free"), None); }
        }
    });
}

#[test]
fn attribute_invalid_vector_preserves_original_refusal() {
    let token = Token::Vector3([1.0, f64::NAN, 3.0]);
    crate::test_support::with_entry_context(|ctx, original| {
        let result = attribute_value(ctx, &token, crate::asm_format!("f3d"));
        match original {
            Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
            None => { assert_eq!(result.expect("invalid vector needs no lane"), None); }
        }
    });
}

#[test]
fn attribute_absent_color_payload_preserves_original_refusal() {
    let record = record(Vec::new());
    crate::test_support::with_entry_context(|ctx, original| {
        let result = direct_attribute_color(ctx, &record);
        match original {
            Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
            None => { assert!(result.expect("empty record recovery is free").is_none()); }
        }
    });
}

#[test]
fn attribute_unknown_color_kind_preserves_original_refusal() {
    let record = record(vec![Token::Ref(-1)]);
    crate::test_support::with_entry_context(|ctx, original| {
        let result = direct_attribute_color(ctx, &record);
        match original {
            Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
            None => { assert!(result.expect("unknown color recovery is free").is_none()); }
        }
    });
}

#[test]
fn attribute_absent_name_chain_preserves_original_refusal() {
    let entity = record(Vec::new());
    let by_index = HashMap::new();
    crate::test_support::with_entry_context(|ctx, original| {
        let result = attribute_chain_name(ctx, &entity, &by_index);
        match original {
            Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
            None => { assert_eq!(result.expect("absent name chain is free"), None); }
        }
    });
}

#[test]
fn attribute_absent_color_chain_preserves_original_refusal_without_lookup() {
    let entity = record(Vec::new());
    crate::test_support::with_entry_context(|ctx, original| {
        let mut calls = 0;
        let result = attribute_chain_color_carrier(ctx, &entity, 1, |_| {
            calls += 1;
            None
        });
        match original {
            Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
            None => assert!(result.expect("absent color chain is free").is_none()),
        }
        assert_eq!(calls, 0);
    });
}

#[test]
fn attribute_empty_collection_preserves_original_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let entity = record(Vec::new());
    let by_index = HashMap::new();
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
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let mut storage = ctx.reserve_scoped(0, "test emitted attributes").expect("empty owner");
        let mut emitted = HashSet::new();
        let mut output = Vec::new();
        let original = dimension.map(|dimension| {
            let refused = match dimension {
                ResourceDimension::WorkUnits => ctx.charge_work(1, "original attribute refusal"),
                ResourceDimension::CollectionItems => ctx.charge_collection_items(1, "original attribute refusal"),
                ResourceDimension::MaterializedBytes => ctx.reserve_scoped(1, "original attribute refusal").map(|_| ()),
                ResourceDimension::RetainedBytes => ctx.charge_retained(1, "original attribute refusal"),
                ResourceDimension::Entities => ctx.charge_entities(1, "original attribute refusal"),
                ResourceDimension::RecursionDepth => ctx.enter_nested("original attribute refusal").map(|_| ()),
                _ => panic!("test dimension"),
            };
            let Err(CodecError::ResourceLimit(first)) = refused else { panic!("original refusal"); };
            assert_eq!(first.dimension, dimension);
            first
        });
        for _ in 0..64 {
            let result = collect_attributes(&ctx, &entity, &AttributeTarget::Document, &by_index,
                (&mut emitted, &mut storage), &mut output, crate::asm_format!("f3d"));
            match original {
                Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
                None => result.expect("empty attribute collection is free"),
            }
            assert!(emitted.is_empty());
            assert!(output.is_empty());
        }
        drop(output);
        drop(emitted);
        drop(storage);
        match original {
            Some(first) => assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first)),
            None => ctx.finish_session().expect("fresh session"),
        }
    }
}
