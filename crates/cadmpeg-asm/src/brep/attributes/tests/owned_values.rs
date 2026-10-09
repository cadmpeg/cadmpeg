// SPDX-License-Identifier: Apache-2.0

use crate::brep::attributes::attribute_value;
use crate::sab::Token;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::AttributeValue;
use cadmpeg_ir::scalar::FiniteReal;

fn vector_lane(token: Token, expected: &[f64]) {
    let count = u64::try_from(expected.len()).expect("fixed vector length");
    let bytes = u64::try_from(expected.len() * std::mem::size_of::<FiniteReal>()).expect("fixed lane bytes");
    for dimension in [None, Some(ResourceDimension::CollectionItems), Some(ResourceDimension::RetainedBytes)] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_entities = 0;
        policy.limits.max_recursion_depth = 0;
        policy.limits.max_collection_items = count;
        policy.limits.max_retained_bytes = bytes;
        match dimension {
            Some(ResourceDimension::CollectionItems) => policy.limits.max_collection_items -= 1,
            Some(ResourceDimension::RetainedBytes) => policy.limits.max_retained_bytes -= 1,
            None => {},
            _ => panic!("lane dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let result = attribute_value(&ctx, &token, crate::asm_format!("f3d"));
        if let Some(dimension) = dimension {
            let first = match result.err().expect("one-short lane refusal") {
                CodecError::ResourceLimit(first) => first,
                other => panic!("expected resource refusal: {other:?}"),
            };
            assert_eq!(first.dimension, dimension);
            assert_eq!(first.operation, "ASM attribute vector values");
            let additional = if dimension == ResourceDimension::CollectionItems { count } else { bytes };
            assert_eq!((first.used, first.additional, first.limit), (0, additional, additional - 1));
            for _ in 0..64 {
                assert!(matches!(attribute_value(&ctx, &token, crate::asm_format!("f3d")),
                    Err(CodecError::ResourceLimit(last)) if last == first));
            }
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
        } else {
            let value = result.expect("exact lane fits").expect("finite vector");
            let AttributeValue::Vector(ref values) = value else { panic!("vector value"); };
            assert_eq!(values.len(), expected.len());
            for (actual, expected) in values.iter().zip(expected) { assert_eq!(actual.get(), *expected); }
            // Allocate no scratch; output backing is retained beyond completion.
            ctx.finish_session().expect("fresh session");
            let AttributeValue::Vector(values) = value else { panic!("retained vector"); };
            assert_eq!(values.len(), expected.len());
        }
    }
}

#[test]
fn attribute_position_admits_exact_owned_lane() {
    vector_lane(Token::Position([1.0, 2.0, 3.0]), &[1.0, 2.0, 3.0]);
}

#[test]
fn attribute_vector3_admits_exact_owned_lane() {
    vector_lane(Token::Vector3([1.0, 2.0, 3.0]), &[1.0, 2.0, 3.0]);
}

#[test]
fn attribute_vector2_admits_exact_owned_lane() {
    vector_lane(Token::Vector2([1.0, 2.0]), &[1.0, 2.0]);
}
