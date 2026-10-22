// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic `FCStd` archive fixtures for crate tests.

pub(crate) mod mutation;
pub(crate) mod source_fidelity;
pub(crate) mod test_archive;

pub(crate) fn validate_native(
    ir: &cadmpeg_ir::document::CadIr,
) -> Vec<cadmpeg_ir::report::check::Finding> {
    let bytes = serde_json::to_vec(ir).expect("test document serializes");
    with_service_context(&bytes, |ctx| {
        crate::validate_native(ctx, ir).expect("validate the native records")
    })
}

pub(crate) fn with_service_context<T>(
    input: &[u8],
    use_context: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(input, &arena, &policy)
        .expect("test input is within service limits");
    use_context(&ctx)
}

/// Builds a checked entry with its content digest for synthetic fixtures.
pub(crate) fn entry_fixture(
    id: String,
    name: String,
    role: cadmpeg_core::container::ContainerRole,
    references: Vec<String>,
    data: Vec<u8>,
) -> crate::test_support::source_fidelity::EntryFixture {
    crate::test_support::source_fidelity::EntryFixture::try_new(id, name, role, references, data)
        .expect("valid entry fixture")
}
