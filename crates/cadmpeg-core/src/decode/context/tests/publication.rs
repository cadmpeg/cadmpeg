// SPDX-License-Identifier: Apache-2.0

use crate::decode::{
    u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ExpandSpec, ResourceDimension,
};
use crate::CodecError;

#[test]
fn exhausted_addresses_refuse_before_installing_owned_arena_output() {
    for expand in [false, true] {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(b"AB", &arena, &DecodePolicy::service()).expect("root");
        ctx.derived_spaces.set(usize::MAX);
        let result = if expand {
            let mut writer = ctx
                .begin_expand(ExpandSpec::Exact(2))
                .expect("private writer");
            writer.write(b"AB").expect("private output");
            writer.finalize()
        } else {
            ctx.concat_views(&[root])
        };
        let Err(CodecError::ResourceLimit(original)) = result else {
            panic!("address space ceiling")
        };
        assert_eq!(
            original.dimension,
            ResourceDimension::Codec("decode address spaces")
        );
        assert_eq!(
            (original.limit, original.used, original.additional),
            (u64_from_index(usize::MAX), u64_from_index(usize::MAX), 1)
        );
        assert_eq!(ctx.derived_spaces.get(), usize::MAX);
        assert_eq!(arena.allocation_state(), (0, 0));
        assert_eq!(ctx.budget.retained_used(), 0);
        assert_eq!(ctx.budget.materialized_used(), 0);
        assert_eq!(root.window(), b"AB");
        assert!(
            matches!(ctx.charge_work(0, "later"), Err(CodecError::ResourceLimit(found)) if found == original)
        );
    }
}

#[test]
fn final_address_is_published_only_with_its_live_arena_payload() {
    for expand in [false, true] {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(b"AB", &arena, &DecodePolicy::service()).expect("root");
        ctx.derived_spaces.set(usize::MAX - 1);
        let accepted = ctx.concat_views(&[root]).expect("last space");
        assert_eq!(accepted.space().index(), usize::MAX);
        let retained = ctx.budget.retained_used();
        let state = arena.allocation_state();
        let result = if expand {
            let mut writer = ctx
                .begin_expand(ExpandSpec::Exact(2))
                .expect("private writer");
            writer.write(b"AB").expect("private output");
            writer.finalize()
        } else {
            ctx.concat_views(&[root])
        };
        assert!(matches!(result, Err(CodecError::ResourceLimit(_))));
        assert_eq!(accepted.window(), b"AB");
        assert_eq!(arena.allocation_state(), state);
        assert_eq!(ctx.budget.retained_used(), retained);
        assert_eq!(ctx.budget.materialized_used(), 0);
        assert_eq!(ctx.derived_spaces.get(), usize::MAX);
    }
}
