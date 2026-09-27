// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic `FCStd` archive fixtures for crate tests.

pub(crate) mod test_archive;

pub(crate) fn validate_native(ir: &cadmpeg_ir::document::CadIr) -> Vec<cadmpeg_ir::report::check::Finding> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within the input limit");
    crate::validate_native(&ctx, ir).expect("test validation is within resource limits")
}
