// SPDX-License-Identifier: Apache-2.0

use crate::feature::rows::FeatureFieldValue;
use crate::psb;

fn field_value(payload: &[u8]) -> crate::feature::rows::FeatureFieldValue {
    crate::decode::with_test_decode_ctx(|ctx| crate::feature::rows::field_value(ctx, payload))
        .expect("field value is admitted")
}

#[test]
fn decodes_compact_feature_scalar_array_extents() {
    let mut payload = vec![psb::token::SCALAR_BODY, 0x80, 0x88, 0x03];
    payload.extend(std::iter::repeat_n(0x0f, 136 * 3));

    let FeatureFieldValue::ScalarArray {
        dimensions,
        count,
        body,
        decoded_values,
    } = field_value(&payload)
    else {
        panic!("scalar array");
    };
    assert_eq!(dimensions, 136);
    assert_eq!(count, 3);
    assert_eq!(body.len(), 408);
    assert_eq!(decoded_values, Some(vec![0.0; 408]));
}

#[test]
fn scalar_feature_cache_drops_while_its_field_output_stays_live() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    const CAP: u64 = 16 * 1024;
    // Reconstructing the leading 0x46 as 0x40 yields IEEE-754 binary64 2.0.
    let payload = [0xf9, 0x01, 0x01, 0x46, 0, 0, 0, 0, 0, 0, 0];
    for scoped in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = CAP;
        policy.limits.max_materialized_bytes = CAP;
        let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy).expect("root");
        let parts = if scoped {
            let parts = ctx.with_scoped_storage("feature scalar output parent", || {
                crate::feature::rows::field_value(&ctx, &payload)
            }).expect("scoped field");
            (parts.0, Some(parts.1))
        } else {
            (crate::feature::rows::field_value(&ctx, &payload).expect("field"), None)
        };
        let storage = parts.1;
        let value = parts.0;
        let FeatureFieldValue::ScalarArray { dimensions, count, body, decoded_values } = &value
        else { panic!("scalar field"); };
        assert_eq!((*dimensions, *count), (1, 1));
        assert_eq!(body.as_slice(), &payload[3..]);
        let values = decoded_values.as_ref().expect("complete scalar extent");
        assert_eq!(values.as_slice(), [2.0]);
        let expected_bytes = u64::try_from(body.capacity()
            + values.capacity() * std::mem::size_of::<f64>()).expect("live field backing");
        let refusal = if scoped {
            ctx.reserve_scoped_limit(CAP + 1, "after feature scalar cache")
                .expect_err("probe live output only")
        } else {
            ctx.charge_retained_limit(CAP + 1, "after feature scalar cache")
                .expect_err("probe retained output only")
        };
        assert_eq!(refusal.dimension, if scoped {
            ResourceDimension::MaterializedBytes
        } else {
            ResourceDimension::RetainedBytes
        });
        assert_eq!((refusal.used, refusal.additional), (expected_bytes, CAP + 1));
        assert!(matches!(crate::feature::rows::field_value(&ctx, &payload),
            Err(CodecError::ResourceLimit(actual)) if actual == refusal));
        assert_eq!(ctx.resource_refusal(), Some(refusal));
        drop(value);
        drop(storage);
    }
}
