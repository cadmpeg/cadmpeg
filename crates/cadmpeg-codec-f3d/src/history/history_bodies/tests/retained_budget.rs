// SPDX-License-Identifier: Apache-2.0

use super::*;

/// The records that the decode of a history state transfers and drops pass
/// the retained-byte budget: the decode gives up the body, and the document
/// keeps decoding.
#[test]
fn a_history_decode_past_the_retained_budget_gives_up_only_its_body() {
    use cadmpeg_core::decode::{test_support::at_charge, ResourceDimension};

    let bytes = carrier_bytes(RefWidth::Eight, true);
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = u64::MAX;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let (bodies, losses) = at_charge(
        ResourceDimension::RetainedBytes,
        "serialize native record",
        0,
        || {
            fill_roles(
                &ctx,
                &bytes,
                RefWidth::Eight,
                (1, MAX_HISTORY_BODY_WORK),
                (true, false),
                1,
            )
        },
    )
    .expect("the document keeps decoding");
    assert!(bodies.is_empty());
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:?}")
    };
    assert_eq!(
        loss.code,
        F3dLossCode::FeatureHistoryBodyBounded.note("bounded").code
    );
    assert!(loss.message.contains("RetainedBytes"), "{}", loss.message);
    assert!(ctx.resource_refusal().is_none());
}
