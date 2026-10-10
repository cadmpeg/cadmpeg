// SPDX-License-Identifier: Apache-2.0

use super::reference_position;
use crate::records::identity::{Located, ReferenceRun};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::cell::Cell;

const ORIGINAL: &str = "original F3D reference predicate refusal";

#[test]
fn reference_position_keeps_original_refusal_before_each_callback_outcome() {
    let runs = [
        ReferenceRun::unlocated(vec![42, 43]),
        ReferenceRun::located(vec![
            Located {
                value: 42,
                offset: 21_u64,
            },
            Located {
                value: 43,
                offset: 32,
            },
        ]),
    ];
    for run in &runs {
        let native = run.clone();
        for outcome in 0..3 {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = u64::MAX;
            crate::test_support::with_decode_policy(&policy, |ctx| {
                let calls = Cell::new(0);
                let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, ORIGINAL, Some(1));
                let result = reference_position(
                    ctx,
                    run,
                    |value| {
                        calls.set(calls.get() + 1);
                        assert_eq!(*value, 42);
                        assert!(matches!(
                            ctx.charge_work(1, ORIGINAL),
                            Err(CodecError::ResourceLimit(_))
                        ));
                        match outcome {
                            0 => Ok(false),
                            1 => Ok(true),
                            _ => Err(CodecError::Malformed("ordinary predicate error".into())),
                        }
                    },
                    "visit F3D reference callback control",
                );
                let Err(CodecError::ResourceLimit(original)) = result else {
                    panic!("original callback refusal");
                };
                assert_eq!(original.operation, ORIGINAL);
                assert_eq!(original.additional, 1);
                assert_eq!(calls.get(), 1);
                assert_eq!(ctx.resource_refusal(), Some(original.clone()));
                assert!(
                    matches!(ctx.charge_work(0, "later reference boundary"), Err(CodecError::ResourceLimit(later)) if later == original)
                );
            });
        }
        assert_eq!(*run, native);
    }
}

#[test]
fn reference_position_preserves_match_order_unvisited_suffix_and_ordinary_error() {
    let run: ReferenceRun<u32> = ReferenceRun::unlocated(vec![42, 43, 44]);
    crate::test_support::with_decode_context(|ctx| {
        let mut visited = Vec::new();
        assert_eq!(
            reference_position(
                ctx,
                &run,
                |value| {
                    visited.push(*value);
                    Ok(*value == 43)
                },
                "test reference match order"
            )
            .unwrap(),
            Some(1)
        );
        assert_eq!(visited, [42, 43]);
        assert!(
            matches!(reference_position(ctx, &run, |_| Err(CodecError::Malformed("ordinary predicate error".into())), "test ordinary reference error"), Err(CodecError::Malformed(message)) if message == "ordinary predicate error")
        );
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn reference_position_empty_run_preserves_preexisting_original_fuse() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, ORIGINAL) else {
            panic!("zero-work refusal");
        };
        let run: ReferenceRun<u32> = ReferenceRun::unlocated(Vec::new());
        assert!(
            matches!(reference_position(ctx, &run, |_| panic!("empty run has no predicate"), "test empty reference fuse"), Err(CodecError::ResourceLimit(later)) if later == original)
        );
    });
}
