// SPDX-License-Identifier: Apache-2.0

use super::super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use crate::test_support::with_entry_context;

#[test]
fn absent_fem_parameter_string_preserves_original_refusal() {
    let integer = super::integer_record(1, &[136, 1, 0]);
    with_entry_context(|ctx, original| {
        for record in [None, Some(&integer)] {
            for index in [0, 1, usize::MAX] {
                let result = record_string(ctx, record, index);
                match original {
                    Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
                    None => assert_eq!(result.unwrap(), None),
                }
            }
        }
    });
}

#[test]
fn absent_fem_resolved_identity_preserves_original_refusal() {
    with_entry_context(|ctx, original| {
        let result = resolved_id(ctx, None);
        match original {
            Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
            None => assert_eq!(result.unwrap(), None),
        }
    });
}

fn absent_reference(note: bool) {
    for populated in [false, true] {
        let directory = if populated { vec![crate::test_support::directory_target(1, 212)] } else { Vec::new() };
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
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
            let resolver = ParameterResolver::new(&directory, &ctx).expect("actual resolver before refusal");
            let original = dimension.map(|dimension| {
                let result = match dimension {
                    ResourceDimension::WorkUnits => ctx.charge_work(1, "test original FEM refusal"),
                    ResourceDimension::CollectionItems => ctx.charge_collection_items(1, "test original FEM refusal"),
                    ResourceDimension::MaterializedBytes => ctx.reserve_scoped(1, "test original FEM refusal").map(|_| ()),
                    ResourceDimension::RetainedBytes => ctx.charge_retained(1, "test original FEM refusal"),
                    ResourceDimension::Entities => ctx.charge_entities(1, "test original FEM refusal"),
                    ResourceDimension::RecursionDepth => ctx.enter_nested("test original FEM refusal").map(|_| ()),
                    _ => panic!("FEM refusal dimension"),
                };
                let Err(CodecError::ResourceLimit(first)) = result else { panic!("original refusal"); };
                assert_eq!((first.dimension, first.limit, first.used, first.additional), (dimension, 0, 0, 1));
                first
            });
            for _ in 0..64 {
                for pointer in [None, Some(0)] {
                    let result = if note { resolve_note(&ctx, &resolver, 1, usize::MAX, pointer) }
                        else { resolve_type(&ctx, &resolver, 1, usize::MAX, pointer, 212, &[0, 1]) };
                    match original {
                        Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
                        None => assert_eq!(result.expect("absent optional reference"), None),
                    }
                }
            }
            drop(resolver);
            match original {
                Some(first) => assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first)),
                None => ctx.finish_session().expect("fresh session"),
            }
        }
    }
}

#[test]
fn absent_fem_type_reference_preserves_original_refusal() { absent_reference(false); }
#[test]
fn absent_fem_note_reference_preserves_original_refusal() { absent_reference(true); }
