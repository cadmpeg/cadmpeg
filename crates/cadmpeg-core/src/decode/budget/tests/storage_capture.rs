// SPDX-License-Identifier: Apache-2.0

use crate::decode::DecodePolicy;
use crate::decode::ResourceLimit;
use crate::CodecError;
use std::cell::Cell;

#[test]
fn scoped_storage_prefuse_skips_builder_and_keeps_live_backing() {
    let arena = crate::decode::DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 3;
    let (ctx, _) =
        crate::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut storage = ctx
        .reserve_scoped(0, "existing scratch")
        .expect("existing owner");
    let text = storage
        .with_storage(|| ctx.copy_retained_text("abc", "live backing"))
        .expect("text");
    let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "original refusal") else {
        panic!("work refuses")
    };
    let calls = Cell::new(0);
    let found = storage
        .with_storage(|| {
            calls.set(calls.get() + 1);
            Ok::<(), CodecError>(())
        })
        .expect_err("existing refusal precedes builder");
    assert!(matches!(found, CodecError::ResourceLimit(limit) if limit == original));
    assert_eq!(calls.get(), 0);
    let found = storage
        .with_storage(|| -> Result<(), CodecError> { panic!("prefused callback must not execute") })
        .expect_err("original refusal precedes panic");
    assert!(matches!(found, CodecError::ResourceLimit(limit) if limit == original));
    assert_eq!(text, "abc");
    assert_eq!(ctx.budget.materialized.get(), 3);
    assert!(ctx.budget.scoped_storage.get().is_none());
    drop(text);
    drop(storage);
    assert_eq!(ctx.budget.materialized.get(), 0);
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn scoped_storage_limit_prefuse_skips_builder_and_keeps_live_backing() {
    let arena = crate::decode::DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 3;
    let (ctx, _) =
        crate::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut storage = ctx
        .reserve_scoped_limit(0, "existing scratch")
        .expect("existing owner");
    let text = storage
        .with_storage_limit(|| ctx.copy_retained_text_limit("abc", "live backing"))
        .expect("text");
    let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "original refusal") else {
        panic!("work refuses")
    };
    let calls = Cell::new(0);
    let found = storage
        .with_storage_limit(|| {
            calls.set(calls.get() + 1);
            Ok::<(), ResourceLimit>(())
        })
        .expect_err("existing refusal precedes builder");
    assert_eq!(found, original);
    assert_eq!(calls.get(), 0);
    let found = storage
        .with_storage_limit(|| -> Result<(), ResourceLimit> {
            panic!("prefused callback must not execute")
        })
        .expect_err("original refusal precedes panic");
    assert_eq!(found, original);
    assert_eq!(text, "abc");
    assert_eq!(ctx.budget.materialized.get(), 3);
    assert!(ctx.budget.scoped_storage.get().is_none());
    drop(text);
    drop(storage);
    assert_eq!(ctx.budget.materialized.get(), 0);
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn provisional_storage_prefuse_skips_builder_and_keeps_live_backing() {
    let arena = crate::decode::DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 3;
    let (ctx, _) =
        crate::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut storage = ctx
        .provisional_retained("existing candidate")
        .expect("existing owner");
    let text = storage
        .with_storage(|| ctx.copy_retained_text("abc", "live backing"))
        .expect("text");
    let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "original refusal") else {
        panic!("work refuses")
    };
    let calls = Cell::new(0);
    let found = storage
        .with_storage(|| {
            calls.set(calls.get() + 1);
            Ok::<(), CodecError>(())
        })
        .expect_err("existing refusal precedes builder");
    assert!(matches!(found, CodecError::ResourceLimit(limit) if limit == original));
    assert_eq!(calls.get(), 0);
    let found = storage
        .with_storage(|| -> Result<(), CodecError> { panic!("prefused callback must not execute") })
        .expect_err("original refusal precedes panic");
    assert!(matches!(found, CodecError::ResourceLimit(limit) if limit == original));
    assert_eq!(text, "abc");
    assert_eq!(ctx.budget.retained.get(), 3);
    assert!(ctx.budget.scoped_storage.get().is_none());
    drop(text);
    drop(storage);
    assert_eq!(ctx.budget.retained.get(), 0);
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn capture_keeps_external_text_charged_after_success_error_and_unwind() {
    use std::io::{Error, ErrorKind};
    use std::panic::{catch_unwind, AssertUnwindSafe};

    #[derive(Clone, Copy, Debug)]
    enum Outcome {
        Success,
        SemanticError,
        SwallowedRefusal,
        Unwind,
        FusedUnwind,
    }

    for retained in [false, true] {
        for outcome in [
            Outcome::Success,
            Outcome::SemanticError,
            Outcome::SwallowedRefusal,
            Outcome::Unwind,
            Outcome::FusedUnwind,
        ] {
            let arena = crate::decode::DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = 3;
            policy.limits.max_materialized_bytes = if retained { 0 } else { 3 };
            policy.limits.max_retained_bytes = if retained { 3 } else { 0 };
            let (ctx, _) = crate::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                .expect("context");
            let mut scratch = ctx.reserve_scoped(0, "scratch").expect("scope");
            let mut candidate = ctx.provisional_retained("candidate").expect("candidate");
            let mut text = String::new();
            let refusal = Cell::new(None);
            let grow = || -> Result<(), CodecError> {
                ctx.append_retained(&mut text, "abc", "capture text")?;
                if matches!(outcome, Outcome::SwallowedRefusal | Outcome::FusedUnwind) {
                    let Err(CodecError::ResourceLimit(original)) =
                        ctx.charge_work(1, "original refusal")
                    else {
                        panic!("fourth work unit must refuse")
                    };
                    refusal.set(Some(original));
                }
                match outcome {
                    Outcome::Success | Outcome::SwallowedRefusal => Ok(()),
                    Outcome::SemanticError => {
                        Err(CodecError::Io(Error::from(ErrorKind::InvalidData)))
                    }
                    Outcome::Unwind | Outcome::FusedUnwind => panic!("external owner unwind"),
                }
            };
            let result = catch_unwind(AssertUnwindSafe(|| {
                if retained {
                    candidate.with_storage(grow)
                } else {
                    scratch.with_storage(grow)
                }
            }));
            match outcome {
                Outcome::Success => assert!(matches!(result, Ok(Ok(())))),
                Outcome::SemanticError => assert!(matches!(result,
                    Ok(Err(CodecError::Io(error))) if error.kind() == ErrorKind::InvalidData)),
                Outcome::SwallowedRefusal => assert!(matches!(result,
                    Ok(Err(CodecError::ResourceLimit(found))) if Some(found) == refusal.get())),
                Outcome::Unwind | Outcome::FusedUnwind => assert!(result.is_err()),
            }
            assert_eq!(text, "abc", "{retained}, {outcome:?}");
            assert_eq!(ctx.budget.work.get(), 3);
            assert_eq!(ctx.budget.materialized.get(), if retained { 0 } else { 3 });
            assert_eq!(ctx.budget.retained.get(), if retained { 3 } else { 0 });
            assert_eq!(scratch.bytes, if retained { 0 } else { 3 });
            assert_eq!(candidate.bytes, if retained { 3 } else { 0 });
            assert!(ctx.budget.scoped_storage.get().is_none());
            assert_eq!(ctx.resource_refusal(), refusal.get());
            drop(text);
            drop(scratch);
            drop(candidate);
            assert_eq!(ctx.budget.materialized.get(), 0);
            assert_eq!(ctx.budget.retained.get(), 0);
            assert_eq!(ctx.resource_refusal(), refusal.get());
        }
    }
}
