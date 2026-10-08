// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_ir::report::loss::LossNote;

#[test]
fn framing_loss_slots_are_live_until_consumed_and_then_released() {
    const MATERIALIZED_LIMIT: u64 = 8192;
    let mut recoveries = crate::card::FramingRecoveries::default();
    crate::test_support::with_service_context(&[], |ctx| {
        recoveries.record(ctx,
            (crate::card::Section::Parameter, crate::card::FramingDefect::ParameterOwner),
            2, 160, format_args!("D1"), format_args!("D3"),
        ).unwrap();
    });
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "iges framing recovery loss slots",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            recoveries.notes(&ctx).map(|_| ())
        },
    );
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = MATERIALIZED_LIMIT;
    for refuse_while_live in [true, false] {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let (notes, storage) = recoveries.notes(&ctx).unwrap();
        assert_eq!(notes.len(), 1);
        assert!(notes[0].message.contains("which declared D1, and the decoder used D3"));
        assert_eq!(notes[0].provenance.as_ref().unwrap().tag.as_deref(),
            Some("parameter-data:framing"));
        if refuse_while_live {
            let error = ctx.reserve_scoped(MATERIALIZED_LIMIT, "live framing loss slots").unwrap_err();
            let cadmpeg_core::CodecError::ResourceLimit(expected) = error else {
                panic!("expected framing loss storage refusal");
            };
            assert_eq!(expected.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(expected.operation, "live framing loss slots");
            assert_eq!(expected.used, 4 * u64::try_from(std::mem::size_of::<LossNote>()).unwrap());
            drop(notes);
            drop(storage);
            assert!(matches!(ctx.finish_session().unwrap_err(),
                cadmpeg_core::CodecError::ResourceLimit(actual) if actual == expected));
        } else {
            drop(notes);
            drop(storage);
            ctx.reserve_scoped(MATERIALIZED_LIMIT, "released framing loss slots").unwrap();
            ctx.finish_session().unwrap();
        }
    }
}
