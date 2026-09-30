// SPDX-License-Identifier: Apache-2.0

fn extrude_32_join_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let (reference, branch) = super::source_and_sketch::extrude_32_fixture();
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        crate::native::features::construction_records::feature_extrude_32_constructions(
            ctx,
            std::slice::from_ref(&reference),
            std::slice::from_ref(&branch),
        )
    };
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| decode(ctx))
            .expect("admitted extrude 32 construction")
            .len(),
        1
    );

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| decode(ctx).expect_err("extrude 32 construction resource limit"),
    )
}

#[test]
fn extrude_32_join_refuses_collection_limit() {
    let error = extrude_32_join_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn extrude_32_join_refuses_retained_limit() {
    let error = extrude_32_join_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn extrude_32_join_refuses_scoped_limit() {
    let error = extrude_32_join_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn extrude_32_join_refuses_work_limit() {
    let error = extrude_32_join_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}
