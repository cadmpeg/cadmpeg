// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use cadmpeg_ir::pmi::PmiTarget;

#[test]
fn target_index_copies_only_new_targets_and_preserves_append_order() {
    crate::test_support::with_service_context(b"", |_, ctx| {
        let mut indices = BTreeMap::new();
        let mut storage = ctx.reserve_scoped(0, "target fixture").expect("scope");
        let mut targets = vec![PmiTarget::ShapeAspect { source_id: cadmpeg_core::nonblank_literal!("#1") }];
        let seen = super::super::target_index(&mut indices, &mut storage, 0, &targets, ctx).expect("index");
        super::super::push_target((seen, &mut storage), &mut targets, (8, "#1"), || panic!("duplicate must not copy a target"), ctx, "target fixture output").expect("duplicate");
        let seen = super::super::target_index(&mut indices, &mut storage, 0, &targets, ctx).expect("reused index");
        super::super::push_target((seen, &mut storage), &mut targets, (8, "#2"), || Ok(PmiTarget::ShapeAspect { source_id: cadmpeg_core::nonblank_literal!("#2").try_clone_for_decode(ctx, "target fixture identity")? }), ctx, "target fixture output").expect("new target");
        assert_eq!(targets, [PmiTarget::ShapeAspect { source_id: cadmpeg_core::nonblank_literal!("#1") }, PmiTarget::ShapeAspect { source_id: cadmpeg_core::nonblank_literal!("#2") }]);
        let seen = super::super::target_index(&mut indices, &mut storage, 0, &targets, ctx).expect("reused index");
        assert_eq!(seen[8], BTreeSet::from([String::from("#1"), String::from("#2")]));
    });
}
