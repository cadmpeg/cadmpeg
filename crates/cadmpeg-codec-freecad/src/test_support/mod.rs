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

pub(crate) mod topology;

pub(crate) fn non_placement_losses(
    report: &cadmpeg_ir::report::decode::DecodeReport,
) -> Vec<&cadmpeg_ir::report::loss::LossNote> {
    report
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() != "feature.primitive-placement-unresolved")
        .collect()
}

/// Assert a design-fragment decode holds design scope with the losses §1.1
/// mandates: the wrapper charges `depth.not-delivered` for a requested
/// class exactly when the decode delivers without it (#209 §1.1). A
/// fragment that also retains model records (such as attributes) delivers
/// full scope with no missing-class loss.
pub(crate) fn assert_design_fragment_scope(result: &cadmpeg_ir::codec::DecodeResult) {
    assert!(
        result.ir().scope.design,
        "design fragment holds design scope"
    );
    let losses = non_placement_losses(result.report());
    if result.ir().scope.model {
        assert!(losses.is_empty(), "{losses:?}");
    } else {
        assert_eq!(losses.len(), 1, "{losses:?}");
        assert_eq!(losses[0].code.local_code(), "depth.not-delivered");
        assert!(losses[0].message.contains("model"), "{}", losses[0].message);
    }
}

/// Assert a model-fragment decode delivers model scope with only the §1.1
/// design not-delivered loss: the fragment locates model parts but no design
/// part (#209 §1.1).
pub(crate) fn assert_model_fragment_scope(result: &cadmpeg_ir::codec::DecodeResult) {
    assert!(result.ir().scope.model, "model fragment holds model scope");
    let losses = non_placement_losses(result.report());
    if result.ir().scope.design {
        assert!(losses.is_empty(), "{losses:?}");
    } else {
        assert_eq!(losses.len(), 1, "{losses:?}");
        assert_eq!(losses[0].code.local_code(), "depth.not-delivered");
        assert!(
            losses[0].message.contains("design"),
            "{}",
            losses[0].message
        );
    }
}
