// SPDX-License-Identifier: Apache-2.0
use super::super::DecodeContext;
use crate::decode::{DecodeArena, DecodePolicy, LimitScope, ResourceDimension, ResourceFailure};
use crate::CodecError;

#[test]
fn temporary_vector_growth_does_not_consume_retained_allowance() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::service())
        .expect("actual decode session");
    let before = ctx.retained_bytes_available();
    let mut values = Vec::<u8>::new();
    ctx.reserve_working(&mut values, 4, "temporary test vector")
        .expect("fallible working allocation");
    values.extend_from_slice(b"work");
    assert_eq!(values, b"work");
    assert_eq!(ctx.retained_bytes_available(), before);
    ctx.finish_session().expect("no retained charge or refusal");
}

fn assert_working_overflow_is_fused<T>() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::service())
        .expect("actual decode session");
    let mut values = Vec::<T>::new();
    let error = ctx
        .reserve_working(&mut values, usize::MAX, "working capacity overflow")
        .expect_err("deterministic capacity overflow before allocation");
    let CodecError::ResourceLimit(expected) = error else {
        panic!("allocation must refuse the session")
    };
    assert_eq!(expected.scope, LimitScope::Session);
    assert_eq!(expected.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(expected.reason, ResourceFailure::AllocationFailed);
    assert_eq!(expected.operation, "working capacity overflow");
    assert_eq!(ctx.resource_refusal(), Some(expected));
    assert_eq!(ctx.retained_bytes_available(), 0);
    assert!(values.is_empty());
    let later = ctx
        .reserve_working(&mut values, 0, "later empty reservation")
        .expect_err("even an empty reservation preserves the fused refusal");
    assert!(matches!(later, CodecError::ResourceLimit(actual) if actual == expected));
    let final_error = ctx
        .finish_session()
        .expect_err("swallowed allocation remains fatal");
    assert!(matches!(final_error, CodecError::ResourceLimit(actual) if actual == expected));
}

#[test]
fn working_capacity_overflow_latches_the_original_session_refusal() {
    assert_working_overflow_is_fused::<u8>();
    assert_working_overflow_is_fused::<u16>();
}

#[test]
fn working_reservation_preserves_an_earlier_entity_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("actual decode session");
    let error = ctx
        .charge_entities(1, "earlier entity refusal")
        .expect_err("entity limit");
    let CodecError::ResourceLimit(expected) = error else {
        panic!("entity limit must refuse the session")
    };
    let mut values = Vec::<u16>::new();
    let error = ctx
        .reserve_working(&mut values, usize::MAX, "later working overflow")
        .expect_err("original refusal wins before capacity validation");
    assert!(matches!(error, CodecError::ResourceLimit(actual) if actual == expected));
    assert!(values.is_empty());
    let final_error = ctx
        .finish_session()
        .expect_err("original refusal remains fatal");
    assert!(matches!(final_error, CodecError::ResourceLimit(actual) if actual == expected));
}
