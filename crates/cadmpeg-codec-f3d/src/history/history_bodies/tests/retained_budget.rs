// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn a_history_body_does_not_serialize_discarded_native_records() {
    use cadmpeg_core::decode::{test_support::charged_at, ResourceDimension};

    let bytes = carrier_bytes(RefWidth::Eight, true);
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let (decoded, serialized) = charged_at(
        ResourceDimension::RetainedBytes,
        "serialize native record",
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
    );
    let (bodies, losses) = decoded.expect("history decode");
    assert_eq!(bodies.len(), 1, "{losses:?}");
    assert_eq!(serialized, 0, "the history evidence uses neutral geometry");
    assert!(ctx.resource_refusal().is_none());
}
