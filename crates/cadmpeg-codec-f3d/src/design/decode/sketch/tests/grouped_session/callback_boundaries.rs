// SPDX-License-Identifier: Apache-2.0

use super::{native_headers, Grouped};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::cell::Cell;

const ORIGINAL: &str = "original F3D grouped key callback refusal";
const ORDINARY: &str = "ordinary F3D grouped key callback error";
const KEY: &str = "f3d:callback";

#[derive(Clone, Copy)]
enum CallbackResult {
    Missing,
    Key,
    Error,
}

fn check_callback(result: CallbackResult) {
    let records = native_headers();
    let input = &records[..2];
    let native = serde_json::to_value(input).unwrap();
    for refuse in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 4096;
        policy.limits.max_retained_bytes = 0;
        if refuse {
            // The actual sealed slice traversal admits its two visits before
            // the first callback. One additional real work unit must refuse.
            policy.limits.max_work_units = u64::try_from(input.len()).unwrap();
            policy.limits.max_collection_items = 0;
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let calls = Cell::new(0);
        let mut storage = ctx.reserve_scoped(0, "test grouped callback backing").unwrap();
        let build = || {
            Grouped::build(&ctx, input, |_| {
                calls.set(calls.get() + 1);
                let work = ctx.charge_work(1, ORIGINAL);
                if refuse {
                    assert!(matches!(work, Err(CodecError::ResourceLimit(_))));
                } else {
                    work.unwrap();
                }
                match result {
                    CallbackResult::Missing => Ok(None),
                    CallbackResult::Key => Ok(Some(KEY)),
                    CallbackResult::Error => Err(CodecError::malformed(ORDINARY)),
                }
            }, "test grouped callback construction")
        };
        // Check the owner's direct result. A capture wrapper's own fuse check
        // would hide an ordinary error substituted by the grouped builder.
        let grouped = if refuse { build() } else { storage.with_storage(build) };
        if refuse {
            let Err(CodecError::ResourceLimit(original)) = grouped else {
                panic!("the original callback refusal must dominate its result");
            };
            assert_eq!(calls.get(), 1);
            assert_eq!(original.dimension, ResourceDimension::WorkUnits);
            assert_eq!(original.limit, u64::try_from(input.len()).unwrap());
            assert_eq!(original.used, u64::try_from(input.len()).unwrap());
            assert_eq!(original.additional, 1);
            assert_eq!(original.operation, ORIGINAL);
            assert_eq!(ctx.resource_refusal(), Some(original.clone()));
            assert!(matches!(ctx.charge_work(0, "later grouped callback operation"),
                Err(CodecError::ResourceLimit(later)) if later == original));
            drop(storage);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(later)) if later == original));
        } else {
            match result {
                CallbackResult::Missing => {
                    let grouped = grouped.unwrap();
                    assert!(grouped.run(KEY).unwrap().is_empty());
                    assert_eq!(calls.get(), input.len());
                    drop(grouped);
                }
                CallbackResult::Key => {
                    let grouped = grouped.unwrap();
                    let run = grouped.run(KEY).unwrap();
                    assert_eq!(run.len(), input.len());
                    for (entry, source) in run.iter().zip(input) {
                        assert_eq!(entry.0, KEY);
                        assert!(std::ptr::eq(entry.1, source));
                    }
                    assert_eq!(calls.get(), input.len());
                    drop(grouped);
                }
                CallbackResult::Error => {
                    assert!(matches!(grouped, Err(CodecError::Malformed(message)) if message == ORDINARY));
                    assert_eq!(calls.get(), 1);
                }
            }
            assert!(ctx.resource_refusal().is_none());
            drop(storage);
            drop(ctx.reserve_scoped(4096, "after grouped callback backing destruction").unwrap());
            ctx.finish_session().unwrap();
        }
        assert_eq!(serde_json::to_value(input).unwrap(), native);
    }
}

#[test]
fn grouped_callback_original_refusal_dominates_missing_key_before_next_callback() {
    check_callback(CallbackResult::Missing);
}

#[test]
fn grouped_callback_original_refusal_dominates_key_before_slot_admission() {
    check_callback(CallbackResult::Key);
}

#[test]
fn grouped_callback_original_refusal_dominates_ordinary_key_error() {
    check_callback(CallbackResult::Error);
}
