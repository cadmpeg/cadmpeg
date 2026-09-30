// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

pub(in crate::eval) fn with_policy<T>(
    policy: DecodePolicy,
    run: impl FnOnce(&DecodeContext<'_>) -> T,
) -> T {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    run(&ctx)
}
